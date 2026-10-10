//! 文件右键里的来源插件动作。
//!
//! 清单 `contributes.source.entryActions[]` 的 operation 只有插件 SDK 约定的五种（`SourceEntryActionOperation`，
//! `docs/api-design.md`）：宿主负责每一种的目录选择、进度、导入和刷新，声明不能让宿主执行别的命令。
//! 能直接调用的方法接到现有 `call_plugin`。下载照 Vue `downloadTargets` 分「到本地」和「到其他资源库」两项
//! （声明了 `targets` 时只出声明的）：到本地先要系统目录对话框，到其他资源库先选目标资源库（`source_target.rs`），
//! 选好才带上 `destination` 调用。清理缓存、重新获取播放资源和下载的载荷照 Vue 带上当前资源库的本地缓存目录
//! （`managedCacheRoot`），网易云插件据此找到真正的播放缓存和暂存目录。
//! 「创建播放列表」（`playlist-from-directory`）照 Vue `runPlaylistAction` 不调用插件：在本地建一个以来源
//! 歌单命名的播放集（已经有就不建），把这个目录加进去，再刷新资源库（`admin_reduce.rs`）。约定以外的 operation
//! 和没写 scope 的声明不出菜单，只记日志（Vue 把约定以外的 operation 一律当成「创建播放列表」，是缺陷）。
//!
//! 来源认证或本地缓存不可用时（Vue `actionDisabled`：登录过期、没有登录、要本地缓存但没就绪，或者条目自己标了
//! 登录过期），这些动作都禁用，菜单里写出原因；没有能下载到的其他资源库时「到其他资源库」禁用。

use serde_json::{Map, Value};

use crate::backend::services::repository::PluginManifest;

use super::files::FileRow;
use super::player::AUDIO_SEQUENCE_TYPE;

/// 来源认证或本地缓存不可用时，禁用的来源动作下面写的原因（Vue `actionDisabled` 的 `confirmLabel`）。
pub const SOURCE_UNAVAILABLE: &str = "来源认证或本地缓存不可用";
/// 没有就绪、可写的其他资源库时，「下载到其他资源库」下面写的原因。
pub const NO_TARGET_REPOSITORY: &str = "没有可下载到的其他资源库";

/// 来源动作要看的当前资源库情况，由菜单从工作台算好交进来。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceContext {
    /// 当前资源库的本地缓存目录（`localCache.path`），载荷里的 `managedCacheRoot`；没有时写 null。
    pub managed_cache_root: Option<String>,
    /// 来源认证或本地缓存不可用（仓库这一侧，见 `WorkspaceRepository::source_unavailable`）。
    pub source_unavailable: bool,
    /// 有没有能下载到的其他资源库：就绪、可写、不是当前这个。
    pub has_target_repository: bool,
}

/// 右键可以画出的一条插件动作。
#[derive(Clone, Debug, PartialEq)]
pub struct FilePluginAction {
    pub value: String,
    pub label: String,
    /// 不能用的原因：有原因时菜单里禁用这一项，并写出原因。
    pub unavailable: Option<&'static str>,
    pub dispatch: FilePluginDispatch,
}

/// 立刻调用插件、先问目录或目标资源库再调用，或者在本地建播放集。
#[derive(Clone, Debug, PartialEq)]
pub enum FilePluginDispatch {
    Call { plugin_id: String, method: String, payload: Value },
    /// 下载到本地：选中文件夹后把 `destination` 写进载荷再调用。`progress` 是这次下载在任务弹层里的进度行。
    AskFolder { plugin_id: String, method: String, payload: Value, progress: DownloadProgress },
    /// 下载到其他资源库：选好目标资源库，把 `destination` 写进载荷再调用；下载回来的文件导入目标资源库并同步它。
    /// `parent_path` 是导入到目标资源库的哪个文件夹：下载目录时是以歌单命名的文件夹（Vue `playlistName`），没有时
    /// 先建，单曲放在根目录。
    AskRepository { plugin_id: String, method: String, payload: Value, progress: DownloadProgress, parent_path: Option<String> },
    /// 来源目录建成本地播放集（Vue `runPlaylistAction`），不调用插件。
    CreatePlaylist(SourcePlaylist),
}

/// 下载的去处，对应清单 `targets` 的两种取值。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownloadTarget {
    LocalDirectory,
    WritableRepository,
}

