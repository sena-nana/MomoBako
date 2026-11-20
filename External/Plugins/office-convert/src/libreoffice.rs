//! 调用 Office 转换，LibreOffice 守护进程与辅助进程的启动、状态和关闭。

use super::*;

pub(super) fn convert_with_microsoft_office(
    converter: &SelectedConverter,
    source_path: &Path,
    pdf_path: &Path,
) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (converter, source_path, pdf_path);
        return Err("Microsoft Office 转换仅支持 Windows。".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let helper_dir = pdf_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let script_path = helper_dir.join("office-convert.ps1");
        fs::write(&script_path, microsoft_office_script(converter.family)).map_err(io_error)?;
        let output = run_command(
            with_no_window(
                Command::new("powershell.exe")
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-File")
                    .arg(&script_path)
                    .arg(source_path)
                    .arg(pdf_path),
            ),
            "convert office document with Microsoft Office",
        )?;
        if !output.status.success() || !pdf_path.is_file() {
            return Err(format!(
                "Microsoft Office 转换失败：{}",
                command_output_message(&output)
            ));
        }
        Ok(())
    }
}

pub(super) fn convert_with_libreoffice(
    runtime: &RuntimeContext,
    converter: &SelectedConverter,
    source_path: &Path,
    pdf_path: &Path,
    temp_dir: &Path,
) -> Result<(), String> {
    ensure_libreoffice_daemon(runtime, &converter.executable_path)?;
    write_libreoffice_conversion_status(runtime, source_path, pdf_path, "running", None, None)?;
    let output_dir = temp_dir.join(format!(
        "pdf-{}",
        preview_cache_temp_key(source_path, pdf_path)
    ));
    if output_dir.is_dir() {
        remove_dir_all_with_warning(
            runtime,
            &output_dir,
            "conversionTempCleanupFailed",
            "Office 转换临时目录清理失败。",
        );
    }
    fs::create_dir_all(&output_dir).map_err(io_error)?;
    let helper_port = helper_port(runtime)?;
    let convert_result = helper_convert(
        runtime,
        &converter.executable_path,
        helper_port,
        source_path,
        &output_dir,
        pdf_path,
    );
    let generated_path = output_dir.join(format!(
        "{}.pdf",
        source_path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("invalid source file name: {}", source_path.display()))?
    ));
    let conversion_mode = match convert_result {
        Ok(mode) => mode,
        Err(error) => {
            write_libreoffice_conversion_status(
                runtime,
                source_path,
                pdf_path,
                "failed",
                None,
                Some(error.clone()),
            )?;
            return Err(format!("LibreOffice 转换失败：{error}"));
        }
    };
    if !generated_path.is_file() {
        write_libreoffice_conversion_status(
            runtime,
            source_path,
            pdf_path,
            "failed",
            conversion_mode.as_deref(),
            Some(format!("转换结果缺失：{}", generated_path.display())),
        )?;
        return Err(format!(
            "LibreOffice 转换失败：未找到输出文件 {}",
            generated_path.display()
        ));
    }
    fs::copy(&generated_path, pdf_path).map_err(io_error)?;
    write_libreoffice_conversion_status(
        runtime,
        source_path,
        pdf_path,
        "completed",
        conversion_mode.as_deref(),
        None,
    )?;
    Ok(())
}

