//! 预览、工具页和来源登录的视觉与点击验收。
//!
//! 用生产文档渲染，点真实控件，再看无障碍树和像素。空白图或点了没变化都算失败。

use std::fs;
use std::path::PathBuf;

use momobako_nana::backend::services::repository::{
    AssetDetail, AssetSummary, PlaybackSessionState, PluginConfigSnapshot, PluginManifest, SystemLogLocation, SystemLogPage,
    SystemLogRecord, SystemLogSource,
};
use momobako_nana::shell::{
    commit_interaction, mount_shell, AdminMessage, InspectEffect, InspectMessage, ShellMessage, ShellViewModel,
    SourceStep, ToolPageEntry, WorkspacePanel, WorkspaceRepository,
};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession, protocol::ThemeName};
use nana_ui_devtools::offscreen;
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::WindowId;

#[test]
fn preview_tool_and_login_surfaces_paint_and_respond() {
    assert!(offscreen::pixels_available(), "界面验收读不到像素，不能当作通过");
    assert_wav_preview();
    assert_import_tool();
    assert_source_login();
    assert_logs_follow();
}

fn assert_wav_preview() {
    for (width, height, theme, name) in [
        (1200u32, 800u32, ThemeName::Light, "wav-preview-light"),
        (1200, 800, ThemeName::Dark, "wav-preview-dark"),
        (390, 844, ThemeName::Light, "wav-preview-narrow"),
    ] {
        let mut model = wav_preview();
        let mut session = open(&model, width, height, theme);
        let before = session.accessibility_dump();
        assert!(has(&before, "播放"), "{name} 缺少播放按钮");
        assert!(has(&before, "audio/tone.wav"), "{name} 缺少预览路径");
        let play = before.iter().find(|node| node.label.as_deref() == Some("播放")).expect("播放");
        assert!(play.bounds.width > 8.0 && play.bounds.height > 8.0, "{name} 播放按钮没有尺寸");
        let stats = shot(&mut session, name);
        assert!(stats.nonclear_ratio > 0.01, "{name} 画面几乎是空的");
        if name == "wav-preview-narrow" {
            assert_preview_owns_the_page(&before);
        }
        if name != "wav-preview-light" {
            continue;
        }
        click(&mut session, "播放");
        let _ = pump(&mut session, &mut model);
        assert!(has(&session.accessibility_dump(), "暂停"), "播放后没有变成暂停");
        let after = shot(&mut session, "wav-preview-playing");
        assert!(after.nonclear_ratio > 0.01, "播放后的画面几乎是空的");
    }
}

fn assert_import_tool() {
    let mut model = import_tool();
    let mut session = open(&model, 1200, 800, ThemeName::Light);
    assert!(has(&session.accessibility_dump(), "从文件夹导入"), "缺少文件夹导入");
    assert!(has(&session.accessibility_dump(), "从 ZIP 导入"), "缺少 ZIP 导入");
    let stats = shot(&mut session, "import-tool-light");
    assert!(stats.nonclear_ratio > 0.01, "导入工具页几乎是空的");
    click(&mut session, "从文件夹导入");
    let _ = pump(&mut session, &mut model);
    assert_eq!(model.workspace.panel, WorkspacePanel::Files, "导入没有回到文件页");
    assert!(
        has(&session.accessibility_dump(), "多个路径用分号分隔"),
        "点击导入没有打开路径对话框"
    );
    let _ = shot(&mut session, "import-tool-dialog");
}

/// 来源账号区照 `SourceAuthenticationSettings.vue`：点「连接新账号」先建扫码会话，
/// 会话回来后出现二维码、提示和「检查登录结果」。
fn assert_source_login() {
    let mut model = login_settings();
    let mut session = open(&model, 1200, 800, ThemeName::Light);
    for label in ["账号与仓库", "认证由 Source 插件处理，宿主只保存安全凭据引用。", "连接新账号"] {
        assert!(has(&session.accessibility_dump(), label), "缺少 {label}");
    }
    let stats = shot(&mut session, "source-login-light");
    assert!(stats.nonclear_ratio > 0.01, "登录设置页几乎是空的");
    click(&mut session, "连接新账号");
    let _ = pump(&mut session, &mut model);
    let calls = format!("{:?}", model.admin.take_effects());
    assert!(calls.contains("auth.createQrSession"), "点击连接新账号没有建扫码会话：{calls}");
    model.reduce(ShellMessage::Admin(AdminMessage::SourceStepFinished {
        step: SourceStep::CreateSession,
        result: Ok(serde_json::json!({ "unikey": "key-1", "qrurl": "https://music.163.com/login?codekey=key-1" })),
    }));
    mount_shell(session.document_mut(), &model).expect("重建");
    session.flush().expect("重新布局");
    let nodes = session.accessibility_dump();
    for label in ["请扫码并在手机端确认，然后检查登录结果。", "扫码登录二维码", "刷新二维码", "检查登录结果"] {
        assert!(has(&nodes, label), "扫码会话缺少 {label}");
    }
    let after = shot(&mut session, "source-login-session");
    assert!(after.nonclear_ratio > 0.01, "扫码会话画面几乎是空的");
}

