//! What the command palette searches, and how results are ranked and grouped.

use std::{collections::HashSet, sync::Arc};

use chrono::{DateTime, TimeZone};
use uuid::Uuid;

use super::{
    collection::{Collection, Item},
    command::Command,
    environment::Environment,
    history::{self, HistorySummary},
    recents::Recent,
    request::HttpMethod,
    settings_index::{Setting, SettingsSection},
};

/// What the palette was opened to search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Everything (cmd+K).
    All,
    /// Saved requests, collections, folders and environments (cmd+P).
    Items,
}

impl Scope {
    fn includes(self, group: Group) -> bool {
        match self {
            Self::All => true,
            Self::Items => group.is_item(),
        }
    }
}

/// Typed first, limits the results to commands in either scope.
pub const COMMAND_PREFIX: char = '>';

pub fn searches_commands_only(query: &str) -> bool {
    Query::parse(query).commands_only
}

/// What a typed query searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Query<'a> {
    text: &'a str,
    commands_only: bool,
}

impl<'a> Query<'a> {
    fn parse(input: &'a str) -> Self {
        let input = input.trim_start();
        match input.strip_prefix(COMMAND_PREFIX) {
            Some(rest) => Self {
                text: rest,
                commands_only: true,
            },
            None => Self {
                text: input,
                commands_only: false,
            },
        }
    }

    fn includes(self, entry: &Entry, scope: Scope) -> bool {
        match entry.group {
            // Recents stand in for suggestions until something is typed.
            Group::Recent => {
                !self.commands_only && self.is_empty() && scope.includes(entry.target.group())
            }
            Group::RecentCommands => self.is_empty() && scope.includes(Group::Commands),
            group if self.commands_only => group == Group::Commands,
            group => scope.includes(group),
        }
    }

    fn is_empty(self) -> bool {
        self.text.trim().is_empty()
    }
}

/// A section of results, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Recent,
    RecentCommands,
    Requests,
    Collections,
    Folders,
    Environments,
    History,
    Commands,
    Settings,
}

impl Group {
    pub const ALL: [Self; 9] = [
        Self::Recent,
        Self::RecentCommands,
        Self::Requests,
        Self::Collections,
        Self::Folders,
        Self::Environments,
        Self::History,
        Self::Commands,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Recent => "Recent",
            Self::RecentCommands => "Recent commands",
            Self::Requests => "Requests",
            Self::Collections => "Collections",
            Self::Folders => "Folders",
            Self::Environments => "Environments",
            Self::History => "History",
            Self::Commands => "Commands",
            Self::Settings => "Settings",
        }
    }

    /// Whether it's one of your saved items, searched in [`Scope::Items`].
    pub fn is_item(self) -> bool {
        matches!(
            self,
            Self::Requests | Self::Collections | Self::Folders | Self::Environments
        )
    }
}

/// What choosing an entry opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Request {
        collection_id: Uuid,
        request_id: Uuid,
    },
    Collection(Uuid),
    Folder {
        collection_id: Uuid,
        folder_id: Uuid,
    },
    SharedEnvironment(Uuid),
    CollectionEnvironment {
        collection_id: Uuid,
        environment_id: Uuid,
    },
    /// A history entry, by its id in the history database.
    History(i64),
    Command(Command),
    SettingsSection(SettingsSection),
    Setting(Setting),
}

impl From<Recent> for Target {
    fn from(recent: Recent) -> Self {
        match recent {
            Recent::Request {
                collection_id,
                request_id,
            } => Self::Request {
                collection_id,
                request_id,
            },
            Recent::Collection { id } => Self::Collection(id),
            Recent::Folder {
                collection_id,
                folder_id,
            } => Self::Folder {
                collection_id,
                folder_id,
            },
            Recent::SharedEnvironment { id } => Self::SharedEnvironment(id),
            Recent::CollectionEnvironment {
                collection_id,
                environment_id,
            } => Self::CollectionEnvironment {
                collection_id,
                environment_id,
            },
            Recent::SettingsPage { section } => Self::SettingsSection(section),
            Recent::Command { command } => Self::Command(command),
        }
    }
}

impl Target {
    /// The group it's listed in, apart from [`Group::Recent`] and [`Group::RecentCommands`].
    pub fn group(self) -> Group {
        match self {
            Self::Request { .. } => Group::Requests,
            Self::Collection(_) => Group::Collections,
            Self::Folder { .. } => Group::Folders,
            Self::SharedEnvironment(_) | Self::CollectionEnvironment { .. } => Group::Environments,
            Self::History(_) => Group::History,
            Self::Command(_) => Group::Commands,
            Self::SettingsSection(_) | Self::Setting(_) => Group::Settings,
        }
    }
}

pub const SHARED_LOCATION: &str = "Shared";

