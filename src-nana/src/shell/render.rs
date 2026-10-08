//! 原生壳层的 Runtime 视图挂载；复用唯一文档与 GPU 上下文。

use std::cell::RefCell;

use super::*;
use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, AppShell, Button, Dialog, FrameworkError, JustifySpec, LengthSpec, MountedView, Progress,
    RadiusTier, RuntimeDocument, ScrollAxes, ScrollView, SemanticColorRole, Stack, Text, TextChanged,
    TextHorizontalAlignment, TextInput, ValidationIntent, ValidationMessage, Workspace,
};
use nana_ui::{ButtonKind, RegionId, RegionRole, RegionState, WorkspaceLayout, WorkspaceModel};

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
    SHELL_MOUNT.with(|slot| {
        if let Some(previous) = slot.borrow_mut().take() {
            let live = previous
                .roots()
                .iter()
                .any(|id| document.context().world().contains(*id));
            if live {
                if let Err(error) = previous.unmount(document.context_mut()) {
                    eprintln!("Nana 卸载上一棵壳层失败：{error}");
                }
            }
        }
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
            } else if let Some(dialog) = delete_repository_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::admin::plugin_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::input::playlist_name_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = playlist_creator_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::folder_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::smart_folder_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(popover) = super::sidebar_view::repository_popover(&view_model) {
                shell = shell.overlay(super::motion::paint_panel(popover, &view_model));
            } else if let Some(popover) = super::admin::task_popover(&view_model) {
                shell = shell.overlay(super::motion::paint_panel(popover, &view_model));
            } else if let Some(menu) = super::files_view::entry_menu(&view_model) {
                shell = shell.overlay(menu);
            }
            shell
        })?;
    super::title_bar::bind_window_controls(document)?;
    crate::window_host::bind_escape(document);
    crate::window_host::bind_file_drop(document);
    SHELL_MOUNT.with(|slot| *slot.borrow_mut() = Some(mounted));
    Ok(())
}

/// 窄窗逻辑宽。默认侧栏不随窗口缩小，主区下限要和它对得上。
const NARROW_WINDOW_PX: f32 = 390.0;
/// Nana 工作区在两条轨道之间留的发丝间隙，画的是壳层底色，和侧栏同色。
pub(super) const WORKBENCH_GAP_PX: f32 = nana_ui_core::HAIRLINE;
/// Vue `.shell` 的 `--lilia-primary-inset`：主区内容左右 24、上下 20。
const PRIMARY_INSET_X: f32 = 24.0;
const PRIMARY_INSET_Y: f32 = 20.0;

/// 主区最小宽。390 里还要放下默认侧栏和间隙，不能再用 320。
/// 宽窗里主区按 fill 铺开，这个下限不会把 1200 的版式压窄。
fn primary_min_px() -> f32 {
    (NARROW_WINDOW_PX - crate::theme_map::SIDEBAR_DEFAULT_PX).max(96.0)
}

/// Vue 标题栏高度。启动页的 `min-height: 100%` 要扣掉它和主区内边距。
const TITLE_BAR_PX: f32 = 36.0;

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
    let fill_height = LengthSpec::CalcViewportOffset {
        axis: nana_ui_core::ViewportAxis::Height,
        value: 100.0,
        offset_px: -(TITLE_BAR_PX + PRIMARY_INSET_Y * 2.0),
    };
    let section = widget(
        Stack::column(0.0)
            .min_height(fill_height)
            .padding(24.0)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center),
    )
    .children((startup_panel(model),))
    .key("workspace-startup")
    .into_any();
    scroll_route(section)
}

/// 设置路由：内边距放在纵向滚动里，整页和内边距一起滚动。
fn settings_route(view_model: &ShellViewModel) -> AnyView {
    scroll_route(super::admin::settings_page(view_model))
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
            MainRegion::MissingRepository => missing_repository_panel(model).into_any(),
            // 没有资源库时标题栏搜索也会切到搜索面板，由搜索面板画「还没有可搜索的资源库」。
            MainRegion::EmptyRepository if model.workspace.panel == WorkspacePanel::Search => {
                super::inspect_search_view::search_panel(model)
            }
            MainRegion::EmptyRepository => super::input::empty_repository_panel(model),
            _ => home_panel(model),
        };
        widget(scroll_view().follow_end(model.admin_logs_follow_end()))
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

