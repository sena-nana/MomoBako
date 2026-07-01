//! aria2 运行时管理与通用下载任务。

use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use zip::ZipArchive;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const RPC_PORT: u16 = 16831;
const WAIT_INTERVAL_MS: u64 = 300;
const DEFAULT_RPC_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub struct Aria2Config<'a> {
    pub plugin_data_dir: &'a Path,
    pub download_url: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aria2StatusRecord {
    pub running: bool,
    pub pid: Option<u32>,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub rpc_url: Option<String>,
    pub secret: Option<String>,
    pub source: Option<String>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
    pub download_url: String,
    pub bundled_archive_path: Option<String>,
}

impl Aria2StatusRecord {
    fn idle(download_url: &str, archive_path: &Path) -> Self {
        Self {
            running: false,
            pid: None,
            executable_path: None,
            version: None,
            rpc_url: None,
            secret: None,
            source: None,
            updated_at: None,
            error: None,
            download_url: download_url.to_string(),
            bundled_archive_path: Some(archive_path.to_string_lossy().to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskRecord {
    pub task_id: String,
    pub gid: String,
    pub url: String,
    pub destination_path: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub total_length: Option<i64>,
    #[serde(default)]
    pub completed_length: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RuntimeSnapshot {
    pub downloads_dir: PathBuf,
    pub helper_dir: PathBuf,
    pub status: Aria2StatusRecord,
    pub queue_size: usize,
}

struct Aria2Paths {
    downloads_dir: PathBuf,
    runtime_dir: PathBuf,
    archive_path: PathBuf,
    helper_dir: PathBuf,
    tasks_dir: PathBuf,
    session_path: PathBuf,
    status_path: PathBuf,
    pid_path: PathBuf,
}

pub fn ensure_runtime(config: &Aria2Config<'_>) -> Result<RuntimeSnapshot, String> {
    let paths = runtime_paths(config.plugin_data_dir);
    fs::create_dir_all(&paths.downloads_dir).map_err(io_error)?;
    fs::create_dir_all(&paths.runtime_dir).map_err(io_error)?;
    fs::create_dir_all(&paths.helper_dir).map_err(io_error)?;
    fs::create_dir_all(&paths.tasks_dir).map_err(io_error)?;
    if !paths.session_path.exists() {
        fs::write(&paths.session_path, "").map_err(io_error)?;
    }

    cleanup_stale_state(&paths)?;
    if let Some(status) = load_status(&paths.status_path)? {
        if status.running && status_record_healthy(&status)? {
            return runtime_snapshot(config, &paths, status);
        }
    }

    let (executable_path, source) = resolve_aria2_executable(config, &paths)?;
    let secret = status_secret(&paths.status_path)?.unwrap_or_else(generate_secret);
    let rpc_url = format!("http://127.0.0.1:{RPC_PORT}/jsonrpc");
    let child = spawn_aria2_process(
        &executable_path,
        &paths,
        &rpc_url,
        &secret,
        &paths.downloads_dir,
    )?;
    let pid = child.id();
    fs::write(&paths.pid_path, pid.to_string()).map_err(io_error)?;

    let mut status = Aria2StatusRecord {
        running: false,
        pid: Some(pid),
        executable_path: Some(executable_path.to_string_lossy().to_string()),
        version: None,
        rpc_url: Some(rpc_url),
        secret: Some(secret),
        source: Some(source),
        updated_at: Some(now_rfc3339()?),
        error: None,
        download_url: config.download_url.to_string(),
        bundled_archive_path: Some(paths.archive_path.to_string_lossy().to_string()),
    };
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(250));
        if let Ok(version) = rpc_get_version(&status) {
            status.running = true;
            status.version = Some(version);
            status.updated_at = Some(now_rfc3339()?);
            status.error = None;
            save_status(&paths.status_path, &status)?;
            return runtime_snapshot(config, &paths, status);
        }
    }
    status.error = Some("aria2 守护进程启动后未通过健康检查。".to_string());
    status.updated_at = Some(now_rfc3339()?);
    save_status(&paths.status_path, &status)?;
    Err("aria2 守护进程启动失败。".to_string())
}

pub fn runtime_status(config: &Aria2Config<'_>) -> Result<RuntimeSnapshot, String> {
    let paths = runtime_paths(config.plugin_data_dir);
    fs::create_dir_all(&paths.tasks_dir).map_err(io_error)?;
    let status = load_status(&paths.status_path)?
        .unwrap_or_else(|| Aria2StatusRecord::idle(config.download_url, &paths.archive_path));
    runtime_snapshot(config, &paths, status)
}

pub fn enqueue_download(
    config: &Aria2Config<'_>,
    url: &str,
    destination_path: &Path,
    metadata: Option<serde_json::Value>,
) -> Result<DownloadTaskRecord, String> {
    let runtime = ensure_runtime(config)?;
    let parent = destination_path
        .parent()
        .ok_or_else(|| format!("download destination is missing parent directory: {}", destination_path.display()))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let file_name = destination_path
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("download destination filename is invalid: {}", destination_path.display()))?;
    let payload = rpc_request(
        &runtime.status,
        "aria2.addUri",
        vec![
            serde_json::json!([url]),
            serde_json::json!({
                "dir": parent.to_string_lossy().to_string(),
                "out": file_name,
                "continue": "true",
                "allow-overwrite": "true",
                "auto-file-renaming": "false",
            }),
        ],
    )?;
    let gid = payload
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "aria2.addUri did not return a gid".to_string())?;
    let record = DownloadTaskRecord {
        task_id: gid.clone(),
        gid,
        url: url.to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        metadata,
        status: "queued".to_string(),
        created_at: now_rfc3339()?,
        finished_at: None,
        total_length: None,
        completed_length: None,
        error: None,
    };
    save_task(&paths_for_snapshot(&runtime).tasks_dir, &record)?;
    Ok(record)
}

pub fn await_download(
    config: &Aria2Config<'_>,
    task_id: &str,
    timeout: Duration,
) -> Result<DownloadTaskRecord, String> {
    let runtime = ensure_runtime(config)?;
    let paths = paths_for_snapshot(&runtime);
    let mut record = load_task(&paths.tasks_dir, task_id)?;
    let started_at = std::time::Instant::now();
    loop {
        let status_payload = rpc_request(
            &runtime.status,
            "aria2.tellStatus",
            vec![
                serde_json::json!(record.gid.clone()),
                serde_json::json!(["gid", "status", "totalLength", "completedLength", "errorMessage"]),
            ],
        )?;
        update_task_from_status_payload(&mut record, &status_payload)?;
        save_task(&paths.tasks_dir, &record)?;
        if matches!(record.status.as_str(), "completed" | "failed" | "removed") {
            return Ok(record);
        }
        if started_at.elapsed() >= timeout {
            record.status = "failed".to_string();
            record.error = Some(format!("download task timed out: {task_id}"));
            if record.finished_at.is_none() {
                record.finished_at = Some(now_rfc3339()?);
            }
            save_task(&paths.tasks_dir, &record)?;
            return Ok(record);
        }
        thread::sleep(Duration::from_millis(WAIT_INTERVAL_MS));
    }
}

pub fn remove_download(config: &Aria2Config<'_>, task_id: &str) -> Result<(), String> {
    let runtime = ensure_runtime(config)?;
    let paths = paths_for_snapshot(&runtime);
    let record = load_task(&paths.tasks_dir, task_id)?;
    let _ = rpc_request(
        &runtime.status,
        "aria2.remove",
        vec![serde_json::json!(record.gid.clone())],
    );
    let _ = rpc_request(
        &runtime.status,
        "aria2.removeDownloadResult",
        vec![serde_json::json!(record.gid)],
    );
    let task_path = task_path(&paths.tasks_dir, task_id);
    if task_path.is_file() {
        fs::remove_file(task_path).map_err(io_error)?;
    }
    Ok(())
}

pub fn download_via_aria2(
    config: &Aria2Config<'_>,
    url: &str,
    destination_path: &Path,
    metadata: Option<serde_json::Value>,
    timeout: Duration,
) -> Result<DownloadTaskRecord, String> {
    #[cfg(test)]
    {
        return download_via_http_for_test(url, destination_path, metadata);
    }
    #[cfg(not(test))]
    let _ = metadata;
    #[cfg(not(test))]
    let record = enqueue_download(config, url, destination_path, metadata)?;
    #[cfg(not(test))]
    let record = await_download(config, &record.task_id, timeout)?;
    #[cfg(not(test))]
    if record.status == "completed" {
        Ok(record)
    } else {
        Err(record
            .error
            .clone()
            .unwrap_or_else(|| format!("aria2 download task ended with status {}", record.status)))
    }
}

#[cfg(test)]
fn download_via_http_for_test(
    url: &str,
    destination_path: &Path,
    metadata: Option<serde_json::Value>,
) -> Result<DownloadTaskRecord, String> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(http_error)?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(http_error)?;
    let mut file = fs::File::create(destination_path).map_err(io_error)?;
    let mut buffer = [0_u8; 16 * 1024];
    let mut completed_length = 0_i64;
    loop {
        let read = response.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(io_error)?;
        completed_length += read as i64;
    }
    file.flush().map_err(io_error)?;
    let finished_at = now_rfc3339()?;
    Ok(DownloadTaskRecord {
        task_id: format!("test-{}", finished_at),
        gid: format!("test-{}", finished_at),
        url: url.to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        metadata,
        status: "completed".to_string(),
        created_at: finished_at.clone(),
        finished_at: Some(finished_at),
        total_length: Some(completed_length),
        completed_length: Some(completed_length),
        error: None,
    })
}

