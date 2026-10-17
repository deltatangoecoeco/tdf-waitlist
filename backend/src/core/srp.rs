//! Who has paid SRP, worked out from the SRP corp's wallet journal and contracts.
//!
//! A character is covered for `coverage_hours` after sending `min_amount` or more to the SRP corp
//! (a player donation), or after accepting a ship loan contract the corp issued for that much.
//! Only the character who paid is covered, and payments don't stack: the latest one counts.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::{Deserialize, Serialize};

use crate::config::SrpConfig;
use crate::core::auth::AuthenticatedAccount;
use crate::core::esi::{ESIScope, SEAT_LOGIN_SCOPES};

/// What the wallet character needs
pub const WALLET_SCOPES: &[ESIScope] = &[
    ESIScope::Wallet_ReadCorporationWallets_v1,
    ESIScope::Contracts_ReadCorporationContracts_v1,
];

/// The SeAT login that connects the wallet character: the scopes every SeAT login needs, plus the
/// wallet's
pub fn connect_scopes() -> Vec<ESIScope> {
    SEAT_LOGIN_SCOPES
        .iter()
        .chain(WALLET_SCOPES)
        .copied()
        .collect()
}

/// Results older than this are not shown: better nothing than a stale "not paid"
const STALE_AFTER_SECS: i64 = 2 * 3600;

/// How long the admin page shows why a connect failed
const CONNECT_ERROR_SHOWN_SECS: i64 = 15 * 60;

const CONNECT_STATE_PREFIX: &[u8] = b"srpconnect:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Donation,
    Contract,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Payment {
    pub paid_at: i64,
    pub amount: f64,
    pub via: Via,
}

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub min_amount: f64,
    pub coverage_secs: i64,
}

impl Rule {
    pub fn from_config(config: &SrpConfig) -> Rule {
        Rule {
            min_amount: config.min_amount as f64,
            coverage_secs: config.coverage_hours * 3600,
        }
    }

    pub fn expires_at(&self, paid_at: i64) -> i64 {
        paid_at + self.coverage_secs
    }

    pub fn covers(&self, paid_at: i64, now: i64) -> bool {
        self.expires_at(paid_at) > now
    }
}

/// The fields we use from ESI's corporation wallet journal
#[derive(Debug, Deserialize)]
pub struct JournalEntry {
    pub date: String,
    pub ref_type: String,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub first_party_id: Option<i64>,
}

/// The fields we use from ESI's corporation contracts
#[derive(Debug, Deserialize)]
pub struct Contract {
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub issuer_corporation_id: i64,
    pub for_corporation: bool,
    pub acceptor_id: i64,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub date_accepted: Option<String>,
}

fn timestamp(date: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|d| d.timestamp())
}

fn insert_latest(paid: &mut HashMap<i64, Payment>, character_id: i64, payment: Payment) {
    match paid.get(&character_id) {
        Some(existing) if existing.paid_at >= payment.paid_at => {}
        _ => {
            paid.insert(character_id, payment);
        }
    }
}

/// Donations to the corp. `first_party_id` is the sender for player_donation entries.
pub fn paid_from_journal(entries: &[JournalEntry], now: i64, rule: &Rule) -> HashMap<i64, Payment> {
    let mut paid = HashMap::new();
    for entry in entries {
        if entry.ref_type != "player_donation" {
            continue;
        }
        let (amount, character_id, paid_at) =
            match (entry.amount, entry.first_party_id, timestamp(&entry.date)) {
                (Some(a), Some(c), Some(t)) => (a, c, t),
                _ => continue,
            };
        if amount >= rule.min_amount && rule.covers(paid_at, now) {
            insert_latest(
                &mut paid,
                character_id,
                Payment {
                    paid_at,
                    amount,
                    via: Via::Donation,
                },
            );
        }
    }
    paid
}

/// Ship loan contracts: item exchanges issued on behalf of the SRP corp, covering the acceptor.
/// Read from the contract list rather than the journal's contract_price entries, whose party ids
/// ESI documents as inconsistent between ref types.
pub fn paid_from_contracts(
    contracts: &[Contract],
    corporation_id: i64,
    now: i64,
    rule: &Rule,
) -> HashMap<i64, Payment> {
    let mut paid = HashMap::new();
    for contract in contracts {
        if contract.kind != "item_exchange"
            || contract.status != "finished"
            || contract.issuer_corporation_id != corporation_id
            || !contract.for_corporation
            || contract.acceptor_id == 0
        {
            continue;
        }
        let (amount, paid_at) = match (
            contract.price,
            contract.date_accepted.as_deref().and_then(timestamp),
        ) {
            (Some(a), Some(t)) => (a, t),
            _ => continue,
        };
        if amount >= rule.min_amount && rule.covers(paid_at, now) {
            insert_latest(
                &mut paid,
                contract.acceptor_id,
                Payment {
                    paid_at,
                    amount,
                    via: Via::Contract,
                },
            );
        }
    }
    paid
}

