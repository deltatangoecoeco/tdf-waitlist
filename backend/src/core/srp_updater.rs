use std::sync::Arc;
use std::time::Duration;

use crate::config::{Config, SrpConfig};
use crate::core::esi::{ESIClient, ESIError};
use crate::core::srp::{
    self, connection, database_error, Applied, Rule, SharedSrpState, WalletRead,
};

/// The journal is cached by ESI for an hour and contracts for five minutes, so checking more often
/// than this gains nothing
const INTERVAL: Duration = Duration::from_secs(300);
/// Stop paging past this; a corp's contract list keeps 30 days, normally one page
const MAX_PAGES: i32 = 10;
/// For each SeAT or ESI step of a check; see `within`
const TIMEOUT: Duration = Duration::from_secs(120);

/// Checks every connected SRP wallet every INTERVAL, or straight away when asked
/// (`Shared::check_now`, after a connect). Checks run one at a time, in this one task: SeAT may
/// rotate the refresh token on every use, so two at once would leave one holding a revoked token.
/// For the same reason only one backend process may run it.
pub struct SrpUpdater {
    esi_client: ESIClient,
    db: Arc<crate::DB>,
    config: Config,
    state: SharedSrpState,
}

impl SrpUpdater {
    pub fn new(db: Arc<crate::DB>, config: Config, state: SharedSrpState) -> SrpUpdater {
        SrpUpdater {
            esi_client: ESIClient::new(
                db.clone(),
                config.esi.client_id.clone(),
                config.esi.client_secret.clone(),
            ),
            db,
            config,
            state,
        }
    }

    pub fn start(self) {
        tokio::spawn(Arc::new(self).run());
    }

    async fn run(self: Arc<Self>) {
        loop {
            let corporation_ids = connection::corporation_ids(&self.db)
                .await
                .unwrap_or_else(|e| {
                    database_error(e);
                    Vec::new()
                });
            for corporation_id in corporation_ids {
                // In its own task, so a panic (the SeAT token parsing unwraps) costs one check,
                // not the updater
                let updater = self.clone();
                if let Err(e) =
                    tokio::spawn(async move { updater.check(corporation_id).await }).await
                {
                    error!("SRP check of corp {} crashed: {}", corporation_id, e);
                    self.state.write().corp_mut(corporation_id).last_error = Some(
                        "The SRP check crashed; the server log has the details. It tries again \
                        in a few minutes."
                            .to_string(),
                    );
                }
            }
            self.state.wait(INTERVAL).await;
        }
    }

    /// Reads one corp's wallet and records the result
    async fn check(&self, corporation_id: i64) {
        // The generation before the wallet: a wallet replaced or removed after this read bumps
        // it, so the result is thrown away rather than published for a wallet that's gone
        let generation = self.state.read().generation(corporation_id);
        let wallet = match connection::get(&self.db, corporation_id).await {
            Ok(Some(wallet)) => wallet,
            Ok(None) => return,
            Err(e) => {
                database_error(e);
                return;
            }
        };
        let result = fetch(&self.esi_client, &self.db, &self.config.srp, &wallet).await;
        let now = chrono::Utc::now().timestamp();

        let message = result.as_ref().err().cloned();
        let applied = self
            .state
            .write()
            .corp_mut(corporation_id)
            .apply(generation, result, now);
        match applied {
            Applied::Failed => warn!(
                "SRP check of corp {} failed: {}",
                corporation_id,
                message.unwrap_or_default()
            ),
            Applied::Recovered => info!("SRP check of corp {} working again", corporation_id),
            Applied::Discarded => info!(
                "SRP check of corp {} discarded: its wallet was connected or disconnected meanwhile",
                corporation_id
            ),
            Applied::Updated => {}
        }
    }
}

/// Whether to fetch another page. Hitting MAX_PAGES with pages left is an error: the payments on
/// the pages not read would otherwise show as unpaid while the check looked healthy.
fn more_pages(page: i32, pages: i32, what: &str) -> Result<bool, String> {
    if page >= pages {
        Ok(false)
    } else if page >= MAX_PAGES {
        Err(format!(
            "The {} has more than {} pages, more than the check reads, so some payments would be \
            missed. Tell the waitlist maintainers.",
            what, MAX_PAGES
        ))
    } else {
        Ok(true)
    }
}

/// Pages through `path` until the last page, or until `done` says a page is enough. Also returns
/// when ESI produced the data, from the first page.
async fn fetch_pages<T: serde::de::DeserializeOwned>(
    esi: &ESIClient,
    path: &str,
    token: &str,
    what: &str,
    done: impl Fn(&[T]) -> bool,
) -> Result<(Vec<T>, Option<i64>), String> {
    let mut all = Vec::new();
    let mut produced_at = None;
    for page in 1.. {
        let fetched = esi
            .get_page::<Vec<T>>(path, token, page)
            .await
            .map_err(|e| describe_esi_error(what, e))?;
        if page == 1 {
            produced_at = fetched.last_modified;
        }
        let enough = done(&fetched.body);
        all.extend(fetched.body);
        if enough || !more_pages(page, fetched.pages, what)? {
            break;
        }
    }
    Ok((all, produced_at))
}

