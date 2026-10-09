//! 文件路由（常驻）：文件面板，含回收站、智能文件夹和文件预览。
//!
//! 主体固定高度，由面板内部自己滚（`.workspace-page__body--fixed`）。分支只在进入路由时建一次，
//! 同步时只写信号：文件列的头部、列表、详情，元数据编辑区和预览页各有自己的信号组。
//!
//! 别的模块的旧视图在这里登记成岛（见 `view_part_primary::Island`）：筛选栏、关闭确认、文件对话框、
//! 导出对话框和两处播放条。岛的内容仍由它们的旧视图函数整块建出，版本变了才换，常驻部分不动。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use nana_ui::runtime::view::{node_ref, signal, widget, AnyView, IntoView, NodeRef, Signal};
use nana_ui::runtime::{LengthSpec, Stack};

use super::files_view::{self, ColumnSignals, ColumnSlots};
use super::hot::HotSignals;
use super::inspect_metadata_view::MetadataSignals;
use super::inspect_view::PreviewSignals;
use super::render::{PRIMARY_INSET_X, PRIMARY_INSET_Y};
use super::view_part_primary::Island;
use super::{MainRegion, ShellViewModel};

/// 文件路由的常驻信号和岛的占位节点。
#[derive(Clone, Copy)]
pub(crate) struct FilesRouteSignals {
    /// 筛选栏开着（有仓库时）。
    filter_open: Signal<bool>,
    filter: NodeRef,
    slots: ColumnSlots,
    column: ColumnSignals,
    metadata: MetadataSignals,
    preview: PreviewSignals,
}

impl FilesRouteSignals {
    /// 在主区块的常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self {
            filter_open: signal(filter_open(model)),
            filter: node_ref(),
            slots: ColumnSlots::new(),
            column: ColumnSignals::new(model),
            metadata: MetadataSignals::new(model),
            preview: PreviewSignals::new(model),
        }
    }

    /// 写入投影，每组只写变了的信号。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        self.filter_open.try_set_if_changed(filter_open(model));
        self.column.write(model);
        self.metadata.write(model);
        self.preview.write(model);
    }
}

/// 首页外框上方的筛选栏：有仓库且筛选栏打开时显示（`route_home::page` 的同一条件）。
fn filter_open(model: &ShellViewModel) -> bool {
    model.workspace.main_region() == MainRegion::HasRepository && model.inspect.filter_bar_open
}

/// 文件路由的岛，和分支里的占位节点一一对应。
pub(super) fn islands(signals: FilesRouteSignals) -> Vec<Island> {
    let slots = signals.slots;
    vec![
        Island { slot: signals.filter, build: filter_bar, stamp: revision },
        // legacy_dialog_island：关闭确认、文件对话框和导出对话框。合并后由浮层块接管，删除。
        Island { slot: slots.close_prompt, build: |model| super::input::close_prompt(model), stamp: revision },
        Island { slot: slots.file_dialog, build: |model| super::workspace_dialogs::file_dialog(model), stamp: revision },
        Island { slot: slots.export_dialog, build: |model| super::workspace_dialogs::export_dialog(model), stamp: revision },
        Island { slot: slots.player, build: workbench_player, stamp: player_stamp },
        Island { slot: slots.preview_bar, build: preview_player, stamp: player_stamp },
    ]
}

/// 岛的版本：ViewModel 每次归约都算新版本，岛跟着整块重建（和改常驻以前一样）。
fn revision(model: &ShellViewModel) -> u64 {
    model.revision
}

fn filter_bar(model: &ShellViewModel) -> Option<AnyView> {
    filter_open(model).then(|| super::inspect_search_view::filter_bar(model))
}

/// 工作台左列底下的播放条：预览页打开时由预览页自己放。
fn workbench_player(model: &ShellViewModel) -> Option<AnyView> {
    (!files_view::previewing(model) && model.player_surface_visible()).then(|| super::player_view::player_surface(model))
}

/// 预览页底部的播放条。
fn preview_player(model: &ShellViewModel) -> Option<AnyView> {
    files_view::previewing(model).then(|| super::player_view::hosted_bar(model))
}

/// 播放条岛的版本：播放条读到的状态（`player_bar.rs` 的 `BarProps`、窄排法、圆角、下载进度）和岛该不该有内容。
///
/// 播放条量到自己的宽度后会发 `BarResized`，每次归约都重建会量了又建、建了又量，所以这里不按 ViewModel
/// 版本，只按它真正读到的值。播放进度和时间走热信号，不在里面。播放条改读别的字段时这里跟着加。
fn player_stamp(model: &ShellViewModel) -> u64 {
    let mut hasher = DefaultHasher::new();
    let player = &model.player;
    let session = &player.session;
    (files_view::previewing(model), model.player_surface_visible()).hash(&mut hasher);
    (player.download_text(), &player.current_id, player.can_play, player.queue_open, &player.repo_id).hash(&mut hasher);
    format!("{:?}", player.mode).hash(&mut hasher);
    (player.settings.image_duration_ms, player.settings.object_fit_cover).hash(&mut hasher);
    (&session.status, &session.error, session.volume.to_bits(), session.duration_ms).hash(&mut hasher);
    player.listed.as_ref().map(|detail| &detail.playlist.playlist_id).hash(&mut hasher);
    for item in &player.queue {
        (&item.id, &item.playlist_id, &item.asset_id, &item.path, &item.filename, &item.extension, &item.status).hash(&mut hasher);
        (&item.status_reason, item.transient, &item.player_type_id, &item.player_label, &item.file_class, &item.thumbnail_path).hash(&mut hasher);
    }
    if let Some(item) = player.current_item() {
        let contribution = player.contributions.iter().find(|entry| entry.player_type_id == item.player_type_id);
        contribution.map(|entry| (&entry.file_class, entry.supports_seek, entry.supports_volume)).hash(&mut hasher);
        let candidate = player.candidates.iter().find(|entry| entry.player_type_id == item.player_type_id);
        candidate.map(|entry| (&entry.file_class, entry.supports_seek, entry.supports_volume)).hash(&mut hasher);
        let thumbnail = item.thumbnail_path.as_deref();
        let ready = model.files.rows.iter().any(|row| row.texture_ready && row.thumbnail_path.as_deref() == thumbnail);
        (thumbnail, ready).hash(&mut hasher);
    }
    (player.bar_width.to_bits(), model.narrow_viewport(), model.admin.corner_radius.to_bits()).hash(&mut hasher);
    hasher.finish()
}

/// 文件路由的分支：首页外框（左右 24、上下 20），筛选栏在上，下面是固定高度的主体。
pub(super) fn view(signals: FilesRouteSignals, hot: HotSignals) -> AnyView {
    let filter = widget(Stack::column(0.0)).visible(signals.filter_open).node_ref(signals.filter).key("files-island-filter");
    let column = files_view::file_column(signals.column, signals.slots, signals.metadata, signals.preview, hot);
    let body = widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0))).children((column,)).key("workspace-page-body");
    widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)).padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y))
        .children((filter, body))
        .key("workspace-page")
        .into_any()
}

#[cfg(test)]
#[path = "route_files_tests.rs"]
mod tests;
