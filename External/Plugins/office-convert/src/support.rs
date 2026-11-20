//! 最小 docx 生成、过期状态清理、进程管理、路径与脚本生成以及命令辅助函数。

use super::*;

pub(super) fn write_minimal_docx(path: &Path) -> Result<(), String> {
    let file = fs::File::create(path).map_err(io_error)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    let entries = [
        ("[Content_Types].xml", CONTENT_TYPES_XML),
        ("_rels/.rels", PACKAGE_RELS_XML),
        ("word/document.xml", WORD_DOCUMENT_XML),
        ("word/_rels/document.xml.rels", WORD_DOCUMENT_RELS_XML),
    ];
    for (name, content) in entries {
        archive
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        std::io::Write::write_all(&mut archive, content.as_bytes()).map_err(io_error)?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) const CONTENT_TYPES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

pub(super) const PACKAGE_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

pub(super) const WORD_DOCUMENT_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;

pub(super) const WORD_DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r>
        <w:t>MomoBako Office Convert Self Check</w:t>
      </w:r>
    </w:p>
    <w:sectPr/>
  </w:body>
</w:document>"#;

pub(super) fn cleanup_stale_libreoffice_state(
    plugin_data_dir: &Path,
    host_runtime: &PluginRuntimeContext,
) -> Result<(), String> {
    let helper_dir = plugin_data_dir.join("helpers").join("libreoffice");
    let pid_path = helper_dir.join("pid.txt");
    let status_path = helper_dir.join("status.json");
    if let Some(pid) = read_pid(&pid_path)? {
        if process_is_running(pid) {
            return Ok(());
        }
    }
    remove_stale_file_with_log(
        host_runtime,
        &pid_path,
        "daemonStateCleanupFailed",
        "LibreOffice 过期 PID 文件清理失败。",
    );
    remove_stale_file_with_log(
        host_runtime,
        &status_path,
        "daemonStateCleanupFailed",
        "LibreOffice 过期状态文件清理失败。",
    );
    Ok(())
}

pub(super) fn remove_stale_file_with_log(
    host_runtime: &PluginRuntimeContext,
    path: &Path,
    action: &str,
    message: &str,
) {
    if !path.is_file() {
        return;
    }
    if let Err(error) = fs::remove_file(path) {
        write_host_log_silently(
            host_runtime,
            "warn",
            action,
            message,
            serde_json::json!({
                "path": path.to_string_lossy().to_string(),
                "error": error.to_string(),
            }),
        );
    }
}

pub(super) fn read_pid(path: &Path) -> Result<Option<u32>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    Ok(raw.trim().parse::<u32>().ok())
}

pub(super) fn process_is_running(pid: u32) -> bool {
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

pub(super) fn stop_process(pid: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|error| format!("failed to stop LibreOffice daemon: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        return Err(format!(
            "failed to stop LibreOffice daemon: {}",
            command_output_message(&output)
        ));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(|error| format!("failed to stop LibreOffice daemon: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "failed to stop LibreOffice daemon: {}",
            command_output_message(&output)
        ))
    }
}

pub(super) fn normalize_service_root_dir(
    value: String,
    plugin_data_dir: &Path,
) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        return Ok(PathBuf::from(trimmed));
    }
    plugin_data_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "unable to infer service root directory".to_string())
}

pub(super) fn office_family_from_extension(extension: &str) -> Option<OfficeFamily> {
    match extension {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "dotm" => Some(OfficeFamily::Word),
        "xls" | "xlsx" | "xlsm" | "xlsb" | "xlt" | "xltx" | "xltm" => {
            Some(OfficeFamily::Spreadsheet)
        }
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "ppsm" | "pot" | "potx" | "potm" => {
            Some(OfficeFamily::Presentation)
        }
        _ => None,
    }
}

