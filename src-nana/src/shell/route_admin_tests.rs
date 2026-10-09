//! 常驻设置、日志、拓展和动作页的回归：无关更新一个节点都不换，相关更新原地改字段；新日志到达
//! 只多建一行；输入框在组合输入中不被打断、刚打的字不被较旧的草稿冲掉；更新以后滚动位置留着；
//! 每一步都和同一 ViewModel 新挂的文档一样。

use nana_ui::runtime::{Entity, ScrollOffset, ScrollView};

use crate::backend::services::repository::{
    RepositoryAction, RepositoryActionStep, SystemLogLocation, SystemLogPage, SystemLogRecord, SystemLogSource,
};
use crate::shell::admin::AdminMessage;
use crate::shell::host_events::HostMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, ThumbnailFrame, WorkspacePanel};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和设置页、日志面板、拓展页都无关的消息：缩略图像素、播放音量、搜索查询。
fn unrelated_messages() -> Vec<ShellMessage> {
    vec![
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame { path: "a.png".into(), natural_width: 1, natural_height: 1, width: 1, height: 1, rgba: vec![0; 4] }]),
        ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.3)),
        ShellMessage::Inspect(crate::shell::InspectMessage::SetQuery("封面".into())),
    ]
}

/// 一条实时日志，时间晚于场景里的四条。
fn log(index: u32) -> ShellMessage {
    ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
        id: format!("log-{index}"),
        timestamp: format!("2026-10-08T08:{index:02}:00Z"),
        level: "info".into(),
        category: "repository".into(),
        action: "sync".into(),
        message: format!("第 {index} 条后台日志"),
        source: SystemLogSource { kind: "host".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: None },
        location: SystemLogLocation::default(),
        context: serde_json::json!({}),
    }))
}

/// 记下这些键的节点，经过 `messages` 之后逐个比较。
fn assert_nodes_kept(harness: &mut ShellHarness, keys: &[&str], messages: Vec<ShellMessage>) {
    let nodes = keys.iter().map(|key| harness.keyed(key).unwrap_or_else(|| panic!("缺少 {key}"))).collect::<Vec<_>>();
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    for message in messages {
        harness.apply(message);
        harness.flush();
        for (key, id) in keys.iter().zip(&nodes) {
            assert_eq!(harness.keyed(key), Some(*id), "无关更新换掉了 {key}");
        }
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻路由不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 设置页：无关更新之后卡片、下拉框、插件卡片和筛选框都还是原来的节点。
#[test]
fn settings_page_keeps_every_node_through_unrelated_updates() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Settings));
    let keys = [
        "settings-scroll",
        "admin-audio-player",
        "admin-corner-smooth",
        "admin-corner-radius",
        "admin-external-card",
        "admin-plugin-keyword",
        "admin-plugin-card-momobako.local-filesystem",
        "admin-cache-card",
    ];
    assert_nodes_kept(&mut harness, &keys, unrelated_messages());
}

/// 设置页的相关更新：圆角样式换了只改分段按钮的外观；插件操作在途时按钮禁用；展开插件设置时
/// 卡片不重建，设置区出现在卡片里。
#[test]
fn settings_updates_patch_fields_in_place() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Settings));
    let smooth = harness.keyed("admin-corner-smooth").expect("平滑");
    let card = harness.keyed("admin-plugin-card-momobako.local-filesystem").expect("插件卡片");
    let next = if harness.model.admin.corner_style == "smooth" { "round" } else { "smooth" };
    harness.apply(ShellMessage::Admin(AdminMessage::SetCornerStyle(next.into())));
    harness.model.admin.take_effects();
    harness.flush();
    assert_eq!(harness.keyed("admin-corner-smooth"), Some(smooth), "分段按钮被重建了");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::Admin(AdminMessage::ToggleSettings("momobako.local-filesystem".into())));
    harness.model.admin.take_effects();
    harness.flush();
    assert!(harness.model.admin.managing, "展开设置要读一次配置");
    assert_eq!(harness.keyed("admin-plugin-card-momobako.local-filesystem"), Some(card), "插件卡片被重建了");
    assert!(harness.keyed("admin-plugin-settings-section-momobako.local-filesystem").is_some(), "设置区没有出现");
    let refresh = harness.node("刷新");
    assert!(harness.nodes().iter().any(|node| node.id == refresh && node.disabled), "插件操作在途时刷新要禁用");
    harness.assert_same_as_fresh_mount();
}

