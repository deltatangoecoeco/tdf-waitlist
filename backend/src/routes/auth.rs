
use std::net::IpAddr;

use branca::Branca;

use rocket::http::RawStr;
use rocket::request::{FromRequest, Outcome};
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket::{request, Request};
use serde::{Deserialize, Serialize};

use crate::app;
use crate::core::auth::{AuthenticatedAccount, AuthenticationError, CookieSetter};
use crate::core::esi::{ESIError, ESIScope, SEAT_LOGIN_SCOPES};
use crate::util::{madness::Madness, types};

pub struct RealIP(IpAddr);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for RealIP {
    type Error = std::convert::Infallible;
    async fn from_request(request: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        match request.client_ip() {
            Some(ip) => Outcome::Success(RealIP(ip)),
            None => Outcome::Forward(()),
        }
    }
}

#[derive(Serialize)]
struct WhoamiResponse {
    account_id: i64,
    access: Vec<&'static str>,
    characters: Vec<types::Character>,
}

#[get("/api/auth/whoami")]
async fn whoami(
    app: &rocket::State<app::Application>,
    account: AuthenticatedAccount,
) -> Result<Json<WhoamiResponse>, Madness> {
    let character = sqlx::query!(
        "SELECT id, name FROM \"character\" WHERE id = $1",
        account.id
    )
    .fetch_one(app.get_db())
    .await?;
    let mut characters = vec![types::Character {
        id: character.id,
        name: character.name,
        account_id: Some(account.id),
    }];

    let valid_token = app.esi_client.check(account.id).await;
    if !valid_token {
        return Err(Madness::ESIError(ESIError::NoToken));
    }

    let alts = sqlx::query!(
        "SELECT id, name FROM alt_character JOIN \"character\" ON alt_character.alt_id = \"character\".id WHERE account_id = $1",
        account.id
    )
    .fetch_all(app.get_db())
    .await?;

    for alt in alts {
        characters.push(types::Character {
            id: alt.id,
            name: alt.name,
            account_id: Some(account.id),
        });
    }

    let mut access_levels = Vec::new();
    for key in account.access {
        access_levels.push(key.as_str());
    }
    // Named SRP managers get the SRP wallet page without a council role
    let named: Vec<(i64, &str)> = characters.iter().map(|c| (c.id, c.name.as_str())).collect();
    if !account.access.contains("srp-manage")
        && crate::core::srp::named_manager(&app.config.srp.managers, &named)
    {
        access_levels.push("srp-manage");
    }

    Ok(Json(WhoamiResponse {
        account_id: account.id,
        access: access_levels,
        characters,
    }))
}

#[get("/api/auth/logout")]
async fn logout<'r>(
    app: &rocket::State<app::Application>,
    account: Option<AuthenticatedAccount>,
) -> Result<CookieSetter, Madness> {
    if let Some(account) = account {
        sqlx::query!(
            "DELETE FROM alt_character WHERE account_id = $1 OR alt_id = $2",
            account.id,
            account.id
        )
        .execute(app.get_db())
        .await?;
    }

    Ok(CookieSetter(
        "".to_string(),
        app.config.esi.url.starts_with("https:"),
       Some("/".to_owned())
    ))
}

#[get("/api/auth/login_url")]
fn login_url(/*fc: bool, client_ip: RealIP,*/ app: &rocket::State<app::Application>) -> String {
    // let ip_bytes = match client_ip.0 {
    //     IpAddr::V4(ip) => [
    //         ip.octets(),
    //         [ 0xff, 0xff, 0xff, 0xff ], [ 0xff, 0xff, 0xff, 0xff], [ 0xff, 0xff, 0xff, 0xff ],
    //     ].concat().try_into().unwrap(),
    //     IpAddr::V6(ip) => ip.octets(),
    // };

    let mut scopes = SEAT_LOGIN_SCOPES.to_vec();
    scopes.extend([
        // ESIScope::PublicData,
        ESIScope::Skills_ReadSkills_v1,
        ESIScope::Clones_ReadImplants_v1,

        ESIScope::Fleets_ReadFleet_v1,
        ESIScope::Fleets_WriteFleet_v1,
        ESIScope::UI_OpenWindow_v1,
    ]);
    // if fc {
    //     scopes.extend(vec![
    //         ESIScope::Fleets_ReadFleet_v1,
    //         ESIScope::Fleets_WriteFleet_v1,
    //         ESIScope::UI_OpenWindow_v1,
    //     ])
    // }

    authorize_url(app, &scopes, b"oauthhellostate")
}

