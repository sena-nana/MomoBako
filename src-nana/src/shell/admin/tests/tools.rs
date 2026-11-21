//! 仓库动作、工具页、内置导入页、API 调试台、插件调用与 ASMR 快捷筛选等工具区的测试。

use super::*;

#[test]
fn repository_actions_follow_selection_and_refuse_when_not_runnable() {
    let mut model = ShellViewModel::default();
    model.repository_id = Some("repo".into());
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    assert!(model.admin.actions_loading);
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::LoadActions { repo_id }) if repo_id == "repo")
    );
    model.repository_id = Some("other".into());
    send(
        &mut model,
        AdminMessage::ActionsLoaded {
            repo_id: "repo".into(),
            result: Ok(vec![action("import", "ready", true)]),
        },
    );
    assert!(model.admin.actions.is_empty());
    model.repository_id = Some("repo".into());
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files));
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    let _ = model.admin.take_effects();
    send(
        &mut model,
        AdminMessage::ActionsLoaded {
            repo_id: "repo".into(),
            result: Ok(vec![
                action("import", "blocked", false),
                action("copy", "ready", true),
            ]),
        },
    );
    assert_eq!(model.admin.active_action_id.as_deref(), Some("import"));
    assert_eq!(support::action_status_label("blocked", false), "不支持");
    assert!(!action_can_run("ready", true, 0, false));
    send(&mut model, AdminMessage::RunAction(Some("copy".into())));
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        AdminMessage::ActionsLoaded {
            repo_id: "repo".into(),
            result: Err("读取失败".into()),
        },
    );
    assert_eq!(model.admin.actions.len(), 2);
    assert_eq!(model.admin.actions_error, "读取失败");
    model.files.set_selected_paths(vec!["a.txt".into()]);
    send(&mut model, AdminMessage::SelectAction("copy".into()));
    send(&mut model, AdminMessage::RunAction(None));
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::RunAction { action_id, paths, .. }) if action_id == "copy" && paths == ["a.txt"]
    ));
    // 执行完重读摘要和当前目录，任务弹层出「读取目录」。
    model.workspace.present_repository(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    });
    send(
        &mut model,
        AdminMessage::ActionRunFinished {
            result: Ok(action("copy", "ready", true)),
        },
    );
    assert!(!model.admin.actions_running);
    assert!(model.files.take_effects().iter().any(|effect| matches!(effect, crate::shell::FilesEffect::Browse { repo_id, .. } if repo_id == "repo")));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, crate::shell::WorkspaceEffect::LoadSnapshotSilent { repo_id } if repo_id == "repo")));
    assert_eq!(
        model.operation.shown().map(|row| row.label.as_str()),
        Some("读取目录")
    );
}

#[test]
fn tool_pages_follow_enabled_manifests_and_keep_the_selection() {
    let mut native = tool_plugin(
        "backend.tool",
        json!([{ "toolPageId": "backend.page", "label": "后端" }]),
    );
    native.runtime = "native-dylib".into();
    let mut disabled = tool_plugin(
        "disabled.tool",
        json!([{ "toolPageId": "disabled.page", "label": "停用" }]),
    );
    disabled.enabled = false;
    let mut broken = tool_plugin(
        "broken.tool",
        json!([{ "toolPageId": "broken.page", "label": "出错" }]),
    );
    broken.status = "error".into();
    let pages = support::tool_pages_from_plugins(&[
        tool_plugin(
            "a.tool",
            json!([{ "toolPageId": "zeta", "label": "Zeta", "order": 5 }, { "toolPageId": "  ", "label": "空" }]),
        ),
        tool_plugin(
            "b.tool",
            json!([{ "toolPageId": "momobako.tool.api-playground", "label": "API Playground", "description": "调试" }]),
        ),
        tool_plugin(
            "c.tool",
            json!([{ "toolPageId": "alpha", "label": "Alpha", "order": 5 }]),
        ),
        native,
        disabled,
        broken,
    ]);
    assert_eq!(
        pages
            .iter()
            .map(|page| page.id.as_str())
            .collect::<Vec<_>>(),
        [
            "alpha",
            "zeta",
            "momobako.tool.api-playground",
            "backend.page"
        ]
    );
    assert_eq!(
        (
            pages[2].native,
            pages[2].plugin_name.as_str(),
            pages[2].description.as_str()
        ),
        (true, "b.tool", "调试")
    );
    assert!(!pages[0].native);

    let mut model = ShellViewModel::default();
    send(&mut model, AdminMessage::SetToolPages(Vec::new()));
    assert!(model.admin.active_tool_page_id.is_none());
    let entry = |id: &str| ToolPageEntry {
        id: id.into(),
        label: id.into(),
        plugin_name: "tool".into(),
        description: String::new(),
        native: false,
    };
    send(
        &mut model,
        AdminMessage::SetToolPages(vec![entry("old"), entry("new")]),
    );
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("old"));
    send(&mut model, AdminMessage::SelectToolPage("missing".into()));
    send(&mut model, AdminMessage::SelectToolPage("new".into()));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
    send(&mut model, AdminMessage::SetToolPages(vec![entry("new")]));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
}

