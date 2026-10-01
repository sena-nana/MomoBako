//! NanaUI 原生页面的真实 Runtime/WGPU 离屏验收。

use momobako_nana::{acceptance_document_for, shell::ShellPage};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nana_ui_devtools::offscreen;

#[test]
fn renders_light_and_dark_shell_at_product_viewports() {
    if !offscreen::pixels_available() {
        return;
    }
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, width, height) in [("light-main", 1200, 800), ("dark-min", 960, 600)] {
        let document = acceptance_document_for(ShellPage::Loading).expect("acceptance document");
        let mut session = RuntimeAgentSession::new(document, width, height).expect("agent session");
        if name.starts_with("dark") {
            session
                .set_theme(nana_ui_devtools::agent::protocol::ThemeName::Dark)
                .expect("dark theme");
        }
        let path = output.join(format!("{name}.png"));
        let stats = session.screenshot_png(&path).expect("PNG snapshot");
        assert_eq!(stats.width, width);
        assert_eq!(stats.height, height);
        assert!(
            stats.nonclear_ratio > 0.0,
            "snapshot must contain painted UI"
        );
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|node| node.label.as_deref() == Some("MomoBako"))
        );
    }
}

#[test]
fn renders_repository_states_with_semantic_labels() {
    if !offscreen::pixels_available() {
        return;
    }
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for page in [
        ShellPage::Loading,
        ShellPage::EmptyRepository,
        ShellPage::Error,
        ShellPage::FileList,
        ShellPage::SelectedFile,
        ShellPage::Playlists,
        ShellPage::PluginSettings,
        ShellPage::TaskRunning,
        ShellPage::Conflict,
        ShellPage::UnsavedEdit,
        ShellPage::Settings,
        ShellPage::Logs,
    ] {
        let document = acceptance_document_for(page.clone()).expect("acceptance document");
        let mut session = RuntimeAgentSession::new(document, 1200, 800).expect("agent session");
        let labels = session
            .accessibility_dump()
            .into_iter()
            .filter_map(|node| node.label)
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == "MomoBako"));
        assert!(labels.iter().any(|label| label == page_title(&page)));

        // 证据与 PNG 使用同一个 RuntimeAgentSession，避免语义树、布局和命中
        // 结果来自静态 mock。scene_probe 同时包含目标布局盒和绘制节点信息。
        let evidence_name = format!("state-{}.json", page_slug(&page));
        let evidence = output.join(evidence_name);
        let root = session
            .accessibility_dump()
            .into_iter()
            .find(|node| node.label.as_deref() == Some("MomoBako"))
            .expect("root accessibility node");
        let probe = session.scene_probe(root.id);
        let hit = session.hit_test(20.0, 20.0);
        let png = output.join(format!("state-{}.png", page_slug(&page)));
        session.screenshot_png(&png).expect("state PNG snapshot");
        let evidence_text = serde_json::to_string_pretty(&serde_json::json!({
            "page": page_slug(&page),
            "accessibility": session.accessibility_dump(),
            "scene_probe": probe,
            "hit_test": hit
        }))
        .expect("serialize acceptance evidence");
        std::fs::write(evidence, evidence_text).expect("write acceptance evidence");
    }
}

#[test]
fn native_actions_are_reachable_through_runtime_hit_testing() {
    if !offscreen::pixels_available() {
        return;
    }
    let document = acceptance_document_for(ShellPage::FileList).expect("acceptance document");
    let mut session = RuntimeAgentSession::new(document, 1200, 800).expect("agent session");
    for label in [
        "资源库",
        "插件",
        "设置",
        "刷新状态",
        "最小化",
        "最大化",
        "关闭",
        "刷新列表",
        "编辑内容",
    ] {
        let node = session
            .accessibility_dump()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing native action: {label}"));
        assert!(session.click_node(node.id).expect("runtime click"), "{label}");
    }
}

fn page_title(page: &ShellPage) -> &'static str {
    match page {
        ShellPage::Loading => "正在加载资源库",
        ShellPage::EmptyRepository => "资源库为空",
        ShellPage::Error => "资源库加载失败",
        ShellPage::FileList => "文件列表",
        ShellPage::SelectedFile => "文件预览",
        ShellPage::Playlists => "播放列表",
        ShellPage::PluginSettings => "插件设置",
        ShellPage::TaskRunning => "任务进行中",
        ShellPage::Conflict => "同步冲突",
        ShellPage::UnsavedEdit => "编辑未保存",
        ShellPage::Settings => "应用设置",
        ShellPage::Logs => "系统日志",
    }
}

fn page_slug(page: &ShellPage) -> &'static str {
    match page {
        ShellPage::Loading => "loading",
        ShellPage::EmptyRepository => "empty-repository",
        ShellPage::Error => "error",
        ShellPage::FileList => "file-list",
        ShellPage::SelectedFile => "selected-file",
        ShellPage::Playlists => "playlists",
        ShellPage::PluginSettings => "plugin-settings",
        ShellPage::TaskRunning => "task-running",
        ShellPage::Conflict => "conflict",
        ShellPage::UnsavedEdit => "unsaved-edit",
        ShellPage::Settings => "settings",
        ShellPage::Logs => "logs",
    }
}
