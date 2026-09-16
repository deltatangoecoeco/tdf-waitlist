use chrono::TimeZone as _;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeSet}, sync::Arc};

struct ESIRawClient {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
}

pub struct ESIClient {
    db: Arc<crate::DB>,
    raw: ESIRawClient,
}

pub struct EsiErrorReason {
    pub error: String,
    pub details: String,
}

impl EsiErrorReason {
    // CCP returns a poorly structured error when fleet invites fail
    // so we need to provide special parsing logic to grab the error
    // message. If the POST error is related to anything else, we should
    // be able to grab the error property without additional logic.
    pub fn new(body: String) -> Self {
        let json: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(&body);

        match json {
            Ok(json) => {
                let body_value = json.get("error").unwrap().to_string();
                let parts: Vec<&str> = body_value.split(", ").collect();

                return EsiErrorReason {
                    error: parts[0].to_string().replace('"', ""),
                    details: "".to_string(), // We could grab the remaining JSON value but
                };                           // I don't know how to do it without the logic
            }                                // crashing due to index out of bounds
            Err(e) => {
                return EsiErrorReason {
                    error: "Failed to parse ESI error reason".to_string(),
                    details: e.to_string(),
                };
            }
        };
    }
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    // expires_in: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfo {
    id: i64,
    name: String,
    valid: bool
}

#[derive(Debug)]
pub struct AuthResult {
    pub character_id: i64,
    pub character_name: String,
    pub access_token: String,
    pub access_token_expiry: chrono::DateTime<chrono::Utc>,
    pub refresh_token: Option<String>,
    pub scopes: BTreeSet<String>,
    pub alt_ids: Option<Vec<AccountInfo>>,
    pub groups: Option<BTreeSet<String>>,
}

#[derive(thiserror::Error, Debug)]
pub enum ESIError {
    #[error("database error")]
    DatabaseError(#[from] sqlx::Error),
    #[error("ESI http error")]
    HTTPError(reqwest::Error),
    #[error("ESI returned {0}")]
    Status(u16),
    #[error("{1}")]
    WithMessage(u16, String),
    #[error("no ESI token found")]
    NoToken,
    #[error("missing ESI scope")]
    MissingScope,
    #[error("JWT decoding failure")]
    JWTError(#[from] jsonwebtoken::errors::Error),
    
}

#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum ESIScope {
    // PublicData,
    Fleets_ReadFleet_v1,
    Fleets_WriteFleet_v1,
    UI_OpenWindow_v1,
    Skills_ReadSkills_v1,
    Clones_ReadImplants_v1,

    CAS_OpenID,
    CAS_Accounts,

    CAS_Groups,
    CAS_Passthrough,
}

impl ESIScope {
    pub fn as_str(&self) -> &'static str {
        use ESIScope::*;
        match self {
            // PublicData => "publicData",
            Fleets_ReadFleet_v1 => "esi-fleets.read_fleet.v1",
            Fleets_WriteFleet_v1 => "esi-fleets.write_fleet.v1",
            UI_OpenWindow_v1 => "esi-ui.open_window.v1",
            Skills_ReadSkills_v1 => "esi-skills.read_skills.v1",
            Clones_ReadImplants_v1 => "esi-clones.read_implants.v1",

            CAS_OpenID => "openid",
            CAS_Accounts => "accounts",
            CAS_Groups => "groups",
            CAS_Passthrough => "passthrough",
        }
    }
}

impl From<reqwest::Error> for ESIError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_status() {
            return ESIError::Status(error.status().unwrap().as_u16());
        }
        ESIError::HTTPError(error)
    }
}

impl ESIRawClient {
    pub fn new(client_id: String, client_secret: String) -> ESIRawClient {

        // let mut buf = Vec::new();
        // File::open("/usr/local/share/ca-certificates/origin_ca_ecc_root.crt").unwrap().read_to_end(&mut buf).unwrap();

        // // create a certificate
        // let cert = reqwest::Certificate::from_pem(&buf).unwrap();

        ESIRawClient {
            http: reqwest::Client::builder()
                .user_agent("Waitlist (https://github.com/TvdW/tdf-waitlist)")
                // .add_root_certificate(cert)
                // .danger_accept_invalid_certs(true)
                .build()
                .unwrap(),
            client_id,
            client_secret,
        }
    }

