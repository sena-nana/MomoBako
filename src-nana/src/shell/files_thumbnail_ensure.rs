//! 浏览时生成缩略图：列表里还没有缩略图的本地文件，逐条请后端 `ensure_thumbnail` 的 `ensure` 生成一张
//! （图片缩放、视频取第 1 秒、音频取内嵌封面），对应 Vue `thumbnails.ts` 的 `loadThumbnailsForSnapshot`。
//!
//! 算法：
//! - 每次归约结束时（`ShellViewModel::reduce` 的收尾）看一眼队列。列表换过（读目录、续读、智能文件夹、变更以后的
//!   快照、换仓库）或者上一条的结果回来了，才重新挑下一条；在途的那条没回来时什么也不做。
//! - 一次只有一条在途。后端在全局写锁里生成缩略图，几条并发也是排队执行，还会多占宿主的任务线程（Vue 开 3 个
//!   worker 也是在写锁上排队）；一条一条地排，别的写操作能插在两条之间。
//! - 先排主选中的文件，再按智能文件夹的行、目录的行的顺序（列表从上往下，先看到的先生成）。回收站不生成；
//!   虚拟条目（来源上的歌曲等）没有本地文件，后端生成不了，不排。
//! - 同一个仓库、目录和回收站里排过的文件不再排：生成失败、或者类型本来就生成不了（PDF、文本……，后端交回空）
//!   都不反复重试；换了目录或仓库就清掉重来。
//! - 结果按请求时的仓库认：换了仓库的丢掉；同一个仓库的写回所有同一路径的行（缩略图、是否自定义、元数据和
//!   色板），解码交给挂着的缩略图那一套（`files_thumbnail.rs`）。

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::{FileContext, FileRow, FilesEffect, FilesState};

/// 一次 `ensure` 交回的缩略图：缓存文件、是不是自定义的，以及顺带回填了色板的元数据。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnsuredThumbnail {
    pub thumbnail_path: Option<String>,
    pub custom: bool,
    pub metadata: Option<BTreeMap<String, Value>>,
}

/// 生成缩略图的队列状态。
#[derive(Clone, Debug, Default)]
pub(in crate::shell) struct EnsureQueue {
    /// 这一份列表（仓库、目录、回收站）里排过的文件。
    seen: HashSet<String>,
    /// 排过的文件属于哪一份列表。
    scope: Option<(String, String, bool)>,
    /// 在途的那一条。
    in_flight: Option<String>,
    /// 列表换过或者结果回来了，要重新挑下一条。
    dirty: bool,
}

impl EnsureQueue {
    /// 列表的行换了：下一次收尾重新挑。
    pub(super) fn mark_rows_changed(&mut self) {
        self.dirty = true;
    }
}

impl FilesState {
    /// 归约收尾：没有在途的、列表换过或者上一条回来了，就挑下一条还没有缩略图的文件，排一个 `EnsureThumbnail`。
    /// `repo_id` 是当前的活动仓库，列表的行都属于它。
    pub(in crate::shell) fn settle_thumbnail_ensure(&mut self, repo_id: Option<&str>) {
        let Some(repo_id) = repo_id else {
            return;
        };
        let scope = (repo_id.to_string(), self.current_path.clone(), self.trash);
        if self.ensure.scope.as_ref() != Some(&scope) {
            self.ensure.scope = Some(scope);
            self.ensure.seen.clear();
            self.ensure.dirty = true;
        }
        if self.ensure.in_flight.is_some() || !self.ensure.dirty {
            return;
        }
        self.ensure.dirty = false;
        if self.trash {
            return;
        }
        let Some(path) = self.next_ensure_candidate() else {
            return;
        };
        self.ensure.seen.insert(path.clone());
        self.ensure.in_flight = Some(path.clone());
        self.effects.push(FilesEffect::EnsureThumbnail { repo_id: repo_id.to_string(), path });
    }

    /// 生成结果回来了：清掉在途标记，同一个仓库的结果写回同一路径的文件行。失败只记日志，这一份列表里不再重试。
    pub(in crate::shell) fn note_thumbnail_ensured(
        &mut self,
        ctx: &FileContext,
        repo_id: &str,
        path: &str,
        result: Result<EnsuredThumbnail, String>,
    ) {
        if self.ensure.in_flight.as_deref() == Some(path) {
            self.ensure.in_flight = None;
        }
        self.ensure.dirty = true;
        if ctx.repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 丢掉别的仓库的缩略图生成结果：{repo_id} {path}");
            return;
        }
        let ensured = match result {
            Ok(ensured) => ensured,
            Err(error) => {
                eprintln!("Nana 生成缩略图失败：{path}：{error}");
                return;
            }
        };
        let Some(thumbnail_path) = ensured.thumbnail_path.filter(|item| !item.trim().is_empty()) else {
            return;
        };
        for row in self.rows.iter_mut().chain(self.virtual_rows.iter_mut()) {
            if row.path == path && row.kind == "file" {
                apply_ensured(row, &thumbnail_path, ensured.custom, ensured.metadata.as_ref());
            }
        }
    }

    /// 下一条要生成缩略图的文件：主选中的优先，再按智能文件夹的行、目录的行从上往下。
    fn next_ensure_candidate(&self) -> Option<String> {
        let wanted = |row: &&FileRow| {
            row.kind == "file"
                && !row.is_virtual
                && row.thumbnail_path.as_deref().is_none_or(|path| path.trim().is_empty())
                && !self.ensure.seen.contains(&row.path)
        };
        let listed = || self.virtual_rows.iter().chain(&self.rows);
        let primary = self.primary.as_deref().and_then(|primary| listed().find(|row| row.path == primary)).filter(wanted);
        primary.or_else(|| listed().find(wanted)).map(|row| row.path.clone())
    }
}

/// 把生成的缩略图写进这一行。纹理还没有，等挂上以后再解码；元数据换成回来的那份（色板跟着换）。
fn apply_ensured(row: &mut FileRow, thumbnail_path: &str, custom: bool, metadata: Option<&BTreeMap<String, Value>>) {
    if row.thumbnail_path.as_deref() != Some(thumbnail_path) {
        row.thumbnail_path = Some(thumbnail_path.to_string());
        row.texture_ready = false;
    }
    row.thumbnail_custom = custom;
    if let Some(metadata) = metadata {
        row.metadata = metadata.clone();
        row.palette = super::super::palette::from_metadata_map(metadata);
    }
}

#[cfg(test)]
#[path = "files_thumbnail_ensure_tests.rs"]
mod tests;
