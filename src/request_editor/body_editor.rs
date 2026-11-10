use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder, px, relative,
};
use gpui_component::{
    ActiveTheme, IndexPath, Sizable, h_flex,
    input::{Editor, EditorState, InputEvent},
    scroll::ScrollableElement,
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    tab::TabBar,
    v_flex,
};

use super::Edited;
use crate::{
    domain::request::{Body, RawKind},
    ui::key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
};

#[derive(Clone, Copy, PartialEq)]
enum BodyMode {
    None,
    FormData,
    Raw,
}

impl BodyMode {
    const ALL: [BodyMode; 3] = [Self::None, Self::FormData, Self::Raw];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::FormData => "Form data",
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
    }
}

pub struct BodyEditor {
    mode: BodyMode,
    raw_kind: Entity<SelectState<Vec<RawKindItem>>>,
    raw: Entity<EditorState>,
    form_data: Entity<KeyValueEditor>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Edited> for BodyEditor {}

impl BodyEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = RawKind::ALL.into_iter().map(RawKindItem).collect();
        let raw_kind = cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx));
        let raw =
            cx.new(|cx| EditorState::new(window, cx).language(editor_language(RawKind::default())));
        let form_data = cx.new(|cx| KeyValueEditor::new("field", window, cx));

        let sub = cx.subscribe(
            &raw_kind,
            |this, _, event: &SelectEvent<Vec<RawKindItem>>, cx| {
                if let SelectEvent::Confirm(Some(kind)) = event {
                    let language = editor_language(*kind);
                    this.raw
                        .update(cx, |raw, cx| raw.set_highlighter(language, cx));
                    cx.emit(Edited);
                }
            },
        );
        let raw_sub = cx.subscribe(&raw, |_, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                cx.emit(Edited);
            }
        });
        let form_sub = cx.subscribe(&form_data, |_, _, _: &KeyValueEditorEvent, cx| {
            cx.emit(Edited)
        });

        Self {
            mode: BodyMode::None,
            raw_kind,
            raw,
            form_data,
            _subscriptions: vec![sub, raw_sub, form_sub],
        }
    }

    /// Content of the modes `body` doesn't use is cleared, so switching mode
    /// doesn't bring back text from a previous request.
    pub fn set_body(&mut self, body: &Body, window: &mut Window, cx: &mut Context<Self>) {
        let (mode, fields, kind, text) = match body {
            Body::None => (BodyMode::None, &[][..], RawKind::default(), ""),
            Body::FormData(fields) => (BodyMode::FormData, &fields[..], RawKind::default(), ""),
            Body::Raw { kind, text } => (BodyMode::Raw, &[][..], *kind, text.as_str()),
        };

        self.mode = mode;
        self.form_data
            .update(cx, |form, cx| form.set_values(fields, window, cx));
        self.raw_kind.update(cx, |select, cx| {
            select.set_selected_value(&kind, window, cx)
        });
        self.raw.update(cx, |raw, cx| {
            raw.set_highlighter(editor_language(kind), cx);
            raw.set_value(text.to_string(), window, cx);
        });
        cx.notify();
    }

    pub fn body(&self, cx: &App) -> Body {
        match self.mode {
            BodyMode::None => Body::None,
            BodyMode::FormData => Body::FormData(self.form_data.read(cx).values(cx)),
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
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.mode = BodyMode::ALL[*ix];
                            cx.emit(Edited);
                            cx.notify();
                        }))
                        .children(BodyMode::ALL.map(BodyMode::label)),
                ),
            )
            .when(self.mode == BodyMode::Raw, |this| {
                this.child(div().w(px(100.)).child(Select::new(&self.raw_kind).small()))
            });

        let content = match self.mode {
            BodyMode::None => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("This request has no body.")
                .into_any_element(),
            BodyMode::FormData => div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(self.form_data.clone())
                .into_any_element(),
            BodyMode::Raw => div()
                .flex_1()
                .min_h_0()
                .child(Editor::new(&self.raw).h(relative(1.)))
                .into_any_element(),
        };

        v_flex().size_full().gap_2().child(toolbar).child(content)
    }
}
