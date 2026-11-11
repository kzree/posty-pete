use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, IntoElement, ParentElement, Render,
    Styled, Subscription, Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, h_flex,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
};
use gpui_kit_assets::IconName as LucideIcon;

use crate::{
    domain::palette::{self, Entry, Group, Scope, Section, Target},
    ui::method_label::method_label,
};

pub enum CommandPaletteEvent {
    Open(Target),
}

/// Searches saved requests, collections, folders and environments; shown in a dialog.
pub struct CommandPalette {
    list: Entity<ListState<PaletteDelegate>>,
    scope: Scope,
    _subscription: Subscription,
}

impl EventEmitter<CommandPaletteEvent> for CommandPalette {}

impl CommandPalette {
    /// `entries` come from `domain::palette::entries`.
    pub fn new(
        entries: Vec<Entry>,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = PaletteDelegate::new(entries, scope);
        let has_results = !delegate.sections.is_empty();
        let list = cx.new(|cx| {
            let mut list = ListState::new(delegate, window, cx).searchable(true);
            if has_results {
                list.set_selected_index(Some(IndexPath::default()), window, cx);
            }
            list
        });
        let subscription = cx.subscribe(&list, |_, list, event, cx| {
            let ListEvent::Confirm(ix) = event else {
                return;
            };
            if let Some(entry) = list.read(cx).delegate().entry_at(*ix) {
                cx.emit(CommandPaletteEvent::Open(entry.target));
            }
        });
        Self {
            list,
            scope,
            _subscription: subscription,
        }
    }

    /// Focuses the search input.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.focus(window, cx));
    }
}

fn placeholder(scope: Scope) -> &'static str {
    match scope {
        Scope::All => "Search everything",
        Scope::Items => "Search requests, collections and environments",
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().h(px(360.)).child(
            List::new(&self.list)
                .search_placeholder(placeholder(self.scope))
                .size_full(),
        )
    }
}

struct PaletteDelegate {
    entries: Vec<Entry>,
    scope: Scope,
    /// Only groups with matches, so list sections and these line up.
    sections: Vec<Section>,
}

impl PaletteDelegate {
    fn new(entries: Vec<Entry>, scope: Scope) -> Self {
        let sections = palette::search(&entries, "", scope);
        Self {
            entries,
            scope,
            sections,
        }
    }

    fn entry_index(&self, ix: IndexPath) -> Option<usize> {
        self.sections.get(ix.section)?.entries.get(ix.row).copied()
    }

    fn entry_at(&self, ix: IndexPath) -> Option<&Entry> {
        self.entries.get(self.entry_index(ix)?)
    }
}

impl ListDelegate for PaletteDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.sections = palette::search(&self.entries, query, self.scope);
        // The list only keeps a selection if the previous results weren't
        // empty; select the best match so Enter always opens it.
        cx.spawn_in(window, async move |list, cx| {
            list.update_in(cx, |list, window, cx| {
                let first = (!list.delegate().sections.is_empty()).then(IndexPath::default);
                list.set_selected_index(first, window, cx);
                cx.notify();
            })
            .ok();
        })
    }

    fn sections_count(&self, _cx: &App) -> usize {
        self.sections.len()
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.sections
            .get(section)
            .map_or(0, |section| section.entries.len())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let group = self.sections.get(section)?.group;
        Some(
            div()
                .px_3()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(group.title()),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let index = self.entry_index(ix)?;
        let entry = &self.entries[index];
        Some(
            ListItem::new(("palette-entry", index))
                .accessibility_label(accessibility_label(entry))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .text_sm()
                        .child(leading(entry, cx))
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
            "No saved requests, collections or environments yet."
        } else {
            "No results"
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

/// The method label, or an icon in a slot of the same width.
fn leading(entry: &Entry, cx: &App) -> gpui::Div {
    if let Some(method) = entry.method {
        return method_label(method, cx);
    }
    let icon = match entry.target.group() {
        Group::Environments => Icon::new(LucideIcon::Layers),
        _ => Icon::new(IconName::Folder),
    };
    div()
        .w(px(52.))
        .flex_none()
        .child(icon.small().text_color(cx.theme().muted_foreground))
}

fn accessibility_label(entry: &Entry) -> String {
    let path = entry.path();
    match entry.target {
        Target::Request { .. } => match entry.method {
            Some(method) => format!("{} {path}", method.as_str()),
            None => path,
        },
        Target::Collection(_) => format!("{path}, collection"),
        Target::Folder { .. } => format!("{path}, folder"),
        Target::SharedEnvironment(_) | Target::CollectionEnvironment { .. } => {
            format!("{path}, environment")
        }
    }
}