/// The payment covering `character_id` now, if any
pub fn current_payment(
    paid: &HashMap<i64, Payment>,
    character_id: i64,
    now: i64,
    rule: &Rule,
) -> Option<Payment> {
    paid.get(&character_id)
        .filter(|p| rule.covers(p.paid_at, now))
        .copied()
}

/// Latest payment per character wins
pub fn merge(mut a: HashMap<i64, Payment>, b: HashMap<i64, Payment>) -> HashMap<i64, Payment> {
    for (character_id, payment) in b {
        insert_latest(&mut a, character_id, payment);
    }
    a
}

/// Journal pages are newest first: once a page ends before the coverage window, later pages
/// can't matter
pub fn journal_page_reaches_past_window(entries: &[JournalEntry], now: i64, rule: &Rule) -> bool {
    match entries.last().and_then(|e| timestamp(&e.date)) {
        Some(t) => !rule.covers(t, now),
        None => true,
    }
}

/// One successful read of a corp's wallet
pub struct WalletRead {
    pub paid: HashMap<i64, Payment>,
    /// When ESI produced the data, if it said: the older of the journal's and the contracts'.
    /// A payment made after this can't show yet.
    pub data_as_of: Option<i64>,
}

/// The older of two times either of which may be unknown
pub fn older(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// One SRP corp's check results
#[derive(Debug, Default)]
pub struct CorpState {
    pub paid: HashMap<i64, Payment>,
    /// When the wallet was last read successfully
    pub checked_at: Option<i64>,
    /// When ESI produced that data; see `WalletRead`
    pub data_as_of: Option<i64>,
    /// Why the last check failed, if it did, in words an admin can act on
    pub last_error: Option<String>,
    /// Bumped whenever this corp's wallet is connected, reconnected or disconnected, so a check
    /// that started before then throws its result away instead of publishing the old one's
    generation: u64,
}

/// What `CorpState::apply` did with a check's result, for the log
#[derive(Debug, PartialEq)]
pub enum Applied {
    Updated,
    /// Updated after the previous check had failed
    Recovered,
    Failed,
    /// The wallet changed while the check ran
    Discarded,
}

impl CorpState {
    pub fn is_fresh(&self, now: i64) -> bool {
        match self.checked_at {
            Some(t) => now - t < STALE_AFTER_SECS,
            None => false,
        }
    }

    /// Records a check's result. `generation` is the one the check started with: if the wallet
    /// changed since, the result is about the old one. A failure keeps the last good result,
    /// which stops being shown once it goes stale.
    pub fn apply(
        &mut self,
        generation: u64,
        result: Result<WalletRead, String>,
        now: i64,
    ) -> Applied {
        if generation != self.generation {
            return Applied::Discarded;
        }
        match result {
            Ok(read) => {
                let recovered = self.last_error.is_some();
                self.paid = read.paid;
                self.data_as_of = read.data_as_of;
                self.checked_at = Some(now);
                self.last_error = None;
                match recovered {
                    true => Applied::Recovered,
                    false => Applied::Updated,
                }
            }
            Err(message) => {
                self.last_error = Some(message);
                Applied::Failed
            }
        }
    }

    /// Payments still covering someone, newest first
    pub fn recent(&self, now: i64, rule: &Rule) -> Vec<(i64, Payment)> {
        let mut recent: Vec<(i64, Payment)> = self
            .paid
            .iter()
            .filter(|(_, p)| rule.covers(p.paid_at, now))
            .map(|(&id, &p)| (id, p))
            .collect();
        recent.sort_by_key(|(_, p)| std::cmp::Reverse(p.paid_at));
        recent
    }
}

#[derive(Debug, Default)]
pub struct SrpState {
    /// By SRP corp id
    corps: HashMap<i64, CorpState>,
    /// When and why the last connect failed. Kept here rather than in the redirect URL, so a
    /// link can't put words on the admin page.
    connect_error: Option<ConnectError>,
}

#[derive(Debug)]
struct ConnectError {
    at: i64,
    message: String,
}

impl SrpState {
    pub fn corp(&self, corporation_id: i64) -> Option<&CorpState> {
        self.corps.get(&corporation_id)
    }

    pub fn corp_mut(&mut self, corporation_id: i64) -> &mut CorpState {
        self.corps.entry(corporation_id).or_default()
    }

    /// To pass to `CorpState::apply` when the check finishes
    pub fn generation(&self, corporation_id: i64) -> u64 {
        self.corp(corporation_id).map_or(0, |c| c.generation)
    }

    /// The corp's wallet was connected or reconnected. Its last good results stay until the new
    /// wallet's first check replaces them, so a reconnect that turns out wrong doesn't blank
    /// pilots' status.
    pub fn wallet_replaced(&mut self, corporation_id: i64) {
        let corp = self.corp_mut(corporation_id);
        corp.generation += 1;
        corp.last_error = None;
        self.connect_error = None;
    }

    /// The corp's wallet was disconnected
    pub fn wallet_removed(&mut self, corporation_id: i64) {
        let corp = self.corp_mut(corporation_id);
        let generation = corp.generation + 1;
        *corp = CorpState {
            generation,
            ..CorpState::default()
        };
    }

    pub fn connect_failed(&mut self, now: i64, message: String) {
        self.connect_error = Some(ConnectError { at: now, message });
    }

    pub fn recent_connect_error(&self, now: i64) -> Option<String> {
        match &self.connect_error {
            Some(e) if now - e.at < CONNECT_ERROR_SHOWN_SECS => Some(e.message.clone()),
            _ => None,
        }
    }
}

/// The SRP check's state, shared between the updater and the routes, and a way to ask the updater
/// for a check now
#[derive(Debug, Default)]
pub struct Shared {
    state: RwLock<SrpState>,
    wake: tokio::sync::Notify,
}

impl Shared {
    // Every write is a few assignments, so a panic elsewhere while the lock was held can't have
    // left the state half-written: carry on rather than fail every later request
    pub fn read(&self) -> RwLockReadGuard<'_, SrpState> {
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, SrpState> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wakes the updater, or makes its next wait return at once if it's mid-check
    pub fn check_now(&self) {
        self.wake.notify_one();
    }

    /// Until `check_now` or `max` passes
    pub async fn wait(&self, max: std::time::Duration) {
        let _ = tokio::time::timeout(max, self.wake.notified()).await;
    }
}

pub type SharedSrpState = Arc<Shared>;

pub fn new_shared_state() -> SharedSrpState {
    Arc::new(Shared::default())
}

/// For an admin: the detail goes to the log, not the page
pub fn database_error(error: sqlx::Error) -> String {
    error!("SRP database error: {}", error);
    "Database error; the server log has the details.".to_string()
}

/// Names come from config, so differences in case and surrounding spaces don't matter.
/// `to_lowercase`, like the database's `lower()`, so non-English names compare the same way.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A `[srp] managers` entry is a character id or a name
fn names_character(entry: &str, id: i64, name: &str) -> bool {
    match entry.trim().parse::<i64>() {
        Ok(entry_id) => entry_id == id,
        Err(_) => same_name(entry, name),
    }
}

/// Whether the account may run the SRP wallet page: council and admin always, anyone else when
/// one of their characters is in `[srp] managers`
pub async fn is_manager(
    db: &crate::DB,
    config: &SrpConfig,
    account: &AuthenticatedAccount,
) -> Result<bool, sqlx::Error> {
    let has_key = account.access.contains("srp-manage");
    // Nothing to look up: the answer can't depend on the account's characters
    if has_key || config.managers.is_empty() {
        return Ok(has_key);
    }
    let characters: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, name FROM \"character\" WHERE id = $1
        UNION ALL
        SELECT c.id, c.name FROM alt_character a JOIN \"character\" c ON c.id = a.alt_id
            WHERE a.account_id = $1",
    )
    .bind(account.id)
    .fetch_all(db)
    .await?;
    let characters: Vec<(i64, &str)> = characters.iter().map(|(id, n)| (*id, n.as_str())).collect();
    Ok(named_manager(&config.managers, &characters))
}

