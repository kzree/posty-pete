use std::rc::Rc;

use gpui::{Anchor, App, IntoElement, RenderOnce, Styled, Window, px};
use gpui_component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use uuid::Uuid;

use domain::environment_menu::{EnvironmentChoice, EnvironmentMenu, EnvironmentMenuItem};

use crate::window_title_bar::TRAILING_MAX_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentPickerAction {
    Choose(EnvironmentChoice),
    /// Open collection `Uuid`'s environments.
    Manage(Uuid),
    /// Open the shared environments, with this one selected if given.
    ManageShared(Option<Uuid>),
}

type ActionHandler = Rc<dyn Fn(&EnvironmentPickerAction, &mut Window, &mut App)>;

/// The title bar's environment dropdown.
#[derive(IntoElement)]
pub struct EnvironmentPicker {
    menu: EnvironmentMenu,
    on_action: ActionHandler,
}

impl EnvironmentPicker {
    pub fn new(
        menu: EnvironmentMenu,
        on_action: impl Fn(&EnvironmentPickerAction, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            menu,
            on_action: Rc::new(on_action),
        }
    }
}

fn menu_item(item: &EnvironmentMenuItem, on_action: &ActionHandler) -> PopupMenuItem {
    let (item, action) = match item {
        EnvironmentMenuItem::Heading(name) => return PopupMenuItem::label(name.clone()),
        EnvironmentMenuItem::Separator => return PopupMenuItem::separator(),
        EnvironmentMenuItem::Choice {
            choice,
            name,
            checked,
        } => (
            PopupMenuItem::new(name.clone()).checked(*checked),
            EnvironmentPickerAction::Choose(*choice),
        ),
        EnvironmentMenuItem::Manage(id) => (
            PopupMenuItem::new("Manage environments…"),
            EnvironmentPickerAction::Manage(*id),
        ),
        EnvironmentMenuItem::ManageShared(id) => (
            PopupMenuItem::new("Manage shared environments…"),
            EnvironmentPickerAction::ManageShared(*id),
        ),
    };
    let on_action = on_action.clone();
    item.on_click(move |_, window, cx| on_action(&action, window, cx))
}

impl RenderOnce for EnvironmentPicker {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let items = Rc::new(self.menu.items);
        let on_action = self.on_action;
        Button::new("environment-picker")
            .ghost()
            .small()
            .label(self.menu.label.clone())
            .dropdown_caret(true)
            .max_w(TRAILING_MAX_WIDTH)
            .accessibility_label(format!("Environment: {}", self.menu.label))
            .tooltip(self.menu.tooltip)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu: PopupMenu, _, _| {
                items
                    .iter()
                    .fold(menu.max_w(px(360.)).scrollable(true), |menu, item| {
                        menu.item(menu_item(item, &on_action))
                    })
            })
    }
}