/// 日志追踪：新日志到达只多建一行，已有日志行的节点一个都不换；缓存满了掉出去的那一行才删掉。
#[test]
fn new_logs_append_rows_without_rebuilding_the_list() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Logs));
    let rows = (1..=4).map(|index| harness.keyed(&format!("admin-log-row-log-{index}")).expect("日志行")).collect::<Vec<_>>();
    let list = harness.keyed("admin-log-list").expect("日志列表");
    let remounts = harness.view_stats().remounts;
    for index in 5..=7 {
        harness.apply(log(index));
        harness.flush();
        assert!(harness.keyed(&format!("admin-log-row-log-{index}")).is_some(), "新日志 {index} 没有出现");
        for (offset, id) in rows.iter().enumerate() {
            assert_eq!(harness.keyed(&format!("admin-log-row-log-{}", offset + 1)), Some(*id), "新日志到达换掉了已有的日志行");
        }
    }
    assert_eq!(harness.keyed("admin-log-list"), Some(list), "日志列表被重建了");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻日志面板不该重挂");
    assert!(harness.find("7 条缓存").is_some(), "缓存数没有跟上");
    harness.assert_same_as_fresh_mount();
}

/// 日志列表滚动区和日志页主体的键。
const LOG_SCROLL: &str = "admin-log-scroll";
const LOGS_BODY: &str = "workspace-page-body-HasRepository-Logs";

/// 最小窗口（960×600）的日志面板，再收到第 5..=`last` 条实时日志，多到一屏放不下。
fn crowded_logs(last: u32) -> ShellHarness {
    let mut harness = ShellHarness::mount_at(ShellViewModel::for_page(ShellPage::Logs), 960.0, 600.0);
    for index in 5..=last {
        harness.apply(log(index));
    }
    harness.flush();
    harness
}

/// 滚动区现在的纵向偏移。
fn scroll_y(harness: &ShellHarness, key: &str) -> f32 {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("缺少滚动区 {key}"));
    harness.document().context().world().scroll_offset(id).unwrap_or_default().y
}

/// 滚动区能滚到的最远纵向偏移。
fn scroll_end(harness: &ShellHarness, key: &str) -> f32 {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("缺少滚动区 {key}"));
    let metrics = harness.document().context().world().scroll_metrics(id).unwrap_or_else(|| panic!("{key} 没有滚动尺寸"));
    (metrics.content_height - metrics.viewport_height).max(0.0)
}

/// 把滚动区滚到 `y`。
fn scroll_list_to(harness: &mut ShellHarness, key: &str, y: f32) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("缺少滚动区 {key}"));
    harness.window.document.context_mut().scroll_to(Entity::<ScrollView>::from_stable_id(id), ScrollOffset { x: 0.0, y }).expect("滚动");
    harness.flush();
}

/// 节点的布局盒。
fn bounds(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("缺少 {key}"));
    harness.document().context().world().layout_box(id).unwrap_or_else(|| panic!("{key} 没有布局盒"))
}

/// 列表停在末尾。
fn at_end(harness: &ShellHarness) -> bool {
    (scroll_y(harness, LOG_SCROLL) - scroll_end(harness, LOG_SCROLL)).abs() < 0.5
}

/// 最小窗口下日志多到放不下：日志页主体不是滚动区，面板正好收在主体里，页头、工具条和筛选留在视口里；
/// 日志列表在筛选下面占满剩下的高度、自己滚，追踪时停在末尾。
#[test]
fn log_list_scrolls_under_a_fixed_header() {
    let harness = crowded_logs(12);
    let body = harness.keyed(LOGS_BODY).expect("日志页主体");
    assert!(harness.document().context().world().scroll_metrics(body).is_none(), "日志页主体不该是滚动区");
    let (body_box, panel) = (bounds(&harness, LOGS_BODY), bounds(&harness, "admin-log-panel"));
    assert!(panel.y + panel.height <= body_box.y + body_box.height + 0.5, "面板超出了主体：{panel:?} / {body_box:?}");
    for key in ["admin-log-title", "admin-log-toolbar", "admin-log-filters"] {
        let node = bounds(&harness, key);
        assert!(node.y >= panel.y && node.y + node.height <= 600.0, "{key} 不在视口里：{node:?}");
    }
    let (filters, list) = (bounds(&harness, "admin-log-filters"), bounds(&harness, LOG_SCROLL));
    assert!(list.y >= filters.y + filters.height, "列表要在筛选下面：{list:?}");
    assert!(list.height >= 150.0, "最小窗口下列表至少放得下一张卡片：{list:?}");
    assert!((panel.y + panel.height - 23.0 - (list.y + list.height)).abs() < 0.5, "列表要占满面板剩下的高度：{list:?} / {panel:?}");
    assert!(scroll_end(&harness, LOG_SCROLL) > 0.0, "日志多到放不下时列表要能滚");
    assert!(at_end(&harness), "追踪时列表停在末尾");
}