/// Whether `[srp] managers` names any of `characters` (the account's main and alts, as id and
/// name). Council and admin are managers whatever this says.
pub fn named_manager(managers: &[String], characters: &[(i64, &str)]) -> bool {
    characters
        .iter()
        .any(|&(id, name)| managers.iter().any(|m| names_character(m, id, name)))
}

/// Manager names in config that match no character the waitlist knows: a typo, a rename, or
/// someone who hasn't logged in yet
pub async fn unknown_managers(
    db: &crate::DB,
    config: &SrpConfig,
) -> Result<Vec<String>, sqlx::Error> {
    if config.managers.is_empty() {
        return Ok(Vec::new());
    }
    let (ids, names): (Vec<_>, Vec<_>) = config
        .managers
        .iter()
        .partition(|m| m.trim().parse::<i64>().is_ok());
    let ids: Vec<i64> = ids.iter().filter_map(|m| m.trim().parse().ok()).collect();
    let names: Vec<String> = names.iter().map(|m| m.trim().to_lowercase()).collect();
    let known: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, name FROM \"character\" WHERE id = ANY($1) OR lower(name) = ANY($2)",
    )
    .bind(&ids)
    .bind(&names)
    .fetch_all(db)
    .await?;
    Ok(config
        .managers
        .iter()
        .filter(|m| !known.iter().any(|(id, name)| names_character(m, *id, name)))
        .cloned()
        .collect())
}

