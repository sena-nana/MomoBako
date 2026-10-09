//! 常驻启动页的回归：投影取值、无关更新不换节点、相关更新只改绑定的字段和变了的那一行，
//! 失败和重试的块按投影出现和消失，每一步都和同一 ViewModel 新挂的一样。

use super::{StartupText, StartupView};
use crate::shell::view_harness::ShellHarness;
use crate::shell::workspace::StartupStepState;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, ThumbnailFrame};

/// 首屏加载中的窗口：启动页、主区独占。
fn loading() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.begin();
    model
}

#[test]
fn projection_follows_the_startup_state() {
    let mut model = loading();
    model.workspace.startup.set_progress(2, "扫描资源库文件", "同步");
    let view = StartupView::project(&model);
    assert_eq!(view.text.title, "扫描资源库文件");
    assert_eq!(view.text.meta, "第 2 / 4 步");
    assert_eq!(view.text.detail, "同步");
    assert!(!view.text.retry);
    let states = view.steps.iter().map(|step| step.state).collect::<Vec<_>>();
    assert_eq!(states, [StartupStepState::Done, StartupStepState::Current, StartupStepState::Pending, StartupStepState::Pending]);

    model.workspace.startup.fail("索引损坏");
    let failed = StartupView::project(&model);
    assert_eq!(
        failed.text,
        StartupText {
            title: "加载失败".into(),
            meta: "第 2 / 4 步".into(),
            detail: failed.text.detail.clone(),
            error: Some("索引损坏".into()),
            retry: true,
        }
    );
    assert_eq!(failed.steps[1].state, StartupStepState::Error);
    assert_eq!(StartupView::project(&model), failed, "同一状态的投影相等");
}

/// 一串和启动页无关的更新：启动页的节点一个都不换，主区分支不重挂。
#[test]
fn unrelated_updates_keep_every_startup_node() {
    let mut harness = ShellHarness::mount(loading());
    let keys = ["workspace-startup-scroll", "startup-title", "startup-meta", "startup-progress", "startup-steps", "startup-index-1", "startup-logs"];
    let nodes = keys.map(|key| harness.keyed(key).unwrap_or_else(|| panic!("启动页缺少 {key}")));
    let branch = harness.route_branch();
    let remounts = harness.view_stats().remounts;
    let unrelated = [
        ShellMessage::ThumbnailPixels(vec![ThumbnailFrame { path: "a.png".into(), natural_width: 1, natural_height: 1, width: 1, height: 1, rgba: vec![0; 4] }]),
        ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.3)),
        ShellMessage::Inspect(crate::shell::InspectMessage::SetQuery("封面".into())),
    ];
    for message in unrelated {
        harness.apply(message);
        harness.flush();
        for (key, id) in keys.iter().zip(nodes) {
            assert_eq!(harness.keyed(key), Some(id), "无关更新换掉了启动页节点 {key}");
        }
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻启动页不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 启动推进一步：标题和步数原地改字，状态变了的两行重建，其余两行不动；失败后错误和「重试」出现。
#[test]
fn startup_progress_patches_fields_and_rebuilds_only_changed_steps() {
    let mut harness = ShellHarness::mount(loading());
    let title = harness.keyed("startup-title").expect("标题");
    let rows = (1..=4).map(|number| harness.keyed(&format!("startup-index-{number}")).expect("步骤")).collect::<Vec<_>>();
    assert!(harness.find("重试").is_none(), "加载中不显示重试");

    harness.model.workspace.startup.set_progress(2, "扫描资源库文件", "同步");
    harness.sync();
    harness.flush();
    assert_eq!(harness.keyed("startup-title"), Some(title), "标题应原地改字");
    assert!(harness.find("扫描资源库文件").is_some(), "标题没有跟上");
    assert!(harness.find("第 2 / 4 步").is_some(), "步数没有跟上");
    let after = (1..=4).map(|number| harness.keyed(&format!("startup-index-{number}")).expect("步骤")).collect::<Vec<_>>();
    assert_ne!(after[0], rows[0], "第 1 步变成完成，整行重建");
    assert_ne!(after[1], rows[1], "第 2 步变成当前，整行重建");
    assert_eq!(after[2..], rows[2..], "状态没变的步骤不该重建");
    harness.assert_same_as_fresh_mount();

    harness.model.workspace.startup.fail("索引损坏");
    harness.sync();
    harness.flush();
    assert_eq!(harness.keyed("startup-title"), Some(title));
    assert!(harness.find("索引损坏").is_some(), "错误没有显示");
    assert!(harness.find("重试").is_some(), "失败后要能重试");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::StartupRetry);
    harness.flush();
    assert!(harness.find("重试").is_none(), "重试后按钮藏起来");
    harness.assert_same_as_fresh_mount();
}

/// 启动完成：路由从启动页换到首页，排法从主区独占换成工作台；再出错回到启动页时按新状态建。
#[test]
fn leaving_and_reentering_the_startup_route_follows_the_model() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Loading));
    harness.assert_same_as_fresh_mount();
    let mut ready = ShellViewModel::for_page(ShellPage::FileList);
    ready.set_viewport_width(1200.0);
    harness.model = ready;
    harness.sync();
    harness.flush();
    assert!(harness.keyed("startup-title").is_none(), "就绪后不该还有启动页");
    harness.assert_same_as_fresh_mount();
}