fn runtime_snapshot(
    config: &Aria2Config<'_>,
    paths: &Aria2Paths,
    status: Aria2StatusRecord,
) -> Result<RuntimeSnapshot, String> {
    let queue_size = if paths.tasks_dir.is_dir() {
        fs::read_dir(&paths.tasks_dir)
            .map_err(io_error)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().and_then(OsStr::to_str) == Some("json"))
            .count()
    } else {
        0
    };
    let status = Aria2StatusRecord {
        download_url: config.download_url.to_string(),
        bundled_archive_path: Some(paths.archive_path.to_string_lossy().to_string()),
        ..status
    };
    Ok(RuntimeSnapshot {
        downloads_dir: paths.downloads_dir.clone(),
        helper_dir: paths.helper_dir.clone(),
        status,
        queue_size,
    })
}

fn paths_for_snapshot(snapshot: &RuntimeSnapshot) -> Aria2Paths {
    let plugin_data_dir = snapshot
        .helper_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    runtime_paths(&plugin_data_dir)
}

fn runtime_paths(plugin_data_dir: &Path) -> Aria2Paths {
    let downloads_dir = plugin_data_dir.join("downloads");
    let helper_dir = plugin_data_dir.join("helpers").join("aria2");
    Aria2Paths {
        runtime_dir: downloads_dir.join("aria2-runtime"),
        archive_path: downloads_dir.join("aria2-runtime.zip"),
        tasks_dir: helper_dir.join("tasks"),
        session_path: helper_dir.join("session.txt"),
        status_path: helper_dir.join("status.json"),
        pid_path: helper_dir.join("pid.txt"),
        downloads_dir,
        helper_dir,
    }
}