/// The SeAT login URL. SeAT accepts a single redirect URI, so every login, including connecting
/// the SRP wallet, comes back through `callback`; `state` says which kind it was.
pub fn authorize_url(app: &app::Application, scopes: &[ESIScope], state: &[u8]) -> String {
    let mut branca = Branca::new(&app.token_secret).unwrap();
    let state = branca.encode(state).unwrap();

    let redirect_uri = RawStr::new(&app.config.esi.url);
    let redirect_uri_encoded = redirect_uri.percent_encode();

    format!(
        // "https://login.eveonline.com/v2/oauth/authorize?response_type=code&redirect_uri={}&client_id={}&scope={}&state={}",
        "https://seat.winterco.org/oauth/authorize?response_type=code&redirect_uri={}&client_id={}&scope={}&state={}",
        redirect_uri_encoded,
        app.config.esi.client_id,
        scopes.iter().fold(String::new(), |acc, scope| acc + " " + scope.as_str()).trim(),
        state
    )
}

// #[derive(Deserialize)]
// struct CallbackData<'r> {
//     code: &'r str,
//     state: Option<&'r str>,
// }

// One per login, so the Redirect variant's size doesn't matter, and Rocket can't respond with a
// Box<Redirect>
#[allow(clippy::large_enum_variant)]
#[derive(Responder)]
enum CallbackResponse {
    Login(CookieSetter),
    Redirect(Redirect),
}

#[get("/api/auth/cb?<code>&<state>")]
async fn callback(
    code: &str,
    state: Option<&str>,
    // input: Json<CallbackData<'_>>,
    // client_ip: RealIP, 
    app: &rocket::State<app::Application>,
    account_raw: Result<AuthenticatedAccount, AuthenticationError>,
) -> Result<CallbackResponse, Madness> {

    let branca = Branca::new(&app.token_secret).unwrap();

    let state = match state {
        Some(state) => state,
        None => return Err(Madness::BadRequest("No state provided".to_owned())),
    };

    let state_bytes = match branca.decode(state, 1800) {
        Ok(state) => state,
        Err(_) => return Err(Madness::BadRequest("Invalid state".to_owned())),
    };

    // Connecting the SRP wallet: not a login, and must not touch the admin's own tokens
    if let Some(character_id) = crate::core::srp::parse_connect_state(&state_bytes) {
        let redirect =
            super::srp::finish_connect(app, account_raw.ok(), code, character_id).await;
        return Ok(CallbackResponse::Redirect(redirect));
    }

    if state_bytes != b"oauthhellostate" {
        return Err(Madness::BadRequest("Invalid state".to_owned()));
    }

    let _account = match account_raw {
        Err(AuthenticationError::MissingCookie) => None,
        Err(AuthenticationError::InvalidToken) => None,
        Err(AuthenticationError::DatabaseError(e)) => return Err(e.into()),
        Ok(acc) => Some(acc),
    };

    let character_id = app
        .esi_client
        .process_authorization_code(code)
        .await?;

    Ok(CallbackResponse::Login(crate::core::auth::create_cookie(
        app,
        character_id,
        Some("/".to_owned()),
    )))
}

pub fn routes() -> Vec<rocket::Route> {
    routes![whoami, logout, login_url, callback,]
}
