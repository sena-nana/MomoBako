use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::Color,
    AppHandle, Manager, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_ID: &str = "tray-open";
const TRAY_QUIT_ID: &str = "tray-quit";
const BG: Color = Color(0x18, 0x18, 0x18, 0xFF);

mod repository_service;
pub mod service_process;
mod window_state;

use repository_service::{
    FileBrowserRequest, FileCreateRequest, FileDeleteRequest, FileImportRequest, FileReadRequest,
    FileRenameRequest, MetadataUpdateRequest, RepositoryExportRequest, RepositoryFolderRequest,
    RepositoryMutationRequest, RevisionActionRequest, SearchRequest, SyncRequest, ThumbnailRequest,
    TrashMutationRequest,
};
use service_process::ServiceBridge;

async fn invoke_service<T, S>(
    bridge: tauri::State<'_, ServiceBridge>,
    request: S,
) -> Result<T, String>
where
    T: DeserializeOwned + Send + 'static,
    S: Serialize + Send + 'static,
{
    let bridge = bridge.inner().clone();
    tauri::async_runtime::spawn_blocking(move || bridge.invoke(&request))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn ping(bridge: tauri::State<'_, ServiceBridge>) -> Result<String, String> {
    invoke_service(bridge, serde_json::json!({ "command": "ping" })).await
}

#[tauri::command]
async fn list_repositories(
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(bridge, serde_json::json!({ "command": "listRepositories" })).await
}

#[tauri::command]
async fn get_repository_snapshot(
    repo_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "getRepositorySnapshot", "repoId": repo_id }),
    )
    .await
}

#[tauri::command]
async fn get_asset_detail(
    repo_id: String,
    asset_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({
            "command": "getAssetDetail",
            "repoId": repo_id,
            "assetId": asset_id
        }),
    )
    .await
}

#[tauri::command]
async fn search_assets(
    request: SearchRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "searchAssets", "request": request }),
    )
    .await
}

#[tauri::command]
async fn update_asset_metadata(
    request: MetadataUpdateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "updateAssetMetadata", "request": request }),
    )
    .await
}

#[tauri::command]
async fn get_file_browser(
    request: FileBrowserRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "getFileBrowser", "request": request }),
    )
    .await
}

#[tauri::command]
async fn read_file(
    request: FileReadRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<Vec<u8>, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "readFile", "request": request }),
    )
    .await
}

#[tauri::command]
async fn create_directory(
    request: FileCreateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "createDirectory", "request": request }),
    )
    .await
}

#[tauri::command]
async fn create_file(
    request: FileCreateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "createFile", "request": request }),
    )
    .await
}

#[tauri::command]
async fn import_entries(
    request: FileImportRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "importEntries", "request": request }),
    )
    .await
}

#[tauri::command]
async fn rename_entry(
    request: FileRenameRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "renameEntry", "request": request }),
    )
    .await
}

#[tauri::command]
async fn delete_entry(
    request: FileDeleteRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "deleteEntry", "request": request }),
    )
    .await
}

#[tauri::command]
async fn mutate_trash(
    request: TrashMutationRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "mutateTrash", "request": request }),
    )
    .await
}

#[tauri::command]
async fn create_repository(
    request: RepositoryMutationRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "createRepository", "request": request }),
    )
    .await
}

#[tauri::command]
async fn import_repository(
    request: RepositoryMutationRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "importRepository", "request": request }),
    )
    .await
}

#[tauri::command]
async fn attach_repository_folder(
    request: RepositoryFolderRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "attachRepositoryFolder", "request": request }),
    )
    .await
}

#[tauri::command]
async fn delete_repository(
    repo_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<(), String> {
    let _: serde_json::Value = invoke_service(
        bridge,
        serde_json::json!({ "command": "deleteRepository", "repoId": repo_id }),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn export_repository(
    request: RepositoryExportRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "exportRepository", "request": request }),
    )
    .await
}

