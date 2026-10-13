use crate::engine::Settings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub name: String,
    pub theme: String,
    pub mode: String,
    pub settings: Settings,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Meu preset".into(),
            theme: "dark".into(),
            mode: "basic".into(),
            settings: Settings::default(),
        }
    }
}
impl Config {
    pub fn validated(mut self) -> Result<Self, String> {
        if self.version != 1
            || !["light", "dark"].contains(&self.theme.as_str())
            || !["basic", "advanced"].contains(&self.mode.as_str())
            || self.name.trim().is_empty()
            || self.name.chars().count() > 80
        {
            return Err("Arquivo de configuração incompatível ou inválido.".into());
        }
        self.settings.validate_options()?;
        self.settings.media_id.clear();
        self.settings.start = 0.0;
        self.settings.preview = false;
        self.settings.compress_only = false;
        Ok(self)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_invalid_values() {
        let config = Config::default();
        let restored: Config =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert!(restored.validated().is_ok());
        let mut bad = Config::default();
        bad.settings.search_radius = 999;
        assert!(bad.validated().is_err());
        let bad = Config {
            version: 2,
            ..Config::default()
        };
        assert!(bad.validated().is_err());
    }
    #[test]
    fn advanced_limits_and_legacy_migration() {
        let legacy: Config =
            serde_json::from_str(include_str!("../examples/gameplay-60.luma.json")).unwrap();
        assert_eq!(legacy.settings.gamma, 2.2);
        assert!(legacy.validated().is_ok());
        let mut invalid = Config::default();
        invalid.settings.gamma = f32::NAN;
        assert!(invalid.validated().is_err());
        let mut invalid = Config::default();
        invalid.settings.gaussian_sigma = 0.0;
        assert!(invalid.validated().is_err());
    }
    #[test]
    fn media_identity_is_never_saved() {
        let mut config = Config::default();
        config.settings.media_id = "private".into();
        config.settings.start = 25.0;
        config.settings.compress_only = true;
        let clean = config.validated().unwrap();
        assert_eq!(clean.settings.media_id, "");
        assert_eq!(clean.settings.start, 0.0);
        assert!(!clean.settings.compress_only);
    }
}
