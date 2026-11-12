use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use serde::Serialize;

pub mod collection;
pub mod environment;
pub mod history;
pub mod jobs;
mod json_dir;
pub mod navigation;
pub mod settings;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

/// The app's per-user data directory.
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dirs =
        directories::ProjectDirs::from("", "", "posty-pete").context("no home directory found")?;
    Ok(dirs.data_dir().to_path_buf())
}

/// Writes `value` as pretty JSON to a temp file next to `path`, then renames
/// it over `path`, so a crash never leaves a half-written file.
fn write_json_atomically<T: Serialize + ?Sized>(path: &Path, value: &T) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(value)?;
    fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))
}