#[tauri::command]
async fn sync_repository(
    request: SyncRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "syncRepository", "request": request }),
    )
    .await
}

#[tauri::command]
async fn ensure_thumbnail(
    request: ThumbnailRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "ensureThumbnail", "request": request }),
    )
    .await
}

#[tauri::command]
async fn undo_last_revision(
    request: RevisionActionRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "undoLastRevision", "request": request }),
    )
    .await
}

#[tauri::command]
async fn redo_last_revision(
    request: RevisionActionRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "redoLastRevision", "request": request }),
    )
    .await
}

#[tauri::command]
async fn list_plugins(
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(bridge, serde_json::json!({ "command": "listPlugins" })).await
}

#[tauri::command]
async fn get_cache_snapshot(
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(bridge, serde_json::json!({ "command": "getCacheSnapshot" })).await
}

#[tauri::command]
async fn get_api_design_snapshot(
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    invoke_service(
        bridge,
        serde_json::json!({ "command": "getApiDesignSnapshot" }),
    )
    .await
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn quit_app(app: &AppHandle) {
    let cache = app.state::<window_state::MainWindowStateCache>();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window_state::persist_main_window_state(app, &cache, &window);
    } else {
        window_state::persist_cached_main_window_state(app, &cache);
    }
    if let Some(bridge) = app.try_state::<ServiceBridge>() {
        bridge.shutdown();
    }
    app.exit(0);
}

fn setup_tray(app: &AppHandle) -> Result<(), tauri::Error> {
    let open = MenuItem::with_id(app, TRAY_OPEN_ID, "打开 MomoBako", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app.default_window_icon().cloned();
    let tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("MomoBako")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_ID => show_main_window(app),
            TRAY_QUIT_ID => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = icon {
        tray.icon(icon).build(app)?;
    } else {
        tray.build(app)?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(window_state::MainWindowStateCache::default())
        .setup(|app| {
            let bridge = ServiceBridge::start(app.handle())?;
            app.manage(bridge);
            setup_tray(app.handle())?;

            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.set_background_color(Some(BG));
                if let Some(state) = window_state::load_main_window_state(app.handle()) {
                    window_state::restore_main_window_state(&window, state);
                }
                let _ = window.show();
                let cache = app.state::<window_state::MainWindowStateCache>();
                window_state::remember_main_window_state(&cache, &window);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            let app_handle = window.app_handle();
            let cache = app_handle.state::<window_state::MainWindowStateCache>();
            match event {
                WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. } => {
                    if let Some(webview_window) = window.get_webview_window(MAIN_WINDOW_LABEL) {
                        window_state::remember_main_window_state(&cache, &webview_window);
                    }
                }
                WindowEvent::CloseRequested { api, .. } => {
                    if let Some(webview_window) = window.get_webview_window(MAIN_WINDOW_LABEL) {
                        window_state::persist_main_window_state(
                            &app_handle,
                            &cache,
                            &webview_window,
                        );
                    } else {
                        window_state::persist_cached_main_window_state(&app_handle, &cache);
                    }
                    api.prevent_close();
                    let _ = window.hide();
                }
                WindowEvent::Destroyed => {
                    if let Some(webview_window) = window.get_webview_window(MAIN_WINDOW_LABEL) {
                        window_state::persist_main_window_state(
                            &app_handle,
                            &cache,
                            &webview_window,
                        );
                    } else {
                        window_state::persist_cached_main_window_state(&app_handle, &cache);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            list_repositories,
            get_repository_snapshot,
            get_asset_detail,
            search_assets,
            update_asset_metadata,
            get_file_browser,
            read_file,
            create_directory,
            create_file,
            import_entries,
            rename_entry,
            delete_entry,
            mutate_trash,
            create_repository,
            import_repository,
            attach_repository_folder,
            delete_repository,
            export_repository,
            sync_repository,
            ensure_thumbnail,
            undo_last_revision,
            redo_last_revision,
            list_plugins,
            get_cache_snapshot,
            get_api_design_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