/// 24px 圆标。当前步用强调色，完成用成功色，失败用危险色。
fn startup_step(item: super::workspace::StartupStepItem) -> AnyView {
    let (fill, border, foreground) = match item.state {
        super::workspace::StartupStepState::Current => (SemanticColorRole::AccentSoft, SemanticColorRole::Accent, SemanticColorRole::Accent),
        super::workspace::StartupStepState::Done => (SemanticColorRole::Subtle, SemanticColorRole::Success, SemanticColorRole::Success),
        super::workspace::StartupStepState::Error => (SemanticColorRole::Subtle, SemanticColorRole::Danger, SemanticColorRole::Danger),
        super::workspace::StartupStepState::Pending => (SemanticColorRole::Subtle, SemanticColorRole::Border, SemanticColorRole::Muted),
    };
    let mut number = Text::new(item.number.to_string()).color(foreground).font_size(12.0).font_weight(600);
    number.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(Stack::row(10.0).align(AlignSpec::Start)).children((
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(24.0))
                .height(LengthSpec::Px(24.0))
                .min_width(LengthSpec::Px(24.0))
                .min_height(LengthSpec::Px(24.0))
                .grow(0.0)
                .shrink(0.0)
                .surface(fill)
                .outline(border, 1.0)
                .radius_px(999.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center),
        )
        .children((widget(number).key(format!("startup-index-{}", item.number)),)),
        widget(Stack::column(2.0)).children((
            text(item.label).key(format!("startup-label-{}", item.number)),
            text(item.detail).key(format!("startup-copy-{}", item.number)),
        )),
    )).into_any()
}

/// 启动四步。标题 20px，步骤号是 24px 圆标，进度只画 6px 条。
fn startup_panel(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let startup = &model.workspace.startup;
    let steps = startup.step_items().into_iter().map(startup_step);
    let error = startup.error.clone().map(|error| text(error).key("startup-error"));
    let retry = (startup.status == StartupStatus::Error).then(|| {
        button("重试").key("startup-retry").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::StartupRetry);
        })
    });
    let mut progress = Progress::new(f64::from(model.motion.startup_percent()), 100.0);
    {
        let layout = std::sync::Arc::make_mut(&mut progress.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Px(6.0));
    }
    let panel = widget(Stack::column(12.0).width(LengthSpec::Px(640.0))).children((
        widget(Text::new("MomoBako").color(SemanticColorRole::Faint).font_size(11.0).font_weight(600)).key("startup-eyebrow"),
        widget(Text::new(startup.step_label.clone()).font_size(20.0).font_weight(600)).key("startup-title"),
        widget(Text::new(format!("第 {} / {} 步", startup.current_step, startup.total_steps)).color(SemanticColorRole::Muted).font_size(13.0)).key("startup-meta"),
        widget(progress).key("startup-progress"),
        text(startup.step_detail.clone()).key("startup-detail"),
        widget(Stack::column(8.0)).children(steps.collect::<Vec<_>>()).key("startup-steps"),
        error,
        retry,
    ));
    widget(
        Stack::fill_column(0.0)
            .padding_xy(24.0, 24.0)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center),
    )
    .children((panel,))
}

/// 缺失仓库的刷新、重定向和删除。忙或删除中时按钮不可再次提交。
fn missing_repository_panel(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let repository = model.workspace.active_repository();
    let name = repository.map(|item| item.name.clone()).unwrap_or_else(|| "资源库不可用".into());
    let path = repository.map(|item| item.path.clone()).unwrap_or_default();
    let cache_issue = repository.is_some_and(|item| item.is_source_cache_issue());
    let summary = if cache_issue {
        "这个来源资源库需要在插件设置中配置本地缓存或重新认证。仓库记录和已有缓存不会被删除。"
    } else {
        "MomoBako 找不到这个资源库的本地文件夹。可以重定向到原资源库位置，或移除这条注册记录和本机缓存。"
    };
    let busy = model.workspace.missing_busy();
    let path_prompt = model.workspace.path_prompt && !cache_issue;
    let path_draft = model.workspace.path_draft.clone();
    let error = (!model.workspace.missing_error.is_empty()).then(|| text(model.workspace.missing_error.clone()).key("missing-error"));
    let path_editor = path_prompt.then(|| {
        widget(Stack::column(8.0)).children((
            widget(TextInput::new(path_draft).label("资源库新位置")).on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program(ShellMessage::MissingPathChanged(event.value.to_string()));
            }),
            button("确认重定向").key("missing-submit-path").disabled(busy).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingSubmitPath);
            }),
        ))
    });
    let card = widget(Stack::column(12.0).width(LengthSpec::Px(560.0))).children((
        widget(super::title_bar::shell_icon(
            nana_ui::icons_tabler::ALERT_TRIANGLE,
            "资源库丢失",
            false,
        ))
        .key("missing-icon"),
        text("资源库丢失").key("missing-eyebrow"),
        text(name).key("missing-name"),
        text(summary).key("missing-summary"),
        text(path).key("missing-path"),
        error,
        path_editor,
        widget(Stack::row(8.0)).children((
            button(model.workspace.missing_primary_label()).key("missing-primary").disabled(busy).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(if cache_issue {
                    ShellMessage::MissingOpenSourceSettings
                } else {
                    ShellMessage::MissingChoosePath
                });
            }),
            button("刷新").key("missing-refresh").disabled(busy).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingRefresh);
            }),
            button(model.workspace.missing_delete_label()).key("missing-delete").disabled(busy).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingOpenDelete);
            }),
        )),
    ));
    widget(
        Stack::fill_column(0.0)
            .padding_xy(24.0, 24.0)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center),
    )
    .children((card,))
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