#[test]
fn builtin_import_pages_follow_the_file_machine() {
    let mut model = ShellViewModel::default();
    let buttons = tool_native::import_buttons(support::TOOL_FILE_MANAGER);
    let blocked = ImportView::project(&model, support::TOOL_FILE_MANAGER);
    assert!(!blocked.enabled);
    assert_eq!(blocked.reason, Some("当前没有可用仓库。"));
    assert_eq!(
        tool_native::import_block_reason(&model),
        Some("当前没有可用仓库。")
    );
    assert!(
        tool_native::import_message(blocked.enabled, import_button(&buttons, "folder")).is_none()
    );

    attach_repository(&mut model, vec!["write".into()]);
    let open = ImportView::project(&model, support::TOOL_FILE_MANAGER);
    assert!(open.enabled);
    assert!(tool_native::import_block_reason(&model).is_none());
    let folder = import_button(&buttons, "folder");
    assert!(matches!(
        &folder,
        FilesMessage::OpenDialog(FileDialog::Import)
    ));
    model.reduce(tool_native::import_message(open.enabled, folder).expect("folder"));
    assert_eq!(
        model.files.dialog,
        FileDialog::Closed,
        "实况导入打开系统选择器，不打开手填路径框"
    );
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(
        matches!(commands.as_slice(), [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }] if request.title.as_deref() == Some("选择导入文件夹")),
        "实况文件夹导入应排队系统选目录：{commands:?}"
    );

    let unselected = ImportView::project(&model, support::TOOL_EAGLE_IMPORTER);
    assert!(unselected.enabled);
    assert_eq!(
        tool_native::eagle_path_label(&unselected.library_path),
        "尚未选择"
    );
    assert!(tool_native::eagle_import_message(&unselected, "copy").is_none());
    model.admin.eagle_library_path = "D:/eagle.library".into();
    let selected = ImportView::project(&model, support::TOOL_EAGLE_IMPORTER);
    assert_eq!(
        tool_native::eagle_path_label(&selected.library_path),
        "D:/eagle.library"
    );
    assert!(matches!(
        tool_native::eagle_import_message(&selected, "copy"),
        Some(ShellMessage::Admin(AdminMessage::RunEagleImport(mode))) if mode == "copy"
    ));

    model.workspace.repositories[0].capabilities.clear();
    assert_eq!(
        tool_native::import_block_reason(&model),
        Some("当前仓库处于只读状态。")
    );
    model.workspace.repositories[0].capabilities = vec!["write".into()];
    model.workspace.panel = WorkspacePanel::Trash;
    assert_eq!(
        tool_native::import_block_reason(&model),
        Some("回收站视图不支持导入。")
    );
    model.workspace.panel = WorkspacePanel::SmartFolder;
    assert_eq!(
        tool_native::import_block_reason(&model),
        Some("虚拟视图不支持导入。")
    );
}

