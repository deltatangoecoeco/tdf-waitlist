use std::collections::HashMap;

use rocket::response::Redirect;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::{
    app::Application,
    core::{
        auth::{authorize_character, AuthenticatedAccount},
        esi::ESIClient,
        esi::ESIError,
        srp::{self, connection, database_error, Payment, Rule, SrpState, Via},
    },
    data::character,
    util::madness::Madness,
};

// Enough for any account's characters; stops the route being used for bulk lookups
const MAX_CHARACTERS: usize = 50;
// Larger than any fleet the waitlist runs
const MAX_FLEET_CHARACTERS: usize = 300;

fn parse_ids(character_ids: &str, max: usize) -> Result<Vec<i64>, Madness> {
    let ids = character_ids
        .split(',')
        .map(|id| id.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Madness::BadRequest("Invalid character_ids".to_string()))?;
    match ids.len() > max {
        true => Err(Madness::BadRequest("Too many characters".to_string())),
        false => Ok(ids),
    }
}

#[derive(Debug, Serialize)]
struct CharacterStatus {
    id: i64,
    paid: bool,
    expires_at: Option<i64>,
    via: Option<Via>,
}

fn character_status(
    paid: &HashMap<i64, Payment>,
    id: i64,
    now: i64,
    rule: &Rule,
) -> CharacterStatus {
    let payment = srp::current_payment(paid, id, now, rule);
    CharacterStatus {
        id,
        paid: payment.is_some(),
        expires_at: payment.map(|p| rule.expires_at(p.paid_at)),
        via: payment.map(|p| p.via),
    }
}

/// What pilots on a waitlist see, given the waitlist's SRP corp
#[derive(Debug)]
enum WaitlistStatus {
    /// The waitlist has no SRP corp
    NotApplicable,
    /// Its corp has no recent successful check: show nothing rather than guess
    Unavailable,
    /// Each character's cover from payments to that corp
    Available {
        checked_at: Option<i64>,
        /// When ESI produced the wallet data; a payment made after it can't show yet
        data_as_of: Option<i64>,
        characters: Vec<CharacterStatus>,
    },
}

fn waitlist_status(
    state: &SrpState,
    corporation_id: Option<i64>,
    ids: &[i64],
    now: i64,
    rule: &Rule,
) -> WaitlistStatus {
    let corporation_id = match corporation_id {
        Some(c) => c,
        None => return WaitlistStatus::NotApplicable,
    };
    match state.corp(corporation_id) {
        Some(corp) if corp.is_fresh(now) => WaitlistStatus::Available {
            checked_at: corp.checked_at,
            data_as_of: corp.data_as_of,
            characters: ids
                .iter()
                .map(|&id| character_status(&corp.paid, id, now, rule))
                .collect(),
        },
        _ => WaitlistStatus::Unavailable,
    }
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    /// False when the check is switched off or this waitlist has no SRP corp: the page can stop
    /// asking
    applies: bool,
    /// False also when there's no recent successful check of the waitlist's SRP corp (not
    /// connected, or failing for over two hours): show nothing rather than guess
    available: bool,
    checked_at: Option<i64>,
    min_amount: i64,
    coverage_hours: i64,
    characters: Vec<CharacterStatus>,
}

/// The characters' SRP cover on `waitlist_id`, from payments to that waitlist's SRP corp
#[get("/api/srp/status?<character_ids>&<waitlist_id>")]
async fn status(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    character_ids: &str,
    waitlist_id: i64,
) -> Result<Json<StatusResponse>, Madness> {
    let ids = parse_ids(character_ids, MAX_CHARACTERS)?;

    let config = &app.config.srp;
    let rule = Rule::from_config(config);
    let now = chrono::Utc::now().timestamp();
    let nothing = |applies| StatusResponse {
        applies,
        available: false,
        checked_at: None,
        min_amount: config.min_amount,
        coverage_hours: config.coverage_hours,
        characters: Vec::new(),
    };

    // Before checking the characters: most requests come from waitlists SRP doesn't apply to
    if !config.enable {
        return Ok(Json(nothing(false)));
    }
    let corporation_id = match srp::waitlists::corporation_of(app.get_db(), waitlist_id).await? {
        Some(c) => c,
        None => return Ok(Json(nothing(false))),
    };

    for &id in &ids {
        authorize_character(app.get_db(), &account, id, None).await?;
    }

    let status = waitlist_status(
        &app.srp_state.read(),
        Some(corporation_id),
        &ids,
        now,
        &rule,
    );
    let (checked_at, characters) = match status {
        WaitlistStatus::Available {
            checked_at,
            characters,
            ..
        } => (checked_at, characters),
        WaitlistStatus::Unavailable => return Ok(Json(nothing(true))),
        WaitlistStatus::NotApplicable => return Ok(Json(nothing(false))),
    };

    Ok(Json(StatusResponse {
        applies: true,
        available: true,
        checked_at,
        min_amount: config.min_amount,
        coverage_hours: config.coverage_hours,
        characters,
    }))
}

