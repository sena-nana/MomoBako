//! 网易云音乐来源插件。
//!
//! 认证、虚拟目录和媒体解析都由 Source 自己负责；Cookie 仅保存在系统凭据库中。

mod actions;
mod auth;
mod catalog;
mod client;
mod media;
mod models;
mod util;

use std::{fs, path::PathBuf};

use momobako_mutsuki_plugin_sdk::{
    export_mutsuki_momobako_plugin, write_host_log_silently, PluginCallEnvelope,
    PluginRuntimeContext,
};

use crate::models::{
    ClearTrackCachePayload, DownloadPlaylistPackagePayload, DownloadTrackPackagePayload,
    PluginPayload, PrepareTrackPlaybackPayload, ResolveLyricsPayload, RuntimeContext,
};

export_mutsuki_momobako_plugin!(
    "momobako.netease.source",
    "0.2.0",
    protocols = [
        "auth.createQrSession",
        "auth.pollQrSession",
        "auth.getLoginStatus",
        "auth.clearLogin",
        "auth.migrateRepositoryCredential",
        "source.describeActions",
        "media.prepareTrackPlayback",
        "media.downloadTrackPackage",
        "media.downloadPlaylistPackage",
        "media.resolveLyrics",
        "media.clearTrackCache",
        "filesystem.ensureAttachable",
        "filesystem.prepareRepositoryRoot",
        "filesystem.listFiles",
        "filesystem.listTree",
        "filesystem.listDirectory",
        "filesystem.listDirectoryPage",
        "filesystem.statEntry",
    ],
    requires = [],
    permissions = [
        "network",
        "filesystem:read",
        "filesystem:write",
        "credential:read",
        "credential:write"
    ],
    handle_call
);

/// 分发 ABI 调用；错误日志只记录方法名，不记录可能包含旧 Cookie 的载荷。
fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    let method = request.method.clone();
    let runtime = runtime_context(request.runtime, request.payload.get("config"))?;
    let result = match method.as_str() {
        "auth.createQrSession" => {
            decode(request.payload).and_then(|p| auth::create_qr_session(&runtime, p))
        }
        "auth.pollQrSession" => {
            decode(request.payload).and_then(|p| auth::poll_qr_session(&runtime, p))
        }
        "auth.getLoginStatus" => auth::get_login_status(&runtime),
        "auth.clearLogin" => auth::clear_login(&runtime),
        "auth.migrateRepositoryCredential" => auth::migrate_repository_credential(&runtime),
        "source.describeActions" => Ok(actions::describe()),
        "media.prepareTrackPlayback" => decode::<PrepareTrackPlaybackPayload>(request.payload)
            .and_then(|p| media::prepare_track_playback(&runtime, p)),
        "media.downloadTrackPackage" => decode::<DownloadTrackPackagePayload>(request.payload)
            .and_then(|p| media::download_track_package(&runtime, p)),
        "media.downloadPlaylistPackage" => {
            decode::<DownloadPlaylistPackagePayload>(request.payload)
                .and_then(|p| media::download_playlist_package(&runtime, p))
        }
        "media.resolveLyrics" => decode::<ResolveLyricsPayload>(request.payload)
            .and_then(|p| media::resolve_lyrics(&runtime, p)),
        "media.clearTrackCache" => decode::<ClearTrackCachePayload>(request.payload)
            .and_then(|p| media::clear_track_cache(&runtime, p)),
        "filesystem.ensureAttachable" => Ok(serde_json::json!({})),
        "filesystem.prepareRepositoryRoot" => decode::<PluginPayload>(request.payload)
            .and_then(|p| prepare_repository_root(p.repo_root.as_deref())),
        "filesystem.listFiles" => catalog::list_files(&runtime),
        "filesystem.listTree" => catalog::list_tree(&runtime),
        "filesystem.listDirectory" => decode::<PluginPayload>(request.payload).and_then(|p| {
            catalog::list_directory(&runtime, p.directory_path.as_deref().unwrap_or_default())
        }),
        "filesystem.listDirectoryPage" => decode::<PluginPayload>(request.payload).and_then(|p| {
            catalog::list_directory_page(
                &runtime,
                p.directory_path.as_deref().unwrap_or_default(),
                p.offset.unwrap_or(0),
                p.limit,
            )
        }),
        "filesystem.statEntry" => decode::<PluginPayload>(request.payload).and_then(|p| {
            catalog::stat_entry(&runtime, p.entry_path.as_deref().unwrap_or_default())
        }),
        "filesystem.createDirectory"
        | "filesystem.createFile"
        | "filesystem.renameEntry"
        | "filesystem.moveEntry"
        | "filesystem.deleteEntry" => Err("网易云音乐资源库当前为只读虚拟源".to_string()),
        _ => Err(format!("unsupported method: {method}")),
    };
    if let Err(error) = &result {
        write_host_log_silently(
            &runtime.host_runtime,
            "error",
            "sourceCallFailed",
            "网易云来源调用失败。",
            serde_json::json!({ "method": method, "error": error }),
        );
    }
    result
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn runtime_context(
    host_runtime: PluginRuntimeContext,
    config: Option<&serde_json::Value>,
) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(host_runtime.plugin_data_dir.clone());
    fs::create_dir_all(plugin_data_dir.join("temp")).map_err(util::io_error)?;
    fs::create_dir_all(plugin_data_dir.join("exports")).map_err(util::io_error)?;
    let runtime = RuntimeContext::new(host_runtime, plugin_data_dir, config);
    media::clear_expired_temp_files(&runtime)?;
    Ok(runtime)
}

fn prepare_repository_root(repo_root: Option<&str>) -> Result<serde_json::Value, String> {
    if let Some(path) = repo_root.map(str::trim).filter(|value| !value.is_empty()) {
        fs::create_dir_all(path).map_err(util::io_error)?;
    }
    Ok(serde_json::json!({}))
}

#[cfg(test)]
mod tests;