/// OAuth state marking a login as "connect this character as the SRP wallet"
pub fn connect_state(character_id: i64) -> Vec<u8> {
    [CONNECT_STATE_PREFIX, character_id.to_string().as_bytes()].concat()
}

pub fn parse_connect_state(state: &[u8]) -> Option<i64> {
    let rest = state.strip_prefix(CONNECT_STATE_PREFIX)?;
    std::str::from_utf8(rest).ok()?.parse().ok()
}

/// The SRP wallets, one per SRP corp (table srp_connection). Runtime-checked queries, like
/// routes/statistics.rs, so sqlx-data.json needs no regeneration.
pub mod connection {
    // Not Debug: it holds the refresh token, which must never reach a log
    #[derive(sqlx::FromRow)]
    pub struct Connection {
        pub character_id: i64,
        pub corporation_id: i64,
        pub corporation_name: String,
        pub refresh_token: String,
        pub connected_by: i64,
        pub connected_at: i64,
    }

    /// By corp name
    pub async fn all(db: &crate::DB) -> Result<Vec<Connection>, sqlx::Error> {
        sqlx::query_as::<_, Connection>(
            "SELECT character_id, corporation_id, corporation_name, refresh_token, connected_by,
                connected_at FROM srp_connection ORDER BY corporation_name",
        )
        .fetch_all(db)
        .await
    }

    pub async fn corporation_ids(db: &crate::DB) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar("SELECT corporation_id FROM srp_connection ORDER BY corporation_name")
            .fetch_all(db)
            .await
    }

    pub async fn get(
        db: &crate::DB,
        corporation_id: i64,
    ) -> Result<Option<Connection>, sqlx::Error> {
        sqlx::query_as::<_, Connection>(
            "SELECT character_id, corporation_id, corporation_name, refresh_token, connected_by,
                connected_at FROM srp_connection WHERE corporation_id = $1",
        )
        .bind(corporation_id)
        .fetch_optional(db)
        .await
    }

    pub async fn exists(db: &crate::DB, corporation_id: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM srp_connection WHERE corporation_id = $1)")
            .bind(corporation_id)
            .fetch_one(db)
            .await
    }

    /// Replaces the corp's previous wallet, if any
    pub async fn save(db: &crate::DB, connection: &Connection) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO srp_connection (corporation_id, character_id, corporation_name,
                refresh_token, connected_by, connected_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (corporation_id) DO UPDATE SET character_id = $2, corporation_name = $3,
                refresh_token = $4, connected_by = $5, connected_at = $6",
        )
        .bind(connection.corporation_id)
        .bind(connection.character_id)
        .bind(&connection.corporation_name)
        .bind(&connection.refresh_token)
        .bind(connection.connected_by)
        .bind(connection.connected_at)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Replaces the refresh token only if it is still `old`: if the wallet was reconnected while
    /// a check was using the old token, the new connection's token must not be overwritten.
    /// Returns whether it was replaced.
    pub async fn update_refresh_token(
        db: &crate::DB,
        corporation_id: i64,
        old: &str,
        new: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE srp_connection SET refresh_token = $3
                WHERE corporation_id = $1 AND refresh_token = $2",
        )
        .bind(corporation_id)
        .bind(old)
        .bind(new)
        .execute(db)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Returns whether there was one
    pub async fn delete(db: &crate::DB, corporation_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM srp_connection WHERE corporation_id = $1")
            .bind(corporation_id)
            .execute(db)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

/// Which SRP corp's payments cover each waitlist's pilots (table srp_waitlist), set on the SRP
/// wallet page. None until someone sets one: a deployment can be shared with communities that
/// don't run SRP. Kept when the corp's wallet is disconnected; the waitlist then shows nothing.
pub mod waitlists {
    #[derive(sqlx::FromRow)]
    pub struct Waitlist {
        pub id: i64,
        pub name: String,
        pub corporation_id: Option<i64>,
        /// Who set the corp and when
        pub set_by: Option<i64>,
        pub set_at: Option<i64>,
    }

    /// Every waitlist that isn't archived, with its SRP corp
    pub async fn list(db: &crate::DB) -> Result<Vec<Waitlist>, sqlx::Error> {
        sqlx::query_as::<_, Waitlist>(
            "SELECT w.id, w.name, s.corporation_id, s.set_by, s.set_at FROM waitlist w
                LEFT JOIN srp_waitlist s ON s.waitlist_id = w.id
                WHERE NOT w.is_archived ORDER BY w.name",
        )
        .fetch_all(db)
        .await
    }

    /// None when there's no such waitlist, or it's archived
    pub async fn active_name(
        db: &crate::DB,
        waitlist_id: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT name FROM waitlist WHERE id = $1 AND NOT is_archived")
            .bind(waitlist_id)
            .fetch_optional(db)
            .await
    }

    pub async fn corporation_of(
        db: &crate::DB,
        waitlist_id: i64,
    ) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar("SELECT corporation_id FROM srp_waitlist WHERE waitlist_id = $1")
            .bind(waitlist_id)
            .fetch_optional(db)
            .await
    }

    /// Sets the waitlist's SRP corp, or None for no SRP. Returns whether anything changed.
    pub async fn set(
        db: &crate::DB,
        waitlist_id: i64,
        corporation_id: Option<i64>,
        by: i64,
        now: i64,
    ) -> Result<bool, sqlx::Error> {
        let query = match corporation_id {
            Some(corporation_id) => sqlx::query(
                "INSERT INTO srp_waitlist (waitlist_id, corporation_id, set_by, set_at)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT (waitlist_id) DO UPDATE
                    SET corporation_id = $2, set_by = $3, set_at = $4
                    WHERE srp_waitlist.corporation_id <> $2",
            )
            .bind(waitlist_id)
            .bind(corporation_id)
            .bind(by)
            .bind(now),
            None => {
                sqlx::query("DELETE FROM srp_waitlist WHERE waitlist_id = $1").bind(waitlist_id)
            }
        };
        Ok(query.execute(db).await?.rows_affected() == 1)
    }
}

