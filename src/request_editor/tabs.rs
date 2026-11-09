use gpui::{Context, IntoElement};
use gpui_component::tab::TabBar;

use super::RequestEditor;

#[derive(Clone, Copy, PartialEq)]
pub enum RequestTab {
    Params,
    Authorization,
    Headers,
    Body,
}

impl RequestTab {
    const ALL: [RequestTab; 4] = [Self::Params, Self::Authorization, Self::Headers, Self::Body];

    fn label(self) -> &'static str {
        match self {
            Self::Params => "Params",
            Self::Authorization => "Authorization",
            Self::Headers => "Headers",
            Self::Body => "Body",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

impl RequestEditor {
    pub(super) fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TabBar::new("request-tabs")
            .underline()
            .selected_index(self.active_tab.index())
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.active_tab = RequestTab::ALL[*ix];
                cx.notify();
            }))
            .children(RequestTab::ALL.map(RequestTab::label))
    }
}
