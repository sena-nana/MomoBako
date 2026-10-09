//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-backend` 收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::{DocumentId, FrameworkError, Task};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    GpuTexture, RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate,
};

pub mod shell;
mod app_dispatch;
mod files_dispatch;
mod inspect_dispatch;
mod player_dispatch;
mod admin_dispatch;
mod sidebar_dispatch;
mod sync_dispatch;
mod task_watch;
mod window_host;
mod host_bridge;
mod window_state;
mod drag_out;
mod tray;
use shell::{
    DeleteMode, ShellMessage, ShellPage, ShellView, ShellViewModel,
    WorkspaceEffect, display_mode_path, mount_shell, sidebar_prefs_path,
};

pub mod capability;
pub mod host_api;
pub mod services;
pub mod theme_map;

/// Nana 宿主直接使用共享领域服务 crate，迁移期 Tauri 仍保留同一服务源码的
/// 适配入口；此 re-export 让后续 ViewModel 接线不需要再穿过 command 层。
pub use momobako_backend as backend;
pub(crate) use shell::prepare_text as prepare_preview_text;
use backend::services::mutsuki_runner::{PROTOCOL_REPOSITORY_RELOCATE, PROTOCOL_REPOSITORY_SYNC};
use backend::services::repository::{
    RepositoryDeleteMode, RepositoryDeleteRequest, RepositoryRelocateRequest,
    SyncRequest,
};
pub mod plugin_api;
pub mod settings;
mod thumbnail_host;
pub mod appearance;

use thumbnail_host::{PendingThumb, ThumbnailGpu};

