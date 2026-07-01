//! Loopback external API server and connection-file helpers.

use super::PREVIEW_HOST;
use crate::services::repository::{
    backend_summary_supports_local_write_access, ExternalAddAssetRequest, RepositoryState,
    RepositorySummary,
};
use serde::Serialize;
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::SystemTime,
};
use tiny_http::{Method, Request, Response, Server, StatusCode};

const EXTERNAL_PATH_PREFIX: &str = "/external/v1/";
const EXTERNAL_CONNECTION_FILE_NAME: &str = "external-api.json";
fn repository_supports_external_add_assets(summary: &RepositorySummary) -> bool {
    summary.status == "ready"
        && backend_summary_supports_local_write_access(&summary.backend)
        && Path::new(&summary.path).is_absolute()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalConnectionFile {
    base_url: String,
    token: String,
    version: String,
    started_at: String,
}

/// Public external API connection payload exposed to the desktop shell and local clients.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalApiConnectionStatus {
    pub base_url: String,
    pub token: String,
    pub version: String,
    pub started_at: String,
    pub ready: bool,
    pub connection_file_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalHealthResponse {
    version: String,
    ready: bool,
    capabilities: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalErrorResponse {
    code: String,
    message: String,
    retryable: bool,
}

/// Starts the loopback external API server.
pub(crate) fn start_external_api_server(
    repository_state: Arc<RepositoryState>,
    write_lock: Arc<Mutex<()>>,
    token: String,
) -> Result<String, String> {
    let server = Server::http(format!("{PREVIEW_HOST}:0")).map_err(|error| error.to_string())?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| "external API server did not bind to a TCP address".to_string())?;
    let external_addr = format!("{PREVIEW_HOST}:{}", addr.port());

    thread::spawn(move || {
        for request in server.incoming_requests() {
            let repository_state = repository_state.clone();
            let write_lock = write_lock.clone();
            let token = token.clone();
            thread::spawn(move || {
                handle_external_api_request(request, &repository_state, &write_lock, &token);
            });
        }
    });

    Ok(external_addr)
}

/// Builds the persisted connection payload written during runtime startup.
pub(crate) fn build_external_connection_status(
    root: &Path,
    addr: &str,
    token: &str,
    started_at: &str,
) -> ExternalApiConnectionStatus {
    ExternalApiConnectionStatus {
        base_url: format!("http://{addr}/external/v1"),
        token: token.to_string(),
        version: "1".to_string(),
        started_at: started_at.to_string(),
        ready: true,
        connection_file_path: root
            .join(EXTERNAL_CONNECTION_FILE_NAME)
            .to_string_lossy()
            .to_string(),
    }
}

/// Writes the external API connection file under the runtime service root.
pub(crate) fn write_external_connection_file(
    connection: &ExternalApiConnectionStatus,
) -> Result<(), String> {
    let connection_file_path = Path::new(&connection.connection_file_path);
    if let Some(parent) = connection_file_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let payload = ExternalConnectionFile {
        base_url: connection.base_url.clone(),
        token: connection.token.clone(),
        version: connection.version.clone(),
        started_at: connection.started_at.clone(),
    };
    let json = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    fs::write(connection_file_path, json).map_err(|error| error.to_string())
}

/// Generates a runtime-local bearer token for the external API server.
pub(crate) fn generate_external_api_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("failed to generate external API token: {error}"))?;
    Ok(hex::encode(bytes))
}

pub(crate) fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

fn generate_external_request_id() -> Result<String, String> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("failed to generate external request id: {error}"))?;
    Ok(format!(
        "external-{}-{}",
        now_unix_millis(),
        hex::encode(bytes)
    ))
}

fn handle_external_api_request(
    mut request: Request,
    repository_state: &Arc<RepositoryState>,
    write_lock: &Arc<Mutex<()>>,
    token: &str,
) {
    let path = request
        .url()
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_string();
    if request.method() == &Method::Options {
        respond_json(request, StatusCode(204), &serde_json::json!({}));
        return;
    }
    if !path.starts_with(EXTERNAL_PATH_PREFIX) {
        respond_external_error(request, StatusCode(404), "notFound", "not found", false);
        return;
    }

    match (request.method(), path.as_str()) {
        (&Method::Get, "/external/v1/health") => {
            respond_json(
                request,
                StatusCode(200),
                &ExternalHealthResponse {
                    version: "1".to_string(),
                    ready: repository_state.ensure_initialized().is_ok(),
                    capabilities: vec!["assets.add.remoteUrl"],
                },
            );
        }
        (&Method::Get, "/external/v1/repositories") => {
            if !external_authorized(&request, token) {
                respond_external_error(
                    request,
                    StatusCode(401),
                    "unauthorized",
                    "unauthorized",
                    false,
                );
                return;
            }
            match repository_state.list_repositories() {
                Ok(repositories) => {
                    let repositories = repositories
                        .into_iter()
                        .filter(repository_supports_external_add_assets)
                        .collect::<Vec<_>>();
                    respond_json(request, StatusCode(200), &repositories);
                }
                Err(error) => {
                    respond_external_error(request, StatusCode(503), "notReady", &error, true);
                }
            }
        }
        (&Method::Post, "/external/v1/assets:add") => {
            if !external_authorized(&request, token) {
                respond_external_error(
                    request,
                    StatusCode(401),
                    "unauthorized",
                    "unauthorized",
                    false,
                );
                return;
            }
            let mut body = String::new();
            if let Err(error) = request.as_reader().read_to_string(&mut body) {
                respond_external_error(
                    request,
                    StatusCode(400),
                    "invalidInput",
                    &format!("invalid request body: {error}"),
                    false,
                );
                return;
            }
            let payload = match serde_json::from_str::<ExternalAddAssetRequest>(&body) {
                Ok(payload) => payload,
                Err(error) => {
                    respond_external_error(
                        request,
                        StatusCode(400),
                        "invalidInput",
                        &format!("invalid JSON: {error}"),
                        false,
                    );
                    return;
                }
            };
            let request_id = match generate_external_request_id() {
                Ok(value) => value,
                Err(error) => {
                    respond_external_error(request, StatusCode(503), "notReady", &error, true);
                    return;
                }
            };
            let Ok(_guard) = write_lock.lock() else {
                respond_external_error(
                    request,
                    StatusCode(503),
                    "notReady",
                    "repository write lock poisoned",
                    true,
                );
                return;
            };
            let response = repository_state.add_external_assets(request_id, payload);
            let status = if response.status == "failed" {
                StatusCode(422)
            } else {
                StatusCode(200)
            };
            respond_json(request, status, &response);
        }
        _ => respond_external_error(
            request,
            StatusCode(404),
            "notFound",
            "external API route not found",
            false,
        ),
    }
}

fn external_authorized(request: &Request, token: &str) -> bool {
    request.headers().iter().any(|header| {
        header.field.equiv("Authorization") && header.value.as_str() == format!("Bearer {token}")
    })
}

fn respond_external_error(
    request: Request,
    status: StatusCode,
    code: &str,
    message: &str,
    retryable: bool,
) {
    respond_json(
        request,
        status,
        &ExternalErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        },
    );
}

fn respond_json<T: Serialize>(request: Request, status: StatusCode, payload: &T) {
    let body = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    let _ = request.respond(
        Response::from_string(body)
            .with_status_code(status)
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "no-store"))
            .with_header(header("Access-Control-Allow-Origin", "*"))
            .with_header(header(
                "Access-Control-Allow-Headers",
                "Authorization, Content-Type",
            ))
            .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")),
    );
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("external API header should be valid")
}
