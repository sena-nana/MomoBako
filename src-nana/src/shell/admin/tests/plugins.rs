//! 设置包读取、插件分组搜索删除、插件标签、插件字段与设置、启用与安装提示的测试。

use super::*;

#[test]
fn settings_bundle_is_all_or_nothing_and_success_clears_the_error() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::OpenSettings);
    assert!(model.admin.loading_settings);
    assert!(
        model
            .admin
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle))
    );
    send(
        &mut model,
        AdminMessage::SettingsBundleLoaded {
            plugins: Ok(vec![plugin("next", "system", "service", "service")]),
            hooks: Ok(Vec::new()),
            api: Err("API 设计读取失败".into()),
            external: Ok(connection()),
        },
    );
    assert!(!model.admin.loading_settings);
    assert!(model.admin.plugins.is_empty());
    assert!(model.admin.external.is_none());
    assert_eq!(model.admin.load_error, "API 设计读取失败");

    let tools = tool_plugin(
        "momobako.tool.file-manager",
        json!([{ "toolPageId": "momobako.tool.file-manager", "label": "文件导入" }]),
    );
    load_bundle(
        &mut model,
        vec![plugin("next", "system", "service", "service"), tools],
    );
    assert_eq!(model.admin.plugins.len(), 2);
    assert_eq!(
        model.admin.external.as_ref().map(|item| item.ready),
        Some(true)
    );
    assert!(model.admin.load_error.is_empty());
    assert_eq!(
        model
            .admin
            .tool_pages
            .iter()
            .map(|page| page.id.as_str())
            .collect::<Vec<_>>(),
        ["momobako.tool.file-manager"]
    );
    assert_eq!(
        model.admin.active_tool_page_id.as_deref(),
        Some("momobako.tool.file-manager")
    );
}

#[test]
fn plugins_group_search_and_confirm_delete() {
    let mut model = ShellViewModel::default();
    let mut user = plugin("user.one", "user", "source", "filesystem");
    user.name = "导入".into();
    let mut custom = plugin("custom.one", "system", "made-up", "service");
    custom.capabilities = vec!["only-custom".into()];
    let parser = plugin("parser.one", "", "", "parser");
    load_bundle(&mut model, vec![user, custom, parser]);
    assert_eq!(
        model
            .admin
            .grouped_plugins()
            .into_iter()
            .map(|(category, _)| category)
            .collect::<Vec<_>>(),
        ["source", "parser", "unclassified"]
    );
    send(&mut model, AdminMessage::SetKeyword("only-custom".into()));
    assert_eq!(
        model.admin.grouped_plugins(),
        vec![("unclassified".into(), vec!["custom.one".into()])]
    );

    send(&mut model, AdminMessage::RequestDelete("parser.one".into()));
    assert!(model.admin.pending_delete.is_none());
    send(&mut model, AdminMessage::RequestDelete("user.one".into()));
    assert_eq!(model.admin.pending_delete.as_deref(), Some("user.one"));
    send(&mut model, AdminMessage::CancelDelete);
    assert!(model.admin.pending_delete.is_none());
    send(&mut model, AdminMessage::RequestDelete("user.one".into()));
    send(&mut model, AdminMessage::ConfirmDelete);
    assert!(model.admin.managing);
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeletePlugin(id)) if id == "user.one")
    );
    send(
        &mut model,
        AdminMessage::PluginsReplaced(Err("删除被拒绝".into())),
    );
    assert_eq!(model.admin.pending_delete.as_deref(), Some("user.one"));
    assert_eq!(model.admin.action_error, "删除被拒绝");
    send(&mut model, AdminMessage::ConfirmDelete);
    send(
        &mut model,
        AdminMessage::PluginsReplaced(Ok(vec![plugin("parser.one", "", "parser", "parser")])),
    );
    assert_eq!(model.admin.action_message, "插件已删除。");
    assert!(model.admin.action_error.is_empty());
    assert!(model.admin.pending_delete.is_none());
    assert!(!model.admin.managing);
}

