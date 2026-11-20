//! 插件列表发布、配置应用、设置页与删除确认、字段编辑与草稿提交、插件包安装。

use super::*;

/// 插件列表换新：重算筛选栏的库类型快捷方式，再重读插件登记的播放器类型。
/// 插件列表变了，登记的播放器也可能变，所以每次都重读。
pub(super) fn publish_plugins(model: &mut ShellViewModel, plugins: &[PluginManifest]) {
    model.inspect.shortcuts =
        super::super::super::inspect_shortcuts::shortcuts_from_plugins(plugins);
    model.admin.push_effect(AdminEffect::LoadPlaylistPlayers);
}

pub(super) fn apply_config(model: &mut ShellViewModel, config: &PluginConfigSnapshot) {
    model
        .admin
        .config_snapshots
        .insert(config.plugin_id.clone(), config.clone());
    model.admin.sync_json_drafts(&config.plugin_id);
}

/// 打开设置页：读设置包和应用设置。
/// 带插件时展开它的设置，插件列表里还没有它时不展开。
pub(crate) fn open_settings_page(model: &mut ShellViewModel, plugin_id: Option<&str>) {
    model.page = ShellPage::Settings;
    model.admin.begin_settings_load();
    model.admin.settings_sync.begin_load();
    model.admin.push_effect(AdminEffect::LoadAppSettings);
    if let Some(plugin_id) = plugin_id {
        open_route(model, plugin_id);
    }
}

pub(super) fn request_delete(model: &mut ShellViewModel, plugin_id: &str) {
    let Some(plugin) = model
        .admin
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == plugin_id)
        .cloned()
    else {
        eprintln!("Nana 找不到要删除的插件：{plugin_id}");
        return;
    };
    if !support::can_delete_plugin(&plugin) {
        return;
    }
    model.admin.pending_delete = Some(plugin.plugin_id);
}

/// 确认删除。成功后对话框关闭；失败时对话框保留，提示写在面板上。
pub(super) fn confirm_delete(model: &mut ShellViewModel) {
    let Some(plugin_id) = model.admin.pending_delete.clone() else {
        return;
    };
    model.admin.reset_action();
    model.admin.managing = true;
    model
        .admin
        .remember_outcome("插件已删除。", "插件删除失败。");
    model
        .admin
        .push_effect(AdminEffect::DeletePlugin(plugin_id));
}

/// 打开或收起插件设置。打开时读一次配置，下载服务和 Office 转换再读运行状态；收起或换插件时来源登录和运行状态重置。
pub(super) fn open_settings(model: &mut ShellViewModel, plugin_id: &str) {
    model.admin.reset_action();
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id) {
        model.admin.active_settings_plugin_id = None;
        flow::bind_plugin(model, None);
        super::super::downloader::release(model);
        super::super::office::release(model);
        return;
    }
    model.admin.active_settings_plugin_id = Some(plugin_id.to_string());
    flow::bind_plugin(model, Some(plugin_id));
    super::super::downloader::on_settings_opened(model, plugin_id);
    super::super::office::on_settings_opened(model, plugin_id);
    model.admin.managing = true;
    model.admin.pending_notice = None;
    model.admin.pending_failure = Some("插件设置读取失败。".into());
    model
        .admin
        .push_effect(AdminEffect::LoadConfig(plugin_id.to_string()));
}

/// 路由带插件标识时打开它的设置；已经打开并有快照就不重复读。
pub(super) fn open_route(model: &mut ShellViewModel, plugin_id: &str) {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty()
        || !model
            .admin
            .plugins
            .iter()
            .any(|plugin| plugin.plugin_id == plugin_id)
    {
        return;
    }
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id)
        && model.admin.config_snapshots.contains_key(plugin_id)
    {
        return;
    }
    open_settings(model, plugin_id);
}

pub(super) fn field_of(
    model: &ShellViewModel,
    plugin_id: &str,
    key: &str,
) -> Option<support::ConfigField> {
    let plugin = model
        .admin
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == plugin_id)?;
    support::settings_fields(plugin)
        .into_iter()
        .find(|field| field.key == key)
}

