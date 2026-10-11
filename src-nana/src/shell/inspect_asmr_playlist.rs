//! ASMR 播放列表：从作品队列里加入的音轨，按资源库分开记。
//!
//! 对应 Vue `library-asmr/src/asmrLibrary.js` 的播放列表：加入作品、随机加一首、清空、点条目打开预览。
//! Vue 存在本地存储 `momobako.library.asmr.playlist.v1`，这里存在设置目录的 `asmr-playlist.json`，
//! 重启以后还在；归约只标脏，由宿主在更新末尾写文件（`lib.rs`）。条目的标题和收听状态在条目还在当前列表里时
//! 按它现在的元数据重算（Vue `resolveActivePlaylist`）。作品里的音轨按自然序排，`2.mp3` 在 `10.mp3` 前面。

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::super::files::FileRow;
use super::super::inspect_library::listening_label;
use super::super::search_collate::compare_zh;
use super::super::ShellViewModel;
use super::{entry_text, is_asmr_audio, AsmrUi};

/// 播放列表里的一项。存进文件的字段名和 Vue 存的一致（`repoId`、`workTitle` 这样的驼峰）。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlaylistItem {
    repo_id: String,
    path: String,
    title: String,
    work_title: String,
    /// 收听状态的标签（「未收听」这样的中文）。
    status: String,
    work_root: Option<String>,
    track_path: Option<String>,
    asset_id: Option<String>,
}

impl PlaylistItem {
    /// 存进文件的样子。可选字段没有时不写。
    fn to_value(&self) -> Value {
        let mut object = Map::new();
        for (key, value) in [("repoId", &self.repo_id), ("path", &self.path), ("title", &self.title), ("workTitle", &self.work_title), ("status", &self.status)] {
            object.insert(key.into(), Value::String(value.clone()));
        }
        for (key, value) in [("workRoot", &self.work_root), ("trackPath", &self.track_path), ("assetId", &self.asset_id)] {
            if let Some(value) = value {
                object.insert(key.into(), Value::String(value.clone()));
            }
        }
        Value::Object(object)
    }

    /// 从文件里的一项读回（Vue `parseStoredPlaylist`）：要有非空的路径；标题没有时取路径的最后一段，别的字段缺省为空。
    fn from_value(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let path = text("path").filter(|path| !path.trim().is_empty())?;
        let title = text("title").filter(|title| !title.is_empty()).unwrap_or_else(|| path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string());
        Some(Self {
            repo_id: text("repoId").unwrap_or_default(),
            title,
            work_title: text("workTitle").unwrap_or_default(),
            status: text("status").unwrap_or_default(),
            work_root: text("workRoot"),
            track_path: text("trackPath"),
            asset_id: text("assetId"),
            path,
        })
    }
}

/// 全部资源库的播放列表和「改过、还没写文件」的标记。
#[derive(Clone, Debug, Default)]
pub(super) struct Playlist {
    items: Vec<PlaylistItem>,
    dirty: bool,
}

/// 预览列下方「播放列表」卡片要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlaylistPanel {
    pub count: String,
    /// 当前文件是 ASMR 音频，能加入作品或随机。
    pub audio: bool,
    /// 这个资源库的条目：路径和文字（当前项前面写「当前」）。
    pub items: Vec<(String, String)>,
}

/// 播放列表文件的位置：设置文件旁边，和侧栏、展示方式这些界面偏好放在一起。
pub fn asmr_playlist_path() -> PathBuf {
    crate::settings::default_path().with_file_name("asmr-playlist.json")
}

impl AsmrUi {
    /// 启动时读播放列表。文件不在是空列表；读不了、不是 JSON 数组时记日志后从空列表开始，坏的项跳过。
    pub fn load_playlist_file(&mut self, path: &Path) {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                eprintln!("Nana 读取 ASMR 播放列表失败：{}：{error}", path.display());
                return;
            }
        };
        self.playlist.items = parse_items(&raw);
        self.playlist.dirty = false;
    }

    /// 播放列表改过、还没写文件。取走标记。
    pub fn take_playlist_dirty(&mut self) -> bool {
        std::mem::take(&mut self.playlist.dirty)
    }

    /// 写播放列表文件。写不进去时记日志，下次改动再写。
    pub fn save_playlist_file(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("Nana 创建 ASMR 播放列表目录失败：{}：{error}", parent.display());
                return;
            }
        }
        let items = Value::Array(self.playlist.items.iter().map(PlaylistItem::to_value).collect());
        let body = match serde_json::to_string_pretty(&items) {
            Ok(body) => body,
            Err(error) => {
                eprintln!("Nana ASMR 播放列表无法序列化：{error}");
                return;
            }
        };
        if let Err(error) = fs::write(path, body + "\n") {
            eprintln!("Nana 保存 ASMR 播放列表失败：{}：{error}", path.display());
        }
    }
}