#[cfg(test)]
impl From<HashMap<i64, Payment>> for WalletRead {
    fn from(paid: HashMap<i64, Payment>) -> WalletRead {
        WalletRead {
            paid,
            data_as_of: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_as_old_as_its_older_source() {
        assert_eq!(older(Some(100), Some(200)), Some(100));
        assert_eq!(older(None, Some(200)), Some(200));
        assert_eq!(older(Some(100), None), Some(100));
        assert_eq!(older(None, None), None);
    }

    const CORP: i64 = 98000001;
    // 2026-10-09T15:00:00Z
    const NOW: i64 = 1791558000;

    fn rule() -> Rule {
        Rule {
            min_amount: 50_000_000.0,
            coverage_secs: 24 * 3600,
        }
    }

    fn donation(date: &str, from: i64, amount: f64) -> JournalEntry {
        JournalEntry {
            date: date.to_string(),
            ref_type: "player_donation".to_string(),
            amount: Some(amount),
            first_party_id: Some(from),
        }
    }

    fn contract(accepted: &str, acceptor: i64, price: f64) -> Contract {
        Contract {
            kind: "item_exchange".to_string(),
            status: "finished".to_string(),
            issuer_corporation_id: CORP,
            for_corporation: true,
            acceptor_id: acceptor,
            price: Some(price),
            date_accepted: Some(accepted.to_string()),
        }
    }

    #[test]
    fn donation_of_min_amount_or_more_covers_the_sender() {
        let entries = vec![
            donation("2026-10-09T14:41:00Z", 1, 50_000_000.0),
            donation("2026-10-09T09:12:00Z", 2, 55_000_000.0),
            donation("2026-10-09T10:00:00Z", 3, 49_999_999.0),
        ];
        let paid = paid_from_journal(&entries, NOW, &rule());
        assert_eq!(paid.len(), 2);
        assert_eq!(paid[&1].via, Via::Donation);
        assert_eq!(paid[&2].amount, 55_000_000.0);
        assert!(!paid.contains_key(&3));
    }

    #[test]
    fn donation_older_than_coverage_is_ignored() {
        let entries = vec![donation("2026-10-08T14:59:00Z", 1, 50_000_000.0)];
        assert!(paid_from_journal(&entries, NOW, &rule()).is_empty());
    }

    #[test]
    fn cover_ends_exactly_after_coverage_hours() {
        // NOW is 15:00; a payment exactly 24 hours earlier no longer covers
        let at_the_limit = vec![donation("2026-10-08T15:00:00Z", 1, 50_000_000.0)];
        assert!(paid_from_journal(&at_the_limit, NOW, &rule()).is_empty());
        let just_inside = vec![donation("2026-10-08T15:00:01Z", 1, 50_000_000.0)];
        assert!(paid_from_journal(&just_inside, NOW, &rule()).contains_key(&1));
    }

    #[test]
    fn other_journal_entries_are_ignored() {
        let mut entry = donation("2026-10-09T14:00:00Z", 1, 50_000_000.0);
        entry.ref_type = "corporation_account_withdrawal".to_string();
        let mut no_sender = donation("2026-10-09T14:00:00Z", 2, 50_000_000.0);
        no_sender.first_party_id = None;
        assert!(paid_from_journal(&[entry, no_sender], NOW, &rule()).is_empty());
    }

    #[test]
    fn small_payments_do_not_add_up() {
        let entries = vec![
            donation("2026-10-09T14:00:00Z", 1, 25_000_000.0),
            donation("2026-10-09T14:05:00Z", 1, 25_000_000.0),
        ];
        assert!(paid_from_journal(&entries, NOW, &rule()).is_empty());
    }

    #[test]
    fn latest_payment_sets_the_window() {
        let entries = vec![
            donation("2026-10-09T08:00:00Z", 1, 50_000_000.0),
            donation("2026-10-09T14:00:00Z", 1, 60_000_000.0),
        ];
        let paid = paid_from_journal(&entries, NOW, &rule());
        assert_eq!(paid[&1].amount, 60_000_000.0);
        assert_eq!(paid[&1].paid_at, timestamp("2026-10-09T14:00:00Z").unwrap());
    }

    #[test]
    fn loan_contract_covers_the_acceptor() {
        let paid = paid_from_contracts(
            &[contract("2026-10-09T13:40:00Z", 7, 50_000_000.0)],
            CORP,
            NOW,
            &rule(),
        );
        assert_eq!(paid[&7].via, Via::Contract);
    }

    #[test]
    fn contracts_that_are_not_corp_loans_are_ignored() {
        let mut personal = contract("2026-10-09T13:40:00Z", 1, 50_000_000.0);
        personal.for_corporation = false;
        let mut other_corp = contract("2026-10-09T13:40:00Z", 2, 50_000_000.0);
        other_corp.issuer_corporation_id = 98000002;
        let mut outstanding = contract("2026-10-09T13:40:00Z", 3, 50_000_000.0);
        outstanding.status = "outstanding".to_string();
        let mut courier = contract("2026-10-09T13:40:00Z", 4, 50_000_000.0);
        courier.kind = "courier".to_string();
        let cheap = contract("2026-10-09T13:40:00Z", 5, 10_000_000.0);
        let old = contract("2026-10-08T13:40:00Z", 6, 50_000_000.0);
        let mut unaccepted = contract("2026-10-09T13:40:00Z", 0, 50_000_000.0);
        unaccepted.date_accepted = None;

        let contracts = [
            personal,
            other_corp,
            outstanding,
            courier,
            cheap,
            old,
            unaccepted,
        ];
        assert!(paid_from_contracts(&contracts, CORP, NOW, &rule()).is_empty());
    }

    #[test]
    fn outgoing_donations_are_ignored() {
        // The corp paying a pilot is a player_donation too, with a negative amount
        let entries = vec![donation("2026-10-09T14:00:00Z", CORP, -50_000_000.0)];
        assert!(paid_from_journal(&entries, NOW, &rule()).is_empty());
    }

    #[test]
    fn same_time_donation_and_contract_keep_the_donation() {
        let journal = paid_from_journal(
            &[donation("2026-10-09T13:00:00Z", 1, 50_000_000.0)],
            NOW,
            &rule(),
        );
        let contracts = paid_from_contracts(
            &[contract("2026-10-09T13:00:00Z", 1, 60_000_000.0)],
            CORP,
            NOW,
            &rule(),
        );
        assert_eq!(merge(journal, contracts)[&1].via, Via::Donation);
    }

    #[test]
    fn current_payment_only_while_covered() {
        let paid = paid_from_journal(
            &[donation("2026-10-09T13:00:00Z", 1, 50_000_000.0)],
            NOW,
            &rule(),
        );
        assert!(current_payment(&paid, 1, NOW, &rule()).is_some());
        assert!(current_payment(&paid, 2, NOW, &rule()).is_none());
        assert!(current_payment(&paid, 1, NOW + 24 * 3600, &rule()).is_none());
    }

    #[test]
    fn merge_keeps_the_latest_payment_per_character() {
        let journal = paid_from_journal(
            &[donation("2026-10-09T09:00:00Z", 1, 50_000_000.0)],
            NOW,
            &rule(),
        );
        let contracts = paid_from_contracts(
            &[contract("2026-10-09T13:00:00Z", 1, 50_000_000.0)],
            CORP,
            NOW,
            &rule(),
        );
        let merged = merge(journal, contracts);
        assert_eq!(merged[&1].via, Via::Contract);
    }

    #[test]
    fn journal_paging_stops_past_the_window() {
        let recent = vec![donation("2026-10-09T10:00:00Z", 1, 1.0)];
        let reaching_back = vec![
            donation("2026-10-09T10:00:00Z", 1, 1.0),
            donation("2026-10-07T10:00:00Z", 1, 1.0),
        ];
        assert!(!journal_page_reaches_past_window(&recent, NOW, &rule()));
        assert!(journal_page_reaches_past_window(
            &reaching_back,
            NOW,
            &rule()
        ));
        assert!(journal_page_reaches_past_window(&[], NOW, &rule()));
    }

    // Shapes from ESI's published OpenAPI spec, with fields we don't use, and an entry missing the
    // optional ones
    #[test]
    fn parses_esi_responses() {
        let journal: Vec<JournalEntry> = serde_json::from_str(
            r#"[
                {"id": 1, "date": "2026-10-09T14:41:00Z", "ref_type": "player_donation",
                 "description": "Aria Kestrel deposited cash into Test Corp's account",
                 "amount": 50000000.0, "balance": 1250000000.0, "first_party_id": 2112000001,
                 "first_party_type": "character", "second_party_id": 98000001},
                {"id": 2, "date": "2026-10-09T14:00:00Z", "ref_type": "brokers_fee",
                 "description": "Market fee"}
            ]"#,
        )
        .unwrap();
        let contracts: Vec<Contract> = serde_json::from_str(
            r#"[
                {"contract_id": 5, "type": "item_exchange", "status": "finished",
                 "issuer_id": 2112000009, "issuer_corporation_id": 98000001, "assignee_id": 2112000002,
                 "acceptor_id": 2112000002, "for_corporation": true, "availability": "personal",
                 "date_issued": "2026-10-09T13:00:00Z", "date_expired": "2026-10-23T13:00:00Z",
                 "date_accepted": "2026-10-09T13:40:00Z", "price": 50000000.0, "volume": 1.0},
                {"contract_id": 6, "type": "courier", "status": "outstanding",
                 "issuer_id": 2112000009, "issuer_corporation_id": 98000001, "assignee_id": 0,
                 "acceptor_id": 0, "for_corporation": false, "availability": "public",
                 "date_issued": "2026-10-09T13:00:00Z", "date_expired": "2026-10-23T13:00:00Z"}
            ]"#,
        )
        .unwrap();

        let paid = merge(
            paid_from_journal(&journal, NOW, &rule()),
            paid_from_contracts(&contracts, CORP, NOW, &rule()),
        );
        assert_eq!(paid.len(), 2);
        assert_eq!(paid[&2112000001].via, Via::Donation);
        assert_eq!(paid[&2112000002].via, Via::Contract);
    }

