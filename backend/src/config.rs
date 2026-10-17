use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub idle_timeout: u64,
    pub connect_timeout: u64,
}

#[derive(Deserialize, Clone)]
pub struct ESIConfig {
    pub client_id: String,
    pub client_secret: String,
    pub url: String,
}

#[derive(Deserialize, Clone)]
pub struct AppConfig {
    pub token_secret: String,
}

#[derive(Deserialize, Clone)]
pub struct SSEConfig {
    pub url: String,
    pub secret: String,
}

#[derive(Deserialize, Clone)]
pub struct FleetUpdaterConfig {
    pub enable: bool,
    pub min_in_fleet: usize,
}

#[derive(Deserialize, Clone)]
pub struct SkillUpdaterConfig {
    pub enable: bool,
    pub runtime: f64,
}

// Optional: a config without an [srp] section leaves the SRP check switched off
#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct SrpConfig {
    pub enable: bool,
    /// Characters (main or alt) whose accounts may run the SRP wallet page, on top of council
    /// and admin. Lets whoever owns the wallet character connect it without a SeAT Exec or IT
    /// role. Each is a character id (survives renames) or an exact in-game name (case-insensitive).
    pub managers: Vec<String>,
    /// Wallet division that receives SRP payments (1 is the master wallet)
    pub division: i32,
    /// ISK; this much or more counts
    pub min_amount: i64,
    /// How long a payment covers the character who made it
    pub coverage_hours: i64,
}

impl SrpConfig {
    /// Catches values that would quietly show everyone as unpaid
    pub fn validate(&self) -> Result<(), String> {
        if !self.enable {
            return Ok(());
        }
        if !(1..=7).contains(&self.division) {
            return Err(format!("[srp] division must be 1 to 7, not {}", self.division));
        }
        if self.min_amount <= 0 {
            return Err(format!("[srp] min_amount must be above 0, not {}", self.min_amount));
        }
        if !(1..=MAX_COVERAGE_HOURS).contains(&self.coverage_hours) {
            return Err(format!(
                "[srp] coverage_hours must be 1 to {}, not {}",
                MAX_COVERAGE_HOURS, self.coverage_hours
            ));
        }
        Ok(())
    }
}

/// A month: far beyond any real SRP rule, and keeps the seconds arithmetic well within range
const MAX_COVERAGE_HOURS: i64 = 24 * 31;

impl Default for SrpConfig {
    fn default() -> Self {
        SrpConfig {
            enable: false,
            managers: Vec::new(),
            division: 1,
            min_amount: 50_000_000,
            coverage_hours: 24,
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub app: AppConfig,
    pub esi: ESIConfig,
    pub sse: SSEConfig,
    pub fleet_updater: FleetUpdaterConfig,
    pub skill_updater: SkillUpdaterConfig,
    #[serde(default)]
    pub srp: SrpConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srp_validation() {
        let valid = SrpConfig { enable: true, ..SrpConfig::default() };
        assert!(valid.validate().is_ok());
        assert!(SrpConfig { coverage_hours: 0, ..valid.clone() }.validate().is_err());
        assert!(SrpConfig { coverage_hours: MAX_COVERAGE_HOURS + 1, ..valid.clone() }
            .validate()
            .is_err());
        assert!(SrpConfig { min_amount: 0, ..valid.clone() }.validate().is_err());
        assert!(SrpConfig { division: 8, ..valid.clone() }.validate().is_err());
        // Not checked while switched off
        assert!(SrpConfig { enable: false, division: 0, ..valid }.validate().is_ok());
    }

    #[test]
    fn srp_section_is_optional() {
        // A Windows checkout has CRLF line endings
        let example = include_str!("../config.example.toml").replace("\r\n", "\n");
        let config: Config = toml::from_str(&example).unwrap();
        assert!(!config.srp.enable);

        let enabled = example.replace("# [srp]\n# enable = true", "[srp]\nenable = true");
        let config: Config = toml::from_str(&enabled).unwrap();
        assert!(config.srp.enable);
        assert!(config.srp.managers.is_empty());
        assert!(config.srp.validate().is_ok());
    }
}
