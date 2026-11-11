mod about;
mod ai;
mod appearance;
mod general;
mod history_and_data;
mod mcp;
mod requests;
mod reveal;
mod shortcuts;

use gpui::{
    AnyElement, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Point, Render, Role, ScrollHandle, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Selectable, StyledExt,
    button::{Button, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::InputState,
    scroll::ScrollableElement as _,
    switch::Switch,
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

pub use crate::domain::settings_index::SettingsSection;
use crate::domain::settings_index::{Setting, SettingsGroup};
use about::AboutPage;
use ai::AiPage;
use appearance::AppearancePage;
use general::GeneralPage;
use history_and_data::HistoryAndDataPage;
use requests::RequestsPage;
use reveal::{Reveal, row, row_standing_in};
use shortcuts::ShortcutsPage;

fn section_icon(section: SettingsSection) -> Icon {
    match section {
        SettingsSection::General => Icon::new(IconName::Settings),
        SettingsSection::Appearance => Icon::new(IconName::Palette),
        SettingsSection::Requests => Icon::new(LucideIcon::Send),
        SettingsSection::HistoryAndData => Icon::new(LucideIcon::RotateCcwClock),
        SettingsSection::Ai => Icon::new(LucideIcon::Sparkles),
        SettingsSection::Shortcuts => Icon::new(LucideIcon::Keyboard),
        SettingsSection::About => Icon::new(IconName::Info),
    }
}

/// The `/settings` page: a menu of sections and the selected one.
pub struct SettingsPage {
    section: SettingsSection,
    /// One per section, in [`SettingsSection::ALL`] order.
    menu_focus: Vec<FocusHandle>,
    scroll: ScrollHandle,
    reveal: Entity<Reveal>,
    general: Entity<GeneralPage>,
    appearance: Entity<AppearancePage>,
    requests: Entity<RequestsPage>,
    history_and_data: Entity<HistoryAndDataPage>,
    ai: Entity<AiPage>,
    shortcuts: Entity<ShortcutsPage>,
    about: Entity<AboutPage>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        let reveal = cx.new(|_| Reveal::new(scroll.clone()));
        Self {
            section: SettingsSection::default(),
            menu_focus: SettingsSection::ALL
                .iter()
                .map(|_| cx.focus_handle())
                .collect(),
            scroll,
            general: cx.new(|cx| GeneralPage::new(reveal.clone(), cx)),
            appearance: cx.new(|cx| AppearancePage::new(reveal.clone(), window, cx)),
            requests: cx.new(|cx| RequestsPage::new(reveal.clone(), window, cx)),
            history_and_data: cx.new(|cx| HistoryAndDataPage::new(reveal.clone(), window, cx)),
            ai: cx.new(|cx| AiPage::new(reveal.clone(), window, cx)),
            shortcuts: cx.new(|_| ShortcutsPage::new(reveal.clone())),
            about: cx.new(|_| AboutPage),
            reveal,
        }
    }

    /// Shows `section`, from the top.
    pub fn open(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.scroll.set_offset(Point::default());
            cx.notify();
        }
    }

    pub fn section(&self) -> SettingsSection {
        self.section
    }

    /// Shows `section` with its menu item focused.
    pub fn show_section(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(section, cx);
        window.focus(&self.menu_focus[section.index()], cx);
    }

    /// Shows `setting`'s page, scrolls its row into view, focuses its
    /// control and highlights the row briefly.
    pub fn reveal(&mut self, setting: Setting, cx: &mut Context<Self>) {
        self.open(setting.section(), cx);
        self.reveal
            .update(cx, |reveal, cx| reveal.start(setting, cx));
    }

    fn open_neighbor(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let from = self
            .menu_focus
            .iter()
            .position(|handle| handle.is_focused(window))
            .map_or(self.section, |index| SettingsSection::ALL[index]);
        let section = from.neighbor(forward);
        self.open(section, cx);
        window.focus(&self.menu_focus[section.index()], cx);
    }

    fn on_menu_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let forward = match event.keystroke.key.as_str() {
            "up" => false,
            "down" => true,
            _ => return,
        };
        self.open_neighbor(forward, window, cx);
        cx.stop_propagation();
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("settings-menu")
            .role(Role::TabList)
            .aria_label("Settings")
            .w(px(200.))
            .flex_none()
            .gap_1()
            .on_key_down(cx.listener(Self::on_menu_key_down))
            .children(
                SettingsSection::ALL
                    .iter()
                    .map(|section| self.render_menu_item(*section, cx)),
            )
    }

    fn render_menu_item(&self, section: SettingsSection, cx: &mut Context<Self>) -> Button {
        let index = section.index();
        Button::new(("settings-section", index))
            .role(Role::Tab)
            .ghost()
            .w_full()
            .selected(self.section == section)
            .track_focus(&self.menu_focus[index])
            .icon(section_icon(section))
            .label(section.title())
            // Pushes the icon and label to the start.
            .child(div().flex_1())
            .on_click(cx.listener(move |this, _, _, cx| this.open(section, cx)))
    }

    fn render_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        let panel = v_flex()
            .id("settings-section")
            .role(Role::TabPanel)
            .aria_label(section.title())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                v_flex()
                    .flex_none()
                    .max_w(px(640.))
                    .gap_1()
                    .child(
                        div()
                            .id("settings-section-heading")
                            .role(Role::Heading)
                            .aria_level(2)
                            .text_base()
                            .font_semibold()
                            .child(section.title()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(section.description()),
                    )
                    .child(div().pt_6().child(self.render_settings())),
            );
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(panel)
            .vertical_scrollbar(&self.scroll)
    }

    fn render_settings(&self) -> AnyElement {
        match self.section {
            SettingsSection::General => self.general.clone().into_any_element(),
            SettingsSection::Appearance => self.appearance.clone().into_any_element(),
            SettingsSection::Requests => self.requests.clone().into_any_element(),
            SettingsSection::HistoryAndData => self.history_and_data.clone().into_any_element(),
            SettingsSection::Ai => self.ai.clone().into_any_element(),
            SettingsSection::Shortcuts => self.shortcuts.clone().into_any_element(),
            SettingsSection::About => self.about.clone().into_any_element(),
        }
    }
}

