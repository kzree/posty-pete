use uuid::Uuid;

use super::{
    collection::{Collection, Item},
    request::HttpMethod,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Collection(Uuid),
    Request {
        collection_id: Uuid,
        request_id: Uuid,
    },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    pub name: String,
    /// The collection and folders holding the entry, joined with " / ".
    /// Empty for a collection.
    pub location: String,
    pub method: Option<HttpMethod>,
}

impl Entry {
    pub fn path(&self) -> String {
        if self.location.is_empty() {
            self.name.clone()
        } else {
            format!("{} / {}", self.location, self.name)
        }
    }
}

/// Every collection followed by its requests, in tree order.
pub fn entries(collections: &[Collection]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for collection in collections {
        entries.push(Entry {
            target: Target::Collection(collection.id),
            name: collection.name.clone(),
            location: String::new(),
            method: None,
        });
        push_requests(
            collection.id,
            &collection.items,
            &collection.name,
            &mut entries,
        );
    }
    entries
}

fn push_requests(collection_id: Uuid, items: &[Item], location: &str, entries: &mut Vec<Entry>) {
    for item in items {
        match item {
            Item::Request(saved) => entries.push(Entry {
                target: Target::Request {
                    collection_id,
                    request_id: saved.id,
                },
                name: saved.name.clone(),
                location: location.to_string(),
                method: Some(saved.request.method),
            }),
            Item::Folder(folder) => push_requests(
                collection_id,
                &folder.items,
                &format!("{location} / {}", folder.name),
                entries,
            ),
        }
    }
}

/// Indices of the entries matching `query`, best first; ties keep tree order.
/// An empty query matches everything.
pub fn search(entries: &[Entry], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut ranked: Vec<(Rank, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(ix, entry)| rank(entry, &query).map(|rank| (rank, ix)))
        .collect();
    ranked.sort();
    ranked.into_iter().map(|(_, ix)| ix).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    ExactName,
    NamePrefix,
    NameWordPrefix,
    NameContains,
    PathContains,
    Fuzzy,
}

/// `query` must be trimmed and lowercase.
fn rank(entry: &Entry, query: &str) -> Option<Rank> {
    let name = entry.name.to_lowercase();
    if name == query {
        return Some(Rank::ExactName);
    }
    if name.starts_with(query) {
        return Some(Rank::NamePrefix);
    }
    if name
        .match_indices(query)
        .any(|(ix, _)| starts_word(&name, ix))
    {
        return Some(Rank::NameWordPrefix);
    }
    if name.contains(query) {
        return Some(Rank::NameContains);
    }
    let path = entry.path().to_lowercase();
    if path.contains(query) {
        return Some(Rank::PathContains);
    }
    is_subsequence(query, &path).then_some(Rank::Fuzzy)
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

    fn sample() -> Vec<Collection> {
        let mut users = Folder::new("Users");
        users.items = vec![
            request("Get user", HttpMethod::Get),
            request("Create user", HttpMethod::Post),
        ];
        let mut api = Collection::new("API");
        api.items = vec![request("Health", HttpMethod::Get), Item::Folder(users)];
        let mut billing = Collection::new("Billing");
        billing.items = vec![request("List invoices", HttpMethod::Get)];
        vec![api, billing]
    }

    fn names(entries: &[Entry], query: &str) -> Vec<String> {
        search(entries, query)
            .into_iter()
            .map(|ix| entries[ix].name.clone())
            .collect()
    }

    #[test]
    fn entries_list_collections_and_nested_requests_with_their_location() {
        let collections = sample();
        let entries = entries(&collections);
        let paths: Vec<String> = entries.iter().map(Entry::path).collect();
        assert_eq!(
            paths,
            [
                "API",
                "API / Health",
                "API / Users / Get user",
                "API / Users / Create user",
                "Billing",
                "Billing / List invoices",
            ]
        );
        assert_eq!(entries[0].target, Target::Collection(collections[0].id));
        assert_eq!(entries[0].method, None);
        assert_eq!(entries[3].location, "API / Users");
        assert_eq!(entries[3].method, Some(HttpMethod::Post));
        let Item::Folder(users) = &collections[0].items[1] else {
            panic!("expected a folder");
        };
        let Item::Request(get_user) = &users.items[0] else {
            panic!("expected a request");
        };
        assert_eq!(
            entries[2].target,
            Target::Request {
                collection_id: collections[0].id,
                request_id: get_user.id,
            }
        );
    }

    #[test]
    fn empty_query_keeps_everything_in_tree_order() {
        let entries = entries(&sample());
        assert_eq!(
            search(&entries, "  "),
            (0..entries.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn matching_ignores_case() {
        let entries = entries(&sample());
        assert_eq!(names(&entries, "HEALTH"), ["Health"]);
    }

    #[test]
    fn name_matches_rank_above_path_matches() {
        let entries = entries(&sample());
        // "users" is in the name of no request but in the path of two.
        assert_eq!(names(&entries, "user"), ["Get user", "Create user"]);
        assert_eq!(names(&entries, "users"), ["Get user", "Create user"]);
        assert_eq!(
            names(&entries, "api"),
            ["API", "Health", "Get user", "Create user"]
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
        let entries = entries(&[collection]);
        assert_eq!(
            names(&entries, "order"),
            ["Order", "Orders", "Place order", "Reorder"]
        );
    }

    #[test]
    fn fuzzy_matches_characters_in_order_across_the_path() {
        let entries = entries(&sample());
        assert_eq!(names(&entries, "usr crt"), ["Create user"]);
        assert_eq!(names(&entries, "bil inv"), ["List invoices"]);
        assert!(names(&entries, "invoices billing").is_empty());
    }

    #[test]
    fn no_match_returns_nothing() {
        let entries = entries(&sample());
        assert!(search(&entries, "xyz").is_empty());
    }
}
