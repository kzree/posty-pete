use std::{cell::RefCell, time::Duration};

use domain::{
    json_body::{self, JsonError},
    request::{Body, RawKind, RawText},
};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
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
    json_check: JsonCheck,
    /// Why the loaded body is empty; dropped when the body is edited or replaced.
    note: Option<SharedString>,
    snapshot: BodySnapshot,
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

        let sub = cx.subscribe(
            &raw_kind,
            |this, _, event: &SelectEvent<Vec<RawKindItem>>, cx| {
                if let SelectEvent::Confirm(Some(kind)) = event {
                    let language = editor_language(*kind);
                    this.raw
                        .update(cx, |raw, cx| raw.set_highlighter(language, cx));
                    this.schedule_json_check(Duration::ZERO, cx);
                    this.edited(cx);
                }
            },
        );
        let raw_sub = cx.subscribe(&raw, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.schedule_json_check(JSON_CHECK_DELAY, cx);
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
            json_check: JsonCheck::default(),
            note: None,
            snapshot: BodySnapshot::default(),
            _subscriptions: vec![sub, raw_sub, form_sub],
        }
    }

    pub fn show_note(&mut self, note: SharedString, cx: &mut Context<Self>) {
        self.note = Some(note);
        cx.notify();
    }

    fn set_mode(&mut self, mode: BodyMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.schedule_json_check(Duration::ZERO, cx);
        self.edited(cx);
        cx.notify();
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        self.snapshot.invalidate();
        if self.note.take().is_some() {
            cx.notify();
        }
        cx.emit(Edited);
    }

    fn is_json(&self, cx: &App) -> bool {
        self.mode == BodyMode::Raw
            && self.raw_kind.read(cx).selected_value() == Some(&RawKind::Json)
    }

    /// Checks the body's JSON on a background thread once `delay` passes
    /// without another call.
    fn schedule_json_check(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.json_check.task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((revision, text))) = this.update(cx, |this, cx| this.start_json_check(cx))
            else {
                return;
            };
            let error = cx
                .background_spawn(async move { json_body::validate(&text).map(error_note) })
                .await;
            this.update(cx, |this, cx| {
                if this
                    .json_check
                    .apply(revision, this.snapshot.revision, error)
                {
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// The text to check and its revision, or `None` once the error is
    /// cleared because the body isn't checked live.
    fn start_json_check(&mut self, cx: &mut Context<Self>) -> Option<(u64, RawText)> {
        let revision = self.snapshot.revision;
        let text = live_json_text(&self.body(cx));
        if text.is_none() && self.json_check.apply(revision, revision, None) {
            cx.notify();
        }
        text.map(|text| (revision, text))
    }

    /// Formats on this thread and reports errors whatever the body's size.
    fn format_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_json(cx) {
            return;
        }
        let Body::Raw { text, .. } = self.body(cx) else {
            return;
        };
        match json_body::format(&text) {
            Ok(formatted) => self.raw.update(cx, |raw, cx| {
                if formatted != text.as_str() {
                    raw.replace_all(formatted, window, cx);
                }
                raw.focus(window, cx);
            }),
            Err(error) => {
                self.json_check.task = None;
                let revision = self.snapshot.revision;
                if self
                    .json_check
                    .apply(revision, revision, Some(error_note(error)))
                {
                    cx.notify();
                }
            }
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
        self.snapshot.invalidate();
        self.form_fields
            .update(cx, |form, cx| form.set_values(fields, window, cx));
        self.raw_kind.update(cx, |select, cx| {
            select.set_selected_value(&kind, window, cx)
        });
        self.raw.update(cx, |raw, cx| {
            raw.set_highlighter(editor_language(kind), cx);
            raw.set_value(text, window, cx);
        });
        self.seed_if_loaded(body, cx);
        self.schedule_json_check(Duration::ZERO, cx);
        cx.notify();
    }

    /// Shares a raw `body` as the snapshot when the editor holds it unchanged.
    fn seed_if_loaded(&mut self, body: &Body, cx: &App) {
        if let Body::Raw { kind, text } = body
            && *self.raw.read(cx).text() == text.as_str()
            && self.raw_kind.read(cx).selected_value() == Some(kind)
        {
            self.snapshot.seed(body.clone());
        }
    }

    /// Built once per revision of the body; clones share a raw body's text.
    pub fn body(&self, cx: &App) -> Body {
        self.snapshot.get_or_build(|| self.build_body(cx))
    }

    fn build_body(&self, cx: &App) -> Body {
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
                text: self.raw.read(cx).text().to_string().into(),
            },
        }
    }
}

