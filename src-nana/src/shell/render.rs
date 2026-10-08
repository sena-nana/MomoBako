//! 原生壳层的 Runtime 视图挂载；复用唯一文档与 GPU 上下文。

use std::cell::RefCell;

use super::*;
use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, AppShell, Button, Dialog, FrameworkError, JustifySpec, LengthSpec, MountedView, Progress,
    RuntimeDocument, ScrollAxes, ScrollView, SemanticColorRole, Stack, Text, TextChanged, TextHorizontalAlignment,
    TextInput, ValidationIntent, ValidationMessage, Workspace,
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
            let settings_page = matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError);
            let is_playlists = matches!(view_model.page, ShellPage::Playlists)
                || view_model.workspace.panel == WorkspacePanel::Playlist;
            let live_files = view_model.files_surface_visible() && !settings_page;
            let show_page = settings_page
                || matches!(view_model.workspace.main_region(), MainRegion::HasRepository);
            let region = view_model.workspace.main_region();
            let primary = if live_files {
                super::files_view::live_file_column(&view_model)
            } else if show_page && !settings_page && is_playlists {
                playlist_page(&view_model)
            } else if show_page
                && !settings_page
                && matches!(
                    view_model.workspace.panel,
                    WorkspacePanel::Logs | WorkspacePanel::Extensions | WorkspacePanel::Actions
                )
            {
                super::workbench::page(vec![super::admin::admin_surface(&view_model)])
            } else if show_page && !settings_page && view_model.workspace.panel == WorkspacePanel::Search {
                let inspect = if view_model.inspect_surface_visible() {
                    super::inspect_view::inspect_surface(&view_model)
                } else {
                    widget(Stack::column(0.0)).into_any()
                };
                super::workbench::page(vec![inspect])
            } else if show_page {
                let plugin_page = if settings_page { super::admin::opened_plugin_pages(&view_model) } else { Vec::new() };
                if settings_page && !plugin_page.is_empty() {
                    // 插件设置打开时主区只留这一张卡，不再叠外观、缓存和关闭行为。
                    super::workbench::page(plugin_page)
                } else {
                    let (eyebrow, title) = section_heading(&view_model);
                    let mut body = Vec::new();
                    body.push(super::admin::admin_surface(&view_model));
                    // 缓存上限和关闭行为不在 Vue 设置页前几张卡里，排在音频、外观、仓库和外部素材之后。
                    if let Some(editor) = settings_editor(&view_model) {
                        body.push(editor);
                    }
                    framed_page(eyebrow, title, body, settings_page)
                }
            } else {
                match region {
                    MainRegion::MissingRepository => missing_repository_panel(&view_model).into_any(),
                    MainRegion::EmptyRepository => super::input::empty_repository_panel(&view_model),
                    MainRegion::Startup | MainRegion::LoadError | MainRegion::HasRepository => {
                        startup_panel(&view_model).into_any()
                    }
                }
            };
            let stage = widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)))
                .children((primary,))
                .key("workspace-body")
                .into_any();
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
                stage.into_any()
            };
            let title_bar = super::title_bar::title_bar(&view_model);
            let mut shell = widget(AppShell::new()).title_bar(title_bar).body(body);
            if let Some(dialog) = super::sidebar_view::folder_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = super::sidebar_view::smart_delete_dialog(&view_model) {
                shell = shell.overlay(super::motion::paint_modal(dialog, &view_model.motion));
            } else if let Some(dialog) = delete_repository_dialog(&view_model) {
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
/// 工作台两条轨道之间的发丝间隙。
const WORKBENCH_GAP_PX: f32 = 1.0;

/// 主区最小宽。390 里还要放下默认侧栏和间隙，不能再用 320。
/// 宽窗里主区按 fill 铺开，这个下限不会把 1200 的版式压窄。
fn primary_min_px() -> f32 {
    (NARROW_WINDOW_PX - crate::theme_map::SIDEBAR_DEFAULT_PX - WORKBENCH_GAP_PX).max(96.0)
}

/// 资源区加主区。分割是工作台的发丝间隙，主区圆角，不画常驻浅色分割条。
/// 侧栏宽度仍用调用方给出的展开值，不改用户保存的折叠状态。
fn workbench(sidebar: AnyView, stage: AnyView, width: f32) -> AnyView {
    let layout = WorkspaceLayout::new([
        RegionState::new(RegionId::Resources, RegionRole::Resources)
            .size(width)
            .min_size(SIDEBAR_MIN_PX)
            .max_size(SIDEBAR_MAX_PX)
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

/// 设置页正文放进纵向滚动。末尾留白挂在设置表面最后，滚到底才离开窗口边。
fn framed_page(eyebrow: impl Into<String>, title: impl Into<String>, body: Vec<AnyView>, scroll: bool) -> AnyView {
    let eyebrow = eyebrow.into();
    let title = title.into();
    let content = widget(Stack::column(12.0)).children(body).into_any();
    let content = if scroll {
        widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(1.0);
            layout.min_height = Some(LengthSpec::Px(0.0));
            layout.height = Some(LengthSpec::Fill);
            layout.flex_basis = Some(LengthSpec::Px(0.0));
        }))
        .children((content,))
        .into_any()
    } else {
        content
    };
    widget(Stack::fill_column(16.0).padding_xy(20.0, 18.0).min_height(LengthSpec::Px(0.0)))
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


