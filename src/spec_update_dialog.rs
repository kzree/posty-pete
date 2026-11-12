use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::{DialogAction, DialogClose, DialogFooter},
    h_flex,
    spinner::Spinner,
    v_flex,
};
use uuid::Uuid;

use domain::{
    collection::{Collection, SourceLocation},
    http::Client,
    openapi::{
        ImportResult,
        load::{self, FetchGuard, LoadError},
        update::{self, AuthChange, Choices, RequestChange, UpdatePlan},
    },
};

use crate::ui::method_label::method_label;

/// What the dialog starts from.
pub enum SpecUpdateStart {
    /// Loads the spec from the collection's source.
    Reload,
    /// A spec already loaded from a new source.
    Loaded(Box<ImportResult>),
}

/// What "Update" hands over: the fresh import and the choices made.
pub struct SpecUpdate {
    pub fresh: Collection,
    pub choices: Choices,
}

enum Status {
    Loading,
    Planning,
    Failed(SharedString),
    Ready {
        fresh: Box<Collection>,
        plan: Box<UpdatePlan>,
        choices: Choices,
    },
}

/// Dropping it aborts the load and stops the plan from landing.
struct PendingLoad {
    _task: Task<()>,
    _guard: Option<FetchGuard>,
}

/// Loads an imported collection's spec and previews what updating changes.
struct SpecUpdateDialog {
    current: Arc<Collection>,
    location: Option<SourceLocation>,
    client: Result<Client, SharedString>,
    status: Status,
    pending: Option<PendingLoad>,
}

