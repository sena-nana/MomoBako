//! 侧栏文件夹也是放下目标。
//!
//! Vue `FolderTreeNode` 在行上写 `data-folder-path`：内部拖动松手把条目移进这个文件夹，
//! 外部文件放下则导入到这里（`useFolderSidebarUi.ts` 的 `handleFolderDrop`）。
//! 指针停在侧栏这一行上满 450ms，就展开并打开它（`handleFolderDragHover`）。
//! 文件列表里的目录卡片仍按原来的命中处理，不把侧栏行算进框选。

use super::{RowMark, ShellViewModel};

/// 指针底下的文件夹。`sidebar` 为真时是目录树的行，否则是文件列表里的目录卡片。
pub(super) struct FolderHit {
    pub path: String,
    pub sidebar: bool,
}

/// 文件列表的行。侧栏目录不参与框选、列表滚动区和「指针在文件区上」。
pub(super) fn file_rows(rows: &[RowMark]) -> Vec<RowMark> {
    rows.iter()
        .filter(|row| row.kind != "sidebar")
        .cloned()
        .collect()
}

/// 指针落在哪个文件夹上。拖着的那张目录卡片自己除外；侧栏上的同一路径仍算目标。
/// 同时盖住指针时取得更小的那一块，避免整棵树的盒子把列表卡片抢走。
pub(super) fn directory_at(rows: &[RowMark], x: f32, y: f32, exclude: &str) -> Option<FolderHit> {
    let mut best: Option<&RowMark> = None;
    for row in rows {
        let sidebar = row.kind == "sidebar";
        let directory = row.kind == "directory";
        if !sidebar && !directory || row.path.is_empty() {
            continue;
        }
        if directory && row.path == exclude {
            continue;
        }
        if !super::contains(row.bounds, x, y) {
            continue;
        }
        let tighter = best.is_none_or(|current| area(row) < area(current));
        if tighter {
            best = Some(row);
        }
    }
    best.map(|row| FolderHit {
        path: row.path.clone(),
        sidebar: row.kind == "sidebar",
    })
}

fn area(row: &RowMark) -> f32 {
    row.bounds.width * row.bounds.height
}

/// 外部文件拖过文件夹时记下路径，离开就清掉。
///
/// 对应 Vue 文件夹上的 `dragover` / `dragleave`。文件列里从一张卡片移到另一张不会再发
/// `FileDropEvent`，所以按每次 `FileDrag` 的坐标命中。侧栏这一行另外记下，文件列的
/// `Leave` 才不会把刚记上的目标清掉；停在侧栏行上时由准备帧把 450ms 走完。
/// 内部拖动自己在算悬停，这里不覆盖。回收站不记。坐标无效只记日志。
pub(crate) fn note_external_folder_drag(
    model: &mut ShellViewModel,
    document: &nana_ui::runtime::RuntimeDocument,
    payload: &nana_ui::InputPayload,
) {
    let nana_ui::InputPayload::FileDrag(drag) = payload else {
        return;
    };
    if model.input.internal_active || model.workspace.panel == super::WorkspacePanel::Trash {
        return;
    }
    let hit = match drag.kind {
        nana_ui::FileDragKind::Hover | nana_ui::FileDragKind::Drop => {
            drag.position.and_then(|(x, y)| {
                if !x.is_finite() || !y.is_finite() {
                    eprintln!("Nana 外部拖放坐标无效，文件夹悬停不更新：{x},{y}");
                    return None;
                }
                let rows = super::collect_rows(document.context().world(), document.document());
                directory_at(&rows, x, y, "")
            })
        }
        nana_ui::FileDragKind::Cancel => None,
    };
    match &hit {
        Some(folder) if model.input.hover_folder.as_deref() != Some(folder.path.as_str()) => {
            model.reduce(super::ShellMessage::Input(
                super::InputMessage::FolderHover {
                    path: folder.path.clone(),
                    trash: false,
                },
            ));
        }
        None => {
            if let Some(path) = model.input.hover_folder.clone() {
                model.reduce(super::ShellMessage::Input(
                    super::InputMessage::FolderLeave(path),
                ));
            }
        }
        Some(_) => {}
    }
    let sidebar = hit.as_ref().is_some_and(|folder| folder.sidebar);
    model.input.hover_on_sidebar = sidebar;
    let opening = sidebar && matches!(drag.kind, nana_ui::FileDragKind::Hover);
    if opening {
        model.input.external_active = true;
        model.input.dragging_files = true;
        super::note_folder_hover(model, hit.map(|folder| folder.path));
    } else {
        super::note_folder_hover(model, None);
    }
}

/// 外部拖放停在侧栏文件夹上时，准备帧用已经拨过的悬停时钟再判一次 450ms。
/// 内部拖动由指针帧自己重判，这里不插进去。
pub(crate) fn tick_external_folder_hover(model: &mut ShellViewModel) {
    if model.input.internal_active
        || model.input.live_gesture.is_some()
        || !model.input.hover_on_sidebar
    {
        return;
    }
    if !model.input.dragging_files && !model.input.external_active {
        return;
    }
    let Some(path) = model.input.hover_folder.clone() else {
        return;
    };
    super::note_folder_hover(model, Some(path));
}