/// 暂停追踪：滚到中间后新日志到达，列表停在原位；恢复追踪后下一条日志到达时回到末尾。
#[test]
fn paused_log_list_holds_its_position() {
    let mut harness = crowded_logs(12);
    harness.apply(ShellMessage::Admin(AdminMessage::SetLogPaused(true)));
    harness.flush();
    scroll_list_to(&mut harness, LOG_SCROLL, 100.0);
    let scroll = harness.keyed(LOG_SCROLL).expect("列表");
    let title = bounds(&harness, "admin-log-title");
    for index in 13..=14 {
        harness.apply(log(index));
        harness.flush();
        assert_eq!(harness.keyed(LOG_SCROLL), Some(scroll), "新日志到达换掉了列表的滚动区");
        assert_eq!(scroll_y(&harness, LOG_SCROLL), 100.0, "暂停时新日志把列表滚走了");
        assert_eq!(bounds(&harness, "admin-log-title"), title, "页头跟着动了");
    }
    harness.apply(ShellMessage::Admin(AdminMessage::SetLogPaused(false)));
    harness.flush();
    assert_eq!(scroll_y(&harness, LOG_SCROLL), 100.0, "恢复追踪本身不滚动，等下一条日志");
    harness.apply(log(15));
    harness.flush();
    assert!(at_end(&harness), "恢复追踪后新日志没有把列表带到末尾");
    harness.assert_same_as_fresh_mount();
}

/// 换到别的面板再回来：日志分支重建，列表从顶部开始，追踪时再跟到末尾；暂停时停在顶部。
/// 和改动前主区滚动区按键重建的规则一样。
#[test]
fn returning_to_logs_restarts_the_list_by_tracking_state() {
    for paused in [false, true] {
        let mut harness = crowded_logs(12);
        if paused {
            harness.apply(ShellMessage::Admin(AdminMessage::SetLogPaused(true)));
        }
        scroll_list_to(&mut harness, LOG_SCROLL, 100.0);
        let list = harness.keyed(LOG_SCROLL).expect("列表");
        for panel in [WorkspacePanel::Extensions, WorkspacePanel::Logs] {
            harness.apply(ShellMessage::SetWorkspacePanel(panel));
            harness.model.admin.take_effects();
            harness.flush();
            harness.flush();
        }
        assert_ne!(harness.keyed(LOG_SCROLL), Some(list), "回来时日志分支应该重建");
        if paused {
            assert_eq!(scroll_y(&harness, LOG_SCROLL), 0.0, "暂停时回到日志面板应从顶部开始");
        } else {
            assert!(scroll_end(&harness, LOG_SCROLL) > 0.0);
            assert!(at_end(&harness), "追踪时回到日志面板应停在末尾");
        }
    }
}

/// 日志筛选：换级别筛选时留下的行不重建，筛掉的行不在无障碍树里。
#[test]
fn log_filters_keep_the_rows_they_still_show() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Logs));
    let info = harness.keyed("admin-log-row-log-1").expect("信息日志");
    harness.apply(ShellMessage::Admin(AdminMessage::ToggleLogLevel("info".into())));
    harness.flush();
    assert_eq!(harness.keyed("admin-log-row-log-1"), Some(info), "留下的日志行被重建了");
    assert!(harness.keyed("admin-log-row-log-3").is_none(), "警告日志应该被筛掉");
    harness.assert_same_as_fresh_mount();
}

