use std::{
    ffi::{c_char, CString},
    fs,
    os::raw::c_char as raw_c_char,
    path::{Path, PathBuf},
    process::Command,
};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, response_error, response_ok, PluginRuntimeContext,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MANIFEST: &str = include_str!("../manifest.json");
const DEFAULT_LIBREOFFICE_DOWNLOAD_URL: &str =
    "https://download.documentfoundation.org/libreoffice/stable/25.8.3/win/x86_64/LibreOffice_25.8.3_Win_x86-64.msi";

#[derive(Debug)]
struct RuntimeContext {
    plugin_data_dir: PathBuf,
    config: PluginConfig,
}

#[derive(Debug)]
struct PluginConfig {
    converter_mode: ConverterMode,
    auto_download_libreoffice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConverterMode {
    Auto,
    MicrosoftOffice,
    LibreOffice,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsurePreviewPdfPayload {
    repo_id: String,
    entry_path: String,
    extension: String,
    source_path: Option<String>,
    source_modified_at: Option<String>,
    source_size_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClearPreviewCachePayload {
    repo_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConverterStatus {
    available: bool,
    path: Option<String>,
    version: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeStatusResponse {
    converter_mode: String,
    microsoft_office: ConverterStatus,
    libreoffice_system: ConverterStatus,
    libreoffice_bundle: ConverterStatus,
    daemon: serde_json::Value,
    auto_download_libre_office: bool,
    bundled_download_url: String,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut raw_c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    match handle_call(input) {
        Ok(value) => response_ok(value),
        Err(error) => response_error(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(input: *const c_char) -> Result<serde_json::Value, String> {
    let request = read_request(input)?;
    let runtime = runtime_context(request.runtime)?;

    match request.method.as_str() {
        "officeConvert.ensurePreviewPdf" => {
            let payload: EnsurePreviewPdfPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            ensure_preview_pdf(&runtime, payload)
        }
        "officeConvert.getRuntimeStatus" => serde_json::to_value(get_runtime_status(&runtime)?)
            .map_err(|error| error.to_string()),
        "officeConvert.clearPreviewCache" => {
            let payload: ClearPreviewCachePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            clear_preview_cache(&runtime, payload)
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn runtime_context(runtime: PluginRuntimeContext) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(runtime.plugin_data_dir);
    fs::create_dir_all(plugin_data_dir.join("helpers").join("libreoffice")).map_err(io_error)?;
    fs::create_dir_all(plugin_data_dir.join("downloads")).map_err(io_error)?;
    let config = PluginConfig {
        converter_mode: ConverterMode::from_config(
            runtime
                .plugin_config
                .get("converterMode")
                .and_then(serde_json::Value::as_str),
        ),
        auto_download_libreoffice: runtime
            .plugin_config
            .get("autoDownloadLibreOffice")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    };
    Ok(RuntimeContext {
        plugin_data_dir,
        config,
    })
}

impl ConverterMode {
    fn from_config(value: Option<&str>) -> Self {
        match value.map(str::trim).unwrap_or_default() {
            "microsoft-office" => Self::MicrosoftOffice,
            "libreoffice" => Self::LibreOffice,
            _ => Self::Auto,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::MicrosoftOffice => "microsoft-office",
            Self::LibreOffice => "libreoffice",
        }
    }
}

fn ensure_preview_pdf(
    runtime: &RuntimeContext,
    payload: EnsurePreviewPdfPayload,
) -> Result<serde_json::Value, String> {
    let source_path = payload
        .source_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "missing sourcePath".to_string())?;
    if !source_path.is_file() {
        return Err(format!(
            "source file is not available: {}",
            source_path.to_string_lossy()
        ));
    }
    let extension = payload.extension.trim().to_ascii_lowercase();
    let source_metadata = fs::metadata(&source_path).map_err(io_error)?;
    let source_size_bytes = payload
        .source_size_bytes
        .filter(|value| *value >= 0)
        .unwrap_or(source_metadata.len() as i64);
    let source_modified_at = payload
        .source_modified_at
        .clone()
        .or_else(|| {
            source_metadata
                .modified()
                .ok()
                .and_then(|value| system_time_to_rfc3339(value).ok())
        })
        .unwrap_or_default();
    let converter = select_converter(runtime)?;
    let cache_key = preview_cache_key(
        &payload.repo_id,
        &payload.entry_path,
        &source_path,
        source_size_bytes,
        &source_modified_at,
        &extension,
        &converter,
    );
    let cache_dir = source_path
        .ancestors()
        .nth(1)
        .map(repository_cache_dir)
        .ok_or_else(|| "unable to infer repository cache directory".to_string())?;
    fs::create_dir_all(&cache_dir).map_err(io_error)?;
    let pdf_path = cache_dir.join(format!("{cache_key}.pdf"));
    let mut cached = pdf_path.is_file();
    if !cached {
        write_preview_pdf_stub(&pdf_path, &source_path, &payload, &converter)?;
        cached = false;
    }
    let metadata = fs::metadata(&pdf_path).map_err(io_error)?;
    let modified_at = metadata
        .modified()
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?;
    Ok(serde_json::json!({
        "pdfPath": pdf_path.to_string_lossy().to_string(),
        "cached": pdf_path.is_file() && cached,
        "converter": converter,
        "cacheKey": cache_key,
        "mediaType": "application/pdf",
        "sizeBytes": metadata.len() as i64,
        "modifiedAt": modified_at
    }))
}

fn get_runtime_status(runtime: &RuntimeContext) -> Result<RuntimeStatusResponse, String> {
    let microsoft_office = detect_microsoft_office();
    let libreoffice_system = detect_system_libreoffice();
    let libreoffice_bundle = detect_bundled_libreoffice(runtime);
    let daemon_status = read_helper_status(
        &runtime
            .plugin_data_dir
            .join("helpers")
            .join("libreoffice")
            .join("status.json"),
    );
    Ok(RuntimeStatusResponse {
        converter_mode: runtime.config.converter_mode.as_str().to_string(),
        microsoft_office,
        libreoffice_system,
        libreoffice_bundle,
        daemon: daemon_status,
        auto_download_libre_office: runtime.config.auto_download_libreoffice,
        bundled_download_url: DEFAULT_LIBREOFFICE_DOWNLOAD_URL.to_string(),
    })
}

fn clear_preview_cache(
    _runtime: &RuntimeContext,
    payload: ClearPreviewCachePayload,
) -> Result<serde_json::Value, String> {
    let repo_root = PathBuf::from(&payload.repo_id);
    let cache_dir = repository_cache_dir(&repo_root);
    let mut removed = 0;
    if cache_dir.is_dir() {
        for entry in fs::read_dir(&cache_dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if path.is_file() {
                fs::remove_file(path).map_err(io_error)?;
                removed += 1;
            }
        }
    }
    Ok(serde_json::json!({
        "repoId": payload.repo_id,
        "removed": removed
    }))
}

fn select_converter(runtime: &RuntimeContext) -> Result<String, String> {
    match runtime.config.converter_mode {
        ConverterMode::MicrosoftOffice => {
            let status = detect_microsoft_office();
            if status.available {
                Ok("microsoft-office".to_string())
            } else {
                Err(status
                    .reason
                    .unwrap_or_else(|| "Microsoft Office is unavailable".to_string()))
            }
        }
        ConverterMode::LibreOffice => {
            let status = detect_system_libreoffice();
            if status.available {
                Ok("libreoffice".to_string())
            } else if runtime.config.auto_download_libreoffice {
                Ok("libreoffice-bundled".to_string())
            } else {
                Err(status
                    .reason
                    .unwrap_or_else(|| "LibreOffice is unavailable".to_string()))
            }
        }
        ConverterMode::Auto => {
            let office = detect_microsoft_office();
            if office.available {
                return Ok("microsoft-office".to_string());
            }
            let libreoffice = detect_system_libreoffice();
            if libreoffice.available {
                return Ok("libreoffice".to_string());
            }
            if runtime.config.auto_download_libreoffice {
                return Ok("libreoffice-bundled".to_string());
            }
            Err("No available Office converter found.".to_string())
        }
    }
}

fn preview_cache_key(
    repo_id: &str,
    entry_path: &str,
    source_path: &Path,
    source_size_bytes: i64,
    source_modified_at: &str,
    extension: &str,
    converter: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(entry_path.as_bytes());
    hasher.update(b"\n");
    hasher.update(source_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(source_size_bytes.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(source_modified_at.as_bytes());
    hasher.update(b"\n");
    hasher.update(extension.as_bytes());
    hasher.update(b"\n");
    hasher.update(converter.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn repository_cache_dir(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".momo")
        .join("cache")
        .join("office-preview")
}

fn write_preview_pdf_stub(
    pdf_path: &Path,
    source_path: &Path,
    payload: &EnsurePreviewPdfPayload,
    converter: &str,
) -> Result<(), String> {
    if let Some(parent) = pdf_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let content = format!(
        "%PDF-1.4\n% MomoBako Office Preview Stub\n1 0 obj<<>>endobj\n2 0 obj<< /Length 84 >>stream\nConverted placeholder for {}\nEntry: {}\nConverter: {}\nendstream\nendobj\ntrailer<<>>\n%%EOF\n",
        source_path.to_string_lossy(),
        payload.entry_path,
        converter
    );
    fs::write(pdf_path, content).map_err(io_error)
}

fn detect_microsoft_office() -> ConverterStatus {
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE",
            r"C:\Program Files\Microsoft Office\root\Office16\POWERPNT.EXE",
            r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE",
        ];
        for candidate in candidates {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return ConverterStatus {
                    available: true,
                    path: Some(path.to_string_lossy().to_string()),
                    version: Some("Office16".to_string()),
                    reason: None,
                };
            }
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到系统 Microsoft Office 安装。".to_string()),
    }
}

fn detect_system_libreoffice() -> ConverterStatus {
    let candidates = if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
        ]
    } else {
        vec![PathBuf::from("/usr/bin/libreoffice"), PathBuf::from("/usr/bin/soffice")]
    };
    for candidate in candidates {
        if candidate.is_file() {
            return ConverterStatus {
                available: true,
                path: Some(candidate.to_string_lossy().to_string()),
                version: None,
                reason: None,
            };
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到系统 LibreOffice 安装。".to_string()),
    }
}

fn detect_bundled_libreoffice(runtime: &RuntimeContext) -> ConverterStatus {
    let downloads_dir = runtime.plugin_data_dir.join("downloads");
    let candidates = [
        downloads_dir.join("LibreOffice").join("program").join(if cfg!(target_os = "windows") {
            "soffice.exe"
        } else {
            "soffice"
        }),
        downloads_dir.join("LibreOffice_25.8.3_Win_x86-64.msi"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return ConverterStatus {
                available: true,
                path: Some(candidate.to_string_lossy().to_string()),
                version: Some("25.8.3".to_string()),
                reason: None,
            };
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未发现已下载的 LibreOffice 运行时。".to_string()),
    }
}

fn read_helper_status(path: &Path) -> serde_json::Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({
            "running": false,
            "pid": null
        }))
}

fn system_time_to_rfc3339(value: std::time::SystemTime) -> Result<String, String> {
    let value: OffsetDateTime = value.into();
    value.format(&Rfc3339).map_err(time_error)
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn time_error(error: impl ToString) -> String {
    error.to_string()
}

#[allow(dead_code)]
fn command_version(path: &Path) -> Option<String> {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