    #[test]
    fn managers_by_name_or_id() {
        let managers = vec!["Wallet Keeper".to_string(), " 2112345678 ".to_string()];
        // A listed alt counts as much as a listed main; names ignore case and spaces
        assert!(named_manager(
            &managers,
            &[(1, "Main Pilot"), (2, "wallet keeper")]
        ));
        // An id survives a rename
        assert!(named_manager(&managers, &[(2112345678, "Renamed Pilot")]));
        assert!(!named_manager(
            &managers,
            &[(1, "Main Pilot"), (2, "Wallet Keeper Two")]
        ));
        // A name that happens to be digits is still read as an id
        assert!(!named_manager(&managers, &[(3, "2112345678")]));
        assert!(!named_manager(&[], &[(2, "Wallet Keeper")]));
    }

    #[test]
    fn names_compare_like_the_database() {
        assert!(same_name(" Wallet Keeper ", "wallet keeper"));
        assert!(same_name("ÉLAN", "élan"));
        assert!(!same_name("Wallet Keeper", "Wallet Keeper Two"));
    }

    #[test]
    fn connect_state_round_trips() {
        assert_eq!(
            parse_connect_state(&connect_state(2112345678)),
            Some(2112345678)
        );
        assert_eq!(parse_connect_state(b"oauthhellostate"), None);
        assert_eq!(parse_connect_state(b"srpconnect:abc"), None);
        assert_eq!(parse_connect_state(b"srpconnect:"), None);
    }

