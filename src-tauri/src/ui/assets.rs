//! Asset source: gpui-kit's component icons plus the Lucide icons ixtable uses.
//! Add an icon here before using `IconName::<Name>` from `gpui_kit::assets`.

use gpui_kit::*;
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        ArrowDownUp,
        CodeXml,
        Construction,
        Database,
        DatabaseZap,
        FilePlus,
        FolderPlus,
        Hash,
        Link,
        PackageOpen,
        Pencil,
        Save,
        SaveAll,
        Shapes,
        Table2,
        Trash,
        WandSparkles,
        X,
        Zap
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
