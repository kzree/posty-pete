use serde::{Deserialize, Serialize};

use super::accent::Accent;

/// The user's settings, grouped like the Settings pages. Missing fields load
/// as defaults and unknown ones are ignored, so older and newer files load.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: AppearanceSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: ThemePreference,
    pub accent: Accent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    #[default]
    Dark,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_round_trips() {
        let mut settings = Settings::default();
        settings.appearance.accent = Accent::Green;
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""accent":"green""#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn missing_accent_loads_as_monochrome() {
        let settings: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"light"}}"#).unwrap();
        assert_eq!(settings.appearance.theme, ThemePreference::Light);
        assert_eq!(settings.appearance.accent, Accent::Monochrome);
    }
}