/// The groups of a settings page, stacked.
fn page() -> gpui::Div {
    v_flex().gap_6()
}

/// A titled group of setting rows.
fn group(group: SettingsGroup) -> GroupBox {
    GroupBox::new()
        .id(group.id())
        .fill()
        .title(div().text_sm().font_medium().child(group.title()))
}

/// A switch for a boolean field of the page `T`, named after `setting`.
fn switch<T: 'static>(
    setting: Setting,
    checked: bool,
    set: fn(&mut T, bool),
    cx: &mut Context<T>,
) -> Switch {
    Switch::new(setting.id())
        .checked(checked)
        .accessibility_label(setting.label())
        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
            set(this, *checked);
            cx.notify();
        }))
}

/// State for a whole-number input stepping within `min..=max`.
fn number_state(
    value: u32,
    min: u32,
    max: u32,
    window: &mut Window,
    cx: &mut Context<InputState>,
) -> InputState {
    InputState::new(window, cx)
        .default_value(value.to_string())
        .min(min.into())
        .max(max.into())
        .validate(|text, _| is_digits(text))
}

fn is_digits(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii_digit())
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("settings-page")
            .role(Role::Group)
            .aria_label("Settings")
            .size_full()
            .child(
                div()
                    .id("settings-heading")
                    .role(Role::Heading)
                    .aria_level(1)
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .text_lg()
                    .font_semibold()
                    .child("Settings"),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .gap_6()
                    .px_4()
                    .pt_2()
                    .pb_4()
                    .child(self.render_menu(cx))
                    .child(self.render_section(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sources of a page's rows.
    fn sources(section: SettingsSection) -> &'static [&'static str] {
        match section {
            SettingsSection::General => &[include_str!("general.rs")],
            SettingsSection::Appearance => &[include_str!("appearance.rs")],
            SettingsSection::Requests => &[include_str!("requests.rs")],
            SettingsSection::HistoryAndData => &[include_str!("history_and_data.rs")],
            SettingsSection::Ai => &[include_str!("ai.rs"), include_str!("mcp.rs")],
            SettingsSection::Shortcuts => &[include_str!("shortcuts.rs")],
            SettingsSection::About => &[include_str!("about.rs")],
        }
    }

    /// Whether the page's sources use the path `name`, not just one it starts.
    fn mentions(section: SettingsSection, name: String) -> bool {
        sources(section).iter().any(|source| {
            source.match_indices(&name).any(|(ix, _)| {
                source[ix + name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
            })
        })
    }

    // Shortcut rows are built from the index, so only the others are checked.
    #[test]
    fn every_indexed_setting_is_on_its_page() {
        for setting in Setting::ROWS {
            let name = format!("Setting::{setting:?}");
            assert!(mentions(setting.section(), name), "{setting:?}");
        }
    }

    #[test]
    fn every_group_is_on_its_page() {
        for group in SettingsGroup::ALL {
            if group.section() == SettingsSection::Shortcuts {
                continue;
            }
            let name = format!("SettingsGroup::{group:?}");
            assert!(mentions(group.section(), name), "{group:?}");
        }
    }

    #[test]
    fn setting_rows_come_from_the_index() {
        for section in SettingsSection::ALL {
            if section == SettingsSection::About {
                continue;
            }
            for source in sources(section) {
                assert!(!source.contains("SettingRow::new"), "{section:?}");
            }
        }
    }

    #[test]
    fn number_inputs_accept_only_digits() {
        assert!(is_digits(""));
        assert!(is_digits("30"));
        assert!(!is_digits("-1"));
        assert!(!is_digits("1.5"));
        assert!(!is_digits("10s"));
    }
}