impl DownloadTarget {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "local-directory" => Some(Self::LocalDirectory),
            "writable-repository" => Some(Self::WritableRepository),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::LocalDirectory => "local-directory",
            Self::WritableRepository => "writable-repository",
        }
    }

    /// Vue `targetLabel`。
    fn label(self) -> &'static str {
        match self {
            Self::LocalDirectory => "本地",
            Self::WritableRepository => "其他资源库",
        }
    }
}

/// 来源目录要建成的播放集。标识是「来源-歌单」，名称取来源的歌单名、没有时用目录名，播放器类型取动作声明的、
/// 默认音频顺序播放。来源载荷里没有歌单标识、目录也没有来源标识时 `playlist_id` 为空，点了只报错。
#[derive(Clone, Debug, PartialEq)]
pub struct SourcePlaylist {
    pub playlist_id: Option<String>,
    pub name: String,
    pub player_type_id: String,
    pub path: String,
}

impl SourcePlaylist {
    /// 照 Vue `runPlaylistAction` 从动作声明和目录行算出要建的播放集。
    fn of(action: &Value, row: &FileRow) -> Self {
        let provider = row.provider_id.clone().unwrap_or_else(|| "source".into());
        let item = source_identifier(row, "playlistId");
        Self {
            playlist_id: item.map(|item| format!("{provider}-{item}")),
            name: playlist_name(row),
            player_type_id: text(action, "playerTypeId").unwrap_or_else(|| AUDIO_SEQUENCE_TYPE.to_string()),
            path: row.path.clone(),
        }
    }
}

/// 来源下载在任务弹层里的进度行（Vue `runDownloadAction`）：种类是动作名；选好去处先是「准备下载 …」，
/// 调用插件时单曲是「正在下载 …」。下载目录时 Vue 先列出目录里的文件再写「正在处理 N 个文件」，
/// Nana 不列目录，停在「准备下载目录」。`subject` 是下载的条目名，收尾的提示和日志用。
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadProgress {
    pub label: String,
    pub subject: String,
    pub preparing: String,
    pub running: Option<String>,
}

impl DownloadProgress {
    fn of(label: &str, row: &FileRow, operation: &str) -> Self {
        if operation == "download-directory" {
            Self { label: label.to_string(), subject: row.name.clone(), preparing: "准备下载目录".into(), running: None }
        } else {
            Self {
                label: label.to_string(),
                subject: row.name.clone(),
                preparing: format!("准备下载 {}", row.name),
                running: Some(format!("正在下载 {}", row.name)),
            }
        }
    }
}

/// 按清单 `entryActions` 和当前行的类型、来源载荷匹配菜单项。
pub fn actions_for(plugins: &[PluginManifest], row: &FileRow, context: &SourceContext) -> Vec<FilePluginAction> {
    let mut actions = Vec::new();
    for plugin in plugins {
        if !plugin.enabled || matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error") {
            continue;
        }
        let Some(list) = plugin.contributes.get("source").and_then(|source| source.get("entryActions")).and_then(|value| value.as_array()) else {
            continue;
        };
        for action in list {
            if matches_row(action, row) {
                actions.extend(prepare(plugin, action, row, context));
            }
        }
    }
    actions
}

