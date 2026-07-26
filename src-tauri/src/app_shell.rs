//! Desktop app-shell bootstrap and lifecycle glue for the Tauri View layer.

use crate::services::logging::{init_app_logger, write_log};
use crate::services::{
    mutsuki_host, mutsuki_runner::build_momo_long_task_runner, runtime::RepositoryRuntime,
};
use crate::{
    services::repository::{SystemLogLocationInput, SystemLogWriteRequest},
    viewmodels::{
        FileBrowserViewModel, MutsukiTaskViewModel, PluginViewModel,
        RepositoryInteractionViewModel, RepositoryManagementViewModel, RepositoryQueryViewModel,
        SystemViewModel,
    },
    window_state,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
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
    let host_runtime = runtime.clone();
    let app_runtime = runtime.clone();
    let runner_generation = Arc::new(AtomicU64::new(1));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_mutsuki::init_with_app(move |_| {
            let config = host_runtime.mutsuki_config()?;
            let runner_runtime = host_runtime.clone();
            let runner_generation = runner_generation.clone();
            Ok(mutsuki_tauri_host::MutsukiTauriHostBuilder::new()
                .config(config)
                .runner_factory(move || {
                    let generation = runner_generation.fetch_add(1, Ordering::Relaxed);
                    build_momo_long_task_runner(runner_runtime.clone(), generation)
                }))
        }))
        .manage(window_state::MainWindowStateCache::default())
        .setup(move |app| {
            let service_root = std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(".service-data");
            let logger = init_app_logger(service_root)?;
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
            let host = app
                .try_state::<Arc<mutsuki_tauri_host::MutsukiTauriHost>>()
                .ok_or_else(|| "Mutsuki Host 未完成启动。".to_string())?
                .inner()
                .clone();
            mutsuki_host::install_host(host)?;
            let file_browser = FileBrowserViewModel::new(runtime.clone());
            let plugin_vm = PluginViewModel::new(runtime.clone());
            let repository_interaction = RepositoryInteractionViewModel::new(runtime.clone());
            let repository_query = RepositoryQueryViewModel::new(runtime.clone());
            let repository_management = RepositoryManagementViewModel::new(runtime.clone());
            let mutsuki_tasks = MutsukiTaskViewModel;
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
