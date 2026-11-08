//! 文件表面的选择归约：单击、双击、右键和框选怎么改选择，替换、切换、范围选择，按新列表修剪选择，
//! 以及单击选中、标签组展开和列表宽度这类只影响画面的状态。
//!
//! 对应 Vue `useFileBrowserPanelViewModel.ts` 的选择规则和 `handleEntryClick`；预览打开与否对应
//! `previewFileEntry`。

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::backend::services::repository::AssetDetail;

use super::{
    EntryMenu, FileContext, FileDialog, FileRow, FilesEffect, FilesState, MenuBranch, SelectionMode,
};

impl FilesState {
    /// 仓库动作的 `selectedCount` 只数多选路径，不含单独的主选。
    pub(crate) fn selected_paths(&self) -> &[String] {
        &self.selected
    }

    /// 测试直接放入多选路径。生产选择仍走文件消息。
    #[cfg(test)]
    pub(crate) fn set_selected_paths(&mut self, paths: Vec<String>) {
        self.selected = paths;
    }

    /// 框选和拖放起点写入多选、主选和锚点。
    pub(crate) fn set_drag_selection(
        &mut self,
        paths: Vec<String>,
        primary: Option<String>,
        anchor: Option<String>,
    ) {
        self.selected = paths;
        self.primary = primary;
        self.anchor = anchor;
    }

    /// 单击一行（Vue `handleEntryClick`）：按选择方式改选择，记下详情跟去的那一项是单击只选中。
    /// 返回详情要跟去的那一项。
    pub(in crate::shell) fn activate_row(
        &mut self,
        ctx: &FileContext,
        path: &str,
        mode: SelectionMode,
    ) -> Option<String> {
        let follow = self.select_row(ctx, path, mode)?;
        self.note_selected_only(&follow);
        Some(follow)
    }

