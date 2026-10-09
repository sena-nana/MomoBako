//! 系统日志面板的投影和常驻信号。
//!
//! [`LogsView`] 按 `WorkspaceLogsPanel.vue` 算好计数、工具条、筛选芯片的选中态、空状态和日志行；
//! [`LogsSignals`] 建在主区块的常驻作用域里，日志行放进 Store 按日志 id 做键。追踪时新日志到达，
//! 只插入新的那一行（缓存满了再删掉掉出去的最旧一行），已有行的节点和绑定都不动。

use nana_ui::runtime::view::{signal, store, Signal, Store};

use super::super::hot::ModelField;
use super::super::ShellViewModel;
use super::support;
use crate::backend::services::repository::SystemLogRecord;

/// 页头两个计数。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LogsHead {
    /// 「N 条缓存」。
    pub cached: String,
    /// 「N 条命中」。
    pub hits: String,
}

/// 工具条：两个下拉框的选项和当前值，暂停、重置和清空按钮的状态。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LogsToolbar {
    pub plugins: Vec<String>,
    pub repos: Vec<String>,
    pub plugin: String,
    pub repo: String,
    pub paused: bool,
    /// 没有生效的筛选时「重置筛选」禁用。
    pub no_filters: bool,
    /// 没有日志时「清空日志」禁用。
    pub no_logs: bool,
}

/// 空状态的标题和说明；有命中时为 `None`。
pub(crate) type LogsEmpty = Option<(&'static str, &'static str)>;

/// 一条日志卡片要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogRowView {
    pub id: String,
    pub level: String,
    pub level_label: String,
    pub kind: String,
    /// 来源标签，空时不显示那枚芯片。
    pub source: String,
    pub time: String,
    pub action: String,
    pub message: String,
    pub category: String,
    /// 「插件 …」「仓库 …」「位置 …」，没有时为空串，那枚芯片不显示。
    pub plugin: String,
    pub repo: String,
    pub location: String,
    /// 有上下文时显示「上下文」。
    pub has_context: bool,
    pub context_open: bool,
    /// 展开时的等宽 JSON；收起时为空，不白算。
    pub context: String,
}

impl LogRowView {
    fn project(model: &ShellViewModel, record: &SystemLogRecord) -> Self {
        let open = model.admin.log_context_open.contains(&record.id);
        let has_context = record.context.as_object().is_some_and(|object| !object.is_empty());
        let location = support::location_label(record);
        Self {
            id: record.id.clone(),
            level: record.level.clone(),
            level_label: support::level_label(&record.level),
            kind: support::source_kind_label(&record.source.kind),
            source: record.source.label.clone().unwrap_or_default(),
            time: support::log_time_label(&record.timestamp),
            action: record.action.clone(),
            message: record.message.clone(),
            category: record.category.clone(),
            plugin: record.source.plugin_id.as_deref().map(|plugin_id| format!("插件 {plugin_id}")).unwrap_or_default(),
            repo: record.source.repo_id.as_deref().map(|repo_id| format!("仓库 {repo_id}")).unwrap_or_default(),
            location: if location.is_empty() { String::new() } else { format!("位置 {location}") },
            has_context,
            context_open: open,
            context: if has_context && open {
                serde_json::to_string_pretty(&record.context).unwrap_or_else(|_| record.context.to_string())
            } else {
                String::new()
            },
        }
    }

    /// 日志行的键：日志 id。
    pub(crate) fn key(row: &LogRowView) -> String {
        row.id.clone()
    }
}

/// 日志面板要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogsView {
    pub head: LogsHead,
    pub toolbar: LogsToolbar,
    pub search: String,
    pub levels: Vec<String>,
    pub kinds: Vec<String>,
    pub empty: LogsEmpty,
    pub rows: Vec<LogRowView>,
    /// 追踪模式下日志列表跟随末尾。
    pub follow_end: bool,
}

impl LogsView {
    /// 从 ViewModel 取日志面板的投影，取舍和旧视图 `logs_panel` 一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let admin = &model.admin;
        let filtered = admin.filtered_logs();
        let empty = if admin.logs.is_empty() {
            Some(("还没有系统日志", "宿主、插件和辅助进程产生的关键操作会在这里持续汇总。"))
        } else if filtered.is_empty() {
            Some(("当前筛选没有命中", "保留最近日志缓存，调整级别、来源或关键字后可以继续查看。"))
        } else {
            None
        };
        let active = support::active_filter_count(&admin.log_levels, &admin.log_kinds, &admin.log_plugin_id, &admin.log_repo_id, &admin.log_search);
        Self {
            head: LogsHead { cached: format!("{} 条缓存", admin.logs.len()), hits: format!("{} 条命中", filtered.len()) },
            toolbar: LogsToolbar {
                plugins: support::unique_sorted(admin.logs.iter().filter_map(|record| record.source.plugin_id.clone())),
                repos: support::unique_sorted(admin.logs.iter().filter_map(|record| record.source.repo_id.clone())),
                plugin: admin.log_plugin_id.clone(),
                repo: admin.log_repo_id.clone(),
                paused: admin.log_paused,
                no_filters: active == 0,
                no_logs: admin.logs.is_empty(),
            },
            search: admin.log_search.clone(),
            levels: admin.log_levels.clone(),
            kinds: admin.log_kinds.clone(),
            empty,
            rows: filtered.iter().map(|record| LogRowView::project(model, record)).collect(),
            follow_end: model.admin_logs_follow_end(),
        }
    }
}

/// 日志面板的信号。句柄都是 `Copy` 的 id。
#[derive(Clone, Copy)]
pub(crate) struct LogsSignals {
    pub(crate) head: Signal<LogsHead>,
    pub(crate) toolbar: Signal<LogsToolbar>,
    pub(crate) search: ModelField,
    /// 选中的级别和来源：`(级别, 来源)`。
    pub(crate) filters: Signal<(Vec<String>, Vec<String>)>,
    pub(crate) empty: Signal<LogsEmpty>,
    pub(crate) rows: Store<Vec<LogRowView>>,
    pub(crate) follow_end: Signal<bool>,
}

impl LogsSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            head: signal(LogsHead::default()),
            toolbar: signal(LogsToolbar::default()),
            search: ModelField::new(""),
            filters: signal((Vec::new(), Vec::new())),
            empty: signal(None),
            rows: store(Vec::new()),
            follow_end: signal(false),
        }
    }

    /// 写入投影：只写变了的信号，日志行按 id 只改变了的项。
    pub(crate) fn write(&self, view: LogsView) {
        self.head.try_set_if_changed(view.head);
        self.toolbar.try_set_if_changed(view.toolbar);
        self.search.sync(&view.search);
        self.filters.try_set_if_changed((view.levels, view.kinds));
        self.empty.try_set_if_changed(view.empty);
        super::super::row_sync::sync_rows(self.rows, LogRowView::key, view.rows);
        self.follow_end.try_set_if_changed(view.follow_end);
    }
}
