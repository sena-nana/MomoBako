//! 浏览时生成缩略图：列表里还没有缩略图的本地文件，逐条请后端 `ensure_thumbnail` 的 `ensure` 生成一张
//! （图片缩放、视频取第 1 秒、音频取内嵌封面），对应 Vue `thumbnails.ts` 的 `loadThumbnailsForSnapshot`。
//! 来源上生成不了的条目（网易云歌曲、歌单目录）若载荷或元数据里有真实的 http(s) 封面地址，改用
//! `save + sourceUrl` 下载并存成自定义缩略图。没有地址就不预取，也不编造封面像素。
//!
//! 算法：
//! - 每次归约结束时（`ShellViewModel::reduce` 的收尾）看一眼队列。列表换过（读目录、续读、智能文件夹、变更以后的
//!   快照、换仓库）或者上一条的结果回来了，才重新挑下一条；在途的那条没回来时什么也不做。
//! - 一次只有一条在途。后端在全局写锁里生成缩略图，几条并发也是排队执行，还会多占宿主的任务线程（Vue 开 3 个
//!   worker 也是在写锁上排队）；一条一条地排，别的写操作能插在两条之间。
//! - 先排主选中的条目，再按智能文件夹的行、目录的行、分类视图的行的顺序（列表从上往下，先看到的先处理）。回收站不处理。
//!   本地文件走 `ensure`。虚拟条目和目录没有本地文件，后端生成不了：有封面地址才走 `save`，没有就跳过。
//! - 本地 `ensure` 交回空、而这一行仍有封面地址时，紧接着排一次 `save`（生成不了才用封面）。`save` 再空或失败就停。
//! - 同一个仓库、目录和回收站里排过的文件不再排：生成失败、或者类型本来就生成不了（PDF、文本……，后端交回空）
//!   都不反复重试；换了目录或仓库就清掉重来。
//! - 结果按请求时的仓库认：换了仓库的丢掉；同一个仓库的写回同一路径的文件行（封面也写歌单目录）的缩略图、
//!   是否自定义、元数据和色板。像素等缓存文件挂上以后再解码（`files_thumbnail.rs`），这里不画假图。

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

/// 在途的一条。`source_url` 有值时是来源封面的 `save`，没有时是本地文件的 `ensure`。
#[derive(Clone, Debug, PartialEq, Eq)]
struct InFlight {
    path: String,
    source_url: Option<String>,
}

/// 下一条要排的请求。
struct EnsureJob {
    path: String,
    source_url: Option<String>,
}

/// 生成缩略图的队列状态。
#[derive(Clone, Debug, Default)]
pub(in crate::shell::files) struct EnsureQueue {
    /// 这一份列表（仓库、目录、回收站）里排过的文件。
    seen: HashSet<String>,
    /// 排过的文件属于哪一份列表。
    scope: Option<(String, String, bool)>,
    /// 在途的那一条。
    in_flight: Option<InFlight>,
    /// 列表换过或者结果回来了，要重新挑下一条。
    dirty: bool,
    /// 下一页预取读回来、还没画进列表的条目。当前列表排完才排它们。
    prefetch_rows: Vec<FileRow>,
}

impl EnsureQueue {
    /// 列表的行换了：下一次收尾重新挑。
    pub(super) fn mark_rows_changed(&mut self) {
        self.dirty = true;
    }