/// 解析播放列表文件：JSON 数组，没有路径的项跳过（Vue `parseStoredPlaylist`）。
fn parse_items(raw: &str) -> Vec<PlaylistItem> {
    let Ok(Value::Array(values)) = serde_json::from_str::<Value>(raw) else {
        eprintln!("Nana ASMR 播放列表文件不是 JSON 数组，从空列表开始");
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| {
            let item = PlaylistItem::from_value(value);
            if item.is_none() {
                eprintln!("Nana ASMR 播放列表跳过没有路径的项：{value}");
            }
            item
        })
        .collect()
}

/// 从一行建播放列表项（Vue `createPlaylistItem`）：只收 ASMR 音频；标题是音轨名，没有时是文件名。
fn create_item(repo_id: &str, row: &FileRow, metadata: &BTreeMap<String, Value>) -> Option<PlaylistItem> {
    if repo_id.is_empty() || !is_asmr_audio(&row.kind, metadata) {
        return None;
    }
    let optional = |key: &str| Some(entry_text(metadata, key)).filter(|text| !text.is_empty());
    Some(PlaylistItem {
        repo_id: repo_id.to_string(),
        path: row.path.clone(),
        title: optional("trackTitle").unwrap_or_else(|| row.name.clone()),
        work_title: entry_text(metadata, "workTitle"),
        status: listening_label(&entry_text(metadata, "listeningStatus")),
        work_root: optional("workRoot"),
        track_path: optional("trackPath"),
        asset_id: row.asset_id.clone(),
    })
}

/// 当前资源库的 id。
fn repo_id(model: &ShellViewModel) -> String {
    model.workspace.active_repo_id.clone().or_else(|| model.repository_id.clone()).unwrap_or_default()
}

/// 这个资源库的条目：还在当前列表里、仍是 ASMR 音频的按现在的元数据重算标题和状态，其余照存下的
/// （Vue `resolveActivePlaylist`）。
fn resolved(model: &ShellViewModel, repo: &str) -> Vec<PlaylistItem> {
    let rows = super::visible_rows(model);
    model
        .asmr
        .playlist
        .items
        .iter()
        .filter(|item| item.repo_id == repo || item.repo_id.is_empty())
        .map(|item| {
            rows.iter()
                .find(|row| row.path == item.path)
                .and_then(|row| create_item(repo, row, &super::row_metadata(model, row)))
                .unwrap_or_else(|| PlaylistItem { repo_id: if item.repo_id.is_empty() { repo.to_string() } else { item.repo_id.clone() }, ..item.clone() })
        })
        .collect()
}

/// 「播放列表」卡片：当前文件是 ASMR 音频，或者这个资源库已经有条目时才有。
pub(super) fn panel(model: &ShellViewModel) -> Option<PlaylistPanel> {
    let audio = super::current_row(model).is_some_and(|row| is_asmr_audio(&row.kind, &super::row_metadata(model, row)));
    let items = resolved(model, &repo_id(model));
    if !audio && items.is_empty() {
        return None;
    }
    let current = model.inspect.target_path.clone().unwrap_or_default();
    let items = items
        .into_iter()
        .map(|item| {
            let note = if item.status.is_empty() { item.work_title.clone() } else { item.status.clone() };
            let label = if note.is_empty() { item.title.clone() } else { format!("{} {note}", item.title) };
            let label = if item.path == current { format!("当前 {label}") } else { label };
            (item.path, label)
        })
        .collect::<Vec<_>>();
    Some(PlaylistPanel { count: format!("{} 项", items.len()), audio, items })
}

