use crate::repository_service::{ExternalAddAssetRequest, RepositoryState, SyncRequest};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, Mutex},
    thread,
    time::SystemTime,
};
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

const PREVIEW_HOST: &str = "127.0.0.1";
const PREVIEW_PATH_PREFIX: &str = "/preview/";
const EXTERNAL_PATH_PREFIX: &str = "/external/v1/";
const EXTERNAL_CONNECTION_FILE_NAME: &str = "external-api.json";
const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";

#[derive(Clone)]
pub struct RepositoryRuntime {
    repository_state: Arc<RepositoryState>,
    watcher_handle: Arc<Mutex<RepositoryWatcher>>,
    write_lock: Arc<Mutex<()>>,
    preview_addr: String,
    external_connection: ExternalApiConnectionStatus,
}

#[derive(Debug)]
struct RepositoryWatcher {
    watcher: RecommendedWatcher,
    watched_paths: BTreeSet<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl RepositoryRuntime {
    pub fn start() -> Result<Self, String> {
        let root = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(".service-data");
        let repository_state = Arc::new(RepositoryState::from_root(root.clone()));
        let write_lock = Arc::new(Mutex::new(()));
        repository_state.ensure_initialized()?;
        let watcher_handle =
            RepositoryWatcher::start(repository_state.clone(), write_lock.clone())?;
        let preview_addr = start_preview_server(repository_state.clone())?;
        let external_token = generate_external_api_token()?;
        let external_addr = start_external_api_server(
            repository_state.clone(),
            write_lock.clone(),
            external_token.clone(),
        )?;
        let started_at = now_unix_millis().to_string();
        let external_connection =
            build_external_connection_status(&root, &external_addr, &external_token, &started_at);
        write_external_connection_file(&external_connection)?;

        Ok(Self {
            repository_state,
            watcher_handle,
            write_lock,
            preview_addr,
            external_connection,
        })
    }

    pub async fn run_read<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        tauri::async_runtime::spawn_blocking(move || operation(&repository_state))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn run_write<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        let write_lock = self.write_lock.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            operation(&repository_state)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn run_repository_collection_write<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        let watcher_handle = self.watcher_handle.clone();
        let write_lock = self.write_lock.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            let response = operation(&repository_state)?;
            sync_watched_paths(&repository_state, &watcher_handle)?;
            Ok(response)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn repository_thumbnail_roots(&self) -> Result<Vec<PathBuf>, String> {
        let repository_state = self.repository_state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            repository_state.list_repository_thumbnail_roots()
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub fn preview_source_url(&self, token: &str) -> String {
        format!("http://{}/preview/{token}", self.preview_addr)
    }

    pub fn external_api_connection_status(&self) -> ExternalApiConnectionStatus {
        ExternalApiConnectionStatus {
            ready: self.repository_state.ensure_initialized().is_ok(),
            ..self.external_connection.clone()
        }
    }
}

fn start_preview_server(repository_state: Arc<RepositoryState>) -> Result<String, String> {
    let server = Server::http(format!("{PREVIEW_HOST}:0")).map_err(|error| error.to_string())?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| "preview server did not bind to a TCP address".to_string())?;
    let preview_addr = format!("{PREVIEW_HOST}:{}", addr.port());

    thread::spawn(move || {
        for request in server.incoming_requests() {
            let repository_state = repository_state.clone();
            thread::spawn(move || {
                handle_preview_request(request, &repository_state);
            });
        }
    });

    Ok(preview_addr)
}

fn start_external_api_server(
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalConnectionFile {
    base_url: String,
    token: String,
    version: String,
    started_at: String,
}

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

fn build_external_connection_status(
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

fn write_external_connection_file(connection: &ExternalApiConnectionStatus) -> Result<(), String> {
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

fn generate_external_api_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("failed to generate external API token: {error}"))?;
    Ok(hex::encode(bytes))
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

fn now_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

fn handle_preview_request(request: Request, repository_state: &Arc<RepositoryState>) {
    match preview_token_from_url(request.url()) {
        Some(token) if request.method() == &Method::Get || request.method() == &Method::Head => {
            let range_header = request
                .headers()
                .iter()
                .find(|item| item.field.equiv("Range"))
                .map(|item| item.value.as_str().to_string());
            let response =
                repository_state
                    .open_preview_file_source(token)
                    .and_then(|(file, media_type)| {
                        build_preview_file_response(file, &media_type, range_header.as_deref())
                    });
            match response {
                Ok(response) => {
                    let _ = request.respond(response);
                }
                Err(error) => {
                    let _ = request
                        .respond(Response::from_string(error).with_status_code(StatusCode(404)));
                }
            }
        }
        Some(_) => {
            let _ = request.respond(
                Response::from_string("method not allowed").with_status_code(StatusCode(405)),
            );
        }
        None => {
            let _ = request
                .respond(Response::from_string("not found").with_status_code(StatusCode(404)));
        }
    }
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
                        .filter(|repo| {
                            repo.status == "ready"
                                && repo.backend.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
                        })
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

fn build_preview_file_response(
    mut file: File,
    media_type: &str,
    range_header: Option<&str>,
) -> Result<ResponseBox, String> {
    let file_size = file.metadata().map_err(|error| error.to_string())?.len();
    let mut headers = base_preview_headers(media_type);
    headers.push(header("Accept-Ranges", "bytes"));

    if let Some(range_value) = range_header {
        let Some(range) = parse_byte_range(range_value, file_size) else {
            return Ok(Response::from_string("range not satisfiable")
                .with_status_code(StatusCode(416))
                .with_header(header("Content-Range", &format!("bytes */{file_size}")))
                .with_header(header("Access-Control-Allow-Origin", "*"))
                .boxed());
        };
        file.seek(SeekFrom::Start(range.start))
            .map_err(|error| error.to_string())?;
        let length = range.end - range.start + 1;
        headers.push(header(
            "Content-Range",
            &format!("bytes {}-{}/{}", range.start, range.end, file_size),
        ));
        return Ok(Response::new(
            StatusCode(206),
            headers,
            file.take(length),
            Some(length as usize),
            None,
        )
        .boxed());
    }

    Ok(Response::from_file(file)
        .with_header(header("Content-Type", media_type))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Accept-Ranges", "bytes"))
        .boxed())
}

fn base_preview_headers(media_type: &str) -> Vec<Header> {
    vec![
        header("Content-Type", media_type),
        header("Cache-Control", "no-store"),
        header("Access-Control-Allow-Origin", "*"),
    ]
}

fn parse_byte_range(value: &str, file_size: u64) -> Option<ByteRange> {
    if file_size == 0 {
        return None;
    }
    let range_value = value.trim().strip_prefix("bytes=")?;
    if range_value.contains(',') {
        return None;
    }
    let (start_value, end_value) = range_value.split_once('-')?;
    if start_value.is_empty() {
        let suffix_length = end_value.parse::<u64>().ok()?;
        if suffix_length == 0 {
            return None;
        }
        let start = file_size.saturating_sub(suffix_length);
        return Some(ByteRange {
            start,
            end: file_size - 1,
        });
    }

    let start = start_value.parse::<u64>().ok()?;
    if start >= file_size {
        return None;
    }
    let end = if end_value.is_empty() {
        file_size - 1
    } else {
        end_value.parse::<u64>().ok()?.min(file_size - 1)
    };
    if end < start {
        return None;
    }
    Some(ByteRange { start, end })
}

fn preview_token_from_url(url: &str) -> Option<&str> {
    let token = url
        .strip_prefix(PREVIEW_PATH_PREFIX)?
        .split(['?', '#'])
        .next()?;
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(token)
    } else {
        None
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("preview header should be valid")
}

impl RepositoryWatcher {
    fn start(
        repository_state: Arc<RepositoryState>,
        write_lock: Arc<Mutex<()>>,
    ) -> Result<Arc<Mutex<Self>>, String> {
        let (tx, rx) = channel::<notify::Result<Event>>();
        let watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|error| error.to_string())?;

        let handle = Arc::new(Mutex::new(Self {
            watcher,
            watched_paths: BTreeSet::new(),
        }));
        let repository_state_for_thread = repository_state.clone();

        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                if let Ok(event) = event {
                    handle_fs_event(&repository_state_for_thread, &write_lock, event);
                }
            }
        });

        sync_watched_paths(&repository_state, &handle)?;
        Ok(handle)
    }
}

