//! 文件表面的缩略图状态：目录读回来以后等空闲再解码，解码出的原始宽高写回列表行，
//! 刷新单条缩略图，以及自定义缩略图保存以后的写回。
//!
//! 解码在调度里做（`files_dispatch.rs`、`thumbs.rs`），纹理由宿主按 `thumb:{path}` 注册；
//! 这里只排 `DecodeThumbnails` 请求，记下哪些行的纹理已经能画。

use super::super::motion::PREFETCH_IDLE_MS;
use super::super::thumbs::ThumbnailFrame;
use super::{FileRow, FilesEffect, FilesState};

impl FilesState {
    /// 虚拟列表还有下一页时，空闲 `PREFETCH_IDLE_MS` 后再解码缩略图。
    pub fn schedule_thumbnail_prefetch(&mut self, has_more: bool, now_ms: u64) {
        self.prefetch_due_ms = has_more.then_some(now_ms.saturating_add(PREFETCH_IDLE_MS));
    }

    pub fn poll_thumbnail_prefetch(&mut self, now_ms: u64, paths: &[String]) -> bool {
        if self.prefetch_due_ms.is_some_and(|due| now_ms >= due) {
            self.prefetch_due_ms = None;
            if !paths.is_empty() {
                self.effects.push(FilesEffect::DecodeThumbnails { paths: paths.to_vec() });
            }
            return true;
        }
        false
    }

    /// 还有缩略图或下一页时，等空闲间隔再解码，不在快照到达的同一拍发出请求。
    pub(super) fn arm_thumbnail_prefetch(&mut self) {
        let needs = self.has_more || !self.undecoded_thumbnail_paths().is_empty();
        self.schedule_thumbnail_prefetch(needs, self.prefetch_clock);
    }

    pub fn undecoded_thumbnail_paths(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| !row.texture_ready)
            .filter_map(|row| row.thumbnail_path.clone())
            .filter(|path| !path.trim().is_empty())
            .collect()
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

    /// 把保存结果写回这一行，并解码新的缩略图文件。没有路径时不假装已经有图。
    pub(in crate::shell) fn note_custom_thumbnail(&mut self, path: &str, thumbnail_path: Option<String>, custom: bool) {
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
            eprintln!("Nana 自定义缩略图写回时找不到条目：{path}");
            return;
        }
        if let Some(thumb) = thumbnail_path.filter(|item| !item.trim().is_empty()) {
            self.effects.push(FilesEffect::DecodeThumbnails { paths: vec![thumb] });
        }
    }

    /// 重新解码这一条缩略图。自定义缩略图保存不走这里。
    pub(in crate::shell) fn refresh_thumbnail(&mut self, path: &str) {
        if path.trim().is_empty() {
            eprintln!("Nana 刷新缩略图缺少路径");
            return;
        }
        self.effects.push(FilesEffect::DecodeThumbnails { paths: vec![path.to_string()] });
    }
}

fn apply_thumbnail(row: &mut FileRow, thumbnail_path: Option<String>, custom: bool) {
    row.thumbnail_path = thumbnail_path;
    row.thumbnail_custom = custom;
    row.texture_ready = false;
}