/// 按 operation 把声明换成能执行的动作（下载按去处可能是两项）。要调用插件的 operation 找不到方法、或者
/// operation 不在约定里时返回空并记下原因。
fn prepare(plugin: &PluginManifest, action: &Value, row: &FileRow, context: &SourceContext) -> Vec<FilePluginAction> {
    let action_id = text(action, "actionId").unwrap_or_else(|| "action".into());
    let label = text(action, "label").unwrap_or_else(|| action_id.clone());
    let operation = text(action, "operation").unwrap_or_default();
    let value = format!("plugin/{}/{}", plugin.plugin_id, action_id);
    let disabled = (context.source_unavailable || entry_login_expired(row)).then_some(SOURCE_UNAVAILABLE);
    let method = action_method(plugin, action, &operation);
    if method.is_none() && matches!(operation.as_str(), "clear-cache" | "refresh-playback" | "download-entry" | "download-directory") {
        eprintln!("Nana 文件插件动作没有可调用的方法：{}/{action_id}（{operation}）", plugin.plugin_id);
        return Vec::new();
    }
    let single = |dispatch| vec![FilePluginAction { value: value.clone(), label: label.clone(), unavailable: disabled, dispatch }];
    match operation.as_str() {
        "clear-cache" | "refresh-playback" => {
            let Some(method) = method else { return Vec::new() };
            let payload = call_payload(row, &operation, context);
            single(FilePluginDispatch::Call { plugin_id: plugin.plugin_id.clone(), method, payload })
        }
        "download-entry" | "download-directory" => {
            let Some(method) = method else { return Vec::new() };
            let targets = download_targets(action, &plugin.plugin_id, &action_id);
            let several = targets.len() > 1;
            targets
                .into_iter()
                .map(|target| {
                    let payload = call_payload(row, &operation, context);
                    let progress = DownloadProgress::of(&label, row, &operation);
                    let plugin_id = plugin.plugin_id.clone();
                    let method = method.clone();
                    let (dispatch, unavailable) = match target {
                        DownloadTarget::LocalDirectory => (FilePluginDispatch::AskFolder { plugin_id, method, payload, progress }, disabled),
                        DownloadTarget::WritableRepository => {
                            let parent_path = if operation == "download-directory" { download_folder(row) } else { None };
                            let unavailable = disabled.or((!context.has_target_repository).then_some(NO_TARGET_REPOSITORY));
                            (FilePluginDispatch::AskRepository { plugin_id, method, payload, progress, parent_path }, unavailable)
                        }
                    };
                    FilePluginAction {
                        value: if several { format!("{value}/{}", target.key()) } else { value.clone() },
                        label: if several { format!("{label}到{}", target.label()) } else { label.clone() },
                        unavailable,
                        dispatch,
                    }
                })
                .collect()
        }
        "playlist-from-directory" => single(FilePluginDispatch::CreatePlaylist(SourcePlaylist::of(action, row))),
        other => {
            eprintln!("Nana 文件插件动作的 operation 不在插件约定里，不出菜单：{}/{action_id}（{other}）", plugin.plugin_id);
            Vec::new()
        }
    }
}

/// 下载的去处（Vue `downloadTargets`）：声明了 `targets` 就只出声明的，没声明时两种都出。约定以外的去处不出，
/// 只记日志。
fn download_targets(action: &Value, plugin_id: &str, action_id: &str) -> Vec<DownloadTarget> {
    let Some(declared) = action.get("targets").and_then(Value::as_array).filter(|targets| !targets.is_empty()) else {
        return vec![DownloadTarget::LocalDirectory, DownloadTarget::WritableRepository];
    };
    declared
        .iter()
        .filter_map(|target| {
            let parsed = target.as_str().and_then(DownloadTarget::parse);
            if parsed.is_none() {
                eprintln!("Nana 文件插件动作的下载去处不在插件约定里，不出这一项：{plugin_id}/{action_id}（{target}）");
            }
            parsed
        })
        .collect()
}