    async fn process_oauth_token(
        &self,
        grant_type: &str,
        token: &str,
        // scopes: Option<&BTreeSet<String>>,
    ) -> Result<OAuthTokenResponse, ESIError> {
        #[derive(Serialize)]
        struct OAuthTokenRequest<'a> {
            grant_type: &'a str,
            refresh_token: Option<&'a str>,
            code: Option<&'a str>,
        }

        let request = OAuthTokenRequest {
            grant_type,
            refresh_token: match grant_type {
                "refresh_token" => Some(token),
                _ => None,
            },
            code: match grant_type {
                "refresh_token" => None,
                _ => Some(token),
            },
        };
        Ok(self
            .http
            .post("https://seat.winterco.org/oauth/token")
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&request)
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?)
    }

    async fn process_oauth_token_passthrough(
        &self,
        character_id: i64,
        token: &str,
        scopes: Option<&BTreeSet<String>>,
    ) -> Result<OAuthTokenResponse, ESIError> {
        #[derive(Serialize)]
        struct OAuthTokenRequest {
            scope: Option<String>,
        }

        // let scope_str = scopes
        //     .map(|s| s.into_iter().cloned().filter(|p| *p == "publicData" || p.starts_with("esi-")).collect() )
        //     .map(|s| join_scopes(s));

        let scope_str = match scopes {
            Some(s) => {
                let mut p_scopes = s.to_owned();
                p_scopes.retain(|s| s == "publicData" || s.starts_with("esi-"));
                Some(join_scopes(&p_scopes))
            }, 
            None => None,
        };

        let request = OAuthTokenRequest {
            scope: scope_str,
        };

        Ok(self
            .http
            .post(format!("https://seat.winterco.org/oauth/passthrough/{}", character_id))
            .bearer_auth(token)
            .form(&request)
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?)
    }

    async fn process_verify(
        &self,
        access_token: &str,
    ) -> Result<(i64, String, Vec<AccountInfo>, BTreeSet<String>, BTreeSet<String>, i64), jsonwebtoken::errors::Error> {
        // #[derive(Debug, Deserialize)]
        // struct VerifyResponse {
        //     #[serde(rename = "CharacterID")]
        //     character_id: i64,
        //     #[serde(rename = "CharacterName")]
        //     character_name: String,
        //     #[serde(rename = "Scopes")]
        //     scopes: String,
        // }

        // let result: VerifyResponse = self
        //     .get("https://login.eveonline.com/oauth/verify", access_token)
        //     .await?
        //     .json()
        //     .await?;
        // let scopes = split_scopes(&result.scopes);
        // Ok((result.character_id, result.character_name, scopes))

        #[derive(Debug, Serialize, Deserialize)]
        struct Claims {
            sub: String,
            uid: String,
            nam: String,
            acct: Vec<AccountInfo>,
            scp: BTreeSet<String>,
            groups: BTreeSet<String>,
            exp: i64,
        }

        let mut novalid = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        novalid.insecure_disable_signature_validation();

        let msg = jsonwebtoken::decode::<Claims>(
            &access_token,
            &jsonwebtoken::DecodingKey::from_secret(b""),
            &novalid,
        )?;

        let mut accounts = msg.claims.acct;
        accounts.retain(|v| v.valid);

        Ok((msg.claims.uid.parse::<i64>().unwrap(), msg.claims.nam, accounts, msg.claims.scp, msg.claims.groups, msg.claims.exp))
    }

    async fn process_verify_passthrough(
        &self,
        access_token: &str,
    ) -> Result<(i64, String, BTreeSet<String>, i64), jsonwebtoken::errors::Error> {
        #[derive(Debug, Serialize, Deserialize)]
        struct Claims {
            sub: String,
            name: String,
            scp: BTreeSet<String>,
            exp: i64
        }

        let mut novalid = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        novalid.insecure_disable_signature_validation();

        let msg = jsonwebtoken::decode::<Claims>(
            &access_token,
            &jsonwebtoken::DecodingKey::from_secret(b""),
            &novalid,
        )?;

        let character_id_string = msg.claims.sub.rsplit_once(":").unwrap().1;

        Ok((character_id_string.parse::<i64>().unwrap(), msg.claims.name, msg.claims.scp, msg.claims.exp))
    }

    pub async fn process_auth(
        &self,
        grant_type: &str,
        token: &str,
        required_scopes: Option<&BTreeSet<String>>,
    ) -> Result<AuthResult, ESIError> {
        let token = self.process_oauth_token(grant_type, token).await?;
        let (character_id, name, alts, scopes, groups, expires) = 
            self.process_verify(&token.id_token.unwrap()).await?;
        if required_scopes.is_some() {
            let required_scopes = required_scopes.unwrap();
            if !scopes.is_superset(&required_scopes) {
                return Err(ESIError::MissingScope);
            }
        }
        Ok(AuthResult {
            character_id,
            character_name: name,
            access_token: token.access_token,
            access_token_expiry: chrono::Utc.timestamp_opt(expires, 0)
                .single()
                .map(|t| t - chrono::Duration::seconds(60))
                .unwrap_or_else(|| chrono::Utc::now()), // Fallback to now if timestamp is invalid
            refresh_token: token.refresh_token,
            scopes,
            alt_ids: Some(alts),
            groups: Some(groups)
        })
    }

    pub async fn process_auth_passthrough(
        &self,
        character_id: i64,
        token: &str,
        scopes: Option<&BTreeSet<String>>,
    ) -> Result<AuthResult, ESIError> {
        let token = self.process_oauth_token_passthrough(character_id, token, scopes).await?;
        let (character_id, name, scopes, expires) = self.process_verify_passthrough(&token.access_token).await?;
        Ok(AuthResult {
            character_id,
            character_name: name,
            access_token: token.access_token,
            access_token_expiry: chrono::Utc.timestamp_opt(expires, 0)
                .single()
                .map(|t| t - chrono::Duration::seconds(60))
                .unwrap_or_else(|| chrono::Utc::now()),
            refresh_token: None,
            scopes,
            alt_ids: None,
            groups: None,
        })
    }

    pub async fn get(&self, url: &str, access_token: Option<String>) -> Result<reqwest::Response, ESIError> {
        match access_token {
            Some(token) => 
                Ok(self
                    .http
                    .get(url)
                    .bearer_auth(&token)
                    .send()
                    .await?
                    .error_for_status()?),
            None =>
                Ok(self
                    .http
                    .get(url)
                    .send()
                    .await?
                    .error_for_status()?)
        }
    }

    pub async fn delete(
        &self,
        url: &str,
        access_token: Option<String>,
    ) -> Result<reqwest::Response, ESIError> {

        match access_token {
            Some(token) => 
                Ok(self
                    .http
                    .delete(url)
                    .bearer_auth(&token)
                    .send()
                    .await?
                    .error_for_status()?),
            None =>
                Ok(self
                    .http
                    .delete(url)
                    .send()
                    .await?
                    .error_for_status()?)
        }
    }

    pub async fn post<E: Serialize + ?Sized>(
        &self,
        url: &str,
        input: &E,
        access_token: Option<String>,
    ) -> Result<reqwest::Response, ESIError> {

        let response = match access_token {
            Some(token) => 
                self
                    .http
                    .post(url)
                    .json(input)
                    .bearer_auth(&token)
                    .send()
                    .await?,
            None => 
                self
                    .http
                    .post(url)
                    .json(input)
                    .send()
                    .await?,
        };

        if let Err(err) = response.error_for_status_ref() {
            let response_body = response.text().await?;
            let payload: EsiErrorReason = EsiErrorReason::new(response_body);
            return Err(ESIError::WithMessage(
                err.status().unwrap().as_u16(),
                payload.error,
            ));
        };

        Ok(response)
    }
}

