use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};

use gpui::SharedString;
use uuid::Uuid;

use domain::{
    collection::{Collection, Item},
    environment::Environment,
    request::HttpMethod,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Row {
    /// A collapsible section header.
    Section {
        section: Section,
        expanded: bool,
    },
    /// Stands in for an expanded section's missing contents.
    Empty(Section),
    Node(Node),
    /// A shared environment.
    Environment {
        id: Uuid,
        name: SharedString,
    },
}

impl Row {
    /// Placeholder rows are skipped by the arrow keys.
    pub(super) fn is_selectable(&self) -> bool {
        !matches!(self, Row::Empty(_))
    }
}

pub(super) use domain::library_expansion::LibrarySection as Section;

/// Which sections are expanded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sections {
    pub(super) collections: bool,
    pub(super) environments: bool,
}

impl Default for Sections {
    fn default() -> Self {
        Self {
            collections: true,
            environments: true,
        }
    }
}

impl Sections {
    pub(super) fn toggle(&mut self, section: Section) {
        let expanded = self.get_mut(section);
        *expanded = !*expanded;
    }

    pub(super) fn expand(&mut self, section: Section) {
        *self.get_mut(section) = true;
    }

    pub(super) fn collapsed(self) -> BTreeSet<Section> {
        [
            (Section::Collections, self.collections),
            (Section::Environments, self.environments),
        ]
        .into_iter()
        .filter_map(|(section, expanded)| (!expanded).then_some(section))
        .collect()
    }

    pub(super) fn with_collapsed(collapsed: &BTreeSet<Section>) -> Self {
        Self {
            collections: !collapsed.contains(&Section::Collections),
            environments: !collapsed.contains(&Section::Environments),
        }
    }

    fn get_mut(&mut self, section: Section) -> &mut bool {
        match section {
            Section::Collections => &mut self.collections,
            Section::Environments => &mut self.environments,
        }
    }
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
    /// The folders holding it, outermost first; empty at a collection's root
    /// and for collections.
    pub(super) folders: Vec<Uuid>,
    /// The item after it in the same folder or root.
    pub(super) next: Option<Uuid>,
}

impl Node {
    /// The folder holding it; `None` at a collection's root and for collections.
    pub(super) fn parent(&self) -> Option<Uuid> {
        self.folders.last().copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NodeKind {
    Collection { expanded: bool },
    Folder { expanded: bool },
    Request(HttpMethod),
}

/// Both section headers always show, so their actions stay reachable.
pub(super) fn flatten(
    collections: &[Arc<Collection>],
    environments: &[Arc<Environment>],
    sections: Sections,
    expanded: &HashSet<Uuid>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    push_section(
        Section::Collections,
        sections.collections,
        collections.is_empty(),
        &mut rows,
    );
    if sections.collections {
        flatten_collections(collections, expanded, &mut rows);
    }
    push_section(
        Section::Environments,
        sections.environments,
        environments.is_empty(),
        &mut rows,
    );
    if sections.environments {
        rows.extend(environments.iter().map(|env| Row::Environment {
            id: env.id,
            name: env.name.clone().into(),
        }));
    }
    rows
}

fn push_section(section: Section, expanded: bool, is_empty: bool, rows: &mut Vec<Row>) {
    rows.push(Row::Section { section, expanded });
    if expanded && is_empty {
        rows.push(Row::Empty(section));
    }
}

fn flatten_collections(
    collections: &[Arc<Collection>],
    expanded: &HashSet<Uuid>,
    rows: &mut Vec<Row>,
) {
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
            folders: Vec::new(),
            next: None,
        }));
        if is_expanded {
            let mut folders = Vec::new();
            flatten_items(
                collection.id,
                &mut folders,
                &collection.items,
                expanded,
                rows,
            );
        }
    }
}

