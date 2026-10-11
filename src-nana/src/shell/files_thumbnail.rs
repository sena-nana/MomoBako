//! 文件表面的缩略图状态：挂着的缩略图等空闲再解码，解码出的原始宽高写回列表行，宿主卸掉的纹理
//! 写回成没有纹理，请后端刷新单条缩略图，以及自定义缩略图保存、清除和刷新以后的写回。
//!
//! 解码在调度里做（`files_dispatch.rs`、`thumbs.rs`），纹理由宿主按 `thumb:{path}` 登记和回收
//! （`thumbnail_host.rs`、`thumbnail_cache.rs`）；这里只排 `DecodeThumbnails` 请求，记下哪些行的纹理
//! 已经能画。宿主报告了挂着的缩略图以后，只解码挂着的：滚出视口的行纹理可能被回收，再滚回来时按挂着的
//! 重新解码，不在视口外整目录解码。

use std::collections::{HashMap, HashSet};

use super::super::motion::PREFETCH_IDLE_MS;
use super::super::thumbs::ThumbnailFrame;
use super::{FileContext, FileRow, FilesEffect, FilesState};

impl FilesState {
    /// 虚拟列表还有下一页时，空闲 `PREFETCH_IDLE_MS` 后再解码缩略图。
    pub fn schedule_thumbnail_prefetch(&mut self, has_more: bool, now_ms: u64) {
        self.prefetch_due_ms = has_more.then_some(now_ms.saturating_add(PREFETCH_IDLE_MS));
    }

    pub fn poll_thumbnail_prefetch(&mut self, now_ms: u64, paths: &[String]) -> bool {
        if self.prefetch_due_ms.is_some_and(|due| now_ms >= due) {
            self.prefetch_due_ms = None;
            if !paths.is_empty() {
                self.request_decode(paths.to_vec());
            }
            return true;
        }
        false
    }

    /// 读目录（含续读）的快照到了：还有缩略图或下一页时，等空闲间隔再解码，不在快照到达的同一拍发出请求；
    /// 新来的行里可能有还没生成缩略图的，生成队列下一次收尾重新挑（`files_thumbnail_ensure.rs`）。
    pub(super) fn arm_thumbnail_prefetch(&mut self) {
        let needs = self.has_more || !self.undecoded_thumbnail_paths().is_empty();
        self.schedule_thumbnail_prefetch(needs, self.prefetch_clock);
        self.ensure.mark_rows_changed();
    }

    /// 还要解码的缩略图文件：列表行和智能文件夹行里没有纹理、没请求过的，宿主报告过挂着的缩略图时
    /// 只取挂着的。同一个文件只出现一次。
    pub fn undecoded_thumbnail_paths(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.rows
            .iter()
            .chain(&self.virtual_rows)
            .filter(|row| !row.texture_ready)
            .filter_map(thumbnail_file)
            .filter(|path| !self.decode_requested.contains(*path))
            .filter(|path| self.mounted_thumbnails.as_ref().is_none_or(|mounted| mounted.contains(*path)))
            .filter(|path| seen.insert(*path))
            .map(str::to_string)
            .collect()
    }

    /// 宿主报告文档里挂着的缩略图文件。和上次不同时记下；挂着的里面有还没解码的，就重新等一个空闲间隔
    /// 再解码（滚动中一直推迟，停下 `PREFETCH_IDLE_MS` 以后才解码）。
    pub(crate) fn note_mounted_thumbnails(&mut self, mounted: HashSet<String>) {
        if self.mounted_thumbnails.as_ref() == Some(&mounted) {
            return;
        }
        self.mounted_thumbnails = Some(mounted);
        self.mounted_version = self.mounted_version.wrapping_add(1);
        if !self.undecoded_thumbnail_paths().is_empty() {
            self.schedule_thumbnail_prefetch(true, self.prefetch_clock);
        }
    }

    /// 挂着的缩略图文件。宿主还没报告过时为 `None`。
    pub(crate) fn mounted_thumbnails(&self) -> Option<&HashSet<String>> {
        self.mounted_thumbnails.as_ref()
    }

    /// 挂着的缩略图变过几次。
    pub(crate) fn mounted_version(&self) -> u64 {
        self.mounted_version
    }

    /// 当前列表行和智能文件夹行引用的缩略图文件。不在这里面、也不挂着的纹理可以立刻卸掉。
    pub(crate) fn listed_thumbnails(&self) -> HashSet<String> {
        self.rows.iter().chain(&self.virtual_rows).filter_map(thumbnail_file).map(str::to_string).collect()
    }

    /// 宿主卸掉了这些缩略图的纹理：对应的行回到类型图标，再挂出来时重新解码。返回有没有行变了。
    pub(crate) fn note_thumbnails_released(&mut self, paths: &[String]) -> bool {
        let released: HashSet<&str> = paths.iter().map(String::as_str).collect();
        let mut changed = false;
        for row in self.rows.iter_mut().chain(self.virtual_rows.iter_mut()) {
            if row.texture_ready && thumbnail_file(row).is_some_and(|path| released.contains(path)) {
                row.texture_ready = false;
                changed = true;
            }
        }
        changed
    }

    /// 宿主没能把这些缩略图上传成纹理：行回到类型图标，记成已经试过，不在每次空闲时重试。
    pub(crate) fn note_thumbnails_failed(&mut self, paths: &[String]) -> bool {
        self.decode_requested.extend(paths.iter().cloned());
        self.note_thumbnails_released(paths)
    }

