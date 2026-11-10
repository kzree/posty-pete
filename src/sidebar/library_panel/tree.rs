use std::{collections::HashSet, sync::Arc};

use gpui::SharedString;
use uuid::Uuid;

use crate::domain::{
    collection::{Collection, Item},
    request::HttpMethod,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Row {
    /// The collapsible "Collections" header.
    Section {
        expanded: bool,
    },
    Node(Node),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
    pub(super) id: Uuid,
    /// The collection this node belongs to; equals `id` for collections.
    pub(super) collection_id: Uuid,
    pub(super) name: SharedString,
    /// 0 for collections.
    pub(super) depth: usize,
    pub(super) kind: NodeKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NodeKind {
    Collection { expanded: bool },
    Folder { expanded: bool },
    Request(HttpMethod),
}

/// No rows at all when there are no collections, so the empty state shows.
pub(super) fn flatten(
    collections: &[Arc<Collection>],
    section_expanded: bool,
    expanded: &HashSet<Uuid>,
) -> Vec<Row> {
    if collections.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![Row::Section {
        expanded: section_expanded,
    }];
    if !section_expanded {
        return rows;
    }
    for collection in collections {
        let is_expanded = expanded.contains(&collection.id);
        rows.push(Row::Node(Node {
            id: collection.id,
            collection_id: collection.id,
            name: collection.name.clone().into(),
            depth: 0,
            kind: NodeKind::Collection {
                expanded: is_expanded,
            },
        }));
        if is_expanded {
            flatten_items(collection.id, &collection.items, 1, expanded, &mut rows);
        }
    }
    rows
}

fn flatten_items(
    collection_id: Uuid,
    items: &[Item],
    depth: usize,
    expanded: &HashSet<Uuid>,
    rows: &mut Vec<Row>,
) {
    for item in items {
        match item {
            Item::Folder(folder) => {
                let is_expanded = expanded.contains(&folder.id);
                rows.push(Row::Node(Node {
                    id: folder.id,
                    collection_id,
                    name: folder.name.clone().into(),
                    depth,
                    kind: NodeKind::Folder {
                        expanded: is_expanded,
                    },
                }));
                if is_expanded {
                    flatten_items(collection_id, &folder.items, depth + 1, expanded, rows);
                }
            }
            Item::Request(saved) => rows.push(Row::Node(Node {
                id: saved.id,
                collection_id,
                name: saved.name.clone().into(),
                depth,
                kind: NodeKind::Request(saved.request.method),
            })),
        }
    }
}

pub(super) fn container_ids(collections: &[Arc<Collection>]) -> HashSet<Uuid> {
    fn walk(items: &[Item], ids: &mut HashSet<Uuid>) {
        for item in items {
            if let Item::Folder(folder) = item {
                ids.insert(folder.id);
                walk(&folder.items, ids);
            }
        }
    }
    let mut ids = HashSet::new();
    for collection in collections {
        ids.insert(collection.id);
        walk(&collection.items, &mut ids);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::library_panel::test_support::{fixture, outline};

    #[test]
    fn no_collections_means_no_rows() {
        assert!(flatten(&[], true, &HashSet::new()).is_empty());
    }

    #[test]
    fn nothing_expanded_shows_only_collections() {
        let (collections, _) = fixture();
        let rows = flatten(&collections, true, &HashSet::new());
        assert_eq!(outline(&rows), ["Collections", "0:API", "0:Empty"]);
    }

    #[test]
    fn collapsed_section_shows_only_its_header() {
        let (collections, ids) = fixture();
        let rows = flatten(&collections, false, &HashSet::from([ids.collection]));
        assert_eq!(outline(&rows), ["Collections"]);
    }

    #[test]
    fn nested_expansion_indents_by_depth() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.outer, ids.inner]);
        let rows = flatten(&collections, true, &expanded);
        assert_eq!(
            outline(&rows),
            [
                "Collections",
                "0:API",
                "1:Root",
                "1:Outer",
                "2:Inner",
                "3:Deep",
                "0:Empty"
            ]
        );
        assert!(matches!(
            &rows[5],
            Row::Node(Node {
                kind: NodeKind::Request(HttpMethod::Post),
                ..
            })
        ));
    }

    #[test]
    fn expanded_folder_inside_collapsed_parent_stays_hidden() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.inner]);
        let rows = flatten(&collections, true, &expanded);
        assert_eq!(
            outline(&rows),
            ["Collections", "0:API", "1:Root", "1:Outer", "0:Empty"]
        );
    }
}
