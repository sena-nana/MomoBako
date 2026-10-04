//! Standard Rust application owner. Custom and JS hosts may still implement
//! `RuntimeProgram` directly; application business state stays in `State`.
use crate::{
    FrameDemand, HostTextureRegistry, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
    SceneGpuRendererRegistry, SceneResourceProducerRegistry, ThemeMode,
};
use nana_ui_platform::{WindowEvent, WindowId};
use nana_ui_scene::{DocumentAccessError, RuntimeDocument};
use std::collections::HashMap;

pub struct ApplicationWindow {
    pub document: RuntimeDocument,
    pub textures: HostTextureRegistry,
    pub renderers: Option<SceneGpuRendererRegistry>,
    pub producers: Option<SceneResourceProducerRegistry>,
    /// Egress for this window's `http(s)` `url(...)` images; `None` refuses them.
    /// Keep one host per window: replacing the `Arc` refetches its images.
    pub fetch_host: Option<nana_ui_platform::SharedFetchHost>,
    pub demand: FrameDemand,
}

impl ApplicationWindow {
    pub fn new() -> Self {
        Self::with_document(RuntimeDocument::new(Self::DOCUMENT))
    }

    /// A window whose built-in components are created from their Rust types
    /// only ([`nana_ui_runtime::BuiltinComponents::Typed`]).
    pub fn typed() -> Self {
        Self::with_document(RuntimeDocument::typed(Self::DOCUMENT))
    }

    // Each window owns an independent UiWorld. Document identities are local
    // to that world; WindowId handles host routing.
    const DOCUMENT: nana_ui_runtime::DocumentId = nana_ui_runtime::DocumentId::new(1).unwrap();

    /// `S::BUILTINS` is a constant in each instance, so the mode not chosen
    /// is not linked.
    fn for_state<S: ApplicationState>() -> Self {
        match S::BUILTINS {
            nana_ui_runtime::BuiltinComponents::Full => Self::new(),
            nana_ui_runtime::BuiltinComponents::Typed => Self::typed(),
        }
    }

    fn with_document(document: RuntimeDocument) -> Self {
        Self {
            document,
            textures: HostTextureRegistry::new(),
            renderers: None,
            producers: None,
            fetch_host: None,
            demand: FrameDemand::OnDemand,
        }
    }
}