pub(super) fn preview_cache_temp_key(source_path: &Path, pdf_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(pdf_path.to_string_lossy().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn remove_files_recursively(
    runtime: &RuntimeContext,
    root: &Path,
) -> Result<i64, String> {
    let mut removed = 0_i64;
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.is_dir() {
            removed += remove_files_recursively(runtime, &path)?;
            if let Err(error) = fs::remove_dir(&path) {
                write_runtime_warning(
                    runtime,
                    "previewCacheCleanupFailed",
                    "Office 预览缓存目录清理失败。",
                    serde_json::json!({
                        "path": path.to_string_lossy().to_string(),
                        "error": error.to_string(),
                    }),
                );
            }
            continue;
        }
        if path.is_file() {
            fs::remove_file(&path).map_err(io_error)?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub(super) fn bundled_libreoffice_installer_path(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("downloads").join(format!(
        "LibreOffice_{DEFAULT_LIBREOFFICE_VERSION}_Win_x86-64.msi"
    ))
}

pub(super) fn bundled_libreoffice_runtime_dir(runtime: &RuntimeContext) -> PathBuf {
    runtime
        .plugin_data_dir
        .join("downloads")
        .join("LibreOffice")
}

pub(super) fn bundled_libreoffice_executable_path(runtime: &RuntimeContext) -> PathBuf {
    bundled_libreoffice_runtime_dir(runtime)
        .join("program")
        .join(if cfg!(target_os = "windows") {
            "soffice.exe"
        } else {
            "soffice"
        })
}

pub(super) fn libreoffice_helper_dir(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("helpers").join("libreoffice")
}

pub(super) fn libreoffice_profile_dir(runtime: &RuntimeContext) -> PathBuf {
    libreoffice_helper_dir(runtime).join("profile")
}

pub(super) fn libreoffice_profile_uri(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(target_os = "windows")]
pub(super) fn libreoffice_helper_script() -> &'static str {
    r#"
param(
  [Parameter(Mandatory = $true)][string]$SofficePath,
  [Parameter(Mandatory = $true)][int]$Port,
  [Parameter(Mandatory = $true)][string]$ProfileUri
)

Add-Type -AssemblyName System.Net
Add-Type -AssemblyName System.Web

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()

function Write-JsonResponse {
  param(
    [Parameter(Mandatory = $true)]$Context,
    [Parameter(Mandatory = $true)][int]$StatusCode,
    [Parameter(Mandatory = $true)]$Body
  )
  $json = [System.Text.Json.JsonSerializer]::Serialize($Body)
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
  $Context.Response.StatusCode = $StatusCode
  $Context.Response.ContentType = "application/json; charset=utf-8"
  $Context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
  $Context.Response.Close()
}

while ($listener.IsListening) {
  try {
    $context = $listener.GetContext()
    $path = $context.Request.Url.AbsolutePath
    if ($path -eq "/health") {
      Write-JsonResponse -Context $context -StatusCode 200 -Body @{
        ok = $true
        runtime = "libreoffice-helper"
      }
      continue
    }
    if ($path -eq "/shutdown") {
      Write-JsonResponse -Context $context -StatusCode 200 -Body @{
        ok = $true
        stopped = $true
      }
      $listener.Stop()
      break
    }
    if ($path -eq "/convert") {
      $reader = New-Object System.IO.StreamReader($context.Request.InputStream, $context.Request.ContentEncoding)
      $body = $reader.ReadToEnd()
      $payload = [System.Text.Json.JsonSerializer]::Deserialize($body, [System.Collections.Generic.Dictionary[string, object]])
      $sourcePath = [string]$payload["sourcePath"]
      $outputDir = [string]$payload["outputDir"]
      $process = Start-Process -FilePath $SofficePath -ArgumentList @(
        "--headless",
        "--nologo",
        "--nofirststartwizard",
        "--convert-to", "pdf",
        "--outdir", $outputDir,
        $sourcePath,
        "-env:UserInstallation=$ProfileUri"
      ) -Wait -PassThru -WindowStyle Hidden
      if ($process.ExitCode -eq 0) {
        Write-JsonResponse -Context $context -StatusCode 200 -Body @{ ok = $true }
      } else {
        Write-JsonResponse -Context $context -StatusCode 500 -Body @{
          ok = $false
          error = "LibreOffice convert exited with code $($process.ExitCode)"
        }
      }
      continue
    }
    Write-JsonResponse -Context $context -StatusCode 404 -Body @{ ok = $false; error = "Not Found" }
  } catch {
    try {
      if ($context) {
        Write-JsonResponse -Context $context -StatusCode 500 -Body @{
          ok = $false
          error = $_.Exception.Message
        }
      }
    } catch {}
  }
}
"#
}

#[cfg(target_os = "windows")]
pub(super) fn microsoft_office_script(family: OfficeFamily) -> &'static str {
    match family {
        OfficeFamily::Word => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$word = $null
$doc = $null
try {
  $word = New-Object -ComObject Word.Application
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $doc = $word.Documents.Open($inputPath, $false, $true)
  $doc.ExportAsFixedFormat($outputPath, 17)
} finally {
  if ($doc -ne $null) { $doc.Close($false) | Out-Null }
  if ($word -ne $null) { $word.Quit() }
}
"#
        }
        OfficeFamily::Spreadsheet => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$excel = $null
$workbook = $null
try {
  $excel = New-Object -ComObject Excel.Application
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $workbook = $excel.Workbooks.Open($inputPath, 0, $true)
  $workbook.ExportAsFixedFormat(0, $outputPath)
} finally {
  if ($workbook -ne $null) { $workbook.Close($false) | Out-Null }
  if ($excel -ne $null) { $excel.Quit() }
}
"#
        }
        OfficeFamily::Presentation => {
            r#"
$inputPath = [System.IO.Path]::GetFullPath($args[0])
$outputPath = [System.IO.Path]::GetFullPath($args[1])
$powerpoint = $null
$presentation = $null
try {
  $powerpoint = New-Object -ComObject PowerPoint.Application
  $presentation = $powerpoint.Presentations.Open($inputPath, $false, $false, $false)
  $presentation.SaveAs($outputPath, 32)
} finally {
  if ($presentation -ne $null) { $presentation.Close() }
  if ($powerpoint -ne $null) { $powerpoint.Quit() }
}
"#
        }
    }
}

pub(super) fn run_command(command: &mut Command, description: &str) -> Result<Output, String> {
    command
        .output()
        .map_err(|error| format!("{description} failed: {error}"))
}

#[cfg(target_os = "windows")]
pub(super) fn with_no_window(command: &mut Command) -> &mut Command {
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(target_os = "windows"))]
pub(super) fn with_no_window(command: &mut Command) -> &mut Command {
    command
}

pub(super) fn command_output_message(output: &Output) -> String {
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

pub(super) fn system_time_to_rfc3339(value: std::time::SystemTime) -> Result<String, String> {
    let value: OffsetDateTime = value.into();
    value.format(&Rfc3339).map_err(time_error)
}

pub(super) fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

pub(super) fn time_error(error: impl ToString) -> String {
    error.to_string()
}

pub(super) fn command_version(path: &Path) -> Option<String> {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
