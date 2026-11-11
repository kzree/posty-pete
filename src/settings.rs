use std::time::Duration;

use anyhow::Context as _;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription, Task};

use crate::domain::{
    settings::Settings,
    store::{
        self,
        settings::{SettingsStore, SettingsWriter},
    },
};

const SAVE_DELAY: Duration = Duration::from_millis(300);

/// The settings, readable and changeable from anywhere. Changes notify the
/// global's observers (`cx.observe_global::<AppSettings>`).
pub struct AppSettings {
    settings: Settings,
    saver: Entity<SettingsSaver>,
}

impl Global for AppSettings {}

impl AppSettings {
    /// Loads `settings.json` on this thread, so call it before opening a window.
    pub fn init(cx: &mut App) {
        let (writer, settings) = open();
        let saver = cx.new(|cx| SettingsSaver::new(writer, cx));
        cx.set_global(Self { settings, saver });
    }

    pub fn get(cx: &App) -> &Settings {
        &cx.global::<Self>().settings
    }

    /// Applies `f`. If that changed anything, notifies observers and saves a
    /// moment later.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Settings)) {
        let current = &cx.global::<Self>().settings;
        let mut settings = current.clone();
        f(&mut settings);
        if settings == *current {
            return;
        }
        let this = cx.global_mut::<Self>();
        this.settings = settings;
        let saver = this.saver.clone();
        saver.update(cx, |saver, cx| saver.schedule_save(cx));
    }

    /// Emits [`SettingsEvent`]s.
    pub fn saver(cx: &App) -> Entity<SettingsSaver> {
        cx.global::<Self>().saver.clone()
    }
}

pub enum SettingsEvent {
    /// `title` says what didn't happen.
    Failed {
        title: &'static str,
        error: anyhow::Error,
    },
}

/// Saves the settings off the UI thread, debounced and in order, and on quit.
pub struct SettingsSaver {
    /// `None` if there is no data directory.
    writer: Option<SettingsWriter>,
    scheduled_save: Option<Task<()>>,
    _quit: Subscription,
}

impl EventEmitter<SettingsEvent> for SettingsSaver {}

impl SettingsSaver {
    fn new(writer: Option<SettingsWriter>, cx: &mut Context<Self>) -> Self {
        Self {
            writer,
            scheduled_save: None,
            _quit: cx.on_app_quit(|this, cx| {
                this.save_now(cx);
                async {}
            }),
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.scheduled_save.is_some() {
            return;
        }
        self.scheduled_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| this.save_in_background(cx)).ok();
        }));
    }

    fn save_in_background(&mut self, cx: &mut Context<Self>) {
        self.scheduled_save = None;
        let job = match self.save_job(cx) {
            Ok(job) => job,
            Err(error) => return cx.emit(failed(error)),
        };
        cx.spawn(async move |this, cx| {
            if let Err(error) = cx.background_spawn(async move { job() }).await {
                this.update(cx, |_, cx| cx.emit(failed(error))).ok();
            }
        })
        .detach();
    }

    /// Saves on this thread if a save is scheduled, still running or failed.
    fn save_now(&mut self, cx: &App) {
        let scheduled = self.scheduled_save.take().is_some();
        let running = self
            .writer
            .as_ref()
            .is_some_and(SettingsWriter::has_pending);
        if !scheduled && !running {
            return;
        }
        if let Err(error) = self.save_job(cx).and_then(|job| job()) {
            tracing::warn!("couldn't save settings: {error:#}");
        }
    }

    fn save_job(
        &mut self,
        cx: &App,
    ) -> anyhow::Result<impl FnOnce() -> anyhow::Result<()> + Send + 'static> {
        let writer = self
            .writer
            .as_mut()
            .context("there is no data directory to save them in")?;
        Ok(writer.job(cx.global::<AppSettings>().settings.clone()))
    }
}

fn failed(error: anyhow::Error) -> SettingsEvent {
    SettingsEvent::Failed {
        title: "Couldn't save settings",
        error,
    }
}

fn open() -> (Option<SettingsWriter>, Settings) {
    match store::data_dir() {
        Ok(dir) => {
            let (store, settings) = SettingsStore::open(dir.join("settings.json"));
            (Some(SettingsWriter::new(store)), settings)
        }
        Err(error) => {
            tracing::warn!("settings won't be kept: {error:#}");
            (None, Settings::default())
        }
    }
}
