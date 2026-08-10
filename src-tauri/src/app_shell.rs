//! Desktop app-shell bootstrap and lifecycle glue for the Tauri View layer.

use crate::services::logging::{init_app_logger, write_log};
use crate::services::{mutsuki_host, mutsuki_runner::MomoTaskRuntime, runtime::RepositoryRuntime};
use crate::{
    services::repository::{SystemLogLocationInput, SystemLogWriteRequest},
    viewmodels::{
        FileBrowserViewModel, MutsukiTaskViewModel, PluginViewModel,
        RepositoryInteractionViewModel, RepositoryManagementViewModel, RepositoryQueryViewModel,
        SystemViewModel,
    },
    window_state,
};
use std::{collections::HashSet, fs, path::Path, sync::Arc};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::Color,
    AppHandle, Builder, Manager, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_ID: &str = "tray-open";
const TRAY_QUIT_ID: &str = "tray-quit";
const BG: Color = Color(0x18, 0x18, 0x18, 0xFF);

/// Applies desktop-shell plugins and lifecycle hooks before command registration.
pub fn builder(runtime: RepositoryRuntime) -> Builder<tauri::Wry> {
    let app_runtime = runtime.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(window_state::MainWindowStateCache::default())
        .setup(move |app| {
            let service_root = app_runtime.service_root();
            let logger = init_app_logger(service_root.clone())?;
            logger.set_app_handle(app.handle().clone())?;
            let _ = write_log(SystemLogWriteRequest {
                level: "info".to_string(),
                category: "app.lifecycle".to_string(),
                action: "startup".to_string(),
                message: "MomoBako 桌面端启动。".to_string(),
                context: None,
                repo_id: None,
                plugin_id: None,
                source_kind: Some("host".to_string()),
                source_label: Some("MomoBako".to_string()),
                location: Some(SystemLogLocationInput {
                    module_path: Some(module_path!().to_string()),
                    file: Some(file!().to_string()),
                    line: Some(line!()),
                }),
            });
            let runtime = app_runtime.clone();
            runtime.set_app_handle(app.handle().clone())?;
            stage_bundled_plugins(app.handle(), &service_root)?;
            let task_runtime = Arc::new(MomoTaskRuntime::new(runtime.clone()));
            let plugin_runtime = Arc::new(mutsuki_host::MomoPluginRuntime::new(
                runtime.clone(),
                task_runtime.clone(),
            ));
            plugin_runtime.reload()?;
            mutsuki_host::install_host(plugin_runtime.clone())?;
            let file_browser = FileBrowserViewModel::new(runtime.clone());
            let plugin_vm = PluginViewModel::new(runtime.clone());
            let repository_interaction = RepositoryInteractionViewModel::new(runtime.clone());
            let repository_query = RepositoryQueryViewModel::new(runtime.clone());
            let repository_management = RepositoryManagementViewModel::new(runtime.clone());
            let mutsuki_tasks = MutsukiTaskViewModel::new(task_runtime);
            let system_vm = SystemViewModel::new(runtime.clone());

            allow_thumbnail_asset_roots(
                app.handle(),
                tauri::async_runtime::block_on(runtime.repository_thumbnail_roots())?,
            )?;
            app.manage(runtime);
            app.manage(file_browser);
            app.manage(plugin_vm);
            app.manage(repository_interaction);
            app.manage(repository_query);
            app.manage(repository_management);
            app.manage(plugin_runtime);
            app.manage(mutsuki_tasks);
            app.manage(system_vm);
            setup_tray(app.handle())?;
            restore_main_window(app);
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
}

/// 将安装包资源中的官方插件同步到宿主拥有的 builtin 目录。
fn stage_bundled_plugins(app: &AppHandle, service_root: &Path) -> Result<(), String> {
    let source_root = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?
        .join("plugins");
    if !source_root.is_dir() {
        return Ok(());
    }
    let target_root = service_root.join("plugins").join("builtin");
    fs::create_dir_all(&target_root).map_err(|error| error.to_string())?;
    let mut bundled_names = HashSet::new();
    for entry in fs::read_dir(&source_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        if source_path.extension().and_then(|value| value.to_str()) != Some("momoplug") {
            continue;
        }
        let file_name = entry.file_name();
        bundled_names.insert(file_name.clone());
        let target_path = target_root.join(&file_name);
        let temporary_path = target_root.join(format!("{}.new", file_name.to_string_lossy()));
        fs::copy(&source_path, &temporary_path).map_err(|error| error.to_string())?;
        if target_path.exists() {
            fs::remove_file(&target_path).map_err(|error| error.to_string())?;
        }
        fs::rename(&temporary_path, &target_path).map_err(|error| error.to_string())?;
    }
    for entry in fs::read_dir(&target_root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path().extension().and_then(|value| value.to_str()) == Some("momoplug")
            && !bundled_names.contains(&entry.file_name())
        {
            fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Reuses the existing asset-scope behavior for repository thumbnail roots.
pub fn allow_thumbnail_asset_roots(
    app: &AppHandle,
    paths: Vec<std::path::PathBuf>,
) -> Result<(), String> {
    for path in paths {
        app.asset_protocol_scope()
            .allow_directory(path, true)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn restore_main_window(app: &mut tauri::App) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.set_background_color(Some(BG));
        if let Some(state) = window_state::load_main_window_state(app.handle()) {
            window_state::restore_main_window_state(&window, state);
        }
        let _ = window.show();
        let cache = app.state::<window_state::MainWindowStateCache>();
        window_state::remember_main_window_state(&cache, &window);
    }
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
    if let Some(runtime) = app.try_state::<RepositoryRuntime>() {
        runtime.shutdown_helpers();
    }
    let _ = write_log(SystemLogWriteRequest {
        level: "info".to_string(),
        category: "app.lifecycle".to_string(),
        action: "shutdown".to_string(),
        message: "MomoBako 桌面端退出。".to_string(),
        context: None,
        repo_id: None,
        plugin_id: None,
        source_kind: Some("host".to_string()),
        source_label: Some("MomoBako".to_string()),
        location: Some(SystemLogLocationInput {
            module_path: Some(module_path!().to_string()),
            file: Some(file!().to_string()),
            line: Some(line!()),
        }),
    });
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