/// 插件筛选框和日志搜索框正在组合输入：后台消息不换输入框节点、不断预编辑；提交后字落在原节点上。
/// 日志搜索框在固定的页头里，聚焦不滚动任何东西。
#[test]
fn admin_inputs_keep_their_preedit_through_updates() {
    for (page, label, scroll) in [(ShellPage::Settings, "筛选插件", Some("settings-scroll")), (ShellPage::Logs, "搜索日志", None)] {
        let mut harness = ShellHarness::mount(ShellViewModel::for_page(page));
        let input = harness.input(label);
        harness.focus(input);
        harness.compose("cha");
        let mut messages = unrelated_messages();
        messages.push(log(9));
        for message in messages {
            harness.apply(message);
            harness.flush();
            assert_eq!(harness.input(label), input, "更新换掉了输入框 {label}");
            assert_eq!(harness.preedit(input).as_deref(), Some("cha"), "更新打断了 {label} 的预编辑");
            assert_eq!(harness.focused(), Some(input), "更新以后焦点离开了 {label}");
        }
        harness.commit("查");
        for message in harness.take_messages() {
            harness.apply(message);
        }
        harness.flush();
        assert_eq!(harness.input(label), input);
        assert_eq!(harness.value(input), "查");
        if let Some(scroll) = scroll {
            scroll_to_top(&mut harness, scroll);
        }
        harness.assert_same_as_fresh_mount();
    }
}

/// 插件设置字段的受控输入：按键的消息排在后台消息后面时，同步不把较旧的草稿写回输入框；
/// 消息都归约以后草稿进了 ViewModel，输入框还是原来的节点。
#[test]
fn plugin_field_drafts_survive_background_messages() {
    let mut harness = ShellHarness::mount(scene("source-auth-gap"));
    let input = harness.input("API Base URL");
    harness.focus(input);
    harness.type_text("h");
    let queued = harness.take_messages();
    assert!(!queued.is_empty(), "打字没有发出消息");
    harness.apply(log(9));
    harness.flush();
    assert_eq!(harness.value(input), "h", "后台消息的同步把刚打的字回滚了");
    harness.type_text("k");
    let mut later = harness.take_messages();
    for message in queued.into_iter().chain(later.drain(..)) {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.input("API Base URL"), input, "字段输入框被重建了");
    assert_eq!(harness.value(input), "hk");
    let drafts = harness.model.admin.field_drafts.values().flat_map(|drafts| drafts.values()).cloned().collect::<Vec<_>>();
    assert!(drafts.iter().any(|draft| draft == "hk"), "草稿没有进 ViewModel：{drafts:?}");
    harness.assert_same_as_fresh_mount();
}

/// 设置页滚到中间：无关更新和相关更新以后滚动容器还是原来那个，偏移留着。
#[test]
fn settings_scroll_survives_updates() {
    let mut harness = ShellHarness::mount_at(ShellViewModel::for_page(ShellPage::Settings), 1200.0, 420.0);
    let scroll = harness.keyed("settings-scroll").expect("设置页滚动容器");
    harness
        .window
        .document
        .context_mut()
        .scroll_to(Entity::<ScrollView>::from_stable_id(scroll), ScrollOffset { x: 0.0, y: 160.0 })
        .expect("滚动");
    harness.flush();
    let mut messages = unrelated_messages();
    messages.push(ShellMessage::Admin(AdminMessage::SetKeyword("local".into())));
    for message in messages {
        harness.apply(message);
        harness.flush();
        assert_eq!(harness.keyed("settings-scroll"), Some(scroll), "更新换掉了设置页的滚动容器");
        assert_eq!(harness.document().context().world().scroll_offset(scroll).unwrap_or_default().y, 160.0, "更新以后设置页回到了顶部");
    }
}

/// 把主区的滚动容器滚回顶部：聚焦输入框会把它滚进视口，新挂的文档没有焦点，比较之前先对齐。
fn scroll_to_top(harness: &mut ShellHarness, key: &str) {
    let scroll = harness.keyed(key).unwrap_or_else(|| panic!("缺少滚动容器 {key}"));
    harness
        .window
        .document
        .context_mut()
        .scroll_to(Entity::<ScrollView>::from_stable_id(scroll), ScrollOffset { x: 0.0, y: 0.0 })
        .expect("滚回顶部");
    harness.flush();
}

/// 一个只有一步的仓库动作。
fn action(id: &str, status: &str, enabled: bool) -> RepositoryAction {
    RepositoryAction {
        action_id: id.into(),
        repo_id: "acceptance-repo".into(),
        source: "builtin".into(),
        source_action_id: None,
        name: format!("动作 {id}"),
        status: status.into(),
        enabled,
        raw: serde_json::Value::Null,
        unsupported_reason: None,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
        steps: vec![RepositoryActionStep {
            step_id: "step".into(),
            action_id: id.into(),
            repo_id: "acceptance-repo".into(),
            step_kind: "copy".into(),
            label: "复制".into(),
            status: status.into(),
            config: serde_json::Value::Null,
            raw: serde_json::Value::Null,
            unsupported_reason: None,
            sort_order: 0,
        }],
        last_run: None,
    }
}

