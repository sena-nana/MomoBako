//! 文件右键菜单，对应 Vue `useWorkspaceContextMenu.ts` 的 `fileEntryContextMenu`。
//!
//! 外观、子菜单、键盘和常驻都是共用的右键菜单（`context_menu.rs`），这里只给条目：
//! 「加入播放列表」「缩略图」是分组，回收站里的「彻底删除」要点两次。
//! 来源插件的条目动作不能用时禁用，紧跟着一行弱色小字写原因（Vue 把原因放在禁用项的 `confirmLabel` 里，
//! 禁用的按钮点不了，原因从来显示不出来）。

use nana_ui::icons_tabler::{CLIPBOARD, EYE, FILES, FOLDER_OPEN, PENCIL, PHOTO, PHOTO_OFF, PHOTO_PLUS, REFRESH, ROTATE, TRASH};
use nana_ui::runtime::view::{signal, AnyView};

use super::super::admin::source_target::{self, RepositoryDownload};
use super::super::admin::AdminMessage;
use super::super::context_menu::{context_menu, MenuAction, MenuChrome, MenuEntry, MenuView};
use super::super::entry_actions::{self, FilePluginAction, FilePluginDispatch, SourceContext};
use super::super::files::{BranchSource, FileContext, FileDialog, FileRow, FilesMessage};
use super::super::input::{repository_absolute, InputMessage};
use super::super::view_part_overlay::session::Projected;
use super::super::workspace::WorkspacePanel;
use super::super::{ShellMessage, ShellViewModel};
use super::style;

/// 文件右键菜单的键前缀和消息：分组的进出、要点两次的项都归文件状态管。
const CHROME: MenuChrome = MenuChrome {
    prefix: "file",
    close: || ShellMessage::Files(FilesMessage::CloseEntryMenu),
    branch: Some(|branch, source: BranchSource, inside| ShellMessage::Files(FilesMessage::MenuBranchChanged { branch, source, inside })),
    arm: Some(|id| ShellMessage::Files(FilesMessage::ArmMenuConfirm(id))),
};

fn files(message: FilesMessage) -> MenuAction {
    MenuAction::Files(message)
}

fn input(message: InputMessage) -> MenuAction {
    MenuAction::Input(message)
}

fn admin(message: AdminMessage) -> MenuAction {
    MenuAction::Admin(message)
}

/// 文件右键菜单的投影。
pub(crate) struct EntryMenuView;

impl EntryMenuView {
    /// 目标条目已经不在列表里时为 `None`。
    pub(crate) fn project(model: &ShellViewModel) -> Option<MenuView> {
        let menu = model.files.entry_menu.as_ref()?;
        let ctx = FileContext::from_model(model);
        let row = model.files.visible_rows(&ctx).into_iter().find(|row| row.path == menu.path)?;
        let mut entries = menu_entries(model, &row);
        let pending = model.files.menu_pending.as_deref();
        for entry in &mut entries {
            entry.pending = entry.confirm_label.is_some() && pending == Some(entry.id.as_str());
        }
        Some(MenuView::new(entries, model.files.menu_branch.open().map(str::to_string)))
    }
}

/// 右键打开的条目菜单。没有目标或目标已不在列表里时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.files.entry_menu.clone()?;
    let Some(initial) = EntryMenuView::project(model) else {
        eprintln!("Nana 右键菜单的条目已经不在列表里：{}", menu.path);
        return None;
    };
    let view = signal(initial);
    Projected::register(view, EntryMenuView::project);
    Some(context_menu(view, menu.x, menu.y, CHROME))
}