fn action_method(plugin: &PluginManifest, action: &Value, operation: &str) -> Option<String> {
    if let Some(method) = text(action, "method") {
        return Some(method);
    }
    let key = match operation {
        "download-entry" => "downloadEntryMethod",
        "download-directory" => "downloadDirectoryMethod",
        "refresh-playback" => "preparePlaybackMethod",
        "clear-cache" => "clearCacheMethod",
        _ => return None,
    };
    plugin
        .contributes
        .get("source")
        .and_then(|source| source.get("media"))
        .and_then(|media| media.get(key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 声明和这一行对得上：`scope` 和条目类型相同（Vue `action.scope !== entry.kind` 就跳过，没写 scope 的声明不出），
/// 声明了来源时来源相同，声明了 `entryKind` 时来源载荷的条目种类相同（老载荷按歌曲、歌单标识认）。
fn matches_row(action: &Value, row: &FileRow) -> bool {
    let scope = action.get("scope").and_then(|value| value.as_str()).unwrap_or("");
    if scope != row.kind {
        return false;
    }
    if let Some(provider) = text(action, "providerId") {
        if row.provider_id.as_deref() != Some(provider.as_str()) {
            return false;
        }
    }
    let Some(entry_kind) = text(action, "entryKind") else {
        return true;
    };
    let actual = row.source_payload.as_ref().and_then(|payload| payload.get("entryKind")).and_then(|value| value.as_str());
    if actual == Some(entry_kind.as_str()) {
        return true;
    }
    match entry_kind.as_str() {
        "track" => row.kind == "file" && source_number(row, "songId").is_some(),
        "playlist-folder" => row.kind == "directory" && source_number(row, "playlistId").is_some(),
        _ => false,
    }
}

/// 条目自己标了登录过期（来源载荷或元数据里的 `loginExpired`，Vue `actionDisabled` 的条目部分）。
fn entry_login_expired(row: &FileRow) -> bool {
    let flagged = |value: Option<&Value>| value.and_then(Value::as_bool) == Some(true);
    flagged(row.source_payload.as_ref().and_then(|payload| payload.get("loginExpired"))) || flagged(row.metadata.get("loginExpired"))
}

/// 调用插件的载荷，照 Vue 各动作的载荷：歌曲、歌单标识，音质，公开的来源载荷，以及当前资源库的本地缓存目录
/// （`managedCacheRoot`，没有时写 null）；重新获取播放资源多一个 `forceRefresh`，下载多一个歌单名。下载的
/// `destination` 选好去处以后再写。
fn call_payload(row: &FileRow, operation: &str, context: &SourceContext) -> Value {
    let mut payload = Map::new();
    if let Some(song_id) = source_number(row, "songId") {
        payload.insert("songId".into(), Value::from(song_id));
    }
    if let Some(playlist_id) = source_number(row, "playlistId") {
        payload.insert("playlistId".into(), Value::from(playlist_id));
    }
    if matches!(operation, "download-entry" | "download-directory") {
        payload.insert("playlistName".into(), Value::String(playlist_name(row)));
    }
    payload.insert("level".into(), Value::String(source_string(row, "level").unwrap_or_else(|| "standard".into())));
    if operation == "refresh-playback" {
        payload.insert("forceRefresh".into(), Value::Bool(true));
    }
    payload.insert("managedCacheRoot".into(), context.managed_cache_root.clone().map_or(Value::Null, Value::String));
    payload.insert("sourcePayload".into(), public_payload(row.source_payload.as_ref()));
    Value::Object(payload)
}

/// 来源歌单名，没有时用条目名（Vue `sourceString(entry, "playlistName") ?? entry.name`）。
fn playlist_name(row: &FileRow) -> String {
    source_string(row, "playlistName").unwrap_or_else(|| row.name.clone())
}

/// 歌单下载到其他资源库时放进的文件夹：来源歌单名，路径分隔符换成「_」（导入只认一层文件夹名）；名字是空的或
/// 只有点时放在根目录。
fn download_folder(row: &FileRow) -> Option<String> {
    let name = playlist_name(row).replace(['/', '\\'], "_");
    let name = name.trim();
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_string())
}

fn public_payload(payload: Option<&Value>) -> Value {
    let Some(object) = payload.and_then(|value| value.as_object()) else {
        return Value::Object(Map::new());
    };
    let mut public = Map::new();
    for (key, value) in object {
        let normalized = key.to_ascii_lowercase();
        if normalized.contains("cookie") || normalized.contains("password") || normalized.contains("secret") || normalized == "token" {
            continue;
        }
        public.insert(key.clone(), value.clone());
    }
    Value::Object(public)
}

/// 条目在来源里的数字标识：先看来源载荷，再看元数据。都没有时，只有条目自己那一种标识退回它的来源标识
/// （`providerItemId`，条目在来源里的标识）：单曲的 `songId`、歌单目录的 `playlistId`。Vue `sourceNumber` 不分键，
/// 单曲没有 `playlistId` 时也拿歌曲的标识去填，这里不照抄。
fn source_number(row: &FileRow, key: &str) -> Option<i64> {
    row.source_payload
        .as_ref()
        .and_then(|payload| payload.get(key))
        .and_then(number_of)
        .or_else(|| row.metadata.get(key).and_then(number_of))
        .or_else(|| own_item_id(row, key).and_then(|id| id.parse().ok()))
}

/// 来源标识的文字（Vue `sourceNumber(..) ?? entry.providerItemId`）：数字标识照 [`source_number`]；条目自己那一种
/// 标识不是数字时原样用它的来源标识。
fn source_identifier(row: &FileRow, key: &str) -> Option<String> {
    source_number(row, key).map(|id| id.to_string()).or_else(|| own_item_id(row, key).map(str::to_string))
}

/// `key` 是条目自己那一种标识时（文件是单曲的 `songId`，目录是歌单的 `playlistId`），条目的来源标识；空白的不算。
fn own_item_id<'a>(row: &'a FileRow, key: &str) -> Option<&'a str> {
    let own = match row.kind.as_str() {
        "file" => "songId",
        "directory" => "playlistId",
        _ => return None,
    };
    if own != key {
        return None;
    }
    row.provider_item_id.as_deref().map(str::trim).filter(|id| !id.is_empty())
}

fn number_of(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn source_string(row: &FileRow, key: &str) -> Option<String> {
    let value = row.source_payload.as_ref().and_then(|payload| payload.get(key)).or_else(|| row.metadata.get(key))?;
    value.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|item| !item.is_empty()).map(str::to_string)
}

#[cfg(test)]
#[path = "entry_actions_tests.rs"]
mod tests;
