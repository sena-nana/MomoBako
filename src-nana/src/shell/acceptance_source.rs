//! 来源资源库的对照场景：网易云这类来源资源库里单曲的右键菜单，和「下载单曲到其他资源库」弹出的「选择目标资源库」。
//!
//! 当前资源库「网易云音乐」挂在网易云来源插件下，本地缓存在 `D:/MomoCache/netease`；根目录一首「晴天.mp3」，来源
//! 载荷是单曲（`entryKind: track`、`songId: 186016`）。插件是仓库自带的全部清单（`acceptance_base.rs`），网易云的
//! 条目动作照清单出：下载单曲（到本地、到其他资源库）、重新获取播放资源、清理播放缓存。状态都走产品的启动、
//! 右键和菜单消息。

use crate::backend::services::repository::{FileBrowserEntry, RepositorySummary};

use super::super::files::FilesMessage;
use super::super::files_view::EntryMenuView;
use super::super::{ShellMessage, ShellViewModel};
use super::base_scene::{file, settle, summary, Base, SourceState};
use super::files_scenes::asset_detail;

/// 右键落点，和文件页的右键场景一样。
const MENU_POINT: (f32, f32) = (400.0, 470.0);
/// 根目录里的单曲。
const TRACK: &str = "晴天.mp3";

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![("source-menu-unavailable", unavailable_menu()), ("source-download-target", download_target())]
}

/// 网易云的单曲：来源载荷带条目种类和歌曲标识。
fn track() -> FileBrowserEntry {
    let mut entry = file(TRACK, 4_200_000);
    entry.provider_id = Some("netease-cloud-music".into());
    entry.provider_item_id = Some("186016".into());
    entry.source_payload = Some(serde_json::json!({ "entryKind": "track", "songId": 186016, "songName": "晴天" }));
    entry
}

/// 资源库列表里能当下载目标的另一个本地资源库。
fn other(repo_id: &str, name: &'static str, path: &str) -> RepositorySummary {
    RepositorySummary { repo_id: repo_id.into(), name: name.into(), path: path.into(), ..summary(name, "ready") }
}

/// 网易云音乐资源库，登录状态照 `logged_in` / `login_expired`；`others` 是列表里别的资源库。
fn netease(logged_in: bool, login_expired: bool, others: Vec<RepositorySummary>) -> ShellViewModel {
    Base {
        name: "网易云音乐",
        entries: vec![track()],
        tree: Vec::new(),
        others,
        source: Some(SourceState {
            plugin_id: "momobako.netease.source",
            kind: "netease-cloud-music",
            cache_path: "D:/MomoCache/netease",
            logged_in,
            login_expired,
        }),
        ..Base::default()
    }
    .model()
}

/// 右键单曲：选中它、打开菜单，再照 Vue 模拟 IPC 答上右键选中时读的素材详情。
fn open_menu(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu { path: TRACK.into(), x: MENU_POINT.0, y: MENU_POINT.1 }));
    match model.files.rows.iter().find(|row| row.path == TRACK).cloned() {
        Some(row) => model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(&row)))),
        None => eprintln!("Nana 来源场景里没有 {TRACK}"),
    }
    let _ = model.inspect.take_effects();
}

/// 登录过期：照 Vue `actionDisabled`，网易云的条目动作都禁用，后面一行写「来源认证或本地缓存不可用」。
fn unavailable_menu() -> ShellViewModel {
    let mut model = netease(true, true, vec![other("second-repo", "第二资源库", "C:/second")]);
    open_menu(&mut model);
    settle(&mut model);
    model
}

/// 登录正常、列表里还有两个本地资源库：右键单曲 →「下载单曲到其他资源库」，弹出「选择目标资源库」列出这两个。
fn download_target() -> ShellViewModel {
    let others = vec![other("second-repo", "第二资源库", "C:/second"), other("third-repo", "第三资源库", "D:/third")];
    let mut model = netease(true, false, others);
    open_menu(&mut model);
    let action = EntryMenuView::project(&model)
        .and_then(|menu| menu.entries.into_iter().find(|entry| entry.label == "下载单曲到其他资源库"))
        .and_then(|entry| entry.action);
    match action {
        Some(action) => {
            model.reduce(ShellMessage::Files(FilesMessage::CloseEntryMenu));
            model.reduce(action.message());
        }
        None => eprintln!("Nana 来源场景的右键菜单里没有「下载单曲到其他资源库」"),
    }
    settle(&mut model);
    model
}