#[test]
fn api_playground_falls_back_and_sends_http_and_plugin_calls() {
    let mut model = ShellViewModel::default();
    model.admin.external = Some(connection());
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::SetKeyword("repositories".into())),
    );
    let selected = super::super::api::selected_ref(&model).expect("兜底端点");
    assert_eq!(selected.path, "/external/v1/repositories");
    assert!(model.admin.api.include_auth);
    assert_eq!(
        super::super::api::request_url(&model),
        "http://127.0.0.1:9/external/v1/repositories"
    );
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    match model.admin.take_effects().pop() {
        Some(AdminEffect::HttpRequest(request)) => {
            assert_eq!(
                (request.method.as_str(), request.body.as_deref()),
                ("GET", None)
            );
            assert!(
                request
                    .headers
                    .contains(&("Authorization".into(), "Bearer 1234567890abcdef".into()))
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(model.admin.api.sending);
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::HttpFinished(Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: "{\"items\":[]}".into(),
        }))),
    );
    assert_eq!(model.admin.api.response_status, "200 OK");
    assert!(model.admin.api.response_headers.contains("content-type"));
    assert!(model.admin.api.response_body.contains("\"items\": []"));
    assert!(!model.admin.api.sending);

    send(&mut model, AdminMessage::Api(ApiMessage::Copy));
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::CopyText(text)) if text.starts_with("curl -X GET 'http://127.0.0.1:9/external/v1/repositories'"))
    );
    assert_eq!(model.admin.api.notice, "请求已复制。");

    send(
        &mut model,
        AdminMessage::Api(ApiMessage::SetKeyword(String::new())),
    );
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::SelectEndpoint(
            "external-http:POST:/external/v1/assets:add".into(),
        )),
    );
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::SetRequestText("{".into())),
    );
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    assert!(model.admin.take_effects().is_empty());
    assert_eq!(model.admin.api.response_status, "ERROR");
    assert!(model.admin.api.error.starts_with("请求 JSON 无效"));

    model.admin.api_design = Some(ApiDesignSnapshot {
        transport: "宿主进程内调用；外部客户端走本机回环 HTTP（/external/v1）".into(),
        endpoints: vec![crate::backend::services::repository::ApiDefinition {
            group: "插件".into(),
            transport: String::new(),
            method: String::new(),
            path: String::new(),
            summary: "状态".into(),
            plugin_id: Some("momobako.service.downloader".into()),
            plugin_method: Some("downloader.getRuntimeStatus".into()),
            requires_auth: None,
            request_template: None,
        }],
    });
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::SelectEndpoint(
            "plugin-call:momobako.service.downloader:downloader.getRuntimeStatus".into(),
        )),
    );
    assert_eq!(model.admin.api.request_text, "{}");
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::CallPlugin { method, origin: PluginCallOrigin::Playground, .. }) if method == "downloader.getRuntimeStatus"
    ));
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::PluginFinished(Ok(
            json!({ "payload": { "running": true } }),
        ))),
    );
    assert_eq!(model.admin.api.response_status, "OK");
    assert!(
        model
            .admin
            .api
            .response_headers
            .contains("downloader.getRuntimeStatus")
    );
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    send(
        &mut model,
        AdminMessage::Api(ApiMessage::PluginFinished(Err("插件未启用".into()))),
    );
    assert_eq!(
        (
            model.admin.api.response_status.as_str(),
            model.admin.api.error.as_str()
        ),
        ("ERROR", "插件未启用")
    );
}

#[test]
fn file_plugin_call_keeps_repository_and_writes_activity() {
    let mut model = ShellViewModel::default();
    send(
        &mut model,
        AdminMessage::CallFilePlugin {
            plugin_id: "momobako.netease.source".into(),
            method: "media.clearTrackCache".into(),
            payload: json!({"songId": 9}),
            repository_id: Some("repo".into()),
        },
    );
    match model.admin.take_effects().pop() {
        Some(AdminEffect::CallPlugin {
            method,
            repository_id,
            origin,
            ..
        }) => {
            assert_eq!(method, "media.clearTrackCache");
            assert_eq!(repository_id.as_deref(), Some("repo"));
            assert_eq!(origin, PluginCallOrigin::FileMenu);
        }
        other => panic!("unexpected {other:?}"),
    }
    // 来源动作完成后重读资源库列表。
    model.workspace.take_effects();
    send(
        &mut model,
        AdminMessage::FilePluginFinished {
            method: "media.clearTrackCache".into(),
            result: Ok(json!({})),
        },
    );
    assert!(matches!(
        model.workspace.take_effects().as_slice(),
        [crate::shell::WorkspaceEffect::RefreshRepositories { .. }]
    ));
    send(
        &mut model,
        AdminMessage::FilePluginFinished {
            method: "media.clearTrackCache".into(),
            result: Err(String::new()),
        },
    );
    assert_eq!(model.files.error, "media.clearTrackCache 调用失败。");
}

