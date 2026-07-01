//! Office Convert 独立 helper。
//!
//! 提供本地 HTTP 控制接口，并在 Windows 上托管后台 Headless LibreOffice 进程：
//! - GET /health
//! - POST /convert
//! - POST /shutdown

use std::{
    env,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde::Deserialize;
use tiny_http::{Method, Response, Server, StatusCode};

#[derive(Debug)]
struct Args {
    soffice_path: PathBuf,
    port: u16,
    profile_uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertRequest {
    source_path: String,
    output_dir: String,
    pdf_path: String,
}

#[cfg(target_os = "windows")]
const HELPER_PIPE_NAME: &str = "momobako-office-convert";

struct HelperState {
    args: Args,
    soffice: Mutex<Option<Child>>,
}

impl HelperState {
    fn new(args: Args) -> Result<Self, String> {
        let state = Self {
            args,
            soffice: Mutex::new(None),
        };
        state.ensure_soffice_ready()?;
        Ok(state)
    }

    fn ensure_soffice_ready(&self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            let mut guard = self
                .soffice
                .lock()
                .map_err(|_| "helper soffice lock poisoned".to_string())?;
            let restart = match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => true,
                    Ok(None) => false,
                    Err(error) => {
                        return Err(format!("failed to query LibreOffice process state: {error}"));
                    }
                },
                None => true,
            };
            if restart {
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                *guard = Some(spawn_soffice_server(&self.args)?);
            }
            let pid = soffice_pid(&guard).ok_or_else(|| "LibreOffice process is missing".to_string())?;
            drop(guard);
            wait_for_soffice_ready()
                .map_err(|error| format!("LibreOffice helper failed to prepare soffice pid {pid}: {error}"))
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(())
        }
    }

    fn soffice_pid(&self) -> Option<u32> {
        self.soffice
            .lock()
            .ok()
            .and_then(|guard| soffice_pid(&guard))
    }

    fn shutdown_soffice(&self) {
        if let Ok(mut guard) = self.soffice.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn main() -> Result<(), String> {
    let args = parse_args()?;
    let state = Arc::new(HelperState::new(args)?);
    let server = Server::http(format!("127.0.0.1:{}", state.args.port))
        .map_err(|error| format!("failed to start helper server: {error}"))?;
    let shutdown = Arc::new(AtomicBool::new(false));
    while !shutdown.load(Ordering::SeqCst) {
        let Some(request) = server
            .recv_timeout(Duration::from_millis(250))
            .ok()
            .flatten()
        else {
            continue;
        };
        match (request.method(), request.url()) {
            (&Method::Get, "/health") => {
                let healthy = state.ensure_soffice_ready().is_ok();
                let soffice_pid = state.soffice_pid();
                let _ = request.respond(json_response(
                    if healthy {
                        StatusCode(200)
                    } else {
                        StatusCode(503)
                    },
                    serde_json::json!({
                        "ok": healthy,
                        "runtime": "office-convert-helper",
                        "sofficeReady": healthy,
                        "sofficePid": soffice_pid,
                    }),
                ));
            }
            (&Method::Post, "/shutdown") => {
                shutdown.store(true, Ordering::SeqCst);
                state.shutdown_soffice();
                let _ = request.respond(json_response(
                    StatusCode(200),
                    serde_json::json!({ "ok": true, "stopped": true }),
                ));
            }
            (&Method::Post, "/convert") => {
                let result = handle_convert(request, &state);
                if let Err(error) = result {
                    eprintln!("{error}");
                }
            }
            _ => {
                let _ = request.respond(json_response(
                    StatusCode(404),
                    serde_json::json!({ "ok": false, "error": "Not Found" }),
                ));
            }
        }
    }
    state.shutdown_soffice();
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    parse_args_from(env::args())
}

fn parse_args_from<I>(args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = String>,
{
    let mut soffice_path = None;
    let mut port = None;
    let mut profile_uri = None;
    let mut args = args.into_iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--soffice-path" => soffice_path = args.next().map(PathBuf::from),
            "--port" => port = args.next().and_then(|value| value.parse::<u16>().ok()),
            "--profile-uri" => profile_uri = args.next(),
            other => return Err(format!("unsupported arg: {other}")),
        }
    }
    Ok(Args {
        soffice_path: soffice_path.ok_or_else(|| "missing --soffice-path".to_string())?,
        port: port.ok_or_else(|| "missing --port".to_string())?,
        profile_uri: profile_uri.ok_or_else(|| "missing --profile-uri".to_string())?,
    })
}

