use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

/// A unique directory under the system temp dir, removed on drop. Not created.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let name = format!("posty-pete-test-{}-{}", std::process::id(), Uuid::new_v4());
        Self(std::env::temp_dir().join(name))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
