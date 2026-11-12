use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

/// A directory of `<uuid>.json` files, one value each.
pub struct JsonDir {
    dir: PathBuf,
}

impl JsonDir {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating directory {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// In no particular order. Unreadable files are skipped with a warning.
    pub fn load_all<T: DeserializeOwned>(&self) -> anyhow::Result<Vec<T>> {
        let entries = fs::read_dir(&self.dir)
            .with_context(|| format!("reading directory {}", self.dir.display()))?;

        let mut values = Vec::new();
        for entry in entries {
            let path = entry
                .with_context(|| format!("reading directory {}", self.dir.display()))?
                .path();
            if !path.is_file() || path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            match read(&path) {
                Ok(value) => values.push(value),
                Err(e) => tracing::warn!("skipping {}: {e:#}", path.display()),
            }
        }
        Ok(values)
    }

    pub fn save<T: Serialize>(&self, id: Uuid, value: &T) -> anyhow::Result<()> {
        super::write_json_atomically(&self.path(id), value)
    }

    pub fn delete(&self, id: Uuid) -> anyhow::Result<()> {
        let path = self.path(id);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                Err(e).with_context(|| format!("deleting {}", path.display()))
            }
            _ => Ok(()),
        }
    }

    fn path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let json = fs::read(path)?;
    Ok(serde_json::from_slice(&json)?)
}
