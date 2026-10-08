//! 文件右键菜单。
//!
//! 菜单项接到已有的打开、定位、播放集和文件消息。自定义缩略图走编号 6 的选文件对话框、
//! 已有剪贴板和 `clear`。刷新仍走现有缩略图解码。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{ContextMenu, ContextMenuEvent, ContextMenuItem};

use super::super::admin::AdminMessage;
use super::super::entry_actions::{self, FilePluginAction, FilePluginDispatch};
use super::super::files::{FileContext, FileDialog, FileRow, FilesMessage};
use super::super::input::{repository_absolute, InputMessage};
use super::super::player::PlayerMessage;
use super::super::workspace::WorkspacePanel;
use super::super::{ShellMessage, ShellViewModel};

/// 右键时带上的条目，菜单回调不再回读会变掉的视图模型。
#[derive(Clone)]
struct MenuTarget {
    path: String,
    kind: String,
    extension: String,
    asset_id: String,
    is_virtual: bool,
    absolute: String,
    has_repo: bool,
    multiple: bool,
    trash: bool,
    smart: bool,
    thumbnail_custom: bool,
    repo_id: Option<String>,
    plugin_actions: Vec<FilePluginAction>,
}

/// 右键打开的条目菜单。没有目标时不占浮层。
pub(super) fn entry_menu(model: &ShellViewModel) -> Option<AnyView> {
    let menu = model.files.entry_menu.clone()?;
    let ctx = FileContext::from_model(model);
    let row = model.files.visible_rows(&ctx).into_iter().find(|row| row.path == menu.path)?;
    let target = MenuTarget::from_model(model, &row);
    let items = menu_items(model, &target);
    Some(
        widget(ContextMenu::new(menu.x, menu.y).items(items))
            .key("file-context-menu")
            .on_cx(move |_, event: &ContextMenuEvent, cx| match event {
                ContextMenuEvent::Search(_) => {}
                ContextMenuEvent::Dismiss => {
                    cx.dispatch_program(ShellMessage::Files(FilesMessage::CloseEntryMenu));
                }
                ContextMenuEvent::Select(value) => {
                    cx.dispatch_program(ShellMessage::Files(FilesMessage::CloseEntryMenu));
                    if let Some(message) = target.action(value) {
                        cx.dispatch_program(message);
                    }
                }
            })
            .into_any(),
    )
}

impl MenuTarget {
    fn from_model(model: &ShellViewModel, row: &FileRow) -> Self {
        let repository = model.workspace.active_repository();
        let root = repository.map(|item| item.path.clone()).unwrap_or_default();
        Self {
            path: row.path.clone(),
            kind: row.kind.clone(),
            extension: row.extension.clone().unwrap_or_default(),
            asset_id: row.asset_id.clone().unwrap_or_default(),
            is_virtual: row.is_virtual,
            absolute: repository_absolute(&root, &row.path),
            has_repo: repository.is_some(),
            multiple: model.files.selected.len() > 1,
            trash: model.workspace.panel == WorkspacePanel::Trash,
            smart: model.workspace.panel == WorkspacePanel::SmartFolder,
            thumbnail_custom: row.thumbnail_custom,
            repo_id: repository.map(|item| item.repo_id.clone()),
            plugin_actions: entry_actions::actions_for(&model.admin.plugins, row),
        }
    }

