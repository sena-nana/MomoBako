//! MomoBako 的 NanaUI 原生宿主骨架。
//!
//! 该 crate 只负责窗口、Runtime 文档和 Nana 控件树；资源库领域服务在
//! `src-backend` 收口后由应用状态注入。离屏测试复用同一棵
//! `RuntimeDocument`，不创建第二套 UI 树或 GPU 设备。

use nana_ui::runtime::FrameworkError;
use nana_ui::{
    ApplicationState, ApplicationWindow, DiagnosticsConfig, HostTexture, NanaApplication,
    ResourcePackOptions, RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate,
};
use nana_ui_platform::ApplicationPaths;

mod app_icon;
mod app_paths;
mod dispatch;
mod drag_out;
pub(crate) mod frame_clock;
mod host_bridge;
mod plugin_host;
pub mod shell;
mod task_watch;
pub(crate) mod tray;
mod window_host;
mod window_state;
use shell::{
    ShellMessage, ShellPage, ShellView, ShellViewModel, display_mode_path, last_repository_path,
    sidebar_prefs_path,
};

#[cfg(any(test, feature = "acceptance"))]
mod acceptance_document;
#[cfg(any(test, feature = "acceptance"))]
pub mod capability;
#[cfg(any(test, feature = "acceptance"))]
pub use acceptance_document::{
    acceptance_document, acceptance_document_at_width, acceptance_document_for,
    acceptance_document_for_model,
};
pub mod host_api;
pub mod services;
pub mod theme_map;

