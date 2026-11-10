use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::FluentBuilder,
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
    id: usize,
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

pub enum KeyValueEditorEvent {
    /// A row was edited, toggled or removed.
    Changed,
}

pub struct KeyValueEditor {
    /// Noun for one row in accessible names, e.g. "header".
    item_name: &'static str,
    key_label: &'static str,
    value_label: &'static str,
    rows: Vec<Row>,
    next_row_id: usize,
}

impl EventEmitter<KeyValueEditorEvent> for KeyValueEditor {}

impl KeyValueEditor {
    pub fn new(item_name: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_column_labels(item_name, "Key", "Value", window, cx)
    }

    /// Like `new`, with other column headers (also used as placeholders).
    pub fn with_column_labels(
        item_name: &'static str,
        key_label: &'static str,
        value_label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            item_name,
            key_label,
            value_label,
            rows: Vec::new(),
            next_row_id: 0,
        };
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

    /// Replaces all rows. Row ids keep counting up so element ids stay unique.
    pub fn set_values(&mut self, values: &[KeyValue], window: &mut Window, cx: &mut Context<Self>) {
        self.rows.clear();
        for kv in values {
            self.push_row(kv.enabled, kv.key.clone(), kv.value.clone(), window, cx);
        }
        self.add_row(window, cx);
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.push_row(true, String::new(), String::new(), window, cx);
    }

    fn push_row(
        &mut self,
        enabled: bool,
        key: String,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (key_label, value_label) = (self.key_label, self.value_label);
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(key_label)
                .default_value(key)
        });
        let value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(value_label)
                .default_value(value)
        });

        let subscriptions = [&key, &value]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        this.ensure_trailing_empty_row(window, cx);
                        cx.emit(KeyValueEditorEvent::Changed);
                        cx.notify();
                    }
                })
            })
            .collect();

        let id = self.next_row_id;
        self.next_row_id += 1;
        self.rows.push(Row {
            id,
            enabled,
            key,
            value,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn row_mut(&mut self, id: usize) -> Option<&mut Row> {
        self.rows.iter_mut().find(|row| row.id == id)
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
            .child(div().flex_1().child(self.key_label))
            .child(div().flex_1().child(self.value_label))
            .child(div().w_6().flex_none());

        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                let is_trailing = ix == last;
                let id = row.id;
                let key = row.key.read(cx).value();
                let item = if key.is_empty() {
                    format!("unnamed {}", self.item_name)
                } else {
                    format!("{} {key}", self.item_name)
                };
                h_flex()
                    .gap_2()
                    .px_1()
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Checkbox::new(("kv-enabled", id))
                                .checked(row.enabled)
                                .accessibility_label(format!("Enable {item}"))
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    if let Some(row) = this.row_mut(id) {
                                        row.enabled = *checked;
                                        cx.emit(KeyValueEditorEvent::Changed);
                                        cx.notify();
                                    }
                                })),
                        )
                    }))
                    .child(div().flex_1().child(Input::new(&row.key).small()))
                    .child(div().flex_1().child(Input::new(&row.value).small()))
                    .child(div().w_6().flex_none().when(!is_trailing, |this| {
                        this.child(
                            Button::new(("kv-delete", id))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Remove")
                                .accessibility_label(format!("Remove {item}"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rows.retain(|row| row.id != id);
                                    cx.emit(KeyValueEditorEvent::Changed);
                                    cx.notify();
                                })),
                        )
                    }))
            })
            .collect::<Vec<_>>();

        v_flex().gap_1().child(header).children(rows)
    }
}
