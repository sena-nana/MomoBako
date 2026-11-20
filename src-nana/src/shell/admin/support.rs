//! 设置、插件、日志、任务弹层和拓展页用到的规则：插件分组和筛选、插件设置字段的规范化、音频播放器
//! 候选、日志筛选、任务弹层的行和各处文案。
//!
//! 除了圆角偏好文件（`corners.json`）的读写，这里都是纯函数。服务调用留在调度器，失败不会被写成已保存。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::backend::services::repository::{
    PlaylistPlayerContribution, PluginHookExecutionRecord, PluginManifest, RepositorySummary,
    SystemLogRecord, TaskProgressSnapshot,
};
use crate::settings;

use super::super::player::{
    AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE, PlayerCandidate, builtin_audio_formats, capability_id,
    resolve_player,
};

pub(crate) use super::time::{hook_time_label, log_time_label, now_iso8601};

mod audio;
mod logs;
mod plugins;

pub use audio::*;
pub use logs::*;
pub use plugins::*;

pub const CORNER_RADIUS_MIN: f64 = 0.0;
pub const CORNER_RADIUS_MAX: f64 = 20.0;
pub const DEFAULT_CORNER_RADIUS: f64 = 8.0;
pub const CORNER_STYLE_KEY: &str = "momobako.corners";
pub const CORNER_RADIUS_KEY: &str = "momobako.cornerRadius";
const PLUGIN_GROUP_ORDER: [&str; 6] = [
    "source",
    "library-kind",
    "parser",
    "preview",
    "service",
    "unclassified",
];

/// Windows 和 Linux 默认平滑，macOS 默认普通。未写入半径时用 MomoBako 的 8。
pub fn default_corner_style() -> &'static str {
    if cfg!(target_os = "macos") {
        "round"
    } else {
        "smooth"
    }
}

pub fn corners_path() -> PathBuf {
    settings::default_path().with_file_name("corners.json")
}

pub fn clamp_radius(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    Some(value.clamp(CORNER_RADIUS_MIN, CORNER_RADIUS_MAX))
}

pub fn read_corners(path: &Path) -> (String, f64) {
    let fallback = (default_corner_style().to_string(), DEFAULT_CORNER_RADIUS);
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return fallback,
        Err(error) => {
            eprintln!("Nana 读取圆角偏好失败（{CORNER_STYLE_KEY}）：{error}");
            return fallback;
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Nana 解析圆角偏好失败（{CORNER_STYLE_KEY}）：{error}");
            return fallback;
        }
    };
    let style = value
        .get(CORNER_STYLE_KEY)
        .and_then(|item| item.as_str())
        .filter(|item| *item == "smooth" || *item == "round")
        .map(str::to_owned)
        .unwrap_or_else(|| default_corner_style().to_string());
    let radius = value
        .get(CORNER_RADIUS_KEY)
        .and_then(|item| item.as_f64())
        .and_then(clamp_radius)
        .unwrap_or(DEFAULT_CORNER_RADIUS);
    (style, radius)
}

/// 原子写入圆角偏好。失败时删掉临时文件并记日志。
pub fn write_corners(path: &Path, style: &str, radius: f64) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Err(error) = fs::create_dir_all(parent) {
        eprintln!("Nana 创建圆角偏好目录失败：{error}");
        return;
    }
    let value = serde_json::json!({
        CORNER_STYLE_KEY: style,
        CORNER_RADIUS_KEY: radius,
    });
    let raw = match serde_json::to_vec_pretty(&value) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("Nana 序列化圆角偏好失败：{error}");
            return;
        }
    };
    let temporary = path.with_extension("json.new");
    if let Err(error) = fs::write(&temporary, &raw) {
        eprintln!("Nana 写入圆角偏好失败：{error}");
        return;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        eprintln!("Nana 提交圆角偏好失败：{error}");
    }
}

/// 下拉字段的一个选项。
#[derive(Clone, Debug, PartialEq)]
pub struct FieldOption {
    pub label: String,
    pub value: Value,
}