#[test]
fn plugin_labels_follow_the_taxonomy() {
    let mut manifest = plugin("user.one", "user", "", "webdav");
    assert_eq!(support::plugin_category(&manifest), "source");
    assert_eq!(support::category_label("library-kind"), "库类型");
    assert_eq!(support::category_label("made-up"), "未分类");
    assert_eq!(support::plugin_runtime_label("vue-module"), "Vue 模块");
    assert_eq!(support::plugin_runtime_label("process"), "未知运行时");
    assert_eq!(support::plugin_source_label("user"), "用户插件");
    manifest.requires = vec!["a".into(), "b".into()];
    assert_eq!(support::dependency_label(&manifest), "必需 2 / 可选 0");
    assert_eq!(support::dependency_status_label("missing"), "缺失");
    assert_eq!(support::hook_status_label("blocked"), "已拦截");
    assert_eq!(support::plugin_status_label(&manifest), "已启用");
    manifest.degraded = true;
    assert_eq!(support::plugin_status_label(&manifest), "降级运行");
    manifest.enabled = false;
    assert_eq!(support::plugin_status_label(&manifest), "未启用");
    manifest.status = "error".into();
    assert_eq!(support::plugin_status_label(&manifest), "错误");
    assert_eq!(support::settings_fields(&manifest).len(), 4);
    assert_eq!(support::plugin_settings_label(&manifest), "插件设置");
    manifest.contributes["settings"]["settingsPage"] =
        json!({ "label": "下载服务", "description": "队列和目录" });
    assert_eq!(support::plugin_settings_label(&manifest), "下载服务");
    assert_eq!(
        support::plugin_settings_description(&manifest),
        "队列和目录"
    );
    manifest.contributes = json!({ "source": { "authentication": { "kind": "qr" } } });
    assert_eq!(support::plugin_settings_label(&manifest), "账号与来源");
    assert_eq!(
        support::plugin_settings_description(&manifest),
        "管理来源账号、登录状态与关联仓库。"
    );
}

#[test]
fn plugin_fields_reject_bad_json_and_reset_empty_numbers() {
    let mut model = ShellViewModel::default();
    load_bundle(
        &mut model,
        vec![plugin("user.one", "user", "service", "service")],
    );
    send(
        &mut model,
        AdminMessage::JsonDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
            value: "{".into(),
        },
    );
    send(
        &mut model,
        AdminMessage::CommitDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
        },
    );
    assert_eq!(model.admin.action_error, "原始 不是有效 JSON。");
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        AdminMessage::JsonDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
            value: " ".into(),
        },
    );
    send(
        &mut model,
        AdminMessage::CommitDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
        },
    );
    assert!(
        model.admin.take_effects().is_empty(),
        "和现在的值（空）显示的一样，不保存"
    );
    send(
        &mut model,
        AdminMessage::JsonDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
            value: "[1]".into(),
        },
    );
    send(
        &mut model,
        AdminMessage::CommitDraft {
            plugin_id: "user.one".into(),
            key: "raw".into(),
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!([1]))
    );
    model.reduce(ShellMessage::PluginConfigLoaded(Err("写入失败".into())));
    assert_eq!(model.admin.action_error, "写入失败");

    send(
        &mut model,
        AdminMessage::ConfigInput {
            plugin_id: "user.one".into(),
            key: "limit".into(),
            text: "  ".into(),
            checked: None,
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeleteConfig { key, .. }) if key == "limit")
    );
    send(
        &mut model,
        AdminMessage::ConfigInput {
            plugin_id: "user.one".into(),
            key: "limit".into(),
            text: "12".into(),
            checked: None,
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!(12.0))
    );
    send(
        &mut model,
        AdminMessage::ConfigInput {
            plugin_id: "user.one".into(),
            key: "on".into(),
            text: String::new(),
            checked: Some(true),
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!(true))
    );
    send(
        &mut model,
        AdminMessage::ConfigInput {
            plugin_id: "user.one".into(),
            key: "mode".into(),
            text: "\"a\"".into(),
            checked: None,
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!("a"))
    );
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "user.one".into(),
        data_directory: "data".into(),
        schema: serde_json::Value::Null,
        values: [("mode".into(), json!("a"))].into_iter().collect(),
    })));
    assert_eq!(model.admin.action_message, "插件设置已保存。");
    send(
        &mut model,
        AdminMessage::ResetConfig {
            plugin_id: "user.one".into(),
            key: "mode".into(),
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeleteConfig { key, .. }) if key == "mode")
    );
    model.reduce(ShellMessage::PluginConfigLoaded(Err(String::new())));
    assert_eq!(model.admin.action_error, "插件设置重置失败。");
}

