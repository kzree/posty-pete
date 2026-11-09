use gpui::{
    Context, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{
    ActiveTheme, Colorize, Icon, IconName,
    tab::{Tab, TabBar},
    tooltip::Tooltip,
    v_flex,
};
// The full Lucide catalog (`gpui_component::IconName` only has the original set).
// Icons used from here must also be listed in `assets.rs`.
use gpui_kit_assets::IconName as LucideIcon;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Library,
    History,
    Settings,
}

impl SidebarTab {
    const ALL: [SidebarTab; 3] = [Self::Library, Self::History, Self::Settings];

    fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::History => "History",
            Self::Settings => "Settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Library => Icon::new(LucideIcon::LibraryBig),
            Self::History => Icon::new(LucideIcon::RotateCcwClock),
            Self::Settings => Icon::new(IconName::Settings),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// The left-hand sidebar: library (saved requests), history, settings.
#[derive(Default)]
pub struct Sidebar {
    active_tab: SidebarTab,
}

impl Sidebar {
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
                // Equal-width tabs, so the icons sit centered across the sidebar.
                Tab::new()
                    .flex_1()
                    .icon(tab.icon())
                    .aria_label(tab.label())
                    .tooltip(move |window, cx| Tooltip::new(tab.label()).build(window, cx))
            }));

        let empty_text = match self.active_tab {
            SidebarTab::Library => "Your library is empty.",
            SidebarTab::History => "Requests you send show up here.",
            SidebarTab::Settings => "Settings coming soon.",
        };

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar.lighten(1.0))
            .child(tabs)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(empty_text),
            )
    }
}
