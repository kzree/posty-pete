use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{ActiveTheme, StyledExt, v_flex};

use super::{group, page};
use crate::{WINDOW_TITLE, ui::setting_row::SettingRow};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = "A desktop client for composing, sending and saving HTTP requests.";

struct Credit {
    name: &'static str,
    licence: &'static str,
    url: &'static str,
}

const CREDITS: [Credit; 3] = [
    Credit {
        name: "Lucide icons",
        licence: "ISC licence (some icons MIT, from Feather)",
        url: "lucide.dev",
    },
    Credit {
        name: "gpui",
        licence: "Apache License 2.0",
        url: "gpui.rs",
    },
    Credit {
        name: "gpui-component",
        licence: "Apache License 2.0",
        url: "github.com/longbridge/gpui-kit",
    },
];

pub struct AboutPage;

impl AboutPage {
    fn render_app(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
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
    }

    fn render_credits(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        group("about-credits", "Open source").children(CREDITS.iter().map(|credit| {
            SettingRow::new(credit.name)
                .description(credit.licence)
                .control(div().text_sm().text_color(muted).child(credit.url))
        }))
    }
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_app(cx))
            .child(self.render_credits(cx))
    }
}