#[test]
fn plugin_settings_toggle_route_and_directory() {
    let mut model = ShellViewModel::default();
    load_bundle(
        &mut model,
        vec![plugin("user.one", "user", "service", "service")],
    );
    send(&mut model, AdminMessage::RoutePlugin("  ".into()));
    send(&mut model, AdminMessage::RoutePlugin("missing".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert_eq!(
        model.admin.active_settings_plugin_id.as_deref(),
        Some("user.one")
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::LoadConfig(id)) if id == "user.one")
    );
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "user.one".into(),
        data_directory: "data".into(),
        schema: serde_json::Value::Null,
        values: [("raw".into(), json!({"ok": true}))].into_iter().collect(),
    })));
    assert!(model.admin.json_drafts["user.one"]["raw"].contains("ok"));
    assert!(!model.admin.managing);
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert!(model.admin.take_effects().is_empty());

    send(
        &mut model,
        AdminMessage::OpenDataDirectory("user.one".into()),
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::OpenDataDirectory { name, .. }) if name == "user.one")
    );
    send(
        &mut model,
        AdminMessage::DataDirectoryFinished {
            name: "user.one".into(),
            result: Ok("C:/plugin".into()),
        },
    );
    assert!(
        model.admin.action_message.is_empty(),
        "宿主打开以后才写已打开"
    );
    crate::host_bridge::perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
    assert_eq!(model.admin.action_message, "已打开“user.one”设置目录。");
    send(
        &mut model,
        AdminMessage::DataDirectoryFinished {
            name: "user.one".into(),
            result: Err(String::new()),
        },
    );
    assert_eq!(model.admin.action_error, "插件设置目录打开失败。");

    send(&mut model, AdminMessage::ToggleSettings("user.one".into()));
    assert!(model.admin.active_settings_plugin_id.is_none());
}

#[test]
fn enable_and_install_report_their_messages() {
    let mut model = ShellViewModel::default();
    load_bundle(
        &mut model,
        vec![plugin("user.one", "user", "service", "service")],
    );
    send(
        &mut model,
        AdminMessage::SetEnabled {
            plugin_id: "user.one".into(),
            enabled: false,
        },
    );
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::SetEnabled { enabled: false, .. })
    ));
    send(
        &mut model,
        AdminMessage::PluginsReplaced(Ok(vec![plugin("user.one", "user", "service", "service")])),
    );
    assert_eq!(model.admin.action_message, "插件已禁用。");
    send(&mut model, AdminMessage::ChooseArchive);
    assert!(model.admin.action_message.is_empty());
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::RequestOpenDialog)
    ));
    send(&mut model, AdminMessage::InstallArchive(Some("  ".into())));
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        AdminMessage::InstallArchive(Some("plugin.momoplug".into())),
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(AdminEffect::Install(path)) if path == "plugin.momoplug")
    );
    send(
        &mut model,
        AdminMessage::PluginsReplaced(Err(String::new())),
    );
    assert_eq!(model.admin.action_error, "插件安装失败。");
}