/// `folders` holds the folders down to `items`, and is as it was on return.
fn flatten_items(
    collection_id: Uuid,
    folders: &mut Vec<Uuid>,
    items: &[Item],
    expanded: &HashSet<Uuid>,
    rows: &mut Vec<Row>,
) {
    let depth = folders.len() + 1;
    for (index, item) in items.iter().enumerate() {
        let next = items.get(index + 1).map(Item::id);
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
                    folders: folders.clone(),
                    next,
                }));
                if is_expanded {
                    folders.push(folder.id);
                    flatten_items(collection_id, folders, &folder.items, expanded, rows);
                    folders.pop();
                }
            }
            Item::Request(saved) => rows.push(Row::Node(Node {
                id: saved.id,
                collection_id,
                name: saved.name.clone().into(),
                depth,
                kind: NodeKind::Request(saved.request.method),
                folders: folders.clone(),
                next,
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
    use crate::sidebar::library_panel::test_support::{environments, fixture, outline};

    const ALL: Sections = Sections {
        collections: true,
        environments: true,
    };

    #[test]
    fn empty_sections_show_their_headers_and_empty_rows() {
        assert_eq!(
            outline(&flatten(&[], &[], ALL, &HashSet::new())),
            ["Collections", "(none)", "Environments", "(none)"]
        );
    }

    #[test]
    fn collapsed_empty_sections_show_only_their_headers() {
        let sections = Sections {
            collections: false,
            environments: false,
        };
        assert_eq!(
            outline(&flatten(&[], &[], sections, &HashSet::new())),
            ["Collections", "Environments"]
        );
    }

    #[test]
    fn nothing_expanded_shows_only_collections() {
        let (collections, _) = fixture();
        let rows = flatten(&collections, &[], ALL, &HashSet::new());
        assert_eq!(
            outline(&rows),
            ["Collections", "0:API", "0:Empty", "Environments", "(none)"]
        );
    }

    #[test]
    fn collapsed_collections_show_only_their_header() {
        let (collections, ids) = fixture();
        let sections = Sections {
            collections: false,
            ..ALL
        };
        let rows = flatten(
            &collections,
            &environments(),
            sections,
            &HashSet::from([ids.collection]),
        );
        assert_eq!(
            outline(&rows),
            ["Collections", "Environments", "env:Local", "env:Staging"]
        );
    }

    #[test]
    fn environments_follow_the_collections_in_order() {
        let (collections, _) = fixture();
        let rows = flatten(&collections, &environments(), ALL, &HashSet::new());
        assert_eq!(
            outline(&rows),
            [
                "Collections",
                "0:API",
                "0:Empty",
                "Environments",
                "env:Local",
                "env:Staging"
            ]
        );
    }

    #[test]
    fn collapsed_environments_show_only_their_header() {
        let (collections, _) = fixture();
        let sections = Sections {
            environments: false,
            ..ALL
        };
        let rows = flatten(&collections, &environments(), sections, &HashSet::new());
        assert_eq!(
            outline(&rows),
            ["Collections", "0:API", "0:Empty", "Environments"]
        );
    }

    #[test]
    fn environment_rows_carry_their_ids() {
        let environments = environments();
        let rows = flatten(&[], &environments, ALL, &HashSet::new());
        assert_eq!(
            rows[3],
            Row::Environment {
                id: environments[0].id,
                name: "Local".into()
            }
        );
    }

    #[test]
    fn nested_expansion_indents_by_depth() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.outer, ids.inner]);
        let rows = flatten(&collections, &[], ALL, &expanded);
        assert_eq!(
            outline(&rows),
            [
                "Collections",
                "0:API",
                "1:Root",
                "1:Outer",
                "2:Inner",
                "3:Deep",
                "0:Empty",
                "Environments",
                "(none)"
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
    fn nodes_know_their_folders_and_next_sibling() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.outer, ids.inner]);
        let rows = flatten(&collections, &[], ALL, &expanded);
        let links: Vec<_> = rows[1..6]
            .iter()
            .map(|row| match row {
                Row::Node(node) => (node.id, node.folders.clone(), node.next, node.depth),
                row => panic!("expected a node, got {row:?}"),
            })
            .collect();
        assert_eq!(
            links,
            [
                (ids.collection, vec![], None, 0),
                (ids.root, vec![], Some(ids.outer), 1),
                (ids.outer, vec![], None, 1),
                (ids.inner, vec![ids.outer], None, 2),
                (ids.deep, vec![ids.outer, ids.inner], None, 3),
            ]
        );
    }

    #[test]
    fn expanded_folder_inside_collapsed_parent_stays_hidden() {
        let (collections, ids) = fixture();
        let expanded = HashSet::from([ids.collection, ids.inner]);
        let rows = flatten(&collections, &[], ALL, &expanded);
        assert_eq!(
            outline(&rows)[..5],
            ["Collections", "0:API", "1:Root", "1:Outer", "0:Empty"]
        );
    }

    #[test]
    fn collapsed_sections_round_trip() {
        for collections in [true, false] {
            for environments in [true, false] {
                let sections = Sections {
                    collections,
                    environments,
                };
                assert_eq!(Sections::with_collapsed(&sections.collapsed()), sections);
            }
        }
        assert_eq!(
            Sections {
                collections: true,
                environments: false
            }
            .collapsed(),
            BTreeSet::from([Section::Environments])
        );
    }

    #[test]
    fn sections_toggle_and_expand_independently() {
        let mut sections = ALL;
        sections.toggle(Section::Environments);
        assert_eq!(
            sections,
            Sections {
                collections: true,
                environments: false
            }
        );
        sections.toggle(Section::Collections);
        sections.expand(Section::Environments);
        assert_eq!(
            sections,
            Sections {
                collections: false,
                environments: true
            }
        );
    }
}
