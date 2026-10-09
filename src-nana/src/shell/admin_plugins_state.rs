//! 插件管理面板的投影和常驻信号。设置页和拓展页共用同一份信号，只换文案。
//!
//! [`PluginPanelView`] 按 Vue `PluginManagerPanel.vue` 的取舍算好页头计数、提示、面板状态、
//! 按分类分组的插件卡片，以及展开了设置的那个插件的设置区（来源账号和字段表单）。
//! [`PluginPanelSignals`] 建在主区块的常驻作用域里：分组和卡片放进 Store，按分类、插件 id 做键，
//! 卡片里的文字和按钮状态各自绑定；筛选框的草稿走 `ModelField`。

use nana_ui::runtime::view::{signal, store, Signal, Store};

use super::super::admin_fields::FieldRowView;
use super::super::hot::ModelField;
use super::super::ShellViewModel;
use super::source_page::{SourceAuthView, SourceRepoRow};
use super::style::PillTone;
use super::support;
use crate::backend::services::repository::PluginManifest;

/// 面板主体显示哪一块。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PanelState {
    #[default]
    Loading,
    /// 筛选后没有插件。
    Empty,
    Groups,
}

/// 依赖、权限和执行记录小块的语气。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChipTone {
    Plain,
    Muted,
    Danger,
}

/// 页头右侧：插件数和刷新、安装能不能点。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PanelHead {
    /// 「N 个插件」，按筛选后的数量。
    pub count: String,
    pub managing: bool,
}

/// 一个分类分组。
#[derive(Clone, Debug, PartialEq, nana_ui::runtime::view::Store)]
pub(crate) struct PluginGroupView {
    pub category: String,
    pub label: String,
    /// 「N 个插件」。
    pub count: String,
    pub cards: Vec<PluginCardView>,
}

/// 一张插件卡片要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginCardView {
    pub plugin_id: String,
    pub name: String,
    pub status: String,
    pub tone: PillTone,
    pub description: String,
    /// 「v版本」。
    pub version: String,
    /// 禁用原因（危险色）和降级原因（弱色），没有时为空。
    pub notices: Vec<(String, bool)>,
    /// 分类、类型、来源、运行时、依赖和能力芯片。
    pub chips: Vec<String>,
    pub dependencies: Vec<(String, ChipTone)>,
    pub permissions: Vec<String>,
    pub hooks: Vec<String>,
    pub executions: Vec<ExecutionView>,
    pub enabled: bool,
    pub can_delete: bool,
    /// 这张卡片的设置区展开着。
    pub settings_open: bool,
}

/// 一条钩子执行记录。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExecutionView {
    /// 转义后的执行 id，重复的加序号。
    pub key: String,
    pub status: String,
    pub tone: ChipTone,
    pub title: String,
    pub slot: String,
    pub message: String,
    pub time: String,
}

/// 展开了设置的插件的设置区页头。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SettingsHead {
    pub label: String,
    pub description: String,
}

/// 插件管理面板要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginPanelView {
    pub head: PanelHead,
    /// 操作失败、操作结果或读取失败的提示，错误时为真。
    pub notice: Option<(String, bool)>,
    pub keyword: String,
    pub state: PanelState,
    /// 只在面板显示分组时投影。
    pub groups: Vec<PluginGroupView>,
    pub settings: SettingsHead,
    /// 展开的插件是来源插件时它的账号与仓库区。
    pub source: Option<SourceAuthView>,
    pub source_repos: Vec<SourceRepoRow>,
    /// 展开的插件声明的设置字段。
    pub fields: Vec<FieldRowView>,
}

