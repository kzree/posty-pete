mod auth_editor;
mod body_editor;
mod method_item;
mod tabs;
mod unresolved_hint;
mod url_bar;

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder,
};
use gpui_component::{
    IndexPath, StyledExt,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    select::{SelectEvent, SelectState},
    v_flex,
};

use crate::{
    domain::request::Request,
    ui::key_value_editor::{KeyValueEditor, KeyValueEditorEvent},
};
use auth_editor::AuthEditor;
use body_editor::BodyEditor;
use method_item::MethodItem;
use tabs::RequestTab;
use unresolved_hint::unresolved_hint;

pub enum RequestEditorEvent {
    SendRequested(Request),
    CopyAsCurl(Request),
    Save,
    /// A field of the request or its name changed.
    Edited,
}

/// Emitted by the body and auth editors when their content changes.
struct Edited;

pub struct RequestEditor {
    name: Entity<InputState>,
    /// The stored name of the linked saved request; `None` while unlinked.
    saved_name: Option<String>,
    method: Entity<SelectState<Vec<MethodItem>>>,
    url: Entity<InputState>,
    active_tab: RequestTab,
    params: Entity<KeyValueEditor>,
    headers: Entity<KeyValueEditor>,
    body: Entity<BodyEditor>,
    auth: Entity<AuthEditor>,
    loading: bool,
    /// Whether the linked saved request has unsaved changes.
    dirty: bool,
    unresolved_hint: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RequestEditorEvent> for RequestEditor {}

impl RequestEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let method =
            cx.new(|cx| SelectState::new(MethodItem::all(), Some(IndexPath::new(0)), window, cx));
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://..."));
        url.update(cx, |input, cx| input.focus(window, cx));

