use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    Styled, Subscription, Task, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
};
use uuid::Uuid;

use crate::{
    domain::quick_open::{self, Entry, Target},
    ui::method_label::method_label,
};

pub enum QuickOpenEvent {
    OpenCollection(Uuid),
    OpenRequest {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

/// A searchable list of saved requests and collections, shown in a dialog.
pub struct QuickOpen {
    list: Entity<ListState<QuickOpenDelegate>>,
    _subscription: Subscription,
}

impl EventEmitter<QuickOpenEvent> for QuickOpen {}

impl QuickOpen {
    /// `entries` come from `domain::quick_open::entries`.
    pub fn new(entries: Vec<Entry>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = QuickOpenDelegate::new(entries);
        let has_entries = !delegate.matches.is_empty();
        let list = cx.new(|cx| {
            let mut list = ListState::new(delegate, window, cx).searchable(true);
            if has_entries {
                list.set_selected_index(Some(IndexPath::default()), window, cx);
            }
            list
        });
        let subscription = cx.subscribe(&list, |_, list, event, cx| {
            let ListEvent::Confirm(ix) = event else {
                return;
            };
            let Some(target) = list.read(cx).delegate().target_at(ix.row) else {
                return;
            };
            cx.emit(match target {
                Target::Collection(id) => QuickOpenEvent::OpenCollection(id),
                Target::Request {
                    collection_id,
                    request_id,
                } => QuickOpenEvent::OpenRequest {
                    collection_id,
                    request_id,
                },
            });
        });
        Self {
            list,
            _subscription: subscription,
        }
    }

    /// Focuses the search input.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.focus(window, cx));
    }
}

impl Render for QuickOpen {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().h(px(360.)).child(
            List::new(&self.list)
                .search_placeholder("Search requests and collections")
                .size_full(),
        )
    }
}

struct QuickOpenDelegate {
    entries: Vec<Entry>,
    /// Indices into `entries`, best match first.
    matches: Vec<usize>,
}

impl QuickOpenDelegate {
    fn new(entries: Vec<Entry>) -> Self {
        let matches = quick_open::search(&entries, "");
        Self { entries, matches }
    }

    fn entry_at(&self, row: usize) -> Option<&Entry> {
        self.entries.get(*self.matches.get(row)?)
    }

    fn target_at(&self, row: usize) -> Option<Target> {
        self.entry_at(row).map(|entry| entry.target)
    }
}

impl ListDelegate for QuickOpenDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.matches = quick_open::search(&self.entries, query);
        // The list only keeps a selection if the previous results weren't
        // empty; select the best match so Enter always opens it.
        cx.spawn_in(window, async move |list, cx| {
            list.update_in(cx, |list, window, cx| {
                let first = (!list.delegate().matches.is_empty()).then(IndexPath::default);
                list.set_selected_index(first, window, cx);
                cx.notify();
            })
            .ok();
        })
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.matches.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let entry = self.entry_at(ix.row)?;
        let label = match entry.method {
            Some(method) => format!("{} {}", method.as_str(), entry.path()),
            None => format!("{}, collection", entry.name),
        };
        Some(
            ListItem::new(ix.row).accessibility_label(label).child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .text_sm()
                    .map(|row| match entry.method {
                        Some(method) => row.child(method_label(method, cx)),
                        None => row.child(
                            div().w(px(52.)).flex_none().child(
                                Icon::new(IconName::Folder)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                        ),
                    })
                    .child(
                        div()
                            .flex_none()
                            .max_w_1_2()
                            .truncate()
                            .child(entry.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(entry.location.clone()),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = if self.entries.is_empty() {
            "No saved requests yet."
        } else {
            "No matches."
        };
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}
