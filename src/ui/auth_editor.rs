use gpui::{
    App, AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, IndexPath, Sizable,
    form::{field, v_form},
    h_flex,
    input::{Input, InputEvent, InputState},
    link::Link,
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    tab::TabBar,
    v_flex,
};

use crate::domain::{
    auth::{AuthSourceLink, InheritNote},
    request::{ApiKeyLocation, Auth},
};

/// What the editor offers and says, depending on whose auth it edits.
#[derive(Clone, Copy)]
pub struct AuthEditorOptions {
    /// The line shown for "Inherit"; `None` doesn't offer it.
    pub inherit: Option<&'static str>,
    /// The line shown for "No auth".
    pub none: &'static str,
}

pub enum AuthEditorEvent {
    Changed,
    /// The link in the Inherit note was clicked.
    OpenSource(AuthSourceLink),
}

#[derive(Clone, Copy, PartialEq)]
enum AuthKind {
    Inherit,
    None,
    ApiKey,
    Basic,
    Bearer,
}

impl AuthKind {
    const ALL: [AuthKind; 5] = [
        Self::Inherit,
        Self::None,
        Self::ApiKey,
        Self::Basic,
        Self::Bearer,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Inherit => "Inherit",
            Self::None => "No auth",
            Self::ApiKey => "API key",
            Self::Basic => "Basic auth",
            Self::Bearer => "Bearer token",
        }
    }
}

#[derive(Clone)]
struct AuthKindItem(AuthKind);

impl SearchableListItem for AuthKindItem {
    type Value = AuthKind;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn value(&self) -> &AuthKind {
        &self.0
    }
}

pub struct AuthEditor {
    options: AuthEditorOptions,
    /// Replaces the options' Inherit line when set.
    inherit_note: Option<InheritNote>,
    kind: AuthKind,
    kind_select: Entity<SelectState<Vec<AuthKindItem>>>,
    api_key_name: Entity<InputState>,
    api_key_value: Entity<InputState>,
    api_key_location: ApiKeyLocation,
    username: Entity<InputState>,
    password: Entity<InputState>,
    token: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<AuthEditorEvent> for AuthEditor {}

impl AuthEditor {
    pub fn new(options: AuthEditorOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = AuthKind::ALL
            .into_iter()
            .filter(|kind| options.inherit.is_some() || *kind != AuthKind::Inherit)
            .map(AuthKindItem)
            .collect();
        let kind_select = cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx));

        let api_key_name = cx.new(|cx| InputState::new(window, cx).placeholder("Key"));
        let api_key_value = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Token")
                .masked(true)
        });

        let sub = cx.subscribe(
            &kind_select,
            |this, _, event: &SelectEvent<Vec<AuthKindItem>>, cx| {
                if let SelectEvent::Confirm(Some(kind)) = event {
                    this.kind = *kind;
                    cx.emit(AuthEditorEvent::Changed);
                    cx.notify();
                }
            },
        );
        let mut subscriptions = vec![sub];
        for input in [&api_key_name, &api_key_value, &username, &password, &token] {
            subscriptions.push(cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.emit(AuthEditorEvent::Changed);
                }
            }));
        }

        Self {
            kind: Self::default_kind(options),
            options,
            inherit_note: None,
            kind_select,
            api_key_name,
            api_key_value,
            api_key_location: ApiKeyLocation::default(),
            username,
            password,
            token,
            _subscriptions: subscriptions,
        }
    }

    fn default_kind(options: AuthEditorOptions) -> AuthKind {
        if options.inherit.is_some() {
            AuthKind::Inherit
        } else {
            AuthKind::None
        }
    }

    /// Inputs of the auth kinds `auth` doesn't use are cleared. Inherit
    /// shows as "No auth" where it isn't offered.
    pub fn set_auth(&mut self, auth: &Auth, window: &mut Window, cx: &mut Context<Self>) {
        let empty = String::new();
        let (kind, api_key_name, api_key_value, username, password, token) = match auth {
            Auth::Inherit => (
                Self::default_kind(self.options),
                &empty,
                &empty,
                &empty,
                &empty,
                &empty,
            ),
            Auth::None => (AuthKind::None, &empty, &empty, &empty, &empty, &empty),
            Auth::ApiKey { key, value, .. } => {
                (AuthKind::ApiKey, key, value, &empty, &empty, &empty)
            }
            Auth::Basic { username, password } => {
                (AuthKind::Basic, &empty, &empty, username, password, &empty)
            }
            Auth::Bearer { token } => (AuthKind::Bearer, &empty, &empty, &empty, &empty, token),
        };

        for (input, text) in [
            (&self.api_key_name, api_key_name),
            (&self.api_key_value, api_key_value),
            (&self.username, username),
            (&self.password, password),
            (&self.token, token),
        ] {
            input.update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        }

        self.api_key_location = match auth {
            Auth::ApiKey { location, .. } => *location,
            _ => ApiKeyLocation::default(),
        };
        self.kind = kind;
        self.kind_select.update(cx, |select, cx| {
            select.set_selected_value(&kind, window, cx)
        });
        cx.notify();
    }

    /// Shows `note` for Inherit instead of the options' line; `None` restores it.
    pub fn set_inherit_note(&mut self, note: Option<InheritNote>, cx: &mut Context<Self>) {
        if self.inherit_note != note {
            self.inherit_note = note;
            cx.notify();
        }
    }

    pub fn auth(&self, cx: &App) -> Auth {
        let text = |input: &Entity<InputState>| input.read(cx).value().to_string();
        match self.kind {
            AuthKind::Inherit => Auth::Inherit,
            AuthKind::None => Auth::None,
            AuthKind::ApiKey => Auth::ApiKey {
                key: text(&self.api_key_name),
                value: text(&self.api_key_value),
                location: self.api_key_location,
            },
            AuthKind::Basic => Auth::Basic {
                username: text(&self.username),
                password: text(&self.password),
            },
            AuthKind::Bearer => Auth::Bearer {
                token: text(&self.token),
            },
        }
    }
}