        let sub = cx.subscribe_in(
            &url,
            window,
            |this, _url, event: &InputEvent, _window, cx| match event {
                InputEvent::PressEnter { .. } => this.send(cx),
                InputEvent::Change => cx.emit(RequestEditorEvent::Edited),
                _ => {}
            },
        );

        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Request name"));
        let name_sub = cx.subscribe(&name, |_, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => cx.emit(RequestEditorEvent::Save),
            InputEvent::Change => cx.emit(RequestEditorEvent::Edited),
            _ => {}
        });

        let params = cx.new(|cx| KeyValueEditor::new("parameter", window, cx));
        let headers = cx.new(|cx| KeyValueEditor::new("header", window, cx));
        let body = cx.new(|cx| BodyEditor::new(window, cx));
        let auth = cx.new(|cx| AuthEditor::new(window, cx));

        let method_sub = cx.subscribe(&method, |_, _, _: &SelectEvent<Vec<MethodItem>>, cx| {
            cx.emit(RequestEditorEvent::Edited)
        });
        let mut subscriptions = vec![sub, name_sub, method_sub];
        for key_values in [&params, &headers] {
            subscriptions.push(
                cx.subscribe(key_values, |_, _, _: &KeyValueEditorEvent, cx| {
                    cx.emit(RequestEditorEvent::Edited)
                }),
            );
        }
        subscriptions.push(cx.subscribe(&body, |_, _, _: &Edited, cx| {
            cx.emit(RequestEditorEvent::Edited)
        }));
        subscriptions.push(cx.subscribe(&auth, |_, _, _: &Edited, cx| {
            cx.emit(RequestEditorEvent::Edited)
        }));

        Self {
            name,
            saved_name: None,
            method,
            url,
            active_tab: RequestTab::Params,
            params,
            headers,
            body,
            auth,
            loading: false,
            dirty: false,
            unresolved_hint: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        self.loading = loading;
        cx.notify();
    }

    pub fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        self.dirty = dirty;
        cx.notify();
    }

    /// Shows the variables the request references but that don't resolve.
    /// `resolvable` is whether there's a collection to resolve against.
    pub fn set_unresolved_variables(
        &mut self,
        names: &[String],
        resolvable: bool,
        cx: &mut Context<Self>,
    ) {
        let hint = unresolved_hint(names, resolvable).map(SharedString::from);
        if hint != self.unresolved_hint {
            self.unresolved_hint = hint;
            cx.notify();
        }
    }

    /// Links the editor to a saved request with this stored name (`None` unlinks).
    /// When only the stored name changes, an unsaved edit of the name field is kept.
    pub fn set_saved_name(
        &mut self,
        saved_name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = self.name.read(cx).value();
        let current = value.trim();
        let replace = match (&self.saved_name, &saved_name) {
            (Some(old), Some(new)) => old != new && (current == old || current == new),
            _ => true,
        };
        if replace {
            self.set_name_value(saved_name.clone().unwrap_or_default(), window, cx);
        }
        self.saved_name = saved_name;
        cx.notify();
    }

    /// Links the editor to a saved request named `name`, replacing the name field.
    pub fn link(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_name_value(name.clone(), window, cx);
        self.saved_name = Some(name);
        cx.notify();
    }

    /// The trimmed name, or `None` if it's empty.
    pub fn name(&self, cx: &App) -> Option<String> {
        let value = self.name.read(cx).value();
        let name = value.trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Trims the name field, or puts the stored name back if it's empty.
    pub fn tidy_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(value) = self.name(cx).or_else(|| self.saved_name.clone()) {
            self.set_name_value(value, window, cx);
        }
    }

    fn set_name_value(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.name.read(cx).value() != value {
            self.name
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    pub fn focus_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.url.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Loads `request` unlinked; call `set_saved_name` afterwards to link it.
    pub fn load_request(&mut self, request: &Request, window: &mut Window, cx: &mut Context<Self>) {
        self.set_saved_name(None, window, cx);
        self.method.update(cx, |method, cx| {
            method.set_selected_value(&request.method, window, cx)
        });
        self.url
            .update(cx, |url, cx| url.set_value(request.url.clone(), window, cx));
        self.params.update(cx, |params, cx| {
            params.set_values(&request.params, window, cx)
        });
        self.headers.update(cx, |headers, cx| {
            headers.set_values(&request.headers, window, cx)
        });
        self.body
            .update(cx, |body, cx| body.set_body(&request.body, window, cx));
        self.auth
            .update(cx, |auth, cx| auth.set_auth(&request.auth, window, cx));
        cx.notify();
    }

    pub fn send(&mut self, cx: &mut Context<Self>) {
        if self.input_is_empty(cx) {
            return;
        }
        let req = self.build_request(cx);
        cx.emit(RequestEditorEvent::SendRequested(req));
    }

    fn input_is_empty(&self, cx: &App) -> bool {
        self.url.read(cx).value().to_string().is_empty()
    }

    pub fn build_request(&self, cx: &App) -> Request {
        Request {
            method: self
                .method
                .read(cx)
                .selected_value()
                .copied()
                .unwrap_or_default(),
            url: self.url.read(cx).value().into(),
            params: self.params.read(cx).values(cx),
            headers: self.headers.read(cx).values(cx),
            body: self.body.read(cx).body(cx),
            auth: self.auth.read(cx).auth(cx),
        }
    }
}

impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.active_tab {
            RequestTab::Params => div()
                .size_full()
                .overflow_y_scrollbar()
                .child(self.params.clone())
                .into_any_element(),
            RequestTab::Headers => div()
                .size_full()
                .overflow_y_scrollbar()
                .child(self.headers.clone())
                .into_any_element(),
            RequestTab::Body => self.body.clone().into_any_element(),
            RequestTab::Authorization => self.auth.clone().into_any_element(),
        };

        v_flex()
            .size_full()
            .p_4()
            .gap_2()
            .when(self.saved_name.is_some(), |this| {
                this.child(
                    Input::new(&self.name)
                        .appearance(false)
                        .text_lg()
                        .font_semibold()
                        .aria_label("Request name"),
                )
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(self.render_url_bar(cx))
                    .child(self.render_unresolved_hint(cx)),
            )
            .child(self.render_tab_bar(cx))
            .child(div().flex_1().min_h_0().child(content))
    }
}