/// 插件声明的一个设置字段，只收 `contributes.settings.fields` 里有键和名称的项。
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub field_type: String,
    pub options: Vec<FieldOption>,
    pub default_value: Option<Value>,
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldChange {
    Set(Value),
    Reset,
    Invalid(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendCount {
    pub plugin_id: String,
    pub name: String,
    pub kind: String,
    pub count: usize,
}

pub fn backend_counts(repositories: &[RepositorySummary]) -> Vec<BackendCount> {
    let mut counts = Vec::<BackendCount>::new();
    for repository in repositories {
        if let Some(current) = counts
            .iter_mut()
            .find(|item| item.plugin_id == repository.backend.plugin_id)
        {
            current.count += 1;
            continue;
        }
        counts.push(BackendCount {
            plugin_id: repository.backend.plugin_id.clone(),
            name: repository.backend.name.clone(),
            kind: repository.backend.kind.clone(),
            count: 1,
        });
    }
    counts
}

pub fn backend_summary(counts: &[BackendCount]) -> String {
    if counts.is_empty() {
        return "无".into();
    }
    counts
        .iter()
        .map(|item| format!("{} ({})", item.name, item.count))
        .collect::<Vec<_>>()
        .join(" / ")
}

pub fn external_status_label(ready: Option<bool>) -> &'static str {
    match ready {
        None => "未加载",
        Some(true) => "运行中",
        Some(false) => "未就绪",
    }
}

pub fn mask_token(token: Option<&str>) -> String {
    let Some(token) = token.filter(|value| !value.is_empty()) else {
        return "未加载".into();
    };
    let head = token.chars().take(10).collect::<String>();
    let tail_start = token.chars().count().saturating_sub(6);
    let tail = token.chars().skip(tail_start).collect::<String>();
    format!("{head}...{tail}")
}

pub fn connection_json(base_url: &str, token: &str, version: &str, started_at: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "baseUrl": base_url,
        "token": token,
        "version": version,
        "startedAt": started_at,
    }))
    .unwrap_or_default()
}

/// 任务弹层的一行：资源库操作或运行中的任务。
#[derive(Clone, Debug, PartialEq)]
pub struct PopoverRow {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub value: f64,
    pub indeterminate: bool,
    pub source: String,
    pub updated_at_ms: i64,
    /// 排队或运行中的后台任务可以取消。仓库操作行和已经在取消的任务为 false。
    pub cancellable: bool,
}

/// 认不出协议的后台任务的标题。宿主的任务只跑下面 [`task_protocol_label`] 列出的协议，正常碰不到。
pub const TASK_LABEL_FALLBACK: &str = "后台任务";

/// 后台任务的协议写成用户看得懂的中文。仓库和文件操作沿用进度行的叫法（
/// 「同步资源库」「复制文件」等），其余照后端协议说明（`docs/api-design.md`）。
pub fn task_protocol_label(protocol_id: &str) -> Option<&'static str> {
    use crate::backend::services::mutsuki_runner::{
        PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE,
        PROTOCOL_ENTRY_IMPORT, PROTOCOL_ENTRY_MOVE, PROTOCOL_PLAYBACK_PREPARE,
        PROTOCOL_PLAYLIST_DOWNLOAD, PROTOCOL_REPOSITORY_ACTION_RUN, PROTOCOL_REPOSITORY_ATTACH,
        PROTOCOL_REPOSITORY_CREATE, PROTOCOL_REPOSITORY_EXPORT, PROTOCOL_REPOSITORY_IMPORT,
        PROTOCOL_REPOSITORY_RELOCATE, PROTOCOL_REPOSITORY_SYNC, PROTOCOL_THUMBNAIL_REQUEST,
    };
    let label = match protocol_id {
        PROTOCOL_REPOSITORY_CREATE => "创建资源库",
        PROTOCOL_REPOSITORY_IMPORT => "导入资源库",
        PROTOCOL_REPOSITORY_ATTACH => "挂载资源库",
        PROTOCOL_REPOSITORY_RELOCATE => "重定向资源库",
        PROTOCOL_REPOSITORY_EXPORT => "导出资源库",
        PROTOCOL_REPOSITORY_SYNC => "同步资源库",
        PROTOCOL_ENTRY_IMPORT => "导入文件",
        PROTOCOL_ARCHIVE_IMPORT => "导入 ZIP",
        PROTOCOL_EAGLE_IMPORT => "导入 Eagle",
        PROTOCOL_ENTRY_COPY => "复制文件",
        PROTOCOL_ENTRY_MOVE => "移动文件",
        PROTOCOL_ENTRY_DELETE => "删除文件",
        PROTOCOL_REPOSITORY_ACTION_RUN => "执行仓库动作",
        PROTOCOL_THUMBNAIL_REQUEST => "生成缩略图",
        PROTOCOL_PLAYBACK_PREPARE => "准备播放",
        PROTOCOL_PLAYLIST_DOWNLOAD => "下载歌单",
        _ => return None,
    };
    Some(label)
}

