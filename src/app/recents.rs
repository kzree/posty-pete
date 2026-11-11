use gpui::Context;

use super::Workspace;
use crate::{
    collections::LoadState,
    domain::recents::{Recent, Recents},
};

impl Workspace {
    /// Makes `recent` the newest of what the palette lists as recent.
    pub(super) fn record_recent(&mut self, recent: Recent, cx: &mut Context<Self>) {
        if self.recents.record(recent) {
            self.schedule_navigation_save(cx);
        }
    }

    /// Adds the stored recents after the ones recorded since startup.
    pub(super) fn restore_recents(&mut self, saved: Recents, cx: &mut Context<Self>) {
        self.recents.merge_older(saved);
        self.prune_recents(cx);
    }

    /// Drops recents whose item was deleted, once the collections are loaded.
    pub(super) fn prune_recents(&mut self, cx: &mut Context<Self>) {
        let collections = self.collections.read(cx);
        if collections.load_state() != LoadState::Loaded {
            return;
        }
        let shared = self.shared_environments.read(cx);
        let shared = shared.is_loaded().then(|| shared.all());
        if self.recents.prune(collections.all(), shared) {
            self.schedule_navigation_save(cx);
        }
    }
}
