//! 文件表面的读目录：换目录读首页、继续加载下一页、静默重读当前目录，以及目录快照和读取失败的归约。
//!
//! 对应 Vue `workspace/files.ts` 的 `loadFileBrowserForDirectory`、`loadMoreFileBrowserEntries` 和结构
//! 更新以后的静默重读。同一时间只有一次读取在途（`BrowsePending`），新的目录请求覆盖它；快照回来时
//! 仓库、路径或回收站和在途的读取对不上就丢掉，不覆盖当前列表。

use crate::backend::services::repository::FileBrowserSnapshot;

use super::{normalize_path, FileContext, FileRow, FilesEffect, FilesState, ReadProgress};

/// 首屏分页，对应 `FILE_BROWSER_INITIAL_PAGE_SIZE`。
pub(super) const INITIAL_PAGE_SIZE: usize = 80;
/// 继续加载的分页，对应 `FILE_BROWSER_APPEND_PAGE_SIZE`。
pub(super) const APPEND_PAGE_SIZE: usize = 160;

/// 在途的目录读取。快照回来时按它认出过期的结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BrowsePending {
    pub(super) repo_id: String,
    pub(super) path: String,
    pub(super) trash: bool,
    pub(super) append: bool,
    /// 结构更新触发的重读。不打开加载态，失败也不把整页改成错误。
    pub(super) silent: bool,
}

impl FilesState {
    /// 替换进行中的浏览。上一页还没回来时，新的目录请求覆盖 pending，而不是被丢掉。
    /// 面包屑和双击进入目录在 Vue 里是静默读取，任务弹层不出进度行。
    pub(in crate::shell) fn request_browse(&mut self, ctx: &FileContext, path: &str) -> bool {
        self.queue_browse(ctx, path, false, false, ReadProgress::Silent)
    }

    /// 同 [`Self::request_browse`]，但按 `read` 在任务弹层出「读取目录」这一行（Vue 的非静默读取）。
    pub(in crate::shell) fn request_browse_reporting(&mut self, ctx: &FileContext, path: &str, read: ReadProgress) -> bool {
        self.queue_browse(ctx, path, false, false, read)
    }

    /// 非静默地重读当前目录（回收站面板读回收站），选择留着、读完按新列表修剪，任务弹层出「读取目录」
    /// 这一行。返回是否排下了读取：文件正在变更、虚拟视图或没有仓库时不读。刷新文件夹树同步以后用，
    /// 对应 Vue `loadFileBrowserForDirectory`。
    pub(in crate::shell) fn reload_current(&mut self, ctx: &FileContext) -> bool {
        let path = self.current_path.clone();
        self.queue_browse(ctx, &path, true, false, ReadProgress::Directory)
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
    pub(in crate::shell) fn apply_browser(&mut self, snapshot: &FileBrowserSnapshot, virtual_view: bool) -> bool {
        let append = self.pending.as_ref().is_some_and(|pending| pending.append);
        if let Some(pending) = &self.pending {
            let same = pending.repo_id == snapshot.repo_id
                && pending.path == normalize_path(&snapshot.current_path)
                && pending.trash == (snapshot.special_location.as_deref() == Some("trash"));
            if !same {
                eprintln!("Nana 忽略过期的文件列表：{}", snapshot.repo_id);
                self.finish_loading();
                return false;
            }
        }
        let mut incoming: Vec<FileRow> = snapshot.entries.iter().map(FileRow::from_entry).collect();
        if append {
            self.rows = merge_rows(&self.rows, &incoming);
        } else {
            self.carry_thumbnails(&mut incoming);
            self.rows = incoming;
            self.virtual_rows.clear();
            self.prune_decode_requests();
        }
        self.current_path = normalize_path(&snapshot.current_path);
        self.trash = snapshot.special_location.as_deref() == Some("trash");
        self.total_entries = snapshot.total_entries;
        self.has_more = !virtual_view && self.rows.len() < snapshot.total_entries;
        self.error.clear();
        self.finish_loading();
        if !append {
            self.settle_operation(true);
        }
        let rows = self.rows.clone();
        self.prune_against(&rows);
        self.arm_thumbnail_prefetch();
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
    pub(super) fn queue_browse(&mut self, ctx: &FileContext, path: &str, keep_selection: bool, allow_category: bool, read: ReadProgress) -> bool {
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
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash,
            append: false,
            silent: false,
        });
        self.loading = true;
        self.loading_more = false;
        self.virtual_rows.clear();
        if !keep_selection {
            self.selected.clear();
            self.primary = None;
            self.anchor = None;
        }
        self.error.clear();
        self.report_read(&path, trash, read);
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash,
            offset: 0,
            limit: INITIAL_PAGE_SIZE,
            append: false,
        });
        true
    }

    pub(super) fn finish_loading(&mut self) {
        self.loading = false;
        self.loading_more = false;
        self.pending = None;
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