#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    /// The target's group, or a recent one.
    pub group: Group,
    pub name: String,
    /// The collection and folders holding a request or folder, the collection
    /// of a collection environment, [`SHARED_LOCATION`] for a shared one, the
    /// page and group of a setting, when a history entry was sent, and empty
    /// otherwise.
    pub location: String,
    pub method: Option<HttpMethod>,
    name_lower: String,
    path_lower: String,
    /// A saved request's "method url", searched like a history entry's.
    url_lower: String,
    keywords: &'static [&'static str],
}

impl Entry {
    fn new(target: Target, name: String, location: String, method: Option<HttpMethod>) -> Self {
        let mut entry = Self {
            target,
            group: target.group(),
            name,
            location,
            method,
            name_lower: String::new(),
            path_lower: String::new(),
            url_lower: String::new(),
            keywords: &[],
        };
        entry.name_lower = entry.name.to_lowercase();
        entry.path_lower = match (target.group(), method) {
            (Group::History, Some(method)) => format!("{} {}", method.as_str(), entry.name),
            _ => entry.path(),
        }
        .to_lowercase();
        entry
    }

    /// "API / Users / Get user" or, for environments and settings, "API › Staging".
    /// A history entry's is its URL.
    pub fn path(&self) -> String {
        if self.location.is_empty() || self.target.group() == Group::History {
            return self.name.clone();
        }
        let separator = match self.target.group() {
            Group::Environments | Group::Settings => "›",
            _ => "/",
        };
        format!("{} {separator} {}", self.location, self.name)
    }
}

/// Collections, then their folders, requests and environments in tree order,
/// then the shared environments.
pub fn entries(collections: &[Arc<Collection>], shared: &[Arc<Environment>]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for collection in collections {
        entries.push(Entry::new(
            Target::Collection(collection.id),
            collection.name.clone(),
            String::new(),
            None,
        ));
        push_items(
            collection.id,
            &collection.items,
            &collection.name,
            &mut entries,
        );
        entries.extend(collection.environments.iter().map(|environment| {
            Entry::new(
                Target::CollectionEnvironment {
                    collection_id: collection.id,
                    environment_id: environment.id,
                },
                environment.name.clone(),
                collection.name.clone(),
                None,
            )
        }));
    }
    entries.extend(shared.iter().map(|environment| {
        Entry::new(
            Target::SharedEnvironment(environment.id),
            environment.name.clone(),
            SHARED_LOCATION.to_string(),
            None,
        )
    }));
    entries
}

/// Entries for `commands`, in the order given.
pub fn command_entries(commands: impl IntoIterator<Item = Command>) -> Vec<Entry> {
    commands
        .into_iter()
        .map(|command| Entry {
            keywords: command.keywords(),
            ..Entry::new(
                Target::Command(command),
                command.title().to_string(),
                String::new(),
                None,
            )
        })
        .collect()
}

/// `summaries` newest first, as the history panel holds them. Repeats of a
/// method and URL keep only the newest.
pub fn history_entries<Tz: TimeZone>(
    summaries: &[HistorySummary],
    now: &DateTime<Tz>,
) -> Vec<Entry> {
    let mut seen = HashSet::new();
    summaries
        .iter()
        .filter(|summary| seen.insert((summary.method.as_str(), summary.url.as_str())))
        .map(|summary| {
            Entry::new(
                Target::History(summary.id),
                summary.url.clone(),
                history::sent_label(summary.sent_at, now),
                Some(summary.method),
            )
        })
        .collect()
}

/// Copies of the `entries` that `recents` (newest first) open, in that order,
/// grouped as recent. Recents without an entry, like a command that can't run
/// now, are left out.
pub fn recent_entries(entries: &[Entry], recents: &[Recent]) -> Vec<Entry> {
    recents
        .iter()
        .filter_map(|&recent| {
            let target = Target::from(recent);
            let entry = entries.iter().find(|entry| entry.target == target)?;
            let group = if recent.is_command() {
                Group::RecentCommands
            } else {
                Group::Recent
            };
            Some(Entry {
                group,
                ..entry.clone()
            })
        })
        .collect()
}

/// The settings pages, then every row on them.
pub fn settings_entries() -> Vec<Entry> {
    let pages = SettingsSection::ALL.into_iter().map(|section| Entry {
        keywords: section.keywords(),
        ..Entry::new(
            Target::SettingsSection(section),
            section.title().to_string(),
            String::new(),
            None,
        )
    });
    let rows = Setting::all().map(|setting| Entry {
        keywords: setting.keywords(),
        ..Entry::new(
            Target::Setting(setting),
            setting.label().to_string(),
            setting.location(),
            None,
        )
    });
    pages.chain(rows).collect()
}