fn resolve_aria2_executable(
    config: &Aria2Config<'_>,
    paths: &Aria2Paths,
) -> Result<(PathBuf, String), String> {
    if let Some(system) = detect_system_aria2() {
        return Ok((system, "system".to_string()));
    }
    if let Some(bundled) = detect_bundled_aria2(paths) {
        return Ok((bundled, "bundled".to_string()));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = config;
        return Err("未探测到系统 aria2，当前仅支持在 Windows 上自动下载自带 aria2。".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        download_archive(config.download_url, &paths.archive_path)?;
        extract_aria2_archive(&paths.archive_path, &paths.runtime_dir)?;
        detect_bundled_aria2(paths)
            .map(|path| (path, "bundled".to_string()))
            .ok_or_else(|| "aria2 运行时下载完成，但未找到 aria2c.exe".to_string())
    }
}

fn detect_system_aria2() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let output = Command::new("where").arg("aria2c").output().ok()?;
    #[cfg(not(target_os = "windows"))]
    let output = Command::new("which").arg("aria2c").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .filter(|path| path.is_file())
}

fn detect_bundled_aria2(paths: &Aria2Paths) -> Option<PathBuf> {
    let binary_name = if cfg!(target_os = "windows") { "aria2c.exe" } else { "aria2c" };
    let candidate = paths.runtime_dir.join(binary_name);
    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

fn download_archive(url: &str, archive_path: &Path) -> Result<(), String> {
    if archive_path.is_file() {
        return Ok(());
    }
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(http_error)?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("failed to download aria2 runtime {url}: {error}"))?;
    let temp_path = archive_path.with_extension("download");
    let mut file = fs::File::create(&temp_path).map_err(io_error)?;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = response.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(io_error)?;
    }
    file.flush().map_err(io_error)?;
    fs::rename(temp_path, archive_path).map_err(io_error)
}

