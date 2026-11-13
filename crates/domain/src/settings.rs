use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::accent::Accent;

/// The user's settings, grouped like the Settings pages. Missing fields load
/// as defaults and unknown ones are ignored, so older and newer files load.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub requests: RequestSettings,
    pub history: HistorySettings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Read at startup only.
    pub restore_last_session: bool,
    pub ask_before_discarding: bool,
    pub open_duplicated_request: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            restore_last_session: true,
            ask_before_discarding: true,
            open_duplicated_request: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: ThemePreference,
    pub accent: Accent,
    pub ui_font_size: u32,
    /// Empty means the platform's default monospace font.
    pub editor_font_family: String,
    pub editor_font_size: u32,
    pub wrap_responses: bool,
}

impl AppearanceSettings {
    pub const DEFAULT_UI_FONT_SIZE: u32 = 16;
    pub const UI_FONT_SIZE: RangeInclusive<u32> = 10..=24;
    pub const DEFAULT_EDITOR_FONT_SIZE: u32 = 13;
    pub const EDITOR_FONT_SIZE: RangeInclusive<u32> = 8..=32;

    /// The interface font size, in range even if the file says otherwise.
    pub fn ui_font_size(&self) -> u32 {
        clamp(self.ui_font_size, &Self::UI_FONT_SIZE)
    }

    pub fn editor_font_size(&self) -> u32 {
        clamp(self.editor_font_size, &Self::EDITOR_FONT_SIZE)
    }

    /// The chosen editor font, or `None` for the default.
    pub fn editor_font_family(&self) -> Option<&str> {
        Some(self.editor_font_family.trim()).filter(|family| !family.is_empty())
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            accent: Accent::default(),
            ui_font_size: Self::DEFAULT_UI_FONT_SIZE,
            editor_font_family: String::new(),
            editor_font_size: Self::DEFAULT_EDITOR_FONT_SIZE,
            wrap_responses: true,
        }
    }
}

fn clamp(value: u32, range: &RangeInclusive<u32>) -> u32 {
    value.clamp(*range.start(), *range.end())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    #[default]
    Dark,
}

/// How requests are sent. Every request shares them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestSettings {
    /// 0 means no timeout.
    pub timeout_secs: u32,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub verify_tls: bool,
    pub proxy: ProxyMode,
    /// Used when `proxy` is [`ProxyMode::Custom`], and kept otherwise.
    pub proxy_url: String,
    /// Empty means none.
    pub user_agent: String,
}

impl RequestSettings {
    pub const DEFAULT_TIMEOUT_SECS: u32 = 30;
    pub const TIMEOUT_SECS: RangeInclusive<u32> = 0..=3600;
    pub const DEFAULT_MAX_REDIRECTS: u32 = 10;
    pub const MAX_REDIRECTS: RangeInclusive<u32> = 1..=50;
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_secs: Self::DEFAULT_TIMEOUT_SECS,
            follow_redirects: true,
            max_redirects: Self::DEFAULT_MAX_REDIRECTS,
            verify_tls: true,
            proxy: ProxyMode::default(),
            proxy_url: String::new(),
            user_agent: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    None,
    /// The proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`,
    /// `ALL_PROXY`, `NO_PROXY`), not the OS network settings.
    #[default]
    System,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    pub retention_days: u32,
}

impl HistorySettings {
    pub const DEFAULT_RETENTION_DAYS: u32 = 14;
    pub const RETENTION_DAYS: RangeInclusive<u32> = 1..=365;