fn push_items(collection_id: Uuid, items: &[Item], location: &str, entries: &mut Vec<Entry>) {
    for item in items {
        match item {
            Item::Request(saved) => entries.push(Entry {
                url_lower: format!("{} {}", saved.request.method.as_str(), saved.request.url)
                    .to_lowercase(),
                ..Entry::new(
                    Target::Request {
                        collection_id,
                        request_id: saved.id,
                    },
                    saved.name.clone(),
                    location.to_string(),
                    Some(saved.request.method),
                )
            }),
            Item::Folder(folder) => {
                entries.push(Entry::new(
                    Target::Folder {
                        collection_id,
                        folder_id: folder.id,
                    },
                    folder.name.clone(),
                    location.to_string(),
                    None,
                ));
                push_items(
                    collection_id,
                    &folder.items,
                    &format!("{location} / {}", folder.name),
                    entries,
                );
            }
        }
    }
}

/// Matching entries of one group, best first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub group: Group,
    /// Indices into the searched entries.
    pub entries: Vec<usize>,
}

/// Per-group limit while the query is broad.
pub const BROAD_LIMIT: usize = 5;
/// Limit of each recent group.
pub const RECENT_LIMIT: usize = 5;
/// Per-group limit otherwise, so a long list stays quick to scan.
pub const NARROW_LIMIT: usize = 50;
/// Queries shorter than this (ignoring spaces) are broad.
const NARROW_QUERY_LEN: usize = 3;

/// The groups with matches for `query`, in [`Group::ALL`] order. Within a
/// group the best match comes first; ties keep tree order. An empty query
/// matches everything; one starting with [`COMMAND_PREFIX`] searches only
/// commands, in either scope. Recent entries are listed first, only while
/// nothing is typed.
pub fn search(entries: &[Entry], query: &str, scope: Scope) -> Vec<Section> {
    let query = Query::parse(query);
    let text = query.text.trim().to_lowercase();
    let mut ranked: Vec<(Group, Rank, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| query.includes(entry, scope))
        .filter_map(|(ix, entry)| rank(entry, &text).map(|rank| (entry.group, rank, ix)))
        .collect();
    ranked.sort();
    // A single group has room for all of it.
    let limit = if query.commands_only {
        NARROW_LIMIT
    } else {
        limit_for(&text)
    };
    Group::ALL
        .into_iter()
        .filter_map(|group| {
            let limit = match group {
                Group::Recent | Group::RecentCommands => RECENT_LIMIT,
                _ => limit,
            };
            let entries: Vec<usize> = ranked
                .iter()
                .filter(|(g, ..)| *g == group)
                .take(limit)
                .map(|(.., ix)| *ix)
                .collect();
            (!entries.is_empty()).then_some(Section { group, entries })
        })
        .collect()
}

