//! 原生壳层的 Runtime 视图挂载；复用唯一文档与 GPU 上下文。

use std::cell::RefCell;

use super::*;
use crate::theme_map::{SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, AppShell, Dialog, FrameworkError, GpuTextureView, JustifySpec, LengthSpec, List, MountedView,
    Progress, RuntimeDocument, SemanticColorRole, SidebarFrame, Stack, Text, TextChanged,
    TextHorizontalAlignment, TextInput, ValidationIntent, ValidationMessage, Workspace,
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
            // 标题栏、导航栏和主工作区分别承担窗口级操作、上下文导航和资源主线。
            let legacy_navigation = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(220.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 18.0),
            )
            .children((
                button("资源库").key("nav-library").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::FileList));
                }),
                button("播放列表").key("nav-playlists").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::Playlists));
                }),
                button("插件").key("nav-plugins").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::PluginSettings));
                }),
                button("设置").key("nav-settings").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Navigate(ShellPage::Settings));
                }),
            ));
            let status_summary = widget(Stack::column(8.0)).children((
                text(view_model.page.title()).key("page-title"),
                text(view_model.page.status()).key("page-status"),
                text(view_model.selection_label()).key("selection"),
                text(view_model.detail.clone()).key("page-detail"),
                text(view_model.file_entries_label()).key("file-entries"),
                text(view_model.plugin_entries_label()).key("plugin-entries"),
                text(view_model.log_entries_label()).key("log-entries"),
                text(view_model.playlist_entries_label()).key("playlist-entries"),
                text(view_model.system_status.clone().unwrap_or_else(|| "尚未读取系统服务状态".into()))
                    .key("system-status"),
            ));
            let task_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .active_task_ids
                    .iter()
                    .map(|task_id| {
                        let task_id = task_id.clone();
                        button(format!("取消任务 {task_id}"))
                            .key(format!("cancel-task-{task_id}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::CancelTask(task_id.clone()));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let task_progress = widget(Stack::fill_column(6.0)).children(
                view_model
                    .task_progress
                    .iter()
                    .take(8)
                    .map(|snapshot| {
                        let status = match snapshot.status.as_str() {
                            "cancelling" => "取消中",
                            "completed" => "已完成",
                            "cancelled" => "已取消",
                            "failed" => "失败",
                            "queued" => "排队中",
                            _ => "进行中",
                        };
                        text(format!(
                            "{} · {}",
                            snapshot
                                .label
                                .clone()
                                .unwrap_or_else(|| snapshot.protocol_id.clone()),
                            status
                        ))
                        .key(format!("task-progress-{}", snapshot.task_id))
                    })
                    .collect::<Vec<_>>(),
            );
            let file_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .browser_entries
                    .iter()
                    .take(8)
                    .map(|entry| {
                        let entry = entry.clone();
                        button(entry.name.clone())
                            .key(format!("file-entry-{}", entry.path))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(entry_message(&entry));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let plugin_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .plugin_entries
                    .iter()
                    .zip(view_model.plugin_entry_ids.iter())
                    .zip(view_model.plugin_enabled.iter())
                    .take(8)
                    .map(|((label, plugin_id), enabled)| {
                        let plugin_id = plugin_id.clone();
                        let select_id = plugin_id.clone();
                        let toggle_id = plugin_id.clone();
                        let delete_id = plugin_id.clone();
                        let next_enabled = !*enabled;
                        widget(Stack::fill_row(8.0)).children((
                            button(label.clone())
                                .key(format!("plugin-entry-{select_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::SelectPlugin(select_id.clone()));
                                }),
                            button(if *enabled { "停用" } else { "启用" })
                                .key(format!("toggle-plugin-{toggle_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::TogglePlugin {
                                        plugin_id: toggle_id.clone(),
                                        enabled: next_enabled,
                                    });
                                }),
                            button("删除")
                                .key(format!("delete-plugin-{delete_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::DeletePlugin(delete_id.clone()));
                                }),
                        ))
                    })
                    .collect::<Vec<_>>(),
            );
            let plugin_config_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .selected_plugin_id
                    .as_ref()
                    .into_iter()
                    .flat_map(|plugin_id| {
                        view_model.plugin_config_keys.iter().map(move |key| {
                            let plugin_id = plugin_id.clone();
                            let key = key.clone();
                            button(format!("删除配置 {key}"))
                                .key(format!("delete-plugin-config-{key}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::DeletePluginConfig {
                                        plugin_id: plugin_id.clone(),
                                        key: key.clone(),
                                    });
                                })
                        })
                    })
                    .collect::<Vec<_>>(),
            );
            let plugin_config_keys = view_model.plugin_config_keys.clone();
            let plugin_config_drafts = view_model.plugin_config_drafts.clone();
            let plugin_config_editors = widget(Stack::fill_column(8.0)).children(
                view_model
                    .selected_plugin_id
                    .as_ref()
                    .into_iter()
                    .flat_map(|plugin_id| {
                        let drafts = plugin_config_drafts.clone();
                        plugin_config_keys
                            .iter()
                            .map(move |key| {
                                let plugin_id = plugin_id.clone();
                                let key = key.clone();
                                let value = drafts.get(&key).cloned().unwrap_or_default();
                                let draft_key = key.clone();
                                let input = widget(TextInput::new(value).label(key.clone())).on_cx(
                                    move |_, event: &TextChanged, cx| {
                                        cx.dispatch_program(ShellMessage::PluginConfigDraftChanged {
                                            key: draft_key.clone(),
                                            value: event.value.to_string(),
                                        });
                                    },
                                );
                                let save_key = key.clone();
                                widget(Stack::fill_row(8.0)).children((
                                    input,
                                    button("保存").key(format!("save-plugin-config-{save_key}"))
                                        .on_cx(move |_, _: &Activate, cx| {
                                            cx.dispatch_program(ShellMessage::SavePluginConfig {
                                                plugin_id: plugin_id.clone(),
                                                key: save_key.clone(),
                                            });
                                        }),
                                ))
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let playlist_actions = widget(Stack::fill_column(6.0)).children(
                view_model
                    .playlist_entries
                    .iter()
                    .zip(view_model.playlist_entry_ids.iter())
                    .take(8)
                            .map(|(label, playlist_id)| {
                        let playlist_id = playlist_id.clone();
                        let open_id = playlist_id.clone();
                        let delete_id = playlist_id.clone();
                        widget(Stack::fill_row(8.0)).children((
                            button(label.clone())
                                .key(format!("playlist-entry-{open_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::SelectPlaylist(open_id.clone()));
                                }),
                            button("删除")
                                .key(format!("delete-playlist-{delete_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::DeletePlaylist(delete_id.clone()));
                                }),
                        ))
                    })
                    .collect::<Vec<_>>(),
            );
            let playlist_item_actions = if view_model.acceptance_scene {
                Some(widget(Stack::fill_column(6.0)).children(
                view_model
                    .playlist_item_entries
                    .iter()
                    .zip(view_model.playlist_item_ids.iter())
                    .take(8)
                    .map(|(label, item_id)| {
                        let item_id = item_id.clone();
                        let playlist_id = view_model.selected_playlist_id.clone().unwrap_or_default();
                        widget(Stack::fill_row(8.0)).children((
                            text(label.clone()).key(format!("playlist-item-{item_id}")),
                            button("上移")
                                .key(format!("move-playlist-item-up-{item_id}"))
                                .on_cx({
                                    let item_id = item_id.clone();
                                    move |_, _: &Activate, cx| cx.dispatch_program(ShellMessage::MovePlaylistItem { item_id: item_id.clone(), direction: -1 })
                                }),
                            button("下移")
                                .key(format!("move-playlist-item-down-{item_id}"))
                                .on_cx({
                                    let item_id = item_id.clone();
                                    move |_, _: &Activate, cx| cx.dispatch_program(ShellMessage::MovePlaylistItem { item_id: item_id.clone(), direction: 1 })
                                }),
                            button("移除")
                                .key(format!("remove-playlist-item-{item_id}"))
                                .on_cx(move |_, _: &Activate, cx| {
                                    cx.dispatch_program(ShellMessage::RemovePlaylistItem {
                                        playlist_id: playlist_id.clone(),
                                        item_id: item_id.clone(),
                                    });
                                }),
                        ))
                    })
                    .collect::<Vec<_>>(),
            ))
            } else {
                None
            };
            let player_surface = if view_model.player_surface_visible() {
                Some(super::player_view::player_surface(&view_model))
            } else {
                None
            };
            let playlist_item_status = if view_model.playlist_item_status.is_empty() {
                None
            } else {
                Some(text(format!("不可播放项目：{}", view_model.playlist_item_status)).key("playlist-item-status"))
            };
            let log_actions = if matches!(view_model.page, ShellPage::Loading) {
                None
            } else {
                Some(widget(Stack::fill_row(8.0)).children((
                    button("清理日志")
                        .key("clear-logs")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::ClearLogs)),
                )))
            };
            let playlist_name = view_model.playlist_name_draft.clone();
            let is_playlists = matches!(view_model.page, ShellPage::Playlists)
                || view_model.workspace.panel == WorkspacePanel::Playlist;
            let playlist_editor = if is_playlists {
                Some(widget(Stack::fill_row(8.0)).children((
                    widget(TextInput::new(playlist_name).label("播放列表名称")).on_cx(
                        |_, event: &TextChanged, cx| {
                            cx.dispatch_program(ShellMessage::PlaylistNameDraftChanged(
                                event.value.to_string(),
                            ));
                        },
                    ),
                    button("保存名称")
                        .key("save-playlist-name")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::SavePlaylistName)),
                )))
            } else {
                None
            };
            let new_playlist_name = view_model.new_playlist_name.clone();
            let playlist_player_choices = widget(Stack::fill_row(6.0)).children(
                view_model
                    .playlist_players
                    .iter()
                    .take(8)
                    .map(|player| {
                        let player_type_id = player.player_type_id.clone();
                        let label = format!("使用 {}", player.label);
                        button(label)
                            .key(format!("playlist-player-{}", player_type_id))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program(ShellMessage::SelectPlaylistPlayer(
                                    player_type_id.clone(),
                                ));
                            })
                    })
                    .collect::<Vec<_>>(),
            );
            let playlist_creator = if is_playlists {
                Some(widget(Stack::fill_column(6.0)).children((
                    widget(TextInput::new(new_playlist_name).label("新建播放列表")).on_cx(
                        |_, event: &TextChanged, cx| {
                            cx.dispatch_program(ShellMessage::NewPlaylistNameChanged(
                                event.value.to_string(),
                            ));
                        },
                    ),
                    playlist_player_choices,
                    button("创建播放列表")
                        .key("create-playlist")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::CreatePlaylist)),
                )))
            } else {
                None
            };
            let playlist_add_current_directory = if is_playlists {
                view_model.selected_playlist_id.clone().map(|playlist_id| {
                    let path = view_model.current_directory.clone();
                    button("添加当前目录").key("add-playlist-current-directory").on_cx(
                        move |_, _: &Activate, cx| {
                            cx.dispatch_program(ShellMessage::AddPlaylistItemsByPaths {
                                playlist_id: playlist_id.clone(),
                                paths: vec![path.clone()],
                            });
                        },
                    )
                })
            } else {
                None
            };
            let settings_editor = if matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError) {
                let cache_limit = view_model.settings_cache_limit_draft.clone();
                let player_id = view_model
                    .settings
                    .default_playlist_player_type_id
                    .clone()
                    .unwrap_or_default();
                let theme = view_model.settings.theme.clone();
                let close_behavior = view_model.settings.close_behavior.clone();
                Some(widget(Stack::column(12.0)).children((
                    super::workbench::section_card("外观", vec![widget(Stack::bar(8.0)).children((
                        text(format!("主题：{theme}")).key("settings-theme-label"),
                        widget(Stack::spacer()),
                        button("浅色").key("settings-theme-light").on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(ShellMessage::SettingsThemeChanged("light".into()));
                        }),
                        button("深色").key("settings-theme-dark").on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(ShellMessage::SettingsThemeChanged("dark".into()));
                        }),
                        button("跟随系统").key("settings-theme-system").on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(ShellMessage::SettingsThemeChanged("system".into()));
                        }),
                    )).into_any()]),
                    super::workbench::section_card("播放与缓存", vec![
                        widget(TextInput::new(cache_limit).label("缩略图缓存上限（MB）")).on_cx(
                            |_, event: &TextChanged, cx| {
                                cx.dispatch_program(ShellMessage::SettingsCacheLimitChanged(event.value.to_string()));
                            },
                        ).into_any(),
                        widget(TextInput::new(player_id).label("默认播放器类型")).on_cx(
                            |_, event: &TextChanged, cx| {
                                cx.dispatch_program(ShellMessage::SettingsPlayerChanged(event.value.to_string()));
                            },
                        ).into_any(),
                    ]),
                    super::workbench::section_card("关闭行为", vec![widget(Stack::bar(8.0)).children((
                        text(format!("关闭行为：{close_behavior}")).key("settings-close-label"),
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
                    )).into_any()]),
                    view_model
                        .settings_error
                        .clone()
                        .map(|error| text(format!("设置提示：{error}")).key("settings-error")),
                    button("保存应用设置").key("save-application-settings").on_cx(
                        |_, _: &Activate, cx| cx.dispatch_program(ShellMessage::SaveSettings),
                    ),
                )))
            } else {
                None
            };
            let preview_slot = if view_model.preview_pixels.is_some() {
                "file-preview"
            } else {
                ""
            };
            let preview_node = if !view_model.acceptance_scene && view_model.inspect_surface_visible() {
                Some(super::inspect_view::inspect_surface(&view_model))
            } else if matches!(view_model.page, ShellPage::SelectedFile) {
                Some(widget(Stack::column(8.0)).children((
                    text("选择图片文件后，预览将在原生纹理节点中显示").key("preview-placeholder"),
                    widget(GpuTextureView::new(preview_slot).contain()).key("file-preview"),
                )).into_any())
            } else {
                None
            };
            let page_actions = if matches!(view_model.page, ShellPage::Loading) {
                None
            } else {
                Some(widget(Stack::fill_row(8.0)).children((
                    button(view_model.page.primary_action())
                        .key("primary-action")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::PrimaryAction)),
                    button(view_model.edit_label())
                        .key("edit-action")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(ShellMessage::EditAction)),
                )))
            };
            let file_browser = if !view_model.acceptance_scene && view_model.files_surface_visible() {
                super::files_view::files_surface(&view_model)
            } else {
                file_actions.into_any()
            };
            let admin_surface = if !view_model.acceptance_scene {
                Some(super::admin::admin_surface(&view_model))
            } else {
                None
            };
            let close_prompt = super::input::close_prompt(&view_model);
            let process = widget(
                Stack::fill_column(8.0)
                    .width(LengthSpec::Px(240.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .padding_xy(16.0, 20.0),
            )
            .children((
                text("当前状态").key("process-heading"),
                text(view_model.page.status()).key("process-status"),
                button("刷新状态").key("refresh").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Refresh);
                }),
            ));
            let live_files = !view_model.acceptance_scene
                && view_model.files_surface_visible()
                && !matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError);
            let show_page = view_model.acceptance_scene
                || matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError)
                || matches!(view_model.workspace.main_region(), MainRegion::HasRepository);
            let region = view_model.workspace.main_region();
            let primary = if live_files {
                drop((
                    status_summary, task_actions, task_progress, file_browser, plugin_actions,
                    plugin_config_actions, plugin_config_editors, playlist_actions, settings_editor,
                    admin_surface, close_prompt, playlist_item_actions, player_surface,
                    playlist_item_status, playlist_editor, playlist_creator,
                    playlist_add_current_directory, log_actions, preview_node, page_actions,
                ));
                super::files_view::live_file_column(&view_model)
            } else if !view_model.acceptance_scene
                && show_page
                && !matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError)
                && is_playlists
            {
                let mut body = Vec::new();
                if let Some(player) = player_surface {
                    body.push(player);
                }
                if view_model.selected_playlist_id.is_some() {
                    if let Some(editor) = playlist_editor {
                        body.push(editor.into_any());
                    }
                    if let Some(add) = playlist_add_current_directory {
                        body.push(add.into_any());
                    }
                }
                let _ = playlist_creator;
                super::workbench::page(body)
            } else if !view_model.acceptance_scene
                && show_page
                && !matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError)
                && matches!(
                    view_model.workspace.panel,
                    WorkspacePanel::Logs | WorkspacePanel::Extensions | WorkspacePanel::Actions
                )
            {
                super::workbench::page(vec![admin_surface.unwrap_or_else(|| widget(Stack::column(0.0)).into_any())])
            } else if !view_model.acceptance_scene
                && show_page
                && !matches!(view_model.page, ShellPage::Settings | ShellPage::SettingsError)
                && view_model.workspace.panel == WorkspacePanel::Search
            {
                super::workbench::page(vec![preview_node.unwrap_or_else(|| widget(Stack::column(0.0)).into_any())])
            } else if !view_model.acceptance_scene && show_page {
                let (eyebrow, title) = section_heading(&view_model);
                let mut body = Vec::new();
                if let Some(editor) = settings_editor {
                    body.push(editor.into_any());
                }
                if let Some(admin) = admin_surface {
                    body.push(admin);
                }
                if is_playlists {
                    body.push(playlist_actions.into_any());
                    if let Some(editor) = playlist_editor {
                        body.push(editor.into_any());
                    }
                    if let Some(creator) = playlist_creator {
                        body.push(creator.into_any());
                    }
                    if let Some(add) = playlist_add_current_directory {
                        body.push(add.into_any());
                    }
                    if let Some(items) = playlist_item_actions {
                        body.push(items.into_any());
                    }
                    if let Some(status) = playlist_item_status {
                        body.push(status.into_any());
                    }
                    if let Some(player) = player_surface {
                        body.push(player);
                    }
                }
                if view_model.workspace.panel == WorkspacePanel::Search {
                    if let Some(preview) = preview_node {
                        body.push(preview);
                    }
                }
                if let Some(logs) = log_actions {
                    if matches!(view_model.workspace.panel, WorkspacePanel::Logs) {
                        body.push(logs.into_any());
                    }
                }
                framed_page(eyebrow, title, body)
            } else if show_page {
                let workspace_actions = widget(Stack::fill_column(8.0)).children((
                    widget(Stack::fill_column(8.0)).children((
                        status_summary,
                        task_actions,
                        task_progress,
                        file_browser,
                        plugin_actions,
                        plugin_config_actions,
                        plugin_config_editors,
                        playlist_actions,
                    )),
                    widget(Stack::fill_column(8.0)).children((
                        settings_editor,
                        admin_surface,
                        close_prompt,
                        playlist_item_actions,
                        player_surface,
                        playlist_item_status,
                        playlist_editor,
                        playlist_creator,
                        playlist_add_current_directory,
                        log_actions,
                        preview_node,
                    )),
                ));
                widget(
                    Stack::fill_column(12.0)
                        .padding_xy(24.0, 20.0)
                        .min_width(LengthSpec::Px(0.0)),
                )
                .children((
                    workspace_actions,
                    widget(List::new().label(view_model.page.title()).style(Stack::column(12.0).node_style()))
                        .children((page_actions,)),
                ))
                .into_any()
            } else {
                drop((
                    status_summary, task_actions, task_progress, file_browser, plugin_actions,
                    plugin_config_actions, plugin_config_editors, playlist_actions, settings_editor,
                    admin_surface, close_prompt, playlist_item_actions, player_surface,
                    playlist_item_status, playlist_editor, playlist_creator,
                    playlist_add_current_directory, log_actions, preview_node, page_actions,
                ));
                match region {
                    MainRegion::MissingRepository => missing_repository_panel(&view_model).into_any(),
                    MainRegion::EmptyRepository => empty_repository_panel(&view_model).into_any(),
                    MainRegion::Startup | MainRegion::LoadError | MainRegion::HasRepository => {
                        startup_panel(&view_model).into_any()
                    }
                }
            };
            let stage = if view_model.acceptance_scene && show_page {
                widget(Stack::fill_row(0.0).min_height(LengthSpec::Px(0.0)))
                    .children((primary, process))
                    .key("workspace-body")
                    .into_any()
            } else {
                widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)))
                    .children((primary,))
                    .key("workspace-body")
                    .into_any()
            };
            let presented_sidebar = view_model.motion.sidebar_presented_width();
            let show_sidebar = presented_sidebar > 0.5
                && (view_model.acceptance_scene || view_model.workspace.startup.status == StartupStatus::Ready);
            let body = if show_sidebar {
                let sidebar = if view_model.acceptance_scene {
                    widget(SidebarFrame::new()).body(legacy_navigation).into_any()
                } else {
                    widget(super::sidebar_view::sidebar_frame())
                        .top(super::sidebar_view::sidebar_switcher(&view_model))
                        .body(super::sidebar_view::sidebar_sections(&view_model))
                        .footer(super::sidebar_view::sidebar_footer(&view_model))
                        .into_any()
                };
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
            }
            shell
        })?;
    super::title_bar::bind_window_controls(document)?;
    crate::window_host::bind_escape(document);
    SHELL_MOUNT.with(|slot| *slot.borrow_mut() = Some(mounted));
    Ok(())
}