fn extract_aria2_archive(archive_path: &Path, runtime_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(runtime_dir).map_err(io_error)?;
    let file = fs::File::open(archive_path).map_err(io_error)?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    let expected_name = if cfg!(target_os = "windows") { "aria2c.exe" } else { "aria2c" };
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let entry_name = entry.name().replace('\\', "/");
        if !entry_name.ends_with(expected_name) {
            continue;
        }
        let target_path = runtime_dir.join(expected_name);
        let mut output = fs::File::create(&target_path).map_err(io_error)?;
        std::io::copy(&mut entry, &mut output).map_err(io_error)?;
        output.flush().map_err(io_error)?;
        return Ok(());
    }
    Err(format!(
        "aria2 archive does not contain {}",
        expected_name
    ))
}

fn spawn_aria2_process(
    executable_path: &Path,
    paths: &Aria2Paths,
    rpc_url: &str,
    secret: &str,
    downloads_dir: &Path,
) -> Result<std::process::Child, String> {
    let port = rpc_url
        .rsplit(':')
        .next()
        .and_then(|value| value.strip_suffix("/jsonrpc"))
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(RPC_PORT);
    let mut command = Command::new(executable_path);
    with_no_window(
        command
            .arg("--enable-rpc=true")
            .arg("--rpc-listen-all=false")
            .arg(format!("--rpc-listen-port={port}"))
            .arg(format!("--rpc-secret={secret}"))
            .arg(format!("--dir={}", downloads_dir.display()))
            .arg(format!("--input-file={}", paths.session_path.display()))
            .arg(format!("--save-session={}", paths.session_path.display()))
            .arg("--save-session-interval=5")
            .arg("--continue=true")
            .arg("--allow-overwrite=true")
            .arg("--auto-file-renaming=false")
            .arg("--summary-interval=0")
            .arg("--daemon=false")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .spawn()
    .map_err(|error| format!("failed to start aria2 runtime: {error}"))
}

fn status_record_healthy(status: &Aria2StatusRecord) -> Result<bool, String> {
    let Some(pid) = status.pid else {
        return Ok(false);
    };
    if !process_is_running(pid) {
        return Ok(false);
    }
    Ok(rpc_get_version(status).is_ok())
}

fn rpc_get_version(status: &Aria2StatusRecord) -> Result<String, String> {
    let payload = rpc_request(status, "aria2.getVersion", Vec::new())?;
    payload
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "aria2.getVersion did not return version".to_string())
}

fn rpc_request(
    status: &Aria2StatusRecord,
    method: &str,
    params: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let rpc_url = status
        .rpc_url
        .as_deref()
        .ok_or_else(|| "aria2 rpc url is missing".to_string())?;
    let secret = status
        .secret
        .as_deref()
        .ok_or_else(|| "aria2 rpc secret is missing".to_string())?;
    let mut final_params = vec![serde_json::json!(format!("token:{secret}"))];
    final_params.extend(params);
    let client = Client::builder()
        .timeout(Duration::from_secs(DEFAULT_RPC_TIMEOUT_SECS))
        .build()
        .map_err(http_error)?;
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "momobako",
            "method": method,
            "params": final_params,
        }))
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(http_error)?;
    let body = response
        .json::<serde_json::Value>()
        .map_err(http_error)?;
    if let Some(error) = body.get("error") {
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown aria2 rpc error");
        return Err(format!("aria2 rpc {method} failed: {message}"));
    }
    body.get("result")
        .cloned()
        .ok_or_else(|| format!("aria2 rpc {method} did not return result"))
}

