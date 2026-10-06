//! 实况工作区的视觉和点击验收。
//!
//! 文档来自生产 `acceptance_document_for_model`。布局盒相交、中文窗口按钮
//! 或缺少像素时失败。`nonclear_ratio > 0` 不算通过。

use std::fs;
use std::path::PathBuf;

use momobako_nana::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};
use momobako_nana::shell::{
    commit_interaction, mount_shell, ShellMessage, ShellViewModel, WorkspaceRepository,
};
use momobako_nana::acceptance_document_for_model;
use nana_ui::runtime::{Entity, StableNodeId, TextInput};
use nana_ui_devtools::agent::{
    AccessibilityDumpNode, AgentSession, BoundsDump, RuntimeAgentSession,
    protocol::{HitDump, ThemeName},
};
use nana_ui_devtools::offscreen;
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::WindowId;

const NAMES: [&str; 3] = ["photos", "audio", "cover.png"];
const DISPLAY_MODES: [&str; 4] = ["自适应", "瀑布流", "网格", "列表"];
const TOOLBAR: [&str; 10] = [
    "新建文件夹",
    "建文件",
    "导入",
    "从 ZIP 导入",
    "复制导入",
    "剪切导入",
    "复制",
    "移动",
    "重命名",
    "删除",
];
const PLAYER: [&str; 8] = [
    "未选择播放内容",
    "0:00 / 0:00",
    "列表循环",
    "上一首",
    "下一首",
    "当前队列",
    "打开预览",
    "播放",
];

#[test]
fn live_workspace_title_bar_layout_and_clicks_hold() {
    assert!(
        offscreen::pixels_available(),
        "实况视觉验收读不到像素，不能当作通过"
    );
    let viewports = [
        (1200u32, 800u32, ThemeName::Light, true),
        (1200, 800, ThemeName::Dark, true),
        (960, 600, ThemeName::Light, false),
        (960, 600, ThemeName::Dark, false),
    ];
    for (width, height, theme, save) in viewports {
        let model = live_files();
        let mut session = open_session(model, width, height, theme);
        let nodes = session.accessibility_dump();
        assert_title_bar(&nodes);
        assert_file_layout(&session, &nodes);
        let stats = session
            .screenshot_png(shot_path(theme, width, height))
            .expect("screenshot");
        assert_eq!(stats.width, width);
        assert_eq!(stats.height, height);
        assert!(stats.width > 0 && stats.height > 0);
        let _ = save;
    }
    assert_startup();
    assert_missing();
    assert_clicks();
    assert_smart_folder_dialog();
}

fn assert_startup() {
    let mut model = ShellViewModel::default();
    model.workspace.startup.begin();
    let mut session = open_session(model, 1200, 800, ThemeName::Light);
    let nodes = session.accessibility_dump();
    assert_title_bar(&nodes);
    assert!(
        nodes.iter().all(|node| node.label.as_deref() != Some("快捷方式")),
        "启动时侧栏应该隐藏"
    );
    for step in ["准备资源库", "同步文件变化", "读取资源索引", "加载首屏内容"] {
        assert!(
            nodes.iter().any(|node| node.label.as_deref().is_some_and(|label| label.contains(step))),
            "缺少启动步骤 {step}"
        );
    }
    assert!(
        nodes.iter().any(|node| node.label.as_deref() == Some("第 0 / 4 步")),
        "缺少四步进度"
    );
    paint(&mut session);
}

fn assert_missing() {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.apply_repository_list(None, Ok(vec![repository("missing")]));
    let mut session = open_session(model, 1200, 800, ThemeName::Light);
    let nodes = session.accessibility_dump();
    assert_title_bar(&nodes);
    for label in ["重定向", "刷新", "删除资源库"] {
        assert!(
            nodes.iter().any(|node| node.label.as_deref() == Some(label)),
            "缺失仓库缺少 {label}"
        );
    }
    paint(&mut session);
}

