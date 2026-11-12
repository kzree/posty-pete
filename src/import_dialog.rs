use std::{path::PathBuf, rc::Rc};

use domain::{
    collection::SourceLocation,
    http::Client,
    openapi::{
        ImportResult,
        load::{self, FetchGuard, FetchHandle, LoadError, Preview},
    },
};
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogAction, DialogClose, DialogFooter},
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    tab::TabBar,
    v_flex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    File,
    Url,
}

impl Mode {
    const ALL: [Self; 2] = [Self::File, Self::Url];

    fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Url => "URL",
        }
    }
}

enum Status {
    Idle,
    Loading,
    Loaded {
        result: Box<ImportResult>,
        preview: Preview,
    },
    Failed(SharedString),
}

/// A load in flight. Dropping it aborts the load and stops `finish` from
/// running.
struct PendingLoad {
    _task: Task<()>,
    _guard: FetchGuard,
}

/// What the dialog is for: importing a new collection, or picking another
/// source for an imported one.
pub struct ImportDialogOptions {
    pub title: &'static str,
    pub confirm_label: &'static str,
    /// Picks the mode, and fills in the URL.
    pub initial: Option<SourceLocation>,
}

impl ImportDialogOptions {
    pub fn import() -> Self {
        Self {
            title: "Import from OpenAPI",
            confirm_label: "Import",
            initial: None,
        }
    }
}

/// Picks an OpenAPI or Swagger spec from a file or a URL and previews the
/// collection it makes.
struct ImportDialog {
    mode: Mode,
    path: Option<PathBuf>,
    url: Entity<InputState>,
    /// Why there's no client when the request settings are invalid.
    client: Result<Client, SharedString>,
    status: Status,
    pending: Option<PendingLoad>,
    _subscription: Subscription,
}

impl ImportDialog {
    fn new(
        client: Result<Client, SharedString>,
        initial: Option<&SourceLocation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (mode, initial_url) = match initial {
            Some(SourceLocation::Url(url)) => (Mode::Url, url.clone()),
            Some(SourceLocation::File(_)) | None => (Mode::File, String::new()),
        };
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://example.com/openapi.json")
                .default_value(initial_url)
        });
        let subscription = cx.subscribe(&url, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.reset(cx);
            }
        });
        Self {
            mode,
            path: None,
            url,
            client,
            status: Status::Idle,
            pending: None,
            _subscription: subscription,
        }
    }

    fn is_loaded(&self) -> bool {
        matches!(self.status, Status::Loaded { .. })
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        self.status = Status::Idle;
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.reset(cx);
        if mode == Mode::Url {
            self.url.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// Enter or Import: hands over the loaded result, or starts fetching the URL.
    fn confirm(&mut self, cx: &mut Context<Self>) -> Option<ImportResult> {
        match std::mem::replace(&mut self.status, Status::Idle) {
            Status::Loaded { result, .. } => return Some(*result),
            status => self.status = status,
        }
        if self.mode == Mode::Url && self.pending.is_none() {
            self.fetch(cx);
        }
        None
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let chosen = match paths.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    let message = format!("Couldn't open the file picker: {error:#}.");
                    this.update(cx, |this, cx| this.fail(message.into(), cx))
                        .ok();
                    return;
                }
            };
            if let Some(path) = chosen {
                this.update(cx, |this, cx| {
                    if this.mode == Mode::File {
                        this.load_file(path, cx)
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn load_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.path = Some(path.clone());
        self.start(load::spawn_load_file(path), cx);
    }

    fn start(&mut self, handle: FetchHandle<ImportResult>, cx: &mut Context<Self>) {
        self.status = Status::Loading;
        let guard = handle.guard();
        let task = cx.spawn(async move |this, cx| {
            if let Some(result) = handle.result().await {
                this.update(cx, |this, cx| this.finish(result, cx)).ok();
            }
        });
        self.pending = Some(PendingLoad {
            _task: task,
            _guard: guard,
        });
        cx.notify();
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let url = match load::parse_url(&self.url.read(cx).value()) {
            Ok(url) => url,
            Err(error) => return self.fail(error.to_string().into(), cx),
        };
        let client = match &self.client {
            Ok(client) => client.clone(),
            Err(error) => return self.fail(error.clone(), cx),
        };
        self.start(load::spawn_load_url(client, url), cx);
    }

    fn finish(&mut self, result: Result<ImportResult, LoadError>, cx: &mut Context<Self>) {
        self.pending = None;
        self.status = match result {
            Ok(result) => Status::Loaded {
                preview: load::preview(&result),
                result: Box::new(result),
            },
            Err(error) => Status::Failed(error.to_string().into()),
        };
        cx.notify();
    }

    fn fail(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.pending = None;
        self.status = Status::Failed(message);
        cx.notify();
    }

    fn render_source(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.pending.is_some();
        match self.mode {
            Mode::File => {
                let muted = cx.theme().muted_foreground;
                let path = match &self.path {
                    Some(path) => div()
                        .text_color(cx.theme().foreground)
                        .child(path.display().to_string()),
                    None => div().text_color(muted).child("No file chosen"),
                };
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("import-choose-file")
                            .label("Choose File…")
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                    )
                    .child(path.flex_1().min_w_0().text_sm().text_ellipsis_start())
                    .into_any_element()
            }
            Mode::Url => {
                let button = if loading {
                    Button::new("import-stop")
                        .label("Stop")
                        .on_click(cx.listener(|this, _, _, cx| this.reset(cx)))
                } else {
                    Button::new("import-fetch")
                        .label("Fetch")
                        .on_click(cx.listener(|this, _, _, cx| this.fetch(cx)))
                };
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.url)))
                    .child(button)
                    .into_any_element()
            }
        }
    }

    fn render_status(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let muted = cx.theme().muted_foreground;
        let element = match &self.status {
            Status::Idle => return None,
            Status::Loading => {
                let text = match self.mode {
                    Mode::File => "Reading the spec…",
                    Mode::Url => "Fetching the spec…",
                };
                h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(muted)
                    .child(Spinner::new().small())
                    .child(text)
                    .into_any_element()
            }
            Status::Failed(message) => div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(message.clone())
                .into_any_element(),
            Status::Loaded { preview, .. } => render_preview(preview, cx).into_any_element(),
        };
        Some(element)
    }
}

