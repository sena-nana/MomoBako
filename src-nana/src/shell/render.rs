//! 壳层各块内容的视图函数：侧栏、主区路由和浮层，以及骨架里工作区的区域布局。
//!
//! 这些函数按 ViewModel 整块建出内容，由 `ShellView` 挂在骨架的槽位里；随动效和播放逐帧变的
//! 字段经 `hot::prop` 绑到热信号上。

use super::*;
use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack, Workspace};
use nana_ui::{RegionId, RegionRole, RegionState, WorkspaceLayout, WorkspaceModel};

use super::view_host::BodyMode;

/// 侧栏：仓库头、分组和底部入口。放进工作区的资源区。
pub(super) fn sidebar_view(model: &ShellViewModel) -> AnyView {
    widget(super::sidebar_view::sidebar_frame())
        .top(super::sidebar_view::sidebar_switcher(model))
        .body(super::sidebar_view::sidebar_sections(model))
        .footer(super::sidebar_view::sidebar_footer(model))
        .into_any()
}

/// 主区。有侧栏时放进工作区的主区；独占时外面再包一层壳层底色，圆角外露出 `--bg-elev`。
pub(super) fn primary_view(model: &ShellViewModel, mode: BodyMode) -> AnyView {
    let stage = primary_stage(primary_route(model));
    match mode {
        BodyMode::Workbench => stage,
        BodyMode::Solo => widget(Stack::fill_column(0.0).surface(SemanticColorRole::Surface)).children((stage,)).into_any(),
    }
}

/// 浮层：对话框、弹层和菜单，同一时刻最多一层，没有时为 `None`。对话框和任务弹层带开合动效。
pub(super) fn overlay_view(model: &ShellViewModel) -> Option<AnyView> {
    let modal = |dialog: AnyView| super::motion::paint_modal(dialog, &model.motion);
    if let Some(dialog) = super::sidebar_view::folder_delete_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::sidebar_view::smart_delete_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::sidebar_view::repository_delete_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::admin::plugin_delete_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::input::playlist_name_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::sidebar_view::playlist_create_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::sidebar_view::folder_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(dialog) = super::sidebar_view::smart_folder_dialog(model) {
        return Some(modal(dialog));
    }
    if let Some(popover) = super::sidebar_view::repository_popover(model) {
        return Some(popover);
    }
    if let Some(menu) = super::sidebar_view::folder_menu(model) {
        return Some(menu);
    }
    if let Some(popover) = super::admin::task_popover(model) {
        return Some(super::motion::paint_panel(popover, &model.motion));
    }
    super::files_view::entry_menu(model)
}

/// 窄窗逻辑宽。默认侧栏不随窗口缩小，主区下限要和它对得上。
const NARROW_WINDOW_PX: f32 = 390.0;
/// Nana 工作区在两条轨道之间留的发丝间隙，画的是壳层底色，和侧栏同色。
pub(super) const WORKBENCH_GAP_PX: f32 = nana_ui_core::HAIRLINE;
/// Vue `.shell` 的 `--lilia-primary-inset`：主区内容左右 24、上下 20。
const PRIMARY_INSET_X: f32 = 24.0;
pub(super) const PRIMARY_INSET_Y: f32 = 20.0;

/// 主区最小宽。390 里还要放下默认侧栏和间隙，不能再用 320。
/// 宽窗里主区按 fill 铺开，这个下限不会把 1200 的版式压窄。
fn primary_min_px() -> f32 {
    (NARROW_WINDOW_PX - crate::theme_map::SIDEBAR_DEFAULT_PX).max(96.0)
}

/// Vue 标题栏高度。启动页的 `min-height: 100%` 要扣掉它和主区内边距。
pub(super) const TITLE_BAR_PX: f32 = 36.0;

/// 主区外框：白底、`Lg` 档圆角（Vue 主区的 `--radius-lg`）。
///
/// 有侧栏时外面还有工作区的主区，同色同圆角；内边距由各路由自己放，
/// 设置页和启动页的内边距要跟着内容一起滚动。
fn primary_stage(primary: AnyView) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Lg),
    )
    .children((primary,))
    .key("workspace-body")
    .into_any()
}

/// 主区路由，对应 `AppShell.vue` 的 `LiliaPrimaryContent`。
///
/// 启动未就绪（含启动失败）只显示启动页；就绪后设置走设置路由，其余都是首页。
fn primary_route(model: &ShellViewModel) -> AnyView {
    let settings_page = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
    match model.workspace.main_region() {
        MainRegion::Startup | MainRegion::LoadError => startup_route(model),
        _ if settings_page => settings_route(model),
        region => home_route(model, region),
    }
}