impl SpecUpdateDialog {
    fn new(
        current: Arc<Collection>,
        start: SpecUpdateStart,
        client: Result<Client, SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            location: current
                .source
                .as_ref()
                .map(|source| source.location.clone()),
            current,
            client,
            status: Status::Loading,
            pending: None,
        };
        match start {
            SpecUpdateStart::Reload => this.load(cx),
            SpecUpdateStart::Loaded(result) => {
                this.location = result
                    .collection
                    .source
                    .as_ref()
                    .map(|source| source.location.clone());
                this.plan(Ok(*result), cx);
            }
        }
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(location) = self.location.clone() else {
            return self.fail("This collection wasn't imported from a spec.".into(), cx);
        };
        let client = self.client.clone();
        let handle = match load::spawn_load(&location, || client.map_err(String::from)) {
            Ok(handle) => handle,
            Err(error) => return self.fail(error.into(), cx),
        };
        let guard = handle.guard();
        self.status = Status::Loading;
        let task = cx.spawn(async move |this, cx| {
            if let Some(result) = handle.result().await {
                this.update(cx, |this, cx| this.plan(result, cx)).ok();
            }
        });
        self.pending = Some(PendingLoad {
            _task: task,
            _guard: Some(guard),
        });
        cx.notify();
    }

    /// Plans the update off the UI thread, since hashing a big collection takes a while.
    fn plan(&mut self, result: Result<ImportResult, LoadError>, cx: &mut Context<Self>) {
        let fresh = match result {
            Ok(result) => result.collection,
            Err(error) => return self.fail(error.to_string().into(), cx),
        };
        self.status = Status::Planning;
        let current = self.current.clone();
        let task = cx.spawn(async move |this, cx| {
            let (fresh, plan) = cx
                .background_spawn(async move {
                    let plan = update::plan(&current, &fresh);
                    (fresh, plan)
                })
                .await;
            this.update(cx, |this, cx| {
                this.pending = None;
                this.status = Status::Ready {
                    choices: plan.default_choices(),
                    fresh: Box::new(fresh),
                    plan: Box::new(plan),
                };
                cx.notify();
            })
            .ok();
        });
        self.pending = Some(PendingLoad {
            _task: task,
            _guard: None,
        });
        cx.notify();
    }

    fn fail(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.pending = None;
        self.status = Status::Failed(message);
        cx.notify();
    }

    fn cancel(&mut self) {
        self.pending = None;
    }

    fn can_update(&self) -> bool {
        matches!(&self.status, Status::Ready { plan, .. } if plan.has_changes())
    }

    fn is_up_to_date(&self) -> bool {
        matches!(&self.status, Status::Ready { plan, .. } if !plan.has_changes())
    }

    /// `None` keeps the dialog open; `Some(None)` closes it with nothing to apply.
    fn confirm(&mut self) -> Option<Option<SpecUpdate>> {
        if self.is_up_to_date() {
            return Some(None);
        }
        if !self.can_update() {
            return None;
        }
        match std::mem::replace(&mut self.status, Status::Loading) {
            Status::Ready { fresh, choices, .. } => Some(Some(SpecUpdate {
                fresh: *fresh,
                choices,
            })),
            _ => None,
        }
    }

    fn toggle(&mut self, toggle: Toggle, checked: bool, cx: &mut Context<Self>) {
        let Status::Ready { choices, .. } = &mut self.status else {
            return;
        };
        fn set<T: Eq + std::hash::Hash>(
            set: &mut std::collections::HashSet<T>,
            value: T,
            on: bool,
        ) {
            if on {
                set.insert(value);
            } else {
                set.remove(&value);
            }
        }
        match toggle {
            Toggle::UseSpec(id) => set(&mut choices.use_spec, id, checked),
            Toggle::Remove(id) => set(&mut choices.remove, id, checked),
            Toggle::RemoveVariable(key) => set(&mut choices.remove_variables, key, checked),
        }
        cx.notify();
    }

    fn render_location(&self, cx: &App) -> Option<impl IntoElement> {
        let location = self.location.as_ref()?;
        Some(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .min_w_0()
                .truncate()
                .child(format!("From {}", location.label())),
        )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match &self.status {
            Status::Loading | Status::Planning => {
                let text = match (&self.status, &self.location) {
                    (Status::Planning, _) => "Comparing with the collection…",
                    (_, Some(SourceLocation::Url(_))) => "Fetching the spec…",
                    _ => "Reading the spec…",
                };
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(muted)
                    .child(Spinner::new().small())
                    .child(text)
                    .into_any_element()
            }
            Status::Failed(message) => v_flex()
                .items_start()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(message.clone()),
                )
                .child(
                    Button::new("spec-update-retry")
                        .small()
                        .outline()
                        .label("Try again")
                        .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .into_any_element(),
            Status::Ready { plan, .. } if !plan.has_changes() => v_flex()
                .gap_1()
                .text_sm()
                .child("Already up to date.")
                .when(!plan.kept.is_empty(), |this| {
                    this.child(div().text_color(muted).child(kept_text(plan.kept.len())))
                })
                .into_any_element(),
            Status::Ready { plan, choices, .. } => self.render_plan(plan, choices, cx),
        }
    }

    fn render_plan(
        &self,
        plan: &UpdatePlan,
        choices: &Choices,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut sections: Vec<AnyElement> = Vec::new();
        if let Some(label) = &plan.source {
            sections.push(
                div()
                    .text_sm()
                    .min_w_0()
                    .truncate()
                    .child(format!("Source changes to {label}"))
                    .into_any_element(),
            );
        }
        if let Some((old, new)) = &plan.version {
            sections.push(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("Version {old} → {new}"))
                    .into_any_element(),
            );
        }
        if !plan.added.is_empty() {
            let rows = plan.added.iter().map(|change| request_row(change, cx));
            sections.push(section("Added", plan.added.len(), None, rows, cx));
        }
        if !plan.updated.is_empty() {
            let rows = plan.updated.iter().map(|change| request_row(change, cx));
            sections.push(section("Updated", plan.updated.len(), None, rows, cx));
        }
        if !plan.conflicts.is_empty() {
            let rows = plan.conflicts.iter().enumerate().map(|(ix, change)| {
                let id = change.id;
                request_row(change, cx)
                    .child(
                        Checkbox::new(("spec-update-use-spec", ix))
                            .small()
                            .label("Use the spec's version")
                            .checked(choices.use_spec.contains(&id))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.toggle(Toggle::UseSpec(id), *checked, cx)
                            })),
                    )
                    .into_any_element()
            });
            sections.push(section(
                "Conflicts",
                plan.conflicts.len(),
                Some("You and the spec both changed these. Your version stays unless you choose the spec's."),
                rows,
                cx,
            ));
        }
        if !plan.removed.is_empty() {
            let rows = plan.removed.iter().enumerate().map(|(ix, removal)| {
                let id = removal.id;
                let change = RequestChange {
                    id,
                    name: removal.name.clone(),
                    method: removal.method,
                    note: removal.edited.then(|| "Edited".to_string()),
                };
                request_row(&change, cx)
                    .child(
                        Checkbox::new(("spec-update-remove", ix))
                            .small()
                            .label("Remove")
                            .checked(choices.remove.contains(&id))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.toggle(Toggle::Remove(id), *checked, cx)
                            })),
                    )
                    .into_any_element()
            });
            sections.push(section(
                "Removed from the spec",
                plan.removed.len(),
                Some("Requests you edited are kept unless you remove them. Kept requests are no longer updated."),
                rows,
                cx,
            ));
        }
        if !plan.kept.is_empty() {
            let rows = plan.kept.iter().map(|change| request_row(change, cx));
            sections.push(section(
                "Kept (your edits)",
                plan.kept.len(),
                Some("The spec didn't change these, so your edits stay."),
                rows,
                cx,
            ));
        }
        if let Some(variables) = self.render_variables(plan, choices, cx) {
            sections.push(variables);
        }
        let auth = match plan.auth {
            AuthChange::Unchanged => None,
            AuthChange::Updated => Some("The collection's auth changes to the spec's."),
            AuthChange::KeptYours => {
                Some("The spec changed the collection's auth, but so did you. Yours stays.")
            }
        };
        if let Some(text) = auth {
            sections.push(
                v_flex()
                    .gap_1()
                    .child(section_title("Authorization"))
                    .child(div().text_sm().child(text))
                    .into_any_element(),
            );
        }
        v_flex()
            .id("spec-update-plan")
            .gap_4()
            .max_h(px(380.))
            .overflow_y_scroll()
            .children(sections)
            .into_any_element()
    }

    fn render_variables(
        &self,
        plan: &UpdatePlan,
        choices: &Choices,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if plan.variables_added.is_empty()
            && plan.variables_updated.is_empty()
            && plan.variables_unused.is_empty()
        {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        let line = |label: &'static str, keys: &[String]| {
            (!keys.is_empty()).then(|| {
                h_flex()
                    .gap_2()
                    .text_sm()
                    .child(div().flex_none().text_color(muted).child(label))
                    .child(div().min_w_0().child(keys.join(", ")))
            })
        };
        let unused = plan.variables_unused.iter().enumerate().map(|(ix, key)| {
            let toggle_key = key.clone();
            h_flex()
                .gap_2()
                .text_sm()
                .child(div().flex_1().min_w_0().truncate().child(key.clone()))
                .child(
                    Checkbox::new(("spec-update-variable", ix))
                        .small()
                        .label("Remove")
                        .checked(choices.remove_variables.contains(key))
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.toggle(Toggle::RemoveVariable(toggle_key.clone()), *checked, cx)
                        })),
                )
        });
        Some(
            v_flex()
                .gap_1()
                .child(section_title("Variables"))
                .children(line("Added", &plan.variables_added))
                .children(line("Updated", &plan.variables_updated))
                .when(!plan.variables_unused.is_empty(), |this| {
                    this.child(
                        div()
                            .mt_1()
                            .text_sm()
                            .text_color(muted)
                            .child("No longer needed by the spec. Kept unless you remove them."),
                    )
                    .children(unused)
                })
                .into_any_element(),
        )
    }
}

