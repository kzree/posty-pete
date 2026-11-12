use domain::{
    json_body,
    request::{Body, RawKind},
};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px, relative,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Editor, EditorState, InputEvent},
    scroll::ScrollableElement,
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    tab::TabBar,
    tooltip::Tooltip,
    v_flex,
};

use super::Edited;
use crate::{
    actions::{BODY_EDITOR_CONTEXT, FormatJsonBody},
    ui::key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
};

#[derive(Clone, Copy, PartialEq)]
enum BodyMode {
    None,
    FormData,
    UrlEncoded,
    Raw,
}

impl BodyMode {
    const ALL: [BodyMode; 4] = [Self::None, Self::FormData, Self::UrlEncoded, Self::Raw];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::FormData => "Form data",
            Self::UrlEncoded => "Form URL-encoded",
            Self::Raw => "Raw",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
}

#[derive(Clone)]
struct RawKindItem(RawKind);

impl SearchableListItem for RawKindItem {
    type Value = RawKind;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn value(&self) -> &RawKind {
        &self.0
    }
}

fn editor_language(kind: RawKind) -> &'static str {
    match kind {
        RawKind::Json => "json",
        RawKind::Text => "text",
        RawKind::Html | RawKind::Xml => "html", // gpui-component 0.9 has no XML grammar
    }
}

pub struct BodyEditor {
    mode: BodyMode,
    raw_kind: Entity<SelectState<Vec<RawKindItem>>>,
    raw: Entity<EditorState>,
    /// Shared by form data and URL-encoded, so switching between them keeps the fields.
    form_fields: Entity<KeyValueEditor>,
    json_error: Option<SharedString>,
    json_check_pending: bool,
    /// Why the loaded body is empty; dropped when the body is edited or replaced.
    note: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Edited> for BodyEditor {}

impl BodyEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = RawKind::ALL.into_iter().map(RawKindItem).collect();
        let raw_kind = cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx));
        let raw =
            cx.new(|cx| EditorState::new(window, cx).language(editor_language(RawKind::default())));
        let form_fields = cx.new(|cx| KeyValueEditor::new("field", window, cx));

        let sub = cx.subscribe_in(
            &raw_kind,
            window,
            |this, _, event: &SelectEvent<Vec<RawKindItem>>, window, cx| {
                if let SelectEvent::Confirm(Some(kind)) = event {
                    let language = editor_language(*kind);
                    this.raw
                        .update(cx, |raw, cx| raw.set_highlighter(language, cx));
                    this.schedule_json_check(window, cx);
                    this.edited(cx);
                }
            },
        );
        let raw_sub = cx.subscribe_in(&raw, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::Change = event {
                this.schedule_json_check(window, cx);
                this.edited(cx);
            }
        });
        let form_sub = cx.subscribe(&form_fields, |this, _, _: &KeyValueEditorEvent, cx| {
            this.edited(cx)
        });

        Self {
            mode: BodyMode::None,
            raw_kind,
            raw,
            form_fields,
            json_error: None,
            json_check_pending: false,
            note: None,
            _subscriptions: vec![sub, raw_sub, form_sub],
        }
    }

    pub fn show_note(&mut self, note: SharedString, cx: &mut Context<Self>) {
        self.note = Some(note);
        cx.notify();
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        if self.note.take().is_some() {
            cx.notify();
        }
        cx.emit(Edited);
    }

    fn is_json(&self, cx: &App) -> bool {
        self.mode == BodyMode::Raw
            && self.raw_kind.read(cx).selected_value() == Some(&RawKind::Json)
    }

    /// Checks the JSON once before the next frame, however many edits come first.
    fn schedule_json_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.json_check_pending {
            return;
        }
        self.json_check_pending = true;
        cx.on_next_frame(window, |this, _, cx| this.check_json(cx));
    }

    fn check_json(&mut self, cx: &mut Context<Self>) {
        self.json_check_pending = false;
        let error = if self.is_json(cx) {
            json_body::validate(&self.raw.read(cx).text().to_string())
                .map(|error| format!("Invalid JSON: {error}").into())
        } else {
            None
        };
        if error != self.json_error {
            self.json_error = error;
            cx.notify();
        }
    }

    fn format_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_json(cx) {
            return;
        }
        let text = self.raw.read(cx).text().to_string();
        match json_body::format(&text) {
            Ok(formatted) => self.raw.update(cx, |raw, cx| {
                if formatted != text {
                    raw.replace_all(formatted, window, cx);
                }
                raw.focus(window, cx);
            }),
            Err(_) => self.check_json(cx),
        }
    }

    /// Content of the modes `body` doesn't use is cleared, so switching mode
    /// doesn't bring back text from a previous request.
    pub fn set_body(&mut self, body: &Body, window: &mut Window, cx: &mut Context<Self>) {
        let (mode, fields, kind, text) = match body {
            Body::None => (BodyMode::None, &[][..], RawKind::default(), ""),
            Body::FormData(fields) => (BodyMode::FormData, &fields[..], RawKind::default(), ""),
            Body::UrlEncoded(fields) => (BodyMode::UrlEncoded, &fields[..], RawKind::default(), ""),
            Body::Raw { kind, text } => (BodyMode::Raw, &[][..], *kind, text.as_str()),
        };

        self.mode = mode;
        self.note = None;
        self.form_fields
            .update(cx, |form, cx| form.set_values(fields, window, cx));
        self.raw_kind.update(cx, |select, cx| {
            select.set_selected_value(&kind, window, cx)
        });
        self.raw.update(cx, |raw, cx| {
            raw.set_highlighter(editor_language(kind), cx);
            raw.set_value(text.to_string(), window, cx);
        });
        self.schedule_json_check(window, cx);
        cx.notify();
    }

    pub fn body(&self, cx: &App) -> Body {
        match self.mode {
            BodyMode::None => Body::None,
            BodyMode::FormData => Body::FormData(self.form_fields.read(cx).values(cx)),
            BodyMode::UrlEncoded => Body::UrlEncoded(self.form_fields.read(cx).values(cx)),
            BodyMode::Raw => Body::Raw {
                kind: self
                    .raw_kind
                    .read(cx)
                    .selected_value()
                    .copied()
                    .unwrap_or_default(),
                text: self.raw.read(cx).text().to_string(),
            },
        }
    }
}