const JSON_CHECK_DELAY: Duration = Duration::from_millis(150);

/// Larger raw JSON bodies are only checked by Format.
const LIVE_JSON_CHECK_LIMIT: usize = 5 * 1024 * 1024;

fn live_json_text(body: &Body) -> Option<RawText> {
    match body {
        Body::Raw {
            kind: RawKind::Json,
            text,
        } if text.len() <= LIVE_JSON_CHECK_LIMIT => Some(text.clone()),
        _ => None,
    }
}

fn error_note(error: JsonError) -> SharedString {
    format!("Invalid JSON: {error}").into()
}

#[derive(Default)]
struct JsonCheck {
    error: Option<SharedString>,
    /// Dropping it cancels the check.
    task: Option<Task<()>>,
}

impl JsonCheck {
    /// Shows the outcome of checking body `revision`, unless the body has
    /// moved on to `current`. Returns whether the shown error changed.
    fn apply(&mut self, revision: u64, current: u64, error: Option<SharedString>) -> bool {
        if revision != current || error == self.error {
            return false;
        }
        self.error = error;
        true
    }
}

/// The body as of the last build, kept until the body changes.
#[derive(Default)]
struct BodySnapshot {
    revision: u64,
    built: RefCell<Option<(u64, Body)>>,
}

impl BodySnapshot {
    /// Called on every change to the raw text, its kind, the form rows or the mode.
    fn invalidate(&mut self) {
        self.revision += 1;
    }

    /// Takes `body` as the current revision's build, so it shares its storage.
    fn seed(&mut self, body: Body) {
        *self.built.get_mut() = Some((self.revision, body));
    }