enum Toggle {
    UseSpec(Uuid),
    Remove(Uuid),
    RemoveVariable(String),
}

fn kept_text(count: usize) -> String {
    match count {
        1 => "Your edits to 1 request are kept.".to_string(),
        n => format!("Your edits to {n} requests are kept."),
    }
}

fn section_title(title: &'static str) -> impl IntoElement {
    div().text_sm().font_weight(FontWeight::MEDIUM).child(title)
}

fn section(
    title: &'static str,
    count: usize,
    hint: Option<&'static str>,
    rows: impl Iterator<Item = impl IntoElement>,
    cx: &App,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .child(section_title(title))
                .child(div().text_sm().text_color(muted).child(count.to_string())),
        )
        .children(hint.map(|hint| div().text_xs().text_color(muted).child(hint)))
        .children(rows)
        .into_any_element()
}

fn request_row(change: &RequestChange, cx: &App) -> gpui::Div {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .gap_2()
        .text_sm()
        .child(method_label(change.method, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(change.name.clone()),
        )
        .children(
            change
                .note
                .clone()
                .map(|note| div().flex_none().text_color(muted).child(note)),
        )
}

impl Render for SpecUpdateDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .children(self.render_location(cx))
            .child(self.render_status(cx))
    }
}

/// A view of its own, so the buttons follow the dialog's state.
struct SpecUpdateFooter {
    dialog: Entity<SpecUpdateDialog>,
    _subscription: Subscription,
}

impl Render for SpecUpdateFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = self.dialog.read(cx);
        if dialog.is_up_to_date() {
            return DialogFooter::new().child(
                DialogAction::new().child(Button::new("spec-update-done").primary().label("Done")),
            );
        }
        DialogFooter::new()
            .child(DialogClose::new().child(Button::new("spec-update-cancel").label("Cancel")))
            .child(
                DialogAction::new().child(
                    Button::new("spec-update-confirm")
                        .primary()
                        .label("Update")
                        .disabled(!dialog.can_update()),
                ),
            )
    }
}

/// Opens the update dialog for `current`, an imported collection. `client`
/// fetches a URL source; `on_update` gets the update once the user confirms.
pub fn open_spec_update_dialog(
    current: Arc<Collection>,
    start: SpecUpdateStart,
    client: Result<Client, SharedString>,
    on_update: impl Fn(SpecUpdate, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title = format!("Update “{}” from spec", current.name);
    let dialog = cx.new(|cx| SpecUpdateDialog::new(current, start, client, cx));
    let footer = cx.new(|cx| SpecUpdateFooter {
        _subscription: cx.observe(&dialog, |_, _, cx| cx.notify()),
        dialog: dialog.clone(),
    });
    let on_update = Rc::new(on_update);

    window.open_dialog(cx, move |modal, _, _| {
        let (for_ok, for_close, on_update) = (dialog.clone(), dialog.clone(), on_update.clone());
        modal
            .title(title.clone())
            .w(px(560.))
            .child(dialog.clone())
            .footer(footer.clone())
            .on_ok(
                move |_, window, cx| match for_ok.update(cx, |dialog, _| dialog.confirm()) {
                    None => false,
                    Some(None) => true,
                    Some(Some(update)) => {
                        on_update(update, window, cx);
                        true
                    }
                },
            )
            .on_close(move |_, _, cx| for_close.update(cx, |dialog, _| dialog.cancel()))
    });
}