impl Render for BodyEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_json = self.is_json(cx);
        let toolbar = h_flex()
            .flex_none()
            .h_6()
            .gap_4()
            .child(
                div().flex_none().child(
                    TabBar::new("body-mode")
                        .segmented()
                        .small()
                        .selected_index(self.mode.index())
                        .on_click(cx.listener(|this, ix: &usize, window, cx| {
                            this.mode = BodyMode::ALL[*ix];
                            this.schedule_json_check(window, cx);
                            this.edited(cx);
                            cx.notify();
                        }))
                        .children(BodyMode::ALL.map(BodyMode::label)),
                ),
            )
            .when(self.mode == BodyMode::Raw, |this| {
                this.child(div().w(px(100.)).child(Select::new(&self.raw_kind).small()))
            })
            .when(is_json, |this| {
                this.child(div().flex_1()).child(
                    Button::new("format-json")
                        .ghost()
                        .small()
                        .label("Format")
                        .tooltip_with_action(
                            "Format JSON",
                            &FormatJsonBody,
                            Some(BODY_EDITOR_CONTEXT),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.format_json(window, cx))),
                )
            });

        let content = match self.mode {
            BodyMode::None => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("This request has no body.")
                .into_any_element(),
            BodyMode::FormData | BodyMode::UrlEncoded => div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(self.form_fields.clone())
                .into_any_element(),
            BodyMode::Raw => v_flex()
                .flex_1()
                .min_h_0()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(Editor::new(&self.raw).h(relative(1.))),
                )
                .when(is_json, |this| this.child(self.render_json_error(cx)))
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .gap_2()
            .key_context(BODY_EDITOR_CONTEXT)
            .on_action(
                cx.listener(|this, _: &FormatJsonBody, window, cx| this.format_json(window, cx)),
            )
            .child(toolbar)
            .when_some(self.note.clone(), |this, note| {
                this.child(render_note(note, cx))
            })
            .child(content)
    }
}

impl BodyEditor {
    /// Always takes its line, so the editor doesn't resize when the error comes and goes.
    fn render_json_error(&self, cx: &Context<Self>) -> impl IntoElement {
        let row = h_flex()
            .id("json-error")
            .flex_none()
            .h_4()
            .gap_1p5()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        let Some(error) = self.json_error.clone() else {
            return row;
        };
        row.child(
            Icon::new(IconName::CircleX)
                .xsmall()
                .flex_none()
                .text_color(cx.theme().danger),
        )
        .child(div().min_w_0().truncate().child(error.clone()))
        .tooltip(move |window, cx| Tooltip::new(error.clone()).build(window, cx))
    }
}

fn render_note(note: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .gap_1p5()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(IconName::Info).small().flex_none())
        .child(note)
}
