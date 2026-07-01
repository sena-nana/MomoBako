//! Office Convert 独立 helper。
//!
//! 当前提供本地 HTTP 控制接口：
//! - GET /health
//! - POST /convert
//! - POST /shutdown

use std::{
    env,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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

fn main() -> Result<(), String> {
    let args = parse_args()?;
    let server = Server::http(format!("127.0.0.1:{}", args.port))
        .map_err(|error| format!("failed to start helper server: {error}"))?;
    let shutdown = Arc::new(AtomicBool::new(false));
    while !shutdown.load(Ordering::SeqCst) {
        let Some(request) = server.recv_timeout(std::time::Duration::from_millis(250)).ok().flatten() else {
            continue;
        };
        match (request.method(), request.url()) {
            (&Method::Get, "/health") => {
                let _ = request.respond(json_response(
                    StatusCode(200),
                    serde_json::json!({ "ok": true, "runtime": "office-convert-helper" }),
                ));
            }
            (&Method::Post, "/shutdown") => {
                shutdown.store(true, Ordering::SeqCst);
                let _ = request.respond(json_response(
                    StatusCode(200),
                    serde_json::json!({ "ok": true, "stopped": true }),
                ));
            }
            (&Method::Post, "/convert") => {
                let result = handle_convert(request, &args);
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
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut soffice_path = None;
    let mut port = None;
    let mut profile_uri = None;
    let mut args = env::args().skip(1);
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

fn handle_convert(mut request: tiny_http::Request, args: &Args) -> Result<(), String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| format!("failed to read convert request: {error}"))?;
    let payload: ConvertRequest =
        serde_json::from_str(&body).map_err(|error| format!("failed to decode convert request: {error}"))?;

    #[cfg(target_os = "windows")]
    {
        let status = Command::new(&args.soffice_path)
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
            .map_err(|error| format!("failed to run soffice convert: {error}"))?;
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
        Ok(())
    }
}

fn json_response(status: StatusCode, value: serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()))
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes("Content-Type", "application/json; charset=utf-8")
                .expect("content-type header should be valid"),
        )
}