#[derive(Debug, Serialize)]
struct FleetStatusResponse {
    /// As for pilots: false when SRP is off or the waitlist has no SRP corp
    applies: bool,
    available: bool,
    checked_at: Option<i64>,
    /// When ESI produced the wallet data. A member who joined the fleet after it may have paid
    /// already without it showing.
    data_as_of: Option<i64>,
    characters: Vec<CharacterStatus>,
}

/// For FCs: the SRP cover of fleet members on `waitlist_id`. Any characters, unlike the pilot
/// route, so it needs fleet access.
#[get("/api/srp/fleet?<character_ids>&<waitlist_id>")]
async fn fleet_status(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    character_ids: &str,
    waitlist_id: i64,
) -> Result<Json<FleetStatusResponse>, Madness> {
    account.require_access("fleet-view")?;
    let ids = parse_ids(character_ids, MAX_FLEET_CHARACTERS)?;

    let config = &app.config.srp;
    let corporation_id = match config.enable {
        true => srp::waitlists::corporation_of(app.get_db(), waitlist_id).await?,
        false => None,
    };
    let now = chrono::Utc::now().timestamp();
    let status = waitlist_status(
        &app.srp_state.read(),
        corporation_id,
        &ids,
        now,
        &Rule::from_config(config),
    );

    let response = match status {
        WaitlistStatus::Available {
            checked_at,
            data_as_of,
            characters,
        } => FleetStatusResponse {
            applies: true,
            available: true,
            checked_at,
            data_as_of,
            characters,
        },
        other => FleetStatusResponse {
            applies: matches!(other, WaitlistStatus::Unavailable),
            available: false,
            checked_at: None,
            data_as_of: None,
            characters: Vec::new(),
        },
    };
    Ok(Json(response))
}

