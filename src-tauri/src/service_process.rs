use crate::repository_service::{
    FileBrowserRequest, FileCreateRequest, FileDeleteRequest, FileImportRequest, FileReadRequest,
    FileRenameRequest, MetadataUpdateRequest, RepositoryFolderRequest, RepositoryMutationRequest,
    RepositoryState, RevisionActionRequest, SearchRequest, SyncRequest, ThumbnailRequest,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::Read,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc::channel, Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Manager};
use tiny_http::{Method, Response, Server, StatusCode};

const SERVICE_HOST: &str = "127.0.0.1";
const SERVICE_START_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct ServiceBridge {
    addr: String,
}

struct RepositoryWatcher {
    watcher: RecommendedWatcher,
    watched_paths: BTreeSet<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
enum ServiceRequest {
    Ping,
    ListRepositories,
    GetRepositorySnapshot {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
    },
    GetAssetDetail {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
        #[serde(rename = "assetId", alias = "asset_id")]
        asset_id: String,
    },
    SearchAssets {
        request: SearchRequest,
    },
    UpdateAssetMetadata {
        request: MetadataUpdateRequest,
    },
    GetFileBrowser {
        request: FileBrowserRequest,
    },
    ReadFile {
        request: FileReadRequest,
    },
    CreateDirectory {
        request: FileCreateRequest,
    },
    CreateFile {
        request: FileCreateRequest,
    },
    ImportEntries {
        request: FileImportRequest,
    },
    RenameEntry {
        request: FileRenameRequest,
    },
    DeleteEntry {
        request: FileDeleteRequest,
    },
    CreateRepository {
        request: RepositoryMutationRequest,
    },
    ImportRepository {
        request: RepositoryMutationRequest,
    },
    AttachRepositoryFolder {
        request: RepositoryFolderRequest,
    },
    DeleteRepository {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
    },
    ExportRepository {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
    },
    SyncRepository {
        request: SyncRequest,
    },
    EnsureThumbnail {
        request: ThumbnailRequest,
    },
    UndoLastRevision {
        request: RevisionActionRequest,
    },
    RedoLastRevision {
        request: RevisionActionRequest,
    },
    ListPlugins,
    GetCacheSnapshot,
    GetApiDesignSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceResponse<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

impl ServiceBridge {
    pub fn start(app: &AppHandle) -> Result<Self, String> {
        let thumbnail_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?
            .join("thumbnails");
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;

        for _ in 0..SERVICE_START_ATTEMPTS {
            let addr = reserve_service_addr()?;
            let mut command = Command::new(&executable);
            command
                .arg("--service-mode")
                .arg(&addr)
                .arg("--thumbnail-dir")
                .arg(&thumbnail_dir);

            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }

            command
                .spawn()
                .map_err(|error| format!("failed to start service process: {error}"))?;

            for _ in 0..40 {
                if ping_service(&addr).is_ok() {
                    return Ok(Self { addr });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        Err("service process did not become ready on a dynamic port".to_string())
    }

    pub fn invoke<T, S>(&self, request: &S) -> Result<T, String>
    where
        T: for<'de> Deserialize<'de>,
        S: Serialize,
    {
        let client = std::net::TcpStream::connect(&self.addr).map_err(|error| error.to_string())?;
        let request_json = serde_json::to_string(request).map_err(|error| error.to_string())?;
        let http_request = format!(
            "POST /rpc HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.addr,
            request_json.len(),
            request_json
        );
        use std::io::Write;
        (&client)
            .write_all(http_request.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut response_raw = String::new();
        let mut reader = std::io::BufReader::new(client);
        reader
            .read_to_string(&mut response_raw)
            .map_err(|error| error.to_string())?;

        let body = decode_http_response_body(&response_raw)?;
        let response: ServiceResponse<T> =
            serde_json::from_slice(&body).map_err(|error| error.to_string())?;
        if response.ok {
            response
                .data
                .ok_or_else(|| "missing service payload".to_string())
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "service request failed".to_string()))
        }
    }
}

fn reserve_service_addr() -> Result<String, String> {
    let listener = TcpListener::bind((SERVICE_HOST, 0)).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    Ok(addr.to_string())
}

impl RepositoryWatcher {
    fn start(repository_state: Arc<Mutex<RepositoryState>>) -> Result<Arc<Mutex<Self>>, String> {
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
                    handle_fs_event(&repository_state_for_thread, event);
                }
            }
        });

        sync_watched_paths(&repository_state, &handle)?;
        Ok(handle)
    }
}

