//! 常驻缺失仓库页的回归：投影取值；无关更新不换节点、不重挂；重定向输入出现、打字、提交和失败时
//! 只改绑定的字段，输入框还是同一个节点、字不丢；每一步都和同一 ViewModel 新挂的一样。

use super::MissingView;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel, ThumbnailFrame};

fn missing() -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "missing")
        .expect("缺失仓库场景")
        .1
}

/// 节点的无障碍状态里是不是禁用。
fn disabled(harness: &ShellHarness, key: &str) -> bool {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("缺失仓库页缺少 {key}"));
    harness.document().context().world().accessibility(id).is_some_and(|state| state.disabled)
}

#[test]
fn projection_follows_the_missing_state() {
    let mut model = missing();
    let view = MissingView::project(&model);
    assert_eq!(view.name, "默认资源库");
    assert!(!view.path.is_empty());
    assert!(!view.cache_issue && !view.busy && !view.prompt);
    assert!(view.error.is_empty());
    assert_eq!((view.primary, view.delete), ("重定向", "删除资源库"));
    assert_eq!(MissingView::project(&model), view, "同一状态的投影相等");

    model.workspace.choose_missing_path();
    assert!(MissingView::project(&model).prompt, "选了重定向以后显示输入");

    let repo_id = model.workspace.active_repo_id.clone().expect("场景有仓库");
    let repository = model.workspace.repositories.iter_mut().find(|item| item.repo_id == repo_id).expect("当前仓库");
    repository.cache_required = true;
    let cached = MissingView::project(&model);
    assert!(cached.cache_issue);
    assert!(!cached.prompt, "来源缓存问题不显示重定向输入");
    assert_eq!(cached.primary, "打开来源设置");
}

/// 和缺失仓库页无关的更新：节点一个都不换，主区分支和侧栏都不重挂。
#[test]
fn unrelated_updates_keep_every_missing_node() {
    let mut harness = ShellHarness::mount(missing());
    let keys = [
        "workspace-missing-scroll",
        "missing-name",
        "missing-summary",
        "missing-path",
        "missing-error",
        "missing-path-input",
        "missing-primary",
        "missing-refresh",
        "missing-delete",
    ];
    let nodes = keys.map(|key| harness.keyed(key).unwrap_or_else(|| panic!("缺失仓库页缺少 {key}")));
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
            assert_eq!(harness.keyed(key), Some(id), "无关更新换掉了缺失仓库页节点 {key}");
        }
    }
    assert_eq!(harness.route_branch(), branch, "常驻路由的分支不该重挂");
    assert_eq!(harness.view_stats().remounts, remounts, "常驻缺失仓库页不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 刚打的字还没归约，先到了一条后台消息：同步写回的是 ViewModel 里较旧的草稿，但草稿相对上次投影
/// 没变，不该写回信号把刚打的字冲掉。
#[test]
fn a_background_message_does_not_roll_back_typed_text() {
    let mut harness = ShellHarness::mount(missing());
    harness.model.workspace.choose_missing_path();
    harness.sync();
    harness.flush();
    let field = harness.input("资源库新位置");
    harness.focus(field);
    harness.type_text("E:");
    let typed = harness.take_messages();
    harness.apply(ShellMessage::Player(crate::shell::player::PlayerMessage::SetVolume(0.4)));
    harness.flush();
    assert_eq!(harness.value(field), "E:", "后台消息把刚打的字冲掉了");
    for message in typed {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.value(field), "E:");
    assert_eq!(harness.model.workspace.path_draft, "E:");
    harness.assert_same_as_fresh_mount();
}

/// 重定向输入：出现后打两个字，消息归约同步以后字还在、还是同一个输入框；提交后按钮禁用、
/// 主按钮写「重定向中...」、输入藏起来；失败后错误条出现，按钮恢复。
#[test]
fn relocation_patches_fields_and_keeps_the_input() {
    let mut harness = ShellHarness::mount(missing());
    assert!(harness.find("资源库新位置").is_none(), "没选重定向时不显示输入");
    harness.model.workspace.choose_missing_path();
    harness.sync();
    harness.flush();
    let field = harness.input("资源库新位置");
    assert_eq!(harness.keyed("missing-path-input"), Some(field), "输入框应是建好的那一个，只是显示出来");
    harness.assert_same_as_fresh_mount();

    harness.focus(field);
    harness.type_text("D:");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.type_text("/新");
    for message in harness.take_messages() {
        harness.apply(message);
    }
    harness.flush();
    assert_eq!(harness.focused(), Some(field), "打字时焦点不该离开输入框");
    assert_eq!(harness.value(field), "D:/新");
    assert_eq!(harness.model.workspace.path_draft, "D:/新");
    harness.assert_same_as_fresh_mount();

    let primary = harness.keyed("missing-primary").expect("主按钮");
    harness.apply(ShellMessage::MissingSubmitPath);
    harness.flush();
    assert!(harness.find("资源库新位置").is_none(), "提交后输入藏起来");
    assert_eq!(harness.keyed("missing-primary"), Some(primary), "主按钮原地改字");
    assert!(harness.find("重定向中...").is_some(), "主按钮没有写「重定向中...」");
    assert!(disabled(&harness, "missing-primary") && disabled(&harness, "missing-refresh") && disabled(&harness, "missing-delete"));
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::MissingRelocateFinished(Err("路径无效".into())));
    harness.flush();
    assert!(harness.find("路径无效").is_some(), "错误条没有显示");
    assert!(!disabled(&harness, "missing-primary"), "失败后按钮恢复");
    harness.assert_same_as_fresh_mount();
}