impl AuthEditor {
    fn render_inherit_note(&self, note: &InheritNote, cx: &mut Context<Self>) -> impl IntoElement {
        let text = |text: &str| SharedString::from(text.to_string());
        h_flex()
            .flex_wrap()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text(&note.prefix))
            .when_some(note.link.clone(), |this, (link, name)| {
                this.child(
                    Link::new("auth-inherit-source")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(AuthEditorEvent::OpenSource(link))
                        }))
                        .child(name),
                )
            })
            .child(text(&note.suffix))
    }
}

impl Render for AuthEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;

        let type_row = h_flex()
            .flex_none()
            .gap_2()
            .child(div().text_sm().text_color(muted).child("Type"))
            .child(
                div()
                    .w(px(140.))
                    .child(Select::new(&self.kind_select).small()),
            );

        let fields = match self.kind {
            AuthKind::Inherit => match &self.inherit_note {
                Some(note) => self.render_inherit_note(note, cx).into_any_element(),
                None => div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.options.inherit.unwrap_or_default())
                    .into_any_element(),
            },
            AuthKind::None => div()
                .text_sm()
                .text_color(muted)
                .child(self.options.none)
                .into_any_element(),
            AuthKind::ApiKey => v_form()
                .child(
                    field()
                        .label("Key")
                        .child(Input::new(&self.api_key_name).small()),
                )
                .child(
                    field()
                        .label("Value")
                        .child(Input::new(&self.api_key_value).small()),
                )
                .child(
                    field().label("Add to").child(
                        h_flex().child(
                            TabBar::new("api-key-location")
                                .segmented()
                                .small()
                                .selected_index(
                                    ApiKeyLocation::ALL
                                        .iter()
                                        .position(|l| *l == self.api_key_location)
                                        .unwrap_or_default(),
                                )
                                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                    let location = ApiKeyLocation::ALL[*ix];
                                    if location != this.api_key_location {
                                        this.api_key_location = location;
                                        cx.emit(AuthEditorEvent::Changed);
                                        cx.notify();
                                    }
                                }))
                                .children(ApiKeyLocation::ALL.map(ApiKeyLocation::label)),
                        ),
                    ),
                )
                .into_any_element(),
            AuthKind::Basic => v_form()
                .child(
                    field()
                        .label("Username")
                        .child(Input::new(&self.username).small()),
                )
                .child(
                    field()
                        .label("Password")
                        .child(Input::new(&self.password).small().mask_toggle()),
                )
                .into_any_element(),
            AuthKind::Bearer => v_form()
                .child(
                    field()
                        .label("Token")
                        .child(Input::new(&self.token).small().mask_toggle()),
                )
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .gap_3()
            .child(type_row)
            .child(div().max_w(px(480.)).child(fields))
    }
}