pub(super) fn ensure_libreoffice_daemon(
    runtime: &RuntimeContext,
    executable_path: &Path,
) -> Result<(), String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    fs::create_dir_all(&helper_dir).map_err(io_error)?;
    let pid_path = helper_dir.join("pid.txt");
    let port = helper_port(runtime)?;
    if let Some(pid) = read_pid(&pid_path)? {
        if process_is_running(pid) {
            if helper_health(port) {
                write_libreoffice_status(runtime, Some(pid), executable_path, true, None)?;
                return Ok(());
            }
            if let Err(error) = stop_process(pid) {
                write_runtime_warning(
                    runtime,
                    "daemonStopFailed",
                    "LibreOffice 异常守护进程停止失败。",
                    serde_json::json!({
                        "pid": pid,
                        "error": error,
                    }),
                );
            }
            cleanup_stale_libreoffice_state(
                &runtime.plugin_data_dir,
                &host_runtime_for_log(runtime),
            )?;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        start_legacy_libreoffice_daemon(runtime, executable_path)
    }
    #[cfg(target_os = "windows")]
    {
        start_windows_libreoffice_helper(runtime, executable_path, port)
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn start_legacy_libreoffice_daemon(
    runtime: &RuntimeContext,
    executable_path: &Path,
) -> Result<(), String> {
    let profile_dir = libreoffice_profile_dir(runtime);
    fs::create_dir_all(&profile_dir).map_err(io_error)?;
    let pid_path = libreoffice_helper_dir(runtime).join("pid.txt");
    let child = {
        let mut command = Command::new(executable_path);
        with_no_window(
            command
                .arg("--headless")
                .arg("--nologo")
                .arg("--nodefault")
                .arg("--nofirststartwizard")
                .arg("--norestore")
                .arg("--invisible")
                .arg("--accept=pipe,name=momobako-office-convert;urp;StarOffice.ComponentContext")
                .arg(format!(
                    "-env:UserInstallation={}",
                    libreoffice_profile_uri(&profile_dir)
                ))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map_err(|error| format!("failed to start LibreOffice daemon: {error}"))?
    };
    let pid = child.id();
    fs::write(&pid_path, pid.to_string()).map_err(io_error)?;
    thread::sleep(Duration::from_secs(2));
    let running = process_is_running(pid);
    write_libreoffice_status(runtime, Some(pid), executable_path, running, None)?;
    if running {
        Ok(())
    } else {
        Err("LibreOffice 守护进程启动失败。".to_string())
    }
}

#[cfg(target_os = "windows")]
pub(super) fn start_windows_libreoffice_helper(
    runtime: &RuntimeContext,
    executable_path: &Path,
    port: u16,
) -> Result<(), String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    let profile_dir = libreoffice_profile_dir(runtime);
    fs::create_dir_all(&profile_dir).map_err(io_error)?;
    let child = if let Some(helper_binary) = bundled_helper_path(runtime) {
        with_no_window(
            Command::new(helper_binary)
                .arg("--soffice-path")
                .arg(executable_path)
                .arg("--port")
                .arg(port.to_string())
                .arg("--profile-uri")
                .arg(libreoffice_profile_uri(&profile_dir))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map_err(|error| format!("failed to start bundled LibreOffice helper: {error}"))?
    } else {
        let script_path = helper_dir.join("office-convert-helper.ps1");
        fs::write(&script_path, libreoffice_helper_script()).map_err(io_error)?;
        with_no_window(
            Command::new("powershell.exe")
                .arg("-NoProfile")
                .arg("-NonInteractive")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&script_path)
                .arg("-SofficePath")
                .arg(executable_path)
                .arg("-Port")
                .arg(port.to_string())
                .arg("-ProfileUri")
                .arg(libreoffice_profile_uri(&profile_dir))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()
        .map_err(|error| format!("failed to start LibreOffice helper: {error}"))?
    };
    let pid = child.id();
    fs::write(helper_dir.join("pid.txt"), pid.to_string()).map_err(io_error)?;
    for _ in 0..LIBREOFFICE_HELPER_STARTUP_RETRIES {
        thread::sleep(Duration::from_millis(250));
        if process_is_running(pid) && helper_health(port) {
            write_libreoffice_status(runtime, Some(pid), executable_path, true, None)?;
            return Ok(());
        }
    }
    write_libreoffice_status(
        runtime,
        Some(pid),
        executable_path,
        false,
        Some("LibreOffice helper 启动后未通过健康检查。".to_string()),
    )?;
    Err("LibreOffice 守护进程启动失败。".to_string())
}

pub(super) fn read_helper_status(path: &Path) -> serde_json::Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "running": false,
                "pid": null
            })
        })
}

