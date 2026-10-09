//! 骨架的几何：工作区的区域布局，以及主区内边距、标题栏高度这些各块共用的尺寸。
//!
//! 三块内容的视图在各自的模块里：侧栏 `view_part_sidebar.rs`，主区 `view_part_primary.rs` 和
//! `route_*.rs`，浮层 `view_part_overlay.rs`。

use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::Workspace;
use nana_ui::{RegionId, RegionRole, RegionState, WorkspaceLayout, WorkspaceModel};

/// 窄窗逻辑宽。默认侧栏不随窗口缩小，主区下限要和它对得上。
const NARROW_WINDOW_PX: f32 = 390.0;
/// Nana 工作区在两条轨道之间留的发丝间隙，画的是壳层底色，和侧栏同色。
pub(super) const WORKBENCH_GAP_PX: f32 = nana_ui_core::HAIRLINE;
/// Vue `.shell` 的 `--lilia-primary-inset`：主区内容左右 24、上下 20。
pub(super) const PRIMARY_INSET_X: f32 = 24.0;
pub(super) const PRIMARY_INSET_Y: f32 = 20.0;

/// 主区最小宽。390 里还要放下默认侧栏和间隙，不能再用 320。
/// 宽窗里主区按 fill 铺开，这个下限不会把 1200 的版式压窄。
fn primary_min_px() -> f32 {
    (NARROW_WINDOW_PX - crate::theme_map::SIDEBAR_DEFAULT_PX).max(96.0)
}

/// Vue 标题栏高度。启动页的 `min-height: 100%` 要扣掉它和主区内边距。
pub(super) const TITLE_BAR_PX: f32 = 36.0;

/// 骨架里的工作区：资源区和主区，资源区宽度按侧栏呈现宽度给。主区圆角，不画常驻浅色分割条。
/// 区域内容由 `ShellView` 挂好后放进来，宽度之后由绑定跟着侧栏呈现宽度走。
///
/// Vue 的主区紧贴侧栏（侧栏 276 时主区从 x=276 起）。Nana 两条轨道之间多一条发丝间隙，
/// 所以资源区少给一条间隙，侧栏右内边距也少一条（见 `sidebar_frame`），
/// 侧栏看上去仍是 `width` 宽，内容盒和 Vue 一样。
pub(super) fn workbench_workspace(width: f32) -> Workspace {
    let layout = WorkspaceLayout::new([
        RegionState::new(RegionId::Resources, RegionRole::Resources)
            .size(super::hot::resources_size(width))
            .min_size(SIDEBAR_MIN_PX - WORKBENCH_GAP_PX)
            .max_size(SIDEBAR_MAX_PX - WORKBENCH_GAP_PX)
            .collapsible(true)
            .resizable(true),
        RegionState::new(RegionId::Primary, RegionRole::Primary)
            .min_size(primary_min_px())
            .fill_priority(1),
    ])
    .expect("工作台只注册资源区和主区");
    Workspace::from_model(&WorkspaceModel::with_layout(layout), [])
}
