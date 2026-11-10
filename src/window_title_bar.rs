use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, HighlightStyle, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, RenderOnce, SharedString, Styled, StyledText, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Sizable, TITLE_BAR_HEIGHT, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
};
use gpui_kit_assets::IconName as LucideIcon;

use crate::actions::{GoBack, GoForward, NewRequest, WORKSPACE_CONTEXT};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

const CONTEXT_SEPARATOR: &str = " / ";

/// What the main pane shows, e.g. a request name with its collection as context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTitle {
    pub context: Option<SharedString>,
    pub name: SharedString,
}

impl WindowTitle {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            context: None,
            name: name.into(),
        }
    }

    pub fn with_context(context: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self {
            context: Some(context.into()),
            name: name.into(),
        }
    }

    /// The title as plain text, e.g. "My API / Get users".
    pub fn label(&self) -> String {
        match &self.context {
            Some(context) => format!("{context}{CONTEXT_SEPARATOR}{}", self.name),
            None => self.name.to_string(),
        }
    }

    pub fn os_title(&self) -> String {
        format!("{} — {}", self.name, crate::WINDOW_TITLE)
    }

    fn styled_text(&self, context_color: gpui::Hsla) -> StyledText {
        match &self.context {
            Some(context) => {
                let prefix = format!("{context}{CONTEXT_SEPARATOR}");
                let muted = HighlightStyle {
                    color: Some(context_color),
                    ..Default::default()
                };
                StyledText::new(format!("{prefix}{}", self.name))
                    .with_highlights([(0..prefix.len(), muted)])
            }
            None => StyledText::new(self.name.clone()),
        }
    }
}

/// `gpui_component::TitleBar`'s private left padding (room for the traffic lights).
fn title_bar_left_inset(window: &Window) -> Pixels {
    let padding = if cfg!(target_os = "macos") {
        px(80.)
    } else {
        px(12.)
    };
    // TitleBar adds `pl_3` when fullscreen.
    if window.is_fullscreen() {
        padding + px(12.)
    } else {
        padding
    }
}

/// Width of the window controls `gpui_component::TitleBar` draws on the right.
fn title_bar_right_inset(window: &Window) -> Pixels {
    if cfg!(any(target_os = "macos", target_family = "wasm")) {
        return px(0.);
    }
    if cfg!(target_os = "linux")
        && !matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        )
    {
        return px(0.);
    }
    let controls = window.window_controls();
    let count = 1 + u8::from(controls.minimize) + u8::from(controls.maximize);
    TITLE_BAR_HEIGHT * f32::from(count)
}

/// Three small icon buttons (`size_6`) with `gap_1` between them.
fn nav_width(rem_size: Pixels) -> Pixels {
    rem_size * (3. * 1.5 + 2. * 0.25)
}

/// Left and right padding, within the area between the title bar's insets, that
/// centres the title in the window and keeps it `gap` clear of the nav buttons
/// and the window controls.
fn title_padding(
    left_inset: Pixels,
    right_inset: Pixels,
    nav_width: Pixels,
    gap: Pixels,
) -> (Pixels, Pixels) {
    let side = (left_inset + nav_width + gap).max(right_inset + gap);
    (side - left_inset, side - right_inset)
}

#[derive(IntoElement)]
pub struct WindowTitleBar {
    title: WindowTitle,
    edited: bool,
    can_go_back: bool,
    can_go_forward: bool,
    on_back: Option<ClickHandler>,
    on_forward: Option<ClickHandler>,
    on_new_request: Option<ClickHandler>,
    back_menu: Option<HistoryMenu>,
    forward_menu: Option<HistoryMenu>,
}

/// The entries a right-click on back or forward lists.
struct HistoryMenu {
    labels: Vec<SharedString>,
    on_select: SelectHandler,
}

impl HistoryMenu {
    fn new(
        labels: Vec<SharedString>,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            labels,
            on_select: Rc::new(on_select),
        }
    }

    fn build(&self, menu: PopupMenu) -> PopupMenu {
        self.labels
            .iter()
            .enumerate()
            .fold(menu.scrollable(true), |menu, (ix, label)| {
                let on_select = self.on_select.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .on_click(move |_, window, cx| on_select(ix, window, cx)),
                )
            })
    }
}

impl WindowTitleBar {
    pub fn new(title: WindowTitle) -> Self {
        Self {
            title,
            edited: false,
            can_go_back: false,
            can_go_forward: false,
            on_back: None,
            on_forward: None,
            on_new_request: None,
            back_menu: None,
            forward_menu: None,
        }
    }

    /// Shows that what the title names has unsaved changes.
    pub fn edited(mut self, edited: bool) -> Self {
        self.edited = edited;
        self
    }

    pub fn can_go_back(mut self, can_go_back: bool) -> Self {
        self.can_go_back = can_go_back;
        self
    }

