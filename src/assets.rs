use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

// Lucide icons beyond the default set that `Assets` embeds.
gpui_kit_assets::icon_assets!(
    ExtraIcons,
    [Dot, FlaskConical, LibraryBig, RotateCcwClock, Trash]
);

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