/// Nana 宿主应用状态，持有共享领域 Runtime 和原生壳层 ViewModel。
pub struct MomoBakoApplication {
    pub services: Option<services::NativeServices>,
    shell: ShellViewModel,
    repositories_load_scheduled: bool,
    preview_gpu: Option<NativePreviewGpu>,
    pub(crate) still_gpu: Option<NativePreviewGpu>,
    pub(crate) thumbnail_gpu: Vec<ThumbnailGpu>,
    pub(crate) pending_thumbs: Vec<PendingThumb>,
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

pub(crate) struct NativePreviewGpu {
    pub(crate) token: String,
    pub(crate) texture: GpuTexture,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl MomoBakoApplication {
    /// 当前图片幻灯片的路径和像素。没有帧时不返回。
    pub(crate) fn slideshow_frame(&self) -> Option<(String, crate::shell::PreviewPixels)> {
        let still = self.shell.player.still.as_ref()?;
        Some((still.path.clone(), still.frame.clone()?))
    }
}

impl Default for MomoBakoApplication {
    fn default() -> Self {
        Self {
            services: None,
            shell: ShellViewModel::default(),
            repositories_load_scheduled: false,
            preview_gpu: None,
            still_gpu: None,
            thumbnail_gpu: Vec::new(),
            pending_thumbs: Vec::new(),
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
            still_gpu: None,
            thumbnail_gpu: Vec::new(),
            pending_thumbs: Vec::new(),
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
        if !self.repositories_load_scheduled {
            if let Some(services) = self.services.as_ref() {
                let query = services.repository_query.clone();
                let executor = services.executor.clone();
                let generation = self.shell.workspace.list_generation;
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: executor.block_on(query.list_repositories()),
                    }
                })) {
                    eprintln!("Nana 资源库加载任务提交失败：{error}");
                }
                self.repositories_load_scheduled = true;
            }
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
        if crate::shell::poll_timers(&mut self.shell) {
            inspect_dispatch::dispatch_inspect_effects(self, context);
        }
        window_host::prepare_motion(&mut self.shell, self.view.as_mut(), window);
        for request in sidebar_dispatch::dispatch_prepared_browses(&mut self.shell) {
            sidebar_dispatch::dispatch_browse_request(self, context, request);
        }
        thumbnail_host::publish_still(self, window, context);
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
        let snapshot_repo_id = app_dispatch::snapshot_repo(&message);
        self.shell.reduce(message);
        app_dispatch::after_snapshot(self, snapshot_repo_id, context);
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

/// 执行工作台归约留下的副作用。服务未启动或任务提交失败时把错误写回状态机，避免步骤停在进行中。
fn dispatch_workspace_effects(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    for effect in app.shell.workspace.take_effects() {
        match effect {
            WorkspaceEffect::PersistSidebar => {
                app.shell.workspace.save_prefs_file(&sidebar_prefs_path());
            }
            WorkspaceEffect::StopPlayback { previous_repo_id } => {
                eprintln!("Nana 仓库切换，停止播放会话：{previous_repo_id}");
                app.shell.reduce(ShellMessage::Player(crate::shell::player::PlayerMessage::Stop {
                    repo_id: Some(previous_repo_id),
                    clear_stored: true,
                }));
            }
            WorkspaceEffect::RefreshRepositories { generation } => {
                let prepared = app.services.as_ref().map(|services| {
                    (services.repository_query.clone(), services.executor.clone())
                });
                let Some((query, executor)) = prepared else {
                    eprintln!("Nana 资源库刷新需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: Err("领域服务未启动".into()),
                    });
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: executor.block_on(query.list_repositories()),
                    }
                })) {
                    eprintln!("Nana 资源库刷新任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::WorkspaceListLoaded {
                        generation,
                        result: Err(format!("资源库列表任务提交失败：{error}")),
                    });
                }
            }
            WorkspaceEffect::SyncRepository { repo_id, generation } => {
                let prepared = app
                    .services
                    .as_ref()
                    .map(|services| (services.tasks.clone(), services.executor.clone()));
                let Some((tasks, executor)) = prepared else {
                    eprintln!("Nana 资源库同步需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::StartupSyncFinished {
                        generation,
                        result: Err("领域服务未启动".into()),
                    });
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    let result = executor
                        .block_on(tasks.execute(PROTOCOL_REPOSITORY_SYNC, SyncRequest { repo_id }))
                        .map(|_| ());
                    ShellMessage::StartupSyncFinished { generation, result }
                })) {
                    eprintln!("Nana 资源库同步任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::StartupSyncFinished {
                        generation,
                        result: Err(format!("资源库同步任务提交失败：{error}")),
                    });
                }
            }
            WorkspaceEffect::LoadSnapshot { repo_id } => {
                let prepared = app.services.as_ref().map(|services| {
                    (services.repository_query.clone(), services.executor.clone())
                });
                let Some((query, executor)) = prepared else {
                    eprintln!("Nana 资源库摘要需要领域服务，当前服务未启动");
                    app.shell.reduce(ShellMessage::RepositorySnapshotLoaded(Err(
                        "领域服务未启动".into(),
                    )));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::RepositorySnapshotLoaded(
                        executor.block_on(query.get_repository_snapshot(repo_id)),
                    )
                })) {
                    eprintln!("Nana 资源库摘要任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::RepositorySnapshotLoaded(Err(format!(
                        "资源库摘要任务提交失败：{error}"
                    ))));
                }
            }
            WorkspaceEffect::RelocateRepository { repo_id, path } => {
                let prepared = app
                    .services
                    .as_ref()
                    .map(|services| (services.tasks.clone(), services.executor.clone()));
                let Some((tasks, executor)) = prepared else {
                    eprintln!("Nana 资源库重定向需要领域服务，当前服务未启动");
                    app.shell
                        .reduce(ShellMessage::MissingRelocateFinished(Err("领域服务未启动".into())));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    let result = executor
                        .block_on(tasks.execute(
                            PROTOCOL_REPOSITORY_RELOCATE,
                            RepositoryRelocateRequest { repo_id, path },
                        ))
                        .map(|_| ());
                    ShellMessage::MissingRelocateFinished(result)
                })) {
                    eprintln!("Nana 资源库重定向任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::MissingRelocateFinished(Err(format!(
                        "资源库重定向任务提交失败：{error}"
                    ))));
                }
            }
            WorkspaceEffect::DeleteRepository { repo_id, mode } => {
                let prepared = app.services.as_ref().map(|services| {
                    (
                        services.repository_management.clone(),
                        services.executor.clone(),
                    )
                });
                let mode = match mode {
                    DeleteMode::RecordOnly => RepositoryDeleteMode::RecordOnly,
                    DeleteMode::DeleteMetadata => RepositoryDeleteMode::DeleteMetadata,
                    DeleteMode::DeleteFolder => RepositoryDeleteMode::DeleteFolder,
                };
                let Some((management, executor)) = prepared else {
                    eprintln!("Nana 资源库删除需要领域服务，当前服务未启动");
                    app.shell
                        .reduce(ShellMessage::MissingDeleteFinished(Err("领域服务未启动".into())));
                    continue;
                };
                if let Err(error) = context.run_task(Task::new(async move {
                    ShellMessage::MissingDeleteFinished(
                        executor.block_on(
                            management.delete_repository(RepositoryDeleteRequest { repo_id, mode }),
                        ),
                    )
                })) {
                    eprintln!("Nana 资源库删除任务提交失败：{error}");
                    app.shell.reduce(ShellMessage::MissingDeleteFinished(Err(format!(
                        "资源库删除任务提交失败：{error}"
                    ))));
                }
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
pub fn run() -> Result<(), nana_ui::HostedRunError> {
    let identity = ApplicationIdentity::new(
        "com.momobako.desktop",
        "MomoBako",
        env!("CARGO_PKG_VERSION"),
    );
    NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<RuntimeApplication<MomoBakoApplication>>(
            window_state::main_window_descriptor(),
        )
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
