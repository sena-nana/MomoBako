//! 常驻搜索路由的回归：筛选栏输入框在组合输入中不被打断、打字不重挂，无关更新一个节点都不换，
//! 再搜一次只换变了的结果行，芯片的选中态原地改；每一步都和同一 ViewModel 新挂的文档一样。

use nana_ui::runtime::StableNodeId;
use nana_ui_core::SemanticColorRole;

use crate::shell::inspect::{FilterList, InspectMessage, SearchRow};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{InspectEffect, ShellMessage, ShellViewModel, ThumbnailFrame};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 和搜索面板、筛选栏无关的消息：缩略图像素、播放音量、任务进度。
fn unrelated_messages() -> Vec<ShellMessage> {
    vec![
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame { path: "a.png".into(), natural_width: 1, natural_height: 1, width: 1, height: 1, rgba: vec![0; 4] }]),
        ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.3)),
        ShellMessage::TaskProgressLoaded(Vec::new()),
    ]
}

/// 跑完一次搜索：结果换成 `rows`。
fn finish_search(harness: &mut ShellHarness, rows: Vec<SearchRow>) {
    harness.apply(ShellMessage::Inspect(InspectMessage::RunSearch));
    let generation = match harness.model.inspect.take_effects().pop() {
        Some(InspectEffect::Search { generation, .. }) => generation,
        other => panic!("没有搜索请求：{other:?}"),
    };
    harness.apply(ShellMessage::Inspect(InspectMessage::SearchFinished { generation, result: Ok(rows) }));
    harness.flush();
}

fn chip_foreground(harness: &ShellHarness, chip: StableNodeId) -> Option<SemanticColorRole> {
    harness.document().context().world().node_style(chip).and_then(|style| style.foreground)
}

/// 筛选栏颜色输入框正在组合输入：无关消息和改了筛选条件的消息都不换输入框节点、不断预编辑，
/// 主区一次都不重挂；提交后字落在原节点上。
#[test]
fn filter_input_keeps_its_preedit_through_updates() {
    let mut harness = ShellHarness::mount(scene("filter-bar-active"));
    let input = harness.input("输入文件颜色");
    harness.focus(input);
    harness.compose("hong");
    let remounts = harness.view_stats().remounts;
    let mut messages = unrelated_messages();
    messages.push(ShellMessage::Inspect(InspectMessage::ToggleFilter { key: FilterList::Formats, value: "pdf".into() }));
    for message in messages {
        harness.apply(message);
        harness.flush();
        assert_eq!(harness.input("输入文件颜色"), input, "更新换掉了颜色输入框");
        assert_eq!(harness.preedit(input).as_deref(), Some("hong"), "更新打断了预编辑");
        assert_eq!(harness.focused(), Some(input), "更新以后焦点离开了输入框");
    }
    assert_eq!(harness.view_stats().remounts, remounts, "常驻搜索路由不该重挂");
    harness.commit("红");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.input("输入文件颜色"), input);
    assert_eq!(harness.value(input), "红");
    assert_eq!(harness.model.inspect.search_ui.draft.color, "红");
    harness.assert_same_as_fresh_mount();
}

/// 高级区输入框打字：消息归约同步以后还是原来的节点，紧接着的按键接着打，后台消息排在按键前面
/// 也不回滚刚打的字。
#[test]
fn typing_in_an_advanced_input_stays_on_the_same_node() {
    let mut harness = ShellHarness::mount(scene("filter-bar"));
    let input = harness.input("元数据");
    harness.focus(input);
    harness.type_text("a");
    let queued = harness.take_messages();
    harness.apply(unrelated_messages().remove(0));
    harness.flush();
    assert_eq!(harness.value(input), "a", "后台消息的同步把刚打的字回滚了");
    for message in queued {
        harness.apply(message);
    }
    harness.type_text("b");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.focused(), Some(input), "打字以后焦点不在原输入框上");
    assert_eq!(harness.value(input), "ab");
    assert_eq!(harness.model.inspect.search_ui.draft.metadata, "ab");
    harness.assert_same_as_fresh_mount();
}

