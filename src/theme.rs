use gpui::{App, Subscription, Window, WindowAppearance};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};

use crate::{domain::settings::ThemePreference, settings::AppSettings};

/// Applies the saved theme now and again whenever the settings change. Call it
/// after [`AppSettings::init`] and before opening a window.
pub fn init(cx: &mut App) {
    apply_preference(cx);
    cx.observe_global::<AppSettings>(apply_preference).detach();
}

/// Keeps the theme in step with the OS while the preference is System.
pub fn follow_system_appearance(window: &Window) -> Subscription {
    window.observe_window_appearance(|window, cx| {
        if preference(cx) == ThemePreference::System {
            set_mode(window.appearance().into(), cx);
        }
    })
}

fn apply_preference(cx: &mut App) {
    let preference = preference(cx);
    cx.set_window_appearance(forced_appearance(preference));
    set_mode(theme_mode(preference, cx.window_appearance()), cx);
}

fn preference(cx: &App) -> ThemePreference {
    AppSettings::get(cx).appearance.theme
}

fn set_mode(mode: ThemeMode, cx: &mut App) {
    if cx.theme().mode != mode {
        Theme::change(mode, None, cx);
    }
}

fn theme_mode(preference: ThemePreference, system: WindowAppearance) -> ThemeMode {
    match preference {
        ThemePreference::System => system.into(),
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
    }
}

/// The appearance the native window chrome is pinned to, or `None` to follow the OS.
fn forced_appearance(preference: ThemePreference) -> Option<WindowAppearance> {
    match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(WindowAppearance::Light),
        ThemePreference::Dark => Some(WindowAppearance::Dark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_follows_the_os() {
        let mode = |os| theme_mode(ThemePreference::System, os);
        assert_eq!(mode(WindowAppearance::Light), ThemeMode::Light);
        assert_eq!(mode(WindowAppearance::VibrantLight), ThemeMode::Light);
        assert_eq!(mode(WindowAppearance::Dark), ThemeMode::Dark);
        assert_eq!(mode(WindowAppearance::VibrantDark), ThemeMode::Dark);
        assert_eq!(forced_appearance(ThemePreference::System), None);
    }

    #[test]
    fn light_and_dark_ignore_the_os() {
        for os in [WindowAppearance::Light, WindowAppearance::Dark] {
            assert_eq!(theme_mode(ThemePreference::Light, os), ThemeMode::Light);
            assert_eq!(theme_mode(ThemePreference::Dark, os), ThemeMode::Dark);
        }
        assert_eq!(
            forced_appearance(ThemePreference::Light),
            Some(WindowAppearance::Light)
        );
        assert_eq!(
            forced_appearance(ThemePreference::Dark),
            Some(WindowAppearance::Dark)
        );
    }
}