impl PluginPanelView {
    /// 从 ViewModel 取插件管理面板的投影，取舍和旧视图 `manager_panel` 一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let admin = &model.admin;
        let filtered = support::filtered_plugins(&admin.plugins, &admin.keyword, &admin.hook_executions);
        let notice = if !admin.action_error.is_empty() {
            Some((admin.action_error.clone(), true))
        } else if !admin.action_message.is_empty() {
            Some((admin.action_message.clone(), false))
        } else if !admin.load_error.is_empty() && !admin.loading_settings {
            Some((admin.load_error.clone(), true))
        } else {
            None
        };
        let state = if admin.loading_settings {
            PanelState::Loading
        } else if filtered.is_empty() {
            PanelState::Empty
        } else {
            PanelState::Groups
        };
        let groups = if state == PanelState::Groups { project_groups(model) } else { Vec::new() };
        let open = admin
            .active_settings_plugin_id
            .as_deref()
            .and_then(|plugin_id| admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id));
        let (source, source_repos) = match open.filter(|plugin| support::has_source_authentication(plugin)) {
            Some(plugin) => {
                let (view, rows) = SourceAuthView::project(model, plugin);
                (Some(view), rows)
            }
            None => (None, Vec::new()),
        };
        Self {
            head: PanelHead { count: format!("{} 个插件", filtered.len()), managing: admin.managing },
            notice,
            keyword: admin.keyword.clone(),
            state,
            groups,
            settings: open
                .map(|plugin| SettingsHead {
                    label: support::plugin_settings_label(plugin),
                    description: support::plugin_settings_description(plugin),
                })
                .unwrap_or_default(),
            source,
            source_repos,
            fields: open.map(|plugin| FieldRowView::project_all(model, plugin)).unwrap_or_default(),
        }
    }
}

/// 按分类分组，组内按插件 id 取卡片。组的计数是这一组的插件数。
fn project_groups(model: &ShellViewModel) -> Vec<PluginGroupView> {
    let admin = &model.admin;
    admin
        .grouped_plugins()
        .into_iter()
        .map(|(category, ids)| PluginGroupView {
            label: support::category_label(&category).to_string(),
            count: format!("{} 个插件", ids.len()),
            cards: ids
                .iter()
                .filter_map(|plugin_id| admin.plugins.iter().find(|plugin| &plugin.plugin_id == plugin_id))
                .map(|plugin| PluginCardView::project(model, plugin))
                .collect(),
            category,
        })
        .collect()
}

impl PluginCardView {
    fn project(model: &ShellViewModel, plugin: &PluginManifest) -> Self {
        let mut notices = Vec::new();
        if let Some(reason) = plugin.disable_reason.as_deref() {
            notices.push((reason.to_string(), true));
        }
        if let Some(reason) = plugin.degradation_reason.as_deref() {
            notices.push((reason.to_string(), false));
        }
        let mut chips = vec![
            support::category_label(&support::plugin_category(plugin)).to_string(),
            plugin.kind.clone(),
            support::plugin_source_label(&plugin.source).to_string(),
            support::plugin_runtime_label(&plugin.runtime).to_string(),
            format!("依赖 {}", support::dependency_label(plugin)),
        ];
        chips.extend(plugin.capabilities.iter().cloned());
        let deps = &plugin.dependency_status;
        let dependency = |prefix: &str, state: &crate::backend::services::repository::PluginDependencyState| {
            let name = state.name.clone().unwrap_or_else(|| state.plugin_id.clone());
            (format!("{prefix} {name} · {}", support::dependency_status_label(&state.status)), dependency_tone(&state.status))
        };
        let mut dependencies = deps.required.iter().map(|state| dependency("必需", state)).collect::<Vec<_>>();
        dependencies.extend(deps.optional.iter().map(|state| dependency("可选", state)));
        Self {
            plugin_id: plugin.plugin_id.clone(),
            name: plugin.name.clone(),
            status: support::plugin_status_label(plugin).to_string(),
            tone: status_tone(plugin),
            description: plugin.description.clone(),
            version: format!("v{}", plugin.version),
            notices,
            chips,
            dependencies,
            permissions: plugin.permissions.clone(),
            hooks: plugin
                .hooks
                .iter()
                .map(|hook| format!("{} · {}", hook.label.clone().unwrap_or_else(|| hook.action.clone()), hook.slot))
                .collect(),
            executions: project_executions(model, &plugin.plugin_id),
            enabled: plugin.enabled,
            can_delete: support::can_delete_plugin(plugin),
            settings_open: model.admin.active_settings_plugin_id.as_deref() == Some(plugin.plugin_id.as_str()),
        }
    }

