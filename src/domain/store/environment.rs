use std::path::PathBuf;

use uuid::Uuid;

use super::json_dir::JsonDir;
use crate::domain::environment::Environment;

/// One `<uuid>.json` file per shared environment in `dir`.
pub struct EnvironmentStore {
    dir: JsonDir,
}

impl EnvironmentStore {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            dir: JsonDir::new(dir)?,
        })
    }

    /// Sorted by name, case-insensitively. Unreadable files are skipped with a warning.
    pub fn load_all(&self) -> anyhow::Result<Vec<Environment>> {
        let mut environments: Vec<Environment> = self.dir.load_all()?;
        environments.sort_by_cached_key(|environment| environment.name.to_lowercase());
        Ok(environments)
    }

    pub fn save(&self, environment: &Environment) -> anyhow::Result<()> {
        self.dir.save(environment.id, environment)
    }

    pub fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        self.dir.delete(id)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::domain::{
        request::KeyValue,
        store::test_support::{TempDir, file_names},
    };

    fn setup() -> (TempDir, EnvironmentStore) {
        let dir = TempDir::new();
        let store = EnvironmentStore::new(dir.path().to_path_buf()).unwrap();
        (dir, store)
    }

    fn staging() -> Environment {
        let mut environment = Environment::new("Staging");
        environment.variables.push(KeyValue {
            enabled: true,
            key: "host".into(),
            value: "staging.example.com".into(),
        });
        environment
    }

    #[test]
    fn save_then_load_round_trips() {
        let (dir, store) = setup();
        let environment = staging();
        store.save(&environment).unwrap();

        assert_eq!(file_names(dir.path()), [format!("{}.json", environment.id)]);
        assert_eq!(store.load_all().unwrap(), [environment]);
    }

    #[test]
    fn saved_file_has_no_base_key() {
        let (dir, store) = setup();
        let environment = staging();
        store.save(&environment).unwrap();

        let path = dir.path().join(format!("{}.json", environment.id));
        let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(json.get("base").is_none());
    }

    #[test]
    fn load_all_sorts_by_name_case_insensitively() {
        let (_dir, store) = setup();
        for name in ["staging", "Local", "prod"] {
            store.save(&Environment::new(name)).unwrap();
        }
        let loaded = store.load_all().unwrap();
        let names: Vec<_> = loaded.iter().map(|env| env.name.as_str()).collect();
        assert_eq!(names, ["Local", "prod", "staging"]);
    }

    #[test]
    fn one_broken_file_does_not_block_the_rest() {
        let (dir, store) = setup();
        let valid = staging();
        store.save(&valid).unwrap();
        let broken = dir.path().join(format!("{}.json", Uuid::new_v4()));
        fs::write(&broken, r#"{"id": "not a uuid"}"#).unwrap();

        assert_eq!(store.load_all().unwrap(), [valid]);
        assert!(broken.exists());
    }

    #[test]
    fn delete_removes_file_and_ignores_unknown_id() {
        let (dir, store) = setup();
        let environment = staging();
        store.save(&environment).unwrap();

        store.delete(environment.id).unwrap();
        assert!(file_names(dir.path()).is_empty());
        store.delete(Uuid::new_v4()).unwrap();
    }
}