    /// 把菜单值收成一条已有的壳层消息。未知值只记日志。
    fn action(&self, value: &str) -> Option<ShellMessage> {
        if let Some(action) = self.plugin_actions.iter().find(|action| action.value == value) {
            return Some(match &action.dispatch {
                FilePluginDispatch::Call { plugin_id, method, payload } => ShellMessage::Admin(AdminMessage::CallFilePlugin {
                    plugin_id: plugin_id.clone(),
                    method: method.clone(),
                    payload: payload.clone(),
                    repository_id: self.repo_id.clone(),
                }),
                FilePluginDispatch::AskFolder { plugin_id, method, payload } => ShellMessage::Input(InputMessage::BeginDownloadFolder {
                    plugin_id: plugin_id.clone(),
                    method: method.clone(),
                    payload: payload.clone(),
                    repository_id: self.repo_id.clone(),
                }),
                FilePluginDispatch::AskName { plugin_id, method, payload } => ShellMessage::Input(InputMessage::OpenSourcePlaylist {
                    plugin_id: plugin_id.clone(),
                    method: method.clone(),
                    payload: payload.clone(),
                    repository_id: self.repo_id.clone(),
                }),
            });
        }
        if let Some(playlist_id) = value.strip_prefix("playlist/") {
            return Some(ShellMessage::Player(PlayerMessage::ToggleMembership {
                playlist_id: playlist_id.to_string(),
                kind: self.kind.clone(),
                extension: self.extension.clone(),
                asset_id: self.asset_id.clone(),
                is_virtual: self.is_virtual,
                path: self.path.clone(),
            }));
        }
        let message = match value {
            "preview" | "enter" => ShellMessage::Files(FilesMessage::OpenRow(self.path.clone())),
            "open" => ShellMessage::Input(InputMessage::OpenEntry {
                has_repo: self.has_repo,
                absolute_path: self.absolute.clone(),
            }),
            "reveal" => ShellMessage::Input(InputMessage::RevealEntry { absolute_path: self.absolute.clone() }),
            "copy" => ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Copy)),
            "rename" => ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::Rename)),
            "delete" => ShellMessage::Files(FilesMessage::DeleteSelected),
            "restore" => ShellMessage::Files(FilesMessage::RestoreSelected),
            "thumbnail/refresh" => ShellMessage::Files(FilesMessage::RefreshThumbnail(self.path.clone())),
            "thumbnail/file" => self.thumbnail_message(ThumbnailMenu::File),
            "thumbnail/clipboard" => self.thumbnail_message(ThumbnailMenu::Clipboard),
            "thumbnail/clear" => self.thumbnail_message(ThumbnailMenu::Clear),
            other => {
                eprintln!("Nana 忽略未知的文件菜单项：{other}");
                return None;
            }
        };
        Some(message)
    }

    /// 选文件、剪贴板和取消都要带上仓库。没有仓库时只记失败。
    fn thumbnail_message(&self, menu: ThumbnailMenu) -> ShellMessage {
        let Some(repo_id) = self.repo_id.clone() else {
            eprintln!("Nana 自定义缩略图缺少仓库");
            return ShellMessage::Files(FilesMessage::NoteError("自定义缩略图缺少仓库。".into()));
        };
        let path = self.path.clone();
        let kind = self.kind.clone();
        ShellMessage::Input(match menu {
            ThumbnailMenu::File => InputMessage::BeginThumbnailFile { repo_id, path, kind },
            ThumbnailMenu::Clipboard => InputMessage::PasteThumbnail { repo_id, path, kind },
            ThumbnailMenu::Clear => InputMessage::ClearThumbnail { repo_id, path, kind },
        })
    }
}

enum ThumbnailMenu {
    File,
    Clipboard,
    Clear,
}

fn menu_items(model: &ShellViewModel, target: &MenuTarget) -> Vec<ContextMenuItem> {
    if target.smart {
        return vec![
            item("preview", "预览", target.kind != "file"),
            item("open", "打开", target.kind != "file"),
            item("reveal", "定位", false),
        ];
    }
    let mut items = Vec::new();
    if target.trash {
        items.push(item("restore", "还原", model.files.mutating));
    }
    items.push(item("preview", "预览", target.kind != "file" || target.trash || target.multiple));
    if target.kind == "directory" {
        items.push(item("enter", "进入", target.trash || target.multiple));
    } else {
        items.push(item("open", "打开", target.trash || target.multiple));
    }
    items.push(item("reveal", "定位", target.trash));
    items.push(item("copy", "复制到…", target.trash || model.files.mutating));
    if !target.trash && !target.multiple {
        let membership = model.player.membership_actions(&target.kind, &target.extension, &target.asset_id, target.is_virtual);
        if !membership.is_empty() {
            items.push(ContextMenuItem::new("playlist", "加入播放列表"));
            for action in membership {
                items.push(ContextMenuItem::new(format!("playlist/{}", action.playlist_id), action.label).disabled(!action.toggle));
            }
        }
    }
    for action in &target.plugin_actions {
        items.push(ContextMenuItem::new(action.value.clone(), action.label.clone()).disabled(target.trash));
    }
    items.push(ContextMenuItem::new("thumbnail", "缩略图").disabled(target.trash));
    items.push(ContextMenuItem::new("thumbnail/refresh", "刷新缩略图").disabled(target.trash));
    items.push(ContextMenuItem::new("thumbnail/file", "自定义缩略图（选择文件）").disabled(target.trash));
    items.push(ContextMenuItem::new("thumbnail/clipboard", "新增自定义缩略图（从剪贴板）").disabled(target.trash));
    items.push(ContextMenuItem::new("thumbnail/clear", "取消自定义缩略图").disabled(target.trash || !target.thumbnail_custom));
    items.push(item("rename", "重命名", target.trash || target.multiple));
    items.push(
        ContextMenuItem::new("delete", if target.trash { "彻底删除" } else { "删除" })
            .hint(if target.trash { "不可恢复" } else { "移入回收站" })
            .danger(true)
            .disabled(model.files.mutating),
    );
    items
}

fn item(value: &str, label: &str, disabled: bool) -> ContextMenuItem {
    ContextMenuItem::new(value, label).disabled(disabled)
}
