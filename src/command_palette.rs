use std::collections::HashMap;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement,
    Render, Styled, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, h_flex,
    kbd::Kbd,
    list::{List, ListDelegate, ListEvent, ListItem, ListState},
};
use gpui_kit_assets::IconName as LucideIcon;

use domain::{
    command::Command,
    palette::{self, Entry, Group, Scope, Section, Target},
};

use crate::{actions, ui::method_label::method_label};

pub enum CommandPaletteEvent {
    Open(Target),
    Run(Command),
}

/// The key binding that runs `command` where `focus` is, read from the keymap.
fn shortcut(command: Command, focus: &FocusHandle, window: &Window) -> Option<Kbd> {
    let action = actions::action(command)?;
    Kbd::binding_for_action_in(action.as_ref(), focus, window)
        .or_else(|| Kbd::global_binding_for_action(action.as_ref(), window))
}

fn shortcuts(entries: &[Entry], focus: &FocusHandle, window: &Window) -> HashMap<Command, Kbd> {
    entries
        .iter()
        .filter_map(|entry| match entry.target {
            Target::Command(command) => Some(command),
            _ => None,
        })
        .filter_map(|command| Some((command, shortcut(command, focus, window)?)))
        .collect()
}

/// Searches saved requests, collections, folders, environments, history,
/// commands and settings; shown in a dialog.
pub struct CommandPalette {
    list: Entity<ListState<PaletteDelegate>>,
    scope: Scope,
    _subscription: Subscription,
}

impl EventEmitter<CommandPaletteEvent> for CommandPalette {}

impl CommandPalette {
    /// `entries` come from `domain::palette`. Shortcuts are the ones that
    /// apply where `focus` is, the element commands run on.
    pub fn new(
        entries: Vec<Entry>,
        scope: Scope,
        focus: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shortcuts = shortcuts(&entries, focus, window);
        let delegate = PaletteDelegate::new(entries, shortcuts, scope);
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
            let Some(entry) = list.read(cx).delegate().entry_at(*ix) else {
                return;
            };
            cx.emit(match entry.target {
                Target::Command(command) => CommandPaletteEvent::Run(command),
                target => CommandPaletteEvent::Open(target),
            });
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
        Scope::All => "Search or type > for commands",
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
    shortcuts: HashMap<Command, Kbd>,
    scope: Scope,
    query: String,
    /// Only groups with matches, so list sections and these line up.
    sections: Vec<Section>,
}

impl PaletteDelegate {
    fn new(entries: Vec<Entry>, shortcuts: HashMap<Command, Kbd>, scope: Scope) -> Self {
        let sections = palette::search(&entries, "", scope);
        Self {
            entries,
            shortcuts,
            scope,
            query: String::new(),
            sections,
        }
    }

    fn entry_index(&self, ix: IndexPath) -> Option<usize> {
        self.sections.get(ix.section)?.entries.get(ix.row).copied()
    }

    fn entry_at(&self, ix: IndexPath) -> Option<&Entry> {
        self.entries.get(self.entry_index(ix)?)
    }

    fn shortcut(&self, entry: &Entry) -> Option<Kbd> {
        match entry.target {
            Target::Command(command) => self.shortcuts.get(&command).cloned(),
            _ => None,
        }
    }

    /// The items scope with nothing saved yet, and no commands asked for.
    fn nothing_to_search(&self) -> bool {
        self.scope == Scope::Items
            && !palette::searches_commands_only(&self.query)
            && !self
                .entries
                .iter()
                .any(|entry| entry.target.group().is_item())
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
        self.query = query.to_string();
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
        let shortcut = self.shortcut(entry);
        // A URL needs the room more than the time it was sent.
        let is_history = entry.target.group() == Group::History;
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
                                .map(|name| {
                                    if is_history {
                                        name.flex_1().min_w_0()
                                    } else {
                                        name.flex_none().max_w_1_2()
                                    }
                                })
                                .truncate()
                                .child(entry.name.clone()),
                        )
                        .child(
                            div()
                                .map(|location| {
                                    if is_history {
                                        location.flex_none()
                                    } else {
                                        location.flex_1().min_w_0()
                                    }
                                })
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(entry.location.clone()),
                        )
                        .when_some(shortcut, |row, shortcut| row.child(shortcut)),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = if self.nothing_to_search() {
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
        Group::Commands => Icon::new(IconName::ChevronRight),
        Group::Settings => Icon::new(IconName::Settings),
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
        Target::History(_) => match entry.method {
            Some(method) => format!("{} {path}, sent {}", method.as_str(), entry.location),
            None => format!("{path}, sent {}", entry.location),
        },
        Target::Command(_) => format!("{path}, command"),
        Target::SettingsSection(_) => format!("{path}, settings page"),
        Target::Setting(_) => format!("{path}, setting"),
    }
}
