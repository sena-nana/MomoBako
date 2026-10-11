//! 在线条目的播放源：经 Mutsuki 协议 `momobako.playback.prepare` 让来源插件把音频准备到本地，再读这个本地文件。
//!
//! 网易云这类来源资源库里的单曲是虚拟条目（`is_virtual`），仓库里没有文件，`read_file` 读不到。Vue 的播放器和
//! 媒体预览插件都经宿主 SDK 的 `prepareEntryPlaybackSourceWithProgress` 跑这个协议；Nana 的预览页和播放条遇到
//! 在线条目时在任务线程里同样跑一次（`inspect_dispatch.rs`、`player_dispatch.rs`），本地条目照旧直接读字节。
//!
//! - Nana 的解码只认本地文件：用响应里的 `localPath`，没有时用 `tempFilePath`（网易云插件两者相同，都在资源库
//!   本地缓存的 `.momo/cache/netease-playback` 下）。来源只交回网址（`sourceUrl`）时不另去下载它，按失败交回。
//! - 进度事件随结果一起交回（`docs/api-design.md`），准备期间看不到中间阶段，界面停在「准备音频」和不定进度，
//!   和 Vue 当时经 Tauri 拿到的一样。
//! - 切曲、停止、换仓库时不取消任务：`execute` 不交出任务编号，来源插件下载中途也不看取消。旧的结果按代次丢弃
//!   （`player_clip::finish_load`、`inspect_support::note_media`）。
//! - 后端在播放源准备好时写下访问时间，结果里带上 [`SourcePrep::prepared`]，归约当场改「最近使用」。
//! - 准备失败（登录失效、网络、缓存目录不可用）记进系统日志，交回来源的原话，预览框和播放条给「重试」。

use serde_json::{json, Value};

use crate::backend::services::logging::write_log;
use crate::backend::services::mutsuki_runner::PROTOCOL_PLAYBACK_PREPARE;
use crate::backend::services::repository::{EntryPlaybackRequest, SystemLogLocationInput, SystemLogWriteRequest};
use crate::backend::viewmodels::MutsukiTaskViewModel;
use crate::shell::SourcePrep;

/// 来源只交回网址、没有本地文件时的原因。
pub(crate) const URL_ONLY: &str = "来源只交回了网络地址，没有本地文件，无法播放";

/// 是谁要准备播放源：日志里分开写，和 Vue 的 `audioRuntime.prepareSource`、`preview.prepareSource` 对应。
#[derive(Clone, Copy, Debug)]
pub(crate) enum Requester {
    Player,
    Preview,
}

impl Requester {
    fn action(self) -> &'static str {
        match self {
            Self::Player => "player.prepareSource",
            Self::Preview => "preview.prepareSource",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::Player => "播放列表播放源准备失败。",
            Self::Preview => "预览播放源准备失败。",
        }
    }
}

/// 在任务线程里准备一条在线条目的播放源并读出字节。成功时附带后端写下的访问；失败交回原因，标成可以重试。
pub(crate) fn prepare_and_read(
    tasks: &MutsukiTaskViewModel,
    executor: &tokio::runtime::Runtime,
    requester: Requester,
    repo_id: &str,
    path: &str,
) -> (Result<Vec<u8>, String>, SourcePrep) {
    let request = EntryPlaybackRequest { repo_id: repo_id.to_string(), path: path.to_string() };
    let local = executor
        .block_on(tasks.execute(PROTOCOL_PLAYBACK_PREPARE, request))
        .and_then(|(output, _)| local_source(&output));
    let local = match local {
        Ok(local) => local,
        Err(error) => {
            report_failure(requester, repo_id, path, &error);
            return (Err(error), SourcePrep::failed());
        }
    };
    let prepared = SourcePrep::prepared(repo_id, path, &crate::shell::admin::support::now_iso8601());
    match std::fs::read(&local) {
        Ok(bytes) => (Ok(bytes), prepared),
        Err(error) => {
            let reason = format!("读取播放源失败：{error}");
            report_failure(requester, repo_id, path, &format!("{reason}（{local}）"));
            (Err(reason), SourcePrep { retryable: true, ..prepared })
        }
    }
}

/// 从准备好的结果（`EntryPlaybackSourceResponse`）里取本地文件：`localPath`，没有时 `tempFilePath`。
/// 只有网址时没有能读的文件。
pub(crate) fn local_source(output: &Value) -> Result<String, String> {
    let Some(fields) = output.as_object() else {
        return Err(format!("播放源结果无法识别：{output}"));
    };
    let filled = |key: &str| fields.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    if let Some(local) = filled("localPath").or_else(|| filled("tempFilePath")) {
        return Ok(local);
    }
    match filled("sourceUrl") {
        Some(url) => {
            eprintln!("Nana 播放源只有网址，Nana 只解本地文件：{url}");
            Err(URL_ONLY.into())
        }
        None => Err("播放源不可用".into()),
    }
}

/// 准备失败写进标准错误和系统日志，记在这个资源库名下（Vue 插件 `ctx.logger.error`）。
fn report_failure(requester: Requester, repo_id: &str, path: &str, error: &str) {
    eprintln!("Nana {}{path}：{error}", requester.message());
    let request = SystemLogWriteRequest {
        level: "error".into(),
        category: "playback".into(),
        action: requester.action().into(),
        message: requester.message().into(),
        context: Some(json!({ "path": path, "message": error })),
        repo_id: Some(repo_id.to_string()),
        plugin_id: None,
        source_kind: Some("host".into()),
        source_label: None,
        location: Some(SystemLogLocationInput { module_path: Some(module_path!().into()), file: Some(file!().into()), line: Some(line!()) }),
    };
    if let Err(error) = write_log(request) {
        eprintln!("Nana 写入播放源失败日志失败：{error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本地文件优先用 `localPath`，没有时用 `tempFilePath`；后端补的回环网址不影响。
    #[test]
    fn local_source_prefers_the_local_file() {
        let output = json!({ "localPath": "D:/cache/a.mp3", "tempFilePath": "D:/cache/b.mp3", "sourceUrl": "http://127.0.0.1/x" });
        assert_eq!(local_source(&output).as_deref(), Ok("D:/cache/a.mp3"));
        let output = json!({ "localPath": " ", "tempFilePath": "D:/cache/b.mp3" });
        assert_eq!(local_source(&output).as_deref(), Ok("D:/cache/b.mp3"));
    }

    /// 只有网址时不去下载它，按失败交回；什么都没有、结果认不出也是失败。
    #[test]
    fn url_only_or_empty_sources_fail() {
        assert_eq!(local_source(&json!({ "sourceUrl": "https://music.example/a.mp3" })), Err(URL_ONLY.to_string()));
        assert_eq!(local_source(&json!({})), Err("播放源不可用".to_string()));
        assert!(local_source(&json!("not an object")).unwrap_err().starts_with("播放源结果无法识别"));
    }
}
