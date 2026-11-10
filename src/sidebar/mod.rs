use gpui::{
    App, Context, Div, Entity, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Window, div,
};
use gpui_component::{
    ActiveTheme, Colorize, Icon,
    tab::{Tab, TabBar},
    tooltip::Tooltip,
    v_flex,
};
// Icons used here must also be listed in `assets.rs`.
use gpui_kit_assets::IconName as LucideIcon;

pub mod history_panel;

use history_panel::HistoryPanel;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Library,
    History,
    Tests,
}

impl SidebarTab {
    const ALL: [SidebarTab; 3] = [Self::Library, Self::History, Self::Tests];

    fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::History => "History",
            Self::Tests => "Tests",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Library => Icon::new(LucideIcon::LibraryBig),
            Self::History => Icon::new(LucideIcon::RotateCcwClock),
            Self::Tests => Icon::new(LucideIcon::FlaskConical),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

pub struct Sidebar {
    active_tab: SidebarTab,
    history_panel: Entity<HistoryPanel>,
}

impl Sidebar {
    pub fn new(history_panel: Entity<HistoryPanel>) -> Self {
        Self {
            active_tab: SidebarTab::default(),
            history_panel,
        }
    }

    pub fn set_active_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.active_tab = tab;
        cx.notify();
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = TabBar::new("sidebar-tabs")
            .underline()
            .selected_index(self.active_tab.index())
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                this.set_active_tab(SidebarTab::ALL[*ix], cx);
            }))
            .children(SidebarTab::ALL.map(|tab| {
                Tab::new()
                    .flex_1()
                    .icon(tab.icon())
                    .aria_label(tab.label())
                    .tooltip(move |window, cx| Tooltip::new(tab.label()).build(window, cx))
            }));

        let content = div().flex_1().min_h_0();
        let content = match self.active_tab {
            SidebarTab::History => content.child(self.history_panel.clone()),
            SidebarTab::Library => content.child(placeholder("Your library is empty.", cx)),
            SidebarTab::Tests => content.child(placeholder("Tests coming soon.", cx)),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar.lighten(1.0))
            .child(tabs)
            .child(content)
    }
}

fn placeholder(text: &'static str, cx: &App) -> Div {
    div()
        .p_3()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
