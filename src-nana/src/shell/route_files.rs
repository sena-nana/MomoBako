//! 文件路由（常驻）：文件面板，含回收站、智能文件夹和文件预览。
//!
//! 主体固定高度，由面板内部自己滚（`.workspace-page__body--fixed`）。分支只在进入路由时建一次，
//! 同步时只写信号：文件列的头部、列表、详情，元数据编辑区和预览页各有自己的信号组。首页外框上方是
//! 常驻筛选栏，它的信号是各首页路由共用的那一份（`RouteSignals::filter`），不在这里另存。
//!
//! 播放条仍是播放器模块的旧视图，在这里登记成岛（见 `view_part_primary::Island`）：工作台左列底下和
//! 预览页底部各一处。岛的内容由播放条的旧视图函数整块建出，版本变了才换，常驻部分不动。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, Stack};

use super::files_view::{self, ColumnSignals, ColumnSlots};
use super::hot::HotSignals;
use super::inspect_metadata_view::MetadataSignals;
use super::inspect_search_view::{resident_filter_bar, FilterBarSignals};
use super::inspect_view::PreviewSignals;
use super::route_search::home_page;
use super::view_part_primary::Island;
use super::ShellViewModel;

/// 文件路由的常驻信号和岛的占位节点。
#[derive(Clone, Copy)]
pub(crate) struct FilesRouteSignals {
    slots: ColumnSlots,
    column: ColumnSignals,
    metadata: MetadataSignals,
    preview: PreviewSignals,
}

impl FilesRouteSignals {
    /// 在主区块的常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self {
            slots: ColumnSlots::new(),
            column: ColumnSignals::new(model),
            metadata: MetadataSignals::new(model),
            preview: PreviewSignals::new(model),
        }
    }

    /// 写入投影，每组只写变了的信号。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        self.column.write(model);
        self.metadata.write(model);
        self.preview.write(model);
    }
}

/// 文件路由的岛，和分支里的占位节点一一对应。
pub(super) fn islands(signals: FilesRouteSignals) -> Vec<Island> {
    let slots = signals.slots;
    vec![
        Island { slot: slots.player, build: workbench_player, stamp: super::player_view::bar_stamp },
        Island { slot: slots.preview_bar, build: preview_player, stamp: super::player_view::bar_stamp },
    ]
}

/// 工作台左列底下的播放条：预览页打开时由预览页自己放。
fn workbench_player(model: &ShellViewModel) -> Option<AnyView> {
    (!files_view::previewing(model) && model.player_surface_visible()).then(|| super::player_view::hosted_bar(model))
}

/// 预览页底部的播放条。
fn preview_player(model: &ShellViewModel) -> Option<AnyView> {
    files_view::previewing(model).then(|| super::player_view::hosted_bar(model))
}

/// 文件路由的分支：首页外框（左右 24、上下 20），常驻筛选栏在上，下面是固定高度的主体。
pub(super) fn view(filter: FilterBarSignals, signals: FilesRouteSignals, hot: HotSignals) -> AnyView {
    let column = files_view::file_column(signals.column, signals.slots, signals.metadata, signals.preview, hot);
    let body = widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0))).children((column,)).key("workspace-page-body");
    home_page(Some(resident_filter_bar(filter)), body.into_any())
}

#[cfg(test)]
#[path = "route_files_tests.rs"]
mod tests;
