//! 预览与播放用到的 lucide 图标。
//!
//! Vue 端用 `@lucide/vue`，Nana 自带的是 Tabler。播放条、播放集页和预览页的
//! 图标形状要和 Vue 一致，这里把 lucide 的 24×24 源登记成宿主图标
//! （`Icon::from_data`），由 Nana 按 `currentColor` 栅格化。几何只给外框，
//! 绘制只读 SVG 源。

use nana_ui::Icon;
use nana_ui_core::{IconData, IconShape};

/// 只给命中和测试用的外框。图标绘制只读 `svg`。
const FRAME: &[IconShape] = &[IconShape::Rect { origin: [2.0, 2.0], size: [20.0, 20.0], filled: false }];

/// 把 lucide 的子元素包成完整 SVG 源。属性和 lucide 默认值一致：描边 2、圆头圆角。
macro_rules! lucide {
    ($ident:ident, $data:ident, $name:literal, $body:literal) => {
        static $data: IconData = IconData {
            name: concat!("lucide-", $name),
            shapes: FRAME,
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
        pub(crate) const $ident: Icon = Icon::from_data(&$data);
    };
}

lucide!(PLAY, PLAY_DATA, "play", r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>"#);
lucide!(PAUSE, PAUSE_DATA, "pause", r#"<rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/>"#);
lucide!(REPEAT, REPEAT_DATA, "repeat", r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/>"#);
lucide!(
    REPEAT_1,
    REPEAT_1_DATA,
    "repeat-1",
    r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 10h1v4"/>"#
);
lucide!(
    SHUFFLE,
    SHUFFLE_DATA,
    "shuffle",
    r#"<path d="m18 14 4 4-4 4"/><path d="m18 2 4 4-4 4"/><path d="M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22"/><path d="M2 6h1.972a4 4 0 0 1 3.6 2.2"/><path d="M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45"/>"#
);
lucide!(
    SKIP_BACK,
    SKIP_BACK_DATA,
    "skip-back",
    r#"<path d="M17.971 4.285A2 2 0 0 1 21 6v12a2 2 0 0 1-3.029 1.715l-9.997-5.998a2 2 0 0 1-.003-3.432z"/><path d="M3 20V4"/>"#
);
lucide!(
    SKIP_FORWARD,
    SKIP_FORWARD_DATA,
    "skip-forward",
    r#"<path d="M21 4v16"/><path d="M6.029 4.285A2 2 0 0 0 3 6v12a2 2 0 0 0 3.029 1.715l9.997-5.998a2 2 0 0 0 .003-3.432z"/>"#
);
lucide!(
    LIST_MUSIC,
    LIST_MUSIC_DATA,
    "list-music",
    r#"<path d="M16 5H3"/><path d="M11 12H3"/><path d="M11 19H3"/><path d="M21 16V5"/><circle cx="18" cy="16" r="3"/>"#
);
lucide!(
    VOLUME_2,
    VOLUME_2_DATA,
    "volume-2",
    r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#
);
lucide!(
    GRIP_VERTICAL,
    GRIP_VERTICAL_DATA,
    "grip-vertical",
    r#"<circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/>"#
);
lucide!(
    TRASH_2,
    TRASH_2_DATA,
    "trash-2",
    r#"<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#
);
lucide!(ARROW_LEFT, ARROW_LEFT_DATA, "arrow-left", r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#);
lucide!(
    EYE,
    EYE_DATA,
    "eye",
    r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>"#
);
lucide!(
    FOLDER_OPEN,
    FOLDER_OPEN_DATA,
    "folder-open",
    r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#
);
lucide!(
    FILE_AUDIO,
    FILE_AUDIO_DATA,
    "file-headphone",
    r#"<path d="M4 6.835V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.706.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2h-.343"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M2 19a2 2 0 0 1 4 0v1a2 2 0 0 1-4 0v-4a6 6 0 0 1 12 0v4a2 2 0 0 1-4 0v-1a2 2 0 0 1 4 0"/>"#
);
lucide!(
    FILE_VIDEO,
    FILE_VIDEO_DATA,
    "file-play",
    r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M15.033 13.44a.647.647 0 0 1 0 1.12l-4.065 2.352a.645.645 0 0 1-.968-.56v-4.704a.645.645 0 0 1 .967-.56z"/>"#
);
lucide!(
    FILE_IMAGE,
    FILE_IMAGE_DATA,
    "file-image",
    r#"<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><circle cx="10" cy="12" r="2"/><path d="m20 17-1.296-1.296a2.41 2.41 0 0 0-3.408 0L9 22"/>"#
);
