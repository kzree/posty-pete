use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

// Extra Lucide icons, beyond the 101 that `gpui_kit_assets::Assets` embeds.
// Add names here as needed; only the listed SVGs end up in the binary.
gpui_kit_assets::icon_assets!(ExtraIcons, [Dot, LibraryBig, RotateCcwClock, Trash]);

/// The app's asset source: extra icons first, then the default component assets.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