/// 资源区加主区。分割是工作台的发丝间隙，主区圆角，不画常驻浅色分割条。
fn workbench(sidebar: AnyView, stage: AnyView, width: f32) -> AnyView {
    let layout = WorkspaceLayout::new([
        RegionState::new(RegionId::Resources, RegionRole::Resources)
            .size(width)
            .min_size(SIDEBAR_MIN_PX)
            .max_size(SIDEBAR_MAX_PX)
            .collapsible(true)
            .resizable(true),
        RegionState::new(RegionId::Primary, RegionRole::Primary)
            .min_size(320.0)
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

fn empty_repository_panel(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let error = model.workspace.startup.error.clone().or_else(|| {
        (!model.workspace.missing_error.is_empty()).then(|| model.workspace.missing_error.clone())
    });
    let card = widget(Stack::column(10.0).width(LengthSpec::Px(520.0)).align(AlignSpec::Center)).children((
        text("还没有可用资源库").key("empty-title"),
        text("拖入一个本地文件夹创建资源库。").key("empty-detail"),
        error.map(|error| text(error).key("empty-error")),
    ));
    widget(
        Stack::fill_column(0.0)
            .padding_xy(24.0, 24.0)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center),
    )
    .children((card,))
}

/// 设置和其余实况页的页头：标题在左，卡片在内容区。
fn framed_page(eyebrow: impl Into<String>, title: impl Into<String>, body: Vec<AnyView>) -> AnyView {
    let eyebrow = eyebrow.into();
    let title = title.into();
    widget(Stack::fill_column(16.0).padding_xy(20.0, 18.0).min_height(LengthSpec::Px(0.0)))
        .children((
            widget(Stack::bar(16.0)).children((
                widget(Stack::column(4.0)).children((
                    text(eyebrow).key("section-eyebrow"),
                    text(title).key("section-title"),
                )),
                widget(Stack::spacer()),
            )),
            widget(Stack::column(12.0)).children(body),
        ))
        .into_any()
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
                button("取消").key("playlist-dialog-cancel").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::ClosePlaylistDialog);
                }),
                button("创建").key("create-playlist").disabled(blocked).on_cx(|_, _: &Activate, cx| {
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
        widget(Stack::column(4.0)).children((
            button(mode.label()).disabled(deleting || !enabled).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingConfirmDelete(mode));
            }),
            text(model.workspace.delete_mode_detail(mode)),
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
            .footer(button("取消").key("delete-dialog-cancel").disabled(deleting).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::MissingCloseDelete);
            })),
    )
}

/// 显示名仅用于标签，服务请求始终使用 DTO 中的完整仓库相对路径。
fn entry_message(entry: &FileBrowserEntry) -> ShellMessage {
    if entry.kind == "directory" {
        ShellMessage::OpenDirectory(entry.path.clone())
    } else {
        ShellMessage::SelectFile { path: entry.path.clone(), asset_id: entry.asset_id.clone() }
    }
}

