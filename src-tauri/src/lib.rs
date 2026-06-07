use tauri::{utils::config::Color, Manager, WindowEvent};

const MAIN_WINDOW_LABEL: &str = "main";
const BG: Color = Color(0x18, 0x18, 0x18, 0xFF);

mod repository_service;
pub mod service_process;
mod window_state;

use repository_service::{
    FileBrowserRequest, FileCreateRequest, FileDeleteRequest, FileRenameRequest, MetadataUpdateRequest,
    RepositoryFolderRequest, RepositoryMutationRequest, RevisionActionRequest, SearchRequest, SyncRequest,
};
use service_process::ServiceBridge;

#[tauri::command]
fn ping(bridge: tauri::State<'_, ServiceBridge>) -> Result<String, String> {
    bridge.invoke(&serde_json::json!({ "command": "ping" }))
}

#[tauri::command]
fn list_repositories(bridge: tauri::State<'_, ServiceBridge>) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "listRepositories" }))
}

#[tauri::command]
fn get_repository_snapshot(
    repo_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "getRepositorySnapshot", "repoId": repo_id }))
}

#[tauri::command]
fn get_asset_detail(
    repo_id: String,
    asset_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({
        "command": "getAssetDetail",
        "repoId": repo_id,
        "assetId": asset_id
    }))
}

#[tauri::command]
fn search_assets(
    request: SearchRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "searchAssets", "request": request }))
}

#[tauri::command]
fn update_asset_metadata(
    request: MetadataUpdateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "updateAssetMetadata", "request": request }))
}

#[tauri::command]
fn get_file_browser(
    request: FileBrowserRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "getFileBrowser", "request": request }))
}

#[tauri::command]
fn create_directory(
    request: FileCreateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "createDirectory", "request": request }))
}

#[tauri::command]
fn create_file(
    request: FileCreateRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "createFile", "request": request }))
}

#[tauri::command]
fn rename_entry(
    request: FileRenameRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "renameEntry", "request": request }))
}

#[tauri::command]
fn delete_entry(
    request: FileDeleteRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "deleteEntry", "request": request }))
}

#[tauri::command]
fn create_repository(
    request: RepositoryMutationRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "createRepository", "request": request }))
}

#[tauri::command]
fn import_repository(
    request: RepositoryMutationRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "importRepository", "request": request }))
}

#[tauri::command]
fn attach_repository_folder(
    request: RepositoryFolderRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "attachRepositoryFolder", "request": request }))
}

#[tauri::command]
fn delete_repository(
    repo_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<(), String> {
    let _: serde_json::Value =
        bridge.invoke(&serde_json::json!({ "command": "deleteRepository", "repoId": repo_id }))?;
    Ok(())
}

#[tauri::command]
fn export_repository(
    repo_id: String,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "exportRepository", "repoId": repo_id }))
}

#[tauri::command]
fn sync_repository(
    request: SyncRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "syncRepository", "request": request }))
}

#[tauri::command]
fn undo_last_revision(
    request: RevisionActionRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "undoLastRevision", "request": request }))
}

#[tauri::command]
fn redo_last_revision(
    request: RevisionActionRequest,
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "redoLastRevision", "request": request }))
}

#[tauri::command]
fn list_plugins(bridge: tauri::State<'_, ServiceBridge>) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "listPlugins" }))
}

#[tauri::command]
fn get_cache_snapshot(bridge: tauri::State<'_, ServiceBridge>) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "getCacheSnapshot" }))
}

#[tauri::command]
fn get_api_design_snapshot(
    bridge: tauri::State<'_, ServiceBridge>,
) -> Result<serde_json::Value, String> {
    bridge.invoke(&serde_json::json!({ "command": "getApiDesignSnapshot" }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let bridge = ServiceBridge::start(app.handle())?;
            app.manage(bridge);

            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.set_background_color(Some(BG));
                if let Some(state) = window_state::load_main_window_state(app.handle()) {
                    window_state::restore_main_window_state(&window, state);
                }
                let _ = window.show();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            if matches!(event, WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) {
                if let Some(webview_window) = window.get_webview_window(MAIN_WINDOW_LABEL) {
                    window_state::persist_main_window_state(&window.app_handle(), &webview_window);
                }
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
            create_directory,
            create_file,
            rename_entry,
            delete_entry,
            create_repository,
            import_repository,
            attach_repository_folder,
            delete_repository,
            export_repository,
            sync_repository,
            undo_last_revision,
            redo_last_revision,
            list_plugins,
            get_cache_snapshot,
            get_api_design_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
