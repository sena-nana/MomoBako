//! 文件路由（常驻）：文件面板，含回收站、智能文件夹和文件预览。
//!
//! 主体固定高度，由面板内部自己滚（`.workspace-page__body--fixed`）。分支只在进入路由时建一次，
//! 同步时只写信号：文件列的头部、列表、详情，元数据编辑区和预览页各有自己的信号组。
//!
//! 别的模块的旧视图在这里登记成岛（见 `view_part_primary::Island`）：筛选栏、关闭确认、文件对话框、
//! 导出对话框和两处播放条。岛的内容仍由它们的旧视图函数整块建出，版本变了才换，常驻部分不动。

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
        Island { slot: slots.player, build: workbench_player, stamp: super::player_view::bar_stamp },
        Island { slot: slots.preview_bar, build: preview_player, stamp: super::player_view::bar_stamp },
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
    (!files_view::previewing(model) && model.player_surface_visible()).then(|| super::player_view::hosted_bar(model))
}

/// 预览页底部的播放条。
fn preview_player(model: &ShellViewModel) -> Option<AnyView> {
    files_view::previewing(model).then(|| super::player_view::hosted_bar(model))
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
