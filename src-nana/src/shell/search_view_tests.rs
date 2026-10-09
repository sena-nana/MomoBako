//! 搜索面板和筛选栏挂到生产壳层后的语义节点：分组名、芯片、输入框名称、禁用态和文案。

use nana_ui::runtime::{AccessibilityNode, AccessibilityRole};

use super::super::acceptance_gap_models;

/// 挂载同名对照场景，取出全部语义节点。
fn nodes(scene: &str) -> Vec<AccessibilityNode> {
    let model = acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == scene)
        .map(|(_, model)| model)
        .unwrap_or_else(|| panic!("没有对照场景 {scene}"));
    let document = crate::acceptance_document_for_model(model).expect("生产文档");
    document.context().world().project_accessibility(document.document())
}

fn find<'a>(nodes: &'a [AccessibilityNode], label: &str) -> Vec<&'a AccessibilityNode> {
    nodes.iter().filter(|node| node.label.as_deref() == Some(label)).collect()
}

fn has(nodes: &[AccessibilityNode], label: &str) -> bool {
    !find(nodes, label).is_empty()
}

fn button<'a>(nodes: &'a [AccessibilityNode], label: &str) -> &'a AccessibilityNode {
    find(nodes, label)
        .into_iter()
        .find(|node| node.role == AccessibilityRole::Button)
        .unwrap_or_else(|| panic!("没有按钮 {label}"))
}

#[test]
fn active_filter_bar_lists_groups_candidates_and_inputs() {
    let nodes = nodes("filter-bar-active");
    for group in ["格式筛选", "文件标签筛选", "文件颜色筛选", "形状筛选", "评分筛选", "高级筛选"] {
        assert!(
            find(&nodes, group).iter().any(|node| node.role == AccessibilityRole::Toolbar),
            "缺少分组 {group}"
        );
    }
    assert!(!has(&nodes, "库类型筛选"), "没有库类型条目时不显示快捷方式");
    for chip in ["pdf", "png", "参考", "封面", "文档", "#3fa796", "红色", "横版", "竖版", "全部", "1 星+", "3 星+", "5 星+"] {
        button(&nodes, chip);
    }
    assert!(has(&nodes, "当前资源库筛选"));
    assert!(has(&nodes, "3 个条件"));
    assert!(!button(&nodes, "清除").disabled, "有条件时可以清除");
    button(&nodes, "关闭筛选栏");
    button(&nodes, "应用");
    let adds = find(&nodes, "添加");
    assert_eq!(adds.len(), 2, "颜色和形状各一个「添加」");
    assert!(adds.iter().all(|node| node.disabled), "输入为空时「添加」禁用");
    for input in [
        "输入文件颜色",
        "输入形状",
        "排除关键词",
        "排除路径",
        "排除标签",
        "排除格式",
        "元数据",
        "排除元数据",
        "排除数值范围",
        "排除日期范围",
        "数值范围",
        "日期范围",
        "排序字段",
        "结果数量",
    ] {
        assert!(find(&nodes, input).iter().any(|node| node.editable), "缺少输入框 {input}");
    }
    assert!(!has(&nodes, "加入筛选"), "Vue 没有「加入筛选」");
}

#[test]
fn plain_filter_bar_hides_empty_groups_and_disables_clear() {
    let nodes = nodes("filter-bar");
    assert!(has(&nodes, "格式筛选"), "格式候选来自仓库摘要");
    assert!(!has(&nodes, "文件标签筛选"), "没有标签候选时不显示标签组");
    assert!(has(&nodes, "文件颜色筛选") && has(&nodes, "形状筛选"), "颜色和形状组总在");
    assert!(!nodes.iter().any(|node| node.label.as_deref().is_some_and(|label| label.ends_with("个条件"))));
    assert!(button(&nodes, "清除").disabled, "没有条件也没有查询时清除禁用");
    assert!(has(&nodes, "等待搜索条件"));
    assert!(has(&nodes, "全局搜索"));
}

#[test]
fn results_panel_shows_scope_summary_counts_and_hit_context() {
    let nodes = nodes("search-results");
    for label in ["默认资源库内筛选", "搜索结果", "当前资源库筛选: 封面", "1 个仓库", "1 条结果", "默认资源库 / cover.png"] {
        assert!(has(&nodes, label), "缺少 {label}");
    }
    assert!(find(&nodes, "cover.png").iter().any(|node| node.role == AccessibilityRole::ListItem), "结果行可以点");
    for chip in ["png", "封面", "参考", "红色", "横版", "4 星"] {
        assert!(has(&nodes, chip), "缺少结果芯片 {chip}");
    }
    assert!(!has(&nodes, "等待搜索条件"));
}