    pub fn retention_days(&self) -> u32 {
        clamp(self.retention_days, &Self::RETENTION_DAYS)
    }
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            retention_days: Self::DEFAULT_RETENTION_DAYS,
        }
    }
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

    #[test]
    fn missing_general_loads_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"light"}}"#).unwrap();
        assert!(settings.general.restore_last_session);
        assert!(settings.general.ask_before_discarding);
        assert!(!settings.general.open_duplicated_request);
    }

    #[test]
    fn missing_general_fields_load_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"general":{"restore_last_session":false,"future":1}}"#)
                .unwrap();
        assert_eq!(
            settings.general,
            GeneralSettings {
                restore_last_session: false,
                ask_before_discarding: true,
                open_duplicated_request: false,
            }
        );
    }

    #[test]
    fn general_round_trips() {
        let settings = Settings {
            general: GeneralSettings {
                restore_last_session: false,
                ask_before_discarding: false,
                open_duplicated_request: true,
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""ask_before_discarding":false"#), "{json}");
        assert!(json.contains(r#""open_duplicated_request":true"#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn missing_requests_load_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"light"}}"#).unwrap();
        let requests = settings.requests;
        assert_eq!(requests, RequestSettings::default());
        assert_eq!(requests.timeout_secs, 30);
        assert!(requests.follow_redirects);
        assert_eq!(requests.max_redirects, 10);
        assert!(requests.verify_tls);
        assert_eq!(requests.proxy, ProxyMode::System);
        assert_eq!(requests.user_agent, "");
    }

    #[test]
    fn missing_request_fields_load_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"requests":{"timeout_secs":0,"future":true}}"#).unwrap();
        assert_eq!(
            settings.requests,
            RequestSettings {
                timeout_secs: 0,
                ..RequestSettings::default()
            }
        );
    }

    #[test]
    fn requests_round_trip() {
        let settings = Settings {
            requests: RequestSettings {
                timeout_secs: 5,
                follow_redirects: false,
                max_redirects: 3,
                verify_tls: false,
                proxy: ProxyMode::Custom,
                proxy_url: "http://proxy.local:3128".into(),
                user_agent: "posty-pete".into(),
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""proxy":"custom""#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn defaults_are_in_range() {
        let defaults = RequestSettings::default();
        assert!(RequestSettings::TIMEOUT_SECS.contains(&defaults.timeout_secs));
        assert!(RequestSettings::MAX_REDIRECTS.contains(&defaults.max_redirects));
    }

    #[test]
    fn missing_font_fields_load_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"light","accent":"red"}}"#).unwrap();
        let appearance = settings.appearance;
        assert_eq!(appearance.ui_font_size, 16);
        assert_eq!(appearance.editor_font_family, "");
        assert_eq!(appearance.editor_font_size, 13);
        assert!(appearance.wrap_responses);
        assert_eq!(appearance.editor_font_family(), None);
    }

    #[test]
    fn fonts_round_trip() {
        let settings = Settings {
            appearance: AppearanceSettings {
                ui_font_size: 14,
                editor_font_family: "Monaco".into(),
                editor_font_size: 15,
                wrap_responses: false,
                ..AppearanceSettings::default()
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""editor_font_family":"Monaco""#), "{json}");
        assert!(json.contains(r#""wrap_responses":false"#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn font_sizes_from_the_file_are_clamped() {
        let appearance = AppearanceSettings {
            ui_font_size: 2,
            editor_font_size: 400,
            ..AppearanceSettings::default()
        };
        assert_eq!(appearance.ui_font_size(), 10);
        assert_eq!(appearance.editor_font_size(), 32);
        let defaults = AppearanceSettings::default();
        assert_eq!(defaults.ui_font_size(), defaults.ui_font_size);
        assert_eq!(defaults.editor_font_size(), defaults.editor_font_size);
    }

    #[test]
    fn blank_editor_font_means_the_default() {
        let mut appearance = AppearanceSettings {
            editor_font_family: "   ".into(),
            ..AppearanceSettings::default()
        };
        assert_eq!(appearance.editor_font_family(), None);
        appearance.editor_font_family = " Fira Code ".into();
        assert_eq!(appearance.editor_font_family(), Some("Fira Code"));
    }

    #[test]
    fn missing_history_loads_as_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"appearance":{"theme":"light"}}"#).unwrap();
        assert_eq!(settings.history.retention_days, 14);
        assert_eq!(settings.history.retention_days(), 14);
    }

    #[test]
    fn history_round_trips() {
        let settings = Settings {
            history: HistorySettings { retention_days: 30 },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""retention_days":30"#), "{json}");
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    fn retention_from_the_file_is_clamped() {
        assert_eq!(HistorySettings { retention_days: 0 }.retention_days(), 1);
        assert_eq!(
            HistorySettings {
                retention_days: 9999
            }
            .retention_days(),
            365
        );
    }
}
