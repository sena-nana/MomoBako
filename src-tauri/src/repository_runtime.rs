use crate::repository_service::{RepositoryState, SyncRequest};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, Mutex},
    thread,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const PREVIEW_HOST: &str = "127.0.0.1";
const PREVIEW_PATH_PREFIX: &str = "/preview/";

#[derive(Clone)]
pub struct RepositoryRuntime {
    repository_state: Arc<RepositoryState>,
    watcher_handle: Arc<Mutex<RepositoryWatcher>>,
    write_lock: Arc<Mutex<()>>,
    preview_addr: String,
}

#[derive(Debug)]
struct RepositoryWatcher {
    watcher: RecommendedWatcher,
    watched_paths: BTreeSet<PathBuf>,
}

impl RepositoryRuntime {
    pub fn start() -> Result<Self, String> {
        let root = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(".service-data");
        let repository_state = Arc::new(RepositoryState::from_root(root));
        let write_lock = Arc::new(Mutex::new(()));
        repository_state.ensure_initialized()?;
        let watcher_handle =
            RepositoryWatcher::start(repository_state.clone(), write_lock.clone())?;
        let preview_addr = start_preview_server(repository_state.clone())?;

        Ok(Self {
            repository_state,
            watcher_handle,
            write_lock,
            preview_addr,
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

    pub fn preview_source_url(&self, token: &str) -> String {
        format!("http://{}/preview/{token}", self.preview_addr)
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

fn handle_preview_request(request: Request, repository_state: &Arc<RepositoryState>) {
    match preview_token_from_url(request.url()) {
        Some(token) if request.method() == &Method::Get || request.method() == &Method::Head => {
            let response =
                repository_state
                    .open_preview_file_source(token)
                    .map(|(file, media_type)| {
                        Response::from_file(file)
                            .with_header(header("Content-Type", &media_type))
                            .with_header(header("Cache-Control", "no-store"))
                            .with_header(header("Access-Control-Allow-Origin", "*"))
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
    use crate::repository_service::{FileReadRequest, RepositoryMutationRequest};
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
    fn preview_server_serves_registered_source_file() {
        let root = unique_temp_dir("preview-server");
        let service_root = root.join("state");
        let repo_root = root.join("repo");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        fs::write(repo_root.join("model.glb"), b"glb-body")
            .expect("preview source should be written");

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
}
