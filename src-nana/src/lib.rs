//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-backend` 收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::{DocumentId, FrameworkError};
use nana_ui::{
    ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication, HostTexture,
    ResourcePackOptions, RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::ApplicationPaths;

pub mod shell;
mod app_icon;
mod app_paths;
mod app_dispatch;
mod plugin_host;
mod files_dispatch;
mod inspect_dispatch;
mod player_dispatch;
mod admin_dispatch;
mod download_dispatch;
mod download_staging;
mod sidebar_dispatch;
mod sync_dispatch;
mod task_guard;
mod task_watch;
mod window_host;
pub(crate) mod frame_clock;
mod host_bridge;
mod window_state;
mod drag_out;
mod tray;
use shell::{
    DeleteMode, ShellMessage, ShellPage, ShellView, ShellViewModel,
    WorkspaceEffect, display_mode_path, last_repository_path, mount_shell, sidebar_prefs_path,
};

pub mod capability;
pub mod host_api;
pub mod services;
pub mod theme_map;

/// Nana 宿主直接使用共享领域服务 crate；宿主代码经 `crate::backend` 接 ViewModel 和服务。
pub use momobako_backend as backend;
pub(crate) use shell::prepare_text as prepare_preview_text;
use backend::services::mutsuki_runner::{PROTOCOL_REPOSITORY_RELOCATE, PROTOCOL_REPOSITORY_SYNC};
use backend::services::repository::{
    RepositoryDeleteMode, RepositoryDeleteRequest, RepositoryRelocateRequest,
    SyncRequest,
};
pub mod settings;
mod thumbnail_cache;
mod thumbnail_host;
pub mod appearance;

use thumbnail_host::ThumbnailHost;

/// Nana 宿主应用状态，持有共享领域 Runtime 和原生壳层 ViewModel。
pub struct MomoBakoApplication {
    pub services: Option<services::NativeServices>,
    shell: ShellViewModel,
    repositories_load_scheduled: bool,
    preview_gpu: Option<NativePreviewGpu>,
    /// 缩略图纹理：等上传的像素和已经登记的纹理，见 `thumbnail_host.rs`。
    pub(crate) thumbnails: ThumbnailHost<HostTexture>,
    host: window_host::HostSession,
    /// 宿主报告的系统深浅色，「跟随系统」时用。
    system_appearance: Option<nana_ui_platform::SystemAppearance>,
    /// 已装进文档的外观。设置变了才重新安装。
    applied_appearance: Option<appearance::Appearance>,
    /// 宿主和文档当前用的主题，随外观同步更新。
    theme: std::sync::Arc<nana_ui::theme::CompiledTheme>,
    /// 窗口文档里常驻的壳层视图。`build` 挂上，`update` 和 `prepare` 同步。
    view: Option<ShellView>,
}

/// 已经登记在 `file-preview` 槽上的预览画面：纹理由宿主纹理表持有，这里记下是哪一张，同一张画面不再登记。
pub(crate) struct NativePreviewGpu {
    pub(crate) token: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Default for MomoBakoApplication {
    fn default() -> Self {
        Self {
            services: None,
            shell: ShellViewModel::default(),
            repositories_load_scheduled: false,
            preview_gpu: None,
            thumbnails: ThumbnailHost::default(),
            host: window_host::HostSession::default(),
            system_appearance: None,
            applied_appearance: None,
            theme: nana_ui::theme::builtin_theme_arc(nana_ui::theme::ThemeAppearance::Dark),
            view: None,
        }
    }
}

impl ApplicationState for MomoBakoApplication {
    type Message = ShellMessage;
    type Error = FrameworkError;

    fn initialize(context: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let services = match services::NativeServices::start() {
            Ok(services) => {
                let events = context.clone();
                services.pump_host_events(move |event| {
                    events.dispatch(ShellMessage::Host(shell::host_events::HostMessage::from_event(event)));
                });
                let tasks = context.clone();
                services.watch_tasks(move |active| tasks.dispatch(ShellMessage::TaskProgressLoaded(active)));
                Some(services)
            }
            Err(error) => {
                eprintln!("Nana 领域 Runtime 启动失败：{error}");
                None
            }
        };
        let mut shell = if services.is_some() {
            ShellViewModel::default()
        } else {
            // 不走验收种子：验收的启动失败页带着夹具仓库和日志，不能出现在真实窗口里。
            let mut shell = ShellViewModel::default();
            shell.page = ShellPage::Error;
            shell.workspace.startup.fail("领域服务启动失败，请检查服务目录和端口配置");
            shell
        };
        if let Some(services) = services.as_ref() {
            // 主题等应用设置启动时就要生效，不能等打开设置页才读。
            match services.load_settings() {
                Ok((settings, diagnostic)) => {
                    if let Some(diagnostic) = diagnostic {
                        eprintln!("Nana {diagnostic}");
                    }
                    shell.settings = settings;
                }
                Err(error) => eprintln!("Nana 启动时读取应用设置失败，先用默认值：{error}"),
            }
        }
        shell.workspace.load_prefs_file(&sidebar_prefs_path());
        shell.workspace.load_last_repository_file(&last_repository_path());
        shell.files.load_display_mode_file(&display_mode_path());
        shell.player.load_default_files();
        shell.admin.load_default_file();
        let system_appearance = context.system_appearance();
        let theme = appearance::initial_theme(appearance::Appearance::from_shell(&shell, system_appearance));
        Ok(Self {
            services,
            shell,
            repositories_load_scheduled: false,
            preview_gpu: None,
            thumbnails: ThumbnailHost::default(),
            host: window_host::HostSession::default(),
            system_appearance,
            applied_appearance: None,
            theme,
            view: None,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        if self.services.is_some() && !self.repositories_load_scheduled {
            self.shell.workspace.prepare_initial_list();
        }
        match self.view.as_mut().filter(|view| view.owns(&window.document)) {
            Some(view) => view.sync(&mut window.document, &self.shell)?,
            None => self.view = Some(ShellView::mount(&mut window.document, &self.shell)?),
        }
        self.shell.surface_dirty = false;
        self.applied_appearance = None;
        appearance::sync(self, &mut window.document);
        if !self.repositories_load_scheduled && self.services.is_some() {
            dispatch_list_repositories(self, context, self.shell.workspace.list_generation);
            self.repositories_load_scheduled = true;
        }
        window_host::install_tray(self, context);
        Ok(())
    }

    fn theme(&self) -> std::sync::Arc<nana_ui::theme::CompiledTheme> {
        self.theme.clone()
    }

    fn prepare(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) {
        let step = self.host.frames.step(std::time::Instant::now());
        if crate::shell::poll_timers(&mut self.shell, step) {
            inspect_dispatch::dispatch_inspect_effects(self, context);
        }
        window_host::prepare_motion(&mut self.shell, self.view.as_mut(), window, step);
        self.host.frames.note_demand(window.demand);
        for request in sidebar_dispatch::dispatch_prepared_browses(&mut self.shell) {
            sidebar_dispatch::dispatch_browse_request(self, context, request);
        }
        thumbnail_host::publish_preview(self, window, context);
        self.publish_thumbnails(window, context);
        appearance::sync(self, &mut window.document);
    }

    fn update(
        &mut self,
        mut message: ShellMessage,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if let ShellMessage::ThumbnailPixels(frames) = &mut message {
            self.queue_thumbnail_frames(frames);
        }
        let Some((id, window)) = windows.iter_mut().next() else {
            return RuntimeProgramUpdate::default();
        };
        if let Some(window_commands) =
            shell::window_action_commands(&message, *id, context.geometry().maximized)
        {
            return RuntimeProgramUpdate {
                window_commands,
                ..RuntimeProgramUpdate::default()
            };
        }
        app_dispatch::dispatch_services(self, &message, context);
        self.shell.reduce(message);
        dispatch_workspace_effects(self, context);
        player_dispatch::dispatch_player_effects(self, context);
        admin_dispatch::dispatch_admin_effects(self, context);
        sidebar_dispatch::dispatch_sidebar_effects(self, context);
        sync_dispatch::dispatch_tree_sync_effects(self, context);
        files_dispatch::dispatch_files_effects(self, context);
        inspect_dispatch::dispatch_inspect_effects(self, context);
        window_host::after_update(self, context);
        let maximized = context.geometry().maximized;
        let window_commands = self.shell.input.take_platform_commands(*id, maximized);
        // 服务派发和宿主请求可能在归约以外写下失败，同步视图之前收进状态区。
        self.shell.observe_failures();
        if let Some(view) = self.view.as_mut() {
            if let Err(error) = view.sync(&mut window.document, &self.shell) {
                eprintln!("Nana 壳层同步失败：{error}");
                self.shell.page = ShellPage::Error;
                return RuntimeProgramUpdate { window_commands, ..RuntimeProgramUpdate::redraw(*id) };
            }
            self.shell.surface_dirty = false;
        }
        RuntimeProgramUpdate { window_commands, ..RuntimeProgramUpdate::redraw(*id) }
    }

    fn window_event(
        &mut self,
        event: &nana_ui_platform::WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        window_host::on_window_event(self, event, context)
    }

    /// 运行时路由完的每个输入。先记下按着的修饰键；Escape 没被控件处理掉、ViewModel 里还有能关的层时，
    /// 发一条消息关掉最上面一层，和别的消息一样经 `update` 归约。
    fn input_event(
        &mut self,
        _id: nana_ui_platform::WindowId,
        input: nana_ui::RoutedInput<'_>,
        _windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        window_host::note_modifiers(&mut self.shell, &input.event.payload);
        if let Some(message) = window_host::escape_message(&self.shell, &input.event.payload, input.disposition.prevent_default) {
            context.dispatch(message);
        }
        Ok(RuntimeProgramUpdate::default())
    }

    /// 系统或标题栏请求关窗：按关闭设置回答，确认和收到托盘时不带关闭命令。
    fn close_requested(
        &mut self,
        id: nana_ui_platform::WindowId,
        _windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        window_host::answer_close(self, id, context)
    }
}

/// 读资源库列表（首屏启动第 1 步和刷新）。服务没起来、任务排不进去或 panic 时按失败交回，启动停在第 1 步给「重试」。
fn dispatch_list_repositories(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, generation: u64) {
    let failed = move |error| ShellMessage::WorkspaceListLoaded { generation, result: Err(error) };
    task_guard::spawn_service(app.services.as_ref(), context, "资源库列表", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || ShellMessage::WorkspaceListLoaded { generation, result: executor.block_on(query.list_repositories()) }
    });
}

/// 执行工作台归约留下的副作用。服务都经 `task_guard` 派发：服务没起来、任务排不进去或 panic 时把错误写回状态机，
/// 启动和换仓库的步骤不会停在进行中。
fn dispatch_workspace_effects(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    for effect in app.shell.workspace.take_effects() {
        match effect {
            WorkspaceEffect::PersistSidebar => {
                app.shell.workspace.save_prefs_file(&sidebar_prefs_path());
            }
            WorkspaceEffect::PersistLastRepository => {
                app.shell.workspace.save_last_repository_file(&last_repository_path());
            }
            WorkspaceEffect::StopPlayback { previous_repo_id } => {
                eprintln!("Nana 仓库切换，停止播放会话：{previous_repo_id}");
                app.shell.reduce(ShellMessage::Player(crate::shell::player::PlayerMessage::Stop {
                    repo_id: Some(previous_repo_id),
                    clear_stored: true,
                }));
            }
            WorkspaceEffect::RefreshRepositories { generation } => dispatch_list_repositories(app, context, generation),
            WorkspaceEffect::SyncRepository { repo_id, generation } => {
                let failed = move |error| ShellMessage::RepositorySynced { generation, result: Err(error) };
                task_guard::spawn_service(app.services.as_ref(), context, "资源库同步", failed, |services| {
                    let tasks = services.tasks.clone();
                    let executor = services.executor.clone();
                    move || {
                        let result = executor
                            .block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id }))
                            .map(|(output, _)| shell::SyncCounts::from_output(&output));
                        ShellMessage::RepositorySynced { generation, result }
                    }
                });
            }
            WorkspaceEffect::LoadSnapshot { repo_id, generation } => {
                let failed = move |error| ShellMessage::RepositorySnapshotLoaded { generation, result: Err(error) };
                task_guard::spawn_service(app.services.as_ref(), context, "资源库摘要", failed, |services| {
                    let query = services.repository_query.clone();
                    let executor = services.executor.clone();
                    move || ShellMessage::RepositorySnapshotLoaded { generation, result: executor.block_on(query.get_repository_snapshot(repo_id)) }
                });
            }
            WorkspaceEffect::LoadFirstScreen { repo_id } => app_dispatch::dispatch_first_screen(app, context, repo_id),
            WorkspaceEffect::RelocateRepository { repo_id, path } => {
                let failed = |error| ShellMessage::MissingRelocateFinished(Err(error));
                task_guard::spawn_service(app.services.as_ref(), context, "资源库重定向", failed, |services| {
                    let tasks = services.tasks.clone();
                    let query = services.repository_query.clone();
                    let executor = services.executor.clone();
                    move || {
                        let result = executor
                            .block_on(tasks.execute(PROTOCOL_REPOSITORY_RELOCATE, RepositoryRelocateRequest { repo_id, path }))
                            .and_then(|(output, _)| app_dispatch::mutated_repository(&executor, &query, &output));
                        ShellMessage::MissingRelocateFinished(result)
                    }
                });
            }
            WorkspaceEffect::DeleteRepository { repo_id, mode } => {
                let mode = match mode {
                    DeleteMode::RecordOnly => RepositoryDeleteMode::RecordOnly,
                    DeleteMode::DeleteMetadata => RepositoryDeleteMode::DeleteMetadata,
                    DeleteMode::DeleteFolder => RepositoryDeleteMode::DeleteFolder,
                };
                let failed = |error| ShellMessage::MissingDeleteFinished(Err(error));
                task_guard::spawn_service(app.services.as_ref(), context, "资源库删除", failed, |services| {
                    let management = services.repository_management.clone();
                    let executor = services.executor.clone();
                    move || {
                        let request = RepositoryDeleteRequest { repo_id, mode };
                        ShellMessage::MissingDeleteFinished(executor.block_on(management.delete_repository(request)))
                    }
                });
            }
            WorkspaceEffect::RefreshRepositoriesSilent => {
                shell::workspace_refresh::dispatch_silent_list(app, context);
            }
            WorkspaceEffect::LoadSnapshotSilent { repo_id } => {
                shell::workspace_refresh::dispatch_silent_snapshot(app, context, repo_id);
            }
        }
    }
}

