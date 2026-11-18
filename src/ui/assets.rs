//! Embedded UI assets: product icons and the component library's default
//! icon set.
//!
//! Product icons come from the Lucide catalog bundled with `gpui-kit-assets`:
//! only the ones named in the table below are embedded, so an icon that is
//! registered cannot be missing from the binary. Emoji category and paging
//! icons are registered here too, so the tree carries no second SVG set.

use gpui::{AssetSource, Result, SharedString};
use gpui_kit::assets::IconName;
use std::borrow::Cow;

/// 产品图标登记表：名称 → Lucide 图标。一张表同时给出查找函数、嵌入集合
/// 与测试名单；未登记的名字回退为圆圈，不会渲染空白。
macro_rules! product_icons {
    ($($($name:literal)|+ => $icon:ident,)*) => {
        gpui_kit::assets::icon_assets!(ProductIcons, [$($icon,)* Circle]);

        /// 产品图标名对应的 Lucide 图标。
        pub fn icon_name(name: &str) -> IconName {
            match name {
                $($($name)|+ => IconName::$icon,)*
                _ => IconName::Circle,
            }
        }

        /// 产品图标在资产源中的路径。
        pub fn icon_path(name: &str) -> SharedString {
            icon_name(name).path()
        }

        #[cfg(test)]
        const NAMES: &[(&str, IconName)] = &[$($(($name, IconName::$icon),)+)*];
    };
}

product_icons! {
    // 类型筛选与卡片类型区
    "TextIcon" => Type,
    "RichTextIcon" => FileText,
    "ImageIcon" => Image,
    "FilesIcon" => Files,
    "LinkIcon" => Link,
    "PathIcon" => Folder,
    "ColorIcon" => Palette,
    "ContactIcon" => Contact,
    "MailIcon" => Mail,
    "PhoneIcon" => Phone,
    "SecretIcon" => KeyRound,
    "QrCodeIcon" => QrCode,
    "BrokenImageIcon" => ImageOff,
    "FileImageIcon" => FileImage,
    "OcrIcon" => ScanText,
    "CloudIcon" => Cloud,
    "GithubIcon" => Github,
    // Emoji 面板分类与分页（emoji 分类在 Lucide 里没有独立图形，用语义最接近的）
    "LayoutGridIcon" => LayoutGrid,
    "FaceSmileIcon" => FaceSlightlySmiling,
    "ClockIcon" => Clock,
    "HandIcon" => Hand,
    "LeafIcon" => Leaf,
    "AppleIcon" => Apple,
    "PlaneIcon" => Plane,
    "TrophyIcon" => Trophy,
    "LightbulbIcon" => Lightbulb,
    "HashIcon" => Hash,
    "FlagIcon" => Flag,
    "ChevronLeftIcon" => ChevronLeft,
    "ChevronRightIcon" => ChevronRight,

    // 动作
    "CopyIcon" => Copy,
    "PasteIcon" => ClipboardPaste,
    "EditIcon" => Pencil,
    "NoteIcon" => StickyNote,
    "DeleteIcon" => Trash,
    "CombineIcon" => Combine,
    "ClearIcon" => Eraser,
    "OpenIcon" | "ShareIcon" => ExternalLink,
    "UploadIcon" => Upload,
    "DownloadIcon" => Download,
    "FindIcon" => TextSearch,
    "ReplaceFieldIcon" => Replace,
    "ReplaceAllIcon" => ReplaceAll,
    "LinkDecodeIcon" => Link,
    "Base64Icon" => Binary,
    "JsonIcon" => Braces,
    "TrimIcon" => TextAlignStart,

    // 状态与控件
    "PlusIcon" => Plus,
    "StarIcon" => Star,
    "StarFilledIcon" => StarFill,
    "PinIcon" => Pin,
    "KeyboardIcon" => Keyboard,
    "CloseIcon" => X,
    "CheckIcon" => Check,
    "CircleIcon" => Circle,
    "CircleCheckIcon" => CircleCheck,
    "ChevronDownIcon" => ChevronDown,
    "BackIcon" => ArrowLeft,
    "SearchIcon" => Search,
    "SettingsIcon" => Settings,
    "BanIcon" => Ban,
    "ClipboardBlockIcon" => ClipboardX,
    "TagIcon" => Tag,
    "InfoIcon" => Info,
    "HelpIcon" => CircleQuestionMark,
}

/// 全仓唯一资产源：产品图标 → gpui-kit 组件自带的默认图标。
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ProductIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ProductIcons.list(path)?;
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::{icon_name, icon_path, Assets, NAMES};
    use gpui::AssetSource;
    use gpui_kit::assets::IconName;

    fn svg(path: &str) -> String {
        let bytes = Assets
            .load(path)
            .expect("asset source")
            .unwrap_or_else(|| panic!("{path} is not embedded"));
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn every_registered_icon_is_embedded_and_drawn() {
        assert!(NAMES.len() > 40);
        for (name, icon) in NAMES {
            assert_eq!(icon_name(name), *icon);
            let path = icon_path(name);
            assert!(!path.is_empty(), "{name}");
            assert!(svg(&path).contains("<svg"), "{name}");
        }
    }

    #[test]
    fn unknown_names_fall_back_to_a_circle() {
        assert_eq!(icon_name("NoSuchIcon"), IconName::Circle);
        assert!(svg(&icon_path("NoSuchIcon")).contains("<circle"));
    }

    /// Emoji 分类与分页图标来自登记表，不再有本地 `picker-icons/` 副本。
    #[test]
    fn emoji_category_icons_come_from_the_table() {
        for name in [
            "LayoutGridIcon",
            "FaceSmileIcon",
            "ClockIcon",
            "HandIcon",
            "LeafIcon",
            "AppleIcon",
            "PlaneIcon",
            "TrophyIcon",
            "LightbulbIcon",
            "HashIcon",
            "FlagIcon",
            "ChevronLeftIcon",
            "ChevronRightIcon",
        ] {
            assert_ne!(icon_name(name), IconName::Circle, "{name}");
            assert!(svg(&icon_path(name)).contains("<svg"), "{name}");
        }
    }
}