/// 启动页。Vue 主区内容层 `overflow: auto`，窗口矮时整页连同内边距一起滚动；
/// `.workspace-startup` 的 `min-height: 100%` 让内容在高窗口里上下居中。
fn startup_route(model: &ShellViewModel) -> AnyView {
    scroll_route(super::startup_view::startup_section(model), "workspace-startup-scroll".into())
}

/// 设置路由：内边距放在纵向滚动里，整页和内边距一起滚动。
fn settings_route(view_model: &ShellViewModel) -> AnyView {
    // 设置页只有一张整页，滚动容器用固定键：同一页重挂时保留滚动位置。
    scroll_route(super::admin::settings_page(view_model), "settings-scroll".into())
}

/// 首页路由，对应 `Home.vue` 的 `.workspace-page`：占满主区、自身不滚动。
///
/// 有仓库且筛选栏打开时，筛选栏放在所有首页面板上方。文件面板（含智能文件夹、回收站和
/// 文件预览）主体固定高度，由面板内部自己滚（`.workspace-page__body--fixed`）；
/// 其他面板的主体是纵向滚动（`.workspace-page__body` 的 `overflow: auto`）。
fn home_route(model: &ShellViewModel, region: MainRegion) -> AnyView {
    let filter = (region == MainRegion::HasRepository && model.inspect.filter_bar_open)
        .then(|| super::inspect_search_view::filter_bar(model));
    let body = if model.files_surface_visible() {
        widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)))
            .children((super::files_view::live_file_column(model),))
            .key("workspace-page-body")
            .into_any()
    } else {
        let panel = match region {
            MainRegion::MissingRepository => super::startup_view::missing_repository_section(model),
            // 没有资源库时标题栏搜索也会切到搜索面板，由搜索面板画「还没有可搜索的资源库」。
            MainRegion::EmptyRepository if model.workspace.panel == WorkspacePanel::Search => {
                super::inspect_search_view::search_panel(model)
            }
            MainRegion::EmptyRepository => super::input::empty_repository_panel(model),
            _ => home_panel(model),
        };
        // 滚动主体按区域和面板取键：换面板时从顶部开始，同一面板重挂时保留滚动位置。
        // 日志面板追踪时跟随末尾。
        widget(scroll_view().follow_end(model.admin_logs_follow_end()))
            .children((panel,))
            .key(format!("workspace-page-scroll-{region:?}-{:?}", model.workspace.panel))
            .into_any()
    };
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y),
    )
    .children((filter, body))
    .key("workspace-page")
    .into_any()
}

/// 首页里文件面板以外的面板：播放集、日志、拓展、动作和搜索结果。
fn home_panel(model: &ShellViewModel) -> AnyView {
    let is_playlists = matches!(model.page, ShellPage::Playlists) || model.workspace.panel == WorkspacePanel::Playlist;
    if is_playlists {
        return playlist_page(model);
    }
    match model.workspace.panel {
        WorkspacePanel::Logs | WorkspacePanel::Extensions | WorkspacePanel::Actions => {
            super::workbench::page(vec![super::admin::admin_surface(model)])
        }
        WorkspacePanel::Search => {
            let inspect = if model.inspect_surface_visible() {
                super::inspect_view::inspect_surface(model)
            } else {
                widget(Stack::column(0.0)).into_any()
            };
            super::workbench::page(vec![inspect])
        }
        _ => {
            eprintln!("Nana 首页没有可显示的面板：{:?}", model.workspace.panel);
            widget(Stack::column(0.0)).into_any()
        }
    }
}

/// 主区内容层的 `overflow: auto`：内边距在滚动内容里，随内容一起滚动。
/// `key` 标出显示的内容，重挂时同键的滚动容器保留滚动位置。
fn scroll_route(content: AnyView, key: String) -> AnyView {
    let padded = widget(Stack::column(0.0).padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y))
        .children((content,))
        .into_any();
    widget(scroll_view()).children((padded,)).key(key).into_any()
}

/// 占满剩余高度的纵向滚动。
fn scroll_view() -> ScrollView {
    ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Fill);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
    })
}

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

/// 播放集页：播放表面，以及不能播放的条目。名称和当前目录不在这一页另开输入。
fn playlist_page(model: &ShellViewModel) -> AnyView {
    let mut body = Vec::new();
    if model.player_surface_visible() {
        body.push(super::player_view::player_surface(model));
    }
    if !model.playlist_item_status.is_empty() {
        body.push(text(format!("不可播放项目：{}", model.playlist_item_status)).key("playlist-item-status").into_any());
    }
    super::workbench::page(body)
}

