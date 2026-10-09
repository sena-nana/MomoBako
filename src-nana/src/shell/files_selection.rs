//! 文件表面的选择归约：替换、切换、范围选择，按新列表修剪选择，以及单击选中、
//! 标签组展开和列表宽度这类只影响画面的状态。
//!
//! 对应 Vue `useFileBrowserPanelViewModel.ts` 的选择规则；预览打开与否对应 `previewFileEntry`。

use std::collections::HashSet;

use super::{FileContext, FileDialog, FileRow, FilesState, SelectionMode};

impl FilesState {
    /// 单击选中的是文件时记下它：素材详情回来后仍留在列表，不切到预览页。
    ///
    /// 单击文件夹两项都不动：右侧详情停在哪个文件、还在读哪个文件都照旧，列表不会因此换成那个文件的
    /// 预览页。Vue 单击只改选择，预览只由双击、「预览」这些显式入口打开（`previewFileEntry`）。
    pub(in crate::shell) fn note_selected_only(&mut self, path: &str) {
        let file = self.rows.iter().chain(self.virtual_rows.iter()).any(|row| row.path == path && row.kind != "directory");
        if !file {
            return;
        }
        self.select_only = Some(path.to_string());
        self.click_load = self.select_only.clone();
    }

    /// 素材详情到达。单击发起的那次读取只在右侧详情里看；双击、搜索结果、外部打开这些
    /// 别处发起的读取回来就进入预览，和 Vue 里它们显式设置 `previewFileEntry` 一致。
    pub(in crate::shell) fn note_detail_arrived(&mut self, path: &str) {
        if self.click_load.as_deref() == Some(path) {
            self.click_load = None;
            return;
        }
        self.select_only = None;
    }

    /// 记下列表区宽度。半像素以内的抖动不算变化，避免布局回报来回重建。
    pub(in crate::shell) fn note_list_width(&mut self, width: f32) {
        if !width.is_finite() || width <= 0.0 {
            eprintln!("Nana 列表区宽度无效：{width}");
            return;
        }
        if self.list_width.is_some_and(|current| (current - width).abs() < 0.5) {
            return;
        }
        self.list_width = Some(width);
    }

    /// 标签组是否展开：用户在这个文件上点过就照点的来，否则有标签才展开（Vue 的 `tagsExpanded`）。
    pub(in crate::shell) fn tags_expanded(&self, path: &str, has_tags: bool) -> bool {
        match &self.tags_open {
            Some((open_path, open)) if open_path == path => *open,
            _ => has_tags,
        }
    }

    /// 预览页是否该替换文件列表。单击选中的文件只在右侧详情里看，和 Vue 的 `previewFileEntry` 一致。
    pub(in crate::shell) fn preview_open(&self, target: Option<&str>) -> bool {
        target.is_some() && self.select_only.as_deref() != target
    }

    pub(in crate::shell) fn selection_has_virtual(&self) -> bool {
        self.selected.iter().any(|path| {
            self.rows.iter().chain(self.virtual_rows.iter()).any(|row| row.path == *path && row.is_virtual)
        })
    }

    pub(in crate::shell) fn select_visible(&mut self, ctx: &FileContext, path: &str, mode: SelectionMode) {
        let rows = self.visible_rows(ctx);
        if !rows.iter().any(|row| row.path == path) {
            return;
        }
        let previous_primary = self.primary.clone();
        match mode {
            SelectionMode::Replace => self.replace_selection(path),
            SelectionMode::Toggle => self.toggle_selection(path),
            SelectionMode::Range => self.range_selection(&rows, path),
        }
        self.clear_rename_if_needed(previous_primary);
    }

    pub(in crate::shell) fn replace_selection(&mut self, path: &str) {
        self.selected = vec![path.to_string()];
        self.primary = Some(path.to_string());
        self.anchor = Some(path.to_string());
    }

    /// Vue `selectWorkspaceEntry` 的 `toggle`：没选中的加进来并成为主选中项和锚点；选中的拿掉，
    /// 拿掉的是主选中项时由剩下的第一项接替，否则主选中项不变；锚点被拿掉时跟主选中项。
    pub(in crate::shell) fn toggle_selection(&mut self, path: &str) {
        let Some(index) = self.selected.iter().position(|item| item == path) else {
            self.selected.push(path.to_string());
            self.primary = Some(path.to_string());
            self.anchor = Some(path.to_string());
            return;
        };
        self.selected.remove(index);
        let kept = |item: &Option<String>| item.as_ref().is_some_and(|item| self.selected.contains(item));
        if !kept(&self.primary) {
            self.primary = self.selected.first().cloned();
        }
        if !kept(&self.anchor) {
            self.anchor = self.primary.clone();
        }
    }

    /// 没有锚点时范围选择退回替换，避免从列表头意外拉出一段。
    pub(in crate::shell) fn range_selection(&mut self, rows: &[FileRow], path: &str) {
        let Some(anchor) = self.anchor.clone() else {
            self.replace_selection(path);
            return;
        };
        let Some(anchor_index) = rows.iter().position(|row| row.path == anchor) else {
            self.replace_selection(path);
            return;
        };
        let Some(index) = rows.iter().position(|row| row.path == path) else {
            return;
        };
        let (start, end) = if anchor_index <= index { (anchor_index, index) } else { (index, anchor_index) };
        self.selected = rows[start..=end].iter().map(|row| row.path.clone()).collect();
        self.primary = Some(path.to_string());
    }

    pub(in crate::shell) fn prune_against(&mut self, rows: &[FileRow]) {
        let paths: HashSet<String> = rows.iter().map(|row| row.path.clone()).collect();
        let previous_primary = self.primary.clone();
        self.selected.retain(|path| paths.contains(path));
        if self.primary.as_ref().is_some_and(|path| !paths.contains(path)) {
            self.primary = self.selected.first().cloned();
        }
        if self.selected.is_empty() {
            self.primary = None;
            self.anchor = None;
        } else if self.anchor.as_ref().is_some_and(|path| !paths.contains(path)) {
            self.anchor = self.primary.clone();
        }
        self.clear_rename_if_needed(previous_primary);
    }

    pub(in crate::shell) fn clear_rename_if_needed(&mut self, previous_primary: Option<String>) {
        let multiple = self.selected.len() != 1;
        let primary_changed = self.primary != previous_primary;
        if multiple || (self.rename_path.is_some() && primary_changed) {
            self.rename_path = None;
            self.name_draft.clear();
            if self.dialog == FileDialog::Rename {
                self.dialog = FileDialog::Closed;
            }
        }
    }
}