impl Default for ApplicationWindow {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ApplicationState: Sized + 'static {
    type Message: Send + 'static;
    type Error: std::fmt::Display;
    /// The built-in component machinery each window installs. `Typed` suits
    /// an application that creates components from Rust types only (`mount`,
    /// declarative views, `.vue`): the components it never creates are not
    /// linked. Tag-based binding (`bind_semantic`) then refuses built-ins.
    const BUILTINS: nana_ui_runtime::BuiltinComponents = nana_ui_runtime::BuiltinComponents::Full;
    /// The `UiReady` point of startup (see [`crate::startup`]): the host can
    /// draw an ordinary document now. Create the state the first screen needs
    /// and start the rest as tasks; [`Self::build`] follows immediately.
    fn initialize(context: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error>;
    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error>;
    fn update(
        &mut self,
        _message: Self::Message,
        _windows: &mut HashMap<WindowId, ApplicationWindow>,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }
    /// Observe framework window lifecycle events without importing native events.
    /// `Closed` is delivered after the document is removed and `window_closed` returns.
    fn window_event(
        &mut self,
        _event: &WindowEvent,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }
    fn theme_mode(&self) -> ThemeMode {
        ThemeMode::Dark
    }
    /// Release application-owned state keyed by a window that has closed.
    fn window_closed(&mut self, _id: WindowId) {}
    fn prepare(
        &mut self,
        _window: &mut ApplicationWindow,
        _context: &RuntimeProgramContext<Self::Message>,
    ) {
    }
    fn presented(
        &mut self,
        _window: &mut ApplicationWindow,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }
    fn rebuild_gpu(
        &mut self,
        _windows: &mut HashMap<WindowId, ApplicationWindow>,
        _context: &RuntimeProgramContext<Self::Message>,
    ) {
    }
    fn host_failure(&mut self, failure: crate::HostFailure) {
        eprintln!("NanaUI host failure: {failure:?}");
    }
    /// Whether the primary window's first document may replace the Early
    /// Splash as soon as it has a frame. See
    /// [`RuntimeProgram::startup_takeover`].
    fn startup_takeover(&self) -> crate::StartupTakeover {
        crate::StartupTakeover::Immediate
    }
    /// The startup moved to a later phase. See
    /// [`RuntimeProgram::startup_changed`].
    fn startup_changed(
        &mut self,
        _status: &crate::StartupStatus,
        _windows: &mut HashMap<WindowId, ApplicationWindow>,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }
}

pub struct RuntimeApplication<State: ApplicationState> {
    pub state: State,
    pub windows: HashMap<WindowId, ApplicationWindow>,
}

impl<State: ApplicationState> RuntimeProgram for RuntimeApplication<State> {
    type Message = State::Message;
    type Error = State::Error;
    fn initialize(
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), Self::Error> {
        let mut state = State::initialize(context)?;
        let mut window = ApplicationWindow::for_state::<State>();
        state.build(&mut window, context)?;
        Ok((
            Self {
                state,
                windows: HashMap::from([(context.window_id(), window)]),
            },
            Vec::new(),
        ))
    }
    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self.windows.get(&id).map(|window| f(&window.document)))
    }
    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self
            .windows
            .get_mut(&id)
            .map(|window| f(&mut window.document)))
    }
    fn update(
        &mut self,
        message: Self::Message,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        self.state.update(message, &mut self.windows, context)
    }
    fn theme_mode(&self) -> ThemeMode {
        self.state.theme_mode()
    }
    fn frame_demand(&self, id: WindowId) -> FrameDemand {
        self.windows
            .get(&id)
            .map_or(FrameDemand::OnDemand, |window| window.demand)
    }
    fn host_textures(&self, id: WindowId) -> Option<HostTextureRegistry> {
        self.windows.get(&id).map(|window| window.textures.clone())
    }
    fn scene_gpu_renderers(&self, id: WindowId) -> Option<SceneGpuRendererRegistry> {
        self.windows
            .get(&id)
            .and_then(|window| window.renderers.clone())
    }
    fn scene_resource_producers(&self, id: WindowId) -> Option<SceneResourceProducerRegistry> {
        self.windows
            .get(&id)
            .and_then(|window| window.producers.clone())
    }
    fn resource_fetch_host(&self, id: WindowId) -> Option<nana_ui_platform::SharedFetchHost> {
        self.windows
            .get(&id)
            .and_then(|window| window.fetch_host.clone())
    }
    fn prepare_window_frame(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Self::Message>,
    ) {
        if let Some(window) = self.windows.get_mut(&id) {
            self.state.prepare(window, context);
        }
    }
    fn window_frame_presented(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        self.windows
            .get_mut(&id)
            .map_or_else(RuntimeProgramUpdate::default, |window| {
                self.state.presented(window, context)
            })
    }
    fn initialize_window(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), String> {
        let mut window = ApplicationWindow::for_state::<State>();
        self.state
            .build(&mut window, context)
            .map_err(|error| error.to_string())?;
        self.windows.insert(id, window);
        Ok(())
    }

    fn discard_window(&mut self, id: WindowId) {
        self.windows.remove(&id);
        self.state.window_closed(id);
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if let WindowEvent::Closed { id } = &event {
            self.windows.remove(id);
            self.state.window_closed(*id);
        }
        let application_update = self.state.window_event(&event, context);
        // The host never closes by itself. A program that redraws or answers
        // with commands is confirming, deferring, or closing on its own.
        // An empty answer still closes, so existing programs keep that default.
        match &event {
            WindowEvent::CloseRequested { id } => {
                let answered = application_update.exit
                    || application_update.redraw != crate::RuntimeRedraw::None
                    || !application_update.window_commands.is_empty();
                if answered {
                    application_update
                } else {
                    RuntimeProgramUpdate {
                        window_commands: vec![nana_ui_platform::host::WindowCommand::Close(*id)],
                        ..RuntimeProgramUpdate::default()
                    }
                    .merge(application_update)
                }
            }
            _ => RuntimeProgramUpdate::default().merge(application_update),
        }
    }
    fn rebuild_gpu(&mut self, context: &RuntimeProgramContext<Self::Message>) {
        self.state.rebuild_gpu(&mut self.windows, context);
    }
    fn host_failure(&mut self, failure: crate::HostFailure) {
        self.state.host_failure(failure);
    }
    fn startup_takeover(&self) -> crate::StartupTakeover {
        self.state.startup_takeover()
    }
    fn startup_changed(
        &mut self,
        status: &crate::StartupStatus,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        self.state
            .startup_changed(status, &mut self.windows, context)
    }
}