fn update_task_from_status_payload(
    record: &mut DownloadTaskRecord,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "aria2 tellStatus response is missing status".to_string())?;
    record.status = match status {
        "complete" => "completed".to_string(),
        "error" => "failed".to_string(),
        other => other.to_string(),
    };
    record.total_length = payload
        .get("totalLength")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<i64>().ok());
    record.completed_length = payload
        .get("completedLength")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<i64>().ok());
    record.error = payload
        .get("errorMessage")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    if matches!(record.status.as_str(), "completed" | "failed" | "removed") && record.finished_at.is_none() {
        record.finished_at = Some(now_rfc3339()?);
    }
    Ok(())
}

fn save_status(path: &Path, status: &Aria2StatusRecord) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_string_pretty(status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn load_status(path: &Path) -> Result<Option<Aria2StatusRecord>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str::<Aria2StatusRecord>(&raw)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn save_task(tasks_dir: &Path, record: &DownloadTaskRecord) -> Result<(), String> {
    fs::create_dir_all(tasks_dir).map_err(io_error)?;
    fs::write(
        task_path(tasks_dir, &record.task_id),
        serde_json::to_string_pretty(record).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

fn load_task(tasks_dir: &Path, task_id: &str) -> Result<DownloadTaskRecord, String> {
    let path = task_path(tasks_dir, task_id);
    let raw = fs::read_to_string(&path).map_err(io_error)?;
    serde_json::from_str::<DownloadTaskRecord>(&raw).map_err(|error| error.to_string())
}

fn task_path(tasks_dir: &Path, task_id: &str) -> PathBuf {
    tasks_dir.join(format!("{task_id}.json"))
}

fn cleanup_stale_state(paths: &Aria2Paths) -> Result<(), String> {
    if let Some(pid) = read_pid(&paths.pid_path)? {
        if process_is_running(pid) {
            return Ok(());
        }
    }
    let _ = fs::remove_file(&paths.pid_path);
    let _ = fs::remove_file(&paths.status_path);
    Ok(())
}

fn read_pid(path: &Path) -> Result<Option<u32>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    Ok(raw.trim().parse::<u32>().ok())
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        let Ok(output) = output else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&format!(",\"{pid}\"")) || stdout.contains(&pid.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

fn status_secret(status_path: &Path) -> Result<Option<String>, String> {
    Ok(load_status(status_path)?.and_then(|status| status.secret))
}

fn generate_secret() -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!(
        "{}:{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    format!("{:x}", hasher.finalize())
}

fn now_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)
}

#[cfg(target_os = "windows")]
fn with_no_window(command: &mut Command) -> &mut Command {
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(target_os = "windows"))]
fn with_no_window(command: &mut Command) -> &mut Command {
    command
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn http_error(error: impl ToString) -> String {
    error.to_string()
}

fn time_error(error: impl ToString) -> String {
    error.to_string()
}

#[allow(dead_code)]
fn command_output_message(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    if !stdout.is_empty() {
        return stdout;
    }
    format!("process exited with status {}", output.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "momobako-aria2-runtime-{name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("test root should be created");
            Self { root }
        }

        fn path(&self, child: &str) -> PathBuf {
            self.root.join(child)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn update_task_from_status_payload_maps_complete_to_completed() {
        let mut record = DownloadTaskRecord {
            task_id: "gid-1".to_string(),
            gid: "gid-1".to_string(),
            url: "http://localhost/test.mp3".to_string(),
            destination_path: "C:/Temp/test.mp3".to_string(),
            metadata: None,
            status: "queued".to_string(),
            created_at: "2026-07-01T10:00:00Z".to_string(),
            finished_at: None,
            total_length: None,
            completed_length: None,
            error: None,
        };
        update_task_from_status_payload(
            &mut record,
            &serde_json::json!({
                "status": "complete",
                "totalLength": "123",
                "completedLength": "123"
            }),
        )
        .expect("status payload should update task");
        assert_eq!(record.status, "completed");
        assert_eq!(record.total_length, Some(123));
        assert_eq!(record.completed_length, Some(123));
        assert!(record.finished_at.is_some());
    }

    #[test]
    fn update_task_from_status_payload_maps_error_to_failed() {
        let mut record = DownloadTaskRecord {
            task_id: "gid-2".to_string(),
            gid: "gid-2".to_string(),
            url: "http://localhost/test.zip".to_string(),
            destination_path: "C:/Temp/test.zip".to_string(),
            metadata: None,
            status: "active".to_string(),
            created_at: "2026-07-01T10:00:00Z".to_string(),
            finished_at: None,
            total_length: None,
            completed_length: None,
            error: None,
        };
        update_task_from_status_payload(
            &mut record,
            &serde_json::json!({
                "status": "error",
                "totalLength": "123",
                "completedLength": "12",
                "errorMessage": "network failed"
            }),
        )
        .expect("status payload should update task");
        assert_eq!(record.status, "failed");
        assert_eq!(record.total_length, Some(123));
        assert_eq!(record.completed_length, Some(12));
        assert_eq!(record.error.as_deref(), Some("network failed"));
        assert!(record.finished_at.is_some());
    }

    #[test]
    fn runtime_status_returns_idle_snapshot_and_counts_persisted_tasks() {
        let workspace = TestWorkspace::new("idle-snapshot");
        let plugin_data_dir = workspace.path("plugin-data");
        let paths = runtime_paths(&plugin_data_dir);
        fs::create_dir_all(&paths.tasks_dir).expect("tasks dir should be created");
        fs::write(
            task_path(&paths.tasks_dir, "task-1"),
            r#"{"taskId":"task-1","gid":"gid-1","url":"http://127.0.0.1/file-1.zip","destinationPath":"C:/Temp/file-1.zip","status":"queued","createdAt":"2026-07-01T10:00:00Z"}"#,
        )
        .expect("first task should be written");
        fs::write(
            task_path(&paths.tasks_dir, "task-2"),
            r#"{"taskId":"task-2","gid":"gid-2","url":"http://127.0.0.1/file-2.zip","destinationPath":"C:/Temp/file-2.zip","status":"active","createdAt":"2026-07-01T10:01:00Z"}"#,
        )
        .expect("second task should be written");

        let snapshot = runtime_status(&Aria2Config {
            plugin_data_dir: &plugin_data_dir,
            download_url: "https://example.test/aria2.zip",
        })
        .expect("runtime status should resolve idle snapshot");

        assert_eq!(snapshot.queue_size, 2);
        assert_eq!(snapshot.downloads_dir, plugin_data_dir.join("downloads"));
        assert_eq!(snapshot.helper_dir, plugin_data_dir.join("helpers").join("aria2"));
        assert!(!snapshot.status.running);
        assert_eq!(snapshot.status.download_url, "https://example.test/aria2.zip");
        assert_eq!(
            snapshot.status.bundled_archive_path.as_deref(),
            Some(
                plugin_data_dir
                    .join("downloads")
                    .join("aria2-runtime.zip")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn cleanup_stale_state_removes_pid_and_status_for_dead_process() {
        let workspace = TestWorkspace::new("cleanup-stale");
        let plugin_data_dir = workspace.path("plugin-data");
        let paths = runtime_paths(&plugin_data_dir);
        fs::create_dir_all(&paths.helper_dir).expect("helper dir should be created");
        fs::write(&paths.pid_path, "999999").expect("pid file should be written");
        fs::write(
            &paths.status_path,
            r#"{"running":true,"pid":999999,"rpcUrl":"http://127.0.0.1:16831/jsonrpc","secret":"secret","downloadUrl":"https://example.test/aria2.zip"}"#,
        )
        .expect("status file should be written");

        cleanup_stale_state(&paths).expect("cleanup should remove stale process state");

        assert!(!paths.pid_path.exists());
        assert!(!paths.status_path.exists());
    }
}