fn assert_clicks() {
    let mut model = live_files();
    let mut session = open_session(model.clone(), 1200, 800, ThemeName::Light);
    assert_eq!(search_placeholder(&session), "搜索文件名、标签、元数据");

    click_label(&mut session, "折叠侧边栏");
    let _ = pump(&mut session, &mut model);
    assert!(
        session.accessibility_dump().iter().any(|node| node.label.as_deref() == Some("展开侧边栏")),
        "侧栏开关没有切换"
    );

    click_label(&mut session, "Minimize");
    let commands = pump(&mut session, &mut model);
    assert!(
        commands.iter().any(|command| matches!(command, WindowCommand::SetMinimized { minimized: true, .. })),
        "最小化没有排队宿主命令：{commands:?}"
    );

    click_label(&mut session, "Maximize");
    let commands = pump(&mut session, &mut model);
    assert!(
        commands.iter().any(|command| matches!(command, WindowCommand::SetMaximized { maximized: true, .. })),
        "最大化没有排队宿主命令：{commands:?}"
    );

    click_label(&mut session, "Close");
    let commands = pump(&mut session, &mut model);
    assert!(
        commands.iter().all(|command| !matches!(command, WindowCommand::Close(_))),
        "关闭跳过了确认：{commands:?}"
    );
    assert!(
        session.accessibility_dump().iter().any(|node| node.label.as_deref() == Some("确认关闭 MomoBako？")),
        "关闭没有进入确认"
    );

    click_label(&mut session, "全局搜索");
    session.type_text("封面检索").expect("输入搜索");
    let _ = pump(&mut session, &mut model);
    let value = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.label.as_deref() == Some("全局搜索"))
        .and_then(|node| node.value);
    assert_eq!(value.as_deref(), Some("封面检索"));

    click_label(&mut session, "显示筛选栏");
    let _ = pump(&mut session, &mut model);
    assert!(
        session.accessibility_dump().iter().any(|node| node.role == "text-input" && node.label.as_deref() == Some("标签")),
        "打开筛选后没有筛选输入"
    );

    click_label(&mut session, "隐藏筛选栏");
    let _ = pump(&mut session, &mut model);
    let nodes = session.accessibility_dump();
    assert_eq!(
        nodes.iter().filter(|node| node.label.as_deref() == Some("显示筛选栏")).count(),
        1,
        "筛选开关被重复挂载"
    );
    assert!(nodes.iter().all(|node| node.label.as_deref() != Some("隐藏筛选栏")));
    assert!(
        nodes.iter().all(|node| !(node.role == "text-input" && node.label.as_deref() == Some("标签"))),
        "关闭筛选后筛选输入还在"
    );
}

fn assert_smart_folder_dialog() {
    let mut model = live_files();
    let mut session = open_session(model.clone(), 1200, 800, ThemeName::Light);
    click_label(&mut session, "新建智能文件夹");
    let _ = pump(&mut session, &mut model);
    let nodes = session.accessibility_dump();
    for label in ["新建智能文件夹", "已选 顶层智能文件夹", "全部匹配", "任一匹配", "取消", "创建"] {
        assert!(has_label(&nodes, label), "新建智能文件夹缺少 {label}");
    }
    assert_eq!(input_placeholder(&session, "名称"), "例如 高评分 PSD");
    let dialog_shot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nana-live-visual/smart-folder-dialog.png");
    session.screenshot_png(&dialog_shot).expect("智能文件夹对话框截图");

    click_input(&mut session, "名称");
    session.type_text("高评分 PSD").expect("输入名称");
    let _ = pump(&mut session, &mut model);
    click_label(&mut session, "创建");
    let _ = pump(&mut session, &mut model);
    assert!(has_label(&session.accessibility_dump(), "正在保存…"), "创建没有进入提交");
}

fn open_session(
    model: ShellViewModel,
    width: u32,
    height: u32,
    theme: ThemeName,
) -> RuntimeAgentSession {
    let document = acceptance_document_for_model(model).expect("production document");
    let mut session = RuntimeAgentSession::new(document, width, height).expect("agent session");
    session.set_theme(theme).expect("theme");
    session.flush().expect("layout");
    let nodes = session.accessibility_dump();
    assert!(
        nodes.iter().any(|node| node.bounds.width > 0.0 && node.bounds.height > 0.0),
        "布局盒为空"
    );
    session
}

fn paint(session: &mut RuntimeAgentSession) {
    let stats = session.screenshot_png(shot_path(ThemeName::Light, 32, 32)).expect("pixels");
    assert!(stats.width > 0 && stats.height > 0);
}

fn shot_path(theme: ThemeName, width: u32, height: u32) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/nana-live-visual");
    fs::create_dir_all(&dir).expect("visual dir");
    let name = match theme {
        ThemeName::Light if width == 1200 => "live-files-light.png",
        ThemeName::Dark if width == 1200 => "live-files-dark.png",
        _ => "live-other.png",
    };
    let _ = height;
    dir.join(name)
}

