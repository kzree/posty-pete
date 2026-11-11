mod about;
mod ai;
mod appearance;
mod general;
mod history_and_data;
mod requests;
mod shortcuts;

use gpui::{
    AnyElement, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, Role, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Selectable, StyledExt,
    button::{Button, ButtonVariants},
    group_box::{GroupBox, GroupBoxVariants},
    h_flex,
    input::InputState,
    scroll::ScrollableElement,
    switch::Switch,
    v_flex,
};
use gpui_kit_assets::IconName as LucideIcon;

use about::AboutPage;
use ai::AiPage;
use appearance::AppearancePage;
use general::GeneralPage;
use history_and_data::HistoryAndDataPage;
use requests::RequestsPage;
use shortcuts::ShortcutsPage;

/// A page of the settings, in menu order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    General,
    Appearance,
    Requests,
    HistoryAndData,
    Ai,
    Shortcuts,
    About,
}

impl SettingsSection {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Appearance,
        Self::Requests,
        Self::HistoryAndData,
        Self::Ai,
        Self::Shortcuts,
        Self::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Requests => "Requests",
            Self::HistoryAndData => "History & data",
            Self::Ai => "AI",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::General => "How the app starts and when it asks before discarding changes.",
            Self::Appearance => "Theme, fonts and how responses are displayed.",
            Self::Requests => "Defaults for sending requests: timeout, redirects, TLS and proxy.",
            Self::HistoryAndData => "How long history is kept and where your data is stored.",
            Self::Ai => "Optional AI features and the provider they use.",
            Self::Shortcuts => "Keyboard shortcuts for common actions.",
            Self::About => "Version, licences and links.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::General => Icon::new(IconName::Settings),
            Self::Appearance => Icon::new(IconName::Palette),
            Self::Requests => Icon::new(LucideIcon::Send),
            Self::HistoryAndData => Icon::new(LucideIcon::RotateCcwClock),
            Self::Ai => Icon::new(LucideIcon::Sparkles),
            Self::Shortcuts => Icon::new(LucideIcon::Keyboard),
            Self::About => Icon::new(IconName::Info),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The section before (`forward == false`) or after this one, stopping at the ends.
    fn neighbor(self, forward: bool) -> Self {
        let index = self.index();
        let index = if forward {
            (index + 1).min(Self::ALL.len() - 1)
        } else {
            index.saturating_sub(1)
        };
        Self::ALL[index]
    }
}

/// The `/settings` page: a menu of sections and the selected one.
pub struct SettingsPage {
    section: SettingsSection,
    /// One per section, in [`SettingsSection::ALL`] order.
    menu_focus: Vec<FocusHandle>,
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
        Self {
            section: SettingsSection::default(),
            menu_focus: SettingsSection::ALL
                .iter()
                .map(|_| cx.focus_handle())
                .collect(),
            general: cx.new(|_| GeneralPage::new()),
            appearance: cx.new(|cx| AppearancePage::new(window, cx)),
            requests: cx.new(|cx| RequestsPage::new(window, cx)),
            history_and_data: cx.new(|cx| HistoryAndDataPage::new(window, cx)),
            ai: cx.new(|cx| AiPage::new(window, cx)),
            shortcuts: cx.new(|_| ShortcutsPage::new()),
            about: cx.new(|_| AboutPage),
        }
    }

    /// Shows `section`.
    pub fn open(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            cx.notify();
        }
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
            .icon(section.icon())
            .label(section.title())
            // Pushes the icon and label to the start.
            .child(div().flex_1())
            .on_click(cx.listener(move |this, _, _, cx| this.open(section, cx)))
    }

    fn render_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        v_flex()
            .id("settings-section")
            .role(Role::TabPanel)
            .aria_label(section.title())
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scrollbar()
            .child(
                v_flex()
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
            )
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
fn group(id: &'static str, title: &'static str) -> GroupBox {
    GroupBox::new()
        .id(id)
        .fill()
        .title(div().text_sm().font_medium().child(title))
}

/// A switch for a boolean field of the page `T`, named after its row's `label`.
fn switch<T: 'static>(
    id: &'static str,
    label: &'static str,
    checked: bool,
    set: fn(&mut T, bool),
    cx: &mut Context<T>,
) -> Switch {
    Switch::new(id)
        .checked(checked)
        .accessibility_label(label)
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

    #[test]
    fn opens_on_general() {
        assert_eq!(SettingsSection::default(), SettingsSection::General);
        assert_eq!(SettingsSection::ALL[0], SettingsSection::General);
    }

    #[test]
    fn menu_order_matches_the_titles() {
        let titles: Vec<_> = SettingsSection::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "General",
                "Appearance",
                "Requests",
                "History & data",
                "AI",
                "Shortcuts",
                "About"
            ]
        );
    }

    #[test]
    fn neighbor_stops_at_the_ends() {
        use SettingsSection::*;
        assert_eq!(General.neighbor(false), General);
        assert_eq!(General.neighbor(true), Appearance);
        assert_eq!(About.neighbor(true), About);
        assert_eq!(About.neighbor(false), Shortcuts);
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