/// 任务弹层的行：仓库操作那一行，加上排队、运行和取消中的后台任务，一起按更新时间倒序。
/// 任务快照的更新时间是 RFC 3339，换成毫秒再比；读不出的排在最后。
///
/// 除了仓库操作，也列宿主观察到的后台任务，规则是：
/// - 标题是任务自己报的标题，没有时按协议写（[`task_protocol_label`]），不写泛泛的「进度」；
/// - 仓库操作那一行已经表示的同一件事不再单列：这次操作在后台跑的协议（`OperationProgress::protocols`，
///   例如加载资源库的同步、复制文件的复制）的任务不出行，侧栏「任务」的计数也只算剩下的行。
pub fn popover_rows(
    tasks: &[TaskProgressSnapshot],
    operation: Option<&super::OperationProgress>,
) -> Vec<PopoverRow> {
    let mut rows = Vec::new();
    if let Some(operation) = operation {
        rows.push(PopoverRow {
            id: "workspace-operation".into(),
            label: operation.label.clone(),
            detail: operation.detail.clone(),
            value: operation.value,
            indeterminate: operation.indeterminate,
            source: "资源库".into(),
            updated_at_ms: operation.updated_at_ms,
            cancellable: false,
        });
    }
    let covered = operation
        .map(|operation| operation.protocols)
        .unwrap_or_default();
    rows.extend(
        tasks
            .iter()
            .filter(|task| !covered.iter().any(|protocol| *protocol == task.protocol_id))
            .map(|task| PopoverRow {
                id: task.task_id.clone(),
                label: task_label(task),
                detail: task
                    .error
                    .clone()
                    .or(task.phase.clone())
                    .unwrap_or_default(),
                value: f64::from(task.percent.unwrap_or(0.0)),
                indeterminate: task.percent.is_none(),
                source: "任务".into(),
                updated_at_ms: super::time::parse_rfc3339_millis(&task.updated_at).unwrap_or_else(
                    || {
                        eprintln!(
                            "Nana 任务快照的更新时间读不出，排在最后：{} {}",
                            task.task_id, task.updated_at
                        );
                        0
                    },
                ),
                cancellable: matches!(task.status.as_str(), "queued" | "running"),
            }),
    );
    rows.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    rows
}

/// 一个后台任务的标题：任务自己报的标题，没有时按协议写，协议也认不出时写「后台任务」。弹层每次同步都
/// 重算这一行，这里不记日志。
fn task_label(task: &TaskProgressSnapshot) -> String {
    if let Some(label) = task
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
    {
        return label.to_string();
    }
    task_protocol_label(&task.protocol_id)
        .unwrap_or(TASK_LABEL_FALLBACK)
        .to_string()
}

pub fn action_status_label(status: &str, enabled: bool) -> &'static str {
    if status != "ready" {
        "不支持"
    } else if !enabled {
        "已停用"
    } else {
        "可执行"
    }
}

pub fn action_can_run(status: &str, enabled: bool, selected_count: usize, running: bool) -> bool {
    status == "ready" && enabled && selected_count > 0 && !running
}

pub fn apply_tool_page_selection(
    pages: &[super::ToolPageEntry],
    active: Option<&str>,
) -> Option<String> {
    if pages.is_empty() {
        return None;
    }
    if active.is_some_and(|id| pages.iter().any(|page| page.id == id)) {
        return active.map(str::to_string);
    }
    Some(pages[0].id.clone())
}

pub const TOOL_FILE_MANAGER: &str = "momobako.tool.file-manager";
pub const TOOL_EAGLE_IMPORTER: &str = "momobako.tool.eagle-importer";
pub const TOOL_API_PLAYGROUND: &str = "momobako.tool.api-playground";

pub fn is_builtin_tool_page(id: &str) -> bool {
    matches!(
        id,
        TOOL_FILE_MANAGER | TOOL_EAGLE_IMPORTER | TOOL_API_PLAYGROUND
    )
}

/// 拓展页的工具：已启用、状态可用的插件里 `contributes.toolPages` 的条目，
/// 按 `order`（缺省 100）、名称、标识排序。页面由宿主画，插件只声明。
pub fn tool_pages_from_plugins(plugins: &[PluginManifest]) -> Vec<super::ToolPageEntry> {
    let mut pages = Vec::new();
    for plugin in plugins {
        if !plugin.enabled || matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error")
        {
            continue;
        }
        let Some(entries) = plugin
            .contributes
            .get("toolPages")
            .and_then(|item| item.as_array())
        else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry
                .get("toolPageId")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
            else {
                eprintln!("Nana 插件 {} 的工具页缺少 toolPageId", plugin.plugin_id);
                continue;
            };
            let label = entry
                .get("label")
                .and_then(|item| item.as_str())
                .unwrap_or(id)
                .to_string();
            let order = entry
                .get("order")
                .and_then(|item| item.as_f64())
                .unwrap_or(100.0);
            pages.push((
                order,
                super::ToolPageEntry {
                    id: id.to_string(),
                    label,
                    plugin_name: plugin.name.clone(),
                    description: entry
                        .get("description")
                        .and_then(|item| item.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    native: is_builtin_tool_page(id),
                },
            ));
        }
    }
    pages.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.label.cmp(&right.1.label))
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    pages.into_iter().map(|(_, page)| page).collect()
}

#[cfg(test)]
mod tasks_tests;