fn sync_watched_paths(
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<(), String> {
    let repositories = repository_state.list_repositories()?;
    let desired_paths = repositories
        .into_iter()
        .filter(|repository| repository.backend.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID)
        .map(|repository| PathBuf::from(repository.path))
        .collect::<BTreeSet<_>>();

    let mut watcher = watcher_handle
        .lock()
        .map_err(|_| "watcher state lock poisoned".to_string())?;
    let current_paths = watcher.watched_paths.clone();

    for path in current_paths.difference(&desired_paths) {
        watcher
            .watcher
            .unwatch(path)
            .map_err(|error| error.to_string())?;
    }

    for path in desired_paths.difference(&current_paths) {
        watcher
            .watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;
    }

    watcher.watched_paths = desired_paths;
    Ok(())
}

fn handle_fs_event(
    repository_state: &Arc<RepositoryState>,
    write_lock: &Arc<Mutex<()>>,
    event: Event,
) {
    let Ok(repositories) = repository_state.list_repositories() else {
        return;
    };

    for path in event.paths {
        let normalized_path = normalize_path(&path);
        if let Some(repository) = repositories
            .iter()
            .find(|repo| normalized_path.starts_with(&normalize_path(Path::new(&repo.path))))
        {
            let Ok(_guard) = write_lock.lock() else {
                return;
            };
            let _ = repository_state.sync_repository(SyncRequest {
                repo_id: repository.repo_id.clone(),
            });
        }
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository_service::{
        install_local_filesystem_test_plugin_archive, FileReadRequest, RepositoryMutationRequest,
    };
    use rusqlite::{params, Connection};
    use std::{
        fs,
        io::{Read, Write},
        net::TcpStream,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "momobako-runtime-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn preview_token_from_url_accepts_registered_route() {
        let token = "a".repeat(64);
        let url = format!("/preview/{token}?v=1");

        assert_eq!(preview_token_from_url(&url), Some(token.as_str()));
    }

    #[test]
    fn preview_token_from_url_rejects_invalid_tokens() {
        assert_eq!(preview_token_from_url("/preview/not-hex"), None);
        assert_eq!(
            preview_token_from_url(&format!("/preview/{}", "g".repeat(64))),
            None
        );
        assert_eq!(
            preview_token_from_url(&format!("/other/{}", "0".repeat(64))),
            None
        );
    }

    #[test]
    fn preview_parse_byte_range_accepts_standard_and_suffix_ranges() {
        assert_eq!(
            parse_byte_range("bytes=2-5", 10),
            Some(ByteRange { start: 2, end: 5 })
        );
        assert_eq!(
            parse_byte_range("bytes=7-", 10),
            Some(ByteRange { start: 7, end: 9 })
        );
        assert_eq!(
            parse_byte_range("bytes=-4", 10),
            Some(ByteRange { start: 6, end: 9 })
        );
        assert_eq!(
            parse_byte_range("bytes=8-99", 10),
            Some(ByteRange { start: 8, end: 9 })
        );
    }

    #[test]
    fn preview_parse_byte_range_rejects_unsatisfiable_ranges() {
        assert_eq!(parse_byte_range("items=0-1", 10), None);
        assert_eq!(parse_byte_range("bytes=10-12", 10), None);
        assert_eq!(parse_byte_range("bytes=5-4", 10), None);
        assert_eq!(parse_byte_range("bytes=0-1,4-5", 10), None);
        assert_eq!(parse_byte_range("bytes=-0", 10), None);
    }

    #[test]
    fn preview_server_serves_registered_source_file() {
        let root = unique_temp_dir("preview-server");
        let service_root = root.join("state");
        let repo_root = root.join("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("model.glb"), b"glb-body")
            .expect("preview source should be written");
        install_local_filesystem_test_plugin_archive(&service_root);

        let state = RepositoryState::from_root(service_root);
        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-preview-server".to_string()),
                name: "Preview Server".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
            })
            .expect("repository should be created")
            .repository
            .repo_id;
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "model.glb".to_string(),
            })
            .expect("preview source should be prepared");

        let addr = start_preview_server(Arc::new(state)).expect("preview server should start");
        let mut stream =
            TcpStream::connect(addr).expect("preview server should accept connections");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");
        write!(
            stream,
            "GET /preview/{} HTTP/1.0\r\nHost: localhost\r\n\r\n",
            response.token
        )
        .expect("request should be written");

        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .expect("response should be readable");

        assert!(raw.starts_with("HTTP/1.0 200 OK"));
        assert!(raw.contains("Content-Type: model/gltf-binary"));
        assert!(raw.ends_with("glb-body"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn external_api_serves_health_and_requires_token() {
        let root = unique_temp_dir("external-api");
        let service_root = root.join("state");
        let repo_root = root.join("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        let state = RepositoryState::from_root(service_root.clone());
        state
            .ensure_initialized()
            .expect("repository state should initialize");
        let repo_id = "repo-external-api".to_string();
        let registry =
            Connection::open(service_root.join("repositories.db")).expect("registry should open");
        registry
            .execute(
                r#"
                INSERT INTO repositories (
                  repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
                )
                VALUES (?1, 'External API Repo', ?2, ?3, '{}', 'ready', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')
                "#,
                params![&repo_id, repo_root.to_string_lossy(), LOCAL_FILESYSTEM_PLUGIN_ID],
            )
            .expect("repository should be registered");
        drop(registry);

        let token = "token-test".to_string();
        let addr =
            start_external_api_server(Arc::new(state), Arc::new(Mutex::new(())), token.clone())
                .expect("external API should start");

        let mut health = TcpStream::connect(&addr).expect("external API should accept connections");
        health
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");
        write!(
            health,
            "GET /external/v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n"
        )
        .expect("request should be written");
        let mut health_raw = String::new();
        health
            .read_to_string(&mut health_raw)
            .expect("health response should be readable");
        assert!(health_raw.starts_with("HTTP/1.0 200 OK"));
        assert!(health_raw.contains("assets.add.remoteUrl"));

        let mut unauthorized =
            TcpStream::connect(&addr).expect("external API should accept connections");
        unauthorized
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");
        write!(
            unauthorized,
            "GET /external/v1/repositories HTTP/1.0\r\nHost: localhost\r\n\r\n"
        )
        .expect("request should be written");
        let mut unauthorized_raw = String::new();
        unauthorized
            .read_to_string(&mut unauthorized_raw)
            .expect("unauthorized response should be readable");
        assert!(unauthorized_raw.starts_with("HTTP/1.0 401 Unauthorized"));
        drop(unauthorized);

        let mut authorized =
            TcpStream::connect(&addr).expect("external API should accept connections");
        authorized
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");
        write!(
            authorized,
            "GET /external/v1/repositories HTTP/1.0\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
        )
        .expect("request should be written");
        let mut authorized_raw = String::new();
        authorized
            .read_to_string(&mut authorized_raw)
            .expect("authorized response should be readable");
        assert!(authorized_raw.starts_with("HTTP/1.0 200 OK"));
        assert!(authorized_raw.contains(&repo_id));
        drop(authorized);
        drop(health);
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }

    #[test]
    fn preview_server_serves_registered_source_file_range() {
        let root = unique_temp_dir("preview-server-range");
        let service_root = root.join("state");
        let repo_root = root.join("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("clip.mp4"), b"media-body")
            .expect("preview source should be written");
        install_local_filesystem_test_plugin_archive(&service_root);

        let state = RepositoryState::from_root(service_root);
        let repo_id = state
            .create_repository(RepositoryMutationRequest {
                repo_id: Some("repo-preview-range".to_string()),
                name: "Preview Range".to_string(),
                path: repo_root.to_string_lossy().to_string(),
                backend_plugin_id: None,
                backend_config: None,
            })
            .expect("repository should be created")
            .repository
            .repo_id;
        let response = state
            .prepare_preview_file_source(FileReadRequest {
                repo_id,
                path: "clip.mp4".to_string(),
            })
            .expect("preview source should be prepared");

        let addr = start_preview_server(Arc::new(state)).expect("preview server should start");
        let mut stream =
            TcpStream::connect(addr).expect("preview server should accept connections");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should be set");
        write!(
            stream,
            "GET /preview/{} HTTP/1.0\r\nHost: localhost\r\nRange: bytes=6-9\r\n\r\n",
            response.token
        )
        .expect("request should be written");

        let mut raw = String::new();
        stream
            .read_to_string(&mut raw)
            .expect("response should be readable");

        assert!(raw.starts_with("HTTP/1.0 206 Partial Content"));
        assert!(raw.contains("Content-Type: video/mp4"));
        assert!(raw.contains("Content-Range: bytes 6-9/10"));
        assert!(raw.ends_with("body"));
        fs::remove_dir_all(root).expect("test temp root should be removed");
    }
}