async fn fetch(
    esi: &ESIClient,
    db: &crate::DB,
    config: &SrpConfig,
    conn: &connection::Connection,
) -> Result<WalletRead, String> {
    let (seat_token, new_refresh_token) = within(async {
        esi.refresh_seat_login(&conn.refresh_token)
            .await
            .map_err(describe_token_error)
    })
    .await?;
    if let Some(new_refresh_token) = new_refresh_token {
        store_refresh_token(db, conn, &new_refresh_token).await?;
    }
    within(read_wallet(esi, config, conn, &seat_token)).await
}

/// Runs `step` with TIMEOUT: the HTTP client has none of its own, and a check that never finishes
/// would stop every later one
async fn within<T>(
    step: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(TIMEOUT, step)
        .await
        .unwrap_or_else(|_| {
            Err(format!(
            "SeAT or ESI took over {} seconds to answer. Usually temporary; the next check runs \
            in a few minutes.",
            TIMEOUT.as_secs()
        ))
        })
}

/// Stores SeAT's replacement refresh token before anything else can fail: if SeAT rotated it, this
/// is the only working copy. Outside any timeout, and tried twice, for the same reason. If the
/// wallet was reconnected meanwhile this replaces nothing, and the check's result is thrown away.
async fn store_refresh_token(
    db: &crate::DB,
    conn: &connection::Connection,
    new: &str,
) -> Result<(), String> {
    let store =
        || connection::update_refresh_token(db, conn.corporation_id, &conn.refresh_token, new);
    if store().await.is_ok() {
        return Ok(());
    }
    store().await.map(|_| ()).map_err(database_error)
}

async fn read_wallet(
    esi: &ESIClient,
    config: &SrpConfig,
    conn: &connection::Connection,
    seat_token: &str,
) -> Result<WalletRead, String> {
    let token = esi
        .passthrough_token(seat_token, conn.character_id, srp::WALLET_SCOPES)
        .await
        .map_err(describe_token_error)?;

    let rule = Rule::from_config(config);
    let now = chrono::Utc::now().timestamp();

    let (journal, journal_as_of) = fetch_pages(
        esi,
        &format!(
            "/corporations/{}/wallets/{}/journal",
            conn.corporation_id, config.division
        ),
        &token,
        "wallet journal",
        |page| srp::journal_page_reaches_past_window(page, now, &rule),
    )
    .await?;
    let (contracts, contracts_as_of) = fetch_pages(
        esi,
        &format!("/corporations/{}/contracts", conn.corporation_id),
        &token,
        "contract list",
        |_: &[srp::Contract]| false,
    )
    .await?;

    Ok(WalletRead {
        paid: srp::merge(
            srp::paid_from_journal(&journal, now, &rule),
            srp::paid_from_contracts(&contracts, conn.corporation_id, now, &rule),
        ),
        data_as_of: srp::older(journal_as_of, contracts_as_of),
    })
}

fn describe_token_error(error: ESIError) -> String {
    match error {
        ESIError::MissingScope => "SeAT didn't grant the wallet and contracts permissions for the \
            wallet character. Check the character has them in SeAT, then reconnect."
            .to_string(),
        ESIError::Status(400) | ESIError::Status(401) => {
            "SeAT refused the stored login; it may have been revoked. Reconnect.".to_string()
        }
        e => format!(
            "Couldn't get a token from SeAT ({}). Usually temporary; if it keeps happening, \
            reconnect.",
            e
        ),
    }
}

fn describe_esi_error(what: &str, error: ESIError) -> String {
    match error {
        ESIError::Status(403) => format!(
            "ESI refused to show the {}: the wallet character must be in the SRP corp, and the \
            journal needs its Junior Accountant (or Accountant) role. Ask a director to restore \
            it, then click Reconnect.",
            what
        ),
        e => format!(
            "Couldn't read the {} from ESI ({}). ESI may be down; the next check runs in a few \
            minutes.",
            what, e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_until_the_last() {
        assert_eq!(more_pages(1, 3, "x"), Ok(true));
        assert_eq!(more_pages(3, 3, "x"), Ok(false));
        assert_eq!(more_pages(1, 1, "x"), Ok(false));
    }

    #[test]
    fn running_out_of_pages_is_an_error() {
        assert!(more_pages(MAX_PAGES, MAX_PAGES + 1, "contract list").is_err());
        assert_eq!(more_pages(MAX_PAGES, MAX_PAGES, "contract list"), Ok(false));
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(describe_token_error(ESIError::MissingScope).contains("SeAT"));
        assert!(describe_token_error(ESIError::Status(401)).ends_with("Reconnect."));
        let refused = describe_esi_error("wallet journal", ESIError::Status(403));
        assert!(refused.contains("wallet journal") && refused.contains("Junior Accountant"));
        assert!(describe_esi_error("contract list", ESIError::Status(503)).contains("next check"));
    }
}