impl ESIClient {
    pub fn new(database: Arc<crate::DB>, client_id: String, client_secret: String) -> ESIClient {
        ESIClient {
            db: database,
            raw: ESIRawClient::new(client_id, client_secret),
        }
    }

    pub async fn process_authorization_code(&self, code: &str) -> Result<i64, ESIError> {
        let required_scopes = vec![
            ESIScope::CAS_OpenID,
            ESIScope::CAS_Accounts,
            ESIScope::CAS_Groups,
            ESIScope::CAS_Passthrough,
            // ESIScope::PublicData,
            ESIScope::Skills_ReadSkills_v1,
            ESIScope::Clones_ReadImplants_v1,

            ESIScope::Fleets_ReadFleet_v1,
            ESIScope::Fleets_WriteFleet_v1,
            ESIScope::UI_OpenWindow_v1,
        ];

        let required_scopes_str: BTreeSet<String> = required_scopes.iter().map(|s| s.as_str().to_owned()).collect();

        let result = self
            .raw
            .process_auth("authorization_code", code, Some(&required_scopes_str))
            .await?;

        // if let Some(previous_token) = sqlx::query!(
        //     "SELECT * FROM refresh_token WHERE character_id = $1",
        //     result.character_id
        // )
        // .fetch_optional(self.db.as_ref())
        // .await?
        // {
        //     let mut merged_scopes = result.scopes.clone();
        //     for extra_scope in split_scopes(&previous_token.scopes) {
        //         merged_scopes.insert(extra_scope);
        //     }

        //     let second_attempt = match self
        //         .raw
        //         .process_auth("refresh_token", &result.refresh_token, Some(&merged_scopes))
        //         .await
        //     {
        //         Ok(r) => r,
        //         Err(ESIError::Status(400)) => result,
        //         Err(e) => return Err(e),
        //     };

        //     result = second_attempt;
        // }

        self.save_auth(&result).await?;
        Ok(result.character_id)
    }