fn background(harness: &ShellHarness, key: &str) -> Option<nana_ui_core::SemanticColorRole> {
    let node = harness.keyed(key).unwrap_or_else(|| panic!("缺少 {key}"));
    harness.document().context().world().node_style(node).and_then(|style| style.background)
}

/// 仓库动作：动作行按 id 只建一次，点选别的动作时行不重建、选中底色换过去，详情跟着换；
/// 读取失败时提示出现，列表留着。
#[test]
fn actions_follow_selection_in_place() {
    let mut model = scene("live-files-plain");
    let repo_id = model.repository_id.clone().expect("场景有仓库");
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    model.admin.take_effects();
    let actions = vec![action("import", "blocked", false), action("copy", "ready", true)];
    model.reduce(ShellMessage::Admin(AdminMessage::ActionsLoaded { repo_id: repo_id.clone(), result: Ok(actions) }));
    let mut harness = ShellHarness::mount(model);
    let rows = ["admin-action-import", "admin-action-copy"].map(|key| harness.keyed(key).unwrap_or_else(|| panic!("缺少 {key}")));
    assert_eq!(background(&harness, "admin-action-import"), Some(nana_ui_core::SemanticColorRole::Hover), "第一个动作默认选中");
    assert!(harness.find("动作 import").is_some());

    harness.apply(ShellMessage::Admin(AdminMessage::SelectAction("copy".into())));
    harness.flush();
    assert_eq!(["admin-action-import", "admin-action-copy"].map(|key| harness.keyed(key).expect("动作行")), rows, "点选换掉了动作行");
    assert_eq!(background(&harness, "admin-action-import"), None, "取消选中的动作还有底色");
    assert_eq!(background(&harness, "admin-action-copy"), Some(nana_ui_core::SemanticColorRole::Hover), "选中底色没有换过去");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::Admin(AdminMessage::ActionsLoaded { repo_id, result: Err("读取失败".into()) }));
    harness.flush();
    assert!(harness.find("读取失败").is_some(), "读取失败的提示没有出现");
    assert_eq!(harness.keyed("admin-action-copy"), Some(rows[1]), "读取失败不该换掉列表");
    harness.assert_same_as_fresh_mount();
}

/// 在常驻的设置、日志、拓展、动作和搜索路由之间来回切换，每一步都和新挂的一样。
#[test]
fn switching_between_resident_routes_matches_a_fresh_mount() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Logs));
    harness.assert_same_as_fresh_mount();
    for panel in [WorkspacePanel::Extensions, WorkspacePanel::Actions, WorkspacePanel::Search, WorkspacePanel::Logs] {
        harness.apply(ShellMessage::SetWorkspacePanel(panel));
        harness.model.admin.take_effects();
        harness.flush();
        harness.assert_same_as_fresh_mount();
    }
    harness.apply(ShellMessage::OpenSettings);
    harness.model.admin.take_effects();
    harness.flush();
    assert!(harness.keyed("settings-scroll").is_some(), "没有进设置页");
    harness.assert_same_as_fresh_mount();
}

/// 从文件面板切到日志面板：历史日志还在读、手上没有日志时写「正在加载系统日志」，不写空状态；
/// 读回以后换成日志列表。每一步都和新挂的一样。
#[test]
fn the_logs_panel_shows_loading_until_history_arrives() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    harness.apply(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    harness.model.admin.take_effects();
    harness.flush();
    assert!(harness.find("正在加载系统日志").is_some(), "读历史日志时要写正在加载");
    assert!(harness.find("还没有系统日志").is_none(), "读的时候不写空状态");
    harness.assert_same_as_fresh_mount();

    let ShellMessage::Host(HostMessage::LogRecorded(record)) = log(1) else {
        unreachable!("log 只造日志广播");
    };
    harness.apply(ShellMessage::LogsLoaded(Ok(SystemLogPage { records: vec![record], next_cursor: None })));
    harness.flush();
    assert!(harness.find("正在加载系统日志").is_none());
    assert!(harness.find("第 1 条后台日志").is_some(), "读回的历史日志要画出来");
    harness.assert_same_as_fresh_mount();
}
