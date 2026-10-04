//! 本构建已编译的 Nana 组件目录，以及播放条和虚拟列表探针。
//!
//! `COMPILED_COMPONENT_IDS` 是能力锁。Nana 目录增减时先改这里和
//! `docs/nana-component-map.md`，再改产品壳层。

/// 当前 `hosted + components + icons-tabler` 构建里 `compiled == true` 的稳定 id。
pub const COMPILED_COMPONENT_IDS: &[&str] = &[
    "about-section",
    "action-menu",
    "action-menu-item",
    "anchored-action-menu",
    "app-shell",
    "app-title-bar",
    "appearance-section",
    "avatar",
    "button",
    "calendar-heatmap",
    "card",
    "checkbox",
    "chip",
    "command-palette",
    "confirm-dialog",
    "context-menu",
    "dialog",
    "dock",
    "dock-panel",
    "donut-chart",
    "drawer",
    "dropdown",
    "empty-state",
    "form-field",
    "gpu-texture-view",
    "gpu-view",
    "graph-canvas",
    "graph-minimap",
    "hosted-textarea",
    "icon-button",
    "image-viewer",
    "interactive-card",
    "key-capture-layer",
    "keymap-layer",
    "labeled-value",
    "level-meter",
    "list-item",
    "native-markdown",
    "overlay-host",
    "pane-chrome",
    "pane-tree",
    "popover",
    "progress",
    "qr-code",
    "range-field",
    "reorder-list",
    "search-dropdown",
    "segmented-control",
    "select",
    "selectable-rich-text",
    "settings",
    "settings-collapsible-card",
    "sidebar-footer",
    "sidebar-frame",
    "sidebar-row",
    "sidebar-section",
    "skeleton",
    "spinner",
    "split-pane",
    "status-badge",
    "switch",
    "tabs",
    "text",
    "text-input",
    "textarea",
    "thumbnail",
    "time-series-chart",
    "toast",
    "tooltip",
    "tree-view",
    "validation-message",
    "workspace",
    "xy-pad",
];

/// 公开类型里没有稳定目录 id、不能靠目录测试发现消失的组件。
pub const UNCATALOGUED_PUBLIC_WIDGETS: &[&str] = &[
    "media-transport-bar",
    "settings-card",
    "settings-row",
    "runtime-list",
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use nana_ui::components::{MediaTransportBar, MediaTransportEvent};
    use nana_ui::icons_tabler::{
        ARCHIVE, BOOKMARK, CLOCK, FILE, FOLDER, LIST, PLAYER_PAUSE, PLAYER_PLAY, SEARCH, SETTINGS,
        TAG, TRASH, VOLUME,
    };
    use nana_ui::{Icon, VirtualListLayout, component_catalog};

    use super::{COMPILED_COMPONENT_IDS, UNCATALOGUED_PUBLIC_WIDGETS};

    #[test]
    fn compiled_catalog_matches_the_locked_product_set() {
        let actual: BTreeSet<_> = component_catalog()
            .iter()
            .filter(|support| support.compiled)
            .map(|support| support.id.as_str())
            .collect();
        let expected: BTreeSet<_> = COMPILED_COMPONENT_IDS.iter().copied().collect();
        assert_eq!(
            actual,
            expected,
            "目录与锁不一致。新增 {:?}，缺失 {:?}",
            actual.difference(&expected).collect::<Vec<_>>(),
            expected.difference(&actual).collect::<Vec<_>>()
        );
        assert!(
            component_catalog().iter().all(|support| support.compiled),
            "本构建不应留下未编译组件"
        );
        for id in UNCATALOGUED_PUBLIC_WIDGETS {
            assert!(!actual.contains(id), "{id} 进入目录后要改地图和探针");
        }
    }

    #[test]
    fn media_transport_bar_exposes_playback_events_without_a_catalog_id() {
        let bar = MediaTransportBar::new();
        assert!(!bar.playing);
        assert!(bar.seekable);
        assert_eq!(bar.volume, 100.0);
        let _ = (
            MediaTransportEvent::PlayPause,
            MediaTransportEvent::Seek(1.0),
            MediaTransportEvent::SeekStarted,
            MediaTransportEvent::SeekEnded,
            MediaTransportEvent::Volume(40.0),
        );
    }

    #[test]
    fn virtual_list_window_is_smaller_than_the_full_file_list() {
        let layout = VirtualListLayout::new(std::iter::repeat_n(28.0, 10_000));
        let window = layout.window(0.0, 800.0, 56.0);
        let cap = VirtualListLayout::uniform_window_item_cap(800.0, 56.0, 28.0);
        assert!(window.range.len() < 100, "可见窗口应远小于一万行");
        assert!(window.range.len() <= cap);
        assert_eq!(layout.len(), 10_000);
    }

    #[test]
    fn product_icons_use_tabler_when_the_shell_catalog_has_no_alias() {
        assert_eq!(Icon::parse_name("lucide-folder"), Some(Icon::Folder));
        assert_eq!(Icon::parse_name("lucide-search"), Some(Icon::Search));
        assert_eq!(Icon::parse_name("lucide-settings"), Some(Icon::Settings));
        assert_eq!(Icon::parse_name("lucide-file"), Some(Icon::File));
        assert!(Icon::parse_name("trash").is_none());
        assert!(Icon::parse_name("player-play").is_none());
        for (icon, name) in [
            (ARCHIVE, "archive"),
            (BOOKMARK, "bookmark"),
            (CLOCK, "clock"),
            (FILE, "file"),
            (FOLDER, "folder"),
            (LIST, "list"),
            (PLAYER_PAUSE, "player-pause"),
            (PLAYER_PLAY, "player-play"),
            (SEARCH, "search"),
            (SETTINGS, "settings"),
            (TAG, "tag"),
            (TRASH, "trash"),
            (VOLUME, "volume"),
        ] {
            assert_eq!(icon.name(), name);
            assert!(!icon.shapes().is_empty(), "{name}");
        }
    }
}
