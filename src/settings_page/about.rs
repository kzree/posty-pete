use domain::settings_index::SettingsGroup;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{ActiveTheme, StyledExt, h_flex, link::Link, v_flex};

use super::{group, page};
use crate::{WINDOW_TITLE, ui::setting_row::SettingRow};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = "A desktop client for composing, sending and saving HTTP requests.";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");
const LICENCE: &str = env!("CARGO_PKG_LICENSE");

struct Credit {
    name: &'static str,
    licence: &'static str,
    url: &'static str,
}

const CREDITS: [Credit; 3] = [
    Credit {
        name: "Lucide icons",
        licence: "ISC licence (some icons MIT, from Feather)",
        url: "https://lucide.dev",
    },
    Credit {
        name: "gpui",
        licence: "Apache License 2.0",
        url: "https://gpui.rs",
    },
    Credit {
        name: "gpui-component",
        licence: "Apache License 2.0",
        url: "https://github.com/longbridge/gpui-kit",
    },
];

/// A `Cargo.toml` value, or `None` when it isn't declared.
fn declared(value: &'static str) -> Option<&'static str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// The URL without its scheme or trailing slash, for display.
fn display_url(url: &str) -> &str {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    url.strip_suffix('/').unwrap_or(url)
}

fn url_link(id: &'static str, url: &'static str) -> Link {
    Link::new(id).href(url).child(display_url(url))
}

pub struct AboutPage;

impl AboutPage {
    fn render_app(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let project_row = |label: &'static str, value: gpui::AnyElement| {
            h_flex()
                .gap_2()
                .text_sm()
                .child(div().text_color(muted).child(label))
                .child(value)
        };
        let links = [
            ("Repository", "about-repository", REPOSITORY),
            ("Homepage", "about-homepage", HOMEPAGE),
        ]
        .into_iter()
        .filter_map(|(label, id, url)| {
            declared(url).map(|url| project_row(label, url_link(id, url).into_any_element()))
        });
        let licence = declared(LICENCE)
            .map(|licence| project_row("Licence", div().child(licence).into_any_element()));

        v_flex()
            .gap_1()
            .child(div().text_base().font_semibold().child(WINDOW_TITLE))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("Version {VERSION}")),
            )
            .child(div().text_sm().child(DESCRIPTION))
            .children(links)
            .children(licence)
    }

    fn render_credits(&self) -> impl IntoElement {
        group(SettingsGroup::Credits).children(CREDITS.iter().map(|credit| {
            SettingRow::new(credit.name)
                .description(credit.licence)
                .control(url_link(credit.name, credit.url).text_sm())
        }))
    }
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_app(cx))
            .child(self.render_credits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_values_are_not_declared() {
        assert_eq!(declared(""), None);
        assert_eq!(declared("  "), None);
        assert_eq!(declared("MIT"), Some("MIT"));
    }

    #[test]
    fn display_url_drops_scheme_and_trailing_slash() {
        assert_eq!(display_url("https://lucide.dev"), "lucide.dev");
        assert_eq!(display_url("http://example.com/x/"), "example.com/x");
        assert_eq!(display_url("example.com"), "example.com");
    }

    #[test]
    fn credit_urls_are_absolute() {
        for credit in &CREDITS {
            assert!(credit.url.starts_with("https://"), "{}", credit.name);
        }
    }
}
