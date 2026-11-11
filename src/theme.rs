use std::sync::OnceLock;

use gpui::{
    App, Global, Hsla, Pixels, Rgba, SharedString, Subscription, Window, WindowAppearance, px, rgb,
};
use gpui_component::{ActiveTheme as _, Theme, ThemeColor, ThemeMode, TypographyTokens};

use crate::{
    domain::{
        accent::Accent,
        settings::{AppearanceSettings, ThemePreference},
    },
    settings::AppSettings,
};

/// How far hover and active states move from the accent, in HSL lightness.
const SHADE_STEP: f32 = 0.06;
const NEAR_BLACK: u32 = 0x0a0a0a;

/// Applies the saved theme, accent and fonts now and again whenever the settings
/// change. Call it after [`AppSettings::init`] and before opening a window.
pub fn init(cx: &mut App) {
    apply_settings(cx);
    cx.observe_global::<AppSettings>(apply_settings).detach();
}

/// Keeps the theme in step with the OS while the preference is System.
pub fn follow_system_appearance(window: &Window) -> Subscription {
    window.observe_window_appearance(|window, cx| {
        let appearance = &AppSettings::get(cx).appearance;
        if appearance.theme == ThemePreference::System {
            apply(window.appearance().into(), appearance.accent, cx);
        }
    })
}

/// The current theme's own primary colour, which the accent paints over.
pub fn neutral_primary(cx: &App) -> Hsla {
    cx.global::<AppliedTheme>().neutral_primary
}

/// The platform's default monospace font, used when none is chosen.
pub fn default_mono_font_family() -> SharedString {
    TypographyTokens::default().mono
}

/// Whether the system has a font family named `family`.
pub fn is_font_installed(family: &str, cx: &App) -> bool {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES
        .get_or_init(|| cx.text_system().all_font_names())
        .iter()
        .any(|name| name == family)
}

/// What [`apply`] last put on screen.
struct AppliedTheme {
    mode: ThemeMode,
    accent: Accent,
    neutral_primary: Hsla,
}

impl Global for AppliedTheme {}

fn apply_settings(cx: &mut App) {
    let appearance = &AppSettings::get(cx).appearance;
    let (preference, accent) = (appearance.theme, appearance.accent);
    cx.set_window_appearance(forced_appearance(preference));
    apply(theme_mode(preference, cx.window_appearance()), accent, cx);
    apply_fonts(false, cx);
}

/// Reloads the mode's theme, which also puts back its own colours and fonts,
/// then paints the accent and the chosen fonts over it.
fn apply(mode: ThemeMode, accent: Accent, cx: &mut App) {
    if cx
        .try_global::<AppliedTheme>()
        .is_some_and(|applied| applied.mode == mode && applied.accent == accent)
    {
        return;
    }
    Theme::change(mode, None, cx);
    let neutral_primary = cx.theme().primary;
    if let Some(hex) = accent.hex() {
        Theme::update(cx, |theme| paint_accent(&mut theme.colors, rgb(hex), mode));
    }
    cx.set_global(AppliedTheme {
        mode,
        accent,
        neutral_primary,
    });
    apply_fonts(true, cx);
}

/// The fonts [`apply_fonts`] last put on screen.
#[derive(Clone, PartialEq)]
struct AppliedFonts {
    size: Pixels,
    mono_size: Pixels,
    mono_family: SharedString,
}

impl Global for AppliedFonts {}

/// Puts the chosen fonts on the theme, unless they are already there and
/// `force` is false. A font that isn't installed falls back to the default.
fn apply_fonts(force: bool, cx: &mut App) {
    let fonts = fonts(&AppSettings::get(cx).appearance, cx);
    if !force && cx.try_global::<AppliedFonts>() == Some(&fonts) {
        return;
    }
    Theme::update(cx, |theme| {
        theme.font_size = fonts.size;
        theme.mono_font_size = fonts.mono_size;
        theme.mono_font_family = fonts.mono_family.clone();
    });
    cx.set_global(fonts);
}

fn fonts(appearance: &AppearanceSettings, cx: &App) -> AppliedFonts {
    let mono_family = appearance
        .editor_font_family()
        .filter(|family| is_font_installed(family, cx))
        .map_or_else(default_mono_font_family, |family| family.to_owned().into());
    AppliedFonts {
        size: px(appearance.ui_font_size() as f32),
        mono_size: px(appearance.editor_font_size() as f32),
        mono_family,
    }
}