/// 「创建播放列表」在本地建播放集，不调用插件；建好以后刷新资源库，
/// 缺仓库或缺歌单标识时只报错，失败写进文件列表的错误。
#[test]
fn source_playlist_is_created_locally_then_the_repository_refreshes() {
    let playlist = |id: Option<&str>| super::super::super::entry_actions::SourcePlaylist {
        playlist_id: id.map(str::to_string),
        name: "夜曲".into(),
        player_type_id: "momobako.playlist.audio-sequence".into(),
        path: "playlists/night".into(),
    };
    let mut model = ShellViewModel::default();
    send(
        &mut model,
        AdminMessage::CreateSourcePlaylist {
            playlist: playlist(Some("netease-3")),
            repository_id: Some("repo".into()),
        },
    );
    match model.admin.take_effects().as_slice() {
        [
            AdminEffect::CreateSourcePlaylist {
                repo_id,
                playlist_id,
                name,
                player_type_id,
                path,
            },
        ] => {
            assert_eq!(
                (repo_id.as_str(), playlist_id.as_str(), name.as_str()),
                ("repo", "netease-3", "夜曲")
            );
            assert_eq!(
                (player_type_id.as_str(), path.as_str()),
                ("momobako.playlist.audio-sequence", "playlists/night")
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    model.workspace.take_effects();
    send(&mut model, AdminMessage::SourcePlaylistFinished(Ok(())));
    assert!(matches!(
        model.workspace.take_effects().as_slice(),
        [crate::shell::WorkspaceEffect::RefreshRepositories { .. }]
    ));
    send(
        &mut model,
        AdminMessage::SourcePlaylistFinished(Err("播放集已存在".into())),
    );
    assert_eq!(model.files.error, "播放集已存在");

    send(
        &mut model,
        AdminMessage::CreateSourcePlaylist {
            playlist: playlist(None),
            repository_id: Some("repo".into()),
        },
    );
    assert_eq!(model.files.error, "来源目录缺少播放列表标识");
    send(
        &mut model,
        AdminMessage::CreateSourcePlaylist {
            playlist: playlist(Some("netease-3")),
            repository_id: None,
        },
    );
    assert_eq!(model.files.error, "没有当前资源库，不能创建播放列表。");
    assert!(
        model.admin.take_effects().is_empty(),
        "缺仓库或缺歌单标识都不建"
    );
}

#[test]
fn slashes_and_repeated_ids_in_external_data_still_mount() {
    assert_eq!(super::super::style::key_part("a/b%c"), "a%2Fb%25c");
    let mut model = ShellViewModel::for_page(super::super::super::ShellPage::PluginSettings);
    let mut odd = plugin("user/tool", "user", "service", "service");
    odd.contributes = json!({
        "settings": { "fields": [
            { "key": "path/root", "label": "根目录" },
            { "key": "path/root", "label": "重复的根目录" }
        ] },
        "toolPages": [{ "toolPageId": "user/page", "label": "工具" }]
    });
    load_bundle(&mut model, vec![odd.clone(), odd]);
    send(&mut model, AdminMessage::ToggleSettings("user/tool".into()));
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records: vec![log_record(
            "log/1",
            "2026-01-01T00:00:00Z",
            "info",
            "host",
            "",
            "",
            "斜杠日志",
        )],
        next_cursor: None,
    })));
    send(&mut model, AdminMessage::ToggleLogContext("log/1".into()));
    if let Err(error) = crate::acceptance_document_for_model(model.clone()) {
        panic!("拓展页的插件、字段和工具页标识带斜杠时挂载失败：{error:?}");
    }
    model.workspace.panel = WorkspacePanel::Logs;
    if let Err(error) = crate::acceptance_document_for_model(model) {
        panic!("日志标识带斜杠时挂载失败：{error:?}");
    }
}

