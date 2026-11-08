//! 文件表面的读目录：换目录读首页、继续加载下一页、静默重读当前目录，以及目录快照和读取失败的归约。
//!
//! 对应 Vue `workspace/files.ts` 的 `loadFileBrowserForDirectory`、`loadMoreFileBrowserEntries` 和结构
//! 更新以后的静默重读。同一时间只有一次读取在途（`BrowsePending`），新的目录请求覆盖它；快照回来时
//! 仓库、路径或回收站和在途的读取对不上就丢掉，不覆盖当前列表。

use crate::backend::services::repository::FileBrowserSnapshot;

use super::{FileContext, FileRow, FilesEffect, FilesState, ReadProgress, normalize_path};

/// 首屏分页，对应 `FILE_BROWSER_INITIAL_PAGE_SIZE`。
pub(super) const INITIAL_PAGE_SIZE: usize = 80;
/// 继续加载的分页，对应 `FILE_BROWSER_APPEND_PAGE_SIZE`。下一页缩略图预取也用这一页大小。
pub(in crate::shell::files) const APPEND_PAGE_SIZE: usize = 160;

/// 导入成功且用户提交时已经有选择：这次重读的目录对上以后改选导入的条目。
///
/// 文件夹和拖放照 Vue `finishFileTransfer` 按来源文件名选。ZIP 和 Eagle 照
/// `finishWorkspaceImport` 选父目录下原先没有的直接子项。没有选择时不记。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ImportFollowup {
    /// 来源路径的最后一段文件名。
    ByName(Vec<String>),
    /// `parent` 是导入所在目录，根目录是空串。`previous` 是提交时已经在列表里的路径。
    NewChildren {
        parent: String,
        previous: Vec<String>,
    },
}

/// 在途的目录读取。快照回来时按它认出过期的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BrowsePending {
    pub(super) repo_id: String,
    pub(super) path: String,
    pub(super) trash: bool,
    pub(super) append: bool,
    /// 结构更新触发的重读。不打开加载态，失败也不把整页改成错误。
    pub(super) silent: bool,
    /// 这次读取是导入以后的重读时，快照对上才改选。别的读取是空的。
    pub(super) select_after: Option<ImportFollowup>,
}

impl FilesState {
    /// 当前在途读取请求的目录；宿主响应必须携带该路径，即使 ViewModel 尚未切换当前目录。
    pub(in crate::shell) fn pending_path(&self) -> Option<&str> {
        self.pending.as_ref().map(|pending| pending.path.as_str())
    }

    /// 替换进行中的浏览。上一页还没回来时，新的目录请求覆盖 pending，而不是被丢掉。
    /// 面包屑和双击进入目录在 Vue 里是静默读取，任务弹层不出进度行。
    pub(in crate::shell) fn request_browse(&mut self, ctx: &FileContext, path: &str) -> bool {
        self.queue_browse(ctx, path, false, false, ReadProgress::Silent)
    }

    /// 同 [`Self::request_browse`]，但按 `read` 在任务弹层出「读取目录」这一行（Vue 的非静默读取）。
    pub(in crate::shell) fn request_browse_reporting(
        &mut self,
        ctx: &FileContext,
        path: &str,
        read: ReadProgress,
    ) -> bool {
        self.queue_browse(ctx, path, false, false, read)
    }

    /// 非静默地重读当前目录（回收站面板读回收站），选择留着、读完按新列表修剪，任务弹层出「读取目录」
    /// 这一行。返回是否排下了读取：文件正在变更、虚拟视图或没有仓库时不读。刷新文件夹树同步以后用，
    /// 对应 Vue `loadFileBrowserForDirectory`。
    pub(in crate::shell) fn reload_current(&mut self, ctx: &FileContext) -> bool {
        let path = self.current_path.clone();
        self.queue_browse(ctx, &path, true, false, ReadProgress::Directory)
    }

    /// 滚离底部哨兵：下一次再靠近可以续读。
    pub(in crate::shell) fn note_scroll_left_sentinel(&mut self) {
        self.scroll_page_armed = false;
    }

    /// 靠近哨兵且这一段还没发过续读。正在读、没有下一页或虚拟视图时不发。
    pub(in crate::shell) fn arm_scroll_page(&mut self, ctx: &FileContext) -> bool {
        if self.scroll_page_armed || !self.can_load_more(ctx) {
            return false;
        }
        self.scroll_page_armed = true;
        true
    }