fn assert_title_bar(nodes: &[AccessibilityDumpNode]) {
    assert!(has_label(nodes, "折叠侧边栏"), "缺少侧栏开关");
    assert!(has_label(nodes, "全局搜索"), "缺少全局搜索");
    assert!(has_label(nodes, "显示筛选栏"), "缺少筛选开关");
    assert!(has_label(nodes, "Minimize"));
    assert!(has_label(nodes, "Maximize"));
    assert!(has_label(nodes, "Close"));
    for banned in ["最小化", "最大化", "关闭", "资源库工作区"] {
        assert!(
            nodes.iter().all(|node| node.label.as_deref() != Some(banned)),
            "标题栏仍是占位文案 {banned}"
        );
    }
    let title = nodes
        .iter()
        .find(|node| node.label.as_deref() == Some("MomoBako") && node.bounds.width > 400.0)
        .expect("标题栏");
    let search = nodes
        .iter()
        .find(|node| node.role == "text-input" && node.label.as_deref() == Some("全局搜索"))
        .expect("全局搜索");
    let filter = nodes
        .iter()
        .find(|node| node.role == "button" && node.label.as_deref() == Some("显示筛选栏"))
        .expect("显示筛选栏");
    assert!(
        filter.bounds.width > 24.0 && filter.bounds.height > 8.0,
        "筛选开关布局盒为空：{:?}",
        filter.bounds
    );
    assert!(
        search.bounds.width > 24.0 && search.bounds.height > 8.0,
        "搜索框布局盒为空：{:?}",
        search.bounds
    );
    let center = center_slot(nodes, search, title);
    assert!(
        contains(&title.bounds, &filter.bounds),
        "筛选开关不在标题栏内 {:?} / {:?}",
        filter.bounds,
        title.bounds
    );
    assert!(
        contains(&center.bounds, &filter.bounds),
        "筛选开关不在标题栏中间槽 {:?} / {:?}",
        filter.bounds,
        center.bounds
    );
    assert!(
        !intersects(&filter.bounds, &search.bounds),
        "筛选开关与搜索框相交 {:?} / {:?}",
        filter.bounds,
        search.bounds
    );
}

/// 搜索框往上找到比整条标题栏窄的中间槽，那是会裁掉溢出内容的盒子。
fn center_slot<'a>(
    nodes: &'a [AccessibilityDumpNode],
    search: &AccessibilityDumpNode,
    title: &AccessibilityDumpNode,
) -> &'a AccessibilityDumpNode {
    let mut current = search.parent;
    let mut slot = None;
    while let Some(id) = current {
        let Some(node) = nodes.iter().find(|node| node.id == id) else {
            break;
        };
        if node.id == title.id {
            break;
        }
        if node.bounds.width > 80.0 && node.bounds.width + 8.0 < title.bounds.width {
            slot = Some(node);
        }
        current = node.parent;
    }
    slot.expect("标题栏中间槽")
}

fn contains(outer: &BoundsDump, inner: &BoundsDump) -> bool {
    inner.x + 0.5 >= outer.x
        && inner.y + 0.5 >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + 0.5
        && inner.y + inner.height <= outer.y + outer.height + 0.5
}

fn assert_file_layout(session: &RuntimeAgentSession, nodes: &[AccessibilityDumpNode]) {
    let task = required(nodes, "任务 2");
    let modes = buttons(nodes, &DISPLAY_MODES);
    let tools = buttons(nodes, &TOOLBAR);
    let names = NAMES.map(|name| {
        nodes
            .iter()
            .find(|node| node.role == "button" && node.label.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("缺少文件名 {name}"))
    });
    let player = nodes
        .iter()
        .filter(|node| node.label.as_deref().is_some_and(|label| PLAYER.contains(&label)))
        .collect::<Vec<_>>();
    assert!(!player.is_empty(), "缺少播放条");
    for node in modes.iter().chain(tools.iter()).chain(names.iter()).chain(player.iter()) {
        assert!(
            node.bounds.width > 0.0 && node.bounds.height > 0.0,
            "布局盒为空：{:?}",
            node.label
        );
    }
    assert_disjoint(&modes, "展示方式");
    assert_disjoint(&tools, "文件工具条");
    for mode in &modes {
        assert!(!intersects(&mode.bounds, &task.bounds), "展示方式与任务读数相交");
        for tool in &tools {
            assert!(!intersects(&mode.bounds, &tool.bounds), "展示方式与文件工具条相交");
        }
    }
    for name in &names {
        for bar in &player {
            assert!(
                !intersects(&name.bounds, &bar.bounds),
                "文件名 {:?} 与播放条 {:?} 相交",
                name.label,
                bar.label
            );
        }
        let probe = AgentSession::scene_probe(session, name.id).unwrap_or_else(|| panic!("读不到文件名布局盒 {:?}", name.label));
        if let Some(hit) = probe.occluded_by {
            assert!(
                !is_player(&hit),
                "播放条遮挡文件名 {:?}: {:?}",
                name.label,
                hit.label
            );
        }
    }
}

