use crate::repository_service::{
    FileBrowserRequest, FileCreateRequest, FileDeleteRequest, FileImportRequest, FileReadRequest,
    FileRenameRequest, MetadataUpdateRequest, RepositoryExportRequest, RepositoryFolderRequest,
    RepositoryMutationRequest, RepositoryState, RevisionActionRequest, SearchRequest, SyncRequest,
    ThumbnailRequest,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::Read,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{mpsc::channel, Arc, Mutex},
    thread,
};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::{AppHandle, Manager};
use tiny_http::{Method, Response, Server, StatusCode};

const SERVICE_HOST: &str = "127.0.0.1";
const SERVICE_START_ATTEMPTS: usize = 3;
#[cfg(test)]
static SERVICE_HANDLE_SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct ServiceBridge {
    addr: String,
    child: Arc<ServiceProcessHandle>,
}

#[derive(Debug)]
struct ServiceProcessHandle {
    child: Mutex<Option<Child>>,
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
    ExportRepository { request: RepositoryExportRequest },
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

            let mut child = command
                .spawn()
                .map_err(|error| format!("failed to start service process: {error}"))?;

            for _ in 0..40 {
                if ping_service(&addr).is_ok() {
                    return Ok(Self {
                        addr,
                        child: Arc::new(ServiceProcessHandle::new(Some(child))),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let _ = child.kill();
            let _ = child.wait();
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

    pub fn shutdown(&self) {
        self.child.shutdown();
    }
}

impl ServiceProcessHandle {
    fn new(child: Option<Child>) -> Self {
        Self {
            child: Mutex::new(child),
        }
    }

    fn shutdown(&self) {
        #[cfg(test)]
        SERVICE_HANDLE_SHUTDOWN_COUNT.fetch_add(1, Ordering::SeqCst);

        let Ok(mut child) = self.child.lock() else {
            return;
        };
        if let Some(mut child) = child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ServiceProcessHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn reserve_service_addr() -> Result<String, String> {
    let listener = TcpListener::bind((SERVICE_HOST, 0)).map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    Ok(addr.to_string())
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

pub fn run_service_process(addr: &str) -> Result<(), String> {
    let root = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(".service-data");
    let thumbnail_root =
        service_thumbnail_dir_from_args()?.unwrap_or_else(|| root.join("thumbnails"));
    let repository_state = Arc::new(RepositoryState::from_roots(root, thumbnail_root));
    let write_lock = Arc::new(Mutex::new(()));
    repository_state.ensure_initialized()?;
    let watcher_handle = RepositoryWatcher::start(repository_state.clone(), write_lock.clone())?;

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

        let repository_state = repository_state.clone();
        let watcher_handle = watcher_handle.clone();
        let write_lock = write_lock.clone();
        thread::spawn(move || {
            let response =
                handle_service_request(&repository_state, &watcher_handle, &write_lock, &body);
            let _ = request.respond(response);
        });
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
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
    write_lock: &Arc<Mutex<()>>,
    body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let result = serde_json::from_str::<ServiceRequest>(body)
        .map_err(|error| error.to_string())
        .and_then(|request| {
            dispatch_request(repository_state, watcher_handle, write_lock, request)
        });

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
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
    write_lock: &Arc<Mutex<()>>,
    request: ServiceRequest,
) -> Result<serde_json::Value, String> {
    match request {
        ServiceRequest::Ping => Ok(serde_json::json!("pong")),
        ServiceRequest::ListRepositories => to_value(repository_state.list_repositories()?),
        ServiceRequest::GetRepositorySnapshot { repo_id } => {
            to_value(repository_state.load_snapshot(&repo_id)?)
        }
        ServiceRequest::GetAssetDetail { repo_id, asset_id } => {
            to_value(repository_state.load_asset_detail(&repo_id, &asset_id)?)
        }
        ServiceRequest::SearchAssets { request } => {
            to_value(repository_state.search_assets(request)?)
        }
        ServiceRequest::UpdateAssetMetadata { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.update_asset_metadata(request)?)
        }
        ServiceRequest::GetFileBrowser { request } => {
            to_value(repository_state.load_file_browser(request)?)
        }
        ServiceRequest::ReadFile { request } => to_value(repository_state.read_file(request)?),
        ServiceRequest::CreateDirectory { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.create_directory(request)?)
        }
        ServiceRequest::CreateFile { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.create_file(request)?)
        }
        ServiceRequest::ImportEntries { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.import_entries(request)?)
        }
        ServiceRequest::RenameEntry { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.rename_entry(request)?)
        }
        ServiceRequest::DeleteEntry { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.delete_entry(request)?)
        }
        ServiceRequest::CreateRepository { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            let response = repository_state.create_repository(request)?;
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::ImportRepository { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            let response = repository_state.import_repository(request)?;
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::AttachRepositoryFolder { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            let response = repository_state.attach_repository_folder(request)?;
            sync_watched_paths(repository_state, watcher_handle)?;
            to_value(response)
        }
        ServiceRequest::DeleteRepository { repo_id } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            repository_state.delete_repository(&repo_id)?;
            sync_watched_paths(repository_state, watcher_handle)?;
            Ok(serde_json::json!(null))
        }
        ServiceRequest::ExportRepository { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.export_repository(request)?)
        }
        ServiceRequest::SyncRepository { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.sync_repository(request)?)
        }
        ServiceRequest::EnsureThumbnail { request } => {
            to_value(repository_state.ensure_thumbnail(request)?)
        }
        ServiceRequest::UndoLastRevision { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.undo_last_revision(request)?)
        }
        ServiceRequest::RedoLastRevision { request } => {
            let _guard = write_lock
                .lock()
                .map_err(|_| "service write lock poisoned".to_string())?;
            to_value(repository_state.redo_last_revision(request)?)
        }
        ServiceRequest::ListPlugins => to_value(repository_state.list_plugins()?),
        ServiceRequest::GetCacheSnapshot => to_value(repository_state.get_cache_snapshot()?),
        ServiceRequest::GetApiDesignSnapshot => {
            to_value(repository_state.get_api_design_snapshot()?)
        }
    }
}

fn sync_watched_paths(
    repository_state: &Arc<RepositoryState>,
    watcher_handle: &Arc<Mutex<RepositoryWatcher>>,
) -> Result<(), String> {
    let repositories = repository_state.list_repositories()?;
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
        child: Arc::new(ServiceProcessHandle::new(None)),
    };
    let value: String = bridge.invoke(&serde_json::json!({ "command": "ping" }))?;
    if value == "pong" {
        Ok(())
    } else {
        Err("service ping failed".to_string())
    }
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

    #[test]
    fn cloned_bridge_drop_keeps_shared_service_handle_alive() {
        SERVICE_HANDLE_SHUTDOWN_COUNT.store(0, Ordering::SeqCst);
        let handle = Arc::new(ServiceProcessHandle::new(None));
        let bridge = ServiceBridge {
            addr: "127.0.0.1:0".to_string(),
            child: handle.clone(),
        };

        let clone = bridge.clone();
        drop(clone);

        assert_eq!(Arc::strong_count(&handle), 2);
        assert_eq!(SERVICE_HANDLE_SHUTDOWN_COUNT.load(Ordering::SeqCst), 0);
        assert!(bridge.child.child.lock().expect("child lock should be healthy").is_none());
    }

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