/// 和 Vue `fileEntryContextMenu` 同样的条目、顺序和禁用规则。
fn menu_entries(model: &ShellViewModel, row: &FileRow) -> Vec<MenuEntry> {
    let repository = model.workspace.active_repository();
    let repo_id = repository.map(|item| item.repo_id.clone());
    let absolute = repository_absolute(&repository.map(|item| item.path.clone()).unwrap_or_default(), &row.path);
    let has_repo = repository.is_some();
    let trash = model.workspace.panel == WorkspacePanel::Trash;
    let smart = model.workspace.panel == WorkspacePanel::SmartFolder;
    let multiple = model.files.selected.len() > 1;
    let mutating = model.files.mutating;
    let file = row.kind == "file";
    let preview = || files(FilesMessage::OpenRow(row.path.clone()));
    let open = input(InputMessage::OpenEntry { has_repo, absolute_path: absolute.clone(), path: row.path.clone() });
    let reveal = input(InputMessage::RevealEntry { absolute_path: absolute.clone() });
    if smart {
        return vec![
            MenuEntry::new("preview", "预览", Some(EYE)).disabled(!file).message(preview()),
            MenuEntry::new("open", "打开", Some(EYE)).disabled(!file).message(open),
            MenuEntry::new("reveal", "定位", Some(FOLDER_OPEN)).message(reveal),
        ];
    }
    let mut entries = Vec::new();
    if trash {
        entries.push(MenuEntry::new("restore", "还原", Some(ROTATE)).disabled(mutating).message(files(FilesMessage::RestoreSelected)));
    }
    entries.push(MenuEntry::new("preview", "预览", Some(EYE)).disabled(!file || trash || multiple).message(preview()));
    let open_entry = if row.kind == "directory" {
        MenuEntry::new("open", "进入", Some(EYE)).message(files(FilesMessage::OpenPath(row.path.clone())))
    } else {
        MenuEntry::new("open", "打开", Some(EYE)).message(open)
    };
    entries.push(open_entry.disabled(trash || multiple));
    entries.push(MenuEntry::new("reveal", "定位", Some(FOLDER_OPEN)).disabled(trash).message(reveal));
    entries.push(
        MenuEntry::new("copy-target", "复制到…", Some(FILES))
            .disabled(trash || mutating)
            .message(files(FilesMessage::OpenDialog(FileDialog::Copy))),
    );
    if !trash && !multiple {
        let playlists = playlist_entries(model, row);
        if !playlists.is_empty() {
            let label = if row.kind == "directory" { "整个加入播放列表" } else { "加入播放列表" };
            let mut group = MenuEntry::new("playlist-membership", label, None);
            group.children = playlists;
            entries.push(group);
        }
    }
    if !trash {
        let context = SourceContext {
            managed_cache_root: repository.and_then(|item| item.cache_path.clone()),
            source_unavailable: repository.is_some_and(|item| item.source_unavailable()),
            has_target_repository: source_target::has_target_repository(&model.workspace),
        };
        let actions = entry_actions::actions_for(&model.admin.plugins, row, &context);
        entries.extend(actions.iter().map(|action| plugin_entry(action, repo_id.clone())));
        // 禁用项后面各写一次原因：同一个原因只写一行。
        let mut reasons: Vec<&'static str> = Vec::new();
        for reason in actions.iter().filter_map(|action| action.unavailable) {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        entries.extend(reasons.into_iter().enumerate().map(|(index, reason)| MenuEntry::note(&format!("plugin-note-{index}"), reason)));
    }
    let mut thumbnail = MenuEntry::new("thumbnail", "缩略图", Some(PHOTO)).disabled(trash);
    thumbnail.children = thumbnail_entries(row, repo_id.clone());
    entries.push(thumbnail);
    entries.push(
        MenuEntry::new("rename", "重命名", Some(PENCIL))
            .disabled(trash || multiple)
            .message(files(FilesMessage::OpenDialog(FileDialog::Rename))),
    );
    let mut delete = MenuEntry::new("delete", if trash { "彻底删除" } else { "删除" }, Some(TRASH))
        .disabled(mutating)
        .danger()
        .message(files(FilesMessage::DeleteSelected));
    delete.confirm_label = trash.then_some("确认彻底删除？再点一次");
    entries.push(delete);
    entries
}

/// 兼容的播放列表：名字前面打勾表示已经在里面。文件切换成员，目录和虚拟条目按路径加入。
fn playlist_entries(model: &ShellViewModel, row: &FileRow) -> Vec<MenuEntry> {
    let extension = row.extension.clone().unwrap_or_default();
    let asset_id = row.asset_id.clone().unwrap_or_default();
    model
        .player
        .membership_actions(&row.kind, &extension, &asset_id, row.is_virtual)
        .into_iter()
        .map(|action| {
            let name = model
                .player
                .playlists
                .iter()
                .find(|playlist| playlist.playlist_id == action.playlist_id)
                .map(|playlist| playlist.name.clone())
                .unwrap_or_else(|| action.label.clone());
            let toggle = MenuAction::Membership {
                playlist_id: action.playlist_id.clone(),
                kind: row.kind.clone(),
                extension: extension.clone(),
                asset_id: asset_id.clone(),
                is_virtual: row.is_virtual,
                path: row.path.clone(),
            };
            let mut entry = MenuEntry::new(&format!("playlist-{}", style::key_part(&action.playlist_id)), name, None).message(toggle);
            entry.checked = action.checked;
            entry
        })
        .collect()
}

/// 来源插件的条目动作。下载到本地先弹目录对话框，下载到其他资源库先选目标资源库；「创建播放列表」在本地建播放集。
/// 不能用的动作禁用，原因写在后面一行。
fn plugin_entry(action: &FilePluginAction, repository_id: Option<String>) -> MenuEntry {
    let message = match &action.dispatch {
        FilePluginDispatch::Call { plugin_id, method, payload } => admin(AdminMessage::CallFilePlugin {
            plugin_id: plugin_id.clone(),
            method: method.clone(),
            payload: payload.clone(),
            repository_id,
        }),
        FilePluginDispatch::AskFolder { plugin_id, method, payload, progress } => input(InputMessage::BeginDownloadFolder {
            plugin_id: plugin_id.clone(),
            method: method.clone(),
            payload: payload.clone(),
            repository_id,
            progress: progress.clone(),
        }),
        FilePluginDispatch::AskRepository { plugin_id, method, payload, progress, parent_path } => {
            admin(AdminMessage::BeginRepositoryDownload(RepositoryDownload {
                plugin_id: plugin_id.clone(),
                method: method.clone(),
                payload: payload.clone(),
                repository_id,
                progress: progress.clone(),
                parent_path: parent_path.clone(),
            }))
        }
        FilePluginDispatch::CreatePlaylist(playlist) => {
            admin(AdminMessage::CreateSourcePlaylist { playlist: playlist.clone(), repository_id })
        }
    };
    MenuEntry::new(&action.value.replace('/', "-"), action.label.clone(), None)
        .disabled(action.unavailable.is_some())
        .message(message)
}

/// 「缩略图」子菜单：选文件、从剪贴板、取消自定义（只有自定义过才可点）、刷新。
fn thumbnail_entries(row: &FileRow, repo_id: Option<String>) -> Vec<MenuEntry> {
    let path = row.path.clone();
    let kind = row.kind.clone();
    let custom = |build: fn(String, String, String) -> InputMessage| -> MenuAction {
        match repo_id.clone() {
            Some(repo_id) => input(build(repo_id, path.clone(), kind.clone())),
            None => {
                eprintln!("Nana 自定义缩略图缺少仓库");
                files(FilesMessage::NoteError("自定义缩略图缺少仓库。".into()))
            }
        }
    };
    vec![
        MenuEntry::new("thumbnail-custom-file", "自定义缩略图（选择文件）", Some(PHOTO_PLUS))
            .message(custom(|repo_id, path, kind| InputMessage::BeginThumbnailFile { repo_id, path, kind })),
        MenuEntry::new("thumbnail-custom-clipboard", "新增自定义缩略图（从剪贴板）", Some(CLIPBOARD))
            .message(custom(|repo_id, path, kind| InputMessage::PasteThumbnail { repo_id, path, kind })),
        MenuEntry::new("thumbnail-clear-custom", "取消自定义缩略图", Some(PHOTO_OFF))
            .disabled(!row.thumbnail_custom)
            .message(custom(|repo_id, path, kind| InputMessage::ClearThumbnail { repo_id, path, kind })),
        MenuEntry::new("thumbnail-refresh", "刷新缩略图", Some(REFRESH))
            .message(files(FilesMessage::RefreshThumbnail(row.path.clone()))),
    ]
}

#[cfg(test)]
#[path = "files_menu_tests.rs"]
mod tests;
