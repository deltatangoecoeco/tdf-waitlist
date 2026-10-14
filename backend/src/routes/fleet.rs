use std::collections::{HashMap, BTreeSet};

use crate::{
    app::Application,
    core::{
        auth::{authorize_character, AuthenticatedAccount},
        esi::{ESIError, ESIScope},
    },
    util::{
        madness::Madness,
        types::{Character, Hull},
    },
};
use eve_data_core::TypeDB;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct FleetStatusFleet {
    id: i64,
    boss: Character,
}

#[derive(Debug, Serialize)]
struct FleetStatusResponse {
    fleets: Vec<FleetStatusFleet>,
}

#[get("/api/fleet/status")]
async fn fleet_status(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
) -> Result<Json<FleetStatusResponse>, Madness> {
    account.require_access("fleet-view")?;

    let fleets = sqlx::query!(
        "
        SELECT fleet.id, boss_id, name, COALESCE(\"alt_character\".account_id, boss_id) AS account_id
        FROM fleet 
        JOIN \"character\" ON fleet.boss_id = \"character\".id 
        LEFT JOIN \"alt_character\" ON fleet.boss_id = \"alt_character\".alt_id 
        WHERE fleet.is_updating = true"
    ).fetch_all(app.get_db()).await?.into_iter()
    .map(|fleet| FleetStatusFleet{
        id: fleet.id.unwrap(),
        boss: Character{
            id: fleet.boss_id.unwrap(),
            name: fleet.name.unwrap(),
            account_id: fleet.account_id,
        }
    }).collect();

    Ok(Json(FleetStatusResponse { fleets }))
}

#[derive(Debug, Deserialize)]
struct CharacterFleet {
    fleet_id: i64,
    // The current ESI spec makes this required, but it is optional so that callers which only
    // want the fleet id keep working if ESI omits it: unknown means "not boss".
    fleet_boss_id: Option<i64>,
}

async fn get_character_fleet(
    app: &rocket::State<Application>,
    character_id: i64,
) -> Result<CharacterFleet, Madness> {
    let basic_info = app
        .esi_client
        .get(
            &format!("/v1/characters/{}/fleet", character_id),
            character_id,
            Some(ESIScope::Fleets_ReadFleet_v1),
        )
        .await;
    if let Err(whatswrong) = basic_info {
        match whatswrong {
            ESIError::Status(404) => return Err(Madness::NotFound("You are not in a fleet")),
            e => return Err(e.into()),
        };
    }
    let basic_info: CharacterFleet = basic_info.unwrap();
    Ok(basic_info)
}

async fn get_current_fleet_id(
    app: &rocket::State<Application>,
    character_id: i64,
) -> Result<i64, Madness> {
    Ok(get_character_fleet(app, character_id).await?.fleet_id)
}

#[derive(Debug, Serialize)]
struct MyFleetResponse {
    fleet_id: i64,
    is_boss: bool,
}

#[get("/api/fleet/me?<character_id>")]
async fn my_fleet(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    character_id: i64,
) -> Result<Json<MyFleetResponse>, Madness> {
    account.require_access("fleet-configure")?;
    authorize_character(app.get_db(), &account, character_id, None).await?;

    let fleet = get_character_fleet(app, character_id).await?;

    Ok(Json(MyFleetResponse {
        fleet_id: fleet.fleet_id,
        is_boss: fleet.fleet_boss_id == Some(character_id),
    }))
}

#[derive(Debug, Serialize)]
struct FleetInfoResponse {
    fleet_id: i64,
    wings: Vec<FleetInfoWing>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FleetInfoWing {
    id: i64,
    name: String,
    squads: Vec<FleetInfoSquad>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FleetInfoSquad {
    id: i64,
    name: String,
}

#[get("/api/fleet/info?<character_id>")]
async fn fleet_info(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    character_id: i64,
) -> Result<Json<FleetInfoResponse>, Madness> {
    account.require_access("fleet-view")?;
    authorize_character(app.get_db(), &account, character_id, None).await?;

    let fleet_id = get_current_fleet_id(app, character_id).await?;

    let wings = app
        .esi_client
        .get(
            &format!("/v1/fleets/{}/wings", fleet_id),
            character_id,
            Some(ESIScope::Fleets_ReadFleet_v1),
        )
        .await;
    if let Err(whatswrong) = wings {
        match whatswrong {
            ESIError::Status(404) => return Err(Madness::NotFound("You are not the fleet boss")),
            e => return Err(e.into()),
        };
    }
    let wings: Vec<FleetInfoWing> = wings?;

    Ok(Json(FleetInfoResponse { fleet_id, wings }))
}

#[derive(Debug, Serialize)]
struct FleetMembersResponse {
    members: Vec<FleetMembersMember>,
}

#[derive(Debug, Serialize)]
struct FleetMembersMember {
    id: i64,
    name: Option<String>,
    account_id: Option<i64>,
    account_name: Option<String>,
    ship: Hull,
    wl_category: Option<String>,
}

#[get("/api/fleet/members?<character_id>")]
async fn fleet_members(
    account: AuthenticatedAccount,
    character_id: i64,
    app: &rocket::State<Application>,
) -> Result<Json<FleetMembersResponse>, Madness> {
    account.require_access("fleet-view")?;
    authorize_character(app.get_db(), &account, character_id, None).await?;

    let fleet_id = get_current_fleet_id(app, character_id).await?;
    let fleet = match sqlx::query!("SELECT boss_id FROM fleet WHERE id = $1 AND is_updating = true", fleet_id)
        .fetch_optional(app.get_db())
        .await?
    {
        Some(fleet) => fleet,
        None => return Err(Madness::NotFound("Fleet not configured")),
    };

    let in_fleet = match crate::core::esi::fleet_members::get(&app.esi_client, fleet_id, fleet.boss_id).await {
        Ok(members) => members,
        Err(e) => match e {
            ESIError::Status(404) => {
                sqlx::query!("UPDATE fleet SET is_updating = false WHERE id = $1", fleet_id)
                    .execute(app.get_db())
                    .await?;
                return Err(e.into());
            },
            _ => return Err(e.into()),
        },
    };
    let character_ids: Vec<_> = in_fleet.iter().map(|member| member.character_id).collect();
    let mut characters = crate::data::character::lookup(app.get_db(), &character_ids).await?;

    let account_id_map: HashMap<i64, i64> = characters.iter()
        .filter(|&(_, character)| character.account_id.is_some())
        .map(|(&id, character)| (id, character.account_id.clone().unwrap()))
        .collect();

    let account_ids = account_id_map.values().cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();

    let account_names = crate::data::character::lookup(app.get_db(), &account_ids).await?;

    let category_lookup: HashMap<_, _> = crate::data::categories::categories()
        .iter()
        .map(|c| (&c.id as &str, &c.name))
        .collect();

    let squads: HashMap<i64, String> = sqlx::query!(
        "SELECT squad_id, category FROM fleet_squad WHERE fleet_id = $1",
        fleet_id
    )
    .fetch_all(app.get_db())
    .await?
    .into_iter()
    .map(|squad| (squad.squad_id, squad.category))
    .collect();

    Ok(Json(FleetMembersResponse {
        members: in_fleet
            .into_iter()
            .map(|member| {
                let character = characters.remove(&member.character_id);
                let (name, account_id) = match character {
                    Some(character) => (Some(character.name), character.account_id),
                    None => (None, None),
                };
                let account_name = match account_id {
                    Some(account_id) => account_names.get(&account_id).map(|character| character.name.clone()),
                    None => None,
                };
                FleetMembersMember {
                    id: member.character_id,
                    name: name,
                    account_id: account_id,
                    account_name: account_name,
                    ship: Hull {
                        id: member.ship_type_id,
                        name: TypeDB::name_of(member.ship_type_id).unwrap(),
                    },
                    wl_category: squads
                        .get(&member.squad_id)
                        .and_then(|s| category_lookup.get(s.as_str()))
                        .map(|s| s.to_string()),
                }
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    character_id: i64,
    fleet_id: i64,
    assignments: HashMap<String, (i64, i64)>,
}

#[post("/api/fleet/register", data = "<input>")]
async fn register_fleet(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    input: Json<RegisterRequest>,
) -> Result<&'static str, Madness> {
    account.require_access("fleet-configure")?;
    authorize_character(app.get_db(), &account, input.character_id, None).await?;

    let mut tx = app.get_db().begin().await?;
    sqlx::query!("DELETE FROM fleet_squad WHERE fleet_id = $1", input.fleet_id)
        .execute(&mut tx)
        .await?;
    sqlx::query!(
        "INSERT INTO fleet (id, boss_id, is_updating) VALUES ($1, $2, true) ON CONFLICT (id) DO UPDATE SET boss_id = $2, is_updating = true",
        input.fleet_id,
        input.character_id
    )
    .execute(&mut tx)
    .await?;

    for category in crate::data::categories::categories() {
        if let Some((wing_id, squad_id)) = input.assignments.get(&category.id) {
            sqlx::query!("INSERT INTO fleet_squad (fleet_id, wing_id, squad_id, category) VALUES ($1, $2, $3, $4)",
            input.fleet_id, wing_id, squad_id, category.id).execute(&mut tx).await?;
        } else {
            return Err(Madness::BadRequest(format!(
                "Missing assignment for {}",
                category.name
            )));
        }
    }

    tx.commit().await?;

    Ok("OK")
}

#[derive(Debug, Deserialize)]
struct FleetCloseRequest {
    character_id: i64,
}
#[post("/api/fleet/close", data = "<input>")]
async fn close_fleet(
    app: &rocket::State<Application>,
    account: AuthenticatedAccount,
    input: Json<FleetCloseRequest>,
) -> Result<&'static str, Madness> {
    authorize_character(app.get_db(), &account, input.character_id, None).await?;
    account.require_access("fleet-configure")?;

    let fleet_id = get_current_fleet_id(app, input.character_id).await?;

    let in_fleet =
        crate::core::esi::fleet_members::get(&app.esi_client, fleet_id, input.character_id).await?;

    for member in in_fleet {
        if member.character_id == input.character_id {
            continue;
        }

        app.esi_client
            .delete(
                &format!("/v1/fleets/{}/members/{}/", fleet_id, member.character_id),
                input.character_id,
                Some(ESIScope::Fleets_WriteFleet_v1),
            )
            .await?;
    }

    Ok("OK")
}

pub fn routes() -> Vec<rocket::Route> {
    routes![
        fleet_status,
        fleet_info,
        close_fleet,
        fleet_members,
        register_fleet,
        my_fleet
    ]
}
