use std::{
    mem,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use anyhow::Context as _;
use gpui::{App, AppContext, Context, Task, Window};
use gpui_router::use_navigate;

use super::{
    EditorContent, Workspace,
    routes::{AppRoute, RouteHistory},
};
use crate::{
    collections::LoadState,
    domain::{
        navigation::{NavEntry, SavedNavigation},
        request::Request,
        store::navigation::NavigationStore,
    },
};

const SAVE_DELAY: Duration = Duration::from_millis(500);

/// Keeps the back/forward history and the draft in `navigation.json`.
pub(super) struct PersistedNavigation {
    phase: Phase,
    scheduled_save: Option<Task<()>>,
}

enum Phase {
    Loading,
    /// Waiting for the collections, so the current entry can be opened.
    Loaded {
        store: NavigationStore,
        saved: Option<SavedNavigation>,
    },
    /// Saving starts only here, so a stored history is never overwritten before it was restored.
    Saving(Writer),
    /// Also used for the rest of the session when the collections failed to
    /// load, so the stored history survives a temporary failure.
    Unavailable,
}

impl PersistedNavigation {
    pub(super) fn new() -> Self {
        Self {
            phase: Phase::Loading,
            scheduled_save: None,
        }
    }

    fn is_waiting_for_collections(&self) -> bool {
        matches!(self.phase, Phase::Loaded { .. })
    }

    /// Ends the wait for the collections. Starts saving and returns the stored
    /// navigation to restore if they loaded; otherwise never saves.
    fn collections_loaded(&mut self, collections: LoadState) -> Option<SavedNavigation> {
        let Phase::Loaded { store, saved } = mem::replace(&mut self.phase, Phase::Unavailable)
        else {
            return None;
        };
        if collections == LoadState::Failed {
            tracing::warn!("collections failed to load: not restoring or saving navigation");
            return None;
        }
        self.phase = Phase::Saving(Writer::new(store));
        saved
    }

    fn save_job(&mut self, saved: SavedNavigation) -> Option<impl FnOnce() + Send + 'static> {
        match &mut self.phase {
            Phase::Saving(writer) => Some(writer.job(saved)),
            _ => None,
        }
    }

    fn is_saving(&self) -> bool {
        matches!(self.phase, Phase::Saving(_))
    }
}

struct Writer {
    store: Arc<NavigationStore>,
    /// The newest state written, so a slow write can't land over a newer one.
    written: Arc<Mutex<u64>>,
    next: u64,
}

impl Writer {
    fn new(store: NavigationStore) -> Self {
        Self {
            store: Arc::new(store),
            written: Arc::default(),
            next: 0,
        }
    }

    fn job(&mut self, saved: SavedNavigation) -> impl FnOnce() + Send + 'static {
        self.next += 1;
        let (seq, store, written) = (self.next, self.store.clone(), self.written.clone());
        move || {
            let mut written = written.lock().unwrap_or_else(PoisonError::into_inner);
            if *written >= seq {
                return;
            }
            *written = seq;
            if let Err(error) = store.save(&saved) {
                tracing::warn!("couldn't save navigation: {error:#}");
            }
        }
    }
}

impl From<AppRoute> for NavEntry {
    fn from(route: AppRoute) -> Self {
        match route {
            AppRoute::Request => NavEntry::Draft,
            AppRoute::SavedRequest {
                collection_id,
                request_id,
            } => NavEntry::SavedRequest {
                collection_id,
                request_id,
            },
            AppRoute::Settings => NavEntry::Settings,
            AppRoute::Collection(id) => NavEntry::Collection { id },
        }
    }
}

impl From<NavEntry> for AppRoute {
    fn from(entry: NavEntry) -> Self {
        match entry {
            NavEntry::Draft => AppRoute::Request,
            NavEntry::SavedRequest {
                collection_id,
                request_id,
            } => AppRoute::SavedRequest {
                collection_id,
                request_id,
            },
            NavEntry::Settings => AppRoute::Settings,
            NavEntry::Collection { id } => AppRoute::Collection(id),
        }
    }
}

/// Rebuilds gpui-router's history to match `history`, which it can't be handed directly.
fn replay_in_router(history: &RouteHistory, window: &mut Window, cx: &mut App) {
    let entries = history.entries();
    let mut navigator = use_navigate(window, cx);
    navigator.replace(entries[0].path());
    for route in &entries[1..] {
        navigator.push(route.path());
    }
    for _ in history.index() + 1..entries.len() {
        navigator.back();
    }
}