/// 在离屏会话里点筛选栏：芯片、颜色输入回车、评分和清除都走生产消息链路。
#[test]
fn clicking_chips_and_submitting_inputs_drive_the_search() {
    use nana_ui_devtools::agent::RuntimeAgentSession;

    let mut model = acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "filter-bar")
        .map(|(_, model)| model)
        .expect("filter-bar 场景");
    let document = crate::acceptance_document_for_model(model.clone()).expect("生产文档");
    let mut session = RuntimeAgentSession::new(document, 1200, 800).expect("离屏会话");
    session.flush().expect("布局");

    let pump = |session: &mut RuntimeAgentSession, model: &mut super::super::ShellViewModel, step: &str| {
        let queued = session.document_mut().context_mut().take_program_messages();
        assert!(!queued.is_empty(), "{step} 没有进入程序消息");
        for message in queued {
            let message = message.downcast::<super::super::ShellMessage>().expect("壳层消息");
            super::super::commit_interaction(model, *message, nana_ui_platform::WindowId(1), false);
        }
        super::super::mount_shell(session.document_mut(), model).expect("重新挂载");
        session.flush().expect("重新布局");
    };
    let node_id = |session: &RuntimeAgentSession, label: &str, role: &str| {
        session
            .accessibility_dump()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(label) && node.role == role)
            .map(|node| node.id)
            .unwrap_or_else(|| panic!("没有 {role} {label}"))
    };

    let png = node_id(&session, "png", "button");
    assert!(session.click_node(png).expect("点 png"));
    pump(&mut session, &mut model, "点 png");
    assert_eq!(model.inspect.filters.formats, ["png"]);
    assert!(has(&session_nodes(&session), "1 个条件"));

    let color = node_id(&session, "输入文件颜色", "text-input");
    assert!(session.click_node(color).expect("点颜色输入"));
    session.type_text("青色").expect("输入颜色");
    pump(&mut session, &mut model, "输入颜色");
    assert_eq!(model.inspect.search_ui.draft.color, "青色");
    // mount_shell 重挂后按键路径恢复焦点，回车直接落在颜色输入框上。
    let color = node_id(&session, "输入文件颜色", "text-input");
    assert!(
        session.accessibility_dump().iter().any(|node| node.id == color && node.focused),
        "重挂后颜色输入框应保持焦点"
    );
    session.key_press("Enter", "Enter", Default::default()).expect("回车提交");
    pump(&mut session, &mut model, "回车提交");
    assert_eq!(model.inspect.filters.colors, ["青色"], "回车按「添加」提交颜色");
    assert!(model.inspect.search_ui.draft.color.is_empty(), "提交后清空输入");
    assert!(session.accessibility_dump().iter().any(|node| node.label.as_deref() == Some("青色") && node.role == "button"));

    let rating = node_id(&session, "4 星+", "button");
    assert!(session.click_node(rating).expect("点 4 星+"));
    pump(&mut session, &mut model, "点 4 星+");
    assert_eq!(model.inspect.filters.min_rating, Some(4.0));

    let clear = node_id(&session, "清除", "button");
    assert!(session.click_node(clear).expect("点清除"));
    pump(&mut session, &mut model, "点清除");
    assert_eq!(model.inspect.active_filter_count(), 0);
    assert!(model.inspect.filter_bar_open, "清除不收起筛选栏");

    let close = node_id(&session, "关闭筛选栏", "button");
    assert!(session.click_node(close).expect("点关闭"));
    pump(&mut session, &mut model, "点关闭");
    assert!(!model.inspect.filter_bar_open);
    assert!(!has(&session_nodes(&session), "当前资源库筛选"));
}

fn session_nodes(session: &nana_ui_devtools::agent::RuntimeAgentSession) -> Vec<AccessibilityNode> {
    session.document().context().world().project_accessibility(session.document().document())
}

#[test]
fn empty_search_keeps_the_filter_bar_closed() {
    let nodes = nodes("search-empty");
    assert!(has(&nodes, "等待搜索条件"));
    assert!(has(&nodes, "当前查询: 不存在的文件"));
    assert!(has(&nodes, "0 条结果"));
    assert!(!has(&nodes, "当前资源库筛选"), "筛选栏关闭");
}