pub fn run_service_process(addr: &str) -> Result<(), String> {
    let root = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(".service-data");
    let thumbnail_root =
        service_thumbnail_dir_from_args()?.unwrap_or_else(|| root.join("thumbnails"));
    let repository_state = Arc::new(Mutex::new(RepositoryState::from_roots(
        root,
        thumbnail_root,
    )));
    repository_state
        .lock()
        .map_err(|_| "service state lock poisoned".to_string())?
        .ensure_initialized()?;
    let watcher_handle = RepositoryWatcher::start(repository_state.clone())?;

    let server = Server::http(addr).map_err(|error| error.to_string())?;
    for mut request in server.incoming_requests() {
        if request.method() != &Method::Post || request.url() != "/rpc" {
            let _ = request.respond(Response::empty(StatusCode(404)));
            continue;
        }

        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            let _ = request.respond(json_response(&ServiceResponse::<serde_json::Value> {
                ok: false,
                data: None,
                error: Some("failed to read request body".to_string()),
            }));
            continue;
        }

        let response = handle_service_request(&repository_state, &watcher_handle, &body);
        let _ = request.respond(response);
    }

    Ok(())
}

fn service_thumbnail_dir_from_args() -> Result<Option<PathBuf>, String> {
    let mut args = std::env::args().skip(3);
    while let Some(arg) = args.next() {
        if arg == "--thumbnail-dir" {
            return args
                .next()
                .map(PathBuf::from)
                .map(Some)
                .ok_or_else(|| "missing --thumbnail-dir value".to_string());
        }
    }
    Ok(None)
}

fn handle_service_request(
    repository_state: &Arc<Mutex<RepositoryState>>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
    body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let result = serde_json::from_str::<ServiceRequest>(body)
        .map_err(|error| error.to_string())
        .and_then(|request| dispatch_request(repository_state, watcher_handle, request));

    match result {
        Ok(payload) => json_response(&ServiceResponse {
            ok: true,
            data: Some(payload),
            error: None::<String>,
        }),
        Err(error) => json_response(&ServiceResponse::<serde_json::Value> {
            ok: false,
            data: None,
            error: Some(error),
        }),
    }
}

fn dispatch_request(
    repository_state: &Arc<Mutex<RepositoryState>>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
    request: ServiceRequest,
) -> Result<serde_json::Value, String> {
    match request {
        ServiceRequest::Ping => Ok(serde_json::json!("pong")),
        ServiceRequest::ListRepositories => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.list_repositories()?)
        }
        ServiceRequest::GetRepositorySnapshot { repo_id } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.load_snapshot(&repo_id)?)
        }
        ServiceRequest::GetAssetDetail { repo_id, asset_id } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.load_asset_detail(&repo_id, &asset_id)?)
        }
        ServiceRequest::SearchAssets { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.search_assets(request)?)
        }
        ServiceRequest::UpdateAssetMetadata { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.update_asset_metadata(request)?)
        }
        ServiceRequest::GetFileBrowser { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.load_file_browser(request)?)
        }
        ServiceRequest::ReadFile { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.read_file(request)?)
        }
        ServiceRequest::CreateDirectory { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.create_directory(request)?
            };
            to_value(response)
        }
        ServiceRequest::CreateFile { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.create_file(request)?
            };
            to_value(response)
        }
        ServiceRequest::ImportEntries { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.import_entries(request)?
            };
            to_value(response)
        }
        ServiceRequest::RenameEntry { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.rename_entry(request)?
            };
            to_value(response)
        }
        ServiceRequest::DeleteEntry { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.delete_entry(request)?
            };
            to_value(response)
        }
        ServiceRequest::CreateRepository { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.create_repository(request)?
            };
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::ImportRepository { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.import_repository(request)?
            };
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::AttachRepositoryFolder { request } => {
            let response = {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.attach_repository_folder(request)?
            };
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::DeleteRepository { repo_id } => {
            {
                let state = repository_state
                    .lock()
                    .map_err(|_| "service state lock poisoned".to_string())?;
                state.delete_repository(&repo_id)?;
            }
            sync_watched_paths(repository_state, watcher_handle)?;
            Ok(serde_json::json!(null))
        }
        ServiceRequest::ExportRepository { repo_id } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.export_repository(&repo_id)?)
        }
        ServiceRequest::SyncRepository { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.sync_repository(request)?)
        }
        ServiceRequest::EnsureThumbnail { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.ensure_thumbnail(request)?)
        }
        ServiceRequest::UndoLastRevision { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.undo_last_revision(request)?)
        }
        ServiceRequest::RedoLastRevision { request } => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.redo_last_revision(request)?)
        }
        ServiceRequest::ListPlugins => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.list_plugins()?)
        }
        ServiceRequest::GetCacheSnapshot => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.get_cache_snapshot()?)
        }
        ServiceRequest::GetApiDesignSnapshot => {
            let state = repository_state
                .lock()
                .map_err(|_| "service state lock poisoned".to_string())?;
            to_value(state.get_api_design_snapshot()?)
        }
    }
}

