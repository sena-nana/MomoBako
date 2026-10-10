//! 文件表面的消息归约：壳层入口 `reduce_message`、文件状态自己的分派 `FilesState::reduce`，
//! 以及 Escape 一层层关掉文件页浮层的顺序。
//!
//! 壳层入口先处理要读整个 ViewModel 的消息（外部拖放、导出的保存位置、标签菜单和新标签），再按当前
//! 仓库和面板算出 `FileContext` 交给文件状态；单击、双击和右键以后右侧详情跟去的文件也在这里换。

use super::super::input;
use super::super::inspect::InspectMessage;
use super::super::workspace::WorkspacePanel;
use super::super::{ShellMessage, ShellViewModel};
use super::{FileContext, FileDialog, FilesEffect, FilesMessage, FilesState, MenuBranch, SelectionMode};

/// 文件页里 Escape 能关掉的一层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesLayer {
    EntryMenu,
    ImportMenu,
    Export,
    Hardlink,
    Dialog,
}

impl FilesState {
    pub(in crate::shell) fn reduce(&mut self, ctx: &FileContext, message: FilesMessage) {
        match message {
            FilesMessage::SetDisplayMode(mode) => {
                self.display_mode = mode;
                self.effects.push(FilesEffect::PersistDisplayMode);
            }
            FilesMessage::ActivateRow(path) => {
                self.activate_row(ctx, &path, SelectionMode::Replace);
            }
            FilesMessage::OpenRow(path) => {
                self.select_only = None;
                self.click_load = None;
                self.open_row(ctx, &path);
            }
            FilesMessage::OpenEntryMenu { path, x, y } => self.open_entry_menu(ctx, &path, x, y),
            FilesMessage::CloseEntryMenu => {
                self.entry_menu = None;
                self.menu_branch = MenuBranch::default();
                self.menu_pending = None;
            }
            FilesMessage::ArmMenuConfirm(item) => self.menu_pending = Some(item),
            FilesMessage::MenuBranchChanged { branch, source, inside } if self.entry_menu.is_some() => self.menu_branch.note(source, branch, inside),
            // 菜单关掉以后才到的进出（节点拆掉时的离开）不再记。
            FilesMessage::MenuBranchChanged { .. } => {}
            FilesMessage::ListResized(width) => self.note_list_width(width),
            FilesMessage::ToggleTags { path, expanded } => self.tags_open = Some((path, !expanded)),
            FilesMessage::SetTagDraft(value) => self.tag_draft = value,
            // 这两条要改检视草稿，在 `reduce_message` 里转成检视消息。
            FilesMessage::ToggleTagMenu | FilesMessage::SubmitTagDraft(_) => {}
            FilesMessage::CopyText(text) => {
                if text.is_empty() {
                    eprintln!("Nana 没有可复制的文字");
                } else {
                    self.effects.push(FilesEffect::CopyText(text));
                }
            }
            FilesMessage::RefreshThumbnail(path) => self.refresh_thumbnail(&path),
            FilesMessage::OpenPath(path) => {
                self.select_only = None;
                self.click_load = None;
                self.request_browse(ctx, &path);
            }
            FilesMessage::LoadMore => {
                self.load_more(ctx);
            }
            FilesMessage::OpenDialog(dialog) => {
                self.open_dialog(ctx, dialog);
            }
            FilesMessage::OpenEagle(mode) => {
                self.open_eagle(ctx, &mode);
            }
            FilesMessage::ToggleImportMenu => self.toggle_import_menu(),
            FilesMessage::ToggleEagleImport => self.toggle_eagle_menu(),
            FilesMessage::SetCreateName(value) => self.create_name = value,
            FilesMessage::SubmitCreateFile => {
                self.submit_inline_file(ctx);
            }
            FilesMessage::DraftChanged(value) => self.set_draft(value),
            FilesMessage::CloseDialog => {
                self.close_dialog();
            }
            FilesMessage::SubmitDialog => {
                self.submit_dialog(ctx);
            }
            FilesMessage::SetExportField { field, value } => self.set_export_field(&field, value),
            FilesMessage::CloseExport => self.close_export(),
            FilesMessage::SubmitExport => self.submit_export(ctx),
            FilesMessage::ChooseExportOutput => {}
            FilesMessage::ExportFinished(result) => self.note_export(result),
            FilesMessage::DeleteSelected => {
                self.delete_selected(ctx);
            }
            FilesMessage::RestoreSelected => {
                self.restore_selected(ctx);
            }
            FilesMessage::RestoreAll => {
                self.mutate_trash(ctx, "restoreAll", Vec::new(), None);
            }
            FilesMessage::EmptyTrash => {
                self.mutate_trash(ctx, "empty", Vec::new(), None);
            }
            FilesMessage::SkipHardlink => {
                self.skip_hardlink();
            }
            FilesMessage::ConfirmHardlink => {
                self.confirm_hardlink(ctx);
            }
            FilesMessage::MutationSnapshot { result, created_name } => match result {
                Ok(snapshot) => self.apply_mutation_snapshot(ctx, snapshot, created_name),
                Err(error) => self.note_mutation_failed(error),
            },
            FilesMessage::ProtocolFinished { result, reload, hardlinks } => {
                self.note_protocol_finished(ctx, result, reload, hardlinks);
            }
            FilesMessage::HardlinksLoaded(result) => self.note_hardlinks(result),
            FilesMessage::HardlinksRefreshed(result) => self.note_hardlinks_silent(result),
            FilesMessage::HardlinksChecked(Ok(prompts)) => self.note_hardlinks_synced(prompts),
            FilesMessage::HardlinksChecked(Err(error)) => eprintln!("Nana 后台读取硬链接候选失败：{error}"),
            FilesMessage::HardlinkConfirmed { result, .. } => self.note_hardlink_confirmed(result),
            FilesMessage::NoteError(error) => self.note_error(error),
            FilesMessage::ThumbnailSaved { path, thumbnail_path, custom } => self.note_custom_thumbnail(&path, thumbnail_path, custom),
            // 要读仓库和面板，在 `reduce_message` 里转成宿主拖放消息。
            FilesMessage::HostDrop(_) => {}
        }
    }

