use std::path::PathBuf;

use anyhow::Context as _;

pub mod collection;
pub mod environment;
pub mod history;
mod json_dir;
pub mod navigation;
#[cfg(test)]
pub mod test_support;

/// The app's per-user data directory.
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dirs =
        directories::ProjectDirs::from("", "", "posty-pete").context("no home directory found")?;
    Ok(dirs.data_dir().to_path_buf())
}