fn limit_for(query: &str) -> usize {
    let len = query.chars().filter(|c| !c.is_whitespace()).count();
    if len < NARROW_QUERY_LEN {
        BROAD_LIMIT
    } else {
        NARROW_LIMIT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    ExactName,
    NamePrefix,
    NameWordPrefix,
    KeywordPrefix,
    NameContains,
    PathContains,
    Url,
    Fuzzy,
}

/// `query` must be trimmed and lowercase; an empty one matches everything.
fn rank(entry: &Entry, query: &str) -> Option<Rank> {
    let name = entry.name_lower.as_str();
    if name == query {
        return Some(Rank::ExactName);
    }
    if name.starts_with(query) {
        return Some(Rank::NamePrefix);
    }
    if name
        .match_indices(query)
        .any(|(ix, _)| starts_word(name, ix))
    {
        return Some(Rank::NameWordPrefix);
    }
    if entry
        .keywords
        .iter()
        .any(|keyword| keyword.starts_with(query))
    {
        return Some(Rank::KeywordPrefix);
    }
    if name.contains(query) {
        return Some(Rank::NameContains);
    }
    let path = entry.path_lower.as_str();
    if path.contains(query) {
        return Some(Rank::PathContains);
    }
    if entry.target.group() == Group::History {
        return contains_every_word(path, query).then_some(Rank::Fuzzy);
    }
    if !entry.url_lower.is_empty() && contains_every_word(&entry.url_lower, query) {
        return Some(Rank::Url);
    }
    is_subsequence(query, path).then_some(Rank::Fuzzy)
}

/// Long URLs would fuzzy-match almost any query, so each word must appear.
fn contains_every_word(text: &str, query: &str) -> bool {
    query.split_whitespace().all(|word| text.contains(word))
}

fn starts_word(text: &str, ix: usize) -> bool {
    text[..ix]
        .chars()
        .next_back()
        .is_none_or(|before| !before.is_alphanumeric())
}

/// Whether the query's non-space characters appear in `text` in order.
fn is_subsequence(query: &str, text: &str) -> bool {
    let mut text = text.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|wanted| text.any(|c| c == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        collection::{Folder, SavedRequest},
        request::Request,
    };

    fn request(name: &str, method: HttpMethod) -> Item {
        Item::Request(SavedRequest::new(
            name,
            Request {
                method,
                ..Request::default()
            },
        ))
    }

    fn sample() -> Vec<Arc<Collection>> {
        let mut users = Folder::new("Users");
        users.items = vec![
            request("Get user", HttpMethod::Get),
            request("Create user", HttpMethod::Post),
        ];
        let mut api = Collection::new("API");
        api.items = vec![request("Health", HttpMethod::Get), Item::Folder(users)];
        api.environments = vec![Environment::new("Staging")];
        let mut billing = Collection::new("Billing");
        billing.items = vec![request("List invoices", HttpMethod::Get)];
        vec![Arc::new(api), Arc::new(billing)]
    }

    fn shared() -> Vec<Arc<Environment>> {
        vec![Arc::new(Environment::new("Local"))]
    }

    fn sample_entries() -> Vec<Entry> {
        entries(&sample(), &shared())
    }

    /// Each section as its title followed by its entries' names.
    fn outline(entries: &[Entry], query: &str, scope: Scope) -> Vec<(&'static str, Vec<String>)> {
        search(entries, query, scope)
            .into_iter()
            .map(|section| {
                let names = section
                    .entries
                    .iter()
                    .map(|&ix| entries[ix].name.clone())
                    .collect();
                (section.group.title(), names)
            })
            .collect()
    }

    fn names(entries: &[Entry], query: &str) -> Vec<String> {
        search(entries, query, Scope::All)
            .into_iter()
            .flat_map(|section| section.entries)
            .map(|ix| entries[ix].name.clone())
            .collect()
    }

    fn many_requests(count: usize) -> Vec<Entry> {
        let mut collection = Collection::new("C");
        collection.items = (0..count)
            .map(|n| request(&format!("Request {n}"), HttpMethod::Get))
            .collect();
        entries(&[Arc::new(collection)], &[])
    }

    #[test]
    fn entries_cover_every_kind_with_its_location() {
        let collections = sample();
        let entries = entries(&collections, &shared());
        let paths: Vec<String> = entries.iter().map(Entry::path).collect();
        assert_eq!(
            paths,
            [
                "API",
                "API / Health",
                "API / Users",
                "API / Users / Get user",
                "API / Users / Create user",
                "API › Staging",
                "Billing",
                "Billing / List invoices",
                "Shared › Local",
            ]
        );
        let api = &collections[0];
        let Item::Folder(users) = &api.items[1] else {
            panic!("expected a folder");
        };
        assert_eq!(entries[0].target, Target::Collection(api.id));
        assert_eq!(
            entries[2].target,
            Target::Folder {
                collection_id: api.id,
                folder_id: users.id,
            }
        );
        let Item::Request(get_user) = &users.items[0] else {
            panic!("expected a request");
        };
        assert_eq!(
            entries[3].target,
            Target::Request {
                collection_id: api.id,
                request_id: get_user.id,
            }
        );
        assert_eq!(entries[4].method, Some(HttpMethod::Post));
        assert_eq!(
            entries[5].target,
            Target::CollectionEnvironment {
                collection_id: api.id,
                environment_id: api.environments[0].id,
            }
        );
        assert_eq!(entries[5].location, "API");
        assert_eq!(
            entries[8].target,
            Target::SharedEnvironment(shared_id(&entries))
        );
    }

    fn shared_id(entries: &[Entry]) -> Uuid {
        match entries.last().map(|entry| entry.target) {
            Some(Target::SharedEnvironment(id)) => id,
            other => panic!("expected a shared environment, got {other:?}"),
        }
    }

    #[test]
    fn empty_query_groups_everything_in_fixed_order_and_tree_order() {
        let entries = sample_entries();
        assert_eq!(
            outline(&entries, "  ", Scope::All),
            [
                (
                    "Requests",
                    vec![
                        "Health".to_string(),
                        "Get user".into(),
                        "Create user".into(),
                        "List invoices".into(),
                    ]
                ),
                ("Collections", vec!["API".into(), "Billing".into()]),
                ("Folders", vec!["Users".into()]),
                ("Environments", vec!["Staging".into(), "Local".into()]),
            ]
        );
    }

    #[test]
    fn groups_without_matches_are_left_out() {
        let entries = sample_entries();
        assert_eq!(
            outline(&entries, "invoices", Scope::All),
            [("Requests", vec!["List invoices".to_string()])]
        );
        assert_eq!(
            outline(&entries, "users", Scope::All),
            [
                (
                    "Requests",
                    vec!["Get user".to_string(), "Create user".into()]
                ),
                ("Folders", vec!["Users".into()]),
            ]
        );
    }

    #[test]
    fn items_scope_searches_your_items() {
        let entries = sample_entries();
        assert_eq!(
            outline(&entries, "", Scope::Items),
            outline(&entries, "", Scope::All)
        );
    }

    #[test]
    fn broad_queries_are_capped_per_group() {
        let entries = many_requests(BROAD_LIMIT + 3);
        for query in ["", "r", "re"] {
            let sections = search(&entries, query, Scope::All);
            assert_eq!(sections[0].group, Group::Requests);
            assert_eq!(sections[0].entries.len(), BROAD_LIMIT, "query {query:?}");
        }
        let sections = search(&entries, "req", Scope::All);
        assert_eq!(sections[0].entries.len(), BROAD_LIMIT + 3);
    }

    #[test]
    fn narrow_queries_have_a_higher_cap() {
        let entries = many_requests(NARROW_LIMIT + 10);
        let sections = search(&entries, "request", Scope::All);
        assert_eq!(sections[0].entries.len(), NARROW_LIMIT);
    }

    #[test]
    fn the_cap_keeps_the_best_matches() {
        let mut collection = Collection::new("C");
        collection.items = (0..BROAD_LIMIT)
            .map(|n| request(&format!("Xo {n}"), HttpMethod::Get))
            .chain([request("Ok", HttpMethod::Get)])
            .collect();
        let entries = entries(&[Arc::new(collection)], &[]);
        let found = names(&entries, "o");
        assert_eq!(found.len(), BROAD_LIMIT);
        assert_eq!(found[0], "Ok");
    }

    #[test]
    fn matching_ignores_case() {
        let entries = sample_entries();
        assert_eq!(names(&entries, "HEALTH"), ["Health"]);
    }

    #[test]
    fn name_matches_rank_above_path_matches_within_a_group() {
        let entries = sample_entries();
        assert_eq!(
            outline(&entries, "api", Scope::All),
            [
                (
                    "Requests",
                    vec![
                        "Health".to_string(),
                        "Get user".into(),
                        "Create user".into(),
                    ]
                ),
                ("Collections", vec!["API".into()]),
                ("Folders", vec!["Users".into()]),
                ("Environments", vec!["Staging".into()]),
            ]
        );
    }

    #[test]
    fn exact_then_prefix_then_word_then_substring() {
        let mut collection = Collection::new("C");
        collection.items = vec![
            request("Reorder", HttpMethod::Get),
            request("Place order", HttpMethod::Get),
            request("Orders", HttpMethod::Get),
            request("Order", HttpMethod::Get),
        ];
        let entries = entries(&[Arc::new(collection)], &[]);
        assert_eq!(
            names(&entries, "order"),
            ["Order", "Orders", "Place order", "Reorder"]
        );
    }

    #[test]
    fn environments_match_on_their_collection_or_shared() {
        let entries = sample_entries();
        assert_eq!(
            outline(&entries, "shared", Scope::All),
            [("Environments", vec!["Local".to_string()])]
        );
        assert_eq!(names(&entries, "api stag"), ["Staging"]);
    }

    #[test]
    fn fuzzy_matches_characters_in_order_across_the_path() {
        let entries = sample_entries();
        assert_eq!(names(&entries, "usr crt"), ["Create user"]);
        assert_eq!(names(&entries, "bil inv"), ["List invoices"]);
        assert!(names(&entries, "invoices billing").is_empty());
    }

    #[test]
    fn no_match_returns_no_sections() {
        let entries = sample_entries();
        assert!(search(&entries, "xyz", Scope::All).is_empty());
    }

    fn with_commands(commands: &[Command]) -> Vec<Entry> {
        let mut entries = sample_entries();
        entries.extend(command_entries(commands.iter().copied()));
        entries
    }

    fn groups(entries: &[Entry], query: &str, scope: Scope) -> Vec<&'static str> {
        outline(entries, query, scope)
            .into_iter()
            .map(|(title, _)| title)
            .collect()
    }

    #[test]
    fn commands_come_last_in_the_full_scope() {
        let entries = with_commands(&[Command::NewRequest, Command::Quit]);
        assert_eq!(
            outline(&entries, "", Scope::All).last(),
            Some(&("Commands", vec!["New request".to_string(), "Quit".into()]))
        );
    }

    #[test]
    fn items_scope_leaves_out_commands() {
        let entries = with_commands(&[Command::NewRequest]);
        assert_eq!(
            groups(&entries, "", Scope::Items),
            ["Requests", "Collections", "Folders", "Environments"]
        );
        assert!(search(&entries, "new", Scope::Items).is_empty());
    }

    #[test]
    fn the_prefix_searches_only_commands_in_either_scope() {
        let entries = with_commands(&[Command::NewRequest, Command::Save]);
        for scope in [Scope::All, Scope::Items] {
            assert_eq!(
                outline(&entries, ">", scope),
                outline(&entries, "> ", scope)
            );
            assert_eq!(groups(&entries, ">", scope), ["Commands"]);
            assert_eq!(
                outline(&entries, " >req", scope),
                [("Commands", vec!["New request".to_string(), "Save".into()])]
            );
        }
        assert!(search(&entries, "> health", Scope::All).is_empty());
    }

    #[test]
    fn the_prefix_alone_lists_every_command() {
        let entries = command_entries(Command::ALL);
        let sections = search(&entries, ">", Scope::Items);
        assert_eq!(sections[0].entries.len(), Command::ALL.len());
    }

    #[test]
    fn a_prefix_inside_the_query_is_searched_for() {
        let entries = with_commands(&[Command::NewRequest]);
        assert!(search(&entries, "new >", Scope::All).is_empty());
    }

    #[test]
    fn commands_match_their_keywords() {
        let entries = with_commands(&Command::ALL);
        assert_eq!(names(&entries, "panel"), ["Toggle sidebar"]);
        assert_eq!(names(&entries, "> exit"), ["Quit"]);
    }

    fn with_settings() -> Vec<Entry> {
        let mut entries = with_commands(&Command::ALL);
        entries.extend(settings_entries());
        entries
    }

    #[test]
    fn settings_come_after_commands_in_the_full_scope_only() {
        let entries = with_settings();
        assert_eq!(groups(&entries, "", Scope::All).last(), Some(&"Settings"));
        assert!(!groups(&entries, "", Scope::Items).contains(&"Settings"));
        assert_eq!(
            groups(&entries, "> timeout", Scope::All),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn pages_come_first_among_settings() {
        let entries = settings_entries();
        assert_eq!(
            outline(&entries, "", Scope::All),
            [(
                "Settings",
                vec![
                    "General".to_string(),
                    "Appearance".into(),
                    "Requests".into(),
                    "History & data".into(),
                    "AI".into(),
                ]
            )]
        );
    }

    #[test]
    fn settings_show_their_page_and_group() {
        let entries = settings_entries();
        let timeout = entries
            .iter()
            .find(|e| e.target == Target::Setting(Setting::Timeout))
            .map(Entry::path);
        assert_eq!(timeout.as_deref(), Some("Requests › Connection › Timeout"));
    }

    fn setting_names(query: &str) -> Vec<String> {
        let entries = settings_entries();
        names(&entries, query)
    }

    #[test]
    fn settings_match_their_keywords() {
        assert_eq!(setting_names("seconds")[0], "Timeout");
        assert!(setting_names("color").contains(&"Accent colour".to_string()));
        assert!(setting_names("color").contains(&"Appearance".to_string()));
        assert_eq!(setting_names("ssl")[0], "Verify TLS certificates");
    }

    #[test]
    fn shortcuts_are_found_by_action_name() {
        let entries = with_settings();
        assert_eq!(
            outline(&entries, "toggle sidebar", Scope::All),
            [
                ("Commands", vec!["Toggle sidebar".to_string()]),
                ("Settings", vec!["Toggle sidebar".to_string()]),
            ]
        );
        assert!(setting_names("command palette").contains(&"Command palette".to_string()));
    }

    #[test]
    fn same_labels_are_told_apart_by_their_group() {
        let entries = settings_entries();
        let paths: Vec<String> = search(&entries, "font size", Scope::All)
            .into_iter()
            .flat_map(|section| section.entries)
            .map(|ix| entries[ix].path())
            .collect();
        assert_eq!(
            paths,
            [
                "Appearance › Interface › Font size",
                "Appearance › Editors › Font size"
            ]
        );
    }

    fn now() -> DateTime<chrono::FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-10T15:30:00+02:00").unwrap()
    }

    fn sent(id: i64, method: HttpMethod, url: &str) -> HistorySummary {
        HistorySummary {
            id,
            sent_at: DateTime::parse_from_rfc3339("2026-11-10T12:05:00Z")
                .unwrap()
                .to_utc(),
            method,
            url: url.to_string(),
            outcome: None,
        }
    }

    fn with_history(history: &[HistorySummary]) -> Vec<Entry> {
        let mut entries = with_settings();
        entries.extend(history_entries(history, &now()));
        entries
    }

    #[test]
    fn history_entries_show_the_url_and_when_it_was_sent() {
        let entries = history_entries(
            &[sent(7, HttpMethod::Post, "https://api.test/users")],
            &now(),
        );
        assert_eq!(entries[0].target, Target::History(7));
        assert_eq!(entries[0].name, "https://api.test/users");
        assert_eq!(entries[0].location, "Today 14:05");
        assert_eq!(entries[0].method, Some(HttpMethod::Post));
        assert_eq!(entries[0].path(), "https://api.test/users");
    }

    #[test]
    fn history_keeps_the_newest_of_repeated_requests() {
        let entries = history_entries(
            &[
                sent(3, HttpMethod::Get, "https://api.test/a"),
                sent(2, HttpMethod::Post, "https://api.test/a"),
                sent(1, HttpMethod::Get, "https://api.test/a"),
            ],
            &now(),
        );
        let targets: Vec<Target> = entries.iter().map(|e| e.target).collect();
        assert_eq!(targets, [Target::History(3), Target::History(2)]);
    }

    #[test]
    fn history_matches_url_fragments_and_method() {
        let entries = with_history(&[
            sent(2, HttpMethod::Post, "https://api.test/orders"),
            sent(1, HttpMethod::Get, "https://api.test/invoices/42"),
        ]);
        let found = |query| {
            outline(&entries, query, Scope::All)
                .into_iter()
                .find(|(title, _)| *title == "History")
                .map(|(_, urls)| urls)
                .unwrap_or_default()
        };
        assert_eq!(found("invoices/4"), ["https://api.test/invoices/42"]);
        assert_eq!(found("post"), ["https://api.test/orders"]);
        assert_eq!(found("get invoices"), ["https://api.test/invoices/42"]);
        assert!(found("delete").is_empty());
        assert!(found("aio").is_empty(), "no fuzzy matches on URLs");
    }

    fn request_at(name: &str, method: HttpMethod, url: &str) -> Item {
        Item::Request(SavedRequest::new(
            name,
            Request {
                method,
                url: url.to_string(),
                ..Request::default()
            },
        ))
    }

    fn with_urls() -> Vec<Entry> {
        let mut collection = Collection::new("C");
        collection.items = vec![
            request_at("Create order", HttpMethod::Post, "https://api.test/orders"),
            request_at("Invoice", HttpMethod::Get, "https://api.test/invoices/42"),
        ];
        entries(&[Arc::new(collection)], &[])
    }

    #[test]
    fn saved_requests_match_url_fragments_and_method() {
        let entries = with_urls();
        assert_eq!(names(&entries, "invoices/4"), ["Invoice"]);
        assert_eq!(names(&entries, "api.test/orders"), ["Create order"]);
        assert_eq!(names(&entries, "get invoices"), ["Invoice"]);
        assert_eq!(names(&entries, "POST orders"), ["Create order"]);
        assert!(names(&entries, "delete orders").is_empty());
        assert!(
            names(&entries, "aio").is_empty(),
            "no fuzzy matches on URLs"
        );
    }

    #[test]
    fn name_matches_rank_above_url_matches() {
        let mut collection = Collection::new("C");
        collection.items = vec![
            request_at("List", HttpMethod::Get, "https://api.test/orders"),
            request_at("Orders report", HttpMethod::Get, "https://api.test/report"),
        ];
        let entries = entries(&[Arc::new(collection)], &[]);
        assert_eq!(names(&entries, "orders"), ["Orders report", "List"]);
    }

    #[test]
    fn saved_requests_matched_by_url_stay_above_history() {
        let mut entries = with_urls();
        entries.extend(history_entries(
            &[sent(1, HttpMethod::Get, "https://api.test/invoices/42")],
            &now(),
        ));
        assert_eq!(
            outline(&entries, "invoices/42", Scope::All),
            [
                ("Requests", vec!["Invoice".to_string()]),
                ("History", vec!["https://api.test/invoices/42".to_string()]),
            ]
        );
    }

    #[test]
    fn history_comes_after_saved_items_even_when_it_matches_better() {
        let entries = with_history(&[sent(1, HttpMethod::Get, "health")]);
        assert_eq!(
            outline(&entries, "health", Scope::All),
            [
                ("Requests", vec!["Health".to_string()]),
                ("History", vec!["health".to_string()]),
            ]
        );
        assert_eq!(
            groups(&entries, "", Scope::All),
            [
                "Requests",
                "Collections",
                "Folders",
                "Environments",
                "History",
                "Commands",
                "Settings"
            ]
        );
    }

    #[test]
    fn history_ties_keep_newest_first() {
        let entries = with_history(&[
            sent(2, HttpMethod::Get, "https://api.test/b"),
            sent(1, HttpMethod::Get, "https://api.test/a"),
        ]);
        assert_eq!(
            outline(&entries, "api.test", Scope::All),
            [(
                "History",
                vec![
                    "https://api.test/b".to_string(),
                    "https://api.test/a".into()
                ]
            )]
        );
    }

    #[test]
    fn history_is_capped_like_other_groups() {
        let history: Vec<HistorySummary> = (0..NARROW_LIMIT as i64 + 10)
            .map(|n| sent(n, HttpMethod::Get, &format!("https://api.test/{n}")))
            .collect();
        let entries = history_entries(&history, &now());
        assert_eq!(
            search(&entries, "", Scope::All)[0].entries.len(),
            BROAD_LIMIT
        );
        assert_eq!(
            search(&entries, "api.test", Scope::All)[0].entries.len(),
            NARROW_LIMIT
        );
    }

    #[test]
    fn history_is_left_out_of_the_items_scope_and_commands() {
        let entries = with_history(&[sent(1, HttpMethod::Get, "https://api.test/new")]);
        assert!(!groups(&entries, "", Scope::Items).contains(&"History"));
        assert!(!groups(&entries, "> new", Scope::All).contains(&"History"));
    }

    #[test]
    fn keyword_matches_rank_between_word_prefix_and_substring_matches() {
        let entries = command_entries([
            Command::SharedEnvironments,
            Command::History,
            Command::SendRequest,
            Command::Revert,
        ]);
        assert_eq!(
            names(&entries, "re"),
            [
                "Revert changes",
                "Send request",
                "History",
                "Shared environments"
            ]
        );
    }

    /// `entries` with the recent ones for `recents` added.
    fn with_recents(mut entries: Vec<Entry>, recents: &[Recent]) -> Vec<Entry> {
        let recent = recent_entries(&entries, recents);
        entries.extend(recent);
        entries
    }

    /// Every kind of entry, with a few of them recent.
    fn recent_sample() -> Vec<Entry> {
        let entries = with_settings();
        let recents = sample_recents(&entries);
        with_recents(entries, &recents)
    }

    fn sample_recents(entries: &[Entry]) -> Vec<Recent> {
        let find = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.target)
                .unwrap()
        };
        let Target::Request {
            collection_id,
            request_id,
        } = find("Get user")
        else {
            panic!("expected a request");
        };
        let Target::Collection(billing) = find("Billing") else {
            panic!("expected a collection");
        };
        vec![
            Recent::Command {
                command: Command::NewCollection,
            },
            Recent::SettingsPage {
                section: SettingsSection::Appearance,
            },
            Recent::Request {
                collection_id,
                request_id,
            },
            Recent::Command {
                command: Command::Save,
            },
            Recent::Collection { id: billing },
        ]
    }

    #[test]
    fn empty_query_lists_recents_first_newest_first() {
        let entries = recent_sample();
        let outline = outline(&entries, "", Scope::All);
        assert_eq!(
            outline[0],
            (
                "Recent",
                vec![
                    "Appearance".to_string(),
                    "Get user".into(),
                    "Billing".into()
                ]
            )
        );
        assert_eq!(
            outline[1],
            (
                "Recent commands",
                vec!["New collection".to_string(), "Save".into()]
            )
        );
        assert_eq!(outline[2].0, "Requests");
        assert_eq!(outline.last().unwrap().0, "Settings");
    }

    #[test]
    fn typing_hides_the_recents() {
        let entries = recent_sample();
        for query in ["get", "> save", "billing"] {
            let groups = groups(&entries, query, Scope::All);
            assert!(!groups.contains(&"Recent"), "{query:?}");
            assert!(!groups.contains(&"Recent commands"), "{query:?}");
        }
        assert_eq!(
            outline(&entries, "get user", Scope::All),
            [("Requests", vec!["Get user".to_string()])]
        );
    }

    #[test]
    fn the_prefix_alone_lists_recent_commands_first() {
        let entries = recent_sample();
        assert_eq!(
            groups(&entries, ">", Scope::All),
            ["Recent commands", "Commands"]
        );
    }

    #[test]
    fn items_scope_lists_only_recent_items() {
        let entries = recent_sample();
        let outline = outline(&entries, "", Scope::Items);
        assert_eq!(
            outline[0],
            ("Recent", vec!["Get user".to_string(), "Billing".into()])
        );
        let groups: Vec<&str> = outline.iter().map(|(title, _)| *title).collect();
        assert_eq!(
            groups,
            [
                "Recent",
                "Requests",
                "Collections",
                "Folders",
                "Environments"
            ]
        );
    }

    #[test]
    fn recents_without_an_entry_are_left_out() {
        let entries = with_commands(&[Command::NewRequest]);
        let recents = [
            Recent::Collection { id: Uuid::new_v4() },
            Recent::Command {
                command: Command::Revert,
            },
            Recent::Command {
                command: Command::NewRequest,
            },
        ];
        let recent = recent_entries(&entries, &recents);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].target, Target::Command(Command::NewRequest));
        assert_eq!(recent[0].group, Group::RecentCommands);
    }

    #[test]
    fn recent_groups_are_capped() {
        let entries = many_requests(RECENT_LIMIT + 3);
        let recents: Vec<Recent> = entries
            .iter()
            .filter_map(|entry| match entry.target {
                Target::Request {
                    collection_id,
                    request_id,
                } => Some(Recent::Request {
                    collection_id,
                    request_id,
                }),
                _ => None,
            })
            .rev()
            .collect();
        let mut all = entries.clone();
        all.extend(recent_entries(&entries, &recents));
        let sections = search(&all, "", Scope::All);
        assert_eq!(sections[0].group, Group::Recent);
        assert_eq!(sections[0].entries.len(), RECENT_LIMIT);
        assert_eq!(
            all[sections[0].entries[0]].name,
            format!("Request {}", RECENT_LIMIT + 2)
        );
    }

    #[test]
    fn without_recents_the_empty_query_is_unchanged() {
        let entries = with_settings();
        assert_eq!(
            outline(&with_recents(with_settings(), &[]), "", Scope::All),
            outline(&entries, "", Scope::All)
        );
    }
}