    #[test]
    fn state_goes_stale() {
        let mut corp = CorpState::default();
        assert!(!corp.is_fresh(NOW));
        corp.checked_at = Some(NOW - 60);
        assert!(corp.is_fresh(NOW));
        corp.checked_at = Some(NOW - STALE_AFTER_SECS);
        assert!(!corp.is_fresh(NOW));
    }

    fn paid_one() -> HashMap<i64, Payment> {
        paid_from_journal(
            &[donation("2026-10-09T13:00:00Z", 1, 50_000_000.0)],
            NOW,
            &rule(),
        )
    }

    #[test]
    fn connection_change_clears_that_corp_only() {
        let mut state = SrpState::default();
        state.corp_mut(CORP).apply(0, Ok(paid_one().into()), NOW);
        state
            .corp_mut(CORP + 1)
            .apply(0, Ok(paid_one().into()), NOW);
        state.connect_failed(NOW, "failed".to_string());
        state.wallet_removed(CORP);
        assert!(!state.corp(CORP).unwrap().is_fresh(NOW));
        assert!(state.corp(CORP).unwrap().paid.is_empty());
        assert_eq!(state.generation(CORP), 1);
        assert!(state.corp(CORP + 1).unwrap().is_fresh(NOW));
        assert_eq!(state.generation(CORP + 1), 0);
    }