    /// Escape 现在能关掉文件页的哪一层：右键菜单、导入菜单、导出对话框、硬链接确认、文件对话框。
    /// 导出中和变更进行中的对话框不关，没有可关的为 `None`。
    pub(crate) fn escape_layer(&self) -> Option<FilesLayer> {
        if self.entry_menu.is_some() {
            Some(FilesLayer::EntryMenu)
        } else if self.import_open {
            Some(FilesLayer::ImportMenu)
        } else if self.export.open && !self.export.busy {
            Some(FilesLayer::Export)
        } else if self.dialog == FileDialog::Hardlink && !self.mutating {
            Some(FilesLayer::Hardlink)
        } else if self.dialog != FileDialog::Closed && !self.mutating {
            Some(FilesLayer::Dialog)
        } else {
            None
        }
    }

    /// Escape 关掉文件页最上面的一层，顺序见 [`Self::escape_layer`]。没有可关的返回 false，交给壳层继续处理。
    pub(crate) fn dismiss_overlay(&mut self) -> bool {
        match self.escape_layer() {
            Some(FilesLayer::EntryMenu) => {
                self.entry_menu = None;
                self.menu_branch = MenuBranch::default();
                self.menu_pending = None;
                true
            }
            Some(FilesLayer::ImportMenu) => {
                self.import_open = false;
                self.eagle_open = false;
                true
            }
            Some(FilesLayer::Export) => {
                self.close_export();
                true
            }
            Some(FilesLayer::Hardlink) => self.skip_hardlink(),
            Some(FilesLayer::Dialog) => self.close_dialog(),
            None => false,
        }
    }
}

/// 壳层把文件消息交到这里。不是文件消息的原样还回去；要换成别的壳层消息时返回那条消息。
pub(in crate::shell) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    if let ShellMessage::AssetDetailLoaded(Ok(detail)) = &message {
        model.files.note_detail_arrived(&detail.summary.path);
    }
    let ShellMessage::Files(message) = message else {
        return Some(message);
    };
    if let FilesMessage::HardlinkConfirmed { repo_id, result } = message {
        super::hardlink::finish_confirm(model, &repo_id, result);
        return None;
    }
    // 拖放的意图按此刻的仓库条件（有没有仓库、可写、在文件面板）收成宿主拖放消息，交给输入归约。
    if let FilesMessage::HostDrop(event) = &message {
        let flags = input::file_drop_flags(model);
        let host = input::file_drop_message(&flags, event);
        return input::reduce_message(model, host);
    }
    // 压缩包导出还没有保存位置时，先弹系统保存对话框，不把空路径交给导出协议。
    let export = &model.files.export;
    let archive_without_path = export.target != "git" && export.output_path.trim().is_empty() && !export.busy;
    let message = if matches!(message, FilesMessage::SubmitExport) && archive_without_path {
        FilesMessage::ChooseExportOutput
    } else {
        message
    };
    if matches!(message, FilesMessage::ChooseExportOutput) {
        let extension = model.files.export.format.trim().to_string();
        let file_name = model.workspace.active_repository().and_then(|repository| {
            let name = repository.name.trim();
            if name.is_empty() || extension.is_empty() { None } else { Some(format!("{name}.{extension}")) }
        });
        model.input.queue_repository_export_dialog(file_name, extension);
        return None;
    }
    match &message {
        FilesMessage::ToggleTagMenu => {
            model.files.tag_draft.clear();
            let next = if model.inspect.tag_menu_open() {
                InspectMessage::CloseTagMenu
            } else {
                InspectMessage::OpenTagMenu
            };
            return Some(ShellMessage::Inspect(next));
        }
        FilesMessage::SubmitTagDraft(value) => {
            let tag = value.trim().to_string();
            if tag.is_empty() {
                return None;
            }
            model.files.tag_draft.clear();
            return Some(ShellMessage::Inspect(InspectMessage::AddTag(tag)));
        }
        _ => {}
    }
    let context = FileContext::from_model(model);
    let show_on_files = matches!(
        message,
        FilesMessage::OpenDialog(FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle)
            | FilesMessage::OpenEagle(_)
    );
    let preview_path = match message {
        FilesMessage::ActivateRow(path) => model.files.activate_row(&context, &path, model.input.modifiers.selection_mode()),
        FilesMessage::OpenRow(ref path) | FilesMessage::OpenEntryMenu { ref path, .. } => {
            let path = path.clone();
            model.files.reduce(&context, message);
            Some(path)
        }
        message => {
            model.files.reduce(&context, message);
            None
        }
    };
    if let Some(path) = preview_path {
        let file = model.files.visible_rows(&context).into_iter().find(|row| row.path == path && row.kind != "directory");
        if let Some(row) = file {
            if model.inspect.target_path.as_deref() != Some(row.path.as_str()) {
                model.inspect.begin_selection(&row.path);
            }
        }
    }
    if show_on_files && model.files.dialog != FileDialog::Closed {
        model.workspace.panel = WorkspacePanel::Files;
    }
    None
}
