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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Read at startup only.
    pub restore_last_session: bool,
    pub ask_before_discarding: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            restore_last_session: true,
            ask_before_discarding: true,
        }
    }
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
            }
        );
    }

    #[test]
    fn general_round_trips() {
        let settings = Settings {
            general: GeneralSettings {
                restore_last_session: false,
                ask_before_discarding: false,
            },
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""ask_before_discarding":false"#), "{json}");
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
}
