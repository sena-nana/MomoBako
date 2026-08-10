//! 归档预览插件的隔离进程入口，使用逐行 JSON 请求响应协议。

use std::io::{self, BufRead, Write};

use momobako_mutsuki_plugin_sdk::{PluginCallEnvelope, PluginRuntimeContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessRequest {
    id: u64,
    method: String,
    payload: serde_json::Value,
    runtime: PluginRuntimeContext,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessResponse {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let response = match line {
            Ok(line) => process_line(&line),
            Err(error) => ProcessResponse {
                id: 0,
                ok: false,
                output: None,
                error: Some(format!("process input error: {error}")),
            },
        };
        match serde_json::to_writer(&mut stdout, &response) {
            Ok(()) => {
                let _ = stdout.write_all(b"\n");
                let _ = stdout.flush();
            }
            Err(error) => {
                eprintln!("failed to encode process response: {error}");
                break;
            }
        }
    }
}

fn process_line(line: &str) -> ProcessResponse {
    let request = match serde_json::from_str::<ProcessRequest>(line.trim_start_matches('\u{feff}'))
    {
        Ok(request) => request,
        Err(error) => {
            return ProcessResponse {
                id: 0,
                ok: false,
                output: None,
                error: Some(format!("invalid process request: {error}")),
            };
        }
    };
    let id = request.id;
    match momobako_service_archive_preview::handle_call(PluginCallEnvelope {
        method: request.method,
        payload: request.payload,
        runtime: request.runtime,
    }) {
        Ok(output) => ProcessResponse {
            id,
            ok: true,
            output: Some(output),
            error: None,
        },
        Err(error) => ProcessResponse {
            id,
            ok: false,
            output: None,
            error: Some(error),
        },
    }
}
