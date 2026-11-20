//! 辅助进程 HTTP 接口调用与 LibreOffice 状态、自检结果的写入。

use super::*;

#[derive(Debug, Default)]
pub(super) struct HelperRuntimeStatus {
    pub(super) healthy: bool,
    pub(super) soffice_ready: bool,
    pub(super) soffice_pid: Option<u32>,
    pub(super) uno_available: bool,
    pub(super) python_valid: bool,
    pub(super) python_path: Option<String>,
}

pub(super) fn helper_health(port: u16) -> bool {
    helper_runtime_status(port)
        .map(|status| status.healthy)
        .unwrap_or(false)
}

pub(super) fn helper_runtime_status(port: u16) -> Option<HelperRuntimeStatus> {
    let Ok(client) = helper_http_client() else {
        return None;
    };
    let response = client
        .get(format!("{}/health", helper_base_url(port)))
        .send()
        .ok()?;
    let healthy = response.status().is_success();
    let payload = response.json::<serde_json::Value>().ok();
    Some(HelperRuntimeStatus {
        healthy,
        soffice_ready: payload
            .as_ref()
            .and_then(|value| value.get("sofficeReady"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(healthy),
        soffice_pid: payload
            .as_ref()
            .and_then(|value| value.get("sofficePid"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        uno_available: payload
            .as_ref()
            .and_then(|value| value.get("unoAvailable"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        python_valid: payload
            .as_ref()
            .and_then(|value| value.get("pythonValid"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        python_path: payload
            .as_ref()
            .and_then(|value| value.get("pythonPath"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

pub(super) fn helper_shutdown(port: u16) -> Result<(), String> {
    let client = helper_http_client()?;
    client
        .post(format!("{}/shutdown", helper_base_url(port)))
        .send()
        .map_err(|error| format!("failed to request LibreOffice helper shutdown: {error}"))?;
    Ok(())
}

pub(super) fn helper_convert(
    _runtime: &RuntimeContext,
    _executable_path: &Path,
    port: u16,
    source_path: &Path,
    output_dir: &Path,
    pdf_path: &Path,
) -> Result<Option<String>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let profile_dir = libreoffice_profile_dir(_runtime);
        let output = run_command(
            with_no_window(
                Command::new(_executable_path)
                    .arg("--headless")
                    .arg("--nologo")
                    .arg("--nofirststartwizard")
                    .arg("--convert-to")
                    .arg("pdf")
                    .arg("--outdir")
                    .arg(output_dir)
                    .arg(source_path)
                    .arg(format!(
                        "-env:UserInstallation={}",
                        libreoffice_profile_uri(&profile_dir)
                    )),
            ),
            "convert office document with LibreOffice",
        )?;
        if output.status.success() {
            return Ok(None);
        }
        return Err(command_output_message(&output));
    }
    #[cfg(target_os = "windows")]
    {
        let client = helper_http_client()?;
        let response = client
            .post(format!("{}/convert", helper_base_url(port)))
            .json(&serde_json::json!({
                "sourcePath": source_path.to_string_lossy().to_string(),
                "outputDir": output_dir.to_string_lossy().to_string(),
                "pdfPath": pdf_path.to_string_lossy().to_string()
            }))
            .send()
            .map_err(|error| format!("failed to request LibreOffice helper convert: {error}"))?;
        if response.status().is_success() {
            let payload = response.json::<serde_json::Value>().ok();
            return Ok(payload
                .as_ref()
                .and_then(|value| value.get("conversionMode"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string));
        }
        Err(response
            .text()
            .unwrap_or_else(|_| "helper convert request failed".to_string()))
    }
}

pub(super) fn write_libreoffice_status(
    runtime: &RuntimeContext,
    pid: Option<u32>,
    executable_path: &Path,
    running: bool,
    error: Option<String>,
) -> Result<(), String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let mut payload = read_helper_status(&status_path);
    let updated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    if let Some(object) = payload.as_object_mut() {
        object.insert("running".to_string(), serde_json::Value::Bool(running));
        object.insert(
            "pid".to_string(),
            pid.map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "path".to_string(),
            serde_json::Value::String(executable_path.to_string_lossy().to_string()),
        );
        object.insert(
            "updatedAt".to_string(),
            serde_json::Value::String(updated_at),
        );
        object.insert(
            "error".to_string(),
            error
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
    }
    fs::write(
        status_path,
        serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

pub(super) fn write_libreoffice_conversion_status(
    runtime: &RuntimeContext,
    source_path: &Path,
    pdf_path: &Path,
    phase: &str,
    conversion_mode: Option<&str>,
    error: Option<String>,
) -> Result<(), String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let mut status = read_helper_status(&status_path);
    let updated_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(time_error)?;
    let last_convert = serde_json::json!({
        "phase": phase,
        "sourcePath": source_path.to_string_lossy().to_string(),
        "pdfPath": pdf_path.to_string_lossy().to_string(),
        "updatedAt": updated_at,
        "conversionMode": conversion_mode,
        "error": error,
    });
    let phase_error = last_convert
        .get("error")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::String("LibreOffice 转换失败。".to_string()));
    if let Some(object) = status.as_object_mut() {
        object.insert("lastConvert".to_string(), last_convert);
        if phase == "failed" {
            object.insert("error".to_string(), phase_error);
        } else if phase == "completed" {
            object.insert("error".to_string(), serde_json::Value::Null);
        }
    }
    fs::write(
        status_path,
        serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}

pub(super) fn latest_conversion_mode(runtime: &RuntimeContext) -> Option<String> {
    read_helper_status(&libreoffice_helper_dir(runtime).join("status.json"))
        .get("lastConvert")
        .and_then(|value| value.get("conversionMode"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

pub(super) fn write_runtime_self_check_status(
    runtime: &RuntimeContext,
    payload: serde_json::Value,
) -> Result<(), String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let mut status = read_helper_status(&status_path);
    if let Some(object) = status.as_object_mut() {
        object.insert("lastSelfCheck".to_string(), payload);
    }
    fs::write(
        status_path,
        serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)
}