/// 把几行加进播放列表，已经在的（同一资源库、同一路径）跳过（Vue `mergePlaylistEntries`）。
fn merge(model: &mut ShellViewModel, paths: Vec<String>) {
    let repo = repo_id(model);
    if repo.is_empty() {
        eprintln!("Nana ASMR 播放列表没有活动资源库");
        return;
    }
    let rows = super::visible_rows(model);
    let fresh = paths
        .iter()
        .filter_map(|path| rows.iter().find(|row| row.path == *path))
        .filter_map(|row| create_item(&repo, row, &super::row_metadata(model, row)))
        .collect::<Vec<_>>();
    let playlist = &mut model.asmr.playlist;
    for item in fresh {
        if playlist.items.iter().any(|existing| (existing.repo_id == item.repo_id || existing.repo_id.is_empty()) && existing.path == item.path) {
            continue;
        }
        playlist.items.push(item);
        playlist.dirty = true;
    }
}

/// 「加入作品」：当前作品的全部音轨按顺序加进来。
pub(super) fn add_work(model: &mut ShellViewModel) {
    let paths = super::work_audio(model).into_iter().map(|row| row.path.clone()).collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("Nana ASMR 当前文件不是作品里的音频，没有可加入的音轨");
        return;
    }
    merge(model, paths);
}

/// 「随机」：从当前作品还没加入的音轨里随机挑一首，都加过时从全部音轨里挑（Vue `addRandom`）。
pub(super) fn add_random(model: &mut ShellViewModel) {
    add_random_with(model, random_index);
}

/// 同上，`pick(n)` 在 `0..n` 里挑一个下标。
fn add_random_with(model: &mut ShellViewModel, pick: impl FnOnce(usize) -> usize) {
    let repo = repo_id(model);
    let work = super::work_audio(model).into_iter().map(|row| row.path.clone()).collect::<Vec<_>>();
    let fresh = work
        .iter()
        .filter(|path| !model.asmr.playlist.items.iter().any(|item| (item.repo_id == repo || item.repo_id.is_empty()) && item.path == **path))
        .cloned()
        .collect::<Vec<_>>();
    let pool = if fresh.is_empty() { work } else { fresh };
    if pool.is_empty() {
        eprintln!("Nana ASMR 当前作品没有可随机的音轨");
        return;
    }
    let index = pick(pool.len()).min(pool.len() - 1);
    merge(model, vec![pool[index].clone()]);
}

/// `0..len` 里的一个随机下标。标准库的 `RandomState` 每次带新的随机键，用它散列当前时刻。
fn random_index(len: usize) -> usize {
    use std::hash::BuildHasher;
    let seed = std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now());
    (seed % len.max(1) as u64) as usize
}

/// 「清空」：只清当前资源库的条目（Vue `clearPlaylistForRepo`，没记资源库的旧条目一起清）。
pub(super) fn clear(model: &mut ShellViewModel) {
    let repo = repo_id(model);
    if repo.is_empty() {
        return;
    }
    let playlist = &mut model.asmr.playlist;
    let before = playlist.items.len();
    playlist.items.retain(|item| item.repo_id != repo && !item.repo_id.is_empty());
    if playlist.items.len() != before {
        playlist.dirty = true;
    }
}

/// 音轨排序用的键：音轨路径，没有时是文件路径（Vue `sortAudioEntries`）。
pub(super) fn track_key(row: &FileRow, metadata: &BTreeMap<String, Value>) -> String {
    Some(entry_text(metadata, "trackPath")).filter(|text| !text.is_empty()).unwrap_or_else(|| row.path.clone())
}

/// Vue `localeCompare(.., "zh-CN", { numeric: true })`：连续的数字按数值比，其余按 zh-CN 比。
///
/// 算法：两边都切成数字段和非数字段逐段比；都是数字段时去掉前导 0 先比位数再比字面，否则按 zh-CN 比这两段；
/// 前面都相等时段少的在前，最后按整串的 zh-CN 顺序定（`01` 和 `1` 这类只差前导 0 的）。
pub(super) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (a, b) = (segments(left), segments(right));
    for (x, y) in a.iter().zip(&b) {
        let order = match (x.parse::<u128>().is_ok(), y.parse::<u128>().is_ok()) {
            (true, true) => {
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            }
            _ => compare_zh(x, y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len()).then_with(|| compare_zh(left, right))
}

/// 切成连续的 ASCII 数字段和其余段。
fn segments(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut digit = None;
    for (index, ch) in text.char_indices() {
        let is_digit = ch.is_ascii_digit();
        if digit.is_some_and(|previous| previous != is_digit) {
            parts.push(&text[start..index]);
            start = index;
        }
        digit = Some(is_digit);
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

#[cfg(test)]
#[path = "inspect_asmr_playlist_tests.rs"]
mod tests;