pub(crate) use shell::decode_preview_pixels;

/// 启动原生 NanaUI 窗口。
///
/// 先按应用身份解析 NanaUI 的应用路径、定下本机读写位置（`app_paths.rs`），再把同一份路径交给 NanaUI。
/// 启动时读发布包的 manifest；以 `NANA_PACKAGE_VALIDATE=1` 启动时 NanaUI 在开窗前做发布包自检并退出，
/// 走不到领域服务，也不写服务目录。
pub fn run() -> Result<(), nana_ui::HostedRunError> {
    let identity = app_paths::identity();
    let paths = match ApplicationPaths::resolve(&identity) {
        Ok(paths) => Some(paths),
        Err(error) => {
            eprintln!("Nana 应用路径解析失败，按开发时的位置读写：{error}");
            None
        }
    };
    app_paths::install(app_paths::HostLocations::resolve(paths.as_ref(), &app_paths::LegacyInputs::from_env()));
    let mut builder = NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .resource_packs(ResourcePackOptions::new());
    if let Some(paths) = paths {
        builder = builder.paths(paths);
    }
    builder.run::<RuntimeApplication<MomoBakoApplication>>(app_icon::with_window_icon(
        window_state::main_window_descriptor(),
    ))
}

/// 为离屏验收创建与生产宿主相同的 Runtime 文档。
pub fn acceptance_document() -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for(ShellPage::Loading)
}

/// 为离屏验收创建指定页面状态的生产 Runtime 文档。
pub fn acceptance_document_for(
    page: ShellPage,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    acceptance_document_for_model(ShellViewModel::for_page(page))
}

/// 按离屏视口宽度挂载验收文档。窄于 Vue 断点的视口走窄屏排布。
pub fn acceptance_document_at_width(
    mut model: ShellViewModel,
    width: f32,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    model.set_viewport_width(width);
    acceptance_document_for_model(model)
}

/// 为离屏验收挂载指定 ViewModel；仍然复用生产壳层挂载函数和同一棵 Runtime 树。
pub fn acceptance_document_for_model(
    model: ShellViewModel,
) -> Result<nana_ui::runtime::RuntimeDocument, FrameworkError> {
    let document_id = DocumentId::new(1).expect("document id 1 is valid");
    let mut document = nana_ui::runtime::RuntimeDocument::new(document_id);
    mount_shell(&mut document, &model)?;
    Ok(document)
}
