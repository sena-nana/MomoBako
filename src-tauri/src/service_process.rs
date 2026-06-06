use crate::repository_service::{
    FileBrowserRequest, FileCreateRequest, FileDeleteRequest, FileRenameRequest,
    MetadataUpdateRequest, RepositoryFolderRequest, RepositoryMutationRequest, RepositoryState,
    RevisionActionRequest, SearchRequest, SyncRequest,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc::channel, Arc, Mutex},
    thread,
};
use tauri::AppHandle;
use tiny_http::{Method, Response, Server, StatusCode};

const DEFAULT_SERVICE_ADDR: &str = "127.0.0.1:49321";

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
    SearchAssets { request: SearchRequest },
    UpdateAssetMetadata { request: MetadataUpdateRequest },
    GetFileBrowser { request: FileBrowserRequest },
    CreateDirectory { request: FileCreateRequest },
    CreateFile { request: FileCreateRequest },
    RenameEntry { request: FileRenameRequest },
    DeleteEntry { request: FileDeleteRequest },
    CreateRepository { request: RepositoryMutationRequest },
    ImportRepository { request: RepositoryMutationRequest },
    AttachRepositoryFolder { request: RepositoryFolderRequest },
    DeleteRepository {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
    },
    ExportRepository {
        #[serde(rename = "repoId", alias = "repo_id")]
        repo_id: String,
    },
    SyncRepository { request: SyncRequest },
    UndoLastRevision { request: RevisionActionRequest },
    RedoLastRevision { request: RevisionActionRequest },
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
    pub fn start(_app: &AppHandle) -> Result<Self, String> {
        let addr = DEFAULT_SERVICE_ADDR.to_string();
        if ping_service(&addr).is_ok() {
            return Ok(Self { addr });
        }

        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut command = Command::new(executable);
        command.arg("--service-mode").arg(&addr);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        command.spawn().map_err(|error| format!("failed to start service process: {error}"))?;

        for _ in 0..40 {
            if ping_service(&addr).is_ok() {
                return Ok(Self { addr });
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Err("service process did not become ready".to_string())
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
        (&client).write_all(http_request.as_bytes()).map_err(|error| error.to_string())?;
        let mut response_raw = String::new();
        let mut reader = std::io::BufReader::new(client);
        reader.read_to_string(&mut response_raw).map_err(|error| error.to_string())?;

        let body = response_raw
            .split("\r\n\r\n")
            .nth(1)
            .ok_or_else(|| "invalid service response".to_string())?;
        let response: ServiceResponse<T> = serde_json::from_str(body).map_err(|error| error.to_string())?;
        if response.ok {
            response.data.ok_or_else(|| "missing service payload".to_string())
        } else {
            Err(response.error.unwrap_or_else(|| "service request failed".to_string()))
        }
    }
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
    let repository_state = Arc::new(Mutex::new(RepositoryState::from_root(root)));
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
        if let Some(repository) = repositories.iter().find(|repo| normalized_path.starts_with(&normalize_path(Path::new(&repo.path)))) {
            let _ = state.sync_repository(SyncRequest {
                repo_id: repository.repo_id.clone(),
            });
        }
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