pub(super) fn apply_field(
    model: &mut ShellViewModel,
    plugin_id: &str,
    key: &str,
    text: &str,
    checked: Option<bool>,
) {
    let Some(field) = field_of(model, plugin_id, key) else {
        eprintln!("Nana 插件 {plugin_id} 没有配置字段 {key}");
        return;
    };
    model.admin.reset_action();
    match support::normalize_field_input(&field, text, checked) {
        FieldChange::Reset => reset_config(model, plugin_id, key),
        FieldChange::Invalid(error) => {
            eprintln!("Nana 插件 {plugin_id} 的字段 {key} 输入无效：{error}");
            model.admin.action_error = error;
        }
        FieldChange::Set(value) => {
            model.admin.managing = true;
            model
                .admin
                .remember_outcome("插件设置已保存。", "插件设置保存失败。");
            model.admin.push_effect(AdminEffect::SetConfig {
                plugin_id: plugin_id.to_string(),
                key: key.to_string(),
                value,
            });
        }
    }
}

pub(super) fn reset_config(model: &mut ShellViewModel, plugin_id: &str, key: &str) {
    model.admin.reset_action();
    model.admin.managing = true;
    model
        .admin
        .remember_outcome("插件设置已重置。", "插件设置重置失败。");
    model.admin.push_effect(AdminEffect::DeleteConfig {
        plugin_id: plugin_id.to_string(),
        key: key.to_string(),
    });
}

/// 单行字段回车，单行和 JSON 字段失焦：有没提交的草稿、
/// 而且和字段现在的值（配置快照，没有时是清单默认值）不同才提交，没改过的不保存。单行字段比显示的文字；
/// JSON 字段比解析出来的值，只改了空白和缩进不算改过，解析不了的算改过、提交时报错。
/// 插件管理在途时控件禁用、焦点也会离开，这时不重复提交，草稿留着。
pub(super) fn commit_draft(model: &mut ShellViewModel, plugin_id: &str, key: &str) {
    let Some(field) = field_of(model, plugin_id, key) else {
        eprintln!("Nana 插件 {plugin_id} 没有配置字段 {key}");
        return;
    };
    let json = field.field_type == "json";
    let drafts = if json {
        &model.admin.json_drafts
    } else {
        &model.admin.field_drafts
    };
    let Some(draft) = drafts
        .get(plugin_id)
        .and_then(|drafts| drafts.get(key))
        .cloned()
    else {
        return;
    };
    let current = support::config_value(&model.admin.config_snapshots, plugin_id, &field);
    let unchanged = if json {
        let current = current.cloned().unwrap_or(serde_json::Value::Null);
        support::parse_json_draft(&field.label, &draft).is_ok_and(|value| value == current)
    } else {
        draft == support::field_text(current)
    };
    if unchanged {
        return;
    }
    if model.admin.managing {
        eprintln!("Nana 插件 {plugin_id} 正在处理，字段 {key} 的草稿先不提交");
        return;
    }
    if json {
        save_json(model, plugin_id, &field, &draft);
    } else {
        apply_field(model, plugin_id, key, &draft, None);
    }
}

/// 保存 JSON 字段：草稿按 JSON 解析，空白按 null 保存；不是有效 JSON 时不保存，写错误。
pub(super) fn save_json(
    model: &mut ShellViewModel,
    plugin_id: &str,
    field: &support::ConfigField,
    draft: &str,
) {
    let key = field.key.as_str();
    model.admin.reset_action();
    match support::parse_json_draft(&field.label, draft) {
        Ok(value) => {
            model.admin.managing = true;
            model
                .admin
                .remember_outcome("插件设置已保存。", "插件设置保存失败。");
            model.admin.push_effect(AdminEffect::SetConfig {
                plugin_id: plugin_id.to_string(),
                key: key.to_string(),
                value,
            });
        }
        Err(error) => {
            eprintln!("Nana 插件 {plugin_id} 的字段 {key} 不是有效 JSON，不保存");
            model.admin.action_error = error;
        }
    }
}

pub(super) fn install_archive(model: &mut ShellViewModel, path: Option<String>) {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return;
    };
    model.admin.reset_action();
    model.admin.managing = true;
    model
        .admin
        .remember_outcome("插件已安装。", "插件安装失败。");
    model.admin.push_effect(AdminEffect::Install(path));
}

/// 安装、删除、启停之后的新列表。删除成功时收起确认框。
pub(super) fn replace_plugins(
    model: &mut ShellViewModel,
    result: Result<Vec<PluginManifest>, String>,
) {
    model.admin.managing = false;
    match result {
        Ok(plugins) => {
            publish_plugins(model, &plugins);
            model.admin.store_plugins(plugins);
            model.admin.pending_delete = None;
            model.admin.apply_success();
        }
        Err(error) => {
            eprintln!("Nana 插件操作失败：{error}");
            model.admin.apply_failure(&error);
        }
    }
}