/// Nana 宿主直接使用共享领域服务 crate；宿主代码经 `crate::backend` 接应用服务（`backend::app`）。
pub use momobako_backend as backend;
pub(crate) use shell::prepare_text as prepare_preview_text;
pub mod appearance;
pub mod settings;
mod thumbnail_cache;
mod thumbnail_host;

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
                    events.dispatch(ShellMessage::Host(
                        shell::host_events::HostMessage::from_event(event),
                    ));
                });
                let tasks = context.clone();
                services.watch_tasks(move |active| {
                    tasks.dispatch(ShellMessage::TaskProgressLoaded(active))
                });
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
            shell
                .workspace
                .startup
                .fail("领域服务启动失败，请检查服务目录和端口配置");
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
        shell
            .workspace
            .load_last_repository_file(&last_repository_path());
        shell.files.load_display_mode_file(&display_mode_path());
        shell
            .asmr
            .load_playlist_file(&crate::shell::asmr_playlist_path());
        shell.player.load_default_files();
        shell.admin.load_default_file();
        let system_appearance = context.system_appearance();
        let theme = appearance::initial_theme(appearance::Appearance::from_shell(
            &shell,
            system_appearance,
        ));
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
        match self
            .view
            .as_mut()
            .filter(|view| view.owns(&window.document))
        {
            Some(view) => view.sync(&mut window.document, &self.shell)?,
            None => self.view = Some(ShellView::mount(&mut window.document, &self.shell)?),
        }
        self.shell.surface_dirty = false;
        self.applied_appearance = None;
        appearance::sync(self, &mut window.document);
        if !self.repositories_load_scheduled && self.services.is_some() {
            dispatch::workspace::dispatch_list_repositories(
                self,
                context,
                self.shell.workspace.list_generation,
            );
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
            dispatch::inspect::effects(self, context);
        }
        window_host::prepare_motion(&mut self.shell, self.view.as_mut(), window, step);
        // 空闲预取在准备帧里排下解码请求（`drive_prefetch`）：这一帧就交出去，不等下一条消息来了才解码。
        dispatch::files::effects(self, context);
        if let Some(message) = shell::load_more_if_sentinel_near(&mut self.shell, &window.document)
        {
            context.dispatch(message);
        }
        // 右键菜单开着时，列表或侧栏一滚就收起，任何一个滚动区都算。
        for message in shell::dismiss_context_menus_on_scroll(&mut self.shell, &window.document) {
            context.dispatch(message);
        }
        self.host.frames.note_demand(window.demand);
        for request in dispatch::sidebar::dispatch_prepared_browses(&mut self.shell) {
            dispatch::sidebar::dispatch_browse_request(self, context, request);
        }
        thumbnail_host::publish_preview(self, window, context);
        self.publish_thumbnails(window, context);
        appearance::sync(self, &mut window.document);
    }

    /// 一帧画完以后：对话框在这一帧的刷新里才关上、控件也可能刚卸掉，照焦点归还的规则补还焦点；
    /// 焦点动了再画一帧，焦点环跟着显示。
    fn presented(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let Some(view) = self
            .view
            .as_mut()
            .filter(|view| view.owns(&window.document))
        else {
            return RuntimeProgramUpdate::default();
        };
        if view.settle_focus(&mut window.document, &self.shell) {
            RuntimeProgramUpdate::redraw(context.window_id())
        } else {
            RuntimeProgramUpdate::default()
        }
    }

    fn update(
        &mut self,
        mut message: ShellMessage,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        window_host::sync_timers(self);
        if let ShellMessage::ThumbnailPixels(frames) = &mut message {
            self.queue_thumbnail_frames(frames);
        }
        if let ShellMessage::Tray(action) = message {
            window_host::on_tray(context, action);
            return RuntimeProgramUpdate::default();
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
        dispatch::before_reduce(self, &message, context);
        self.shell.reduce(message);
        dispatch::after_reduce(self, context);
        if self.shell.asmr.take_playlist_dirty() {
            self.shell
                .asmr
                .save_playlist_file(&crate::shell::asmr_playlist_path());
        }
        window_host::after_update(self, context);
        let maximized = context.geometry().maximized;
        let window_commands = self.shell.input.take_platform_commands(*id, maximized);
        // 服务派发和宿主请求可能在归约以外写下失败，同步视图之前收进状态区。
        self.shell.observe_failures();
        if let Some(view) = self.view.as_mut() {
            if let Err(error) = view.sync(&mut window.document, &self.shell) {
                eprintln!("Nana 壳层同步失败：{error}");
                self.shell.page = ShellPage::Error;
                return RuntimeProgramUpdate {
                    window_commands,
                    ..RuntimeProgramUpdate::redraw(*id)
                };
            }
            self.shell.surface_dirty = false;
        }
        RuntimeProgramUpdate {
            window_commands,
            ..RuntimeProgramUpdate::redraw(*id)
        }
    }

    fn window_event(
        &mut self,
        event: &nana_ui_platform::WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        window_host::on_window_event(self, event, context)
    }

    /// 运行时路由完的每个输入。先记下按着的修饰键；Escape 没被控件处理掉、ViewModel 里还有能关的层时，
    /// 发一条消息关掉最上面一层。窗口失焦时收起右键菜单，拖动已经离开够远时交给系统拖放。
    /// 主键指针离开窗口时，同样把内部拖动交给已有的离开归约。外部文件拖放按坐标记下文件夹悬停，
    /// 必须在本帧的宿主放下归约之前写好，放下才会进这个文件夹。这些消息和别的消息一样经 `update` 归约。
    fn input_event(
        &mut self,
        id: nana_ui_platform::WindowId,
        input: nana_ui::RoutedInput<'_>,
        windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        window_host::sync_timers(self);
        window_host::note_modifiers(&mut self.shell, &input.event.payload);
        match windows.get(&id) {
            Some(window) => shell::note_external_folder_drag(
                &mut self.shell,
                &window.document,
                &input.event.payload,
            ),
            None if matches!(input.event.payload, nana_ui::InputPayload::FileDrag(_)) => {
                eprintln!("Nana 外部拖放时窗口不在，文件夹悬停不更新");
            }
            None => {}
        }
        if let Some(message) = window_host::escape_message(
            &self.shell,
            &input.event.payload,
            input.disposition.prevent_default,
        ) {
            context.dispatch(message);
        }
        for message in shell::focus_loss_messages(&self.shell, &input.event.payload) {
            context.dispatch(message);
        }
        if let Some(message) = shell::pointer_leave_message(&self.shell, &input.event.payload) {
            context.dispatch(message);
        }
        Ok(RuntimeProgramUpdate::default())
    }

    /// 最早到期的计时器的时刻（`window_host::next_timer_in_ms`）。只有计时器在等时窗口不画帧，NanaUI 到点调 `wake`。
    fn next_wakeup(&self) -> Option<std::time::Instant> {
        window_host::next_timer_in_ms(&self.shell).map(|due_in| self.host.frames.deadline(due_in))
    }

    /// 计时器到点：拨钟并触发到点的计时器，把排下的检视和文件请求交出去。界面要跟着变（有过归约或标了脏）
    /// 时画一帧，只排了预取解码时不画。
    fn wake(
        &mut self,
        now: std::time::Instant,
        _windows: &mut std::collections::HashMap<nana_ui_platform::WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let revision = self.shell.revision;
        let timer_ms = self.host.frames.sync_timers(now);
        let step = frame_clock::FrameStep {
            timer_ms,
            motion_ms: 0,
        };
        if crate::shell::poll_timers(&mut self.shell, step) {
            dispatch::inspect::effects(self, context);
        }
        window_host::run_timers(&mut self.shell, timer_ms);
        dispatch::files::effects(self, context);
        if self.shell.revision != revision || self.shell.surface_dirty {
            RuntimeProgramUpdate::redraw(context.window_id())
        } else {
            RuntimeProgramUpdate::default()
        }
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
    app_paths::install(app_paths::HostLocations::resolve(
        paths.as_ref(),
        &app_paths::LegacyInputs::from_env(),
    ));
    let mut builder = NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .resource_packs(ResourcePackOptions::new());
    if let Some(paths) = paths {
        builder = builder.paths(paths);
    }
    let descriptor = app_icon::with_window_icon(window_state::main_window_descriptor());
    match window_state::store(&descriptor) {
        Some(store) => {
            builder.run_with_store::<RuntimeApplication<MomoBakoApplication>>(descriptor, store)
        }
        None => builder.run::<RuntimeApplication<MomoBakoApplication>>(descriptor),
    }
}