#[test]
fn asmr_shortcuts_publish_onto_the_filter_bar() {
    let mut model = ShellViewModel::default();
    let mut manifest = plugin("momobako.library.asmr", "builtin", "library-kind", "asmr");
    manifest.contributes = json!({
        "libraryExtension": { "libraryKind": "asmr", "searchShortcuts": ["works", "missing"] }
    });
    load_bundle(&mut model, vec![manifest]);
    assert_eq!(model.inspect.shortcuts.len(), 1);
    assert_eq!(model.inspect.shortcuts[0].label, "ASMR 作品");
    assert_eq!(model.inspect.shortcuts[0].metadata, "libraryKind=asmr");
}

/// 选中的勾选框（API 调试页的鉴权勾选框用 NanaUI 默认配色）：「✓」用主题勾选框配方的选中字色，
/// 选中底要和它拉开对比，勾上了才看得出来；选中底是 `accent-soft` 时白色的勾在浅色主题下看不见。
#[test]
fn a_checked_checkbox_keeps_its_mark_visible() {
    use nana_ui::theme::{ComponentRecipeId, ThemeAppearance, builtin_theme_arc};

    fn luminance(color: [f32; 4]) -> f32 {
        let channel = |value: f32| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
    }
    let checkbox = nana_ui::runtime::Checkbox::new("Authorization: Bearer token", true);
    let fill_role = checkbox
        .style
        .interaction
        .selected
        .background
        .expect("选中底色");
    for mode in [ThemeAppearance::Light, ThemeAppearance::Dark] {
        let theme = builtin_theme_arc(mode);
        let colors = theme.style_model();
        let fill = colors.color(fill_role).as_rgba_array();
        let mark = colors
            .color(
                theme
                    .recipes()
                    .foreground(ComponentRecipeId::Checkbox, true),
            )
            .as_rgba_array();
        assert!(
            fill[3] >= 1.0,
            "{mode:?}：选中底要不透明，勾的对比才不随下面的底变"
        );
        let (light, dark) = (
            luminance(fill).max(luminance(mark)),
            luminance(fill).min(luminance(mark)),
        );
        let contrast = (light + 0.05) / (dark + 0.05);
        assert!(
            contrast >= 3.0,
            "{mode:?}：选中勾选框的「✓」和底色对比只有 {contrast:.2}"
        );
    }
}

/// 缓存卡片读当前资源库的真实占用：缩略图、其余缓存文件和总占用三格，逐项写文件数和大小。
/// 没有快照或快照属于别的资源库时三格写「未加载」。
#[test]
fn cache_card_shows_the_active_repository_usage() {
    use super::super::settings_state::{SettingsView, human_bytes};

    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo-a".into());
    let view = SettingsView::project(&model);
    assert_eq!(view.cache.metrics, ["未加载", "未加载", "未加载"]);

    send(&mut model, AdminMessage::CacheLoaded(Ok(cache("repo-a"))));
    let view = SettingsView::project(&model);
    assert_eq!(view.cache.metrics, ["3", "2", "1.5 MB"]);
    let rows: Vec<_> = view
        .cache
        .entries
        .iter()
        .map(|row| (row.name.as_str(), row.value.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("缩略图", "3 个文件 · 2.0 KB"),
            ("Office 预览 PDF", "2 个文件 · 1.5 MB"),
        ]
    );

    model.workspace.active_repo_id = Some("repo-b".into());
    assert_eq!(
        SettingsView::project(&model).cache.metrics,
        ["未加载", "未加载", "未加载"],
        "别的资源库的快照不显示"
    );
    assert_eq!(human_bytes(512), "512 B");
    assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
}
