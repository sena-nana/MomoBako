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

mod repository_runtime;
mod repository_service;
mod window_state;

use repository_runtime::RepositoryRuntime;
use repository_service::{
    ApiDesignSnapshot, AssetDetail, CacheSnapshot, FileBrowserRequest, FileBrowserSnapshot,
    FileCopyRequest, FileCreateRequest, FileDeleteRequest, FileImportRequest,
    FilePreviewSourceResponse, FileReadRequest, FileRenameRequest, HardlinkCandidateResponse,
    HardlinkConfirmRequest, HardlinkConfirmResponse, MetadataUpdateRequest, MetadataUpdateResponse,
    PluginEnabledRequest, PluginInstallRequest, PluginManifest, PluginMutationResponse,
    RepositoryExportRequest, RepositoryExportResponse, RepositoryFolderRequest,
    RepositoryMutationRequest, RepositoryMutationResponse, RepositorySnapshot, RepositorySummary,
    RevisionActionRequest, RevisionActionResponse, SearchRequest, SearchResponse, SyncRequest,
    SyncResult, ThumbnailRequest, ThumbnailResponse, TrashMutationRequest,
};

#[tauri::command]
async fn ping() -> Result<String, String> {
    Ok("pong".to_string())
}

#[tauri::command]
async fn list_repositories(
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<Vec<RepositorySummary>, String> {
    runtime.run_read(|state| state.list_repositories()).await
}

#[tauri::command]
async fn get_repository_snapshot(
    repo_id: String,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RepositorySnapshot, String> {
    runtime
        .run_read(move |state| state.load_snapshot(&repo_id))
        .await
}

#[tauri::command]
async fn get_asset_detail(
    repo_id: String,
    asset_id: String,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<AssetDetail, String> {
    runtime
        .run_read(move |state| state.load_asset_detail(&repo_id, &asset_id))
        .await
}

#[tauri::command]
async fn search_assets(
    request: SearchRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<SearchResponse, String> {
    runtime
        .run_read(move |state| state.search_assets(request))
        .await
}

#[tauri::command]
async fn update_asset_metadata(
    request: MetadataUpdateRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<MetadataUpdateResponse, String> {
    runtime
        .run_write(move |state| state.update_asset_metadata(request))
        .await
}

#[tauri::command]
async fn get_file_browser(
    request: FileBrowserRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_read(move |state| state.load_file_browser(request))
        .await
}

#[tauri::command]
async fn read_file(
    request: FileReadRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<Vec<u8>, String> {
    runtime
        .run_read(move |state| state.read_file(request))
        .await
}

#[tauri::command]
async fn prepare_preview_file_source(
    request: FileReadRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FilePreviewSourceResponse, String> {
    let mut response = runtime
        .run_read(move |state| state.prepare_preview_file_source(request))
        .await?;
    response.source_url = Some(runtime.preview_source_url(&response.token));
    Ok(response)
}

#[tauri::command]
async fn create_directory(
    request: FileCreateRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.create_directory(request))
        .await
}

#[tauri::command]
async fn create_file(
    request: FileCreateRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.create_file(request))
        .await
}

#[tauri::command]
async fn import_entries(
    request: FileImportRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.import_entries(request))
        .await
}

#[tauri::command]
async fn copy_entries(
    request: FileCopyRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.copy_entries(request))
        .await
}

#[tauri::command]
async fn rename_entry(
    request: FileRenameRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.rename_entry(request))
        .await
}

#[tauri::command]
async fn delete_entry(
    request: FileDeleteRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.delete_entry(request))
        .await
}

#[tauri::command]
async fn mutate_trash(
    request: TrashMutationRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<FileBrowserSnapshot, String> {
    runtime
        .run_write(move |state| state.mutate_trash(request))
        .await
}

#[tauri::command]
async fn create_repository(
    request: RepositoryMutationRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RepositoryMutationResponse, String> {
    runtime
        .run_repository_collection_write(move |state| state.create_repository(request))
        .await
}

#[tauri::command]
async fn import_repository(
    request: RepositoryMutationRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RepositoryMutationResponse, String> {
    runtime
        .run_repository_collection_write(move |state| state.import_repository(request))
        .await
}

#[tauri::command]
async fn attach_repository_folder(
    request: RepositoryFolderRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RepositoryMutationResponse, String> {
    runtime
        .run_repository_collection_write(move |state| state.attach_repository_folder(request))
        .await
}

#[tauri::command]
async fn delete_repository(
    repo_id: String,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<(), String> {
    runtime
        .run_repository_collection_write(move |state| state.delete_repository(&repo_id))
        .await
}

#[tauri::command]
async fn export_repository(
    request: RepositoryExportRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RepositoryExportResponse, String> {
    runtime
        .run_write(move |state| state.export_repository(request))
        .await
}

#[tauri::command]
async fn sync_repository(
    request: SyncRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<SyncResult, String> {
    runtime
        .run_write(move |state| state.sync_repository(request))
        .await
}

#[tauri::command]
async fn list_hardlink_candidates(
    repo_id: String,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<HardlinkCandidateResponse, String> {
    runtime
        .run_read(move |state| state.list_hardlink_candidates(&repo_id))
        .await
}

#[tauri::command]
async fn confirm_hardlink_candidate(
    request: HardlinkConfirmRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<HardlinkConfirmResponse, String> {
    runtime
        .run_write(move |state| state.confirm_hardlink_candidate(request))
        .await
}

#[tauri::command]
async fn ensure_thumbnail(
    request: ThumbnailRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<ThumbnailResponse, String> {
    runtime
        .run_write(move |state| state.ensure_thumbnail(request))
        .await
}

#[tauri::command]
async fn undo_last_revision(
    request: RevisionActionRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RevisionActionResponse, String> {
    runtime
        .run_write(move |state| state.undo_last_revision(request))
        .await
}

#[tauri::command]
async fn redo_last_revision(
    request: RevisionActionRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<RevisionActionResponse, String> {
    runtime
        .run_write(move |state| state.redo_last_revision(request))
        .await
}

#[tauri::command]
async fn list_plugins(
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<Vec<PluginManifest>, String> {
    runtime.run_read(|state| state.list_plugins()).await
}

#[tauri::command]
async fn set_plugin_enabled(
    request: PluginEnabledRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<PluginMutationResponse, String> {
    runtime
        .run_write(move |state| state.set_plugin_enabled(request))
        .await
}

#[tauri::command]
async fn delete_plugin(
    plugin_id: String,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<PluginMutationResponse, String> {
    runtime
        .run_write(move |state| state.delete_plugin(plugin_id))
        .await
}

#[tauri::command]
async fn install_plugin_from_archive(
    request: PluginInstallRequest,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<PluginMutationResponse, String> {
    runtime
        .run_write(move |state| state.install_plugin_from_archive(request))
        .await
}

#[tauri::command]
async fn get_cache_snapshot(
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<CacheSnapshot, String> {
    runtime.run_read(|state| state.get_cache_snapshot()).await
}

#[tauri::command]
async fn get_api_design_snapshot(
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<ApiDesignSnapshot, String> {
    runtime
        .run_read(|state| state.get_api_design_snapshot())
        .await
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn persist_main_window_state(app: &AppHandle) {
    let cache = app.state::<window_state::MainWindowStateCache>();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window_state::persist_main_window_state(app, &cache, &window);
    } else {
        window_state::persist_cached_main_window_state(app, &cache);
    }
}

fn quit_app(app: &AppHandle) {
    persist_main_window_state(app);
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
            let resource_dir = app.path().resource_dir().ok();
            let runtime = RepositoryRuntime::start_with_resource_dir(resource_dir)?;
            app.manage(runtime);
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
                WindowEvent::CloseRequested { .. } => {
                    quit_app(&app_handle);
                }
                WindowEvent::Destroyed => {
                    persist_main_window_state(&app_handle);
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
            prepare_preview_file_source,
            create_directory,
            create_file,
            import_entries,
            copy_entries,
            rename_entry,
            delete_entry,
            mutate_trash,
            create_repository,
            import_repository,
            attach_repository_folder,
            delete_repository,
            export_repository,
            sync_repository,
            list_hardlink_candidates,
            confirm_hardlink_candidate,
            ensure_thumbnail,
            undo_last_revision,
            redo_last_revision,
            list_plugins,
            set_plugin_enabled,
            delete_plugin,
            install_plugin_from_archive,
            get_cache_snapshot,
            get_api_design_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
