use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySection {
    Collections,
    Environments,
}

/// Which library rows are open, kept across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LibraryExpansion {
    /// Expanded collections and folders, sorted so the file stays stable.
    pub expanded: BTreeSet<Uuid>,
    pub collapsed_sections: BTreeSet<LibrarySection>,
}

impl LibraryExpansion {
    pub fn is_default(&self) -> bool {
        self.expanded.is_empty() && self.collapsed_sections.is_empty()
    }
}

/// Entries that are malformed, or sections this version doesn't know, are
/// skipped rather than failing the whole file.
impl<'de> Deserialize<'de> for LibraryExpansion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let serde_json::Value::Object(mut fields) = serde_json::Value::deserialize(deserializer)?
        else {
            return Ok(Self::default());
        };
        Ok(Self {
            expanded: entries(fields.remove("expanded")),
            collapsed_sections: entries(fields.remove("collapsed_sections")),
        })
    }
}

fn entries<T: serde::de::DeserializeOwned + Ord>(list: Option<serde_json::Value>) -> BTreeSet<T> {
    let Some(serde_json::Value::Array(list)) = list else {
        return BTreeSet::new();
    };
    list.into_iter()
        .filter_map(|entry| serde_json::from_value(entry).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_with_sorted_ids() {
        let ids: BTreeSet<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let expansion = LibraryExpansion {
            expanded: ids.clone(),
            collapsed_sections: [LibrarySection::Environments].into(),
        };
        let value = serde_json::to_value(&expansion).unwrap();
        let written: Vec<String> = value["expanded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect();
        let mut sorted = written.clone();
        sorted.sort();
        assert_eq!(written, sorted);
        assert_eq!(
            value["collapsed_sections"],
            serde_json::json!(["environments"])
        );
        assert_eq!(
            serde_json::from_value::<LibraryExpansion>(value).unwrap(),
            expansion
        );
    }

    #[test]
    fn malformed_and_unknown_entries_are_skipped() {
        let id = Uuid::new_v4();
        let expansion: LibraryExpansion = serde_json::from_str(&format!(
            r#"{{ "expanded": ["{id}", "nope", 3], "collapsed_sections": ["collections", "tests"] }}"#
        ))
        .unwrap();
        assert_eq!(
            expansion,
            LibraryExpansion {
                expanded: [id].into(),
                collapsed_sections: [LibrarySection::Collections].into(),
            }
        );
        for json in ["null", "[]", r#"{ "expanded": "x" }"#] {
            assert_eq!(
                serde_json::from_str::<LibraryExpansion>(json).unwrap(),
                LibraryExpansion::default()
            );
        }
    }
}