    /// 单击只改选择，目录不进入。右侧详情跟着主选中项走（Vue `currentFileEntry`），返回它跟去的那一项：
    /// 点中的这一行成了主选中项就是它；Ctrl 点掉主选中项、接替的是文件时是接替的那一项；主选中项
    /// 没变、接替的是目录或选择空了时不跟，详情原样留着。跟去的是文件时顺便读取素材。
    pub(in crate::shell) fn select_row(
        &mut self,
        ctx: &FileContext,
        path: &str,
        mode: SelectionMode,
    ) -> Option<String> {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能选择条目");
            return None;
        }
        if !self.visible_rows(ctx).iter().any(|row| row.path == path) {
            eprintln!("Nana 找不到要点选的文件：{path}");
            return None;
        }
        let previous = self.primary.clone();
        self.select_visible(ctx, path, mode);
        let primary = self.primary.clone()?;
        let row = self
            .visible_rows(ctx)
            .into_iter()
            .find(|row| row.path == primary)?;
        let directory = row.kind == "directory";
        if primary != path && (previous.as_deref() == Some(primary.as_str()) || directory) {
            return None;
        }
        if !directory {
            if let (Some(repo_id), Some(asset_id)) = (ctx.repo_id.clone(), row.asset_id) {
                self.effects
                    .push(FilesEffect::LoadAsset { repo_id, asset_id });
            }
        }
        Some(primary)
    }

    /// 双击一行。回收站里的文件照 Vue `previewFileEntryByDoubleClick` 留在列表：
    /// 已经是单击选中的那一条就不再读详情，也清不掉「只在右侧看」；还没选中才按单击选上。
    /// 其余情况和 [`Self::open_row`] 一样：目录进入，文件打开预览。
    pub(in crate::shell) fn open_row_message(&mut self, ctx: &FileContext, path: &str) {
        let trash_file = ctx.trash
            && self
                .visible_rows(ctx)
                .iter()
                .any(|row| row.path == path && row.kind != "directory");
        if trash_file {
            if self.primary.as_deref() == Some(path) && self.select_only.as_deref() == Some(path) {
                return;
            }
            self.activate_row(ctx, path, SelectionMode::Replace);
            return;
        }
        self.select_only = None;
        self.click_load = None;
        self.open_row(ctx, path);
    }

    /// 双击进入非虚拟目录，或打开文件预览。回收站里的文件不经这里。
    pub(in crate::shell) fn open_row(&mut self, ctx: &FileContext, path: &str) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能打开条目");
            return false;
        }
        let Some(row) = self
            .visible_rows(ctx)
            .into_iter()
            .find(|row| row.path == path)
        else {
            eprintln!("Nana 找不到要打开的文件：{path}");
            return false;
        };
        if row.kind == "directory" && !ctx.is_virtual() {
            return self.request_browse(ctx, &row.path);
        }
        let kind = row.kind.clone();
        let asset_id = row.asset_id.clone();
        let row_path = row.path.clone();
        self.select_visible(ctx, &row_path, SelectionMode::Replace);
        if kind != "directory" {
            if let (Some(repo_id), Some(asset_id)) = (ctx.repo_id.clone(), asset_id) {
                self.effects
                    .push(FilesEffect::LoadAsset { repo_id, asset_id });
            }
        }
        true
    }

    /// 右键打开菜单。路径还没选中时先像单击一样替换成这一条，右侧详情跟着换。
    pub(in crate::shell) fn open_entry_menu(
        &mut self,
        ctx: &FileContext,
        path: &str,
        x: f32,
        y: f32,
    ) {
        if self.visible_rows(ctx).iter().all(|row| row.path != path) {
            eprintln!("Nana 找不到要打开菜单的文件：{path}");
            return;
        }
        if !self.selected.iter().any(|item| item == path) {
            self.activate_row(ctx, path, SelectionMode::Replace);
        }
        self.menu_branch = MenuBranch::default();
        self.menu_pending = None;
        self.entry_menu = Some(EntryMenu {
            path: path.to_string(),
            x,
            y,
        });
    }

    /// 单击选中的是文件时记下它：素材详情回来后仍留在列表，不切到预览页。
    ///
    /// 单击文件夹两项都不动：右侧详情停在哪个文件、还在读哪个文件都照旧，列表不会因此换成那个文件的
    /// 预览页。Vue 单击只改选择，预览只由双击、「预览」这些显式入口打开（`previewFileEntry`）。
    pub(in crate::shell) fn note_selected_only(&mut self, path: &str) {
        let file = self
            .rows
            .iter()
            .chain(self.virtual_rows.iter())
            .any(|row| row.path == path && row.kind != "directory");
        if !file {
            return;
        }
        self.select_only = Some(path.to_string());
        self.click_load = self.select_only.clone();
    }

    /// 素材详情到达。单击发起的那次读取只在右侧详情里看；双击和外部打开这些别处发起的读取回来就进入预览。
    /// 搜索结果里的文件在目录快照到达时已经打开预览，不靠这一次才开。
    pub(in crate::shell) fn note_detail_arrived(&mut self, path: &str) {
        if self.click_load.as_deref() == Some(path) {
            self.click_load = None;
            return;
        }
        self.select_only = None;
    }

    /// 清掉详情卡上叠着的素材元数据。同一个仓库刷新时照 Vue 先把 `activeAssetDetail` 设成 null。
    pub(in crate::shell) fn clear_detail_metadata(&mut self) {
        if self.detail_asset_id.is_empty() && self.detail_metadata.is_empty() {
            return;
        }
        self.detail_asset_id.clear();
        self.detail_metadata.clear();
    }

    /// 记下素材详情的元数据，供详情卡按素材 id 叠到列表条目上。
    ///
    /// 列表行的扩展名、大小、硬链接、修改时间和元数据都不改。没有素材 id 时清掉上一份并记日志，
    /// 避免空 id 和没有素材 id 的条目对上。
    pub(in crate::shell) fn note_detail_metadata(&mut self, detail: &AssetDetail) {
        let asset_id = detail.summary.asset_id.trim();
        if asset_id.is_empty() {
            eprintln!("Nana 素材详情没有素材 id，详情卡不叠元数据");
            self.detail_asset_id.clear();
            self.detail_metadata.clear();
            return;
        }
        self.detail_asset_id = asset_id.to_string();
        self.detail_metadata.clear();
        for entry in &detail.metadata {
            self.detail_metadata
                .insert(entry.key.clone(), entry.value.clone());
        }
    }

    /// 保存读回的是当前叠着的那份素材时，换成服务器现在的元数据。别的素材不盖住正在看的这一份。
    pub(in crate::shell) fn refresh_detail_metadata(&mut self, detail: &AssetDetail) {
        if self.detail_asset_id == detail.summary.asset_id {
            self.note_detail_metadata(detail);
        }
    }

    /// 和这条的素材 id 相同的详情元数据。对不上、还没读回或素材 id 是空的，返回 `None`。
    pub(in crate::shell) fn detail_metadata(
        &self,
        asset_id: Option<&str>,
    ) -> Option<&BTreeMap<String, Value>> {
        let asset_id = asset_id.map(str::trim).filter(|id| !id.is_empty())?;
        (self.detail_asset_id == asset_id).then_some(&self.detail_metadata)
    }

    /// 记下列表区宽度。半像素以内的抖动不算变化，避免布局回报来回重建。
    pub(in crate::shell) fn note_list_width(&mut self, width: f32) {
        if !width.is_finite() || width <= 0.0 {
            eprintln!("Nana 列表区宽度无效：{width}");
            return;
        }
        if self
            .list_width
            .is_some_and(|current| (current - width).abs() < 0.5)
        {
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

    /// 播放条或播放集显式打开预览（Vue `setPreviewFilePath` + `selectWorkspaceEntry`）。
    ///
    /// 预览页立刻换上，不等这次素材详情回来。单击选中的同一条也要打开：清掉「只在右侧详情里看」
    /// 和还没回来的单击读取，选择换成这一条。路径是空的只记日志，选择不动。
    pub(in crate::shell) fn reveal_explicit_preview(&mut self, path: &str) {
        let path = path.trim();
        if path.is_empty() {
            eprintln!("Nana 没有可打开预览的路径");
            return;
        }
        let previous = self.primary.clone();
        self.select_only = None;
        self.click_load = None;
        self.replace_selection(path);
        self.clear_rename_if_needed(previous);
    }

    /// 预览页是否该替换文件列表。单击选中的文件只在右侧详情里看，和 Vue 的 `previewFileEntry` 一致。
    pub(in crate::shell) fn preview_open(&self, target: Option<&str>) -> bool {
        target.is_some() && self.select_only.as_deref() != target
    }

    /// 预览页「返回」（Vue `exitPreview`）：把正在看的文件记成单击选中，预览页让位给文件列表。
    /// 不改目录、不改多选、不排目录读取。没有正在看的文件时记日志，选择不动。
    pub(in crate::shell) fn close_preview(&mut self, target: Option<&str>) {
        let Some(path) = target.map(str::trim).filter(|path| !path.is_empty()) else {
            eprintln!("Nana 没有正在看的文件，不能从预览返回");
            return;
        };
        self.select_only = Some(path.to_string());
    }

    pub(in crate::shell) fn selection_has_virtual(&self) -> bool {
        self.selected.iter().any(|path| {
            self.rows
                .iter()
                .chain(self.virtual_rows.iter())
                .any(|row| row.path == *path && row.is_virtual)
        })
    }

    pub(in crate::shell) fn select_visible(
        &mut self,
        ctx: &FileContext,
        path: &str,
        mode: SelectionMode,
    ) {
        let rows = self.visible_rows(ctx);
        if !rows.iter().any(|row| row.path == path) {
            return;
        }
        let previous_primary = self.primary.clone();
        match mode {
            SelectionMode::Replace => self.replace_selection(path),
            SelectionMode::Toggle => self.toggle_selection(path),
            SelectionMode::Range => {
                let ordered = Self::screen_order(ctx, &rows);
                self.range_selection(&ordered, path);
            }
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
        let kept = |item: &Option<String>| {
            item.as_ref()
                .is_some_and(|item| self.selected.contains(item))
        };
        if !kept(&self.primary) {
            self.primary = self.selected.first().cloned();
        }
        if !kept(&self.anchor) {
            self.anchor = self.primary.clone();
        }
    }

    /// 范围选择用的顺序，和卡片画出来的顺序一样。
    ///
    /// 普通目录和回收站照 Vue `createDerivedStateFromSnapshot` 的 `visibleEntries`：文件夹保持
    /// 原来的先后，整组排在文件前面。快照把两种穿插着给时，画面已经先画文件夹，Shift 也按这个顺序拉。
    /// 分类视图和智能文件夹不拆成两组，仍按列表本身的顺序。
    fn screen_order(ctx: &FileContext, rows: &[FileRow]) -> Vec<FileRow> {
        if ctx.is_virtual() {
            return rows.to_vec();
        }
        let mut directories = Vec::new();
        let mut files = Vec::new();
        for row in rows {
            if row.kind == "directory" {
                directories.push(row.clone());
            } else {
                files.push(row.clone());
            }
        }
        directories.extend(files);
        directories
    }

    /// 没有锚点时范围选择退回替换，避免从列表头意外拉出一段。
    ///
    /// `rows` 必须已经是画面顺序（[`Self::select_visible`] 里的 [`screen_order`]）。
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
        let (start, end) = if anchor_index <= index {
            (anchor_index, index)
        } else {
            (index, anchor_index)
        };
        self.selected = rows[start..=end]
            .iter()
            .map(|row| row.path.clone())
            .collect();
        self.primary = Some(path.to_string());
    }

    /// 智能文件夹查回来以后照 Vue `selectSmartFolder` 清掉选择。
    ///
    /// 查询期间列表已经换成「正在读取目录」，选择留到这一份结果对上再清；
    /// 用户已经离开智能文件夹面板时不清，免得把目录里的选择一起抹掉。
    pub(in crate::shell) fn drop_smart_selection(&mut self) {
        let previous = self.primary.clone();
        self.selected.clear();
        self.primary = None;
        self.anchor = None;
        self.select_only = None;
        self.click_load = None;
        self.clear_rename_if_needed(previous);
    }

    pub(in crate::shell) fn prune_against(&mut self, rows: &[FileRow]) {
        let paths: HashSet<String> = rows.iter().map(|row| row.path.clone()).collect();
        let previous_primary = self.primary.clone();
        self.selected.retain(|path| paths.contains(path));
        if self
            .primary
            .as_ref()
            .is_some_and(|path| !paths.contains(path))
        {
            self.primary = self.selected.first().cloned();
        }
        if self.selected.is_empty() {
            self.primary = None;
            self.anchor = None;
        } else if self
            .anchor
            .as_ref()
            .is_some_and(|path| !paths.contains(path))
        {
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