/// 一串无关更新：搜索面板、结果行和筛选栏的节点一个都不换，主区分支不重挂。
#[test]
fn unrelated_updates_keep_every_search_node() {
    let mut harness = ShellHarness::mount(scene("search-results"));
    let keys = [
        "workspace-page-scroll-HasRepository-Search",
        "search-workbench-panel",
        "search-workbench-summary",
        "search-hit-acceptance-repo:cover.png",
        "workspace-filter-bar",
        "workspace-filter-format-png",
        "workspace-filter-color-input",
        "workspace-filter-sort-direction",
    ];
    let nodes = keys.map(|key| harness.keyed(key).unwrap_or_else(|| panic!("搜索页缺少 {key}")));
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    for message in unrelated_messages() {
        harness.apply(message);
        harness.flush();
        for (key, id) in keys.iter().zip(nodes) {
            assert_eq!(harness.keyed(key), Some(id), "无关更新换掉了 {key}");
        }
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts);
    harness.assert_same_as_fresh_mount();
}

/// 再搜一次：留下的结果行节点不换，内容变了的字段原地改；新结果多出一行；结果清空时空状态出现。
#[test]
fn a_new_search_keeps_the_rows_it_still_has() {
    let mut harness = ShellHarness::mount(scene("search-results"));
    let cover = harness.model.inspect.results[0].clone();
    let row = harness.keyed("search-hit-acceptance-repo:cover.png").expect("结果行");
    let mut retagged = cover.clone();
    retagged.tags = vec!["参考".into()];
    let mut page = cover.clone();
    page.asset_id = "notes/page.pdf".into();
    page.path = "notes/page.pdf".into();
    page.filename = "page.pdf".into();
    finish_search(&mut harness, vec![retagged, page]);
    assert_eq!(harness.keyed("search-hit-acceptance-repo:cover.png"), Some(row), "留下的结果行被重建了");
    assert!(harness.find("page.pdf").is_some(), "新结果没有出现");
    assert!(harness.find("2 条结果").is_some(), "结果数没有跟上");
    harness.assert_same_as_fresh_mount();

    finish_search(&mut harness, Vec::new());
    assert!(harness.find("没有匹配的文件").is_some(), "结果清空后没有空状态");
    harness.assert_same_as_fresh_mount();
}

/// 点选格式芯片：芯片还是原来的节点，选中态的颜色原地换；「N 个条件」跟着变。
#[test]
fn toggling_a_chip_restyles_it_in_place() {
    let mut harness = ShellHarness::mount(scene("filter-bar-active"));
    let chip = harness.keyed("workspace-filter-format-pdf").expect("pdf 芯片");
    assert_eq!(chip_foreground(&harness, chip), Some(SemanticColorRole::Muted), "没选时是次要字色");
    harness.apply(ShellMessage::Inspect(InspectMessage::ToggleFilter { key: FilterList::Formats, value: "pdf".into() }));
    harness.flush();
    assert_eq!(harness.keyed("workspace-filter-format-pdf"), Some(chip), "芯片被重建了");
    assert_eq!(chip_foreground(&harness, chip), Some(SemanticColorRole::Accent), "选中以后没有换成强调色");
    assert!(harness.find("4 个条件").is_some());
    harness.assert_same_as_fresh_mount();
}

/// 关上再打开筛选栏：节点留着、只换显隐，关着时不在无障碍树里，打开后和新挂的一样。
#[test]
fn closing_and_reopening_the_filter_bar_keeps_its_nodes() {
    let mut harness = ShellHarness::mount(scene("filter-bar"));
    let bar = harness.keyed("workspace-filter-bar").expect("筛选栏");
    harness.apply(ShellMessage::Inspect(InspectMessage::CloseFilterBar));
    harness.flush();
    assert!(harness.find("当前资源库筛选").is_none(), "关上以后筛选栏还在无障碍树里");
    harness.assert_same_as_fresh_mount();
    harness.apply(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
    harness.flush();
    assert_eq!(harness.keyed("workspace-filter-bar"), Some(bar), "重新打开换了筛选栏节点");
    assert!(harness.find("当前资源库筛选").is_some());
    harness.assert_same_as_fresh_mount();
}
