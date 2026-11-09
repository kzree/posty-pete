use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Styled, Subscription, Window,
    div, prelude::FluentBuilder,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};

use super::RequestEditor;
use crate::domain::request::QueryParam;

/// One editable row in the Params table.
pub(super) struct ParamRow {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
    /// Dropped together with the row.
    _subscriptions: Vec<Subscription>,
}

impl ParamRow {
    pub(super) fn is_empty(&self, cx: &App) -> bool {
        [&self.key, &self.value]
            .iter()
            .all(|input| input.read(cx).value().is_empty())
    }

    pub(super) fn to_param(&self, cx: &App) -> QueryParam {
        QueryParam {
            enabled: self.enabled,
            key: self.key.read(cx).value().to_string(),
            value: self.value.read(cx).value().to_string(),
        }
    }
}

impl RequestEditor {
    pub(super) fn add_param_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("Key"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));

        // Typing into the trailing row adds a fresh empty row below it.
        let subscriptions = [&key, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        this.ensure_trailing_empty_row(window, cx);
                    }
                })
            })
            .collect();

        self.params.push(ParamRow {
            enabled: true,
            key,
            value,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn ensure_trailing_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let needs_row = self.params.last().is_none_or(|row| !row.is_empty(cx));
        if needs_row {
            self.add_param_row(window, cx);
        }
    }

    pub(super) fn render_params(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let last = self.params.len().saturating_sub(1);

        let header = h_flex()
            .gap_2()
            .px_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().w_6().flex_none())
            .child(div().flex_1().child("Key"))
            .child(div().flex_1().child("Value"))
            .child(div().w_6().flex_none());

        let rows = self
            .params
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                let is_trailing = ix == last;
                h_flex()
                    .gap_2()
                    .px_1()
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Checkbox::new(("param-enabled", ix))
                                .checked(row.enabled)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    this.params[ix].enabled = *checked;
                                    cx.notify();
                                })),
                        )
                    }))
                    .child(div().flex_1().child(Input::new(&row.key).small()))
                    .child(div().flex_1().child(Input::new(&row.value).small()))
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Button::new(("param-delete", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.params.remove(ix);
                                    cx.notify();
                                })),
                        )
                    }))
            })
            .collect::<Vec<_>>();

        v_flex().gap_1().child(header).children(rows)
    }
}