    pub(in crate::shell) fn can_load_more(&self, ctx: &FileContext) -> bool {
        !ctx.is_virtual()
            && ctx.trash == self.trash
            && ctx.repo_id.is_some()
            && self.has_more
            && !self.loading
            && !self.loading_more
            && !self.mutating
    }

    pub(in crate::shell) fn load_more(&mut self, ctx: &FileContext) -> bool {
        if !self.can_load_more(ctx) {
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let path = self.current_path.clone();
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash: self.trash,
            append: true,
            silent: false,
            select_after: None,
        });
        self.loading_more = true;
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash: self.trash,
            offset: self.rows.len(),
            limit: APPEND_PAGE_SIZE,
            append: true,
        });
        true
    }

    /// 应用目录快照。pending 与仓库、路径或回收站不一致时保留原列表。
    /// `pending == None` 表示启动或侧栏发起的替换。追加按 `kind:path` 去重。
    /// `has_more` 看合并后的条数是否小于 `total_entries`；虚拟视图永远不再分页。
    pub(in crate::shell) fn apply_browser(
        &mut self,
        snapshot: &FileBrowserSnapshot,
        virtual_view: bool,
    ) -> bool {
        let append = self.pending.as_ref().is_some_and(|pending| pending.append);
        let followup = if append {
            None
        } else {
            self.pending
                .as_ref()
                .and_then(|pending| pending.select_after.clone())
        };
        if let Some(pending) = &self.pending {
            let same = pending.repo_id == snapshot.repo_id
                && pending.path == normalize_path(&snapshot.current_path)
                && pending.trash == (snapshot.special_location.as_deref() == Some("trash"));
            if !same {
                if pending.select_after.is_some() {
                    eprintln!("Nana 导入后的目录快照过期，不改选：{}", snapshot.repo_id);
                }
                eprintln!("Nana 忽略过期的文件列表：{}", snapshot.repo_id);
                self.finish_loading();
                return false;
            }
        }
        let mut incoming: Vec<FileRow> = snapshot.entries.iter().map(FileRow::from_entry).collect();
        if append {
            let before = self.rows.len();
            self.rows = merge_rows(&self.rows, &incoming);
            if self.rows.len() > before {
                self.scroll_page_armed = false;
            } else {
                eprintln!(
                    "Nana 继续加载没有新条目，停在当前列表：{}",
                    self.current_path
                );
            }
        } else {
            self.carry_thumbnails(&mut incoming);
            self.rows = incoming;
            self.virtual_rows.clear();
            self.prune_decode_requests();
            self.scroll_page_armed = false;
        }
        self.current_path = normalize_path(&snapshot.current_path);
        self.trash = snapshot.special_location.as_deref() == Some("trash");
        self.shown_directory = Some((self.current_path.clone(), self.trash));
        self.total_entries = snapshot.total_entries;
        self.has_more = !virtual_view && self.rows.len() < snapshot.total_entries;
        self.error.clear();
        self.finish_loading();
        if !append {
            self.settle_operation(true);
        }
        let rows = self.rows.clone();
        self.prune_against(&rows);
        if let Some(followup) = followup {
            self.select_imported(&followup);
        }
        self.arm_thumbnail_prefetch();
        self.arm_virtual_page_prefetch(&snapshot.repo_id);
        true
    }

    /// 静默重读当前目录。不改变加载文案和选择；虚拟视图直接返回。
    pub(in crate::shell) fn reload_silent(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，跳过静默刷新");
            return false;
        }
        if ctx.smart_folder || ctx.category_virtual {
            eprintln!("Nana 虚拟视图不按目录静默刷新");
            return false;
        }
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 静默刷新没有活动仓库");
            return false;
        };
        let path = self.current_path.clone();
        let trash = ctx.trash;
        let limit = INITIAL_PAGE_SIZE.max(self.rows.len());
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash,
            append: false,
            silent: true,
            select_after: None,
        });
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash,
            offset: 0,
            limit,
            append: false,
        });
        true
    }

    /// 只有文件表面自己发起的加载失败才吃掉错误。启动失败仍交给壳层切到错误页。
    /// 静默刷新失败只记日志，保留当前列表和页面。
    pub(in crate::shell) fn note_load_failed(&mut self, error: &str) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.select_after.is_some())
        {
            eprintln!("Nana 导入后的目录读取失败，不改选：{error}");
        }
        if self.pending.as_ref().is_some_and(|pending| pending.silent) {
            eprintln!("Nana 静默刷新目录失败：{error}");
            self.pending = None;
            return true;
        }
        if !self.loading && !self.loading_more {
            return false;
        }
        eprintln!("Nana 读取文件列表失败：{error}");
        self.error = error.to_string();
        self.finish_loading();
        self.settle_operation(false);
        true
    }

    pub(in crate::shell) fn set_virtual_rows(&mut self, mut rows: Vec<FileRow>) {
        self.carry_thumbnails(&mut rows);
        self.virtual_rows = rows;
        self.prune_decode_requests();
        let rows = self.virtual_rows.clone();
        self.prune_against(&rows);
    }

    /// 读一个目录的首页，替换进行中的浏览并清掉错误。`keep_selection` 时选择留着、读完按新列表修剪；
    /// `allow_category` 时分类视图也能读（变更以后重读）；`read` 决定任务弹层出不出「读取目录」这一行。
    pub(super) fn queue_browse(
        &mut self,
        ctx: &FileContext,
        path: &str,
        keep_selection: bool,
        allow_category: bool,
        read: ReadProgress,
    ) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能切换目录");
            return false;
        }
        if ctx.smart_folder || (ctx.category_virtual && !allow_category) {
            eprintln!("Nana 虚拟视图不能按目录分页");
            return false;
        }
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 目录浏览没有活动仓库");
            return false;
        };
        let path = normalize_path(path);
        let trash = ctx.trash;
        // Vue `keepsCurrentViewVisible`：静默重读已经显示的这一层时，列表和选择留着，
        // 不换成「正在读取目录」。面包屑点当前这一级、侧栏再点正在看的文件夹都是这种。
        let keep_visible =
            matches!(read, ReadProgress::Silent) && self.shows_directory(&path, trash);
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.select_after.is_some())
        {
            eprintln!("Nana 导入后的目录还没读回来就被下一次读取换掉，不改选");
        }
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash,
            append: false,
            silent: keep_visible,
            select_after: None,
        });
        self.loading = !keep_visible;
        self.loading_more = false;
        self.virtual_rows.clear();
        if !keep_selection && !keep_visible {
            self.selected.clear();
            self.primary = None;
            self.anchor = None;
        }
        if !keep_visible {
            self.error.clear();
        }
        self.report_read(&path, trash, read);
        let limit = if keep_visible {
            INITIAL_PAGE_SIZE.max(self.rows.len())
        } else {
            INITIAL_PAGE_SIZE
        };
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash,
            offset: 0,
            limit,
            append: false,
        });
        true
    }

    /// 侧栏再点已经显示的这一层：只记下静默重读，请求由侧栏自己发出。
    /// 列表不换成「正在读取目录」，选择留着；失败走 `pending.silent`，只记日志。
    pub(in crate::shell) fn arm_shown_reread(&mut self, repo_id: String, path: &str, trash: bool) {
        let path = normalize_path(path);
        if !self.shows_directory(&path, trash) {
            eprintln!("Nana 要静默重读的不是正在显示的目录：{path}");
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.select_after.is_some())
        {
            eprintln!("Nana 导入后的目录还没读回来就被当前目录的重读换掉，不改选");
        }
        self.pending = Some(BrowsePending {
            repo_id,
            path,
            trash,
            append: false,
            silent: true,
            select_after: None,
        });
        self.loading = false;
        self.loading_more = false;
    }

    /// 移动成功以后按 Vue `moveWorkspaceEntries` 改选。
    ///
    /// 目标就是正在看的目录时，记下源文件名，等这次重读的快照对上再选同名条目。
    /// 目标是别的目录时，选择立刻换成当前列表里第一个没被移走的条目：先按列表顺序找文件夹，
    /// 再找文件，和 Vue `visibleEntries` 一样；一个不剩就清空。单击只选中的那一条被移走时，
    /// 只选中跟着新的文件走，新的是文件夹就清掉，不因此打开预览。
    pub(super) fn prepare_move_selection(&mut self, sources: &[String], parent: &str) {
        let parent = normalize_path(parent);
        if parent == normalize_path(&self.current_path) {
            let names = sources
                .iter()
                .map(|path| entry_leaf(path))
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if names.is_empty() {
                eprintln!("Nana 移动来源没有文件名，不改选");
                return;
            }
            self.import_followup = Some(ImportFollowup::ByName(names));
            return;
        }
        let moved = sources
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let next = self
            .rows
            .iter()
            .find(|row| row.kind == "directory" && !moved.contains(row.path.as_str()))
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|row| row.kind != "directory" && !moved.contains(row.path.as_str()))
            })
            .map(|row| row.path.clone());
        let previous_primary = self.primary.clone();
        match &next {
            Some(path) => {
                self.selected = vec![path.clone()];
                self.primary = Some(path.clone());
                self.anchor = Some(path.clone());
            }
            None => {
                self.selected.clear();
                self.primary = None;
                self.anchor = None;
            }
        }
        if self
            .select_only
            .as_ref()
            .is_some_and(|path| moved.contains(path.as_str()))
        {
            let stayed = next.filter(|path| {
                self.rows
                    .iter()
                    .any(|row| row.path == *path && row.kind != "directory")
            });
            self.select_only = stayed;
        }
        self.clear_rename_if_needed(previous_primary);
    }

    /// 复制成功以后按 Vue `finishFileTransfer` 记下源文件名，等目标目录的快照对上再选同名条目。
    ///
    /// 来源没有文件名时不记。读回来只修剪已经不在列表里的选择，不挑一条凑数。
    pub(super) fn prepare_copy_selection(&mut self, sources: &[String]) {
        let names = sources
            .iter()
            .map(|path| entry_leaf(path))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if names.is_empty() {
            eprintln!("Nana 复制来源没有文件名，不改选");
            return;
        }
        self.import_followup = Some(ImportFollowup::ByName(names));
    }

    /// 文件夹导入或拖放导入：提交时已经有选择才记下源文件名。没有选择时清掉上一份改选。
    pub(super) fn arm_named_import(&mut self, sources: &[String]) {
        if !self.explicit_selection() {
            self.import_followup = None;
            return;
        }
        let names = sources
            .iter()
            .map(|path| entry_leaf(path))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if names.is_empty() {
            eprintln!("Nana 导入来源没有文件名，不改选");
            self.import_followup = None;
            return;
        }
        self.import_followup = Some(ImportFollowup::ByName(names));
    }

    /// ZIP 或 Eagle 导入：提交时已经有选择才记下当前目录和当时的路径。
    pub(super) fn arm_new_children_import(&mut self, parent: &Option<String>) {
        if !self.explicit_selection() {
            self.import_followup = None;
            return;
        }
        self.import_followup = Some(ImportFollowup::NewChildren {
            parent: parent.clone().unwrap_or_default(),
            previous: self.rows.iter().map(|row| row.path.clone()).collect(),
        });
    }

    fn explicit_selection(&self) -> bool {
        self.primary.is_some() || !self.selected.is_empty()
    }

    /// 按导入时记下的规则改选。对不上任何条目时保留修剪后的选择，并记一条日志。
    fn select_imported(&mut self, followup: &ImportFollowup) {
        let paths = match followup {
            ImportFollowup::ByName(names) => {
                let names: std::collections::HashSet<&str> =
                    names.iter().map(String::as_str).collect();
                self.rows
                    .iter()
                    .filter(|row| names.contains(row.name.as_str()))
                    .map(|row| row.path.clone())
                    .collect::<Vec<_>>()
            }
            ImportFollowup::NewChildren { parent, previous } => {
                let previous: std::collections::HashSet<&str> =
                    previous.iter().map(String::as_str).collect();
                self.rows
                    .iter()
                    .filter(|row| {
                        entry_parent(&row.path) == *parent && !previous.contains(row.path.as_str())
                    })
                    .map(|row| row.path.clone())
                    .collect::<Vec<_>>()
            }
        };
        if paths.is_empty() {
            eprintln!("Nana 导入完成但没有可改选的条目，保留当前选择");
            return;
        }
        let previous_primary = self.primary.clone();
        let primary = paths[0].clone();
        self.selected = paths;
        self.primary = Some(primary.clone());
        self.anchor = Some(primary);
        self.clear_rename_if_needed(previous_primary);
    }

    pub(super) fn finish_loading(&mut self) {
        self.loading = false;
        self.loading_more = false;
        self.pending = None;
    }
}

/// 来源路径的最后一段，照 Vue `entryNameFromPath`。反斜杠先收成斜杠，丢掉结尾的斜杠。
fn entry_leaf(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// 仓库相对路径的父目录。没有斜杠时是根目录，写成空串。照 Vue `parent_relative_path`。
fn entry_parent(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rfind('/') {
        Some(index) => normalized[..index].to_string(),
        None => String::new(),
    }
}

/// 已有键保留原顺序，新键追加到末尾。
fn merge_rows(existing: &[FileRow], incoming: &[FileRow]) -> Vec<FileRow> {
    let mut seen: std::collections::HashSet<String> = existing.iter().map(FileRow::key).collect();
    let mut merged = existing.to_vec();
    for row in incoming {
        if seen.insert(row.key()) {
            merged.push(row.clone());
        }
    }
    merged
}