#[derive(Debug, Serialize)]
struct NamedCharacter {
    id: i64,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct RecentPayment {
    character: NamedCharacter,
    amount: f64,
    paid_at: i64,
    expires_at: i64,
    via: Via,
}

/// One connected SRP corp's wallet and its check
#[derive(Debug, Serialize)]
struct Wallet {
    corporation_id: i64,
    corporation_name: String,
    character: NamedCharacter,
    connected_by: NamedCharacter,
    connected_at: i64,
    checked_at: Option<i64>,
    last_error: Option<String>,
    /// Payments still inside the coverage window, newest first. How an admin checks the result
    /// against the in-game wallet.
    recent: Vec<RecentPayment>,
}

#[derive(Debug, Serialize)]
struct AdminWaitlist {
    id: i64,
    name: String,
    /// The SRP corp whose payments cover its pilots; None for no SRP. May be a corp whose wallet
    /// has since been disconnected.
    corporation_id: Option<i64>,
    set_by: Option<NamedCharacter>,
    set_at: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AdminResponse {
    enabled: bool,
    min_amount: i64,
    coverage_hours: i64,
    division: i32,
    /// Manager names in config that match no character the waitlist knows
    unknown_managers: Vec<String>,
    /// Why the last connect failed, for a few minutes after it did
    connect_error: Option<String>,
    /// By corp name
    wallets: Vec<Wallet>,
    /// Every waitlist that isn't archived, by name
    waitlists: Vec<AdminWaitlist>,
}

async fn require_manager(app: &Application, account: &AuthenticatedAccount) -> Result<(), Madness> {
    match srp::is_manager(app.get_db(), &app.config.srp, account).await? {
        true => Ok(()),
        false => Err(Madness::AccessDenied),
    }
}

#[get("/api/srp/admin")]
async fn admin_status(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
) -> Result<Json<AdminResponse>, Madness> {
    require_manager(app, &account).await?;

    let config = &app.config.srp;
    let rule = Rule::from_config(config);
    let now = chrono::Utc::now().timestamp();

    let connections = connection::all(app.get_db()).await?;
    let waitlists = srp::waitlists::list(app.get_db()).await?;
    let unknown_managers = srp::unknown_managers(app.get_db(), config).await?;

    // Copied out so the lock isn't held across the name lookup below
    struct Check {
        checked_at: Option<i64>,
        last_error: Option<String>,
        recent: Vec<(i64, Payment)>,
    }
    let (connect_error, checks) = {
        let state = app.srp_state.read();
        let checks: Vec<Check> = connections
            .iter()
            .map(|c| match state.corp(c.corporation_id) {
                Some(corp) => Check {
                    checked_at: corp.checked_at,
                    last_error: corp.last_error.clone(),
                    recent: corp.recent(now, &rule),
                },
                None => Check {
                    checked_at: None,
                    last_error: None,
                    recent: Vec::new(),
                },
            })
            .collect();
        (state.recent_connect_error(now), checks)
    };

    let mut ids: Vec<i64> = Vec::new();
    for (c, check) in connections.iter().zip(&checks) {
        ids.extend([c.character_id, c.connected_by]);
        ids.extend(check.recent.iter().map(|(id, _)| *id));
    }
    ids.extend(waitlists.iter().filter_map(|w| w.set_by));
    let names = character::lookup(app.get_db(), &ids).await?;
    let named = |id: i64| NamedCharacter {
        id,
        name: names.get(&id).map(|c| c.name.clone()),
    };

    let wallets = connections
        .into_iter()
        .zip(checks)
        .map(|(c, check)| Wallet {
            corporation_id: c.corporation_id,
            corporation_name: c.corporation_name,
            character: named(c.character_id),
            connected_by: named(c.connected_by),
            connected_at: c.connected_at,
            checked_at: check.checked_at,
            last_error: check.last_error,
            recent: check
                .recent
                .into_iter()
                .map(|(id, p)| RecentPayment {
                    character: named(id),
                    amount: p.amount,
                    paid_at: p.paid_at,
                    expires_at: rule.expires_at(p.paid_at),
                    via: p.via,
                })
                .collect(),
        })
        .collect();

    Ok(Json(AdminResponse {
        enabled: config.enable,
        min_amount: config.min_amount,
        coverage_hours: config.coverage_hours,
        division: config.division,
        unknown_managers,
        connect_error,
        wallets,
        waitlists: waitlists
            .into_iter()
            .map(|w| AdminWaitlist {
                id: w.id,
                name: w.name,
                corporation_id: w.corporation_id,
                set_by: w.set_by.map(named),
                set_at: w.set_at,
            })
            .collect(),
    }))
}

/// Starts connecting `character_id` as the wallet character of its corp: returns the SeAT login
/// URL. The login comes back through the normal auth callback, which hands it to `finish_connect`.
#[get("/api/srp/admin/connect?<character_id>")]
async fn connect(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    character_id: i64,
) -> Result<String, Madness> {
    require_manager(app, &account).await?;
    if !app.config.srp.enable {
        return Err(Madness::BadRequest(SWITCHED_OFF.to_string()));
    }
    authorize_character(app.get_db(), &account, character_id, None).await?;

    Ok(super::auth::authorize_url(
        app,
        &srp::connect_scopes(),
        &srp::connect_state(character_id),
    ))
}

#[derive(Deserialize)]
struct Disconnect {
    corporation_id: i64,
}

/// Removes one SRP corp's wallet. Waitlists using that corp keep it, and show nothing until it's
/// reconnected.
#[post("/api/srp/admin/disconnect", data = "<input>")]
async fn disconnect(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    input: Json<Disconnect>,
) -> Result<&'static str, Madness> {
    require_manager(app, &account).await?;
    let deleted = connection::delete(app.get_db(), input.corporation_id).await?;
    app.srp_state.write().wallet_removed(input.corporation_id);
    if deleted {
        info!(
            "SRP wallet of corp {} disconnected by {}",
            input.corporation_id, account.id
        );
    }
    Ok("OK")
}

#[derive(Deserialize)]
struct SetWaitlist {
    waitlist_id: i64,
    /// None for no SRP
    corporation_id: Option<i64>,
}

/// Sets which SRP corp's payments cover a waitlist's pilots, or none
#[post("/api/srp/admin/waitlist", data = "<input>")]
async fn set_waitlist(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    input: Json<SetWaitlist>,
) -> Result<&'static str, Madness> {
    require_manager(app, &account).await?;
    let name = srp::waitlists::active_name(app.get_db(), input.waitlist_id)
        .await?
        .ok_or_else(|| Madness::BadRequest("No such waitlist".to_string()))?;
    if let Some(corporation_id) = input.corporation_id {
        if !connection::exists(app.get_db(), corporation_id).await? {
            return Err(Madness::BadRequest(
                "That SRP corp's wallet isn't connected".to_string(),
            ));
        }
    }