    pub fn prefetch_pending(&self) -> bool {
        self.prefetch_due_ms.is_some()
    }

    /// 实况帧把空闲时钟向前拨。没有待预取时停在原地。
    pub fn tick_prefetch(&mut self, step_ms: u64) -> u64 {
        if self.prefetch_due_ms.is_some() {
            self.prefetch_clock = self.prefetch_clock.saturating_add(step_ms);
        }
        self.prefetch_clock
    }

    /// 用解码出的原始宽高替换布局。同一路径的行一起更新。
    pub(in crate::shell) fn note_thumbnail_sizes(&mut self, frames: &[ThumbnailFrame]) {
        for frame in frames {
            self.decode_requested.remove(&frame.path);
            if frame.natural_width == 0 || frame.natural_height == 0 {
                continue;
            }
            for row in self.rows.iter_mut().chain(self.virtual_rows.iter_mut()) {
                if row.thumbnail_path.as_deref() == Some(frame.path.as_str()) {
                    row.pixel_width = frame.natural_width;
                    row.pixel_height = frame.natural_height;
                    row.texture_ready = true;
                }
            }
        }
    }

    /// 自定义缩略图不锁整页变更。失败由调度器记错误。
    pub(crate) fn queue_thumbnail(&mut self, effect: FilesEffect) {
        self.effects.push(effect);
    }

    /// 把保存、清除或刷新的结果写回这一行，并重新解码缩略图文件：刷新以后缓存路径不变、内容换了，纹理也要换。
    /// 没有路径时不假装已经有图。
    pub(in crate::shell) fn note_thumbnail_saved(&mut self, path: &str, thumbnail_path: Option<String>, custom: bool) {
        let mut found = false;
        for row in &mut self.rows {
            if row.path == path {
                apply_thumbnail(row, thumbnail_path.clone(), custom);
                found = true;
            }
        }
        for row in &mut self.virtual_rows {
            if row.path == path {
                apply_thumbnail(row, thumbnail_path.clone(), custom);
                found = true;
            }
        }
        if !found {
            eprintln!("Nana 缩略图写回时找不到条目：{path}");
            return;
        }
        if let Some(thumb) = thumbnail_path.filter(|item| !item.trim().is_empty()) {
            self.request_decode(vec![thumb.trim().to_string()]);
        }
    }

    /// 「刷新缩略图」：请后端按源文件重新生成（`ensure_thumbnail` 的 `refresh`，对应 Vue
    /// `refreshWorkspaceEntryThumbnail`），结果和自定义缩略图一样经 `ThumbnailSaved` 写回。
    pub(in crate::shell) fn refresh_thumbnail(&mut self, ctx: &FileContext, path: &str) {
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 刷新缩略图没有活动仓库：{path}");
            self.note_error("刷新缩略图缺少仓库。".into());
            return;
        };
        let Some(kind) = self.rows.iter().chain(&self.virtual_rows).find(|row| row.path == path).map(|row| row.kind.clone()) else {
            eprintln!("Nana 刷新缩略图的条目已经不在列表里：{path}");
            return;
        };
        self.effects.push(FilesEffect::MutateThumbnail {
            repo_id,
            path: path.to_string(),
            kind,
            action: "refresh".into(),
            source_path: None,
            image_bytes: None,
        });
    }

    /// 换上新列表时，同一个缩略图文件已经有纹理的行接着用它：重读目录不让卡片闪回类型图标，也不重新解码
    /// （Vue 的 `<img>` 地址不变时同样不重新加载）。新条目的元数据没给宽高时沿用解码出的原始宽高。
    pub(super) fn carry_thumbnails(&self, incoming: &mut [FileRow]) {
        let ready: HashMap<&str, (u32, u32)> = self
            .rows
            .iter()
            .chain(&self.virtual_rows)
            .filter(|row| row.texture_ready)
            .filter_map(|row| thumbnail_file(row).map(|path| (path, (row.pixel_width, row.pixel_height))))
            .collect();
        for row in incoming.iter_mut() {
            let Some(&(width, height)) = thumbnail_file(row).and_then(|path| ready.get(path)) else {
                continue;
            };
            row.texture_ready = true;
            if row.pixel_width == 0 || row.pixel_height == 0 {
                row.pixel_width = width;
                row.pixel_height = height;
            }
        }
    }

    /// 列表换了以后，解码请求只留还在列表里的：换回来的目录里解码失败过的缩略图可以再试。换上的行里可能有
    /// 还没生成缩略图的，生成队列下一次收尾重新挑。
    pub(super) fn prune_decode_requests(&mut self) {
        let listed = self.listed_thumbnails();
        self.decode_requested.retain(|path| listed.contains(path));
        self.ensure.mark_rows_changed();
    }

    fn request_decode(&mut self, paths: Vec<String>) {
        self.decode_requested.extend(paths.iter().cloned());
        self.effects.push(FilesEffect::DecodeThumbnails { paths });
    }
}

/// 行引用的缩略图文件（去掉首尾空白），没有时为 `None`。
fn thumbnail_file(row: &FileRow) -> Option<&str> {
    row.thumbnail_path.as_deref().map(str::trim).filter(|path| !path.is_empty())
}

fn apply_thumbnail(row: &mut FileRow, thumbnail_path: Option<String>, custom: bool) {
    row.thumbnail_path = thumbnail_path;
    row.thumbnail_custom = custom;
    row.texture_ready = false;
}