    async fn save_auth(&self, auth: &super::esi::AuthResult) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;

        
        for character in auth.alt_ids.as_ref().unwrap() {
            sqlx::query!(
                "DELETE FROM alt_character WHERE account_id = $1 OR alt_id = $1",
                character.id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "INSERT INTO \"character\" (id, name) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET name = $2",
                character.id,
                character.name
            )
            .execute(&mut *tx)
            .await?;

        }

        for character in auth.alt_ids.as_ref().unwrap() {

            if auth.character_id != character.id {
                sqlx::query!(
                    "INSERT INTO alt_character (account_id, alt_id) VALUES ($1, $2)",
                    auth.character_id,
                    character.id,
                )
                .execute(&mut *tx)
                .await?;

                sqlx::query!(
                    "DELETE FROM admin WHERE character_id = $1",
                    character.id,
                )
                .execute(&mut *tx)
                .await?;
            }

        }

        sqlx::query!(
            "DELETE FROM alt_character WHERE account_id = $1 AND alt_id NOT IN (SELECT unnest($2::bigint[]))",
            auth.character_id,
            &auth.alt_ids.as_ref().unwrap().iter().map(|c| c.id).collect::<Vec<_>>(),
        )
        .execute(&mut *tx)
        .await?;

        macro_rules! maprole {
            ($groups:ident, $group:expr, $role:expr) => (
                if $groups.contains($group) {
                    return Some($role);
                }
            );
        }