    let now = chrono::Utc::now().timestamp();
    if srp::waitlists::set(
        app.get_db(),
        input.waitlist_id,
        input.corporation_id,
        account.id,
        now,
    )
    .await?
    {
        match input.corporation_id {
            Some(c) => info!(
                "SRP on waitlist {} ({}) now from corp {}, by {}",
                name, input.waitlist_id, c, account.id
            ),
            None => info!(
                "SRP removed from waitlist {} ({}) by {}",
                name, input.waitlist_id, account.id
            ),
        }
    }
    Ok("OK")
}

const SWITCHED_OFF: &str = "The SRP check is switched off in the server config.";

/// Completes a connect started by `connect`, then sends the admin back to the SRP wallet page,
/// which shows the reason if it failed. The check itself runs in the updater, not in this login
/// redirect: it can take a while, and the request may be dropped.
pub async fn finish_connect(
    app: &Application,
    account: Option<AuthenticatedAccount>,
    code: &str,
    character_id: i64,
) -> Redirect {
    match save_connection(app, account, code, character_id).await {
        Ok(corporation_id) => {
            app.srp_state.write().wallet_replaced(corporation_id);
            app.srp_state.check_now();
        }
        Err(message) => {
            warn!("SRP wallet connect failed: {}", message);
            let now = chrono::Utc::now().timestamp();
            app.srp_state.write().connect_failed(now, message);
        }
    }
    Redirect::to("/fc/srp")
}

/// The corp `character_id` is in, as id and name
async fn corporation_of(esi: &ESIClient, character_id: i64) -> Result<(i64, String), String> {
    #[derive(Deserialize)]
    struct CharacterInfo {
        corporation_id: i64,
    }
    #[derive(Deserialize)]
    struct CorporationInfo {
        name: String,
    }
    let info: CharacterInfo = esi
        .get(
            &format!("/v5/characters/{}/", character_id),
            character_id,
            None,
        )
        .await
        .map_err(|e| format!("Couldn't look up the character's corp: {}", e))?;
    let corporation: CorporationInfo = esi
        .get(
            &format!("/v5/corporations/{}/", info.corporation_id),
            character_id,
            None,
        )
        .await
        .map_err(|e| format!("Couldn't look up the corp: {}", e))?;
    Ok((info.corporation_id, corporation.name))
}

/// Stores the login as the wallet of the character's corp, replacing that corp's previous one.
/// Returns the corp.
async fn save_connection(
    app: &Application,
    account: Option<AuthenticatedAccount>,
    code: &str,
    character_id: i64,
) -> Result<i64, String> {
    let account = account.ok_or(
        "Log in to the waitlist first, then connect the SRP wallet from the FC dashboard.",
    )?;
    if !srp::is_manager(app.get_db(), &app.config.srp, &account)
        .await
        .map_err(database_error)?
    {
        return Err("You don't have permission to connect the SRP wallet.".to_string());
    }
    if !app.config.srp.enable {
        return Err(SWITCHED_OFF.to_string());
    }

    // Not save_auth: this login must not replace the admin's normal one
    let auth = app
        .esi_client
        .exchange_code(code, &srp::connect_scopes())
        .await
        .map_err(|e| match e {
            ESIError::MissingScope => "SeAT didn't grant the wallet and contracts permissions. \
                Check the character has them in SeAT, then try again."
                .to_string(),
            e => format!("The SeAT login failed: {}", e),
        })?;
    let character_name = auth
        .alt_ids
        .iter()
        .flatten()
        .find(|a| a.id == character_id)
        .map(|a| a.name.clone())
        .or_else(|| (auth.character_id == character_id).then(|| auth.character_name.clone()))
        .ok_or("The character you picked isn't on the SeAT account you logged in with.")?;
    let refresh_token = auth
        .refresh_token
        .ok_or("SeAT didn't return a refresh token.")?;

    let (corporation_id, corporation_name) = corporation_of(&app.esi_client, character_id).await?;

    character::upsert(app.get_db(), character_id, &character_name)
        .await
        .map_err(database_error)?;
    connection::save(
        app.get_db(),
        &connection::Connection {
            character_id,
            corporation_id,
            corporation_name,
            refresh_token,
            connected_by: account.id,
            connected_at: chrono::Utc::now().timestamp(),
        },
    )
    .await
    .map_err(database_error)?;

    info!(
        "SRP wallet of corp {} connected: character {}, by {}",
        corporation_id, character_id, account.id
    );
    Ok(corporation_id)
}