    /// 卡片在分组里的键。
    pub(crate) fn key(card: &PluginCardView) -> String {
        card.plugin_id.clone()
    }
}

impl PluginGroupView {
    /// 分组的键。
    pub(crate) fn key(group: &PluginGroupView) -> String {
        group.category.clone()
    }
}

/// 最近三条执行记录。
fn project_executions(model: &ShellViewModel, plugin_id: &str) -> Vec<ExecutionView> {
    let records = model.admin.hook_executions.iter().filter(|record| record.plugin_id == plugin_id).take(3).collect::<Vec<_>>();
    let keys = super::style::unique_keys(records.iter().map(|record| record.execution_id.as_str()));
    records
        .into_iter()
        .zip(keys)
        .map(|(record, key)| ExecutionView {
            key,
            status: support::hook_status_label(&record.status),
            tone: match record.status.as_str() {
                "failed" => ChipTone::Danger,
                "blocked" => ChipTone::Muted,
                _ => ChipTone::Plain,
            },
            title: record.hook_label.clone().unwrap_or_else(|| record.hook_action.clone()),
            slot: record.hook_slot.clone(),
            message: record.message.clone(),
            time: support::hook_time_label(&record.started_at),
        })
        .collect()
}

/// 状态胶囊的语气：错误或不可用危险色，降级强调色淡底，未启用灰，其余强调色。
fn status_tone(plugin: &PluginManifest) -> PillTone {
    if plugin.status == "unavailable" || plugin.status == "error" {
        PillTone::Danger
    } else if plugin.degraded && plugin.enabled {
        PillTone::Warning
    } else if !plugin.enabled || plugin.status == "disabled" {
        PillTone::Ghost
    } else {
        PillTone::Accent
    }
}

fn dependency_tone(status: &str) -> ChipTone {
    match status {
        "missing" | "unavailable" | "error" => ChipTone::Danger,
        "disabled" => ChipTone::Muted,
        _ => ChipTone::Plain,
    }
}

/// 插件管理面板的信号。句柄都是 `Copy` 的 id，值在建它的作用域里。
#[derive(Clone, Copy)]
pub(crate) struct PluginPanelSignals {
    pub(crate) head: Signal<PanelHead>,
    pub(crate) notice: Signal<Option<(String, bool)>>,
    pub(crate) keyword: ModelField,
    pub(crate) state: Signal<PanelState>,
    pub(crate) groups: Store<Vec<PluginGroupView>>,
    pub(crate) settings: Signal<SettingsHead>,
    pub(crate) source: Signal<Option<SourceAuthView>>,
    pub(crate) source_repos: Store<Vec<SourceRepoRow>>,
    pub(crate) fields: Store<Vec<FieldRowView>>,
}

impl PluginPanelSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            head: signal(PanelHead::default()),
            notice: signal(None),
            keyword: ModelField::new(""),
            state: signal(PanelState::default()),
            groups: store(Vec::new()),
            settings: signal(SettingsHead::default()),
            source: signal(None),
            source_repos: store(Vec::new()),
            fields: store(Vec::new()),
        }
    }

    /// 写入投影：只写变了的信号，分组、来源仓库和字段按键只改变了的项。
    pub(crate) fn write(&self, view: PluginPanelView) {
        self.head.try_set_if_changed(view.head);
        self.notice.try_set_if_changed(view.notice);
        self.keyword.sync(&view.keyword);
        self.state.try_set_if_changed(view.state);
        super::bind::sync_rows(self.groups, PluginGroupView::key, view.groups);
        self.settings.try_set_if_changed(view.settings);
        self.source.try_set_if_changed(view.source);
        super::bind::sync_rows(self.source_repos, SourceRepoRow::key, view.source_repos);
        super::bind::sync_rows(self.fields, FieldRowView::key, view.fields);
    }
}
