use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, h_flex, tooltip::Tooltip};

/// The warning line for unresolved variables. It takes its line even
/// without a hint, so what's below doesn't move when one comes and goes.
#[derive(IntoElement)]
pub struct UnresolvedHint {
    id: ElementId,
    hint: Option<SharedString>,
}

impl UnresolvedHint {
    pub fn new(id: impl Into<ElementId>, hint: Option<SharedString>) -> Self {
        Self {
            id: id.into(),
            hint,
        }
    }
}

impl RenderOnce for UnresolvedHint {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let row = h_flex()
            .id(self.id)
            .h_4()
            .gap_1p5()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        let Some(hint) = self.hint else {
            return row;
        };
        row.child(
            Icon::new(IconName::TriangleAlert)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().warning),
        )
        .child(div().min_w_0().truncate().child(hint.clone()))
        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
    }
}

/// The hint for `names`, the unresolved variables, or
/// `None` if there are none. `resolvable` is whether there's a collection
/// to resolve against; otherwise no variables apply.
pub fn unresolved_hint(names: &[String], resolvable: bool) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let list = names.join(", ");
    Some(match (resolvable, names.len()) {
        (false, _) => format!("Choose an environment to use variables: {list}"),
        (true, 1) => format!("Unresolved variable: {list}"),
        (true, _) => format!("Unresolved variables: {list}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn nothing_unresolved_has_no_hint() {
        assert_eq!(unresolved_hint(&[], true), None);
        assert_eq!(unresolved_hint(&[], false), None);
    }

    #[test]
    fn resolvable_lists_unresolved_names() {
        assert_eq!(
            unresolved_hint(&names(&["base", "token"]), true).as_deref(),
            Some("Unresolved variables: base, token")
        );
        assert_eq!(
            unresolved_hint(&names(&["base"]), true).as_deref(),
            Some("Unresolved variable: base")
        );
    }

    #[test]
    fn without_a_collection_asks_for_an_environment() {
        assert_eq!(
            unresolved_hint(&names(&["base"]), false).as_deref(),
            Some("Choose an environment to use variables: base")
        );
        assert_eq!(
            unresolved_hint(&names(&["base", "id"]), false).as_deref(),
            Some("Choose an environment to use variables: base, id")
        );
    }
}