pub fn routes() -> Vec<rocket::Route> {
    routes![
        status,
        fleet_status,
        admin_status,
        set_waitlist,
        connect,
        disconnect
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TDF_CORP: i64 = 98000001;
    const CN_CORP: i64 = 98000002;
    const NOW: i64 = 1_000_000;

    fn paid(character_id: i64) -> HashMap<i64, Payment> {
        [(
            character_id,
            Payment {
                paid_at: NOW - 3600,
                amount: 50_000_000.0,
                via: Via::Donation,
            },
        )]
        .into()
    }

    fn paid_ids(status: WaitlistStatus) -> Vec<i64> {
        match status {
            WaitlistStatus::Available { characters, .. } => characters
                .into_iter()
                .filter(|c| c.paid)
                .map(|c| c.id)
                .collect(),
            other => panic!("expected a status, got {:?}", other),
        }
    }

    // The fleet page tells "may have paid, can't show yet" from "not paid" by the data's age
    #[test]
    fn status_carries_when_the_wallet_data_was_produced() {
        let rule = Rule {
            min_amount: 50_000_000.0,
            coverage_secs: 24 * 3600,
        };
        let mut state = SrpState::default();
        let read = srp::WalletRead {
            paid: paid(1),
            data_as_of: Some(NOW - 1200),
        };
        state.corp_mut(TDF_CORP).apply(0, Ok(read), NOW);
        match waitlist_status(&state, Some(TDF_CORP), &[1, 2], NOW, &rule) {
            WaitlistStatus::Available {
                checked_at,
                data_as_of,
                ..
            } => {
                assert_eq!(checked_at, Some(NOW));
                assert_eq!(data_as_of, Some(NOW - 1200));
            }
            other => panic!("expected a status, got {:?}", other),
        }
    }

    // Two waitlists on different SRP corps: a pilot is covered only on the waitlist whose corp
    // they paid
    #[test]
    fn each_waitlist_counts_only_its_own_corps_payments() {
        let rule = Rule {
            min_amount: 50_000_000.0,
            coverage_secs: 24 * 3600,
        };
        let mut state = SrpState::default();
        // Character 1 paid the TDF corp, character 2 the CN corp
        state.corp_mut(TDF_CORP).apply(0, Ok(paid(1).into()), NOW);
        state.corp_mut(CN_CORP).apply(0, Ok(paid(2).into()), NOW);
        let pilots = [1, 2, 3];

        let tdf_waitlist = waitlist_status(&state, Some(TDF_CORP), &pilots, NOW, &rule);
        assert_eq!(paid_ids(tdf_waitlist), vec![1]);
        let cn_waitlist = waitlist_status(&state, Some(CN_CORP), &pilots, NOW, &rule);
        assert_eq!(paid_ids(cn_waitlist), vec![2]);

        // A waitlist with no SRP corp shows nothing
        assert!(matches!(
            waitlist_status(&state, None, &pilots, NOW, &rule),
            WaitlistStatus::NotApplicable
        ));
        // Nor does one whose corp has no wallet (disconnected) or no recent check
        assert!(matches!(
            waitlist_status(&state, Some(98000003), &pilots, NOW, &rule),
            WaitlistStatus::Unavailable
        ));
        state.wallet_removed(CN_CORP);
        assert!(matches!(
            waitlist_status(&state, Some(CN_CORP), &pilots, NOW, &rule),
            WaitlistStatus::Unavailable
        ));
        // The other corp's waitlist is unaffected
        let tdf_waitlist = waitlist_status(&state, Some(TDF_CORP), &pilots, NOW, &rule);
        assert_eq!(paid_ids(tdf_waitlist), vec![1]);
    }

    #[test]
    fn character_status_shows_only_current_cover() {
        let rule = Rule {
            min_amount: 50_000_000.0,
            coverage_secs: 24 * 3600,
        };
        let paid: HashMap<i64, Payment> = [(
            1,
            Payment {
                paid_at: 1000,
                amount: 50_000_000.0,
                via: Via::Contract,
            },
        )]
        .into();

        let covered = character_status(&paid, 1, 2000, &rule);
        assert!(covered.paid);
        assert_eq!(covered.expires_at, Some(1000 + 24 * 3600));
        assert_eq!(covered.via, Some(Via::Contract));

        let expired = character_status(&paid, 1, 1000 + 24 * 3600, &rule);
        assert!(!expired.paid && expired.expires_at.is_none() && expired.via.is_none());
        assert!(!character_status(&paid, 2, 2000, &rule).paid);
    }
}