fn render_preview(preview: &Preview, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let base_url = match &preview.base_url {
        Some(url) => div().child(url.clone()),
        None => div().text_color(muted).child("Not set"),
    };
    v_flex()
        .gap_1()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(preview.name.clone()),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(preview.counts.clone()),
        )
        .child(
            h_flex()
                .gap_2()
                .text_sm()
                .child(div().flex_none().text_color(muted).child("Base URL"))
                .child(base_url.min_w_0().truncate()),
        )
        .when(!preview.warnings.is_empty(), |this| {
            this.child(
                v_flex()
                    .id("import-warnings")
                    .mt_2()
                    .gap_1()
                    .max_h(px(140.))
                    .overflow_y_scroll()
                    .text_xs()
                    .text_color(muted)
                    .children(preview.warnings.iter().cloned()),
            )
        })
}

impl Render for ImportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = Mode::ALL
            .iter()
            .position(|mode| *mode == self.mode)
            .unwrap_or_default();
        v_flex()
            .gap_3()
            .child(
                h_flex().child(
                    TabBar::new("import-source")
                        .segmented()
                        .small()
                        .selected_index(selected)
                        .on_click(cx.listener(|this, ix: &usize, window, cx| {
                            this.set_mode(Mode::ALL[*ix], window, cx)
                        }))
                        .children(Mode::ALL.map(Mode::label)),
                ),
            )
            .child(self.render_source(cx))
            .children(self.render_status(cx))
    }
}

/// A view of its own, so Import re-renders as the dialog loads.
struct ImportFooter {
    dialog: Entity<ImportDialog>,
    confirm_label: &'static str,
    _subscription: Subscription,
}

impl Render for ImportFooter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loaded = self.dialog.read(cx).is_loaded();
        DialogFooter::new()
            .child(DialogClose::new().child(Button::new("import-cancel").label("Cancel")))
            .child(
                DialogAction::new().child(
                    Button::new("import-confirm")
                        .primary()
                        .label(self.confirm_label)
                        .disabled(!loaded),
                ),
            )
    }
}

/// Opens the import dialog. `client` fetches spec URLs; `on_import` gets the
/// converted collection once the user confirms the preview.
pub fn open_import_dialog(
    options: ImportDialogOptions,
    client: Result<Client, SharedString>,
    on_import: impl Fn(ImportResult, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = cx.new(|cx| ImportDialog::new(client, options.initial.as_ref(), window, cx));
    let footer = cx.new(|cx| ImportFooter {
        _subscription: cx.observe(&dialog, |_, _, cx| cx.notify()),
        dialog: dialog.clone(),
        confirm_label: options.confirm_label,
    });
    let title = options.title;
    let on_import = Rc::new(on_import);

    window.open_dialog(cx, move |modal, _, _| {
        let (for_ok, for_close, on_import) = (dialog.clone(), dialog.clone(), on_import.clone());
        modal
            .title(title)
            .w(px(480.))
            .child(dialog.clone())
            .footer(footer.clone())
            .on_ok(move |_, window, cx| {
                let Some(result) = for_ok.update(cx, |dialog, cx| dialog.confirm(cx)) else {
                    return false;
                };
                on_import(result, window, cx);
                true
            })
            .on_close(move |_, _, cx| for_close.update(cx, |dialog, cx| dialog.reset(cx)))
    });
}