fn handle_convert(mut request: tiny_http::Request, state: &Arc<HelperState>) -> Result<(), String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| format!("failed to read convert request: {error}"))?;
    let payload: ConvertRequest = serde_json::from_str(&body)
        .map_err(|error| format!("failed to decode convert request: {error}"))?;

    #[cfg(target_os = "windows")]
    {
        state.ensure_soffice_ready()?;
        let status = convert_with_managed_runtime(&state.args, &payload)
            .or_else(|uno_error| {
                convert_with_cli_fallback(&state.args, &payload).map_err(|fallback_error| {
                    format!(
                        "managed LibreOffice convert failed: {uno_error}; fallback convert failed: {fallback_error}"
                    )
                })
            });
        let status = match status {
            Ok(value) => value,
            Err(error) => {
                let _ = request.respond(json_response(
                    StatusCode(500),
                    serde_json::json!({
                        "ok": false,
                        "error": error,
                    }),
                ));
                return Ok(());
            }
        };
        if status.success() {
            let _ = request.respond(json_response(
                StatusCode(200),
                serde_json::json!({
                    "ok": true,
                    "pdfPath": payload.pdf_path,
                }),
            ));
            return Ok(());
        }
        let _ = request.respond(json_response(
            StatusCode(500),
            serde_json::json!({
                "ok": false,
                "error": format!("LibreOffice convert exited with status {}", status),
            }),
        ));
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = request.respond(json_response(
            StatusCode(501),
            serde_json::json!({
                "ok": false,
                "error": "office-convert-helper currently supports Windows only",
            }),
        ));
        let _ = payload;
        let _ = state;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn spawn_soffice_server(args: &Args) -> Result<Child, String> {
    let mut command = Command::new(&args.soffice_path);
    with_no_window(
        command
            .arg("--headless")
            .arg("--nologo")
            .arg("--nodefault")
            .arg("--nofirststartwizard")
            .arg("--norestore")
            .arg("--invisible")
            .arg(format!(
                "--accept=pipe,name={HELPER_PIPE_NAME};urp;StarOffice.ComponentContext"
            ))
            .arg(format!("-env:UserInstallation={}", args.profile_uri))
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .spawn()
    .map_err(|error| format!("failed to start LibreOffice background process: {error}"))
}

#[cfg(target_os = "windows")]
fn wait_for_soffice_ready() -> Result<(), String> {
    let pipe_name = windows_named_pipe_name();
    for _ in 0..20 {
        if windows_named_pipe_exists(&pipe_name) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err("LibreOffice background process did not reach ready state".to_string())
}

#[cfg(target_os = "windows")]
fn convert_with_managed_runtime(args: &Args, payload: &ConvertRequest) -> Result<std::process::ExitStatus, String> {
    let python_path = libreoffice_python_path(&args.soffice_path)
        .ok_or_else(|| "LibreOffice Python runtime is unavailable".to_string())?;
    let script_path = write_uno_bridge_script()?;
    let status = with_no_window(
        Command::new(&python_path)
            .arg(&script_path)
            .arg(&payload.source_path)
            .arg(&payload.output_dir)
            .arg(&payload.pdf_path)
            .arg(&args.profile_uri)
            .arg(HELPER_PIPE_NAME)
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .status()
    .map_err(|error| format!("failed to run LibreOffice UNO bridge: {error}"))?;
    Ok(status)
}

#[cfg(target_os = "windows")]
fn convert_with_cli_fallback(args: &Args, payload: &ConvertRequest) -> Result<std::process::ExitStatus, String> {
    Command::new(&args.soffice_path)
        .arg("--headless")
        .arg("--nologo")
        .arg("--nofirststartwizard")
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(&payload.output_dir)
        .arg(&payload.source_path)
        .arg(format!("-env:UserInstallation={}", args.profile_uri))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("failed to run soffice convert: {error}"))
}

#[cfg(target_os = "windows")]
fn libreoffice_python_path(soffice_path: &PathBuf) -> Option<PathBuf> {
    let program_dir = soffice_path.parent()?;
    for candidate in [
        program_dir.join("python.exe"),
        program_dir.join("python-core-3.11.11").join("bin").join("python.exe"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn write_uno_bridge_script() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("momobako-office-convert-helper");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("uno_convert.py");
    std::fs::write(&path, uno_bridge_script()).map_err(|error| error.to_string())?;
    Ok(path)
}

#[cfg(target_os = "windows")]
fn uno_bridge_script() -> &'static str {
    r#"
import os
import sys
from pathlib import Path

import uno
from com.sun.star.beans import PropertyValue

source_path = Path(sys.argv[1]).resolve()
output_dir = Path(sys.argv[2]).resolve()
pdf_path = Path(sys.argv[3]).resolve()
profile_uri = sys.argv[4]
pipe_name = sys.argv[5]

local_context = uno.getComponentContext()
resolver = local_context.ServiceManager.createInstanceWithContext(
    "com.sun.star.bridge.UnoUrlResolver",
    local_context,
)
context = resolver.resolve(
    f"uno:pipe,name={pipe_name};urp;StarOffice.ComponentContext"
)
desktop = context.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", context)

def prop(name, value):
    item = PropertyValue()
    item.Name = name
    item.Value = value
    return item

load_props = (
    prop("Hidden", True),
    prop("ReadOnly", True),
)
store_props = (
    prop("FilterName", "writer_pdf_Export"),
)

document = desktop.loadComponentFromURL(
    uno.systemPathToFileUrl(str(source_path)),
    "_blank",
    0,
    load_props,
)
try:
    document.storeToURL(
        uno.systemPathToFileUrl(str(pdf_path)),
        store_props,
    )
finally:
    document.close(True)
"#
}

#[cfg(target_os = "windows")]
fn windows_named_pipe_name() -> String {
    format!(r"\\.\pipe\{HELPER_PIPE_NAME}")
}

#[cfg(target_os = "windows")]
fn windows_named_pipe_exists(name: &str) -> bool {
    std::fs::OpenOptions::new().read(true).open(name).is_ok()
}

#[cfg(target_os = "windows")]
fn with_no_window(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(target_os = "windows"))]
fn with_no_window(command: &mut Command) -> &mut Command {
    command
}

fn json_response(
    status: StatusCode,
    value: serde_json::Value,
) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()))
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes("Content-Type", "application/json; charset=utf-8")
                .expect("content-type header should be valid"),
        )
}

fn soffice_pid(guard: &Option<Child>) -> Option<u32> {
    guard.as_ref().map(Child::id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_requires_mandatory_fields() {
        let error = parse_args_from(["helper", "--port", "8080"].into_iter().map(String::from))
            .expect_err("missing soffice path should fail");
        assert!(error.contains("--soffice-path"));
    }

    #[test]
    fn windows_named_pipe_name_uses_fixed_helper_channel() {
        #[cfg(target_os = "windows")]
        assert_eq!(windows_named_pipe_name(), r"\\.\pipe\momobako-office-convert");

        #[cfg(not(target_os = "windows"))]
        {
            let _ = "non-windows";
        }
    }

    #[test]
    fn uno_bridge_script_mentions_managed_pipe_name() {
        #[cfg(target_os = "windows")]
        assert!(uno_bridge_script().contains("uno:pipe,name="));

        #[cfg(not(target_os = "windows"))]
        {
            let _ = "non-windows";
        }
    }
}