    /// 预取的下一页：记下来，下一次收尾再排。已经记过的路径不重复记。
    pub(in crate::shell::files) fn offer_unlisted(&mut self, rows: Vec<FileRow>) {
        let mut added = false;
        for row in rows {
            let known = self
                .prefetch_rows
                .iter()
                .any(|item| item.path == row.path && item.kind == row.kind);
            if !known {
                self.prefetch_rows.push(row);
                added = true;
            }
        }
        if added {
            self.dirty = true;
        }
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
            self.ensure.prefetch_rows.clear();
            self.ensure.dirty = true;
        }
        if self.ensure.in_flight.is_some() || !self.ensure.dirty {
            return;
        }
        self.ensure.dirty = false;
        if self.trash {
            return;
        }
        let Some(job) = self.next_ensure_candidate() else {
            return;
        };
        self.queue_ensure(repo_id, job);
    }

    /// 生成结果回来了：清掉在途标记，同一个仓库的结果写回同一路径的行。失败只记日志，这一份列表里不再重试。
    /// 本地 `ensure` 没有写出缩略图、但这一行有真实封面地址时，紧接着改排 `save`。
    pub(in crate::shell) fn note_thumbnail_ensured(
        &mut self,
        ctx: &FileContext,
        repo_id: &str,
        path: &str,
        result: Result<EnsuredThumbnail, String>,
    ) {
        let requested_cover = self
            .ensure
            .in_flight
            .as_ref()
            .is_some_and(|item| item.path == path && item.source_url.is_some());
        if self
            .ensure
            .in_flight
            .as_ref()
            .is_some_and(|item| item.path == path)
        {
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
                if requested_cover {
                    eprintln!("Nana 预取来源封面失败：{path}：{error}");
                } else {
                    eprintln!("Nana 生成缩略图失败：{path}：{error}");
                }
                return;
            }
        };
        let Some(thumbnail_path) = ensured
            .thumbnail_path
            .filter(|item| !item.trim().is_empty())
        else {
            if !requested_cover {
                if let Some(url) = self.cover_url_for(path) {
                    self.queue_ensure(
                        repo_id,
                        EnsureJob {
                            path: path.to_string(),
                            source_url: Some(url),
                        },
                    );
                }
            } else {
                eprintln!("Nana 来源封面没有写入缩略图：{path}");
            }
            return;
        };
        for row in self
            .rows
            .iter_mut()
            .chain(self.virtual_rows.iter_mut())
            .chain(self.category_rows.iter_mut())
        {
            // 本地生成只写文件。来源封面同时写歌单目录。
            if row.path == path && (row.kind == "file" || requested_cover) {
                apply_ensured(
                    row,
                    &thumbnail_path,
                    ensured.custom,
                    ensured.metadata.as_ref(),
                );
            }
        }
    }

    /// 下一条：主选中的优先，再按智能文件夹的行、目录的行、分类视图的行从上往下。
    /// 未分类、未标签和最近使用画的是分类视图，只看目录行会漏掉不在当前目录里的文件。
    fn next_ensure_candidate(&self) -> Option<EnsureJob> {
        let wanted = |row: &FileRow| {
            missing_thumbnail(row)
                && !self.ensure.seen.contains(&row.path)
                && ensure_job(row).is_some()
        };
        let listed = || {
            self.virtual_rows
                .iter()
                .chain(&self.rows)
                .chain(&self.category_rows)
        };
        let primary = self
            .primary
            .as_deref()
            .and_then(|primary| listed().find(|row| row.path == primary))
            .filter(|row| wanted(row));
        if let Some(job) = primary
            .or_else(|| listed().find(|row| wanted(row)))
            .and_then(ensure_job)
        {
            return Some(job);
        }
        // 当前列表没有可排的，才排还没画出来的下一页。
        self.ensure
            .prefetch_rows
            .iter()
            .find(|row| wanted(row))
            .and_then(ensure_job)
    }

    /// 分类视图的行换过：下一次收尾重新挑还没有缩略图的文件。
    pub(in crate::shell) fn note_listed_rows_changed(&mut self) {
        self.ensure.mark_rows_changed();
    }

    /// 这一行的封面地址。列表里同一路径有多行时用先找到的那条。
    fn cover_url_for(&self, path: &str) -> Option<String> {
        self.rows
            .iter()
            .chain(&self.virtual_rows)
            .chain(&self.category_rows)
            .chain(&self.ensure.prefetch_rows)
            .find(|row| row.path == path)
            .and_then(cover_source_url)
    }

    /// 排一条 `ensure` 或封面 `save`，并记成在途。路径已经排过，失败也不再重试。
    fn queue_ensure(&mut self, repo_id: &str, job: EnsureJob) {
        self.ensure.seen.insert(job.path.clone());
        self.ensure.in_flight = Some(InFlight {
            path: job.path.clone(),
            source_url: job.source_url.clone(),
        });
        self.effects.push(FilesEffect::EnsureThumbnail {
            repo_id: repo_id.to_string(),
            path: job.path,
            source_url: job.source_url,
        });
    }
}

/// 还没有缩略图文件的行。
fn missing_thumbnail(row: &FileRow) -> bool {
    row.thumbnail_path
        .as_deref()
        .is_none_or(|path| path.trim().is_empty())
}

/// 这一行该怎么要缩略图。本地文件用 `ensure`（不带地址）。其余条目只在有真实封面地址时用 `save`。
fn ensure_job(row: &FileRow) -> Option<EnsureJob> {
    if row.kind == "file" && !row.is_virtual {
        return Some(EnsureJob {
            path: row.path.clone(),
            source_url: None,
        });
    }
    let source_url = cover_source_url(row)?;
    Some(EnsureJob {
        path: row.path.clone(),
        source_url: Some(source_url),
    })
}

/// 来源封面的真实地址。歌曲是 `sourcePayload.coverUrl`（同步后也可能镜像在元数据 `coverUrl`），
/// 歌单目录是 `sourcePayload.playlistCoverUrl`。只接受 http/https，和后端下载限制一致。
fn cover_source_url(row: &FileRow) -> Option<String> {
    let payload = row.source_payload.as_ref();
    http_url(payload.and_then(|value| value.get("coverUrl")))
        .or_else(|| http_url(payload.and_then(|value| value.get("playlistCoverUrl"))))
        .or_else(|| http_url(row.metadata.get("coverUrl")))
}

/// `http://` 或 `https://` 开头的非空地址。其它写法（空、null、ftp、相对路径）不当作封面。
fn http_url(value: Option<&Value>) -> Option<String> {
    let url = value?.as_str()?.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url.to_string())
    } else {
        None
    }
}

impl FilesState {
    /// 「保存封面」的结果写回这一条。不碰浏览队列的在途标记。没有路径时不编造缩略图。
    pub(in crate::shell) fn note_cover_saved(&mut self, path: &str, ensured: EnsuredThumbnail) {
        let Some(thumbnail_path) = ensured
            .thumbnail_path
            .filter(|item| !item.trim().is_empty())
        else {
            eprintln!("Nana 保存封面没有写入缩略图：{path}");
            return;
        };
        let mut found = false;
        for row in self
            .rows
            .iter_mut()
            .chain(self.virtual_rows.iter_mut())
            .chain(self.category_rows.iter_mut())
        {
            if row.path == path {
                apply_ensured(
                    row,
                    &thumbnail_path,
                    ensured.custom,
                    ensured.metadata.as_ref(),
                );
                found = true;
            }
        }
        if !found {
            eprintln!("Nana 保存封面时找不到条目：{path}");
            return;
        }
        self.request_decode(vec![thumbnail_path]);
    }
}

/// 把生成的缩略图写进这一行。纹理还没有，等挂上以后再解码；元数据换成回来的那份（色板跟着换）。
fn apply_ensured(
    row: &mut FileRow,
    thumbnail_path: &str,
    custom: bool,
    metadata: Option<&BTreeMap<String, Value>>,
) {
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