impl Workspace {
    /// Reads `navigation.json` off the UI thread and restores it once the
    /// collections are loaded too.
    pub(super) fn load_navigation(window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let opened = cx.background_spawn(async { open_and_load() }).await;
            this.update_in(cx, |this, window, cx| {
                this.persisted_navigation.phase = match opened {
                    Ok((store, saved)) => Phase::Loaded { store, saved },
                    Err(error) => {
                        tracing::warn!("navigation won't be kept: {error:#}");
                        Phase::Unavailable
                    }
                };
                this.restore_navigation(window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Restores the stored navigation if both it and the collections are loaded.
    pub(super) fn restore_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let collections = self.collections.read(cx).load_state();
        if collections == LoadState::Loading
            || !self.persisted_navigation.is_waiting_for_collections()
        {
            return;
        }
        match self.persisted_navigation.collections_loaded(collections) {
            Some(saved) if self.navigation_untouched(cx) => {
                self.apply_saved_navigation(saved, window, cx)
            }
            Some(_) => tracing::info!("not restoring navigation: the user already moved on"),
            None => {}
        }
    }

    fn navigation_untouched(&self, cx: &App) -> bool {
        self.route_history.entries() == [AppRoute::Request]
            && self.editor_content == EditorContent::Draft
            && self.request_editor.read(cx).build_request(cx) == Request::default()
    }

    fn apply_saved_navigation(
        &mut self,
        saved: SavedNavigation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let routes = saved.entries.into_iter().map(AppRoute::from).collect();
        let Some(history) = RouteHistory::from_parts(routes, saved.index) else {
            return;
        };
        self.route_history = history;
        if !self.route_exists(self.current_route(), cx) {
            self.route_history.replace_current(AppRoute::Request);
        }

        replay_in_router(&self.route_history, window, cx);

        self.show_current_route(AppRoute::Request, window, cx);
        match self.editor_content {
            EditorContent::Draft => {
                if let Some(draft) = saved.draft {
                    self.load_editor(EditorContent::Draft, &draft, window, cx);
                }
            }
            EditorContent::Saved { .. } => self.draft = saved.draft,
        }
    }

    /// Saves the navigation a moment later, so bursts of changes write once.
    pub(super) fn schedule_navigation_save(&mut self, cx: &mut Context<Self>) {
        let persisted = &mut self.persisted_navigation;
        if !persisted.is_saving() || persisted.scheduled_save.is_some() {
            return;
        }
        persisted.scheduled_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                this.persisted_navigation.scheduled_save = None;
                if let Some(job) = this.navigation_save_job(cx) {
                    cx.background_spawn(async move { job() }).detach();
                }
            })
            .ok();
        }));
    }

    /// Saves the navigation on this thread, for quitting or closing the window.
    pub(super) fn save_navigation_now(&mut self, cx: &mut Context<Self>) {
        self.persisted_navigation.scheduled_save = None;
        if let Some(job) = self.navigation_save_job(cx) {
            job();
        }
    }

    fn navigation_save_job(&mut self, cx: &App) -> Option<impl FnOnce() + Send + 'static> {
        if !self.persisted_navigation.is_saving() {
            return None;
        }
        let draft = match self.editor_content {
            EditorContent::Draft => Some(self.request_editor.read(cx).build_request(cx)),
            EditorContent::Saved { .. } => self.draft.clone(),
        };
        let saved = SavedNavigation {
            entries: self
                .route_history
                .entries()
                .iter()
                .map(|route| NavEntry::from(*route))
                .collect(),
            index: self.route_history.index(),
            draft,
        };
        self.persisted_navigation.save_job(saved)
    }
}

/// A corrupt file is ignored.
fn open_and_load() -> anyhow::Result<(NavigationStore, Option<SavedNavigation>)> {
    let dirs =
        directories::ProjectDirs::from("", "", "posty-pete").context("no home directory found")?;
    let store = NavigationStore::new(dirs.data_dir().join("navigation.json"));
    let saved = store.load().unwrap_or_else(|error| {
        tracing::warn!("ignoring saved navigation: {error:#}");
        None
    });
    Ok((store, saved))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::*;

    struct TempFile(PathBuf);

    impl TempFile {
        fn new() -> Self {
            let name = format!("posty-pete-test-{}-{}", std::process::id(), Uuid::new_v4());
            let dir = std::env::temp_dir().join(name);
            Self(dir.join("navigation.json"))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    fn navigation(entries: Vec<NavEntry>) -> SavedNavigation {
        SavedNavigation {
            entries,
            index: 0,
            draft: None,
        }
    }

    /// `navigation.json` already holds `stored`, as read at startup.
    fn loaded(file: &TempFile, stored: &SavedNavigation) -> PersistedNavigation {
        let store = NavigationStore::new(file.0.clone());
        store.save(stored).unwrap();
        PersistedNavigation {
            phase: Phase::Loaded {
                store,
                saved: Some(stored.clone()),
            },
            scheduled_save: None,
        }
    }

    #[test]
    fn failed_collections_skip_the_restore_and_leave_the_file_alone() {
        let file = TempFile::new();
        let stored = navigation(vec![NavEntry::Settings]);
        let mut persisted = loaded(&file, &stored);
        let before = fs::read(&file.0).unwrap();

        assert!(persisted.collections_loaded(LoadState::Failed).is_none());
        assert!(!persisted.is_saving());
        assert!(
            persisted
                .save_job(navigation(vec![NavEntry::Draft]))
                .is_none()
        );
        assert_eq!(fs::read(&file.0).unwrap(), before);
    }

    #[test]
    fn loaded_collections_restore_and_start_saving() {
        let file = TempFile::new();
        let stored = navigation(vec![NavEntry::Settings]);
        let mut persisted = loaded(&file, &stored);

        assert_eq!(
            persisted.collections_loaded(LoadState::Loaded),
            Some(stored)
        );
        let job = persisted
            .save_job(navigation(vec![NavEntry::Draft]))
            .unwrap();
        job();
        let store = NavigationStore::new(file.0.clone());
        assert_eq!(
            store.load().unwrap(),
            Some(navigation(vec![NavEntry::Draft]))
        );
    }

    #[test]
    fn routes_round_trip_through_nav_entries() {
        let routes = [
            AppRoute::Request,
            AppRoute::SavedRequest {
                collection_id: Uuid::new_v4(),
                request_id: Uuid::new_v4(),
            },
            AppRoute::Settings,
            AppRoute::Collection(Uuid::new_v4()),
        ];
        for route in routes {
            assert_eq!(AppRoute::from(NavEntry::from(route)), route);
        }
    }
}