/// 日志面板的页头和筛选固定，日志列表自己滚动：追踪时列表停在末尾，最后一条在列表里、第一条滚出去了；
/// 暂停后列表停在顶部。两种情况页头都留在视口里。
fn assert_logs_follow() {
    const SUBLINE: &str = "统一查看宿主、插件与辅助进程的实时日志流。";
    let mut model = ready_library();
    model.workspace.panel = WorkspacePanel::Logs;
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage { records: (0..30).map(log_record).collect(), next_cursor: None })));
    let mut session = open(&model, 1200, 800, ThemeName::Light);
    session.flush().expect("跟随后的布局");
    let nodes = session.accessibility_dump();
    assert!(visible(&nodes, SUBLINE, 0.0, 800.0), "追踪时页头被滚走了");
    let top = list_top(&nodes);
    assert!(visible(&nodes, "第 29 条日志", top, 800.0), "追踪时最后一条日志不在列表里");
    assert!(!visible(&nodes, "第 0 条日志", top, 800.0), "追踪时列表没有滚到末尾");
    let _ = shot(&mut session, "logs-follow");

    let mut paused = ready_library();
    paused.workspace.panel = WorkspacePanel::Logs;
    paused.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage { records: (0..30).map(log_record).collect(), next_cursor: None })));
    paused.reduce(ShellMessage::Admin(AdminMessage::SetLogPaused(true)));
    let mut session = open(&paused, 1200, 800, ThemeName::Light);
    session.flush().expect("暂停后的布局");
    let nodes = session.accessibility_dump();
    assert!(visible(&nodes, SUBLINE, 0.0, 800.0), "暂停后页头不在视口里");
    let top = list_top(&nodes);
    assert!(visible(&nodes, "第 0 条日志", top, 800.0), "暂停后列表应停在顶部");
    assert!(!visible(&nodes, "第 29 条日志", top, 800.0), "暂停后不该滚到最后一条");
}

/// 日志列表视口的上沿：来源筛选最后一枚芯片的下沿，列表就在它下面。
fn list_top(nodes: &[nana_ui_devtools::agent::AccessibilityDumpNode]) -> f32 {
    let chip = nodes.iter().find(|node| node.label.as_deref() == Some("辅助进程")).expect("来源筛选芯片");
    chip.bounds.y + chip.bounds.height
}

/// 标签等于 `label` 的节点有一部分落在纵向 `[top, bottom)` 里。
fn visible(nodes: &[nana_ui_devtools::agent::AccessibilityDumpNode], label: &str, top: f32, bottom: f32) -> bool {
    nodes
        .iter()
        .filter(|node| node.label.as_deref() == Some(label))
        .any(|node| node.bounds.y < bottom && node.bounds.y + node.bounds.height > top)
}

fn log_record(index: u32) -> SystemLogRecord {
    SystemLogRecord {
        id: format!("log-{index:02}"),
        timestamp: format!("2026-10-08T07:{index:02}:00Z"),
        level: "info".into(),
        category: "repository".into(),
        action: "sync".into(),
        message: format!("第 {index} 条日志"),
        source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: Some("repo".into()) },
        location: SystemLogLocation::default(),
        context: serde_json::json!({}),
    }
}

fn wav_preview() -> ShellViewModel {
    let mut model = ready_library();
    model.workspace.panel = WorkspacePanel::Files;
    let detail = asset("audio/tone.wav", "wav");
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail)));
    let InspectEffect::LoadMedia { path, generation, .. } = model.inspect.take_effects().pop().expect("音视频请求") else {
        panic!("预览没有排队读取音频");
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
        path,
        generation,
        result: Ok(PlaybackSessionState {
            session_id: "preview-repo".into(),
            repo_id: "repo".into(),
            playlist_id: String::new(),
            playlist_item_id: None,
            status: "paused".into(),
            current_time_ms: 0,
            duration_ms: Some(2_000),
            volume: 1.0,
            can_seek: true,
            can_volume: true,
            error: None,
            updated_at: String::new(),
        }),
        pcm: None,
        frames: None,
    }));
    model
}

