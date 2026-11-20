//! Microsoft Office 与 LibreOffice 的探测、内置 LibreOffice 的准备与下载。

use super::*;

pub(super) fn select_microsoft_office(family: OfficeFamily) -> Result<SelectedConverter, String> {
    let status = detect_microsoft_office_for_family(family);
    if !status.available {
        return Err(status
            .reason
            .unwrap_or_else(|| "Microsoft Office is unavailable".to_string()));
    }
    Ok(SelectedConverter {
        kind: ConverterKind::MicrosoftOffice,
        family,
        executable_path: PathBuf::from(
            status
                .path
                .ok_or_else(|| "Microsoft Office executable path is missing".to_string())?,
        ),
        version: status.version,
    })
}

pub(super) fn select_libreoffice(
    runtime: &RuntimeContext,
    family: OfficeFamily,
) -> Result<SelectedConverter, String> {
    let status = detect_system_libreoffice();
    if status.available {
        return Ok(SelectedConverter {
            kind: ConverterKind::LibreOfficeSystem,
            family,
            executable_path: PathBuf::from(
                status
                    .path
                    .ok_or_else(|| "LibreOffice executable path is missing".to_string())?,
            ),
            version: status.version,
        });
    }
    if let Some(path) = ensure_bundled_libreoffice(runtime)? {
        return Ok(SelectedConverter {
            kind: ConverterKind::LibreOfficeBundled,
            family,
            executable_path: path.clone(),
            version: command_version(&path)
                .or_else(|| Some(DEFAULT_LIBREOFFICE_VERSION.to_string())),
        });
    }
    Err(status
        .reason
        .unwrap_or_else(|| "LibreOffice is unavailable".to_string()))
}

pub(super) fn detect_microsoft_office_for_family(family: OfficeFamily) -> ConverterStatus {
    #[cfg(test)]
    if let Some(status) = test_support::test_detect_microsoft_office_for_family(family) {
        return status;
    }
    #[cfg(target_os = "windows")]
    {
        let executable_names = match family {
            OfficeFamily::Word => ["WINWORD.EXE"].as_slice(),
            OfficeFamily::Spreadsheet => ["EXCEL.EXE"].as_slice(),
            OfficeFamily::Presentation => ["POWERPNT.EXE"].as_slice(),
        };
        let mut candidates = Vec::new();
        for root in [
            r"C:\Program Files\Microsoft Office\root\Office16",
            r"C:\Program Files\Microsoft Office\Office16",
            r"C:\Program Files (x86)\Microsoft Office\root\Office16",
            r"C:\Program Files (x86)\Microsoft Office\Office16",
        ] {
            for executable_name in executable_names {
                candidates.push(PathBuf::from(root).join(executable_name));
            }
        }
        for candidate in candidates {
            if candidate.is_file() {
                return ConverterStatus {
                    available: true,
                    path: Some(candidate.to_string_lossy().to_string()),
                    version: candidate
                        .parent()
                        .and_then(|parent| parent.file_name())
                        .map(|value| value.to_string_lossy().to_string())
                        .or_else(|| Some("Office16".to_string())),
                    reason: None,
                };
            }
        }
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未探测到适用于当前文档类型的 Microsoft Office 安装。".to_string()),
    }
}

