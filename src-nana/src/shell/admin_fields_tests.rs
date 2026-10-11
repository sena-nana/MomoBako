//! 插件设置字段在真实视图里的回归：文本字段改完不按回车、焦点一离开就保存（Vue 文本框的 `@change`），
//! JSON 文本域失焦保存（Vue `@blur`；多行框按回车只换行，回车提交走不到）；没改过的字段失焦不保存。

use serde_json::json;

use crate::shell::admin::style::key_part;
use crate::shell::admin::AdminEffect;
use crate::shell::view_harness::ShellHarness;
use crate::shell::ShellViewModel;

/// 网易云来源插件：设置里有「API Base URL」这些文本字段。
const NETEASE: &str = "momobako.netease.source";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 归约视图这段时间发出的消息。
fn settle(harness: &mut ShellHarness) {
    let messages = harness.take_messages();
    harness.settle(messages);
}

/// 排下的插件设置保存：字段键和值。
fn saved(harness: &mut ShellHarness) -> Vec<(String, serde_json::Value)> {
    harness
        .model
        .admin
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            AdminEffect::SetConfig { key, value, .. } => Some((key, value)),
            _ => None,
        })
        .collect()
}

/// 焦点挪到插件搜索框上，字段失焦。
fn leave_field(harness: &mut ShellHarness) {
    let keyword = harness.input_within("admin-plugin-keyword");
    harness.focus(keyword);
    settle(harness);
}

/// 文本字段：没改过时失焦不保存；打字时不保存，改完点到别处（不按回车）保存这次的值。
#[test]
fn a_text_field_saves_its_draft_when_focus_leaves() {
    let mut harness = ShellHarness::mount(scene("source-auth-gap"));
    let input = harness.input("API Base URL");
    harness.focus(input);
    leave_field(&mut harness);
    assert!(saved(&mut harness).is_empty(), "没改过的字段失焦不保存");

    harness.focus(input);
    harness.type_text("https://ncm.example");
    settle(&mut harness);
    assert!(saved(&mut harness).is_empty(), "打字时不保存");
    leave_field(&mut harness);
    assert_eq!(saved(&mut harness), [("apiBaseUrl".to_string(), json!("https://ncm.example"))], "失焦应该保存草稿");
}

/// JSON 字段：多行框里改完点到别处保存，按 JSON 解析以后写进配置。
#[test]
fn a_json_field_saves_when_focus_leaves() {
    let mut model = scene("source-auth-gap");
    let plugin = model.admin.plugins.iter_mut().find(|plugin| plugin.plugin_id == NETEASE).expect("网易云插件");
    plugin.contributes["settings"]["fields"]
        .as_array_mut()
        .expect("设置字段")
        .push(json!({ "key": "headers", "label": "请求头", "type": "json" }));
    let mut harness = ShellHarness::mount(model);
    let area = harness.keyed(&format!("admin-field-json-{}-headers", key_part(NETEASE))).expect("JSON 文本域");
    harness.focus(area);
    harness.type_text("{\"x\": 1}");
    settle(&mut harness);
    assert!(saved(&mut harness).is_empty(), "打字时不保存");
    leave_field(&mut harness);
    assert_eq!(saved(&mut harness), [("headers".to_string(), json!({ "x": 1 }))], "失焦应该保存 JSON");
}
