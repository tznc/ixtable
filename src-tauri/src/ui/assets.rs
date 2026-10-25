//! Asset source: gpui-kit's component icons, the Lucide icons ixtable uses and the app icon.
//! Add an icon here before using `IconName::<Name>` from `gpui_kit::assets`.

use gpui_kit::*;
use std::borrow::Cow;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        ArrowDownUp,
        Bold,
        ChartColumn,
        ChevronFirst,
        ChevronLast,
        Code,
        CodeXml,
        Construction,
        Database,
        DatabaseZap,
        Download,
        FilePlus,
        FolderPlus,
        Funnel,
        Gauge,
        Hash,
        Heading,
        Image,
        Italic,
        Link,
        List,
        ListOrdered,
        ListTodo,
        Minus,
        PackageOpen,
        Paperclip,
        Pencil,
        Save,
        SaveAll,
        Scan,
        Shapes,
        SquareCode,
        Strikethrough,
        Table,
        Table2,
        TextQuote,
        Trash,
        Underline,
        WandSparkles,
        X,
        Zap,
        ZoomIn,
        ZoomOut
    ]
);

pub struct AppAssets;

/// The app icon (`branding/app-icon.svg`), drawn by `img(APP_ICON)` in the sidebar and start screen.
pub const APP_ICON: &str = "brand/app-icon.svg";

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON {
            return Ok(Some(Cow::Borrowed(include_bytes!("../../../branding/app-icon.svg"))));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        if APP_ICON.starts_with(path) {
            paths.push(APP_ICON.into());
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON, AppAssets};
    use gpui_kit::AssetSource as _;

    #[test]
    fn bundles_the_app_icon_svg() {
        let bytes = AppAssets.load(APP_ICON).expect("load").expect("the app icon is bundled");
        assert!(bytes.starts_with(b"<svg"));
        assert!(AppAssets.list("brand/").expect("list").iter().any(|p| p == APP_ICON));
    }
}