    pub fn can_go_forward(mut self, can_go_forward: bool) -> Self {
        self.can_go_forward = can_go_forward;
        self
    }

    pub fn on_back(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_back = Some(Box::new(handler));
        self
    }

    pub fn on_forward(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_forward = Some(Box::new(handler));
        self
    }

    /// Entries listed on a right-click on back; `on_select` gets the chosen index.
    pub fn back_menu(
        mut self,
        labels: Vec<SharedString>,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.back_menu = Some(HistoryMenu::new(labels, on_select));
        self
    }

    /// Entries listed on a right-click on forward; `on_select` gets the chosen index.
    pub fn forward_menu(
        mut self,
        labels: Vec<SharedString>,
        on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.forward_menu = Some(HistoryMenu::new(labels, on_select));
        self
    }

    pub fn on_new_request(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_new_request = Some(Box::new(handler));
        self
    }
}

fn with_click(button: Button, handler: Option<ClickHandler>) -> Button {
    match handler {
        Some(handler) => button.on_click(handler),
        None => button,
    }
}

/// Wraps `button` rather than attaching the menu to it: an open menu becomes a
/// child of its element, which would cost an icon button its square size.
fn with_history_menu(
    id: impl Into<ElementId>,
    button: Button,
    menu: Option<HistoryMenu>,
) -> AnyElement {
    match menu.filter(|menu| !menu.labels.is_empty()) {
        Some(menu) => div()
            .id(id)
            .child(button)
            .context_menu(move |popup, _, _| menu.build(popup))
            .into_any_element(),
        None => button.into_any_element(),
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let rem_size = window.rem_size();
        let (title_left, title_right) = title_padding(
            title_bar_left_inset(window),
            title_bar_right_inset(window),
            nav_width(rem_size),
            rem_size * 0.75,
        );

        let back = Button::new("go-back")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::ChevronLeft))
            .accessibility_label("Back")
            .tooltip_with_action("Back", &GoBack, Some(WORKSPACE_CONTEXT))
            .disabled(!self.can_go_back);
        let forward = Button::new("go-forward")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::ChevronRight))
            .accessibility_label("Forward")
            .tooltip_with_action("Forward", &GoForward, Some(WORKSPACE_CONTEXT))
            .disabled(!self.can_go_forward);
        let home = Button::new("new-request")
            .ghost()
            .small()
            .icon(Icon::new(LucideIcon::House))
            .accessibility_label("New request")
            .tooltip_with_action("New request", &NewRequest, Some(WORKSPACE_CONTEXT));

        TitleBar::new().child(
            h_flex()
                .relative()
                .flex_1()
                .h_full()
                .min_w_0()
                .child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .justify_center()
                        .pl(title_left)
                        .pr(title_right)
                        .gap_1p5()
                        .text_sm()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(self.title.styled_text(cx.theme().muted_foreground)),
                        )
                        .when(self.edited, |this| {
                            this.child(
                                div()
                                    .flex_none()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Edited"),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .id("title-bar-nav")
                        .h_full()
                        .gap_1()
                        // Keeps a click on a button from starting a window drag, and
                        // a right-click from opening the Linux window menu.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .child(with_history_menu(
                            "go-back-menu",
                            with_click(back, self.on_back),
                            self.back_menu,
                        ))
                        .child(with_history_menu(
                            "go-forward-menu",
                            with_click(forward, self.on_forward),
                            self.forward_menu,
                        ))
                        .child(with_click(home, self.on_new_request)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_padding_centres_between_unequal_insets() {
        let (left, right) = title_padding(px(80.), px(0.), px(80.), px(12.));
        assert_eq!((left, right), (px(92.), px(172.)));
        // Content edges are mirror images: 80 + 92 from the left, 0 + 172 from the right.
        assert_eq!(px(80.) + left, px(0.) + right);
    }

    #[test]
    fn title_padding_clears_wide_window_controls() {
        let (left, right) = title_padding(px(12.), px(102.), px(80.), px(12.));
        assert_eq!(px(12.) + left, px(102.) + right);
        assert!(left >= px(80.) + px(12.));
        assert_eq!(right, px(12.));
    }

    #[test]
    fn nav_width_matches_small_icon_buttons() {
        assert_eq!(nav_width(px(16.)), px(80.));
    }

    #[test]
    fn label_joins_context_and_name() {
        assert_eq!(
            WindowTitle::with_context("My API", "Get users").label(),
            "My API / Get users"
        );
        assert_eq!(WindowTitle::new("Settings").label(), "Settings");
    }

    #[test]
    fn os_title_uses_the_name_only() {
        let title = WindowTitle::with_context("My API", "Get users");
        assert_eq!(
            title.os_title(),
            format!("Get users — {}", crate::WINDOW_TITLE)
        );
    }
}
