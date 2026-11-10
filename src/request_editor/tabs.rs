use gpui::{Anchor, Context, IntoElement};
use gpui_component::{
    IconName, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    tab::TabBar,
};

use super::{RequestEditor, RequestEditorEvent};

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
            .suffix(self.render_more_menu(cx))
    }

    fn render_more_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.weak_entity();
        Button::new("request-more")
            .icon(IconName::Ellipsis)
            .ghost()
            .xsmall()
            .tooltip("More actions")
            .accessibility_label("More actions")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let editor = editor.clone();
                menu.item(
                    PopupMenuItem::new("Copy as cURL").on_click(move |_, _, cx| {
                        editor
                            .update(cx, |editor, cx| {
                                let request = editor.build_request(cx);
                                cx.emit(RequestEditorEvent::CopyAsCurl(request));
                            })
                            .ok();
                    }),
                )
            })
    }
}