pub(super) fn current_libreoffice_daemon_status(
    runtime: &RuntimeContext,
) -> Result<serde_json::Value, String> {
    let status_path = libreoffice_helper_dir(runtime).join("status.json");
    let pid_path = libreoffice_helper_dir(runtime).join("pid.txt");
    let mut status = read_helper_status(&status_path);
    let pid = read_pid(&pid_path)?;
    let running = pid.map(process_is_running).unwrap_or(false);
    let port = helper_port(runtime).ok();
    let helper_runtime = port.and_then(helper_runtime_status);
    if let Some(object) = status.as_object_mut() {
        object.insert("running".to_string(), serde_json::Value::Bool(running));
        object.insert(
            "healthy".to_string(),
            serde_json::Value::Bool(
                helper_runtime
                    .as_ref()
                    .map(|value| value.healthy)
                    .unwrap_or(false),
            ),
        );
        object.insert(
            "helperType".to_string(),
            serde_json::Value::String(helper_type_label(runtime).to_string()),
        );
        object.insert(
            "port".to_string(),
            port.map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "baseUrl".to_string(),
            port.map(|value| serde_json::Value::String(helper_base_url(value)))
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "control".to_string(),
            serde_json::json!({
                "health": "http-local",
                "convert": "http-local",
                "shutdown": "plugin-call"
            }),
        );
        object.insert(
            "sofficeReady".to_string(),
            helper_runtime
                .as_ref()
                .map(|value| serde_json::Value::Bool(value.soffice_ready))
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "sofficePid".to_string(),
            helper_runtime
                .as_ref()
                .and_then(|value| value.soffice_pid)
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "unoAvailable".to_string(),
            helper_runtime
                .as_ref()
                .map(|value| serde_json::Value::Bool(value.uno_available))
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "pythonValid".to_string(),
            helper_runtime
                .as_ref()
                .map(|value| serde_json::Value::Bool(value.python_valid))
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "pythonPath".to_string(),
            helper_runtime
                .as_ref()
                .and_then(|value| value.python_path.as_ref())
                .map(|value| serde_json::Value::String(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        if !running {
            object.insert(
                "error".to_string(),
                serde_json::Value::String("LibreOffice 守护进程未运行。".to_string()),
            );
        }
    }
    if let Some(path) = status.get("path").and_then(serde_json::Value::as_str) {
        write_libreoffice_status(
            runtime,
            pid,
            Path::new(path),
            running,
            if running {
                None
            } else {
                Some("LibreOffice 守护进程未运行。".to_string())
            },
        )?;
        return Ok(read_helper_status(&status_path));
    }
    Ok(status)
}

pub(super) fn shutdown_libreoffice_daemon(
    runtime: &RuntimeContext,
) -> Result<serde_json::Value, String> {
    let helper_dir = libreoffice_helper_dir(runtime);
    let pid_path = helper_dir.join("pid.txt");
    let status_path = helper_dir.join("status.json");
    let pid = read_pid(&pid_path)?;
    let Some(pid) = pid else {
        remove_file_with_warning(
            runtime,
            &status_path,
            "daemonStateCleanupFailed",
            "LibreOffice 守护进程状态文件清理失败。",
        );
        return Ok(serde_json::json!({
            "stopped": false,
            "reason": "daemon-not-running"
        }));
    };
    let port = helper_port(runtime)?;
    if helper_health(port) {
        if let Err(error) = helper_shutdown(port) {
            write_runtime_warning(
                runtime,
                "daemonShutdownRequestFailed",
                "LibreOffice 守护进程关闭请求失败。",
                serde_json::json!({
                    "port": port,
                    "pid": pid,
                    "error": error,
                }),
            );
        }
    }
    stop_process(pid)?;
    remove_file_with_warning(
        runtime,
        &pid_path,
        "daemonStateCleanupFailed",
        "LibreOffice 守护进程 PID 文件清理失败。",
    );
    let mut status = read_helper_status(&status_path);
    if let Some(object) = status.as_object_mut() {
        object.insert("running".to_string(), serde_json::Value::Bool(false));
        object.insert("healthy".to_string(), serde_json::Value::Bool(false));
        object.insert(
            "helperType".to_string(),
            serde_json::Value::String(helper_type_label(runtime).to_string()),
        );
        object.insert("port".to_string(), serde_json::Value::from(port));
        object.insert(
            "baseUrl".to_string(),
            serde_json::Value::String(helper_base_url(port)),
        );
        object.insert("pid".to_string(), serde_json::Value::from(pid));
        object.insert(
            "path".to_string(),
            read_helper_status(&status_path)
                .get("path")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "updatedAt".to_string(),
            serde_json::Value::String(
                OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .map_err(time_error)?,
            ),
        );
        object.insert(
            "error".to_string(),
            serde_json::Value::String("LibreOffice 守护进程已关闭。".to_string()),
        );
        object.insert(
            "control".to_string(),
            serde_json::json!({
                "health": "http-local",
                "convert": "http-local",
                "shutdown": "plugin-call"
            }),
        );
    }
    fs::write(
        &status_path,
        serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    Ok(serde_json::json!({
        "stopped": true,
        "pid": pid
    }))
}

pub(super) fn helper_port(runtime: &RuntimeContext) -> Result<u16, String> {
    let port_path = libreoffice_helper_dir(runtime).join("port.txt");
    if let Ok(raw) = fs::read_to_string(&port_path) {
        if let Ok(value) = raw.trim().parse::<u16>() {
            if value > 0 {
                return Ok(value);
            }
        }
    }
    let port = reserve_local_port()?;
    fs::write(&port_path, port.to_string()).map_err(io_error)?;
    Ok(port)
}

pub(super) fn reserve_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("failed to reserve helper port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to read helper port: {error}"))?
        .port();
    drop(listener);
    Ok(port)
}

pub(super) fn helper_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub(super) fn helper_type_label(runtime: &RuntimeContext) -> &'static str {
    if cfg!(target_os = "windows") {
        if bundled_helper_path(runtime).is_some() {
            "native-http"
        } else {
            "powershell-http"
        }
    } else {
        "native-cli-fallback"
    }
}

pub(super) fn bundled_helper_path(runtime: &RuntimeContext) -> Option<PathBuf> {
    let runtime_dir = runtime.plugin_runtime_dir.as_path();
    if runtime_dir.as_os_str().is_empty() {
        return None;
    }
    let file_name = if cfg!(target_os = "windows") {
        "office-convert-helper.exe"
    } else {
        "office-convert-helper"
    };
    let candidate = runtime_dir.join(file_name);
    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

pub(super) fn helper_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(LIBREOFFICE_HELPER_HEALTH_TIMEOUT_SECS))
        .build()
        .map_err(|error| error.to_string())
}