fn is_player(hit: &HitDump) -> bool {
    hit.label.as_deref().is_some_and(|label| PLAYER.contains(&label))
}

fn buttons<'a>(nodes: &'a [AccessibilityDumpNode], labels: &[&str]) -> Vec<&'a AccessibilityDumpNode> {
    labels
        .iter()
        .map(|label| {
            nodes
                .iter()
                .find(|node| node.role == "button" && node.label.as_deref() == Some(*label))
                .unwrap_or_else(|| panic!("缺少按钮 {label}"))
        })
        .collect()
}

fn required<'a>(nodes: &'a [AccessibilityDumpNode], label: &str) -> &'a AccessibilityDumpNode {
    nodes
        .iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("缺少 {label}"))
}

fn has_label(nodes: &[AccessibilityDumpNode], label: &str) -> bool {
    nodes.iter().any(|node| node.label.as_deref() == Some(label))
}

fn assert_disjoint(nodes: &[&AccessibilityDumpNode], kind: &str) {
    for (index, node) in nodes.iter().enumerate() {
        for other in nodes.iter().skip(index + 1) {
            assert!(
                !intersects(&node.bounds, &other.bounds),
                "{kind} 相交：{:?} {:?}",
                node.label,
                other.label
            );
        }
    }
}

fn intersects(a: &BoundsDump, b: &BoundsDump) -> bool {
    a.width > 0.0
        && a.height > 0.0
        && b.width > 0.0
        && b.height > 0.0
        && a.x < b.x + b.width
        && a.x + a.width > b.x
        && a.y < b.y + b.height
        && a.y + a.height > b.y
}

fn live_files() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model
        .workspace
        .apply_repository_list(None, Ok(vec![repository("ready")]));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot())));
    model.active_tasks = 2;
    model
}

fn repository(status: &str) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: "repo".into(),
        name: "动画素材".into(),
        path: "D:/Libraries/Anime".into(),
        status: status.into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    }
}

fn snapshot() -> FileBrowserSnapshot {
    let entries = NAMES
        .into_iter()
        .map(|name| FileBrowserEntry {
            path: name.into(),
            name: name.into(),
            kind: if name == "cover.png" { "file" } else { "directory" }.into(),
            extension: None,
            size_bytes: None,
            size_label: None,
            modified_at: None,
            asset_id: None,
            status: None,
            thumbnail_path: None,
            thumbnail_custom: false,
            hardlink_group_id: None,
            hardlink_state: None,
            tags: Vec::new(),
            alias_paths: Vec::new(),
            folder_metadata: None,
            metadata: std::collections::BTreeMap::new(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        })
        .collect::<Vec<_>>();
    FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "D:/Libraries/Anime".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

fn pump(session: &mut RuntimeAgentSession, model: &mut ShellViewModel) -> Vec<WindowCommand> {
    let queued = session.document_mut().context_mut().take_program_messages();
    assert!(!queued.is_empty(), "点击没有进入程序消息");
    let mut commands = Vec::new();
    for message in queued {
        let message = message.downcast::<ShellMessage>().expect("壳层消息");
        commands.extend(commit_interaction(model, *message, WindowId(1), false));
    }
    mount_shell(session.document_mut(), model).expect("remount");
    session.flush().expect("relayout");
    commands
}

fn click_input(session: &mut RuntimeAgentSession, label: &str) {
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.role == "text-input" && node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("点不到输入 {label}"));
    assert!(session.click_node(node.id).expect("click"), "点击没有命中输入 {label}");
}

fn click_label(session: &mut RuntimeAgentSession, label: &str) {
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("点不到 {label}"));
    assert!(session.click_node(node.id).expect("click"), "点击没有命中 {label}");
}

fn search_placeholder(session: &RuntimeAgentSession) -> String {
    input_placeholder(session, "全局搜索")
}

fn input_placeholder(session: &RuntimeAgentSession, label: &str) -> String {
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|node| node.role == "text-input" && node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("缺少输入 {label}"));
    let id = StableNodeId::new(node.id).expect("input id");
    session
        .document()
        .context()
        .read(Entity::<TextInput>::from_stable_id(id), |input| input.placeholder.to_string())
        .unwrap_or_else(|_| panic!("读不到输入 {label}"))
}