fn sync_watched_paths(
    repository_state: &Arc<Mutex<RepositoryState>>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<(), String> {
    let repositories = repository_state
        .lock()
        .map_err(|_| "service state lock poisoned".to_string())?
        .list_repositories()?;
    let desired_paths = repositories
        .into_iter()
        .filter(|repository| repository.backend.plugin_id == "builtin.local-filesystem")
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

fn to_value<T: Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn json_response<T: Serialize>(payload: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    Response::from_data(body).with_status_code(StatusCode(200))
}

fn decode_http_response_body(response_raw: &str) -> Result<Vec<u8>, String> {
    let (headers, body) = response_raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid service response".to_string())?;

    if headers.lines().any(|line| {
        let line = line.trim();
        line.len() >= "Transfer-Encoding:".len()
            && line[.."Transfer-Encoding:".len()].eq_ignore_ascii_case("Transfer-Encoding:")
            && line["Transfer-Encoding:".len()..]
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
    }) {
        decode_chunked_body(body)
    } else {
        Ok(body.as_bytes().to_vec())
    }
}

fn decode_chunked_body(body: &str) -> Result<Vec<u8>, String> {
    let bytes = body.as_bytes();
    let mut cursor = 0usize;
    let mut decoded = Vec::new();

    loop {
        let size_end = find_crlf(bytes, cursor)
            .ok_or_else(|| "invalid chunked service response".to_string())?;
        let size_text = std::str::from_utf8(&bytes[cursor..size_end])
            .map_err(|error| error.to_string())?
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|error| format!("invalid chunk size: {error}"))?;
        cursor = size_end + 2;

        if size == 0 {
            break;
        }

        let chunk_end = cursor
            .checked_add(size)
            .ok_or_else(|| "chunked service response is too large".to_string())?;
        if chunk_end + 2 > bytes.len() || &bytes[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err("invalid chunked service response".to_string());
        }
        decoded.extend_from_slice(&bytes[cursor..chunk_end]);
        cursor = chunk_end + 2;
    }

    Ok(decoded)
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|position| start + position)
}

fn ping_service(addr: &str) -> Result<(), String> {
    let bridge = ServiceBridge {
        addr: addr.to_string(),
    };
    let value: String = bridge.invoke(&serde_json::json!({ "command": "ping" }))?;
    if value == "pong" {
        Ok(())
    } else {
        Err("service ping failed".to_string())
    }
}

fn handle_fs_event(repository_state: &Arc<Mutex<RepositoryState>>, event: Event) {
    let Ok(state) = repository_state.lock() else {
        return;
    };
    let Ok(repositories) = state.list_repositories() else {
        return;
    };

    for path in event.paths {
        let normalized_path = normalize_path(&path);
        if let Some(repository) = repositories
            .iter()
            .find(|repo| normalized_path.starts_with(&normalize_path(Path::new(&repo.path))))
        {
            let _ = state.sync_repository(SyncRequest {
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

    #[test]
    fn decodes_plain_http_response_body() {
        let raw = "HTTP/1.1 200 OK\r\nContent-Length: 31\r\n\r\n{\"ok\":true,\"data\":\"pong\"}";

        let body = decode_http_response_body(raw).expect("plain body should decode");

        assert_eq!(body, br#"{"ok":true,"data":"pong"}"#);
    }

    #[test]
    fn decodes_chunked_http_response_body() {
        let raw = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Transfer-Encoding: chunked\r\n",
            "\r\n",
            "5\r\n",
            "{\"ok\"",
            "\r\n",
            "14\r\n",
            ":true,\"data\":\"pong\"}",
            "\r\n",
            "0\r\n",
            "\r\n"
        );

        let body = decode_http_response_body(raw).expect("chunked body should decode");

        assert_eq!(body, br#"{"ok":true,"data":"pong"}"#);
    }

    #[test]
    fn rejects_invalid_chunked_http_response_body() {
        let raw = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Transfer-Encoding: chunked\r\n",
            "\r\n",
            "5\r\n",
            "{\"ok\""
        );

        let error = decode_http_response_body(raw).expect_err("truncated chunk should fail");

        assert!(error.contains("invalid chunked service response"));
    }
}
