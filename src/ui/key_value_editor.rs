use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, prelude::FluentBuilder,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};

use crate::domain::request::KeyValue;

struct Row {
    enabled: bool,
    key: Entity<InputState>,
    value: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Row {
    fn is_empty(&self, cx: &App) -> bool {
        [&self.key, &self.value]
            .iter()
            .all(|input| input.read(cx).value().is_empty())
    }

    fn to_key_value(&self, cx: &App) -> KeyValue {
        KeyValue {
            enabled: self.enabled,
            key: self.key.read(cx).value().to_string(),
            value: self.value.read(cx).value().to_string(),
        }
    }
}

pub struct KeyValueEditor {
    rows: Vec<Row>,
}

impl KeyValueEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self { rows: Vec::new() };
        this.add_row(window, cx);
        this
    }

    pub fn values(&self, cx: &App) -> Vec<KeyValue> {
        self.rows
            .iter()
            .filter(|row| !row.is_empty(cx))
            .map(|row| row.to_key_value(cx))
            .collect()
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("Key"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));

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

        self.rows.push(Row {
            enabled: true,
            key,
            value,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn ensure_trailing_empty_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rows.last().is_none_or(|row| !row.is_empty(cx)) {
            self.add_row(window, cx);
        }
    }
}

impl Render for KeyValueEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let last = self.rows.len().saturating_sub(1);

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
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                let is_trailing = ix == last;
                h_flex()
                    .gap_2()
                    .px_1()
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Checkbox::new(("kv-enabled", ix))
                                .checked(row.enabled)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    this.rows[ix].enabled = *checked;
                                    cx.notify();
                                })),
                        )
                    }))
                    .child(div().flex_1().child(Input::new(&row.key).small()))
                    .child(div().flex_1().child(Input::new(&row.value).small()))
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Button::new(("kv-delete", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rows.remove(ix);
                                    cx.notify();
                                })),
                        )
                    }))
            })
            .collect::<Vec<_>>();

        v_flex().gap_1().child(header).children(rows)
    }
}