        match &auth.groups {
            Some(groups) => {
                let role = (|g: &BTreeSet<String>| {
                    maprole!(g, "Superuser", "admin");
                    maprole!(g, "IT", "admin");
                    maprole!(g, "Exec", "council");
                    maprole!(g, "T2-FC", "fc");
                    maprole!(g, "[AUTO] T2-FC", "fc");
                    maprole!(g, "Junior-FC", "fc");
                    maprole!(g, "[AUTO] Junior-FC", "fc");
                    maprole!(g, "[AUTO] T1-FC", "fc");
                    maprole!(g, "[AUTO] Waitlist FC", "fc");
                    None
                })(groups);

                match role {
                    Some(role) => {
                        sqlx::query!(
                            "INSERT INTO admin (character_id, role, granted_at) VALUES ($1, $2, $3) ON CONFLICT (character_id) DO UPDATE SET role = $2, granted_at = $3",
                            auth.character_id,
                            role,
                            sqlx::types::chrono::Utc::now().naive_utc().and_utc().timestamp()
                        )
                        .execute(&mut *tx)
                        .await?;
                    },
                    None => {
                        sqlx::query!(
                            "DELETE FROM admin WHERE character_id = $1",
                            auth.character_id,
                        )
                        .execute(&mut *tx)
                        .await?;
                    }
                }
            },
            None => {
                sqlx::query!(
                    "DELETE FROM admin WHERE character_id = $1",
                    auth.character_id,
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        let expiry_timestamp = auth.access_token_expiry.timestamp();
        let scopes = join_scopes(&auth.scopes);
        sqlx::query!(
            "INSERT INTO access_token (character_id, access_token, expires, scopes) VALUES ($1, $2, $3, $4) ON CONFLICT (character_id) DO UPDATE SET access_token = $2, expires = $3, scopes = $4",
            auth.character_id,
            auth.access_token,
            expiry_timestamp,
            scopes,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "INSERT INTO refresh_token (character_id, refresh_token, scopes) VALUES ($1, $2, $3) ON CONFLICT (character_id) DO UPDATE SET refresh_token = $2, scopes = $3",
            auth.character_id,
            auth.refresh_token.as_ref().unwrap(),
            scopes,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn access_token_idp(
        &self,
        character_id: i64,
    ) -> Result<(String, BTreeSet<String>), ESIError> {
        let main_character_id = {
            if let Some(record) = sqlx::query!(
                "SELECT account_id FROM alt_character WHERE alt_id = $1",
                character_id
            )
            .fetch_optional(self.db.as_ref())
            .await?
            {
                record.account_id
            } else {
                character_id
            }
        };

        if let Some(record) = sqlx::query!(
            "SELECT * FROM access_token WHERE character_id = $1",
            character_id
        )
        .fetch_optional(self.db.as_ref())
        .await?
        {
            if record.expires >= chrono::Utc::now().timestamp() {
                return Ok((record.access_token, split_scopes(&record.scopes)));
            }
        }

        let refresh = match sqlx::query!(
            "SELECT * FROM refresh_token WHERE character_id = $1",
            main_character_id
        )
        .fetch_optional(self.db.as_ref())
        .await?
        {
            Some(r) => r,
            None => return Err(ESIError::NoToken),
        };

        let refresh_scopes = split_scopes(&refresh.scopes);
        let refreshed = match self
            .raw
            .process_auth(
                "refresh_token",
                &refresh.refresh_token,
                Some(&refresh_scopes),
            )
            .await
        {
            Ok(r) => r,
            Err(ESIError::Status(400)) => {
                self.clear_access_tokens(main_character_id).await?;
                return Err(ESIError::NoToken);
            }
            Err(e) => return Err(e),
        };
        self.save_auth(&refreshed).await?;
        Ok((refreshed.access_token, refresh_scopes))
    }

    async fn access_token_raw(
        &self,
        character_id: i64,
    ) -> Result<(String, BTreeSet<String>), ESIError> {
        if let Some(record) = sqlx::query!(
            "SELECT * FROM access_token_esi WHERE character_id = $1",
            character_id
        )
        .fetch_optional(self.db.as_ref())
        .await?
        {
            if record.expires >= chrono::Utc::now().timestamp() {
                return Ok((record.access_token, split_scopes(&record.scopes)));
            }
        }

        let (access_token, refresh_scopes) = self
            .access_token_idp(character_id)
            .await?;

        let passthroughed = match self
            .raw
            .process_auth_passthrough(
                character_id, 
                &access_token, 
                Some(&refresh_scopes)
            )
            .await 
        {
            Ok(r) => {
                let expiry_timestamp = r.access_token_expiry.timestamp();
                let scopes = join_scopes(&r.scopes);
                sqlx::query!(
                    "INSERT INTO access_token_esi (character_id, access_token, expires, scopes) VALUES ($1, $2, $3, $4) ON CONFLICT (character_id) DO UPDATE SET access_token = $2, expires = $3, scopes = $4",
                    r.character_id,
                    r.access_token,
                    expiry_timestamp,
                    scopes,
                )
                .execute(self.db.as_ref())
                .await?;
                r
            },
            Err(ESIError::Status(400)) => {
                // self.clear_access_tokens(main_character_id).await?;
                return Err(ESIError::NoToken);
            }
            Err(e) => return Err(e),
        };

        Ok((passthroughed.access_token, passthroughed.scopes))
    }

    async fn clear_access_tokens(&self, account_id: i64) -> Result<(), ESIError> {
        warn!(
            "Deleting refresh token for character {} as it failed to be used: HTTP 400",
            account_id
        );
        let mut tx = self.db.begin().await?;
        sqlx::query!(
            "DELETE FROM access_token WHERE character_id = $1",
            account_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM access_token_esi WHERE character_id = $1",
            account_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM refresh_token WHERE character_id = $1",
            account_id
        )
        .execute(&mut *tx)
        .await?;

        let alts = sqlx::query!(
            "SELECT alt_id FROM alt_character WHERE account_id = $1",
            account_id
        ).fetch_all(&mut *tx)
        .await?;

        for alt in alts {
            sqlx::query!(
                "DELETE FROM access_token_esi WHERE character_id = $1",
                alt.alt_id
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    async fn access_token(&self, character_id: i64, scope: ESIScope) -> Result<String, ESIError> {
        let (token, scopes) = self.access_token_raw(character_id).await?;

        if !scopes.contains(scope.as_str()) {
            return Err(ESIError::MissingScope);
        }

        Ok(token)
    }

    pub async fn get<D: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        character_id: i64,
        scope: Option<ESIScope>,
    ) -> Result<D, ESIError> {
        let access_token = match scope {
            Some(scope) => Some(self.access_token(character_id, scope).await?),
            None => None,
        };
        let url = format!("https://esi.evetech.net{}", path);
        Ok(self.raw.get(&url, access_token).await?.json().await?)
    }

    pub async fn delete(
        &self,
        path: &str,
        character_id: i64,
        scope: Option<ESIScope>,
    ) -> Result<(), ESIError> {
        let access_token = match scope {
            Some(scope) => Some(self.access_token(character_id, scope).await?),
            None => None,
        };
        let url = format!("https://esi.evetech.net{}", path);
        self.raw.delete(&url, access_token).await?;
        Ok(())
    }

    pub async fn post<E: Serialize + ?Sized>(
        &self,
        path: &str,
        input: &E,
        character_id: i64,
        scope: Option<ESIScope>,
    ) -> Result<(), ESIError> {
        let access_token = match scope {
            Some(scope) => Some(self.access_token(character_id, scope).await?),
            None => None,
        };
        let url = format!("https://esi.evetech.net{}", path);
        self.raw.post::<E>(&url, input, access_token).await?;
        Ok(())
    }

    pub async fn check(&self, account_id: i64) -> bool {
        let result = self.access_token_idp(account_id).await;
        match result {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

pub mod fleet_members {
    use eve_data_core::TypeID;

    use crate::core::esi::ESIScope;

    use super::{ESIClient, ESIError};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct ESIFleetMember {
        pub character_id: i64,
        pub ship_type_id: TypeID,
        pub squad_id: i64,
    }

    pub async fn get(
        client: &ESIClient,
        fleet_id: i64,
        boss_id: i64,
    ) -> Result<Vec<ESIFleetMember>, ESIError> {
        Ok(client
            .get(
                &format!("/v1/fleets/{}/members", fleet_id),
                boss_id,
                Some(ESIScope::Fleets_ReadFleet_v1),
            )
            .await?)
    }
}

fn split_scopes(input: &str) -> BTreeSet<String> {
    input
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn join_scopes(input: &BTreeSet<String>) -> String {
    input.iter().fold(String::new(), |a, b| a + b + " ")
}