/// 新建播放集对话框。空播放集页不再把表单铺在虚线框下面。
fn playlist_creator_dialog(model: &ShellViewModel) -> Option<impl IntoView + use<'_>> {
    if !model.playlist_dialog_open {
        return None;
    }
    let name = model.new_playlist_name.clone();
    let selected = model.selected_new_playlist_player_type_id.clone();
    let players = model.playlist_players.iter().take(8).map(|player| {
        let player_type_id = player.player_type_id.clone();
        let chosen = selected.as_deref() == Some(player.player_type_id.as_str());
        let label = if chosen {
            format!("已选 {}", player.label)
        } else {
            format!("使用 {}", player.label)
        };
        button(label).key(format!("playlist-player-{player_type_id}")).on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::SelectPlaylistPlayer(player_type_id.clone()));
        })
    }).collect::<Vec<_>>();
    let blocked = name.trim().is_empty() || selected.is_none();
    let notice = matches!(model.detail.as_str(), "播放列表名称不能为空" | "请先选择播放器类型" | "正在创建播放列表…")
        .then(|| widget(ValidationMessage::new(model.detail.clone(), ValidationIntent::Danger)).key("playlist-dialog-notice"));
    Some(
        widget(Dialog::new("新建播放集"))
            .body(widget(Stack::column(8.0)).children((
                widget(TextInput::new(name).label("名称").placeholder("例如 通勤歌单 / 参考分镜")).on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program(ShellMessage::NewPlaylistNameChanged(event.value.to_string()));
                }),
                text("播放类型").key("playlist-dialog-type"),
                widget(Stack::column(6.0)).children(players),
                notice,
            )))
            .footer(widget(Stack::row(8.0)).children((
                widget(super::workbench::ghost_button("取消")).key("playlist-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::ClosePlaylistDialog);
                }),
                widget(super::workbench::primary_button("创建")).key("create-playlist").disabled(blocked).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::CreatePlaylist);
                }),
            ))),
    )
}

fn delete_repository_dialog(model: &ShellViewModel) -> Option<impl IntoView + use<'_>> {
    let dialog = model.workspace.dialogs.last()?;
    let super::workspace::WorkspaceDialog::Delete(dialog) = dialog;
    let repository = model.workspace.repositories.iter().find(|item| item.repo_id == dialog.repo_id);
    let summary = repository
        .map(|item| format!("资源库“{}”位于 {}。下面每个操作都会移除当前注册记录。", item.name, item.path))
        .unwrap_or_else(|| "下面每个操作都会移除当前注册记录。".into());
    let deleting = model.workspace.deleting_mode.is_some();
    let modes = [DeleteMode::RecordOnly, DeleteMode::DeleteMetadata, DeleteMode::DeleteFolder];
    let options = modes.into_iter().map(|mode| {
        let enabled = model.workspace.delete_mode_enabled(mode);
        let kind = match mode {
            DeleteMode::RecordOnly => ButtonKind::Ghost,
            DeleteMode::DeleteMetadata | DeleteMode::DeleteFolder => ButtonKind::Danger,
        };
        widget(Stack::column(4.0)).children((
            widget(Button::new(mode.label()).kind(kind).disabled(deleting || !enabled)).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingConfirmDelete(mode));
            }),
            widget(super::workbench::meta(model.workspace.delete_mode_detail(mode))),
        ))
    });
    let error = (!dialog.error.is_empty()).then(|| widget(ValidationMessage::new(dialog.error.clone(), ValidationIntent::Danger)));
    Some(
        widget(Dialog::new("删除资源库"))
            .body(widget(Stack::column(8.0)).children((
                text(summary).key("delete-dialog-summary"),
                widget(Stack::column(8.0)).children(options.collect::<Vec<_>>()),
                error,
                deleting.then(|| text("处理中...").key("delete-dialog-busy")),
            )))
            .footer(widget(super::workbench::ghost_button("取消")).key("delete-dialog-cancel").disabled(deleting).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingCloseDelete);
            })),
    )
}