fn import_tool() -> ShellViewModel {
    let mut model = ready_library();
    model.workspace.panel = WorkspacePanel::Extensions;
    model.reduce(ShellMessage::Admin(momobako_nana::shell::AdminMessage::SetToolPages(vec![
        ToolPageEntry {
            id: "momobako.tool.file-manager".into(),
            label: "文件导入".into(),
            plugin_name: "file-manager".into(),
            description: String::new(),
            native: true,
        },
    ])));
    model
}

/// 拓展页插件管理里只有一个来源插件，并已展开它的设置。
fn login_settings() -> ShellViewModel {
    let mut model = ready_library();
    model.workspace.panel = WorkspacePanel::Extensions;
    let plugin: PluginManifest = serde_json::from_value(serde_json::json!({
        "pluginId": "netease",
        "name": "网易云",
        "version": "1.0.0",
        "kind": "source",
        "description": "",
        "capabilities": [],
        "enabled": true,
        "sdk": "backend",
        "entry": null,
        "source": "builtin",
        "runtime": "native",
        "permissions": [],
        "compat": {"sdkVersion": "1"},
        "status": "ready",
        "contributes": {
            "source": {
                "authentication": {
                    "kind": "qr",
                    "createSessionMethod": "auth.createQrSession",
                    "statusMethod": "auth.getLoginStatus",
                    "clearMethod": "auth.clearLogin"
                }
            }
        }
    }))
    .expect("插件清单");
    model.admin.plugins = vec![plugin];
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings("netease".into())));
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "netease".into(),
        data_directory: "C:/plugins/netease".into(),
        schema: serde_json::Value::Null,
        values: Default::default(),
    })));
    let _ = model.admin.take_effects();
    model
}

fn ready_library() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.apply_repository_list(None, Ok(vec![WorkspaceRepository {
        repo_id: "repo".into(),
        name: "动画素材".into(),
        path: "D:/Libraries/Anime".into(),
        status: "ready".into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    }]));
    model
}

fn asset(path: &str, extension: &str) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: "asset".into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename: path.rsplit('/').next().unwrap_or(path).into(),
            extension: extension.into(),
            size_bytes: 16,
            size_label: "16 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

fn open(model: &ShellViewModel, width: u32, height: u32, theme: ThemeName) -> RuntimeAgentSession {
    let document = momobako_nana::acceptance_document_at_width(model.clone(), width as f32).expect("文档");
    let mut session = RuntimeAgentSession::new(document, width, height).expect("会话");
    session.set_theme(theme).expect("主题");
    session.flush().expect("布局");
    session
}

fn shot(session: &mut RuntimeAgentSession, name: &str) -> nana_ui_devtools::agent::protocol::PixelStats {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nana-live-visual");
    fs::create_dir_all(&dir).expect("截图目录");
    session.screenshot_png(dir.join(format!("{name}.png"))).expect("截图")
}

fn pump(session: &mut RuntimeAgentSession, model: &mut ShellViewModel) -> Vec<WindowCommand> {
    let queued = session.document_mut().context_mut().take_program_messages();
    assert!(!queued.is_empty(), "点击没有进入程序消息");
    let mut commands = Vec::new();
    for message in queued {
        let message = message.downcast::<ShellMessage>().expect("壳层消息");
        commands.extend(commit_interaction(model, *message, WindowId(1), false));
    }
    mount_shell(session.document_mut(), model).expect("重建");
    session.flush().expect("重新布局");
    commands
}

fn click(session: &mut RuntimeAgentSession, label: &str) {
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("点不到 {label}"));
    assert!(session.click_node(node.id).expect("click"), "点击没有命中 {label}");
}

fn has(nodes: &[nana_ui_devtools::agent::AccessibilityDumpNode], label: &str) -> bool {
    nodes.iter().any(|node| node.label.as_deref().is_some_and(|text| text.contains(label)))
}

/// 文件预览和搜索面板互斥，和 Vue `Home.vue` 的 `v-else-if` 一样：窄屏预览页上不叠搜索说明。
fn assert_preview_owns_the_page(nodes: &[nana_ui_devtools::agent::AccessibilityDumpNode]) {
    assert!(
        nodes.iter().any(|node| node.label.as_deref() == Some("文件预览")),
        "窄屏缺少文件预览"
    );
    assert!(
        nodes.iter().all(|node| !node.label.as_deref().is_some_and(|text| text.contains("输入关键词"))),
        "窄屏预览页上叠了搜索说明"
    );
}
