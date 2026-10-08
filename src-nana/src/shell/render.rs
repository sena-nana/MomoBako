//! 原生壳层的 Runtime 视图挂载；复用唯一文档与 GPU 上下文。

use std::cell::RefCell;

use super::*;
use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AppShell, FrameworkError, LengthSpec, MountedView, RadiusTier, RuntimeDocument, ScrollAxes, ScrollView,
    SemanticColorRole, Stack, TextChanged, TextInput, Workspace,
};
use nana_ui::{RegionId, RegionRole, RegionState, WorkspaceLayout, WorkspaceModel};


thread_local! {
    /// 上一次挂上的壳层。再次挂载前先卸掉，避免文档里叠多棵壳。
    static SHELL_MOUNT: RefCell<Option<MountedView>> = const { RefCell::new(None) };
}

/// 在给定 Runtime 文档中挂载完整的 MomoBako 壳层。
pub fn mount_shell(
    document: &mut RuntimeDocument,
    model: &ShellViewModel,
) -> Result<(), FrameworkError> {
    let document_id = document.document();
    let view_model = model.clone();
    // 卸掉旧树前记下焦点，新树挂上后按键路径找回，避免文本框每打一个字就失焦。
    let kept_focus = SHELL_MOUNT.with(|slot| {
        let previous = slot.borrow_mut().take()?;
        let live = previous
            .roots()
            .iter()
            .any(|id| document.context().world().contains(*id));
        if !live {
            return None;
        }
        let kept = super::remount_focus::capture(document, previous.roots());
        if let Err(error) = previous.unmount(document.context_mut()) {
            eprintln!("Nana 卸载上一棵壳层失败：{error}");
        }
        kept
    });
    let mounted = document
        .context_mut()
        .mount_view_root(document_id, move || {
            // 标题栏、侧栏和主工作区共用同一套产品表面。验收页不再另挂按钮列表。
            let stage = primary_stage(primary_route(&view_model));
            let presented_sidebar = view_model.motion.sidebar_presented_width();
            let show_sidebar = presented_sidebar > 0.5
                && view_model.workspace.startup.status == StartupStatus::Ready;
            let body = if show_sidebar {
                let sidebar = widget(super::sidebar_view::sidebar_frame())
                    .top(super::sidebar_view::sidebar_switcher(&view_model))
                    .body(super::sidebar_view::sidebar_sections(&view_model))
                    .footer(super::sidebar_view::sidebar_footer(&view_model))
                    .into_any();
                workbench(sidebar, stage, presented_sidebar)
            } else {
                // 启动和侧栏收起时主区独占一行，圆角外露出壳层的 --bg-elev。
                widget(Stack::fill_column(0.0).surface(SemanticColorRole::Surface))
                    .children((stage,))
                    .into_any()
            };
            let title_bar = super::title_bar::title_bar(&view_model);
            let mut shell = widget(AppShell::new()).title_bar(title_bar).body(body);
            if let Some(dialog) = super::sidebar_view::folder_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::smart_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::repository_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::input::playlist_name_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::playlist_create_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::folder_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::smart_folder_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(popover) = super::sidebar_view::repository_popover(&view_model) {
                shell = shell.overlay(popover);
            } else if let Some(menu) = super::sidebar_view::folder_menu(&view_model) {
                shell = shell.overlay(menu);
            } else if let Some(popover) = super::admin::task_popover(&view_model) {
                shell = shell.overlay(super::motion::paint_panel(popover, &view_model.motion));
            } else if let Some(menu) = super::files_view::entry_menu(&view_model) {
                shell = shell.overlay(menu);
            }
            shell
        })?;
    super::title_bar::bind_window_controls(document)?;
    crate::window_host::bind_escape(document);
    crate::window_host::bind_file_drop(document);
    super::sidebar_view::bind_field_labels(document);
    if let Some(focus) = kept_focus {
        focus.restore(document, mounted.roots());
    }
    SHELL_MOUNT.with(|slot| *slot.borrow_mut() = Some(mounted));
    Ok(())
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
    scroll_route(super::startup_view::startup_section(model))
}