fn paint_accent(colors: &mut ThemeColor, accent: Rgba, mode: ThemeMode) {
    let foreground: Hsla = foreground_on(accent).into();
    let accent: Hsla = accent.into();
    let hover = shade(accent, 1, mode.is_dark());
    let active = shade(accent, 2, mode.is_dark());

    colors.primary = accent;
    colors.primary_hover = hover;
    colors.primary_active = active;
    colors.primary_foreground = foreground;
    colors.button_primary = accent;
    colors.button_primary_hover = hover;
    colors.button_primary_active = active;
    colors.button_primary_foreground = foreground;
    colors.sidebar_primary = accent;
    colors.sidebar_primary_foreground = foreground;

    colors.ring = accent;
    colors.slider_bar = accent;
    colors.progress_bar = accent;
    colors.list_active = accent.alpha(0.15);
    colors.list_active_border = accent;
    colors.table_active = accent.alpha(0.15);
    colors.table_active_border = accent;
    colors.selection = accent.alpha(0.3);
    colors.drag_border = accent;
    colors.drop_target = accent.alpha(0.2);
}

/// `color` moved `steps` shades away from the background: darker in light
/// mode, lighter in dark mode.
fn shade(color: Hsla, steps: u8, dark: bool) -> Hsla {
    let delta = SHADE_STEP * f32::from(steps);
    let l = if dark {
        color.l + delta
    } else {
        color.l - delta
    };
    Hsla {
        l: l.clamp(0., 1.),
        ..color
    }
}

/// White or near-black, whichever contrasts more with `background`.
fn foreground_on(background: Rgba) -> Rgba {
    let white = gpui::white().to_rgb();
    let near_black = rgb(NEAR_BLACK);
    if contrast(background, white) >= contrast(background, near_black) {
        white
    } else {
        near_black
    }
}

/// WCAG 2 contrast ratio, from 1 to 21.
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG 2 relative luminance.
fn luminance(color: Rgba) -> f32 {
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
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

    #[test]
    fn contrast_matches_wcag() {
        let white = gpui::white().to_rgb();
        let black = gpui::black().to_rgb();
        assert!((contrast(white, black) - 21.).abs() < 0.01);
        assert!((contrast(black, white) - 21.).abs() < 0.01);
        assert!((contrast(white, white) - 1.).abs() < 0.01);
        assert!((contrast(rgb(0x3b82f6), white) - 3.68).abs() < 0.01);
    }

    #[test]
    fn foreground_picks_the_stronger_contrast() {
        let white = gpui::white().to_rgb();
        assert_eq!(foreground_on(rgb(0x1e3a8a)), white);
        assert_eq!(foreground_on(rgb(0x7f1d1d)), white);
        assert_eq!(foreground_on(rgb(0x22c55e)), rgb(NEAR_BLACK));
        assert_eq!(foreground_on(rgb(0xf97316)), rgb(NEAR_BLACK));
        assert_eq!(foreground_on(rgb(0xfde047)), rgb(NEAR_BLACK));
    }

    #[test]
    fn every_accent_has_readable_text() {
        for hex in Accent::ALL.iter().filter_map(|a| a.hex()) {
            let accent = rgb(hex);
            assert!(contrast(accent, foreground_on(accent)) >= 4.5, "{hex:06x}");
        }
    }

    #[test]
    fn shades_move_away_from_the_background() {
        let base: Hsla = rgb(0x3b82f6).into();
        let light = [shade(base, 1, false), shade(base, 2, false)];
        assert!(light[0].l < base.l && light[1].l < light[0].l);
        let dark = [shade(base, 1, true), shade(base, 2, true)];
        assert!(dark[0].l > base.l && dark[1].l > dark[0].l);
        assert_eq!(
            (light[0].h, light[0].s, light[0].a),
            (base.h, base.s, base.a)
        );
    }

    #[test]
    fn shades_stay_in_range() {
        let white: Hsla = gpui::white();
        let black: Hsla = gpui::black();
        assert_eq!(shade(white, 2, true).l, 1.);
        assert_eq!(shade(black, 2, false).l, 0.);
    }
}