    fn get_or_build(&self, build: impl FnOnce() -> Body) -> Body {
        let mut built = self.built.borrow_mut();
        match &*built {
            Some((revision, body)) if *revision == self.revision => body.clone(),
            _ => {
                let body = build();
                *built = Some((self.revision, body.clone()));
                body
            }
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
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.set_mode(BodyMode::ALL[*ix], cx)
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
        let Some(error) = self.json_check.error.clone() else {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use domain::request::{KeyValue, RawText};

    use super::*;

    fn raw(text: &str) -> Body {
        Body::Raw {
            kind: RawKind::Json,
            text: text.into(),
        }
    }

    fn raw_text(body: &Body) -> &RawText {
        match body {
            Body::Raw { text, .. } => text,
            _ => panic!("not a raw body"),
        }
    }

    #[test]
    fn a_stale_json_check_is_ignored() {
        let mut check = JsonCheck::default();
        let error: Option<SharedString> = Some("Invalid JSON: trailing comma".into());
        assert!(!check.apply(1, 2, error.clone()));
        assert_eq!(check.error, None);

        assert!(check.apply(2, 2, error.clone()));
        assert!(!check.apply(1, 2, None));
        assert_eq!(check.error, error);

        assert!(!check.apply(2, 2, error.clone()));
        assert!(check.apply(3, 3, None));
        assert_eq!(check.error, None);
    }

    #[test]
    fn large_json_skips_the_live_check_but_not_format() {
        let small = raw("[1 2]");
        assert_eq!(live_json_text(&small).as_deref(), Some("[1 2]"));
        assert!(RawText::ptr_eq(
            &live_json_text(&small).unwrap(),
            raw_text(&small)
        ));

        let text = format!("[1 2]{}", " ".repeat(LIVE_JSON_CHECK_LIMIT));
        let large = raw(&text);
        assert_eq!(live_json_text(&large), None);
        let error = json_body::format(raw_text(&large)).unwrap_err();
        assert_eq!((error.line, error.column), (1, 4));

        let plain = Body::Raw {
            kind: RawKind::Text,
            text: "[1 2]".into(),
        };
        assert_eq!(live_json_text(&plain), None);
        assert_eq!(live_json_text(&Body::None), None);
    }

    #[test]
    fn unchanged_body_shares_the_snapshot() {
        let snapshot = BodySnapshot::default();
        let builds = Cell::new(0);
        let build = || {
            builds.set(builds.get() + 1);
            raw("{\"a\": 1}")
        };
        let first = snapshot.get_or_build(build);
        let second = snapshot.get_or_build(build);
        assert_eq!(builds.get(), 1);
        assert!(RawText::ptr_eq(raw_text(&first), raw_text(&second)));
    }

    #[test]
    fn a_change_builds_a_new_snapshot() {
        let mut snapshot = BodySnapshot::default();
        let first = snapshot.get_or_build(|| raw("{}"));
        snapshot.invalidate();
        let same_text = snapshot.get_or_build(|| raw("{}"));
        assert!(!RawText::ptr_eq(raw_text(&first), raw_text(&same_text)));
        assert_eq!(first, same_text);

        snapshot.invalidate();
        let row = KeyValue {
            enabled: true,
            key: "a".into(),
            value: "1".into(),
        };
        let form = snapshot.get_or_build(|| Body::FormData(vec![row.clone()]));
        assert_eq!(form, Body::FormData(vec![row]));
        assert_eq!(snapshot.get_or_build(|| Body::None), form);

        snapshot.invalidate();
        assert_eq!(snapshot.get_or_build(|| Body::None), Body::None);
    }

    #[test]
    fn a_seeded_snapshot_shares_the_loaded_body_until_a_change() {
        let mut snapshot = BodySnapshot::default();
        let loaded = raw(&"x".repeat(1024 * 1024));
        snapshot.invalidate();
        snapshot.seed(loaded.clone());
        let built = snapshot.get_or_build(|| panic!("seeded body was rebuilt"));
        assert!(RawText::ptr_eq(raw_text(&built), raw_text(&loaded)));

        snapshot.invalidate();
        let edited = snapshot.get_or_build(|| raw("{}"));
        assert_eq!(edited, raw("{}"));
    }

    mod views {
        use domain::request::KeyValue;
        use gpui::{Entity, TestAppContext, VisualTestContext};

        use super::{raw, raw_text};
        use crate::request_editor::body_editor::*;
        use crate::test_support::{open_view, type_into};

        fn editor(cx: &mut TestAppContext) -> (Entity<BodyEditor>, &mut VisualTestContext) {
            open_view(cx, BodyEditor::new)
        }

        fn body(editor: &Entity<BodyEditor>, cx: &mut VisualTestContext) -> Body {
            editor.read_with(cx, |editor, cx| editor.body(cx))
        }

        fn type_raw(editor: &Entity<BodyEditor>, text: &str, cx: &mut VisualTestContext) {
            let raw = editor.read_with(cx, |editor, _| editor.raw.clone());
            raw.update_in(cx, |raw, window, cx| {
                raw.replace_all(text.to_string(), window, cx)
            });
        }

        /// What the kind dropdown does when a kind is picked.
        fn pick_kind(editor: &Entity<BodyEditor>, kind: RawKind, cx: &mut VisualTestContext) {
            let select = editor.read_with(cx, |editor, _| editor.raw_kind.clone());
            select.update_in(cx, |select, window, cx| {
                select.set_selected_value(&kind, window, cx);
                cx.emit(SelectEvent::Confirm(Some(kind)));
            });
        }

        fn set_mode(editor: &Entity<BodyEditor>, mode: BodyMode, cx: &mut VisualTestContext) {
            editor.update(cx, |editor, cx| editor.set_mode(mode, cx));
        }

        fn load(editor: &Entity<BodyEditor>, body: &Body, cx: &mut VisualTestContext) {
            editor.update_in(cx, |editor, window, cx| editor.set_body(body, window, cx));
        }

        fn json_error(editor: &Entity<BodyEditor>, cx: &mut VisualTestContext) -> Option<String> {
            editor.read_with(cx, |editor, _| {
                editor.json_check.error.as_ref().map(|e| e.to_string())
            })
        }

        fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
            KeyValue {
                enabled,
                key: key.into(),
                value: value.into(),
            }
        }

        #[gpui::test]
        fn typing_in_the_raw_body_builds_a_new_snapshot(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            set_mode(&editor, BodyMode::Raw, cx);
            let before = body(&editor, cx);
            assert_eq!(before, raw(""));

            type_raw(&editor, "{\"a\": 1}", cx);
            let typed = body(&editor, cx);
            assert_eq!(typed, raw("{\"a\": 1}"));
            assert!(!RawText::ptr_eq(raw_text(&before), raw_text(&typed)));

            let again = body(&editor, cx);
            assert!(RawText::ptr_eq(raw_text(&typed), raw_text(&again)));
        }

        #[gpui::test]
        fn changing_the_raw_kind_builds_a_new_snapshot(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            set_mode(&editor, BodyMode::Raw, cx);
            type_raw(&editor, "<p>", cx);
            let json = body(&editor, cx);

            pick_kind(&editor, RawKind::Html, cx);
            let html = body(&editor, cx);
            assert_eq!(
                html,
                Body::Raw {
                    kind: RawKind::Html,
                    text: "<p>".into()
                }
            );
            assert!(!RawText::ptr_eq(raw_text(&json), raw_text(&html)));
        }

        #[gpui::test]
        fn switching_mode_builds_a_new_snapshot(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            assert_eq!(body(&editor, cx), Body::None);
            set_mode(&editor, BodyMode::FormData, cx);
            assert_eq!(body(&editor, cx), Body::FormData(Vec::new()));
            set_mode(&editor, BodyMode::UrlEncoded, cx);
            assert_eq!(body(&editor, cx), Body::UrlEncoded(Vec::new()));
            set_mode(&editor, BodyMode::Raw, cx);
            assert_eq!(body(&editor, cx), raw(""));
            set_mode(&editor, BodyMode::None, cx);
            assert_eq!(body(&editor, cx), Body::None);
        }

        #[gpui::test]
        fn form_row_changes_build_a_new_snapshot(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            set_mode(&editor, BodyMode::FormData, cx);
            let form = editor.read_with(cx, |editor, _| editor.form_fields.clone());

            let (key, value) = form.read_with(cx, |form, _| form.row_inputs(0));
            type_into(&key, "a", cx);
            assert_eq!(body(&editor, cx), Body::FormData(vec![row(true, "a", "")]));
            type_into(&value, "1", cx);
            assert_eq!(body(&editor, cx), Body::FormData(vec![row(true, "a", "1")]));

            form.update(cx, |form, cx| form.toggle_row(0, false, cx));
            assert_eq!(
                body(&editor, cx),
                Body::FormData(vec![row(false, "a", "1")])
            );

            form.update(cx, |form, cx| form.remove_row_at(0, cx));
            assert_eq!(body(&editor, cx), Body::FormData(Vec::new()));
        }

        #[gpui::test]
        fn a_loaded_raw_body_is_the_snapshot_until_an_edit(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            let loaded = raw("{\"name\": \"posty\"}");
            load(&editor, &loaded, cx);
            let shown = body(&editor, cx);
            assert!(RawText::ptr_eq(raw_text(&shown), raw_text(&loaded)));

            type_raw(&editor, "{\"name\": \"pete\"}", cx);
            let edited = body(&editor, cx);
            assert_eq!(edited, raw("{\"name\": \"pete\"}"));
            assert!(!RawText::ptr_eq(raw_text(&edited), raw_text(&loaded)));
        }

        #[gpui::test]
        fn a_body_the_editor_doesnt_hold_isnt_seeded(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            load(&editor, &raw("{}"), cx);
            let other_text = raw("[]");
            editor.update(cx, |editor, cx| {
                editor.snapshot.invalidate();
                editor.seed_if_loaded(&other_text, cx);
            });
            assert_eq!(body(&editor, cx), raw("{}"));

            let other_kind = Body::Raw {
                kind: RawKind::Text,
                text: "{}".into(),
            };
            editor.update(cx, |editor, cx| {
                editor.snapshot.invalidate();
                editor.seed_if_loaded(&other_kind, cx);
            });
            let built = body(&editor, cx);
            assert_eq!(built, raw("{}"));
            assert!(!RawText::ptr_eq(raw_text(&built), raw_text(&other_kind)));
        }

        #[gpui::test]
        fn an_invalid_edit_shows_its_error_after_the_delay(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            load(&editor, &raw("{}"), cx);
            cx.run_until_parked();
            assert_eq!(json_error(&editor, cx), None);

            type_raw(&editor, "[1 2]", cx);
            cx.executor().advance_clock(JSON_CHECK_DELAY / 2);
            assert_eq!(json_error(&editor, cx), None);
            cx.executor().advance_clock(JSON_CHECK_DELAY);
            let error = json_error(&editor, cx).expect("error shown");
            assert!(error.starts_with("Invalid JSON"), "{error}");

            type_raw(&editor, "[1, 2]", cx);
            cx.executor().advance_clock(JSON_CHECK_DELAY / 2);
            assert_eq!(json_error(&editor, cx).as_ref(), Some(&error));
            cx.executor().advance_clock(JSON_CHECK_DELAY);
            assert_eq!(json_error(&editor, cx), None);
        }

        #[gpui::test]
        fn a_check_finishing_after_a_change_is_dropped(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            load(&editor, &raw("{}"), cx);
            type_raw(&editor, "[1 2]", cx);
            cx.run_until_parked();

            let scheduler = cx.dispatcher.scheduler().clone();
            scheduler.clock().advance(JSON_CHECK_DELAY);
            // The timer wakes the check on the UI thread, which hands the
            // text to a background task.
            let mut woken = false;
            loop {
                let (foreground, background) = scheduler.pending_task_counts();
                woken |= foreground > 0;
                if woken && foreground == 0 && background > 0 {
                    break;
                }
                assert!(scheduler.tick(), "the check never left the UI thread");
            }
            // A change that schedules no check of its own.
            let form = editor.read_with(cx, |editor, _| editor.form_fields.clone());
            let (key, _) = form.read_with(cx, |form, _| form.row_inputs(0));
            type_into(&key, "a", cx);
            cx.run_until_parked();
            assert_eq!(json_error(&editor, cx), None);
        }

        #[gpui::test]
        fn only_the_last_of_quick_edits_is_checked(cx: &mut TestAppContext) {
            let (editor, cx) = editor(cx);
            load(&editor, &raw("{}"), cx);
            cx.run_until_parked();

            type_raw(&editor, "[1 2]", cx);
            cx.executor().advance_clock(JSON_CHECK_DELAY / 3);
            type_raw(&editor, "{\n  \"a\": 1,\n}", cx);
            cx.executor().advance_clock(JSON_CHECK_DELAY * 2 / 3);
            assert_eq!(json_error(&editor, cx), None, "the first edit's check ran");

            cx.executor().advance_clock(JSON_CHECK_DELAY);
            let expected = error_note(json_body::validate("{\n  \"a\": 1,\n}").unwrap());
            assert_eq!(json_error(&editor, cx), Some(expected.to_string()));
        }
    }
}