    #[test]
    fn a_reconnect_keeps_the_last_good_result() {
        let mut state = SrpState::default();
        state.corp_mut(CORP).apply(0, Ok(paid_one().into()), NOW);
        state.connect_failed(NOW, "failed".to_string());
        state.wallet_replaced(CORP);
        assert!(state.recent_connect_error(NOW).is_none());
        assert!(state.corp(CORP).unwrap().is_fresh(NOW));
        assert_eq!(state.corp(CORP).unwrap().paid.len(), 1);
        assert_eq!(state.generation(CORP), 1);
    }

    #[test]
    fn connect_error_is_shown_for_a_while() {
        let mut state = SrpState::default();
        state.connect_failed(NOW, "failed".to_string());
        assert_eq!(
            state.recent_connect_error(NOW + 60).as_deref(),
            Some("failed")
        );
        assert_eq!(
            state.recent_connect_error(NOW + CONNECT_ERROR_SHOWN_SECS),
            None
        );
    }

    #[test]
    fn a_failed_check_keeps_the_last_good_result() {
        let mut corp = CorpState::default();
        assert_eq!(corp.apply(0, Ok(paid_one().into()), NOW), Applied::Updated);
        assert_eq!(
            corp.apply(0, Err("ESI down".to_string()), NOW + 300),
            Applied::Failed
        );
        assert_eq!(corp.paid.len(), 1);
        assert_eq!(corp.checked_at, Some(NOW));
        assert_eq!(corp.last_error.as_deref(), Some("ESI down"));
        assert_eq!(
            corp.apply(0, Ok(paid_one().into()), NOW + 600),
            Applied::Recovered
        );
        assert!(corp.last_error.is_none());
    }

    #[test]
    fn a_check_for_an_old_wallet_is_thrown_away() {
        let mut state = SrpState::default();
        let started_with = state.generation(CORP);
        state.wallet_replaced(CORP);
        assert_eq!(
            state
                .corp_mut(CORP)
                .apply(started_with, Ok(paid_one().into()), NOW),
            Applied::Discarded
        );
        assert!(state.corp(CORP).unwrap().paid.is_empty());
        let generation = state.generation(CORP);
        assert_eq!(
            state
                .corp_mut(CORP)
                .apply(generation, Ok(paid_one().into()), NOW),
            Applied::Updated
        );
    }

    #[test]
    fn recent_is_newest_first_and_only_covered() {
        let mut corp = CorpState::default();
        corp.paid = paid_from_journal(
            &[
                donation("2026-10-09T09:00:00Z", 1, 50_000_000.0),
                donation("2026-10-09T13:00:00Z", 2, 50_000_000.0),
            ],
            NOW,
            &rule(),
        );
        let ids: Vec<i64> = corp
            .recent(NOW, &rule())
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, vec![2, 1]);
        // 1 paid at 09:00, so its cover ends at 09:00 the next day
        let ids: Vec<i64> = corp
            .recent(NOW + 20 * 3600, &rule())
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(ids, vec![2]);
    }
}
