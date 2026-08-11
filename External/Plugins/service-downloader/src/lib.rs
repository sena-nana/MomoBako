//! 通用下载队列服务。
//!
//! 领域来源的认证、URL 解析和媒体缓存不属于本插件；这里仅维护 aria2 队列。

mod aria2_runtime;

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use aria2_runtime::Aria2Config;
use momobako_mutsuki_plugin_sdk::{
    export_mutsuki_momobako_plugin, PluginCallEnvelope, PluginRuntimeContext,
};
use serde::Deserialize;

const DEFAULT_ARIA2_DOWNLOAD_URL: &str =
    "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip";

#[derive(Debug)]
struct RuntimeContext {
    host_runtime: PluginRuntimeContext,
    plugin_data_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnqueueDownloadPayload {
    url: String,
    destination_path: String,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AwaitDownloadPayload {
    task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveDownloadPayload {
    task_id: String,
}

export_mutsuki_momobako_plugin!(
    "momobako.service.downloader",
    "0.2.0",
    protocols = [
        "downloader.ensureRuntime",
        "downloader.enqueueDownload",
        "downloader.awaitDownload",
        "downloader.removeDownload",
        "downloader.getRuntimeStatus",
    ],
    requires = [],
    permissions = ["network", "filesystem:write", "filesystem:read"],
    handle_call
);

fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    let runtime = runtime_context(request.runtime)?;
    match request.method.as_str() {
        "downloader.ensureRuntime" => ensure_runtime(&runtime),
        "downloader.enqueueDownload" => {
            decode(request.payload).and_then(|payload| enqueue_download(&runtime, payload))
        }
        "downloader.awaitDownload" => {
            decode(request.payload).and_then(|payload| await_download(&runtime, payload))
        }
        "downloader.removeDownload" => {
            decode(request.payload).and_then(|payload| remove_download(&runtime, payload))
        }
        "downloader.getRuntimeStatus" => get_runtime_status(&runtime),
        method => Err(format!("unsupported method: {method}")),
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn runtime_context(host_runtime: PluginRuntimeContext) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(host_runtime.plugin_data_dir.clone());
    fs::create_dir_all(plugin_data_dir.join("downloads")).map_err(io_error)?;
    Ok(RuntimeContext {
        host_runtime,
        plugin_data_dir,
    })
}

fn ensure_runtime(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let snapshot = aria2_runtime::ensure_runtime(&aria2_config(runtime))?;
    Ok(serde_json::json!({
        "runtime": "aria2",
        "downloadsDir": snapshot.downloads_dir.to_string_lossy(),
        "helperDir": snapshot.helper_dir.to_string_lossy(),
        "downloadUrl": DEFAULT_ARIA2_DOWNLOAD_URL,
        "aria2": snapshot.status,
        "queueSize": snapshot.queue_size
    }))
}

fn enqueue_download(
    runtime: &RuntimeContext,
    payload: EnqueueDownloadPayload,
) -> Result<serde_json::Value, String> {
    let record = aria2_runtime::enqueue_download(
        &aria2_config(runtime),
        &payload.url,
        Path::new(&payload.destination_path),
        payload.metadata,
    )?;
    Ok(serde_json::json!({
        "taskId": record.task_id,
        "gid": record.gid,
        "status": record.status,
        "destinationPath": record.destination_path
    }))
}

fn await_download(
    runtime: &RuntimeContext,
    payload: AwaitDownloadPayload,
) -> Result<serde_json::Value, String> {
    let record = aria2_runtime::await_download(
        &aria2_config(runtime),
        &payload.task_id,
        Duration::from_secs(300),
    )?;
    serde_json::to_value(record).map_err(|error| error.to_string())
}

fn remove_download(
    runtime: &RuntimeContext,
    payload: RemoveDownloadPayload,
) -> Result<serde_json::Value, String> {
    aria2_runtime::remove_download(&aria2_config(runtime), &payload.task_id)?;
    Ok(serde_json::json!({ "taskId": payload.task_id, "removed": true }))
}

fn get_runtime_status(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let snapshot = aria2_runtime::runtime_status(&aria2_config(runtime))?;
    Ok(serde_json::json!({
        "runtime": "aria2",
        "aria2": snapshot.status,
        "queueSize": snapshot.queue_size,
        "downloadsDir": snapshot.downloads_dir.to_string_lossy(),
        "downloadUrl": DEFAULT_ARIA2_DOWNLOAD_URL
    }))
}

fn aria2_config(runtime: &RuntimeContext) -> Aria2Config<'_> {
    Aria2Config {
        host_runtime: Some(&runtime.host_runtime),
        plugin_data_dir: &runtime.plugin_data_dir,
        download_url: DEFAULT_ARIA2_DOWNLOAD_URL,
    }
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_enqueue_payload_accepts_metadata() {
        let payload: EnqueueDownloadPayload = serde_json::from_value(serde_json::json!({
            "url": "https://example.invalid/file.zip",
            "destinationPath": "download/file.zip",
            "metadata": { "kind": "archive" }
        }))
        .expect("payload should deserialize");
        assert_eq!(payload.metadata.expect("metadata")["kind"], "archive");
    }
}
