//! What the command palette searches, and how results are ranked and grouped.

use std::sync::Arc;

use uuid::Uuid;

use super::{
    collection::{Collection, Item},
    command::Command,
    environment::Environment,
    request::HttpMethod,
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

    fn includes(self, group: Group, scope: Scope) -> bool {
        if self.commands_only {
            group == Group::Commands
        } else {
            scope.includes(group)
        }
    }
}

/// A section of results, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Requests,
    Collections,
    Folders,
    Environments,
    Commands,
}

impl Group {
    pub const ALL: [Self; 5] = [
        Self::Requests,
        Self::Collections,
        Self::Folders,
        Self::Environments,
        Self::Commands,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Requests => "Requests",
            Self::Collections => "Collections",
            Self::Folders => "Folders",
            Self::Environments => "Environments",
            Self::Commands => "Commands",
        }
    }

    fn is_item(self) -> bool {
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
    Command(Command),
}

impl Target {
    pub fn group(self) -> Group {
        match self {
            Self::Request { .. } => Group::Requests,
            Self::Collection(_) => Group::Collections,
            Self::Folder { .. } => Group::Folders,
            Self::SharedEnvironment(_) | Self::CollectionEnvironment { .. } => Group::Environments,
            Self::Command(_) => Group::Commands,
        }
    }
}

pub const SHARED_LOCATION: &str = "Shared";

#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    pub name: String,
    /// The collection and folders holding a request or folder, the collection
    /// of a collection environment, [`SHARED_LOCATION`] for a shared one, and
    /// empty for a collection or command.
    pub location: String,
    pub method: Option<HttpMethod>,
    name_lower: String,
    path_lower: String,
    keywords: &'static [&'static str],
}

impl Entry {
    fn new(target: Target, name: String, location: String, method: Option<HttpMethod>) -> Self {
        let mut entry = Self {
            target,
            name,
            location,
            method,
            name_lower: String::new(),
            path_lower: String::new(),
            keywords: &[],
        };
        entry.name_lower = entry.name.to_lowercase();
        entry.path_lower = entry.path().to_lowercase();
        entry
    }

    /// "API / Users / Get user" or, for environments, "API › Staging".
    pub fn path(&self) -> String {
        if self.location.is_empty() {
            return self.name.clone();
        }
        let separator = match self.target.group() {
            Group::Environments => "›",
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

fn push_items(collection_id: Uuid, items: &[Item], location: &str, entries: &mut Vec<Entry>) {
    for item in items {
        match item {
            Item::Request(saved) => entries.push(Entry::new(
                Target::Request {
                    collection_id,
                    request_id: saved.id,
                },
                saved.name.clone(),
                location.to_string(),
                Some(saved.request.method),
            )),
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
/// Per-group limit otherwise, so a long list stays quick to scan.
pub const NARROW_LIMIT: usize = 50;
/// Queries shorter than this (ignoring spaces) are broad.
const NARROW_QUERY_LEN: usize = 3;

/// The groups with matches for `query`, in [`Group::ALL`] order. Within a
/// group the best match comes first; ties keep tree order. An empty query
/// matches everything; one starting with [`COMMAND_PREFIX`] searches only
/// commands, in either scope.
pub fn search(entries: &[Entry], query: &str, scope: Scope) -> Vec<Section> {
    let query = Query::parse(query);
    let text = query.text.trim().to_lowercase();
    let mut ranked: Vec<(Group, Rank, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| query.includes(entry.target.group(), scope))
        .filter_map(|(ix, entry)| rank(entry, &text).map(|rank| (entry.target.group(), rank, ix)))
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
    is_subsequence(query, path).then_some(Rank::Fuzzy)
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
}
