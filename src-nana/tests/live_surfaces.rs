//! 预览、工具页和来源登录的视觉与点击验收。
//!
//! 用生产文档渲染，点真实控件，再看无障碍树和像素。空白图或点了没变化都算失败。

use std::fs;
use std::path::PathBuf;

use momobako_nana::backend::services::repository::{
    AssetDetail, AssetSummary, PlaybackSessionState, PluginManifest,
};
use momobako_nana::shell::{
    commit_interaction, mount_shell, InspectEffect, InspectMessage, ShellMessage, ShellViewModel,
    ToolPageEntry, WorkspacePanel, WorkspaceRepository,
};
use nana_ui_devtools::agent::{AgentSession, BoundsDump, RuntimeAgentSession, protocol::ThemeName};
use nana_ui_devtools::offscreen;
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::WindowId;

#[test]
fn preview_tool_and_login_surfaces_paint_and_respond() {
    assert!(offscreen::pixels_available(), "界面验收读不到像素，不能当作通过");
    assert_wav_preview();
    assert_import_tool();
    assert_source_login();
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
            assert_search_copy_stays_off_preview(&before);
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

fn assert_source_login() {
    let mut model = login_settings();
    let mut session = open(&model, 1200, 800, ThemeName::Light);
    for label in ["创建登录会话", "查询登录状态", "退出登录"] {
        assert!(has(&session.accessibility_dump(), label), "缺少 {label}");
    }
    let stats = shot(&mut session, "source-login-light");
    assert!(stats.nonclear_ratio > 0.01, "登录设置页几乎是空的");
    click(&mut session, "创建登录会话");
    let _ = pump(&mut session, &mut model);
    assert!(
        has(&session.accessibility_dump(), "正在调用 auth.createQrSession…"),
        "点击登录没有出现调用文案"
    );
    let _ = shot(&mut session, "source-login-calling");
}

fn wav_preview() -> ShellViewModel {
    let mut model = ready_library();
    model.workspace.panel = WorkspacePanel::Search;
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

fn login_settings() -> ShellViewModel {
    let mut model = ready_library();
    model.page = momobako_nana::shell::ShellPage::Settings;
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
    let document = momobako_nana::acceptance_document_for_model(model.clone()).expect("文档");
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

/// 窄屏里搜索说明必须留在自己的卡片，不能盖住下面的文件预览。
fn assert_search_copy_stays_off_preview(nodes: &[nana_ui_devtools::agent::AccessibilityDumpNode]) {
    let preview = nodes.iter().find(|node| node.label.as_deref() == Some("文件预览")).expect("窄屏缺少文件预览");
    let copy = nodes
        .iter()
        .find(|node| node.label.as_deref().is_some_and(|text| text.contains("输入关键词")))
        .expect("窄屏缺少搜索说明");
    assert!(
        !bounds_intersect(&copy.bounds, &preview.bounds),
        "搜索说明盖住了文件预览：说明 {:?}，预览 {:?}",
        copy.bounds,
        preview.bounds
    );
}

fn bounds_intersect(a: &BoundsDump, b: &BoundsDump) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}
