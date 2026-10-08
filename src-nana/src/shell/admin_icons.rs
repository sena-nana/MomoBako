//! 设置、插件、日志页用到的 lucide 图标。
//!
//! Vue 用 `@lucide/vue`，Nana 内置的是 Tabler。图标形状要和 Vue 一致，这里把用到的
//! lucide 源 SVG（ISC 许可，与前端依赖同源）包成宿主 `IconData`，由 Nana 的图标栅格化
//! 路径按 `currentColor` 着色。只收录本模块真正用到的图标。

use nana_ui_core::{Icon, IconData, IconShape};

/// 栅格化只读 `svg`，命中测试按节点矩形，所以不另存几何。
const EMPTY: &[IconShape] = &[];

/// 包一份静态 lucide 数据。
const fn lucide(data: &'static IconData) -> Icon {
    Icon::from_data(data)
}

/// lucide `circle-check`（`CheckCircle2`）。
pub(crate) const CHECK_CIRCLE2: Icon = lucide(&CHECK_CIRCLE2_DATA);
static CHECK_CIRCLE2_DATA: IconData = IconData { name: "lucide-circle-check", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/></svg>"# };
/// lucide `copy`（`Copy`）。
pub(crate) const COPY: Icon = lucide(&COPY_DATA);
static COPY_DATA: IconData = IconData { name: "lucide-copy", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>"# };
/// lucide `database`（`Database`）。
pub(crate) const DATABASE: Icon = lucide(&DATABASE_DATA);
static DATABASE_DATA: IconData = IconData { name: "lucide-database", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/></svg>"# };
/// lucide `download`（`Download`）。
pub(crate) const DOWNLOAD: Icon = lucide(&DOWNLOAD_DATA);
static DOWNLOAD_DATA: IconData = IconData { name: "lucide-download", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/></svg>"# };
/// lucide `file-braces`（`FileJson`）。
pub(crate) const FILE_JSON: Icon = lucide(&FILE_JSON_DATA);
static FILE_JSON_DATA: IconData = IconData { name: "lucide-file-braces", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 12a1 1 0 0 0-1 1v1a1 1 0 0 1-1 1 1 1 0 0 1 1 1v1a1 1 0 0 0 1 1"/><path d="M14 18a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1 1 1 0 0 1-1-1v-1a1 1 0 0 0-1-1"/></svg>"# };
/// lucide `moon`（`Moon`）。
pub(crate) const MOON: Icon = lucide(&MOON_DATA);
static MOON_DATA: IconData = IconData { name: "lucide-moon", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401"/></svg>"# };
/// lucide `radius`（`Radius`）。
pub(crate) const RADIUS: Icon = lucide(&RADIUS_DATA);
static RADIUS_DATA: IconData = IconData { name: "lucide-radius", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.34 17.52a10 10 0 1 0-2.82 2.82"/><circle cx="19" cy="19" r="2"/><path d="m13.41 13.41 4.18 4.18"/><circle cx="12" cy="12" r="2"/></svg>"# };
/// lucide `server-cog`（`ServerCog`）。
pub(crate) const SERVER_COG: Icon = lucide(&SERVER_COG_DATA);
static SERVER_COG_DATA: IconData = IconData { name: "lucide-server-cog", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m10.852 14.772-.383.923"/><path d="M13.148 14.772a3 3 0 1 0-2.296-5.544l-.383-.923"/><path d="m13.148 9.228.383-.923"/><path d="m13.53 15.696-.382-.924a3 3 0 1 1-2.296-5.544"/><path d="m14.772 10.852.923-.383"/><path d="m14.772 13.148.923.383"/><path d="M4.5 10H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2h-.5"/><path d="M4.5 14H4a2 2 0 0 0-2 2v4a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-4a2 2 0 0 0-2-2h-.5"/><path d="M6 18h.01"/><path d="M6 6h.01"/><path d="m9.228 10.852-.923-.383"/><path d="m9.228 13.148-.923.383"/></svg>"# };
/// lucide `square-round-corner`（`SquareRoundCorner`）。
pub(crate) const SQUARE_ROUND_CORNER: Icon = lucide(&SQUARE_ROUND_CORNER_DATA);
static SQUARE_ROUND_CORNER_DATA: IconData = IconData { name: "lucide-square-round-corner", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11a8 8 0 0 0-8-8"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/></svg>"# };
/// lucide `sun`（`Sun`）。
pub(crate) const SUN: Icon = lucide(&SUN_DATA);
static SUN_DATA: IconData = IconData { name: "lucide-sun", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></svg>"# };
/// lucide `volume-2`（`Volume2`）。
pub(crate) const VOLUME2: Icon = lucide(&VOLUME2_DATA);
static VOLUME2_DATA: IconData = IconData { name: "lucide-volume-2", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/></svg>"# };
/// lucide `folder-open`（`FolderOpen`）。
pub(crate) const FOLDER_OPEN: Icon = lucide(&FOLDER_OPEN_DATA);
static FOLDER_OPEN_DATA: IconData = IconData { name: "lucide-folder-open", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/></svg>"# };
/// lucide `power`（`Power`）。
pub(crate) const POWER: Icon = lucide(&POWER_DATA);
static POWER_DATA: IconData = IconData { name: "lucide-power", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/></svg>"# };
/// lucide `refresh-cw`（`RefreshCw`）。
pub(crate) const REFRESH_CW: Icon = lucide(&REFRESH_CW_DATA);
static REFRESH_CW_DATA: IconData = IconData { name: "lucide-refresh-cw", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>"# };
/// lucide `settings`（`Settings`）。
pub(crate) const SETTINGS: Icon = lucide(&SETTINGS_DATA);
static SETTINGS_DATA: IconData = IconData { name: "lucide-settings", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/></svg>"# };
/// lucide `trash-2`（`Trash2`）。
pub(crate) const TRASH2: Icon = lucide(&TRASH2_DATA);
static TRASH2_DATA: IconData = IconData { name: "lucide-trash-2", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>"# };
/// lucide `upload`（`Upload`）。
pub(crate) const UPLOAD: Icon = lucide(&UPLOAD_DATA);
static UPLOAD_DATA: IconData = IconData { name: "lucide-upload", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m17 8-5-5-5 5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/></svg>"# };
/// lucide `key-round`（`KeyRound`）。
pub(crate) const KEY_ROUND: Icon = lucide(&KEY_ROUND_DATA);
static KEY_ROUND_DATA: IconData = IconData { name: "lucide-key-round", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"/><circle cx="16.5" cy="7.5" r=".5" fill="currentColor"/></svg>"# };
/// lucide `loader-circle`（`LoaderCircle`）。
pub(crate) const LOADER_CIRCLE: Icon = lucide(&LOADER_CIRCLE_DATA);
static LOADER_CIRCLE_DATA: IconData = IconData { name: "lucide-loader-circle", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>"# };
/// lucide `log-out`（`LogOut`）。
pub(crate) const LOG_OUT: Icon = lucide(&LOG_OUT_DATA);
static LOG_OUT_DATA: IconData = IconData { name: "lucide-log-out", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/></svg>"# };
/// lucide `clipboard-list`（`ClipboardList`）。
pub(crate) const CLIPBOARD_LIST: Icon = lucide(&CLIPBOARD_LIST_DATA);
static CLIPBOARD_LIST_DATA: IconData = IconData { name: "lucide-clipboard-list", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><path d="M12 11h4"/><path d="M12 16h4"/><path d="M8 11h.01"/><path d="M8 16h.01"/></svg>"# };
/// lucide `x`（`X`）。
pub(crate) const X: Icon = lucide(&X_DATA);
static X_DATA: IconData = IconData { name: "lucide-x", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"# };
/// lucide `eraser`（`Eraser`）。
pub(crate) const ERASER: Icon = lucide(&ERASER_DATA);
static ERASER_DATA: IconData = IconData { name: "lucide-eraser", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 21H8a2 2 0 0 1-1.42-.587l-3.994-3.999a2 2 0 0 1 0-2.828l10-10a2 2 0 0 1 2.829 0l5.999 6a2 2 0 0 1 0 2.828L12.834 21"/><path d="m5.082 11.09 8.828 8.828"/></svg>"# };
/// lucide `pause`（`Pause`）。
pub(crate) const PAUSE: Icon = lucide(&PAUSE_DATA);
static PAUSE_DATA: IconData = IconData { name: "lucide-pause", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/></svg>"# };
/// lucide `play`（`Play`）。
pub(crate) const PLAY: Icon = lucide(&PLAY_DATA);
static PLAY_DATA: IconData = IconData { name: "lucide-play", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/></svg>"# };
/// lucide `search`（`Search`）。
pub(crate) const SEARCH: Icon = lucide(&SEARCH_DATA);
static SEARCH_DATA: IconData = IconData { name: "lucide-search", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/></svg>"# };
/// lucide `shield-alert`（`ShieldAlert`）。
pub(crate) const SHIELD_ALERT: Icon = lucide(&SHIELD_ALERT_DATA);
static SHIELD_ALERT_DATA: IconData = IconData { name: "lucide-shield-alert", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>"# };
/// lucide `chevron-down`（`ChevronDown`）。
pub(crate) const CHEVRON_DOWN: Icon = lucide(&CHEVRON_DOWN_DATA);
static CHEVRON_DOWN_DATA: IconData = IconData { name: "lucide-chevron-down", shapes: EMPTY, svg: r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"# };