/// 设置路由：内边距放在纵向滚动里，整页和内边距一起滚动。
fn settings_route(view_model: &ShellViewModel) -> AnyView {
    let plugin_page = super::admin::opened_plugin_pages(view_model);
    let content = if !plugin_page.is_empty() {
        // 插件设置打开时主区只留这一张卡，不再叠外观、缓存和关闭行为。
        widget(Stack::column(16.0)).children(plugin_page).into_any()
    } else {
        let (eyebrow, title) = section_heading(view_model);
        let mut body = Vec::new();
        body.push(super::admin::admin_surface(view_model));
        // 缓存上限和关闭行为不在 Vue 设置页前几张卡里，排在音频、外观、仓库和外部素材之后。
        if let Some(editor) = settings_editor(view_model) {
            body.push(editor);
        }
        framed_page(eyebrow, title, body)
    };
    scroll_route(content)
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
        widget(scroll_view())
            .children((panel,))
            .key("workspace-page-body")
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
fn scroll_route(content: AnyView) -> AnyView {
    let padded = widget(Stack::column(0.0).padding_xy(PRIMARY_INSET_X, PRIMARY_INSET_Y))
        .children((content,))
        .into_any();
    widget(scroll_view()).children((padded,)).key("workspace-primary-scroll").into_any()
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

/// 资源区加主区。主区圆角，不画常驻浅色分割条。
/// 侧栏宽度仍用调用方给出的展开值，不改用户保存的折叠状态。
///
/// Vue 的主区紧贴侧栏（侧栏 276 时主区从 x=276 起）。Nana 两条轨道之间多一条发丝间隙，
/// 所以资源区少给一条间隙，侧栏右内边距也少一条（见 `sidebar_frame`），
/// 侧栏看上去仍是 `width` 宽，内容盒和 Vue 一样。
fn workbench(sidebar: AnyView, stage: AnyView, width: f32) -> AnyView {
    let layout = WorkspaceLayout::new([
        RegionState::new(RegionId::Resources, RegionRole::Resources)
            .size((width - WORKBENCH_GAP_PX).max(0.0))
            .min_size(SIDEBAR_MIN_PX - WORKBENCH_GAP_PX)
            .max_size(SIDEBAR_MAX_PX - WORKBENCH_GAP_PX)
            .collapsible(true)
            .resizable(true),
        RegionState::new(RegionId::Primary, RegionRole::Primary)
            .min_size(primary_min_px())
            .fill_priority(1),
    ])
    .expect("工作台只注册资源区和主区");
    widget(Workspace::from_model(&WorkspaceModel::with_layout(layout), []))
        .region(RegionId::Resources, sidebar)
        .region(RegionId::Primary, stage)
        .into_any()
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

/// 缓存上限和关闭行为。主题在外观卡里。保存仍走原来的设置消息。
fn settings_editor(view_model: &ShellViewModel) -> Option<AnyView> {
    if !matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError) {
        return None;
    }
    let cache_limit = view_model.settings_cache_limit_draft.clone();
    let player_id = view_model.settings.default_playlist_player_type_id.clone().unwrap_or_default();
    let close_behavior = view_model.settings.close_behavior.clone();
    Some(
        widget(Stack::column(12.0))
            .children((
                super::workbench::section_card(
                    "播放与缓存",
                    vec![
                        widget(TextInput::new(cache_limit).label("缩略图缓存上限（MB）"))
                            .on_cx(|_, event: &TextChanged, cx| {
                                cx.dispatch_program(ShellMessage::SettingsCacheLimitChanged(event.value.to_string()));
                            })
                            .into_any(),
                        widget(TextInput::new(player_id).label("默认播放器类型"))
                            .on_cx(|_, event: &TextChanged, cx| {
                                cx.dispatch_program(ShellMessage::SettingsPlayerChanged(event.value.to_string()));
                            })
                            .into_any(),
                    ],
                ),
                super::workbench::section_card(
                    "关闭行为",
                    vec![widget(Stack::bar(8.0))
                        .children((
                            text(format!("关闭行为：{}", close_label(&close_behavior))).key("settings-close-label"),
                            widget(Stack::spacer()),
                            button("确认后关闭").key("settings-close-confirm").on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::SettingsCloseBehaviorChanged("confirm".into()));
                            }),
                            button("最小化到托盘").key("settings-close-tray").on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::SettingsCloseBehaviorChanged("minimizeToTray".into()));
                            }),
                            button("直接退出").key("settings-close-quit").on_cx(|_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::SettingsCloseBehaviorChanged("quit".into()));
                            }),
                        ))
                        .into_any()],
                ),
                view_model.settings_error.clone().map(|error| text(format!("设置提示：{error}")).key("settings-error")),
                button("保存应用设置").key("save-application-settings").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::SaveSettings);
                }),
            ))
            .into_any(),
    )
}

/// 设置页标题和正文。滚动和外边距由设置路由给，这里是普通的纵向排列。
fn framed_page(eyebrow: impl Into<String>, title: impl Into<String>, body: Vec<AnyView>) -> AnyView {
    let eyebrow = eyebrow.into();
    let title = title.into();
    let content = widget(Stack::column(12.0)).children(body).into_any();
    widget(Stack::column(16.0))
        .children((
            widget(Stack::bar(16.0)).children((
                widget(Stack::column(4.0)).children((
                    text(eyebrow).key("section-eyebrow"),
                    text(title).key("section-title"),
                )),
                widget(Stack::spacer()),
            )),
            content,
        ))
        .into_any()
}

/// 关闭行为存的是 confirm、minimizeToTray、quit。画面用按钮上的中文。
fn close_label(value: &str) -> &str {
    match value {
        "confirm" => "确认后关闭",
        "minimizeToTray" => "最小化到托盘",
        "quit" => "直接退出",
        other => other,
    }
}

fn section_heading(model: &ShellViewModel) -> (&'static str, String) {
    if matches!(model.page, ShellPage::Settings | ShellPage::SettingsError) {
        return ("应用", "设置".into());
    }
    match model.workspace.panel {
        WorkspacePanel::Playlist => ("播放集", "播放集".into()),
        WorkspacePanel::Extensions => ("拓展能力", "文件系统与插件".into()),
        WorkspacePanel::Logs => ("LOGS", "系统日志".into()),
        WorkspacePanel::Actions => ("仓库", "动作".into()),
        WorkspacePanel::Search => ("搜索", "搜索结果".into()),
        _ => ("工作台", model.page.title().into()),
    }
}