pub(super) fn detect_system_libreoffice() -> ConverterStatus {
    #[cfg(test)]
    if let Some(status) = test_support::test_detect_system_libreoffice() {
        return status;
    }
    let candidates = if cfg!(target_os = "windows") {
        vec![
            PathBuf::from(r"C:\Program Files\LibreOffice\program\soffice.exe"),
            PathBuf::from(r"C:\Program Files (x86)\LibreOffice\program\soffice.exe"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin/libreoffice"),
            PathBuf::from("/usr/bin/soffice"),
        ]
    };
    for candidate in candidates {
        if candidate.is_file() {
            return ConverterStatus {
                available: true,
                path: Some(candidate.to_string_lossy().to_string()),
                version: command_version(&candidate),
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

pub(super) fn detect_bundled_libreoffice(runtime: &RuntimeContext) -> ConverterStatus {
    #[cfg(test)]
    if let Some(status) = test_support::test_detect_bundled_libreoffice() {
        return status;
    }
    let runtime_dir = bundled_libreoffice_runtime_dir(runtime);
    let executable = bundled_libreoffice_executable_path(runtime);
    let installer = bundled_libreoffice_installer_path(runtime);
    if executable.is_file() {
        return ConverterStatus {
            available: true,
            path: Some(executable.to_string_lossy().to_string()),
            version: command_version(&executable)
                .or_else(|| Some(DEFAULT_LIBREOFFICE_VERSION.to_string())),
            reason: None,
        };
    }
    if installer.is_file() {
        return ConverterStatus {
            available: false,
            path: Some(installer.to_string_lossy().to_string()),
            version: Some(DEFAULT_LIBREOFFICE_VERSION.to_string()),
            reason: Some(format!(
                "已下载 LibreOffice 安装包，等待解压到 {}。",
                runtime_dir.display()
            )),
        };
    }
    ConverterStatus {
        available: false,
        path: None,
        version: None,
        reason: Some("未发现已下载的 LibreOffice 运行时。".to_string()),
    }
}

pub(super) fn ensure_bundled_libreoffice(
    runtime: &RuntimeContext,
) -> Result<Option<PathBuf>, String> {
    #[cfg(test)]
    if let Some(path) = test_support::test_ensure_bundled_libreoffice_path() {
        return Ok(path.map(PathBuf::from));
    }
    let executable = bundled_libreoffice_executable_path(runtime);
    if executable.is_file() {
        return Ok(Some(executable));
    }
    if !runtime.config.auto_download_libreoffice {
        return Ok(None);
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("当前仅支持在 Windows 上自动下载自带 LibreOffice。".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let installer_path = bundled_libreoffice_installer_path(runtime);
        download_runtime(runtime, DEFAULT_LIBREOFFICE_DOWNLOAD_URL, &installer_path)?;
        extract_libreoffice_installer(runtime, &installer_path)?;
        if executable.is_file() {
            return Ok(Some(executable));
        }
        Err(format!(
            "LibreOffice 安装包已处理，但未找到可执行文件：{}",
            executable.display()
        ))
    }
}

#[cfg(target_os = "windows")]
pub(super) fn extract_libreoffice_installer(
    runtime: &RuntimeContext,
    installer_path: &Path,
) -> Result<(), String> {
    let runtime_dir = bundled_libreoffice_runtime_dir(runtime);
    fs::create_dir_all(&runtime_dir).map_err(io_error)?;
    let output = run_command(
        with_no_window(
            Command::new("msiexec.exe")
                .arg("/a")
                .arg(installer_path)
                .arg("/qn")
                .arg(format!("TARGETDIR={}", runtime_dir.display())),
        ),
        "extract bundled LibreOffice runtime",
    )?;
    if !output.status.success() {
        return Err(format!(
            "failed to extract bundled LibreOffice runtime: {}",
            command_output_message(&output)
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn extract_libreoffice_installer(
    _runtime: &RuntimeContext,
    _installer_path: &Path,
) -> Result<(), String> {
    Err("当前仅支持在 Windows 上解压自带 LibreOffice 运行时。".to_string())
}

pub(super) fn download_runtime(
    runtime: &RuntimeContext,
    url: &str,
    target_path: &Path,
) -> Result<(), String> {
    if target_path.is_file() {
        return Ok(());
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.to_string())?;
    let mut target = fs::File::create(target_path).map_err(io_error)?;
    response
        .copy_to(&mut target)
        .map_err(|error| error.to_string())?;
    write_host_log_silently(
        &PluginRuntimeContext {
            plugin_id: OFFICE_CONVERT_PLUGIN_ID.to_string(),
            plugin_data_dir: runtime.plugin_data_dir.to_string_lossy().to_string(),
            service_root_dir: runtime.service_root_dir.to_string_lossy().to_string(),
            plugin_runtime_dir: runtime.plugin_runtime_dir.to_string_lossy().to_string(),
            plugin_config: Default::default(),
        },
        "info",
        "runtimeDownloaded",
        "LibreOffice 运行时下载完成。",
        serde_json::json!({ "url": url, "targetPath": target_path }),
    );
    Ok(())
}
