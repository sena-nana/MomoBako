//! Nana-owned winit + wgpu loop for [`crate::RuntimeProgram`].
//!
//! Paint goes through [`crate::SceneWgpuPainter`].

mod accessibility;
mod browser;
mod dialogs;
mod display;
mod host_services;
mod input;
mod presence;
mod present;
mod schedule;
mod startup;
mod windows;

use accessibility::PendingAccessibility;
use host_services::{NativeWindowServices, WindowInputSource};

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::presentation::{ResolvedWindowPresentation, WindowSurfaceTarget};
use nana_ui_core::{AppearanceSettings, RESIZE_HANDLE_SIZE, SharedStore, TITLE_BAR_HEIGHT};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{
    CompositionInput, DisplayBounds, FileDragInput, FileDragKind, FullscreenRequest,
    InputModifiers, InputPayload, MousePassthroughMode, PointerInput, PointerPhase, PointerType,
    SystemAppearance, WindowEvent, WindowGeometry, WindowIcon, WindowId, WindowLevel,
    WindowModeState, WindowResizeEdge, clamp_position_to_displays,
    clear_registered_application_icon, persist_live_window_geometry, register_application_icon,
    restore_window_geometry, window_resize_edge,
};
use nana_ui_runtime::{
    AccessibilityUpdate, AppTitleBar, Entity, FrameworkError, LayoutViewport, StableNodeId, Task,
};
#[cfg(target_os = "macos")]
use nana_window::set_application_icon_png;
use nana_window::{
    Appearance, FallbackColor, FrameResizeEdge, LiveSizeMove, MaterialOutcome,
    apply_hosted_system_material, arm_frameless_guard, clear_system_material,
    prepare_client_chrome, resize_custom_frame, set_frameless_styles,
};
use winit::application::ApplicationHandler;
use winit::cursor::CursorIcon;
use winit::data_transfer::{DataTransferId, TypeHint};
use winit::dpi::PhysicalPosition;
use winit::event::{
    ButtonSource, DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerKind,
    PointerSource, TabletToolKind, WindowEvent as WinitWindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop, AsyncRequestSerial, ControlFlow, DndAction, EventLoop, EventLoopProxy,
};
use winit::icon::{Icon, RgbaIcon};
use winit::keyboard::ModifiersState;
#[cfg(target_os = "macos")]
use winit::platform::macos::{WindowAttributesMacOS, WindowExtMacOS};
#[cfg(target_os = "windows")]
use winit::platform::windows::{CornerPreference, WindowAttributesWindows, WindowExtWindows};
use winit::raw_window_handle::HasWindowHandle;
#[cfg(target_os = "windows")]
use winit::raw_window_handle::RawWindowHandle;
use winit::window::Theme as WinitTheme;

#[cfg(not(target_os = "android"))]
use crate::accessibility::HostedAccessibility;
use crate::gpu_raw::GpuRaw;
use crate::nana_text::NanaTextShaper;
use crate::runtime_host::{
    HostDocumentAccess, HostFailure, ReportHostFailure, RuntimeProgram, RuntimeProgramContext,
    RuntimeProgramUpdate, RuntimeRedraw, WindowDescriptor,
};
use crate::scene_paint::{ScenePaintViewport, SceneWgpuPainter};
use crate::{
    HostTextureRegistry, HostedGpuError, HostedGpuSurface, HostedRunError, RuntimeAnimationClock,
    TitleBarDragTracker, WindowChromeAction, WindowChromeEvent, WindowChromeState,
    apply_title_bar_pointer, title_bar_hits_window_control as pointer_hits_window_control,
    window_commands_for_chrome_action,
};

const GPU_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const MAX_PROGRAM_DISPATCHES: usize = 32;
const TASK_QUEUE_CAPACITY: usize = 256;
const TASK_WORKERS: usize = 4;

/// Run a [`RuntimeProgram`] on the Nana Scene host.
///
/// Startup options set with [`crate::with_startup`] (or
/// [`crate::run_runtime_with_startup`]) on this thread apply to this host.
pub fn run_runtime_scene<Program: RuntimeProgram>(
    settings: WindowDescriptor,
) -> Result<(), HostedRunError> {
    let entry = Instant::now();
    let options = crate::runtime_host::take_pending_startup();
    startup::startup_event(startup::StartupMark::Entry, Duration::ZERO);
    let event_loop = EventLoop::new().map_err(HostedRunError::EventLoop)?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let (message_tx, message_rx) = mpsc::channel();
    let startup_failure = Arc::new(Mutex::new(None));
    let runner = SceneRunner::<Program>::Loading(Box::new(startup::LoadingStartup {
        proxy: event_loop.create_proxy(),
        message_tx,
        message_rx,
        settings,
        startup_failure: Arc::clone(&startup_failure),
        options,
        entry,
    }));
    let run = event_loop.run_app(runner);
    nana_diagnostics::event!(nana_diagnostics::framework::host::EVENT_LOOP_EXITED);
    run.map_err(HostedRunError::EventLoop)?;
    match startup_failure.lock().ok().and_then(|guard| guard.clone()) {
        Some(message) => Err(HostedRunError::Startup(message)),
        None => Ok(()),
    }
}

enum SceneRunner<Program: RuntimeProgram> {
    Loading(Box<startup::LoadingStartup<Program::Message>>),
    /// The window is up (with its splash, if any) and the device is being
    /// requested off the event thread.
    Starting(Box<startup::PendingStartup<Program::Message>>),
    Ready(Box<WindowManager<Program>>),
    Finished {
        startup_failure: Arc<Mutex<Option<String>>>,
    },
}

/// Clears native material registrations on every failed creation path.
/// What mirroring a window's scene into the platform compositor has cost since
/// the host started.
///
/// The steady-state contract: once a window settles, none of these move again,
/// however many GPU frames it goes on presenting. A retained compositor holds
/// the visuals where they were put; re-synchronising them at the GPU's frame
/// rate is work with no effect, and these counters are how a probe proves it
/// is not happening.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompositionWork {
    pub native_content: crate::NativeContentWork,
    /// Platform-compositor transactions published for this window. Zero on a
    /// window that presents through a plain native surface, and on every
    /// platform without a composition target.
    pub commits: usize,
    /// Visual-tree mutations staged for those transactions.
    pub tree_mutations: usize,
    /// Native chrome reconciliations this host has set out to do, over every
    /// window. Moving a window adds none.
    pub native_chrome_writes: usize,
}

struct PendingNativeWindow(Option<Arc<dyn winit::window::Window>>);
impl PendingNativeWindow {
    /// The window survived; its material stays applied.
    fn keep(mut self) {
        self.0 = None;
    }
}
impl Drop for PendingNativeWindow {
    fn drop(&mut self) {
        if let Some(window) = self.0.as_ref() {
            clear_system_material(window.as_ref());
        }
    }
}

struct WindowContext {
    surface_retry: Option<Instant>,
    applied_appearance: Option<windows::WindowAppearance>,
    cursor_override: Option<CursorIcon>,
    cursor_visible_override: Option<bool>,
    passthrough_mode: MousePassthroughMode,
    os_mouse_passthrough: bool,
    material_override: Option<nana_window::MaterialEffect>,
    surface: HostedGpuSurface,
    geometry: WindowGeometry,
    input: InputTracker,
    /// This window as an input source: its endpoint, stamps and what the
    /// Runtime last had applied to it.
    input_source: WindowInputSource,
    /// The only authority for what this window presents and for the native
    /// chrome that presentation requires. Nothing re-derives either from the
    /// request; see [`crate::presentation`].
    presentation: ResolvedWindowPresentation,
    settings: WindowDescriptor,
    #[cfg(not(target_os = "android"))]
    accessibility: Option<HostedAccessibility>,
    accessibility_pending: PendingAccessibility,
    size_move: LiveSizeMove,
    /// Level last applied; platforms do not report it back.
    level: WindowLevel,
    /// Mode last delivered through `WindowEvent::ModeChanged`.
    mode: Option<WindowModeState>,
    /// Fullscreen applied once the window is visible and not fullscreen.
    pending_fullscreen: Option<FullscreenRequest>,
    /// Native window buttons shown; re-applied after native style changes.
    native_controls_visible: bool,
    /// Title-bar placeholder the native window buttons follow, once found.
    native_controls: Option<nana_ui_runtime::StableNodeId>,
    /// Its last laid-out box, so a visibility or style change can put the
    /// buttons back within the same turn instead of a frame later.
    native_controls_box: std::cell::Cell<Option<nana_ui_runtime::LayoutBox>>,
    /// Taskbar entry state last applied successfully.
    skip_taskbar: bool,
    /// Descriptor outcome, applied before the first show and delivered after `Ready`.
    skip_taskbar_report: Option<Result<(), crate::WindowError>>,
    pointer_presence: presence::PointerPresence,
    /// The desktop shadow applied to this window (#215).
    shadow: nana_window::shadow::WindowShadowState,
    /// The visible body the shadow follows, with the scene projection and
    /// size it was derived for; derived again only when either changes.
    shadow_body: Option<ShadowBody>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ShadowBody {
    revision: (u64, u64),
    size: (f32, f32),
    visible: bool,
    shape: nana_ui_platform::WindowVisualShape,
}

impl Drop for WindowContext {
    fn drop(&mut self) {
        // The companion goes before the window it follows.
        self.shadow.release();
        clear_system_material(self.surface.window().as_ref());
    }
}

struct WindowManager<Program: RuntimeProgram> {
    program: Program,
    embedded: bool,
    shutting_down: bool,
    wake_deadline: Option<Instant>,
    host_work: Arc<schedule::HostWorkWake>,
    host_work_deadline: Option<Instant>,
    windows: crate::WindowService,
    window_requests: Option<Receiver<crate::window_service::Request>>,
    next_window_id: u64,
    // Native children drop before their owning GPU/window resources.
    browsers: HashMap<(WindowId, String), browser::HostedBrowser>,
    graphics: crate::HostedGpuShared,
    /// What this process needs from its GPU backend. Process-wide, because the
    /// backend, adapter and device are.
    gpu_backend_policy: crate::GpuBackendPolicy,
    /// How a transparent client stops DWM rendering a non-client area under it.
    /// Read once: the two strategies are compared on a real machine, not mixed
    /// within one run.
    non_client: nana_window::NonClientRenderingStrategy,
    /// Whether this process can present a window through a platform
    /// compositor. Settled before the first window existed, because the
    /// redirection bitmap is a creation-time flag.
    ///
    /// This says the path *exists*, not that any window is on it: each window
    /// asks for its own target from its descriptor.
    composition: crate::presentation::CompositionAvailability,
    painters: HashMap<nana_gpu::GpuTextureFormat, SceneWgpuPainter>,
    native_renderers:
        HashMap<nana_gpu::GpuTextureFormat, Arc<crate::native_content::NativeContentRenderer>>,
    text: NanaTextShaper,
    proxy: EventLoopProxy,
    message_tx: Sender<Program::Message>,
    messages: Receiver<Program::Message>,
    file_dialogs: dialogs::FileDialogs,
    tasks: SyncSender<Task<Program::Message>>,
    animation_clock: RuntimeAnimationClock,
    surface_generation: u64,
    frame_schedules: HashMap<WindowId, crate::runtime_host::FrameSchedule>,
    /// Host-texture slots each window's scene samples, with the subscription
    /// that wakes it; re-subscribed only when the slot set or registry changes.
    texture_subscriptions: HashMap<WindowId, (HashSet<Arc<str>>, crate::TextureSubscription)>,
    texture_redraws: Arc<Mutex<HashSet<WindowId>>>,
    image_targets: Arc<Mutex<HashMap<String, HashSet<WindowId>>>>,
    image_window_keys: HashMap<WindowId, HashSet<String>>,
    occluded: HashSet<WindowId>,
    /// Per-window mirror of the scene's native-content regions into the
    /// platform compositor. This is what makes the mirror retained: a frame
    /// whose scene projections did not move rebuilds nothing and stages
    /// nothing.
    native_content: HashMap<WindowId, crate::native_content::NativeContentMirror>,
    /// Times the host has set out to reconcile a window's native chrome — DWM
    /// corner preference, border colour, non-client rendering policy, frame
    /// styles, style guard. Counted at the attempt, so a window being moved
    /// must not add to it at all: position is not a window flag, so nothing
    /// winit does on a move can take the chrome back off.
    native_chrome_writes: std::cell::Cell<usize>,
    window_contexts: HashMap<WindowId, WindowContext>,
    window_ids: HashMap<winit::window::WindowId, WindowId>,
    closing_windows: HashSet<WindowId>,
    next_gpu_retry: Option<Instant>,
    render_suspended: bool,
    last_theme: crate::ThemeMode,
    /// System reduce-motion preference, as last delivered to the program.
    reduced_motion: bool,
    settings: WindowDescriptor,
    chrome: HashMap<WindowId, WindowChromeSession>,
    bind_after_present: HashSet<WindowId>,
    startup_failure: Arc<Mutex<Option<String>>>,
    store: SharedStore,
    /// What programs see through `RuntimeProgramContext::store`; the host
    /// keeps `store` for its own bounded shutdown flush.
    program_store: SharedStore,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    live_frame_resize: Option<(WindowId, nana_window::LiveFrameResize)>,
    /// Window, the button code whose release ends the gesture, and the session.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    live_frame_move: Option<(WindowId, i16, nana_window::LiveFrameMove)>,
    #[cfg(target_os = "macos")]
    present_transaction_pinned: HashSet<WindowId>,
    /// This host's startup: the coordinator, the splash it owns until
    /// handoff, and the record programs read.
    startup: startup::HostStartup,
    /// Monotonic generation per source identity. Window ids may be reused by
    /// an embedding host, so a reopened window must not accept old events.
    input_generations: HashMap<WindowId, u64>,
    /// The process's clipboard, the one backend every window's input uses.
    clipboard: nana_ui_platform::SharedClipboardHost,
    /// Routed events of the drain in progress, reused across drains.
    routed_input: Vec<nana_ui_runtime::RoutedEvent>,
    /// The primary window's icons, while the startup thread still renders them.
    pending_icons: Option<Receiver<SceneIcons>>,
    /// Messages `initialize` returned, not yet applied (only with a splash).
    startup_messages: std::collections::VecDeque<Program::Message>,
}

impl<Program: RuntimeProgram> Drop for WindowManager<Program> {
    fn drop(&mut self) {
        // App destructors may join workers waiting for window requests. Resolve
        // those requests before Rust drops `program` (the first field).
        self.window_requests.take();
        // Shutdown is the bounded final-flush lane. Normal UI updates only
        // advance the coordinator generation and never synchronously flush.
        let _ = self.store.flush();
    }
}

struct WindowChromeSession {
    state: WindowChromeState,
    drag: TitleBarDragTracker,
}

fn scene_image_keys(scene: &nana_ui_scene::UiScene) -> HashSet<String> {
    let mut keys = HashSet::new();
    for primitive in scene.primitives() {
        match &primitive.kind {
            nana_ui_scene::ScenePrimitiveKind::Quad { surface, .. }
            | nana_ui_scene::ScenePrimitiveKind::QuadBatch { surface, .. } => {
                surface_image_keys(surface, &mut keys);
            }
            nana_ui_scene::ScenePrimitiveKind::Custom {
                mask: Some(nana_ui_core::MaskImage::Url(url)),
                ..
            } => {
                keys.insert(url.clone());
            }
            _ => {}
        }
    }
    keys
}

fn surface_image_keys(surface: &nana_ui_scene::QuadSurfacePaint, keys: &mut HashSet<String>) {
    if let Some(image) = surface.background_image.as_ref() {
        add_background_image_key(image, keys);
    }
    for image in &surface.background_layers {
        add_background_image_key(image, keys);
    }
    if let Some(image) = surface.content_image.as_ref() {
        add_background_image_key(image, keys);
    }
    if let Some(image) = surface.mask.as_ref()
        && let nana_ui_core::MaskImage::Url(url) = image
    {
        keys.insert(url.clone());
    }
    if let Some(border) = surface.border_image.as_ref() {
        add_background_image_key(&border.source, keys);
    }
}

fn add_background_image_key(image: &nana_ui_core::BackgroundImage, keys: &mut HashSet<String>) {
    if let nana_ui_core::BackgroundImage::Url { url, .. } = image {
        keys.insert(url.clone());
    }
}

fn replace_image_target_index(
    targets: &mut HashMap<String, HashSet<WindowId>>,
    window_keys: &mut HashMap<WindowId, HashSet<String>>,
    id: WindowId,
    keys: HashSet<String>,
) {
    let previous = window_keys.insert(id, keys.clone()).unwrap_or_default();
    for key in previous {
        if let Some(ids) = targets.get_mut(&key) {
            ids.remove(&id);
            if ids.is_empty() {
                targets.remove(&key);
            }
        }
    }
    for key in keys {
        targets.entry(key).or_default().insert(id);
    }
}

fn remove_image_target_index(
    targets: &mut HashMap<String, HashSet<WindowId>>,
    window_keys: &mut HashMap<WindowId, HashSet<String>>,
    id: WindowId,
) {
    let Some(previous) = window_keys.remove(&id) else {
        return;
    };
    for key in previous {
        if let Some(ids) = targets.get_mut(&key) {
            ids.remove(&id);
            if ids.is_empty() {
                targets.remove(&key);
            }
        }
    }
}

impl WindowChromeSession {
    fn new(id: WindowId) -> Self {
        Self {
            state: WindowChromeState::for_window(id, crate::WindowChrome::platform_default()),
            drag: TitleBarDragTracker::default(),
        }
    }
}

impl<Program: RuntimeProgram> SceneRunner<Program> {
    fn fail(&mut self, event_loop: &dyn ActiveEventLoop, message: impl Into<String>) {
        let slot = match self {
            Self::Loading(loading) => Arc::clone(&loading.startup_failure),
            Self::Starting(pending) => Arc::clone(pending.startup_failure()),
            Self::Finished { startup_failure } => Arc::clone(startup_failure),
            Self::Ready(ready) => Arc::clone(&ready.startup_failure),
        };
        let message = message.into();
        nana_diagnostics::fault!(
            nana_diagnostics::framework::host::STARTUP_FAILED;
            "{message}"
        );
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(message);
        }
        // Dropping the pending startup releases its splash, window and
        // surface on this thread.
        *self = Self::Finished {
            startup_failure: slot,
        };
        event_loop.exit();
    }

    /// Takes the device the startup thread produced, if it has, and moves on:
    /// to `Ready`, to the next presentation target, or to a startup failure.
    fn poll_startup(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Self::Starting(pending) = self else {
            return;
        };
        let Some(result) = pending.take_device() else {
            return;
        };
        let slot = Arc::clone(pending.startup_failure());
        let Self::Starting(pending) = std::mem::replace(
            self,
            Self::Finished {
                startup_failure: Arc::clone(&slot),
            },
        ) else {
            unreachable!("checked Starting");
        };
        match pending.finish::<Program>(event_loop, result) {
            Ok(startup::StartupStep::Ready(ready)) => *self = Self::Ready(ready),
            Ok(startup::StartupStep::Retry(pending)) => *self = Self::Starting(pending),
            Err(error) => self.fail(event_loop, error),
        }
    }

    /// When the startup times event-loop callbacks, the start of this one.
    fn block_started(&self) -> Option<Instant> {
        let measures = match self {
            Self::Starting(_) => true,
            Self::Ready(ready) => ready.startup.measures_blocks(),
            Self::Loading(_) | Self::Finished { .. } => false,
        };
        measures.then(Instant::now)
    }

    /// Longest event-thread callback while the startup is measured.
    fn note_block(&mut self, started: Option<Instant>) {
        let Some(started) = started else {
            return;
        };
        match self {
            Self::Starting(pending) => pending.note_block(started.elapsed()),
            Self::Ready(ready) => ready.note_startup_block(started.elapsed()),
            Self::Loading(_) | Self::Finished { .. } => {}
        }
    }
}

impl<Program: RuntimeProgram> ApplicationHandler for SceneRunner<Program> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !matches!(self, Self::Loading(_)) {
            return;
        }
        let started = Some(Instant::now());
        let Self::Loading(loading) = std::mem::replace(
            self,
            Self::Finished {
                startup_failure: Arc::new(Mutex::new(None)),
            },
        ) else {
            unreachable!("checked Loading");
        };
        let slot = Arc::clone(&loading.startup_failure);
        match startup::PendingStartup::begin::<Program>(event_loop, *loading) {
            Ok(pending) => *self = Self::Starting(Box::new(pending)),
            Err(error) => {
                *self = Self::Finished {
                    startup_failure: slot,
                };
                self.fail(event_loop, error);
            }
        }
        self.note_block(started);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        let started = self.block_started();
        match self {
            Self::Starting(_) => self.poll_startup(event_loop),
            Self::Ready(ready) => ready.drain_host_work(event_loop),
            Self::Loading(_) | Self::Finished { .. } => {}
        }
        self.note_block(started);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WinitWindowEvent,
    ) {
        let started = self.block_started();
        match self {
            Self::Starting(pending) => {
                if let WinitWindowEvent::ScaleFactorChanged { scale_factor, .. } = &event {
                    pending.rescale_splash(window_id, *scale_factor);
                }
                // The program does not exist yet; closing the window cancels
                // the startup. Nothing was initialized, so nothing is
                // reported as a failure.
                if pending.owns(window_id) && matches!(event, WinitWindowEvent::CloseRequested) {
                    let slot = Arc::clone(pending.startup_failure());
                    if let Self::Starting(pending) = std::mem::replace(
                        self,
                        Self::Finished {
                            startup_failure: slot,
                        },
                    ) {
                        pending.cancel();
                    }
                    event_loop.exit();
                    return;
                }
            }
            Self::Ready(ready) => {
                let Some(id) = ready.window_ids.get(&window_id).copied() else {
                    return;
                };
                ready.handle_window_event(event_loop, id, event);
            }
            Self::Loading(_) | Self::Finished { .. } => return,
        }
        self.note_block(started);
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let started = self.block_started();
        let Self::Ready(ready) = self else {
            return;
        };
        ready.about_to_wait(event_loop);
        self.note_block(started);
    }
}

/// The window, GPU context and surface the first window ended up with.
struct PrimaryBootstrap {
    window: Arc<dyn winit::window::Window>,
    graphics: crate::HostedGpuShared,
    surface: HostedGpuSurface,
    target: crate::presentation::ResolvedSurfaceTarget,
    /// Whether this process can present through a platform compositor at all.
    /// Every later window asks its own target against this, rather than
    /// inheriting the first window's answer.
    composition: crate::presentation::CompositionAvailability,
    /// The material request this window was created with, and what the
    /// platform gave back for it. Carried out so the caller resolves its first
    /// presentation without asking the platform for the same thing twice.
    requested_material: crate::MaterialEffect,
    applied_material: MaterialOutcome,
}

/// Creates an embedded host's first window and binds it to a presentation
/// target on the embedder's device, falling back to the plain native path when
/// the composed one cannot be completed. A standalone host does the same in
/// [`startup::PendingStartup`], with the device requested off the event thread.
///
/// A composed window is created with `WS_EX_NOREDIRECTIONBITMAP`, and that is a
/// creation-time flag: winit derives it from its own `NO_BACK_BUFFER` and
/// rewrites the whole ex-style on every change, so nothing can add or remove it
/// afterwards. The window therefore has to be created for the target *before*
/// anything can find out whether that target works — the composition device,
/// its target for the HWND, the visual, the visual's GPU surface and the
/// premultiplied alpha that surface must negotiate are all downstream of it.
///
/// So the composed window is provisional. If any of those steps fails, its
/// composition objects are released, the window is dropped — which posts
/// winit's destroy message for that HWND — and a plain window is created in
/// its place. The failed HWND is never reused, and it cannot be: without a
/// redirection bitmap there is nothing for `DwmExtendFrameIntoClientArea` to
/// composite, which is the only per-pixel alpha a DX12 HWND swapchain has, so
/// a window kept from that attempt would silently be the one shape of window
/// that can never be transparent on either path. It was created hidden, so
/// nothing of it was ever on screen.
///
/// The outcome reaches the program through the resolved presentation rather
/// than only the log: see [`crate::SurfaceTargetFallback`].
fn bootstrap_primary_window(
    event_loop: &dyn ActiveEventLoop,
    settings: &WindowDescriptor,
    shared_gpu: crate::HostedGpuShared,
    policy: crate::GpuBackendPolicy,
    theme: crate::ThemeMode,
    material_mode: crate::MaterialEffect,
) -> Result<PrimaryBootstrap, String> {
    let bootstrap = gpu_bootstrap(policy, Some(&shared_gpu));
    let mut attempt = primary_surface_target(settings, policy, &bootstrap, material_mode)?;
    let mut composed_error = None;
    loop {
        match attach_primary_surface(
            event_loop,
            settings,
            shared_gpu.clone(),
            attempt.resolved,
            theme,
            material_mode,
        )
        .and_then(|bound| composed_fault(attempt).map_or(Ok(bound), Err))
        {
            Ok(PrimaryAttachment {
                window,
                graphics,
                surface,
                requested_material,
                applied_material,
            }) => {
                let composition =
                    settle_primary_target(attempt, policy, graphics.adapter_info().backend);
                return Ok(PrimaryBootstrap {
                    window,
                    graphics,
                    surface,
                    target: attempt,
                    composition,
                    requested_material,
                    applied_material,
                });
            }
            // The provisional window and its composition objects were dropped
            // by the failed attempt; the plain retry creates its own window
            // rather than adopting that HWND.
            Err(error) => {
                attempt = next_primary_target(attempt, error, &mut composed_error)?;
            }
        }
    }
}

/// The target the primary window starts on.
///
/// Two separate questions, in order. First: can this process present through
/// a platform compositor at all? That is the GPU backend's answer, it is
/// process-wide, and it has to be settled before any window exists because the
/// redirection bitmap is a creation-time flag. Second: does *this* window want
/// that path? That is per window, and every other window this process opens
/// asks it again for itself.
fn primary_surface_target(
    settings: &WindowDescriptor,
    policy: crate::GpuBackendPolicy,
    bootstrap: &crate::hosted_context::GpuBootstrap,
    material_mode: crate::MaterialEffect,
) -> Result<crate::presentation::ResolvedSurfaceTarget, String> {
    use crate::presentation::{resolve_window_surface_target, window_surface_request};
    let requested = window_surface_request(
        settings.surface,
        window_wants_transparent_surface(settings.transparent, material_mode),
        policy,
    );
    let target = resolve_window_surface_target(
        requested,
        settings.surface.requires_composition(),
        composition_availability(bootstrap),
    );
    match target.forbidden_fallback() {
        // The application said it would rather not start than present this
        // window another way.
        Some(reason) => Err(format!(
            "window requires a platform compositor surface: {}",
            reason.label()
        )),
        None => Ok(target),
    }
}

/// The target to try after `failed` could not be built. The plain path is the
/// last one there is: a failure there is a real startup failure, and it names
/// the composed attempt too when there was one.
fn next_primary_target(
    failed: crate::presentation::ResolvedSurfaceTarget,
    error: String,
    composed_error: &mut Option<String>,
) -> Result<crate::presentation::ResolvedSurfaceTarget, String> {
    match next_bootstrap_attempt(failed) {
        Some(next) => {
            *composed_error = Some(error);
            Ok(next)
        }
        None => Err(match composed_error.take() {
            Some(composed) => format!("{error} (after composition failed: {composed})"),
            None => error,
        }),
    }
}

/// Reports a fallback the primary window ended up on, and answers what later
/// windows can be composed against. A composed target that could not be built
/// for this window will not build for another, so that failure narrows the
/// whole process; otherwise the answer is the device the window ended up on.
fn settle_primary_target(
    target: crate::presentation::ResolvedSurfaceTarget,
    policy: crate::GpuBackendPolicy,
    backend: wgpu::Backend,
) -> crate::presentation::CompositionAvailability {
    use crate::presentation::CompositionAvailability;
    match target.fallback {
        Some(reason) => {
            eprintln!(
                "nana window surface: {}; presenting through the plain window path instead",
                reason.label()
            );
            nana_diagnostics::set_session_info("window.presentation_fallback", reason.label());
            CompositionAvailability::Unavailable
        }
        None => CompositionAvailability::for_backend(policy, backend),
    }
}

/// The composed path's failure branch is not reachable from a test that has
/// no way to make DirectComposition fail, so the acceptance probe asks for it
/// explicitly.
fn composed_fault(target: crate::presentation::ResolvedSurfaceTarget) -> Option<String> {
    target
        .resolved
        .composed()
        .then(composition_fault_injection)
        .flatten()
}

/// The GPU bootstrap this process starts from.
///
/// A process that never asked for a compositor-capable backend has not narrowed
/// its backend selection and must not be assumed to have got one, so nothing is
/// probed for it at all. An embedded host's device already exists, so there is
/// no narrowing left to do and the answer is simply what that device is.
fn gpu_bootstrap(
    policy: crate::GpuBackendPolicy,
    shared_gpu: Option<&crate::HostedGpuShared>,
) -> crate::hosted_context::GpuBootstrap {
    use crate::hosted_context::GpuBootstrap;
    if !policy.wants_composition() {
        return GpuBootstrap::plain();
    }
    GpuBootstrap::probe(shared_gpu.map(|gpu| gpu.adapter_info().backend))
}

fn composition_availability(
    bootstrap: &crate::hosted_context::GpuBootstrap,
) -> crate::presentation::CompositionAvailability {
    if bootstrap.composition_available() {
        crate::presentation::CompositionAvailability::Available
    } else {
        crate::presentation::CompositionAvailability::Unavailable
    }
}

/// The target to try after `attempt` failed, or `None` when the attempt was
/// already the plain native path and there is nothing left below it.
///
/// Shared by the first window and every window opened later: a composed target
/// is provisional wherever it is asked for, because the redirection bitmap is
/// decided before anything can find out whether the target works.
///
/// Keeping the sequencing here, off the window and GPU calls, is what lets the
/// fallback order be tested on a machine that has no DirectComposition at all.
pub(super) fn next_bootstrap_attempt(
    attempt: crate::presentation::ResolvedSurfaceTarget,
) -> Option<crate::presentation::ResolvedSurfaceTarget> {
    use crate::presentation::{ResolvedSurfaceTarget, SurfaceTargetFallback};
    // A window that requires the compositor has no next attempt: it asked to
    // fail rather than present another way.
    (attempt.resolved.composed() && !attempt.required).then(|| {
        ResolvedSurfaceTarget::fell_back(
            attempt.requested,
            SurfaceTargetFallback::TargetUnavailable,
            attempt.required,
        )
    })
}

/// Fault injection for the acceptance probe that has to exercise the composed
/// path's failure branch (`NANA_FORCE_COMPOSITION_FAILURE`). Nothing in the
/// product reads it; it exists so "the composition target could not be built"
/// is a state a real run can be put into on purpose.
fn composition_fault_injection() -> Option<String> {
    fault_flag("NANA_FORCE_COMPOSITION_FAILURE")
        .then(|| "composition failure requested by NANA_FORCE_COMPOSITION_FAILURE".to_owned())
}

/// A fault-injection switch for an acceptance probe: set, non-empty and not
/// `0`. Nothing in the product sets one.
fn fault_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty() && value != "0")
}

/// A window bound to one presentation target, and what its material request
/// resolved to on it.
struct PrimaryAttachment {
    window: Arc<dyn winit::window::Window>,
    graphics: crate::HostedGpuShared,
    surface: HostedGpuSurface,
    requested_material: crate::MaterialEffect,
    applied_material: MaterialOutcome,
}

/// One attempt at a presentation target: a window created for it and a surface
/// bound to it. The window is dropped — and with it the HWND — if the surface
/// cannot be built, so a failed attempt leaves nothing behind.
fn attach_primary_surface(
    event_loop: &dyn ActiveEventLoop,
    settings: &WindowDescriptor,
    graphics: crate::HostedGpuShared,
    target: WindowSurfaceTarget,
    theme: crate::ThemeMode,
    material_mode: crate::MaterialEffect,
) -> Result<PrimaryAttachment, String> {
    let (window, provisional, requested_material, applied_material) =
        create_primary_window(event_loop, settings, target, theme, material_mode)?;
    let want_transparent = requested_material.wants_transparent_surface();
    let surface = graphics
        .create_surface_with_mode(
            Arc::clone(&window),
            want_transparent,
            surface_mode_for(target),
        )
        .map_err(|error| error.to_string())?;
    // Kept: the material applied above belongs to the window that survived.
    provisional.keep();
    Ok(PrimaryAttachment {
        window,
        graphics,
        surface,
        requested_material,
        applied_material,
    })
}

/// A hidden primary window for one presentation target, with the default
/// material applied. The guard drops the window (and clears the material) if
/// the caller does not [`PendingNativeWindow::keep`] it.
fn create_primary_window(
    event_loop: &dyn ActiveEventLoop,
    settings: &WindowDescriptor,
    target: WindowSurfaceTarget,
    theme: crate::ThemeMode,
    material_mode: crate::MaterialEffect,
) -> Result<
    (
        Arc<dyn winit::window::Window>,
        PendingNativeWindow,
        crate::MaterialEffect,
        MaterialOutcome,
    ),
    String,
> {
    let window: Arc<dyn winit::window::Window> = Arc::from(
        event_loop
            .create_window(
                scene_window_attributes(
                    settings,
                    &scene_desktop(event_loop, settings.constrain_to_work_area),
                    target,
                )
                .with_visible(false),
            )
            .map_err(|error| format!("failed to create scene window: {error}"))?,
    );
    // The early splash can make this window visible before the first resolved
    // presentation is available. Arm the frameless style now so winit's first
    // show cannot expose the native frame around the splash.
    if !settings.system_caption {
        arm_frameless_guard(window.as_ref(), true);
    }
    // Holds the only other reference for the length of the attempt. An early
    // return drops this guard and the local `window` together, so the last
    // reference goes with them and the HWND created for a target that did not
    // work out is destroyed rather than reused.
    let provisional = PendingNativeWindow(Some(window.clone()));
    // The taskbar button shows as soon as the window does, which with a
    // splash is before the startup thread's icons arrive. The attributes above
    // already rendered this icon; resolving it again is a clone.
    #[cfg(target_os = "windows")]
    window.set_taskbar_icon(winit_icon(&resolved_scene_icon(settings.icon.as_ref())));
    // The program does not exist yet; `complete_startup` re-applies the
    // material with the host's own colour once it does.
    let (requested_material, applied_material) = apply_window_material(
        window.as_ref(),
        theme,
        settings,
        material_mode,
        AppearanceSettings::DEFAULT_BACKDROP_OPACITY,
        None,
    );
    Ok((window, provisional, requested_material, applied_material))
}

/// The persisted geometry applied to the primary descriptor, which is then
/// checked; returns the host's store.
fn prepare_primary_descriptor(settings: &mut WindowDescriptor) -> Result<SharedStore, String> {
    let store = crate::runtime_host::take_pending_store();
    restore_window_geometry(
        settings,
        &nana_ui_core::ViewStateStore::new(store.clone(), settings.restoration_scope.clone()),
    );
    crate::window_service::validate_descriptor(settings).map_err(|error| error.to_string())?;
    if settings.parent.is_some() {
        return Err("initial window cannot have a parent".into());
    }
    Ok(store)
}

/// An embedded host's startup: the embedder already owns the event loop and
/// the device, and has put nothing of NanaUI on screen, so there is no splash
/// and nothing to request off the event thread.
fn initialize<Program: RuntimeProgram>(
    event_loop: &dyn ActiveEventLoop,
    proxy: EventLoopProxy,
    message_tx: Sender<Program::Message>,
    message_rx: Receiver<Program::Message>,
    mut settings: WindowDescriptor,
    startup_failure: Arc<Mutex<Option<String>>>,
    shared_gpu: crate::HostedGpuShared,
) -> Result<WindowManager<Program>, String> {
    let store = prepare_primary_descriptor(&mut settings)?;
    let bootstrap = bootstrap_primary_window(
        event_loop,
        &settings,
        shared_gpu,
        Program::gpu_backend_policy(),
        crate::ThemeMode::default(),
        Program::startup_window_material_mode(),
    )?;
    let startup =
        startup::HostStartup::settled(crate::SplashOutcome::Skipped(crate::SplashSkip::Embedded));
    complete_startup(
        event_loop,
        StartupChannels {
            proxy,
            message_tx,
            message_rx,
            startup_failure,
        },
        settings,
        store,
        true,
        PrimaryStart {
            bootstrap,
            #[cfg(not(target_os = "android"))]
            accessibility: None,
            painter: None,
            icons: None,
            startup,
        },
    )
}

/// Where the program's messages and a startup failure go.
struct StartupChannels<Message> {
    proxy: EventLoopProxy,
    message_tx: Sender<Message>,
    message_rx: Receiver<Message>,
    startup_failure: Arc<Mutex<Option<String>>>,
}

/// A primary window bound to its device, and what the startup made for it
/// before the program existed.
struct PrimaryStart {
    bootstrap: PrimaryBootstrap,
    /// Created before the window was first shown, when it was shown early.
    #[cfg(not(target_os = "android"))]
    accessibility: Option<HostedAccessibility>,
    /// Built off the event thread together with the device.
    painter: Option<SceneWgpuPainter>,
    /// Rendered off the event thread while the device was requested.
    icons: Option<Receiver<SceneIcons>>,
    startup: startup::HostStartup,
}

/// Everything after the device exists: the program (`UiReady`), its material,
/// the host's window bookkeeping and the first show. Shared by the standalone
/// and the embedded host, so the two cannot drift apart.
fn complete_startup<Program: RuntimeProgram>(
    event_loop: &dyn ActiveEventLoop,
    channels: StartupChannels<Program::Message>,
    settings: WindowDescriptor,
    store: SharedStore,
    embedded: bool,
    start: PrimaryStart,
) -> Result<WindowManager<Program>, String> {
    let StartupChannels {
        proxy,
        message_tx,
        message_rx,
        startup_failure,
    } = channels;
    let PrimaryStart {
        bootstrap,
        #[cfg(not(target_os = "android"))]
            accessibility: early_accessibility,
        painter: early_painter,
        icons,
        startup: host_startup,
    } = start;
    let non_client = nana_window::NonClientRenderingStrategy::from_env();
    let PrimaryBootstrap {
        window,
        graphics,
        mut surface,
        target,
        composition: process_composition,
        mut requested_material,
        applied_material,
    } = bootstrap;
    // Declared after the window so that, on a failure below, the splash is
    // taken off before the window goes.
    let mut host_startup = host_startup;
    let pending_native = PendingNativeWindow(Some(window.clone()));
    // Icons rendered off the event thread are applied when they arrive
    // (`drain_host_work`); nothing waits for them.
    let pending_icons = match icons {
        Some(icons) => match icons.try_recv() {
            Ok(rendered) => {
                rendered.apply(window.as_ref());
                None
            }
            Err(mpsc::TryRecvError::Empty) => Some(icons),
            Err(mpsc::TryRecvError::Disconnected) => {
                apply_scene_window_icon(window.as_ref(), settings.icon.as_ref(), true);
                None
            }
        },
        None => {
            apply_scene_window_icon(window.as_ref(), settings.icon.as_ref(), true);
            None
        }
    };
    let format = surface.format();
    let mut presentation = ResolvedWindowPresentation::resolve(
        &settings,
        requested_material,
        applied_material,
        surface.wgpu_alpha_mode(),
        graphics.adapter_info().backend,
        target,
        non_client,
    );
    let host_work = Arc::new(schedule::HostWorkWake::new(proxy.clone()));
    let window_wake = Arc::clone(&host_work);
    let (windows, window_requests) =
        crate::WindowService::channel(Arc::new(move || window_wake.wake()));
    windows.register(WindowId::PRIMARY);
    let startup_wake = Arc::clone(&host_work);
    host_startup
        .handle
        .set_wake(Some(Arc::new(move || startup_wake.wake())));
    let tasks = spawn_task_workers(message_tx.clone(), Arc::clone(&host_work));
    // A view's future woken on a worker thread is polled on this thread.
    {
        let task_wake = Arc::clone(&host_work);
        nana_ui_runtime::view::set_task_wake(move || task_wake.wake());
    }
    let geometry = window_geometry(window.as_ref());
    let reduced_motion = nana_window::system_reduced_motion().unwrap_or(false);
    let context = program_context(
        message_tx.clone(),
        Arc::clone(&host_work),
        &graphics,
        WindowId::PRIMARY,
        geometry,
        tasks.clone(),
        presentation,
        CompositionWork::default(),
        window.theme().map(system_appearance_from_winit),
        &host_startup.handle,
    )
    .with_windows(&windows)
    .with_window_tag(settings.tag.clone())
    .with_reduced_motion(reduced_motion)
    .with_store(nana_ui_core::ProgramStore::share(Arc::clone(&store)));
    host_startup.ui_ready_begins();
    let (program, startup) = Program::initialize(&context).map_err(|error| error.to_string())?;
    // Locals drop in reverse order: if the remaining host setup fails, close
    // the inbox before the initialized program can join request-waiting workers.
    let startup_requests = window_requests;
    let last_theme = program.theme_mode();
    let last_material_mode = program.window_material_mode_for(WindowId::PRIMARY);
    let backdrop_opacity = program.appearance_backdrop_opacity_for(WindowId::PRIMARY);
    let window_background = program.window_background();
    let applied;
    (requested_material, applied) = apply_window_material(
        window.as_ref(),
        last_theme,
        &settings,
        last_material_mode,
        backdrop_opacity,
        window_background,
    );
    graphics
        .apply_surface_alpha_mode(
            &mut surface,
            window_wants_transparent_surface(settings.transparent, last_material_mode),
        )
        .map_err(|error| error.to_string())?;
    // The surface has answered, so the effective material and the chrome that
    // matches it are settled together, before the window is ever shown.
    presentation = ResolvedWindowPresentation::resolve(
        &settings,
        requested_material,
        applied,
        surface.wgpu_alpha_mode(),
        graphics.adapter_info().backend,
        target,
        non_client,
    );
    apply_resolved_presentation(
        window.as_ref(),
        last_theme,
        &settings,
        &presentation,
        backdrop_opacity,
        window_background,
        false,
    );
    #[cfg(not(target_os = "android"))]
    let accessibility = early_accessibility.or_else(|| {
        Some(HostedAccessibility::new(
            Arc::clone(&window),
            true,
            window.scale_factor() as f32,
        ))
    });
    let mut window_ids = HashMap::new();
    window_ids.insert(window.id(), WindowId::PRIMARY);
    let animation_clock = RuntimeAnimationClock::now();
    let skip_taskbar_report = windows::descriptor_skip_taskbar(window.as_ref(), &settings);
    let primary = WindowContext {
        surface_retry: None,
        applied_appearance: None,
        cursor_override: None,
        cursor_visible_override: None,
        passthrough_mode: MousePassthroughMode::Off,
        os_mouse_passthrough: false,
        material_override: None,
        surface,
        geometry,
        input: InputTracker::default(),
        input_source: WindowInputSource::new(
            nana_ui_platform::InputSourceId(WindowId::PRIMARY.0),
            nana_ui_platform::EndpointGeneration(1),
        ),
        presentation,
        settings: settings.clone(),
        #[cfg(not(target_os = "android"))]
        accessibility,
        accessibility_pending: PendingAccessibility::default(),
        size_move: LiveSizeMove::install(window.as_ref())?,
        level: if settings.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        },
        mode: None,
        pending_fullscreen: settings.fullscreen,
        native_controls_visible: true,
        native_controls: None,
        native_controls_box: std::cell::Cell::new(None),
        skip_taskbar: matches!(skip_taskbar_report, Some(Ok(()))),
        skip_taskbar_report,
        pointer_presence: presence::PointerPresence::default(),
        shadow: nana_window::shadow::WindowShadowState::default(),
        shadow_body: None,
    };
    pending_native.keep();
    let mut ready = WindowManager {
        program,
        embedded,
        gpu_backend_policy: Program::gpu_backend_policy(),
        non_client,
        composition: process_composition,
        shutting_down: false,
        wake_deadline: None,
        host_work: Arc::clone(&host_work),
        host_work_deadline: None,
        windows,
        window_requests: Some(startup_requests),
        next_window_id: 1 << 63,
        graphics,
        painters: HashMap::new(),
        native_renderers: HashMap::new(),
        text: NanaTextShaper::default(),
        proxy,
        message_tx,
        messages: message_rx,
        file_dialogs: dialogs::FileDialogs::default(),
        browsers: HashMap::new(),
        tasks,
        animation_clock,
        surface_generation: 0,
        frame_schedules: HashMap::new(),
        texture_subscriptions: HashMap::new(),
        texture_redraws: Arc::new(Mutex::new(HashSet::new())),
        image_targets: Arc::new(Mutex::new(HashMap::new())),
        image_window_keys: HashMap::new(),
        occluded: HashSet::new(),
        native_content: HashMap::new(),
        native_chrome_writes: std::cell::Cell::new(0),
        window_contexts: HashMap::from([(WindowId::PRIMARY, primary)]),
        window_ids,
        closing_windows: HashSet::new(),
        next_gpu_retry: None,
        render_suspended: false,
        last_theme,
        reduced_motion,
        settings,
        chrome: HashMap::new(),
        bind_after_present: HashSet::new(),
        startup_failure,
        program_store: nana_ui_core::ProgramStore::share(Arc::clone(&store)),
        store,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        live_frame_resize: None,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        live_frame_move: None,
        #[cfg(target_os = "macos")]
        present_transaction_pinned: HashSet::new(),
        startup: host_startup,
        input_generations: HashMap::from([(WindowId::PRIMARY, 1)]),
        clipboard: host_services::process_clipboard(),
        routed_input: Vec::new(),
        pending_icons,
        startup_messages: std::collections::VecDeque::new(),
    };
    ready
        .program
        .sync_animation_clock(ready.animation_clock.epoch());
    match early_painter {
        Some(painter) => ready.adopt_painter(format, painter),
        None => {
            let _ = ready.painter_mut(format);
        }
    }
    ready.prepare_window_chrome(
        WindowId::PRIMARY,
        ready.geometry_of(WindowId::PRIMARY).maximized,
    );
    crate::host_diagnostics::record_adapter(ready.graphics.adapter_info());
    crate::host_diagnostics::window_opened(
        WindowId::PRIMARY,
        &ready.geometry_of(WindowId::PRIMARY),
    );
    let shown_early = ready.startup.shown_early();
    // With the window already on screen behind its splash, the startup
    // messages are applied in batches over the next turns, and an immediate
    // takeover waits for them; otherwise they are applied before the show.
    let startup = if shown_early {
        ready.startup_messages = startup.into();
        ready.host_work.wake();
        Vec::new()
    } else {
        startup
    };
    let policy = ready.program.startup_takeover();
    ready.startup_ui_ready(policy);
    let update = ready.program.window_event(
        WindowEvent::Ready {
            id: WindowId::PRIMARY,
            geometry: ready.geometry_of(WindowId::PRIMARY),
        },
        &ready.context(),
    );
    ready.apply_update(event_loop, update, None);
    for message in startup {
        if event_loop.exiting() {
            break;
        }
        ready.process_message(event_loop, message);
    }
    if !event_loop.exiting() && ready.window(WindowId::PRIMARY).is_some() {
        if !shown_early {
            windows::set_native_visible(
                window.as_ref(),
                ready.settings.visible,
                ready.settings.focus_on_show,
            );
        }
        ready.reconcile_native_chrome(WindowId::PRIMARY);
        window.request_redraw();
        ready.finish_ready(event_loop, WindowId::PRIMARY);
    }
    Ok(ready)
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    fn update_image_targets(&mut self, id: WindowId, scene: &nana_ui_scene::UiScene) {
        let keys = scene_image_keys(scene);
        if self.image_window_keys.get(&id) == Some(&keys) {
            return;
        }
        if let Ok(mut targets) = self.image_targets.lock() {
            replace_image_target_index(&mut targets, &mut self.image_window_keys, id, keys);
        } else {
            self.image_window_keys.insert(id, keys);
        }
    }

    fn context(&self) -> RuntimeProgramContext<Program::Message> {
        self.context_for(
            self.window_contexts
                .keys()
                .copied()
                .min()
                .unwrap_or(WindowId::PRIMARY),
        )
    }

    fn context_for(&self, id: WindowId) -> RuntimeProgramContext<Program::Message> {
        program_context(
            self.message_tx.clone(),
            Arc::clone(&self.host_work),
            &self.graphics,
            id,
            self.geometry_of(id),
            self.tasks.clone(),
            self.presentation_of(id),
            self.composition_work_of(id),
            self.window(id)
                .and_then(|w| w.theme())
                .map(system_appearance_from_winit),
            &self.startup.handle,
        )
        .with_windows(&self.windows)
        .with_window_tag(
            self.window_contexts
                .get(&id)
                .and_then(|host| host.settings.tag.clone()),
        )
        .with_reduced_motion(self.reduced_motion)
        .with_store(Arc::clone(&self.program_store))
    }

    fn apply_update(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        update: RuntimeProgramUpdate,
        painting: Option<WindowId>,
    ) {
        if self.shutting_down {
            return;
        }
        if update.exit {
            self.shutting_down = true;
            // Reject requests made by close callbacks and resolve queued futures
            // before application-owned state is released.
            self.window_requests.take();
            self.browsers.clear();
            self.close_all_file_dialogs();
            let ids = self.known_window_ids();
            for id in ids {
                self.close_window(event_loop, id);
            }
            if !self.embedded {
                event_loop.exit();
            }
            return;
        }
        for command in update.window_commands {
            // Move/SetBounds only request the native operation here. The OS
            // Moved/Resized event later refreshes cached geometry and records
            // it through `sync_geometry`; persistence never belongs on this
            // synchronous update path.
            self.apply_window_command(event_loop, command);
            if event_loop.exiting() {
                return;
            }
        }
        self.reconcile_browser_lifetimes();
        for id in windows_to_redraw(update.redraw, &self.known_window_ids()) {
            if painting == Some(id) {
                continue;
            }
            self.request_redraw(id);
        }
        // A program update, timer or other window may have written signals a
        // view in this window reads; its bindings apply at the next frame, so
        // that frame must be asked for.
        self.request_reactive_redraws(painting);
    }

    /// Ask for a frame of every window (but `painting`) with bindings to
    /// apply.
    fn request_reactive_redraws(&mut self, painting: Option<WindowId>) {
        for id in self.known_window_ids() {
            if painting != Some(id)
                && self
                    .program
                    .read_document(id, |document| document.context().has_pending_reactive())
                    .unwrap_or(false)
            {
                self.request_redraw(id);
            }
        }
    }

    fn known_window_ids(&self) -> Vec<WindowId> {
        self.window_contexts.keys().copied().collect()
    }

    fn window(&self, id: WindowId) -> Option<&Arc<dyn winit::window::Window>> {
        self.window_contexts
            .get(&id)
            .map(|host| host.surface.window())
    }

    fn geometry_of(&self, id: WindowId) -> WindowGeometry {
        self.window_contexts
            .get(&id)
            .map(|host| host.geometry)
            .unwrap_or_default()
    }

    /// What `id` presents, and how. A window that is gone presents nothing, so
    /// it reports a plain opaque window rather than the last live window's
    /// state: there is one authority per window and no process-wide copy of it
    /// to go stale.
    fn presentation_of(&self, id: WindowId) -> ResolvedWindowPresentation {
        self.window_contexts
            .get(&id)
            .map_or_else(ResolvedWindowPresentation::closed, |host| host.presentation)
    }

    fn material_of(&self, id: WindowId) -> MaterialOutcome {
        self.presentation_of(id).effective()
    }

    /// What mirroring `id`'s scene into the platform compositor has cost.
    fn composition_work_of(&self, id: WindowId) -> CompositionWork {
        #[allow(unused_mut)]
        let mut work = CompositionWork {
            native_content: self
                .native_content
                .get(&id)
                .map(crate::native_content::NativeContentMirror::work)
                .unwrap_or_default(),
            commits: 0,
            tree_mutations: 0,
            native_chrome_writes: self.native_chrome_writes.get(),
        };
        #[cfg(target_os = "windows")]
        if let Some(composition) = self
            .window_contexts
            .get(&id)
            .and_then(|host| host.surface.windows_composition())
        {
            let composed = composition.work();
            work.commits = composed.commits;
            work.tree_mutations = composed.tree_mutations;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = id;
        work
    }

    fn request_redraw(&self, id: WindowId) {
        if let Some(window) = self.window(id) {
            window.request_redraw();
        }
    }

    fn request_redraw_all(&self) {
        for id in self.known_window_ids() {
            self.request_redraw(id);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Host-owned resources form one callback context"
)]
fn program_context<Message: Send + 'static>(
    message_tx: Sender<Message>,
    wake: Arc<schedule::HostWorkWake>,
    graphics: &crate::HostedGpuShared,
    id: WindowId,
    geometry: WindowGeometry,
    tasks: SyncSender<Task<Message>>,
    presentation: ResolvedWindowPresentation,
    composition_work: CompositionWork,
    appearance: Option<nana_ui_platform::SystemAppearance>,
    startup: &crate::StartupHandle,
) -> RuntimeProgramContext<Message> {
    RuntimeProgramContext::new(
        id,
        geometry,
        graphics.gpu().clone(),
        presentation,
        composition_work,
        Arc::new(move |message| {
            if message_tx.send(message).is_ok() {
                wake.wake();
            }
        }),
        tasks,
        // System-wide preference: sampling the primary window is enough, and
        // the window handle itself never crosses this boundary.
        appearance,
        startup.clone(),
    )
}

fn spawn_task_workers<Message: Send + 'static>(
    message_tx: Sender<Message>,
    wake: Arc<schedule::HostWorkWake>,
) -> SyncSender<Task<Message>> {
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Task<Message>>(TASK_QUEUE_CAPACITY);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..TASK_WORKERS {
        let receiver = Arc::clone(&receiver);
        let message_tx = message_tx.clone();
        let wake = Arc::clone(&wake);
        std::thread::spawn(move || {
            loop {
                let task = {
                    let Ok(receiver) = receiver.lock() else {
                        return;
                    };
                    let Ok(task) = receiver.recv() else {
                        return;
                    };
                    task
                };
                let message = pollster::block_on(task.into_future());
                if message_tx.send(message).is_err() {
                    return;
                }
                wake.wake();
            }
        });
    }
    sender
}

fn accessibility_snapshot<Program: RuntimeProgram>(
    program: &mut Program,
    id: WindowId,
) -> Vec<nana_ui_runtime::AccessibilityNode> {
    program
        .read_document(id, |document| {
            document
                .context()
                .world()
                .project_accessibility(document.document())
        })
        .unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
fn accessibility_world_generation<Program: RuntimeProgram>(
    program: &mut Program,
    id: WindowId,
) -> Option<u64> {
    program.read_document(id, |document| document.context().world().generation())
}

#[cfg(not(target_os = "android"))]
fn next_accessibility_update(
    flush: Option<AccessibilityUpdate>,
    program: Option<AccessibilityUpdate>,
    scale_factor_changed: bool,
    projector_generation: Option<u64>,
    world_generation: Option<u64>,
    snapshot: impl FnOnce() -> Vec<nana_ui_runtime::AccessibilityNode>,
) -> Option<AccessibilityUpdate> {
    if scale_factor_changed {
        return Some(AccessibilityUpdate::Full {
            generation: world_generation,
            nodes: snapshot(),
        });
    }
    if flush.is_some() && program.is_some() {
        return Some(AccessibilityUpdate::Full {
            generation: world_generation,
            nodes: snapshot(),
        });
    }
    if let Some(update) = flush.or(program) {
        let queued = match &update {
            AccessibilityUpdate::Full { generation, .. } => *generation,
            AccessibilityUpdate::Delta(delta) => Some(delta.generation),
        };
        if (projector_generation.is_none() && matches!(&update, AccessibilityUpdate::Delta(_)))
            || world_generation.is_some_and(|world| queued.is_some_and(|queued| queued < world))
        {
            // A cold adapter has no base tree. The application may already
            // have drained initial work, leaving only a partial frame delta.
            return Some(AccessibilityUpdate::Full {
                generation: world_generation,
                nodes: snapshot(),
            });
        }
        return Some(update);
    }
    if projector_generation.is_some() && projector_generation == world_generation {
        return None;
    }
    Some(AccessibilityUpdate::Full {
        generation: world_generation,
        nodes: snapshot(),
    })
}

fn window_surface_effect(
    settings_transparent: bool,
    appearance: crate::MaterialEffect,
) -> crate::MaterialEffect {
    if settings_transparent {
        crate::MaterialEffect::Transparent
    } else {
        appearance
    }
}

fn window_wants_transparent_surface(
    settings_transparent: bool,
    appearance: crate::MaterialEffect,
) -> bool {
    window_surface_effect(settings_transparent, appearance).wants_transparent_surface()
}

fn apply_scene_material(
    window: &dyn winit::window::Window,
    theme: crate::ThemeMode,
    requested: crate::MaterialEffect,
    backdrop_opacity: f32,
    window_background: Option<nana_ui_core::SemanticColor>,
) -> MaterialOutcome {
    let appearance = match theme {
        crate::ThemeMode::Dark => Appearance::Dark,
        crate::ThemeMode::Light => Appearance::Light,
    };
    let (red, green, blue, _) = window_background
        .unwrap_or_else(|| theme.palette().background)
        .to_u8_rgba();
    let alpha = (AppearanceSettings::clamp_backdrop_opacity(backdrop_opacity) * 255.0 + 0.5) as u8;
    apply_hosted_system_material(
        window,
        requested,
        appearance,
        FallbackColor::rgba(red, green, blue, alpha),
    )
}

fn apply_window_transparency(window: &dyn winit::window::Window, requested: crate::MaterialEffect) {
    // The window's shadow is applied once it is shown, by its
    // `WindowShadowState` (`WindowManager::apply_window_shadow`).
    window.set_transparent(requested.wants_transparent_surface());
}

/// Asks the platform for the window's requested material.
///
/// Native chrome is deliberately *not* written here. Chrome follows the
/// effective presentation, and that is only known once the surface has
/// negotiated its alpha — see [`ResolvedWindowPresentation`] and
/// [`apply_resolved_presentation`].
fn apply_window_material(
    window: &dyn winit::window::Window,
    theme: crate::ThemeMode,
    settings: &WindowDescriptor,
    appearance: crate::MaterialEffect,
    backdrop_opacity: f32,
    window_background: Option<nana_ui_core::SemanticColor>,
) -> (crate::MaterialEffect, MaterialOutcome) {
    let requested = window_surface_effect(settings.transparent, appearance);
    let material = apply_scene_material(
        window,
        theme,
        requested,
        backdrop_opacity,
        window_background,
    );
    apply_window_transparency(window, requested);
    (requested, material)
}

/// Writes every native consequence of a resolved presentation.
///
/// This is the one place that touches the window's material state and its
/// non-client chrome once the surface has answered, so the two cannot disagree:
///
/// - a request that was demoted has its native material undone, because the
///   glass a `Transparent` request extended across the client must not stay on
///   a window that now presents `Solid`;
/// - the frame styles, corner preference and border stroke come from
///   `presentation.chrome()`, which was derived from the effective material.
///
/// Every host call site must record the write with
/// [`WindowManager::note_native_chrome_write`], so the steady-state gate reads
/// a number that includes it — a chrome write nothing counted would let the
/// contract report zero while a window rewrote its frame styles every frame.
/// The one exception is the first window's own creation, which happens before
/// the host exists and before any measurement starts.
fn apply_resolved_presentation(
    window: &dyn winit::window::Window,
    theme: crate::ThemeMode,
    settings: &WindowDescriptor,
    presentation: &ResolvedWindowPresentation,
    backdrop_opacity: f32,
    window_background: Option<nana_ui_core::SemanticColor>,
    allow_caption_change: bool,
) {
    if presentation.needs_material_reset() {
        // The outcome is already recorded on the presentation; this call is
        // only here to put the platform back where that outcome says it is.
        let _ = apply_scene_material(
            window,
            theme,
            presentation.effective().effect,
            backdrop_opacity,
            window_background,
        );
        apply_window_transparency(window, presentation.effective().effect);
    }
    apply_native_chrome(window, settings, presentation, allow_caption_change);
}

/// Starts a native window drag; `Ok` means the drag started and the platform
/// now owns the button release.
fn drag_scene_window(window: &dyn winit::window::Window) -> Result<(), winit::error::RequestError> {
    if nana_window::drag_custom_title_bar(window) {
        return Ok(());
    }
    // winit's AppKit drag uses `currentEvent` without checking it is still the
    // press, so it would report a drag AppKit ignored as started.
    #[cfg(target_os = "macos")]
    {
        Err(winit::error::RequestError::Ignored)
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.drag_window()
    }
}

fn resize_scene_window(window: &dyn winit::window::Window, edge: WindowResizeEdge) {
    if resize_custom_frame(window, frame_resize_edge(edge)) {
        return;
    }
    let _ = window.drag_resize_window(match edge {
        WindowResizeEdge::North => winit::window::ResizeDirection::North,
        WindowResizeEdge::South => winit::window::ResizeDirection::South,
        WindowResizeEdge::East => winit::window::ResizeDirection::East,
        WindowResizeEdge::West => winit::window::ResizeDirection::West,
        WindowResizeEdge::NorthEast => winit::window::ResizeDirection::NorthEast,
        WindowResizeEdge::NorthWest => winit::window::ResizeDirection::NorthWest,
        WindowResizeEdge::SouthEast => winit::window::ResizeDirection::SouthEast,
        WindowResizeEdge::SouthWest => winit::window::ResizeDirection::SouthWest,
    });
}

fn frame_resize_edge(edge: WindowResizeEdge) -> FrameResizeEdge {
    match edge {
        WindowResizeEdge::North => FrameResizeEdge::North,
        WindowResizeEdge::South => FrameResizeEdge::South,
        WindowResizeEdge::East => FrameResizeEdge::East,
        WindowResizeEdge::West => FrameResizeEdge::West,
        WindowResizeEdge::NorthEast => FrameResizeEdge::NorthEast,
        WindowResizeEdge::NorthWest => FrameResizeEdge::NorthWest,
        WindowResizeEdge::SouthEast => FrameResizeEdge::SouthEast,
        WindowResizeEdge::SouthWest => FrameResizeEdge::SouthWest,
    }
}

fn frame_resize_edge_for(
    settings: &WindowDescriptor,
    geometry: &WindowGeometry,
    fullscreen: bool,
    x: f32,
    y: f32,
) -> Option<WindowResizeEdge> {
    if settings.system_caption || !settings.resizable || geometry.maximized || fullscreen {
        return None;
    }
    window_resize_edge(geometry.logical_size, x, y, RESIZE_HANDLE_SIZE)
}

fn window_cursor_override(cursor: crate::WindowCursor) -> (Option<CursorIcon>, Option<bool>) {
    use crate::WindowCursor;
    match cursor {
        WindowCursor::Automatic => (None, None),
        WindowCursor::Default => (Some(CursorIcon::Default), None),
        WindowCursor::Pointer => (Some(CursorIcon::Pointer), None),
        WindowCursor::Text => (Some(CursorIcon::Text), None),
        WindowCursor::Move => (Some(CursorIcon::Move), None),
        WindowCursor::Grab => (Some(CursorIcon::Grab), None),
        WindowCursor::Grabbing => (Some(CursorIcon::Grabbing), None),
        WindowCursor::NotAllowed => (Some(CursorIcon::NotAllowed), None),
        WindowCursor::Crosshair => (Some(CursorIcon::Crosshair), None),
        WindowCursor::Help => (Some(CursorIcon::Help), None),
        WindowCursor::Wait => (Some(CursorIcon::Wait), None),
        WindowCursor::Progress => (Some(CursorIcon::Progress), None),
        WindowCursor::ZoomIn => (Some(CursorIcon::ZoomIn), None),
        WindowCursor::ZoomOut => (Some(CursorIcon::ZoomOut), None),
        WindowCursor::None => (Some(CursorIcon::Default), Some(false)),
    }
}

/// The resize cursor a window frame edge shows.
fn frame_edge_cursor(edge: WindowResizeEdge) -> CursorIcon {
    match edge {
        WindowResizeEdge::East | WindowResizeEdge::West => CursorIcon::EwResize,
        WindowResizeEdge::North | WindowResizeEdge::South => CursorIcon::NsResize,
        WindowResizeEdge::NorthEast | WindowResizeEdge::SouthWest => CursorIcon::NeswResize,
        WindowResizeEdge::NorthWest | WindowResizeEdge::SouthEast => CursorIcon::NwseResize,
    }
}

fn scene_paint_viewport(
    geometry: &WindowGeometry,
    material: MaterialOutcome,
    theme: crate::ThemeMode,
    window_background: Option<nana_ui_core::SemanticColor>,
) -> ScenePaintViewport {
    ScenePaintViewport {
        logical_size: [geometry.logical_size.0, geometry.logical_size.1],
        physical_size: [geometry.physical_size.0, geometry.physical_size.1],
        scale_factor: geometry.scale_factor,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: scene_clear_color(theme, material, window_background),
        clear: true,
    }
}

fn scene_clear_color(
    theme: crate::ThemeMode,
    material: MaterialOutcome,
    window_background: Option<nana_ui_core::SemanticColor>,
) -> [f32; 4] {
    if material.wants_transparent_surface() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    // Theme and host colours are sRGB; the painter's clear, like every quad it
    // draws, is linear (`ScenePaintViewport::clear_color`). Passing them
    // through unconverted encoded the palette's #181818 twice and cleared an
    // uncovered window to #565656 — the same colour an Early Splash in the
    // system background could not match.
    let color = window_background.unwrap_or_else(|| theme.palette().background);
    crate::scene_paint::pack_linear([color.r, color.g, color.b, color.a])
}

/// Scale between desktop pixels and the global logical space shared by
/// `WindowDescriptor::initial_position`, `WindowGeometry::logical_position`
/// and display bounds. macOS positions are points, already global. Elsewhere
/// the desktop is one physical pixel grid, so one scale for every position (the
/// primary display's) keeps logical positions unambiguous when displays use
/// different scale factors.
fn desktop_scale(own_scale: f64, primary_scale: Option<f64>) -> f64 {
    #[cfg(target_os = "macos")]
    {
        let _ = primary_scale;
        own_scale
    }
    #[cfg(not(target_os = "macos"))]
    primary_scale
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(own_scale)
}

/// Scale of the display that defines the desktop space: the primary one, or the
/// first listed when the platform names no primary, so every conversion agrees.
fn window_reference_scale(window: &dyn winit::window::Window) -> Option<f64> {
    window
        .primary_monitor()
        .or_else(|| window.available_monitors().next())
        .map(|monitor| monitor.scale_factor())
}

fn desktop_position(position: (f64, f64), scale: f64) -> winit::dpi::Position {
    #[cfg(target_os = "macos")]
    {
        let _ = scale;
        winit::dpi::LogicalPosition::new(position.0, position.1).into()
    }
    #[cfg(not(target_os = "macos"))]
    winit::dpi::PhysicalPosition::new(
        (position.0 * scale).round() as i32,
        (position.1 * scale).round() as i32,
    )
    .into()
}

/// Live displays in the global logical space and the scale that defines it.
struct Desktop {
    displays: Vec<DisplayBounds>,
    /// Per display, desktop units per logical unit of a window on it: its own
    /// scale over the desktop scale (1 on macOS). A missing entry counts as 1.
    size_ratios: Vec<f64>,
    scale: f64,
}

impl Desktop {
    /// Size ratio of the display holding `position`, or the nearest one.
    fn size_ratio_at(&self, position: (f64, f64)) -> f64 {
        let distance = |display: &DisplayBounds| {
            let dx = (display.position.0 - position.0)
                .max(position.0 - (display.position.0 + display.size.0))
                .max(0.0);
            let dy = (display.position.1 - position.1)
                .max(position.1 - (display.position.1 + display.size.1))
                .max(0.0);
            dx * dx + dy * dy
        };
        self.displays
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
            .and_then(|(index, _)| self.size_ratios.get(index).copied())
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
            .unwrap_or(1.0)
    }
}

fn scene_window_attributes(
    settings: &WindowDescriptor,
    desktop: &Desktop,
    target: WindowSurfaceTarget,
) -> winit::window::WindowAttributes {
    let displays = desktop.displays.as_slice();
    let mut settings = settings.clone();
    if settings.constrain_to_work_area {
        let position = settings.initial_position.unwrap_or_else(|| {
            displays
                .first()
                .map_or((0.0, 0.0), |display| display.position)
        });
        // Bounds are desktop units; the size is logical on the target display.
        let ratio = desktop.size_ratio_at(position);
        let (position, size) = nana_ui_platform::fit_window_to_displays(
            position,
            (
                settings.initial_size.0 * ratio,
                settings.initial_size.1 * ratio,
            ),
            displays,
        );
        let size = (size.0 / ratio, size.1 / ratio);
        settings.initial_position = Some(position);
        settings.initial_size = size;
        settings.minimum_size = (
            settings.minimum_size.0.min(size.0),
            settings.minimum_size.1.min(size.1),
        );
    }
    let mut attributes = winit::window::WindowAttributes::default()
        .with_title(settings.title.clone())
        .with_transparent(settings.transparent)
        .with_active(settings.focus_on_show)
        .with_resizable(settings.resizable)
        .with_window_level(window_level(settings.always_on_top))
        .with_surface_size(winit::dpi::LogicalSize::new(
            settings.initial_size.0,
            settings.initial_size.1,
        ))
        .with_min_surface_size(winit::dpi::LogicalSize::new(
            settings.minimum_size.0,
            settings.minimum_size.1,
        ))
        .with_maximized(settings.maximized);
    if let Some((x, y)) = settings.initial_position {
        let ratio = desktop.size_ratio_at((x, y));
        let size = (
            settings.initial_size.0 * ratio,
            settings.initial_size.1 * ratio,
        );
        let position = clamp_position_to_displays((x, y), size, displays);
        attributes = attributes.with_position(desktop_position(position, desktop.scale));
    }
    // winit ignores window icons on macOS, where rasterizing the default one
    // is the longest step before the window exists; the Dock icon is applied
    // separately.
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = winit_icon(&resolved_scene_icon(settings.icon.as_ref())) {
        attributes = attributes.with_window_icon(Some(icon));
    }

    apply_scene_window_chrome(attributes, &settings, target)
}

/// Live display bounds in the global logical coordinate space, matching the
/// coordinate space of `WindowDescriptor::initial_position`.
fn scene_desktop(event_loop: &dyn ActiveEventLoop, work_area: bool) -> Desktop {
    let infos = display::display_infos(event_loop);
    // Same reference as `window_reference_scale`: primary, else first listed.
    let primary = infos
        .iter()
        .find(|display| display.primary)
        .or(infos.first())
        .map(|display| display.scale_factor);
    let scale = desktop_scale(
        infos.first().map_or(1.0, |display| display.scale_factor),
        primary,
    );
    let (displays, size_ratios) = infos
        .into_iter()
        .filter_map(|mut display| {
            if work_area
                && let Some((position, size)) = display
                    .physical_position
                    .and_then(nana_window::display_work_area)
            {
                display.physical_position = Some(position);
                display.physical_size = Some(size);
            }
            let display_desktop_scale = desktop_scale(display.scale_factor, primary);
            display
                .logical_bounds(display_desktop_scale)
                .map(|bounds| (bounds, display.scale_factor / display_desktop_scale))
        })
        .unzip();
    Desktop {
        displays,
        size_ratios,
        scale,
    }
}

fn resolved_scene_icon(per_window: Option<&WindowIcon>) -> WindowIcon {
    nana_app_icon::resolved_application_icon(per_window)
}

fn winit_icon(icon: &WindowIcon) -> Option<Icon> {
    RgbaIcon::new(icon.rgba.clone(), icon.width, icon.height)
        .ok()
        .map(Icon::from)
}

fn apply_scene_window_icon(
    window: &dyn winit::window::Window,
    per_window: Option<&WindowIcon>,
    apply_app_icon: bool,
) {
    SceneIcons::render(per_window, apply_app_icon).apply(window);
}

/// The application (Dock) icon alone, for when no window is left to carry it.
fn apply_application_icon() {
    #[cfg(target_os = "macos")]
    if let Some(png) = SceneIcons::render(None, true).application_png {
        set_application_icon_png(&png);
    }
}

/// A window's icons, rendered. Rendering the default mark — and on macOS the
/// Dock icon's padded PNG — is the expensive part and needs no window, so a
/// startup renders them on a thread while the device is requested; applying
/// them is cheap.
struct SceneIcons {
    window: WindowIcon,
    #[cfg(target_os = "macos")]
    application_png: Option<Vec<u8>>,
}

impl SceneIcons {
    fn render(per_window: Option<&WindowIcon>, application: bool) -> Self {
        let window = resolved_scene_icon(per_window);
        #[cfg(target_os = "macos")]
        let application_png = application
            .then(|| {
                let icon = nana_app_icon::with_system_grid(&window);
                nana_app_icon::encode_png(icon.width, icon.height, &icon.rgba).ok()
            })
            .flatten();
        #[cfg(not(target_os = "macos"))]
        let _ = application;
        Self {
            window,
            #[cfg(target_os = "macos")]
            application_png,
        }
    }

    fn apply(&self, window: &dyn winit::window::Window) {
        // winit 的 Win32 后端把共享的 RGBA 缓冲原地 R/B 翻转成 BGRA;同一 Icon
        // 转换第二次会把颜色换回去,所以每个入口都拿到独立缓冲,恰好转换一次。
        window.set_window_icon(winit_icon(&self.window));
        #[cfg(target_os = "windows")]
        window.set_taskbar_icon(winit_icon(&self.window));
        #[cfg(target_os = "macos")]
        if let Some(png) = self.application_png.as_deref() {
            set_application_icon_png(png);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(all(target_os = "macos", not(test)), allow(dead_code))]
struct WindowsSceneChrome {
    decorations: bool,
    undecorated_shadow: bool,
    no_redirection_bitmap: bool,
    rounded_corners: bool,
}

#[cfg_attr(all(target_os = "macos", not(test)), allow(dead_code))]
fn windows_scene_chrome(
    system_caption: bool,
    transparent: bool,
    target: WindowSurfaceTarget,
) -> WindowsSceneChrome {
    WindowsSceneChrome {
        decorations: system_caption,
        // winit's undecorated-shadow path insets the client by 1px on the top
        // so DWM can attach a drop shadow. That leaves a strip the title bar
        // cannot paint. Windows 11 rounded corners already provide a shadow.
        // Transparent overlays skip rounding so DWM does not stroke a rectangle.
        undecorated_shadow: false,
        // The redirection bitmap belongs to the presentation path, not to the
        // material: DirectComposition draws its visual over that bitmap, so a
        // composed window has to be created without one or an opaque surface
        // shows through underneath. A window on the plain path keeps it — its
        // swapchain may well be presenting into it.
        no_redirection_bitmap: target.composed(),
        rounded_corners: !system_caption && !transparent,
    }
}

/// The surface mode that reaches a presentation target.
const fn surface_mode_for(target: WindowSurfaceTarget) -> crate::HostedSurfaceMode {
    #[cfg(target_os = "windows")]
    {
        match target {
            WindowSurfaceTarget::Composition => crate::HostedSurfaceMode::WindowsComposition,
            WindowSurfaceTarget::NativeWindow => crate::HostedSurfaceMode::Window,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        crate::HostedSurfaceMode::Window
    }
}

/// Writes the native chrome a resolved presentation requires.
///
/// Re-apply after any winit call that rewrites native window style. The policy
/// is read off the presentation, never re-derived: `presentation.chrome()` was
/// settled from the effective material when the presentation was resolved.
///
/// `allow_caption_change` is false while the host is still inside
/// `can_create_surfaces`, which this crate has long kept free of
/// `SetWindowPos(SWP_FRAMECHANGED)` on the grounds that it hangs the UI thread.
/// That attribution has never been re-confirmed, and this path no longer rests
/// on it: the guard is armed either way, so nothing is deferred. Showing the
/// window rewrites the whole style through winit and the strip lands on that
/// write, before the window has been presented once, so writing it earlier
/// would only add a frame change nobody reads.
fn apply_native_chrome<W: HasWindowHandle + ?Sized>(
    window: &W,
    settings: &WindowDescriptor,
    presentation: &ResolvedWindowPresentation,
    allow_caption_change: bool,
) {
    let Some(chrome) = presentation.chrome() else {
        return;
    };
    let _ = prepare_client_chrome(window, f64::from(TITLE_BAR_HEIGHT), chrome.rounded_corners);
    // Both directions are written: a window whose material flips at runtime
    // would otherwise keep whichever policy it was last given.
    let _ = nana_window::set_non_client_rendering(window, chrome.non_client_rendering_enabled());
    let _ = if allow_caption_change {
        set_frameless_styles(window, chrome.frameless(), settings.resizable)
    } else {
        arm_frameless_guard(window, chrome.frameless())
    };
}

fn apply_scene_window_chrome(
    attributes: winit::window::WindowAttributes,
    settings: &WindowDescriptor,
    target: WindowSurfaceTarget,
) -> winit::window::WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        // The presentation target only changes chrome on Windows, where it
        // decides the redirection bitmap and the undecorated shadow. macOS
        // reads `system_caption` alone.
        let _ = target;
        if settings.system_caption {
            return attributes.with_decorations(true);
        }
        attributes
            .with_decorations(true)
            .with_platform_attributes(Box::new(
                WindowAttributesMacOS::default()
                    .with_titlebar_transparent(true)
                    .with_fullsize_content_view(true)
                    .with_title_hidden(true)
                    .with_movable_by_window_background(false),
            ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let chrome = windows_scene_chrome(settings.system_caption, settings.transparent, target);
        let attributes = attributes.with_decorations(chrome.decorations);
        #[cfg(target_os = "windows")]
        let attributes = {
            let mut win = WindowAttributesWindows::default()
                .with_no_redirection_bitmap(chrome.no_redirection_bitmap)
                .with_undecorated_shadow(chrome.undecorated_shadow);
            if !chrome.decorations {
                win = win.with_corner_preference(if chrome.rounded_corners {
                    CornerPreference::Round
                } else {
                    CornerPreference::DoNotRound
                });
            }
            if let Some(icon) = winit_icon(&resolved_scene_icon(settings.icon.as_ref())) {
                win = win.with_taskbar_icon(Some(icon));
            }
            attributes.with_platform_attributes(Box::new(win))
        };
        attributes
    }
}

fn scene_aux_window_attributes(
    settings: &WindowDescriptor,
    parent: Option<&dyn winit::window::Window>,
    desktop: &Desktop,
    target: WindowSurfaceTarget,
) -> Result<winit::window::WindowAttributes, String> {
    let attributes = scene_window_attributes(settings, desktop, target).with_visible(false);
    if settings.modal && parent.is_none() {
        return Err("modal window requires a parent".into());
    }
    // Any child is owned by its parent HWND, modal or not: it stays above the
    // parent, minimizes with it and never gets its own taskbar button.
    #[cfg(target_os = "windows")]
    let attributes = if let Some(parent) = parent {
        let handle = parent
            .window_handle()
            .map_err(|error| format!("failed to acquire owner handle: {error}"))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("Windows owner is not an HWND".into());
        };
        {
            let chrome =
                windows_scene_chrome(settings.system_caption, settings.transparent, target);
            let mut win = WindowAttributesWindows::default()
                .with_no_redirection_bitmap(chrome.no_redirection_bitmap)
                .with_undecorated_shadow(chrome.undecorated_shadow)
                .with_owner_window(handle.hwnd.get() as _);
            if !chrome.decorations {
                win = win.with_corner_preference(if chrome.rounded_corners {
                    CornerPreference::Round
                } else {
                    CornerPreference::DoNotRound
                });
            }
            if let Some(icon) = winit_icon(&resolved_scene_icon(settings.icon.as_ref())) {
                win = win.with_taskbar_icon(Some(icon));
            }
            attributes.with_platform_attributes(Box::new(win))
        }
    } else {
        attributes
    };
    Ok(attributes)
}

fn allows_modal_parent_event(event: &WinitWindowEvent) -> bool {
    matches!(
        event,
        WinitWindowEvent::RedrawRequested
            | WinitWindowEvent::SurfaceResized(_)
            | WinitWindowEvent::Moved(_)
            | WinitWindowEvent::ScaleFactorChanged { .. }
            | WinitWindowEvent::Occluded(_)
            | WinitWindowEvent::Destroyed
    )
}

/// Host-owned Forward passthrough: OS pointer must not reach widgets until
/// sampling has recovered hit-testing. Down is never synthesized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForwardPointerAction {
    Dispatch,
    RestorePassthrough,
    IgnoreUntilRecovered,
}

fn forward_pointer_event(event: &WinitWindowEvent) -> bool {
    matches!(
        event,
        WinitWindowEvent::PointerMoved { .. }
            | WinitWindowEvent::PointerEntered { .. }
            | WinitWindowEvent::PointerLeft { .. }
            | WinitWindowEvent::PointerButton { .. }
    )
}

fn forward_pointer_action(
    mode: MousePassthroughMode,
    os_passthrough: bool,
    hits_content: bool,
    event: &WinitWindowEvent,
) -> ForwardPointerAction {
    if mode != MousePassthroughMode::Forward || !forward_pointer_event(event) {
        return ForwardPointerAction::Dispatch;
    }
    if os_passthrough {
        return ForwardPointerAction::IgnoreUntilRecovered;
    }
    match event {
        WinitWindowEvent::PointerLeft { .. } => ForwardPointerAction::RestorePassthrough,
        WinitWindowEvent::PointerMoved { .. }
        | WinitWindowEvent::PointerEntered { .. }
        | WinitWindowEvent::PointerButton { .. } => {
            if hits_content {
                ForwardPointerAction::Dispatch
            } else {
                ForwardPointerAction::RestorePassthrough
            }
        }
        _ => ForwardPointerAction::Dispatch,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoutedWindowCommand {
    SetMousePassthrough(WindowId, MousePassthroughMode),
    SetSkipTaskbar(WindowId, bool),
    Open(WindowId),
    Focus(WindowId),
    Close(WindowId),
    SetTitle(WindowId),
    Move(WindowId),
    SetBounds(WindowId),
    SetFullscreen(WindowId),
    SetMinimized(WindowId),
    SetMaximized(WindowId),
    SetAlwaysOnTop(WindowId),
    SetNativeWindowControlsVisible(WindowId),
    SetIcon(WindowId),
    SetMenuBar(WindowId),
    OpenFileDialog(WindowId),
    SetApplicationIcon,
    Drag(WindowId),
    SetShadow(WindowId),
    Ignore,
}

fn route_window_command(command: &WindowCommand, known: &[WindowId]) -> RoutedWindowCommand {
    let known = |id: WindowId| known.contains(&id);
    match command {
        WindowCommand::SetMousePassthrough { id, enabled } => {
            RoutedWindowCommand::SetMousePassthrough(
                *id,
                MousePassthroughMode::passthrough(*enabled),
            )
        }
        WindowCommand::SetMousePassthroughForward { id, enabled } => {
            RoutedWindowCommand::SetMousePassthrough(*id, MousePassthroughMode::forward(*enabled))
        }
        WindowCommand::SetSkipTaskbar { id, skip_taskbar } => {
            RoutedWindowCommand::SetSkipTaskbar(*id, *skip_taskbar)
        }
        WindowCommand::Open { id, .. } if known(*id) => RoutedWindowCommand::Focus(*id),
        WindowCommand::Open { id, .. } => RoutedWindowCommand::Open(*id),
        WindowCommand::Close(id) if !known(*id) => RoutedWindowCommand::Ignore,
        WindowCommand::Close(id) => RoutedWindowCommand::Close(*id),
        WindowCommand::Focus(id) if known(*id) => RoutedWindowCommand::Focus(*id),
        WindowCommand::SetTitle { id, .. } if known(*id) => RoutedWindowCommand::SetTitle(*id),
        WindowCommand::Move { id, .. } if known(*id) => RoutedWindowCommand::Move(*id),
        WindowCommand::SetBounds { id, .. } if known(*id) => RoutedWindowCommand::SetBounds(*id),
        WindowCommand::SetFullscreen { id, .. } if known(*id) => {
            RoutedWindowCommand::SetFullscreen(*id)
        }
        WindowCommand::SetMinimized { id, .. } if known(*id) => {
            RoutedWindowCommand::SetMinimized(*id)
        }
        WindowCommand::SetMaximized { id, .. } if known(*id) => {
            RoutedWindowCommand::SetMaximized(*id)
        }
        WindowCommand::SetAlwaysOnTop { id, .. } if known(*id) => {
            RoutedWindowCommand::SetAlwaysOnTop(*id)
        }
        WindowCommand::SetNativeWindowControlsVisible { id, .. } if known(*id) => {
            RoutedWindowCommand::SetNativeWindowControlsVisible(*id)
        }
        WindowCommand::SetIcon { id, .. } if known(*id) => RoutedWindowCommand::SetIcon(*id),
        WindowCommand::SetMenuBar { id, .. } if known(*id) => RoutedWindowCommand::SetMenuBar(*id),
        WindowCommand::OpenFileDialog { id, .. } => RoutedWindowCommand::OpenFileDialog(*id),
        WindowCommand::SetApplicationIcon { .. } => RoutedWindowCommand::SetApplicationIcon,
        WindowCommand::Drag(id) if known(*id) => RoutedWindowCommand::Drag(*id),
        WindowCommand::SetShadow { id, .. } if known(*id) => RoutedWindowCommand::SetShadow(*id),
        _ => RoutedWindowCommand::Ignore,
    }
}

fn windows_to_redraw(redraw: RuntimeRedraw, known: &[WindowId]) -> Vec<WindowId> {
    match redraw {
        RuntimeRedraw::None => Vec::new(),
        RuntimeRedraw::Window(id) => known.iter().copied().filter(|known| *known == id).collect(),
        RuntimeRedraw::All => known.to_vec(),
        RuntimeRedraw::Windows(ids) => known
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect(),
    }
}

/// Drop HostTexture views bound to the previous Device, then the caller runs
/// `rebuild_gpu` so programs can re-register on the new one.
fn invalidate_program_host_textures(
    window_ids: impl IntoIterator<Item = WindowId>,
    mut host_textures: impl FnMut(WindowId) -> Option<HostTextureRegistry>,
) -> usize {
    let mut invalidated = 0;
    for id in window_ids {
        if let Some(registry) = host_textures(id) {
            invalidated += registry.invalidate_all();
        }
    }
    invalidated
}

/// Always invoke the program input hook. Runtime `prevent_default` still
/// requests a window redraw; it does not drop Gallery/Vue delivery. A failed
/// handler degrades to an empty update (the caller reports it via
/// `host_failure`) instead of panicking.
fn scene_runtime_input_update(
    disposition: nana_ui_platform::InputDisposition,
    id: WindowId,
    program_input: Result<RuntimeProgramUpdate, FrameworkError>,
) -> RuntimeProgramUpdate {
    let program_update = program_input.unwrap_or_default();
    if disposition.prevent_default {
        RuntimeProgramUpdate::redraw(id).merge(program_update)
    } else {
        program_update
    }
}

fn window_level(always_on_top: bool) -> winit::window::WindowLevel {
    if always_on_top {
        winit::window::WindowLevel::AlwaysOnTop
    } else {
        winit::window::WindowLevel::Normal
    }
}

fn window_geometry(window: &dyn winit::window::Window) -> WindowGeometry {
    let scale_factor = normalized_scale_factor(window.scale_factor() as f32);
    let physical_size = window.surface_size();
    let physical_position = window.outer_position().ok();
    let desktop = desktop_scale(f64::from(scale_factor), window_reference_scale(window));
    WindowGeometry {
        physical_position: physical_position.map(|position| (position.x, position.y)),
        physical_size: (physical_size.width, physical_size.height),
        logical_position: physical_position.map(|position| {
            let logical = position.to_logical::<f32>(desktop);
            (logical.x, logical.y)
        }),
        logical_size: (
            physical_size.width as f32 / scale_factor,
            physical_size.height as f32 / scale_factor,
        ),
        scale_factor,
        maximized: geometry_maximized(window),
    }
}

/// macOS 的 winit `is_maximized` 底层是 `is_zoomed`:窗口 mask 为 borderless(进入
/// 全屏后)时它靠临时改回 Titled|Resizable 再回滚来查询,查询本身会触发 resize 事件,
/// 在 resize 事件处理路径中调用就形成死循环。全屏(原生或 simple)语义上不
/// maximized,直接短路;`simple_fullscreen()` 是纯状态读,无副作用。
#[cfg(target_os = "macos")]
fn geometry_maximized(window: &dyn winit::window::Window) -> bool {
    !(window.fullscreen().is_some() || WindowExtMacOS::simple_fullscreen(window))
}

#[cfg(not(target_os = "macos"))]
fn geometry_maximized(window: &dyn winit::window::Window) -> bool {
    window.is_maximized()
}

/// Where a window sits in desktop-logical space, and what takes a point from
/// the window's own logical scale into that same space.
///
/// The two differ whenever the window is not on the display that defines the
/// desktop space — the usual mixed-DPI desktop — so a client point cannot be
/// added to the origin as it stands. `size_ratios` in [`Desktop`] is the same
/// factor for a display.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ScreenSpace {
    origin: (f32, f32),
    client_ratio: f32,
}

fn window_screen_origin(window: &dyn winit::window::Window) -> Option<ScreenSpace> {
    let own = window.scale_factor().max(0.01);
    let scale = desktop_scale(own, window_reference_scale(window));
    window.outer_position().ok().map(|position| {
        let origin = position.to_logical::<f32>(scale);
        ScreenSpace {
            origin: (origin.x, origin.y),
            client_ratio: (own / scale) as f32,
        }
    })
}

fn normalized_scale_factor(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

/// The layout-resolved key name: a named key's name (`ArrowLeft`, `Enter`),
/// a character key's character.
fn platform_input_key(key: &winit::keyboard::Key) -> std::borrow::Cow<'static, str> {
    match key {
        winit::keyboard::Key::Named(named) => format!("{named:?}").into(),
        winit::keyboard::Key::Character(character) => character.to_string().into(),
        winit::keyboard::Key::Dead(_) => "Dead".into(),
        winit::keyboard::Key::Unidentified(_) => "Unidentified".into(),
    }
}

/// The key's position as a W3C `code` (`KeyA`, `Digit1`, `ShiftLeft`).
fn platform_physical_key(key: &winit::keyboard::PhysicalKey) -> std::borrow::Cow<'static, str> {
    match key {
        winit::keyboard::PhysicalKey::Code(code) => format!("{code:?}").into(),
        winit::keyboard::PhysicalKey::Unidentified(_) => "Unidentified".into(),
    }
}

fn system_input_modifiers(keys: nana_window::KeyboardModifiers) -> InputModifiers {
    InputModifiers {
        alt: keys.alt,
        control: keys.control,
        meta: keys.meta,
        shift: keys.shift,
    }
}

fn platform_input_modifiers(value: ModifiersState) -> InputModifiers {
    InputModifiers {
        alt: value.alt_key(),
        control: value.control_key(),
        meta: value.meta_key(),
        shift: value.shift_key(),
    }
}

fn dnd_advertises_files(event_loop: &dyn ActiveEventLoop, transfer: DataTransferId) -> bool {
    event_loop
        .data_transfer(transfer)
        .map(|transfer| transfer.has_type(&TypeHint::UriList))
        .unwrap_or(true)
}

fn platform_composition(ime: &winit::event::Ime) -> CompositionInput {
    match ime {
        winit::event::Ime::Enabled => CompositionInput::Enabled,
        winit::event::Ime::Disabled => CompositionInput::Disabled,
        winit::event::Ime::Preedit(text, selection) => CompositionInput::Update {
            text: text.clone(),
            selection: *selection,
        },
        winit::event::Ime::Commit(text) => CompositionInput::Commit(text.clone()),
        winit::event::Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => CompositionInput::DeleteSurrounding {
            before_bytes: *before_bytes,
            after_bytes: *after_bytes,
        },
        _ => CompositionInput::Disabled,
    }
}

fn mouse_button_code(button: MouseButton) -> i16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        other => other as u8 as i16,
    }
}

fn mouse_button_mask(button: i16) -> u16 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        4 => 16,
        _ => 0,
    }
}

/// W3C code of the primary mouse button, the one every platform's own window
/// drag assumes is held.
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
const PRIMARY_MOUSE_BUTTON: i16 = 0;

/// The button a held-button gesture belongs to, primary first.
///
/// `buttons` is the hosted mask ([`mouse_button_mask`]), so a gesture holding
/// several buttons reports the primary one while it is down; that is the one
/// whose platform behavior a window drag should follow.
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
fn held_mouse_button(buttons: u16) -> Option<i16> {
    [PRIMARY_MOUSE_BUTTON, 1, 2, 3, 4]
        .into_iter()
        .find(|&button| buttons & mouse_button_mask(button) != 0)
}

/// What a pointer event does to a running host-driven window move.
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameMoveStep {
    /// Follow the pointer; the gesture continues.
    Follow,
    /// The gesture is over; drop the session.
    Finish,
    /// Swallow the event; the gesture continues.
    Hold,
}

/// Decides a running window move from one pointer event, given the `owner`
/// button whose release ends it.
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
fn frame_move_step(phase: PointerPhase, button: i16, owner: i16) -> FrameMoveStep {
    match phase {
        PointerPhase::Move => FrameMoveStep::Follow,
        // The pinned winit win32 proc synthesizes `PointerLeft` from a
        // client-rect bounds check even while the gesture holds capture, so a
        // drag crossing the window edge arrives here as `Cancel`. Ending on it
        // would drop the window the moment it leaves its own former bounds.
        PointerPhase::Cancel => FrameMoveStep::Hold,
        PointerPhase::Up if button == owner => FrameMoveStep::Finish,
        // A fresh primary press stands in for a release the platform never
        // delivered, and matches the system move loop, which any click ends.
        PointerPhase::Down if button == PRIMARY_MOUSE_BUTTON => FrameMoveStep::Finish,
        _ => FrameMoveStep::Hold,
    }
}

fn screen_position(space: Option<ScreenSpace>, client: (f32, f32)) -> (f32, f32) {
    space.map_or(client, |space| {
        (
            space.origin.0 + client.0 * space.client_ratio,
            space.origin.1 + client.1 * space.client_ratio,
        )
    })
}

struct MappedPointer {
    pointer_id: u64,
    pointer_type: PointerType,
    is_primary: bool,
    pressure: Option<f32>,
    tangential_pressure: f32,
    tilt_x: i16,
    tilt_y: i16,
    twist: u16,
}

fn tablet_pointer_id(device_id: Option<DeviceId>, kind: TabletToolKind) -> u64 {
    let device = device_id
        .map(|id| id.into_raw().unsigned_abs())
        .unwrap_or(0);
    let kind_index = u64::from(kind != TabletToolKind::Pen);
    1000 + device.saturating_mul(2) + kind_index
}

fn canonical_device_id(device_id: Option<DeviceId>) -> nana_ui_platform::DeviceId {
    nana_ui_platform::DeviceId(
        device_id
            .map(|id| id.into_raw().unsigned_abs())
            .unwrap_or(0),
    )
}

fn mapped_pointer(
    pointer_id: u64,
    pointer_type: PointerType,
    primary: bool,
    pressure: Option<f32>,
) -> MappedPointer {
    MappedPointer {
        pointer_id,
        pointer_type,
        is_primary: primary,
        pressure,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
    }
}

fn map_tablet(
    kind: TabletToolKind,
    data: &winit::event::TabletToolData,
    primary: bool,
    device_id: Option<DeviceId>,
) -> MappedPointer {
    let tilt = data.clone().tilt();
    let angle = data.clone().angle();
    MappedPointer {
        pointer_id: tablet_pointer_id(device_id, kind),
        pointer_type: PointerType::Pen,
        is_primary: primary,
        pressure: data
            .force
            .as_ref()
            .map(|force| force.normalized(angle) as f32),
        tangential_pressure: data.tangential_force.unwrap_or(0.0),
        tilt_x: tilt.map(|tilt| i16::from(tilt.x)).unwrap_or(0),
        tilt_y: tilt.map(|tilt| i16::from(tilt.y)).unwrap_or(0),
        twist: data.twist.unwrap_or(0),
    }
}

fn map_pointer_kind(
    kind: &PointerKind,
    primary: bool,
    device_id: Option<DeviceId>,
) -> MappedPointer {
    match kind {
        PointerKind::Touch(finger_id) => mapped_pointer(
            finger_id.into_raw() as u64 + 2,
            PointerType::Touch,
            primary,
            None,
        ),
        PointerKind::TabletTool(kind) => mapped_pointer(
            tablet_pointer_id(device_id, *kind),
            PointerType::Pen,
            primary,
            None,
        ),
        PointerKind::Mouse | PointerKind::Unknown | _ => {
            mapped_pointer(1, PointerType::Mouse, primary, None)
        }
    }
}

fn map_pointer_source(
    source: &PointerSource,
    primary: bool,
    device_id: Option<DeviceId>,
) -> MappedPointer {
    match source {
        PointerSource::Touch { finger_id, force } => mapped_pointer(
            finger_id.into_raw() as u64 + 2,
            PointerType::Touch,
            primary,
            force.as_ref().map(|force| force.normalized(None) as f32),
        ),
        PointerSource::TabletTool { kind, data } => map_tablet(*kind, data, primary, device_id),
        PointerSource::Mouse | PointerSource::Unknown | _ => {
            map_pointer_kind(&PointerKind::Mouse, primary, device_id)
        }
    }
}

fn map_button_source(
    source: &ButtonSource,
    primary: bool,
    device_id: Option<DeviceId>,
) -> MappedPointer {
    match source {
        ButtonSource::Touch { finger_id, force } => mapped_pointer(
            finger_id.into_raw() as u64 + 2,
            PointerType::Touch,
            primary,
            force.as_ref().map(|force| force.normalized(None) as f32),
        ),
        ButtonSource::TabletTool { kind, data, .. } => map_tablet(*kind, data, primary, device_id),
        ButtonSource::Mouse(_) | ButtonSource::Unknown(_) | _ => {
            map_pointer_kind(&PointerKind::Mouse, primary, device_id)
        }
    }
}

/// One native event lowered to canonical input, not yet stamped: the device
/// it came from, its payload, the text a key press typed, and when.
#[derive(Debug)]
struct LoweredInput {
    device: nana_ui_platform::DeviceId,
    payload: InputPayload,
    /// For a key press: the text it typed, delivered right after the key.
    text: Option<String>,
    /// Runtime clock time of the event.
    now: Duration,
}

impl LoweredInput {
    fn event(device: nana_ui_platform::DeviceId, payload: InputPayload, now: Duration) -> Self {
        Self {
            device,
            payload,
            text: None,
            now,
        }
    }
}

#[derive(Debug, Default)]
struct InputTracker {
    cursor: (f32, f32),
    /// Device of the last pointer or key event, for events that carry none
    /// (window focus).
    last_device: nana_ui_platform::DeviceId,
    buttons: u16,
    modifiers: ModifiersState,
    pending_file_paths: Vec<PathBuf>,
    file_drop_emitted: bool,
    pending_dnd: Option<DataTransferId>,
    pending_dnd_serial: Option<AsyncRequestSerial>,
    drop_waiting_for_data: bool,
    /// Keys held at the release while its paths are still being fetched.
    drop_modifiers: Option<InputModifiers>,
}

impl InputTracker {
    fn clear_pointers(&mut self) {
        self.buttons = 0;
    }

    /// Ends the mouse gesture whose release the platform will not deliver.
    fn cancel_mouse(&mut self, screen_origin: Option<ScreenSpace>) -> PointerInput {
        let buttons = std::mem::take(&mut self.buttons);
        self.pointer_event(
            mapped_pointer(1, PointerType::Mouse, true, None),
            PointerPhase::Cancel,
            -1,
            buttons,
            false,
            platform_input_modifiers(self.modifiers),
            screen_origin,
            Some(0.0),
        )
    }

    fn set_cursor_physical(&mut self, position: PhysicalPosition<f64>, scale: f32) {
        let point = position.to_logical::<f32>(f64::from(scale));
        self.cursor = (point.x, point.y);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit fields of the host or GPU projection contract"
    )]
    fn pointer_event(
        &self,
        mapped: MappedPointer,
        phase: PointerPhase,
        button: i16,
        buttons: u16,
        activation_click: bool,
        modifiers: InputModifiers,
        screen_origin: Option<ScreenSpace>,
        pressure: Option<f32>,
    ) -> PointerInput {
        let screen = screen_position(screen_origin, self.cursor);
        PointerInput {
            phase,
            pointer_id: nana_ui_platform::PointerId(mapped.pointer_id),
            pointer_type: mapped.pointer_type,
            x: self.cursor.0,
            y: self.cursor.1,
            screen_x: screen.0,
            screen_y: screen.1,
            button,
            buttons,
            pressure: pressure.unwrap_or_else(|| {
                mapped
                    .pressure
                    .unwrap_or(if buttons == 0 { 0.0 } else { 0.5 })
            }),
            tangential_pressure: mapped.tangential_pressure,
            tilt_x: mapped.tilt_x,
            tilt_y: mapped.tilt_y,
            twist: mapped.twist,
            is_primary: mapped.is_primary,
            activation_click,
            modifiers,
        }
    }

    /// Keys held during an OS drag. The target window gets no modifier
    /// events while the source owns the keyboard, so prefer the system state
    /// and fall back to the last tracked one where it cannot be sampled.
    fn drag_modifiers(&self) -> InputModifiers {
        nana_window::keyboard_modifiers()
            .map(system_input_modifiers)
            .unwrap_or_else(|| platform_input_modifiers(self.modifiers))
    }

    /// Forget the drag: its paths, its transfer and whether it was dropped.
    fn reset_file_drag(&mut self) {
        self.pending_file_paths.clear();
        self.file_drop_emitted = false;
        self.drop_waiting_for_data = false;
        self.drop_modifiers = None;
        self.pending_dnd = None;
        self.pending_dnd_serial = None;
    }

    fn begin_file_drag(&mut self, transfer: DataTransferId, serial: Option<AsyncRequestSerial>) {
        self.reset_file_drag();
        self.pending_dnd = Some(transfer);
        self.pending_dnd_serial = serial;
    }

    /// The drag's files hovering at the cursor.
    fn hover_files(&self) -> FileDragInput {
        FileDragInput {
            kind: FileDragKind::Hover,
            paths: self.pending_file_paths.clone(),
            position: Some(self.cursor),
            modifiers: self.drag_modifiers(),
        }
    }

    /// The release: the drag's files at the cursor, sent once.
    fn drop_files(&mut self, modifiers: InputModifiers) -> FileDragInput {
        self.file_drop_emitted = true;
        self.drop_waiting_for_data = false;
        self.pending_dnd = None;
        self.pending_dnd_serial = None;
        FileDragInput {
            kind: FileDragKind::Drop,
            paths: std::mem::take(&mut self.pending_file_paths),
            position: Some(self.cursor),
            modifiers,
        }
    }

    fn wait_for_drop_data(&mut self, transfer: DataTransferId, serial: AsyncRequestSerial) {
        self.pending_dnd = Some(transfer);
        self.pending_dnd_serial = Some(serial);
        self.drop_waiting_for_data = true;
        // The keys may be let go before the paths arrive; the drop is now.
        self.drop_modifiers = Some(self.drag_modifiers());
    }

    /// The paths of a release could not be read: end the drag as cancelled.
    fn abandon_drop(&mut self, transfer: DataTransferId) -> Option<FileDragInput> {
        if self.pending_dnd != Some(transfer) || !self.drop_waiting_for_data {
            return None;
        }
        self.reset_file_drag();
        self.file_drop_emitted = true;
        Some(FileDragInput::cancel())
    }

    fn accepts_dnd_serial(&self, transfer: DataTransferId, serial: AsyncRequestSerial) -> bool {
        self.pending_dnd == Some(transfer)
            && self
                .pending_dnd_serial
                .is_none_or(|pending| pending == serial)
    }

    fn ingest_file_paths(
        &mut self,
        transfer: DataTransferId,
        paths: Vec<PathBuf>,
    ) -> Option<FileDragInput> {
        if self.pending_dnd != Some(transfer) {
            return None;
        }
        self.pending_file_paths = paths;
        if self.drop_waiting_for_data {
            if self.file_drop_emitted {
                return None;
            }
            let modifiers = self
                .drop_modifiers
                .take()
                .unwrap_or_else(|| self.drag_modifiers());
            return Some(self.drop_files(modifiers));
        }
        Some(self.hover_files())
    }

    /// Lower one native event to canonical input, or `None` for events that
    /// are not input.
    fn map(
        &mut self,
        event: &WinitWindowEvent,
        scale: f32,
        screen_origin: Option<ScreenSpace>,
        now: Duration,
    ) -> Option<LoweredInput> {
        let modifiers = platform_input_modifiers(self.modifiers);
        let (device, payload, text) = match event {
            WinitWindowEvent::PointerMoved {
                device_id,
                position,
                primary,
                source,
            } => {
                self.set_cursor_physical(*position, scale);
                (
                    device_id,
                    InputPayload::Pointer(self.pointer_event(
                        map_pointer_source(source, *primary, *device_id),
                        PointerPhase::Move,
                        -1,
                        self.buttons,
                        false,
                        modifiers,
                        screen_origin,
                        None,
                    )),
                    None,
                )
            }
            WinitWindowEvent::PointerEntered {
                device_id,
                position,
                primary,
                kind,
            } => {
                self.set_cursor_physical(*position, scale);
                (
                    device_id,
                    InputPayload::Pointer(self.pointer_event(
                        map_pointer_kind(kind, *primary, *device_id),
                        PointerPhase::Move,
                        -1,
                        self.buttons,
                        false,
                        modifiers,
                        screen_origin,
                        None,
                    )),
                    None,
                )
            }
            WinitWindowEvent::PointerButton {
                device_id,
                state,
                position,
                primary,
                button,
                is_macos_activation_click,
            } => {
                self.set_cursor_physical(*position, scale);
                let mouse = button.clone().mouse_button().unwrap_or(MouseButton::Left);
                let button_code = mouse_button_code(mouse);
                let pressed = *state == ElementState::Pressed;
                let mask = mouse_button_mask(button_code);
                if pressed {
                    self.buttons |= mask;
                } else {
                    self.buttons &= !mask;
                }
                (
                    device_id,
                    InputPayload::Pointer(self.pointer_event(
                        map_button_source(button, *primary, *device_id),
                        if pressed {
                            PointerPhase::Down
                        } else {
                            PointerPhase::Up
                        },
                        button_code,
                        self.buttons,
                        *is_macos_activation_click,
                        modifiers,
                        screen_origin,
                        None,
                    )),
                    None,
                )
            }
            // Leaving the surface is not a cancellation: hover clears while
            // a capture, if any, keeps its pointer. The leave carries its own
            // device, not the last event's.
            WinitWindowEvent::PointerLeft {
                device_id,
                position,
                primary,
                kind,
            } => {
                if let Some(position) = position {
                    self.set_cursor_physical(*position, scale);
                }
                self.buttons = 0;
                let pointer = map_pointer_kind(kind, *primary, *device_id);
                (
                    device_id,
                    InputPayload::PointerLeave {
                        pointer_id: nana_ui_platform::PointerId(pointer.pointer_id),
                    },
                    None,
                )
            }
            WinitWindowEvent::MouseWheel {
                device_id, delta, ..
            } => {
                let (delta_x, delta_y, unit) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (*x, *y, nana_ui_platform::WheelUnit::Lines)
                    }
                    MouseScrollDelta::PixelDelta(delta) => (
                        (delta.x / f64::from(scale)) as f32,
                        (delta.y / f64::from(scale)) as f32,
                        nana_ui_platform::WheelUnit::Pixels,
                    ),
                    _ => (0.0, 0.0, nana_ui_platform::WheelUnit::Pixels),
                };
                // The wheel belongs to the mouse: same pointer, same hover.
                (
                    device_id,
                    InputPayload::Wheel(nana_ui_platform::WheelInput {
                        pointer_id: nana_ui_platform::PointerId(1),
                        x: self.cursor.0,
                        y: self.cursor.1,
                        delta_x,
                        delta_y,
                        unit,
                        modifiers,
                    }),
                    None,
                )
            }
            // A press synthesized on focus gain was typed into another window,
            // e.g. the Esc that dismissed an owned native dialog; delivering it
            // would run that shortcut a second time here.
            WinitWindowEvent::KeyboardInput {
                event,
                is_synthetic: true,
                ..
            } if event.state == ElementState::Pressed => return None,
            WinitWindowEvent::KeyboardInput {
                device_id, event, ..
            } => {
                let pressed = event.state == ElementState::Pressed;
                let key = nana_ui_platform::KeyInput {
                    physical: nana_ui_platform::PhysicalKey(platform_physical_key(
                        &event.physical_key,
                    )),
                    logical: nana_ui_platform::LogicalKey(platform_input_key(&event.logical_key)),
                    state: if pressed {
                        nana_ui_platform::KeyState::Pressed
                    } else {
                        nana_ui_platform::KeyState::Released
                    },
                    repeat: event.repeat,
                    modifiers,
                };
                let text = event
                    .text
                    .as_ref()
                    .filter(|text| pressed && !text.is_empty())
                    .map(ToString::to_string);
                (device_id, InputPayload::Key(key), text)
            }
            WinitWindowEvent::Ime(ime) => {
                return Some(LoweredInput::event(
                    self.last_device,
                    InputPayload::Composition(platform_composition(ime)),
                    now,
                ));
            }
            _ => return None,
        };
        let device = canonical_device_id(*device);
        self.last_device = device;
        Some(LoweredInput {
            device,
            payload,
            text,
            now,
        })
    }

    fn map_file_window_event(&mut self, event: &WinitWindowEvent) -> Option<FileDragInput> {
        match event {
            WinitWindowEvent::DragEntered { id: transfer, .. } => {
                if self.pending_dnd != Some(*transfer) {
                    self.begin_file_drag(*transfer, None);
                }
                Some(self.hover_files())
            }
            WinitWindowEvent::DragPosition { id: transfer, .. } => {
                if self.pending_dnd != Some(*transfer) || self.file_drop_emitted {
                    return None;
                }
                Some(self.hover_files())
            }
            WinitWindowEvent::DragLeft { .. } => {
                self.reset_file_drag();
                Some(FileDragInput::cancel())
            }
            WinitWindowEvent::DragDropped { .. } => {
                if self.file_drop_emitted {
                    return None;
                }
                Some(self.drop_files(self.drag_modifiers()))
            }
            _ => None,
        }
    }
}

fn platform_window_event(
    event: &WinitWindowEvent,
    id: WindowId,
    geometry: WindowGeometry,
) -> Option<WindowEvent> {
    Some(match event {
        WinitWindowEvent::CloseRequested => WindowEvent::CloseRequested { id },
        WinitWindowEvent::Destroyed => WindowEvent::Closed { id },
        WinitWindowEvent::Occluded(hidden) => WindowEvent::VisibilityChanged {
            id,
            hidden: *hidden,
        },
        WinitWindowEvent::Focused(focused) => WindowEvent::FocusChanged {
            id,
            focused: *focused,
        },
        WinitWindowEvent::SurfaceResized(_) | WinitWindowEvent::ScaleFactorChanged { .. } => {
            WindowEvent::Resized { id, geometry }
        }
        WinitWindowEvent::Moved(_) => WindowEvent::Moved { id, geometry },
        WinitWindowEvent::ThemeChanged(theme) => WindowEvent::AppearanceChanged {
            id,
            appearance: system_appearance_from_winit(*theme),
        },
        _ => return None,
    })
}

/// winit reports light/dark on macOS, Windows, Android and web; the remaining
/// platforms never emit `ThemeChanged`, so no appearance event is synthesised.
pub(crate) const fn system_appearance_from_winit(theme: WinitTheme) -> SystemAppearance {
    match theme {
        WinitTheme::Light => SystemAppearance::Light,
        WinitTheme::Dark => SystemAppearance::Dark,
    }
}

/// Adapter for an existing winit event loop. Call from the host's window thread.
/// This adapter never creates or exits an event loop and shares the supplied GPU.
pub struct EmbeddedRuntime<Program: RuntimeProgram> {
    manager: WindowManager<Program>,
}
impl<Program: RuntimeProgram> EmbeddedRuntime<Program> {
    pub fn new(
        event_loop: &dyn ActiveEventLoop,
        proxy: EventLoopProxy,
        graphics: crate::HostedGpuShared,
        settings: WindowDescriptor,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        initialize(
            event_loop,
            proxy,
            tx,
            rx,
            settings,
            Arc::new(Mutex::new(None)),
            graphics,
        )
        .map(|manager| Self { manager })
    }
    pub fn windows(&self) -> &crate::WindowService {
        &self.manager.windows
    }
    /// Forward the embedding host's device-loss notification on the window thread
    /// before forwarding further window events. This does not exit the host loop
    /// or install/replace the host's device callback.
    pub fn notify_device_lost(&mut self) {
        // Embedded devices never raise the hosted device-lost flag; the
        // embedder tells us here instead. Recording it on the context lets
        // everything holding it (producer threads, the JS runtime) see the
        // loss; taking the report right away keeps the fault below the only
        // one.
        let graphics = &self.manager.graphics;
        nana_gpu::__framework::mark_lost(
            graphics.gpu(),
            nana_gpu::GpuDeviceLost {
                reason: nana_gpu::GpuLossReason::Unknown,
                message: "reported by the embedding host".into(),
            },
        );
        let _ = graphics.take_device_lost_report();
        if !self.manager.render_suspended {
            nana_diagnostics::fault!(
                nana_diagnostics::framework::gpu::DEVICE_LOST,
                reason = 0u64;
                "reported by the embedding host"
            );
            nana_diagnostics::snapshot("device-lost");
        }
        self.manager.render_suspended = true;
        self.manager.next_gpu_retry = None;
    }

    pub fn needs_gpu_replacement(&self) -> bool {
        self.manager.render_suspended
    }
    /// Called by the embedding host after it replaces its device.
    ///
    /// Always adopts `graphics`: surfaces are rebound in place, and DXGI's one
    /// swap chain per HWND leaves no old surface to fall back to. Windows whose
    /// rebind failed recover individually; the error is returned only when
    /// every window failed.
    pub fn replace_gpu(&mut self, graphics: crate::HostedGpuShared) -> Result<(), String> {
        let outcomes: Vec<_> = self
            .manager
            .window_contexts
            .iter_mut()
            .map(|(&id, host)| (id, graphics.recreate_surface(&mut host.surface)))
            .collect();
        let all_failed = outcomes.iter().all(|(_, outcome)| outcome.is_err());
        let error = outcomes
            .first()
            .and_then(|(_, outcome)| outcome.as_ref().err())
            .filter(|_| all_failed)
            .map(ToString::to_string);
        self.manager.switch_gpu(graphics, outcomes);
        error.map_or(Ok(()), Err)
    }
    /// Displays connected now.
    pub fn displays(&self, event_loop: &dyn ActiveEventLoop) -> Vec<nana_ui_platform::DisplayInfo> {
        display::display_infos(event_loop)
    }

    /// Create a window synchronously on the host's window thread.
    /// Resolves exactly like `WindowService::create_window`, without a queue round trip.
    pub fn create_window(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        descriptor: WindowDescriptor,
    ) -> Result<crate::WindowHandle, crate::WindowError> {
        crate::window_service::validate_descriptor(&descriptor)?;
        self.manager.create_service_window(event_loop, descriptor)
    }
    pub fn is_empty(&self) -> bool {
        self.manager.window_contexts.is_empty()
    }
    /// Returns false for windows owned by another component of the host.
    pub fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        native_id: winit::window::WindowId,
        event: WinitWindowEvent,
    ) -> bool {
        let Some(id) = self.manager.window_ids.get(&native_id).copied() else {
            return false;
        };
        self.manager.handle_window_event(event_loop, id, event);
        true
    }
    pub fn wake(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.manager.drain_host_work(event_loop);
    }
    /// Returns NanaUI's next deadline for the host to merge with its own timers.
    pub fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) -> Option<Instant> {
        self.manager.about_to_wait(event_loop);
        self.manager.wake_deadline
    }
}

#[cfg(test)]
mod tests {
    use super::ScreenSpace;
    #[cfg(not(target_os = "macos"))]
    use super::desktop_scale;
    #[cfg(not(target_os = "android"))]
    use super::next_accessibility_update;
    use super::{
        Desktop, DisplayBounds, FileDragInput, FileDragKind, ForwardPointerAction, FrameMoveStep,
        InputTracker, PRIMARY_MOUSE_BUTTON, RoutedWindowCommand, desktop_position,
        frame_edge_cursor, frame_move_step, held_mouse_button, invalidate_program_host_textures,
        mouse_button_code, mouse_button_mask, platform_composition, platform_input_key,
        platform_input_modifiers, platform_physical_key, platform_window_event,
        remove_image_target_index, replace_image_target_index, route_window_command,
        scene_clear_color, scene_runtime_input_update, scene_window_attributes, screen_position,
        surface_image_keys, system_input_modifiers, tablet_pointer_id, window_cursor_override,
        window_level, window_surface_effect, window_wants_transparent_surface,
        windows_scene_chrome, windows_to_redraw, winit_icon,
    };
    use crate::presentation::{
        ResolvedSurfaceTarget, ResolvedWindowPresentation, WindowSurfaceTarget,
    };
    use crate::{
        HostTexture, HostTextureAlphaMode, HostTextureRegistry, MaterialEffect, MaterialFallback,
        MaterialOutcome, RuntimeProgramUpdate, RuntimeRedraw, ThemeMode,
    };
    use nana_ui_platform::host::WindowCommand;
    use nana_ui_platform::{
        CompositionInput, InputDisposition, InputModifiers, InputPayload, KeyInput, KeyState,
        MousePassthroughMode, PointerId, PointerInput, PointerPhase, PointerType, WheelInput,
        WheelUnit, WindowDescriptor, WindowEvent, WindowGeometry, WindowIcon, WindowId,
        WindowResizeEdge,
    };
    #[cfg(not(target_os = "android"))]
    use nana_ui_runtime::{AccessibilityDelta, AccessibilityUpdate, FrameworkError};
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        ButtonSource, DeviceId, ElementState, FingerId, MouseButton, MouseScrollDelta, PointerKind,
        PointerSource, TabletToolData, TabletToolKind, TouchPhase, WindowEvent as WinitWindowEvent,
    };
    use winit::keyboard::{Key, ModifiersState, NamedKey};

    /// Lower one native event and keep only the payload.
    fn map_input(
        tracker: &mut InputTracker,
        event: &WinitWindowEvent,
        scale: f32,
        origin: Option<ScreenSpace>,
    ) -> Option<InputPayload> {
        tracker
            .map(event, scale, origin, std::time::Duration::ZERO)
            .map(|lowered| lowered.payload)
    }

    fn geometry() -> WindowGeometry {
        WindowGeometry {
            physical_size: (200, 100),
            logical_size: (100.0, 50.0),
            scale_factor: 2.0,
            ..WindowGeometry::default()
        }
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn empty_flush_reprojects_when_the_program_already_drained_runtime_work() {
        let Some(AccessibilityUpdate::Full { generation, nodes }) =
            next_accessibility_update(None, None, false, None, Some(3), Vec::new)
        else {
            panic!("drained SystemWork must still reach AccessKit from the world");
        };
        assert_eq!(generation, Some(3));
        assert!(nodes.is_empty());
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn matching_generations_do_not_rebuild_an_idle_tree() {
        assert!(
            next_accessibility_update(None, None, false, Some(3), Some(3), || panic!(
                "idle frames must not snapshot"
            ))
            .is_none()
        );
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn program_accessibility_queue_is_the_host_source_when_flush_is_empty() {
        let queued = AccessibilityUpdate::Delta(AccessibilityDelta {
            generation: 2,
            updated: Vec::new(),
            removed: Vec::new(),
        });
        assert_eq!(
            next_accessibility_update(
                None,
                Some(queued.clone()),
                false,
                Some(1),
                Some(2),
                || panic!("queued deltas must not force a world snapshot"),
            ),
            Some(queued)
        );
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn scale_change_reprojects_even_when_generations_match() {
        let Some(AccessibilityUpdate::Full { generation, .. }) =
            next_accessibility_update(None, None, true, Some(1), Some(1), Vec::new)
        else {
            panic!("DPI change must reproject the current world");
        };
        assert_eq!(generation, Some(1));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn flush_and_program_deltas_reproject_the_world_instead_of_dropping_one() {
        let flush = AccessibilityUpdate::Delta(AccessibilityDelta {
            generation: 3,
            updated: Vec::new(),
            removed: Vec::new(),
        });
        let program = AccessibilityUpdate::Delta(AccessibilityDelta {
            generation: 2,
            updated: Vec::new(),
            removed: Vec::new(),
        });
        let Some(AccessibilityUpdate::Full { generation, .. }) = next_accessibility_update(
            Some(flush),
            Some(program),
            false,
            Some(1),
            Some(3),
            Vec::new,
        ) else {
            panic!("two accessibility sources must not drop hierarchy for layout");
        };
        assert_eq!(generation, Some(3));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn stale_program_queue_yields_to_the_current_world_snapshot() {
        let queued = AccessibilityUpdate::Delta(AccessibilityDelta {
            generation: 1,
            updated: Vec::new(),
            removed: Vec::new(),
        });
        let Some(AccessibilityUpdate::Full { generation, .. }) =
            next_accessibility_update(None, Some(queued), false, Some(1), Some(3), Vec::new)
        else {
            panic!("stale queued delta must reproject AccessKit from the world");
        };
        assert_eq!(generation, Some(3));
    }

    #[test]
    fn scene_windows_use_client_chrome_and_runtime_settings() {
        let mut settings = WindowDescriptor::new("Scene");
        settings.transparent = true;
        settings.always_on_top = true;
        settings.resizable = false;
        settings.maximized = true;
        settings.initial_size = (640.0, 480.0);
        settings.minimum_size = (320.0, 240.0);
        let attributes = scene_window_attributes(
            &settings,
            &Desktop {
                displays: Vec::new(),
                size_ratios: Vec::new(),
                scale: 1.0,
            },
            WindowSurfaceTarget::NativeWindow,
        );

        assert_eq!(attributes.title, "Scene");
        #[cfg(target_os = "macos")]
        assert!(attributes.decorations);
        #[cfg(not(target_os = "macos"))]
        assert!(!attributes.decorations);
        assert!(attributes.transparent);
        assert!(attributes.maximized);
        assert!(!attributes.resizable);
        assert_eq!(
            attributes.window_level,
            winit::window::WindowLevel::AlwaysOnTop
        );
        assert_eq!(window_level(false), winit::window::WindowLevel::Normal);

        let transparent_client = windows_scene_chrome(
            settings.system_caption,
            settings.transparent,
            WindowSurfaceTarget::NativeWindow,
        );
        assert!(!transparent_client.decorations);
        assert!(!transparent_client.undecorated_shadow);
        assert!(!transparent_client.no_redirection_bitmap);
        assert!(!transparent_client.rounded_corners);

        let opaque_client = windows_scene_chrome(false, false, WindowSurfaceTarget::NativeWindow);
        assert!(!opaque_client.decorations);
        assert!(!opaque_client.undecorated_shadow);
        assert!(!opaque_client.no_redirection_bitmap);
        assert!(opaque_client.rounded_corners);

        settings.system_caption = true;
        let caption = scene_window_attributes(
            &settings,
            &Desktop {
                displays: Vec::new(),
                size_ratios: Vec::new(),
                scale: 1.0,
            },
            WindowSurfaceTarget::NativeWindow,
        );
        assert!(caption.decorations);
        let transparent_caption =
            windows_scene_chrome(true, true, WindowSurfaceTarget::NativeWindow);
        assert!(transparent_caption.decorations);
        assert!(!transparent_caption.undecorated_shadow);
        assert!(!transparent_caption.no_redirection_bitmap);
        let opaque_caption = windows_scene_chrome(true, false, WindowSurfaceTarget::NativeWindow);
        assert!(opaque_caption.decorations);
        assert!(!opaque_caption.no_redirection_bitmap);
    }

    /// The redirection bitmap belongs to the presentation path, not the
    /// material. A composed window has to be created without one, because
    /// DirectComposition draws its visual over that bitmap and an opaque
    /// surface would show through underneath; a window on the plain path keeps
    /// it whatever its material, because its swapchain may be presenting into
    /// it.
    #[test]
    fn only_a_composed_window_is_created_without_a_redirection_bitmap() {
        for system_caption in [false, true] {
            for transparent in [false, true] {
                assert!(
                    !windows_scene_chrome(
                        system_caption,
                        transparent,
                        WindowSurfaceTarget::NativeWindow
                    )
                    .no_redirection_bitmap
                );
                assert!(
                    windows_scene_chrome(
                        system_caption,
                        transparent,
                        WindowSurfaceTarget::Composition
                    )
                    .no_redirection_bitmap
                );
            }
        }
    }

    /// A host that leaves `transparent: false` so the user can return to an
    /// opaque background still runs transparent, and its chrome has to follow
    /// the live surface or DWM keeps stroking and shadowing the HWND.
    ///
    /// The chrome is no longer derived at the call site: it is settled on the
    /// resolved presentation, so this asserts that the presentation a live
    /// transparent surface produces carries the transparent policy even though
    /// the descriptor says otherwise.
    fn resolved(
        settings: &WindowDescriptor,
        requested: MaterialEffect,
        alpha: wgpu::CompositeAlphaMode,
        backend: wgpu::Backend,
    ) -> ResolvedWindowPresentation {
        let applied = match requested {
            MaterialEffect::Transparent => MaterialOutcome::transparent(),
            MaterialEffect::Solid => MaterialOutcome::chosen_solid(),
            effect => MaterialOutcome::native(effect),
        };
        ResolvedWindowPresentation::resolve(
            settings,
            requested,
            applied,
            alpha,
            backend,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
            nana_window::NonClientRenderingStrategy::StripFrameStyles,
        )
    }

    #[test]
    fn client_chrome_follows_the_live_surface_not_the_descriptor() {
        use crate::presentation::NativeChromePolicy;
        let mut settings = WindowDescriptor::new("Scene");
        assert!(!settings.transparent);

        let transparent = resolved(
            &settings,
            MaterialEffect::Transparent,
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::Backend::Vulkan,
        );
        assert_eq!(
            transparent.chrome(),
            Some(NativeChromePolicy::transparent(
                nana_window::NonClientRenderingStrategy::StripFrameStyles,
            ))
        );

        let opaque = resolved(
            &settings,
            MaterialEffect::Solid,
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::Backend::Vulkan,
        );
        assert_eq!(opaque.chrome(), Some(NativeChromePolicy::OPAQUE));

        settings.system_caption = true;
        assert_eq!(
            resolved(
                &settings,
                MaterialEffect::Transparent,
                wgpu::CompositeAlphaMode::PreMultiplied,
                wgpu::Backend::Vulkan,
            )
            .chrome(),
            None
        );
    }

    /// A composed attempt that cannot be completed falls back to the plain
    /// native path exactly once, and the plain path has nothing below it: a
    /// failure there is a real startup failure rather than another retry.
    #[test]
    fn a_failed_composed_attempt_falls_back_to_the_plain_path_and_stops_there() {
        use super::next_bootstrap_attempt;
        use crate::presentation::SurfaceTargetFallback;

        let composed = ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::Composition, false);
        let next = next_bootstrap_attempt(composed).expect("a composed attempt has a fallback");
        assert_eq!(next.requested, WindowSurfaceTarget::Composition);
        assert_eq!(next.resolved, WindowSurfaceTarget::NativeWindow);
        assert_eq!(
            next.fallback,
            Some(SurfaceTargetFallback::TargetUnavailable)
        );
        assert_eq!(next_bootstrap_attempt(next), None);
        assert_eq!(
            next_bootstrap_attempt(ResolvedSurfaceTarget::honoured(
                WindowSurfaceTarget::NativeWindow,
                false
            )),
            None
        );
        // A window the backend already ruled out never creates a composed
        // HWND at all, so it has no second attempt to make either.
        let no_backend = ResolvedSurfaceTarget::fell_back(
            WindowSurfaceTarget::Composition,
            SurfaceTargetFallback::BackendUnavailable,
            false,
        );
        assert_eq!(next_bootstrap_attempt(no_backend), None);

        // A window that requires the compositor has no fallback to take: it
        // asked to fail rather than present another way.
        assert_eq!(
            next_bootstrap_attempt(ResolvedSurfaceTarget::honoured(
                WindowSurfaceTarget::Composition,
                true
            )),
            None
        );
    }

    /// The P0 inconsistency, as a contract: a `Transparent` request on a DX12
    /// HWND surface falls back to `Solid`, and the window's native chrome is
    /// the opaque policy in the same value the renderer reads its material
    /// from. Reconciling again from that stored presentation — which is what
    /// every later maximize, DPI change and style restore does — must not walk
    /// the chrome back to transparent.
    #[test]
    fn a_dx12_hwnd_fallback_keeps_opaque_chrome_across_later_reconciles() {
        use crate::presentation::NativeChromePolicy;
        let settings = WindowDescriptor::new("Scene");
        let first = resolved(
            &settings,
            MaterialEffect::Transparent,
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::Backend::Dx12,
        );
        assert_eq!(first.effective().effect, MaterialEffect::Solid);
        assert_eq!(
            first.effective().fallback,
            Some(MaterialFallback::NativeMaterialUnavailable)
        );
        assert_eq!(first.chrome(), Some(NativeChromePolicy::OPAQUE));

        // A later reconcile re-applies the effective material, whose surface
        // still answers Opaque. The chrome stays put and the material no
        // longer needs resetting, so the pair has settled.
        let again = resolved(
            &settings,
            first.effective().effect,
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::Backend::Dx12,
        );
        assert_eq!(again.chrome(), first.chrome());
        assert!(!again.needs_material_reset());
    }

    /// Mica and Acrylic report `wants_transparent_surface()` like a fully
    /// transparent surface does, but they are painted by DWM: dropping the
    /// round clip would erase the backdrop the window asked for.
    #[test]
    fn a_system_backdrop_keeps_the_round_clip_that_paints_it() {
        use crate::presentation::NativeChromePolicy;
        let settings = WindowDescriptor::new("Scene");
        for material in [MaterialEffect::Mica, MaterialEffect::Acrylic] {
            assert!(material.wants_transparent_surface());
            let chrome = resolved(
                &settings,
                material,
                wgpu::CompositeAlphaMode::PreMultiplied,
                wgpu::Backend::Dx12,
            )
            .chrome();
            assert_eq!(chrome, Some(NativeChromePolicy::OPAQUE));
        }
    }

    #[test]
    fn window_and_taskbar_icons_convert_independent_buffers() {
        // winit 的 Win32 后端原地翻转共享缓冲;同一 winit Icon 不允许被转换两次,
        // 否则任务栏大图标的 R/B 被换回、蓝色标记显示为橙黄。
        let source = WindowIcon::from_rgba(vec![73; 8 * 8 * 4], 8, 8).expect("valid icon source");
        let window_icon = winit_icon(&source).expect("window icon");
        let taskbar_icon = winit_icon(&source).expect("taskbar icon");
        assert!(
            !std::sync::Arc::ptr_eq(&window_icon.0, &taskbar_icon.0),
            "each applied icon must own its RGBA buffer"
        );
    }

    #[test]
    fn scene_windows_reclamp_offscreen_initial_positions_to_live_displays() {
        let mut settings = WindowDescriptor::new("Scene");
        settings.initial_size = (888.0, 586.0);
        settings.initial_position = Some((2100.0, 40.0));
        let main = Desktop {
            displays: vec![DisplayBounds {
                position: (0.0, 0.0),
                size: (1920.0, 1080.0),
            }],
            size_ratios: Vec::new(),
            scale: 1.0,
        };

        let attributes =
            scene_window_attributes(&settings, &main, WindowSurfaceTarget::NativeWindow);
        assert_eq!(
            attributes.position,
            Some(desktop_position((1032.0, 40.0), 1.0))
        );

        let disconnected = Desktop {
            displays: vec![
                main.displays[0],
                DisplayBounds {
                    position: (1920.0, 0.0),
                    size: (1080.0, 1920.0),
                },
            ],
            size_ratios: Vec::new(),
            scale: 1.0,
        };
        let attributes =
            scene_window_attributes(&settings, &disconnected, WindowSurfaceTarget::NativeWindow);
        assert_eq!(
            attributes.position,
            Some(desktop_position((2100.0, 40.0), 1.0))
        );

        settings.initial_position = None;
        assert_eq!(
            scene_window_attributes(&settings, &main, WindowSurfaceTarget::NativeWindow).position,
            None
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    /// A 100% primary display with a 125% display below it: a window saved on
    /// the second display reopens at the same desktop pixel, not on the seam.
    fn mixed_scale_displays_share_one_desktop_space() {
        let display = |x, y, scale, primary| nana_ui_platform::DisplayInfo {
            id: nana_ui_platform::DisplayId(u128::from(primary)),
            name: None,
            physical_position: Some((x, y)),
            physical_size: Some((1920, 1080)),
            scale_factor: scale,
            refresh_rate_millihertz: None,
            primary,
        };
        let primary = display(0, 0, 1.0, true);
        let below = display(-7, 1080, 1.25, false);
        let scale = desktop_scale(below.scale_factor, Some(primary.scale_factor));
        let displays: Vec<_> = [primary, below]
            .iter()
            .map(|display| display.logical_bounds(scale).unwrap())
            .collect();
        assert_eq!(displays[1].position, (-7.0, 1080.0));
        assert!(displays[0].position.1 + displays[0].size.1 <= displays[1].position.1);

        let mut settings = WindowDescriptor::new("Saved");
        settings.initial_size = (1280.0, 800.0);
        settings.initial_position = Some((116.0, 1127.0));
        let attributes = scene_window_attributes(
            &settings,
            &Desktop {
                displays,
                size_ratios: vec![1.0, 1.25],
                scale,
            },
            WindowSurfaceTarget::NativeWindow,
        );
        assert_eq!(
            attributes.position,
            Some(winit::dpi::PhysicalPosition::new(116, 1127).into())
        );
    }

    #[test]
    fn native_and_transparent_materials_clear_the_surface_to_zero_alpha() {
        let solid = scene_clear_color(ThemeMode::Dark, MaterialOutcome::chosen_solid(), None);
        assert!(solid[3] > 0.0, "opaque windows keep a readable clear color");
        assert_eq!(
            scene_clear_color(ThemeMode::Dark, MaterialOutcome::transparent(), None),
            [0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            scene_clear_color(
                ThemeMode::Dark,
                MaterialOutcome::native(MaterialEffect::Mica),
                None
            ),
            [0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            scene_clear_color(
                ThemeMode::Light,
                MaterialOutcome::native(MaterialEffect::Acrylic),
                None
            ),
            [0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn the_clear_colour_is_the_palette_colour_in_linear_light() {
        // The painter clears in linear light; #181818 must come out as
        // #181818 rather than encoded a second time.
        let [r, g, b, a] =
            scene_clear_color(ThemeMode::Dark, MaterialOutcome::chosen_solid(), None);
        let expected = ((24.0f32 / 255.0 + 0.055) / 1.055).powf(2.4);
        for channel in [r, g, b] {
            assert!((channel - expected).abs() < 1e-6, "{channel} != {expected}");
        }
        assert_eq!(a, 1.0);
    }

    #[test]
    fn an_opaque_window_clears_to_the_host_colour_rather_than_the_theme() {
        // A host whose window frames content of its own — a stage, a canvas —
        // wants one surround in both themes, so its answer wins over the
        // palette and does not move when the theme does.
        let black = nana_ui_core::SemanticColor::rgb8(0, 0, 0);
        for theme in [ThemeMode::Dark, ThemeMode::Light] {
            assert_eq!(
                scene_clear_color(theme, MaterialOutcome::chosen_solid(), Some(black)),
                [0.0, 0.0, 0.0, 1.0]
            );
        }
        // Without an answer the palette still decides, and the two modes differ.
        assert_ne!(
            scene_clear_color(ThemeMode::Dark, MaterialOutcome::chosen_solid(), None),
            scene_clear_color(ThemeMode::Light, MaterialOutcome::chosen_solid(), None)
        );
        // A transparent surface is still transparent: the host colour describes
        // what an opaque window fills with, not whether it is opaque.
        assert_eq!(
            scene_clear_color(
                ThemeMode::Light,
                MaterialOutcome::transparent(),
                Some(black)
            ),
            [0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn an_opaque_surface_reports_the_transparent_request_as_a_fallback() {
        use wgpu::Backend::{Dx12, Gl, Metal, Vulkan};
        use wgpu::CompositeAlphaMode::{Opaque, PostMultiplied, PreMultiplied};
        let demoted = MaterialOutcome::solid(MaterialFallback::NativeMaterialUnavailable);
        // Windows DX12 only ever advertises Opaque for an HWND surface, where
        // the transparent clear color shows as solid black. The request has to
        // come back as a reported fallback rather than silently look honoured.
        assert!(scene_clear_color(ThemeMode::Dark, demoted, None)[3] > 0.0);
        for requested in [
            MaterialOutcome::transparent(),
            MaterialOutcome::native(MaterialEffect::Mica),
            MaterialOutcome::native(MaterialEffect::Acrylic),
        ] {
            for backend in [Dx12, Vulkan, Metal] {
                assert_eq!(
                    effective_for(requested, Opaque, backend),
                    demoted,
                    "{backend:?} honours the configured alpha mode"
                );
                for alpha in [PreMultiplied, PostMultiplied] {
                    assert_eq!(effective_for(requested, alpha, backend), requested);
                }
            }
            // GLES hardcodes Opaque and never reads the configured mode back,
            // so it says nothing about whether the window composites.
            assert_eq!(effective_for(requested, Opaque, Gl), requested);
        }
        // An opaque request is already honoured by an opaque surface.
        for chosen in [
            MaterialOutcome::chosen_solid(),
            MaterialOutcome::solid(MaterialFallback::PlatformDoesNotProvideNativeMaterial),
        ] {
            assert_eq!(effective_for(chosen, Opaque, Dx12), chosen);
        }
    }

    /// The effective material a surface's answer produces, read off the
    /// resolved presentation rather than from a separate demotion helper.
    fn effective_for(
        applied: MaterialOutcome,
        alpha: wgpu::CompositeAlphaMode,
        backend: wgpu::Backend,
    ) -> MaterialOutcome {
        ResolvedWindowPresentation::resolve(
            &WindowDescriptor::new("Scene"),
            applied.effect,
            applied,
            alpha,
            backend,
            ResolvedSurfaceTarget::honoured(WindowSurfaceTarget::NativeWindow, false),
            nana_window::NonClientRenderingStrategy::StripFrameStyles,
        )
        .effective()
    }

    #[test]
    fn transparent_aux_keeps_transparent_when_primary_appearance_is_solid() {
        let appearance = MaterialEffect::Solid;
        assert_eq!(
            window_surface_effect(false, appearance),
            MaterialEffect::Solid
        );
        assert_eq!(
            window_surface_effect(true, appearance),
            MaterialEffect::Transparent
        );
        assert!(window_surface_effect(true, appearance).wants_transparent_surface());
        assert!(window_wants_transparent_surface(true, appearance));
        assert!(!window_wants_transparent_surface(false, appearance));
    }

    #[test]
    fn transparent_surface_picks_non_opaque_alpha_before_surface_lock() {
        let appearance = MaterialEffect::Solid;
        let transparent = window_wants_transparent_surface(true, appearance);
        assert!(transparent);
        assert_eq!(
            crate::hosted_context::preferred_alpha_mode(
                &[
                    wgpu::CompositeAlphaMode::Opaque,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    wgpu::CompositeAlphaMode::PostMultiplied,
                ],
                transparent,
            ),
            wgpu::CompositeAlphaMode::PreMultiplied
        );
        assert_eq!(
            crate::hosted_context::preferred_alpha_mode(
                &[
                    wgpu::CompositeAlphaMode::Opaque,
                    wgpu::CompositeAlphaMode::PostMultiplied,
                ],
                transparent,
            ),
            wgpu::CompositeAlphaMode::PostMultiplied
        );
        let opaque = window_wants_transparent_surface(false, appearance);
        assert!(!opaque);
        assert_eq!(
            crate::hosted_context::preferred_alpha_mode(
                &[
                    wgpu::CompositeAlphaMode::Opaque,
                    wgpu::CompositeAlphaMode::PostMultiplied,
                ],
                opaque,
            ),
            wgpu::CompositeAlphaMode::Opaque
        );
    }

    #[test]
    fn input_key_uses_named_debug_and_character_text() {
        assert_eq!(
            platform_input_key(&Key::Named(NamedKey::ArrowDown)),
            "ArrowDown"
        );
        assert_eq!(platform_input_key(&Key::Character("V".into())), "V");
        assert_eq!(
            platform_input_key(&Key::Unidentified(winit::keyboard::NativeKey::Unidentified)),
            "Unidentified"
        );
        // Physical keys are W3C codes, not the backend's debug spelling.
        assert_eq!(
            platform_physical_key(&winit::keyboard::PhysicalKey::Code(
                winit::keyboard::KeyCode::KeyA
            )),
            "KeyA"
        );
    }

    #[test]
    fn modifiers_map_control_alt_shift_and_meta() {
        let modifiers = platform_input_modifiers(
            ModifiersState::CONTROL
                | ModifiersState::ALT
                | ModifiersState::SHIFT
                | ModifiersState::META,
        );
        assert!(modifiers.control);
        assert!(modifiers.alt);
        assert!(modifiers.shift);
        assert!(modifiers.meta);
    }

    #[test]
    fn physical_cursor_positions_become_logical() {
        let mut tracker = InputTracker::default();
        tracker.set_cursor_physical(PhysicalPosition::new(100.0, 50.0), 2.0);
        assert_eq!(tracker.cursor, (50.0, 25.0));
        tracker.set_cursor_physical(PhysicalPosition::new(220.0, 110.0), 2.0);
        assert_eq!(tracker.cursor, (110.0, 55.0));
    }

    #[test]
    fn focus_gain_synthetic_press_is_not_delivered_but_real_keys_are() {
        let escape = |state, is_synthetic| WinitWindowEvent::KeyboardInput {
            device_id: None,
            event: winit::event::KeyEvent {
                physical_key: winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape),
                logical_key: Key::Named(NamedKey::Escape),
                text: None,
                location: winit::keyboard::KeyLocation::Standard,
                state,
                repeat: false,
                text_with_all_modifiers: None,
                key_without_modifiers: Key::Named(NamedKey::Escape),
            },
            is_synthetic,
        };
        let mut tracker = InputTracker::default();
        assert!(
            map_input(
                &mut tracker,
                &escape(ElementState::Pressed, true),
                1.0,
                None
            )
            .is_none()
        );
        assert!(matches!(
            map_input(
                &mut tracker,
                &escape(ElementState::Released, true),
                1.0,
                None
            ),
            Some(InputPayload::Key(KeyInput {
                state: KeyState::Released,
                ..
            }))
        ));
        assert!(matches!(
            map_input(&mut tracker, &escape(ElementState::Pressed, false), 1.0, None),
            Some(InputPayload::Key(ref key)) if key.is_pressed() && key.logical.0 == "Escape"
        ));
    }

    #[test]
    fn mouse_buttons_use_the_hosted_mask_contract() {
        assert_eq!(mouse_button_code(MouseButton::Left), 0);
        assert_eq!(mouse_button_code(MouseButton::Right), 2);
        assert_eq!(mouse_button_mask(0), 1);
        assert_eq!(mouse_button_mask(1), 4);
        assert_eq!(mouse_button_mask(2), 2);
    }

    #[test]
    fn a_window_move_follows_its_own_button_and_survives_a_synthetic_cancel() {
        const MIDDLE: i16 = 1;
        // A middle-button gesture owns the move: the window follows, and the
        // left release of an unrelated click does not drop it.
        assert_eq!(
            frame_move_step(PointerPhase::Move, -1, MIDDLE),
            FrameMoveStep::Follow
        );
        assert_eq!(
            frame_move_step(PointerPhase::Up, PRIMARY_MOUSE_BUTTON, MIDDLE),
            FrameMoveStep::Hold
        );
        assert_eq!(
            frame_move_step(PointerPhase::Up, MIDDLE, MIDDLE),
            FrameMoveStep::Finish
        );
        // Win32 reports a drag crossing the window bounds as a cancel while
        // the gesture still holds the pointer; the move must outlive it.
        assert_eq!(
            frame_move_step(PointerPhase::Cancel, -1, MIDDLE),
            FrameMoveStep::Hold
        );
        // A primary press stands in for a release that never arrived.
        assert_eq!(
            frame_move_step(PointerPhase::Down, PRIMARY_MOUSE_BUTTON, MIDDLE),
            FrameMoveStep::Finish
        );
    }

    #[test]
    fn a_held_gesture_reports_its_primary_button_first() {
        assert_eq!(held_mouse_button(0), None);
        assert_eq!(held_mouse_button(mouse_button_mask(1)), Some(1));
        assert_eq!(held_mouse_button(mouse_button_mask(2)), Some(2));
        assert_eq!(
            held_mouse_button(mouse_button_mask(PRIMARY_MOUSE_BUTTON) | mouse_button_mask(1)),
            Some(PRIMARY_MOUSE_BUTTON)
        );
    }

    #[test]
    fn mouse_cancel_ends_a_press_whose_release_never_arrives() {
        let mut tracker = InputTracker::default();
        map_input(
            &mut tracker,
            &WinitWindowEvent::PointerButton {
                device_id: None,
                state: ElementState::Pressed,
                position: PhysicalPosition::new(20.0, 10.0),
                primary: true,
                button: ButtonSource::Mouse(MouseButton::Left),
                is_macos_activation_click: false,
            },
            1.0,
            None,
        );
        let PointerInput {
            phase,
            pointer_type,
            buttons,
            ..
        } = tracker.cancel_mouse(None);
        assert_eq!(phase, PointerPhase::Cancel);
        assert_eq!(pointer_type, PointerType::Mouse);
        assert_eq!(buttons, 1);

        let Some(InputPayload::Pointer(PointerInput { phase, buttons, .. })) = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerMoved {
                device_id: None,
                position: PhysicalPosition::new(60.0, 10.0),
                primary: true,
                source: PointerSource::Mouse,
            },
            1.0,
            None,
        ) else {
            panic!("expected pointer");
        };
        assert_eq!(phase, PointerPhase::Move);
        assert_eq!(buttons, 0);
    }

    #[test]
    fn pointer_move_down_and_leave_match_hosted_coordinates() {
        let mut tracker = InputTracker::default();
        let moved = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerMoved {
                device_id: None,
                position: PhysicalPosition::new(20.0, 40.0),
                primary: true,
                source: PointerSource::Mouse,
            },
            2.0,
            Some(ScreenSpace {
                origin: (100.0, 200.0),
                client_ratio: 1.0,
            }),
        )
        .expect("cursor move");
        let InputPayload::Pointer(PointerInput {
            phase,
            x,
            y,
            screen_x,
            screen_y,
            pointer_type,
            button,
            ..
        }) = moved
        else {
            panic!("expected pointer");
        };
        assert_eq!(phase, PointerPhase::Move);
        assert_eq!(pointer_type, PointerType::Mouse);
        assert_eq!((x, y), (10.0, 20.0));
        assert_eq!((screen_x, screen_y), (110.0, 220.0));
        assert_eq!(button, -1);

        let down = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerButton {
                device_id: None,
                state: ElementState::Pressed,
                position: PhysicalPosition::new(20.0, 40.0),
                primary: true,
                button: ButtonSource::Mouse(MouseButton::Left),
                is_macos_activation_click: false,
            },
            2.0,
            Some(ScreenSpace {
                origin: (100.0, 200.0),
                client_ratio: 1.0,
            }),
        )
        .expect("mouse down");
        let InputPayload::Pointer(PointerInput {
            phase,
            buttons,
            pressure,
            ..
        }) = down
        else {
            panic!("expected pointer");
        };
        assert_eq!(phase, PointerPhase::Down);
        assert_eq!(buttons, 1);
        assert_eq!(pressure, 0.5);

        let activation = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerButton {
                device_id: None,
                state: ElementState::Pressed,
                position: PhysicalPosition::new(20.0, 40.0),
                primary: true,
                button: ButtonSource::Mouse(MouseButton::Left),
                is_macos_activation_click: true,
            },
            2.0,
            Some(ScreenSpace {
                origin: (100.0, 200.0),
                client_ratio: 1.0,
            }),
        )
        .expect("activation down");
        let InputPayload::Pointer(PointerInput {
            activation_click, ..
        }) = activation
        else {
            panic!("expected pointer");
        };
        assert!(activation_click);

        let left = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerLeft {
                device_id: None,
                position: None,
                primary: true,
                kind: PointerKind::Mouse,
            },
            2.0,
            Some(ScreenSpace {
                origin: (100.0, 200.0),
                client_ratio: 1.0,
            }),
        )
        .expect("cursor left");
        // Leaving is not a cancel: hover goes, a capture keeps its pointer.
        assert_eq!(
            left,
            InputPayload::PointerLeave {
                pointer_id: PointerId(1)
            }
        );
        assert_eq!(tracker.cursor.0, 10.0);
        assert_eq!(tracker.buttons, 0);
    }

    #[test]
    fn pointer_enter_emits_move_without_a_followup_moved() {
        let mut tracker = InputTracker::default();
        let entered = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerEntered {
                device_id: None,
                position: PhysicalPosition::new(20.0, 40.0),
                primary: true,
                kind: PointerKind::Mouse,
            },
            2.0,
            Some(ScreenSpace {
                origin: (100.0, 200.0),
                client_ratio: 1.0,
            }),
        )
        .expect("pointer enter");
        let InputPayload::Pointer(PointerInput {
            phase,
            x,
            y,
            screen_x,
            screen_y,
            pointer_type,
            button,
            ..
        }) = entered
        else {
            panic!("expected pointer");
        };
        assert_eq!(phase, PointerPhase::Move);
        assert_eq!(pointer_type, PointerType::Mouse);
        assert_eq!((x, y), (10.0, 20.0));
        assert_eq!((screen_x, screen_y), (110.0, 220.0));
        assert_eq!(button, -1);
    }

    #[test]
    fn pointer_leave_uses_event_position_when_present() {
        let mut tracker = InputTracker {
            cursor: (1.0, 1.0),
            ..InputTracker::default()
        };
        let left = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerLeft {
                device_id: None,
                position: Some(PhysicalPosition::new(40.0, 80.0)),
                primary: true,
                kind: PointerKind::Mouse,
            },
            2.0,
            None,
        )
        .expect("pointer leave");
        assert!(matches!(left, InputPayload::PointerLeave { .. }));
        assert_eq!(tracker.cursor, (20.0, 40.0));
    }

    #[test]
    fn tablet_pointer_ids_split_device_and_tool_kind() {
        let pen = DeviceId::from_raw(2);
        let other = DeviceId::from_raw(3);
        assert_ne!(
            tablet_pointer_id(Some(pen), TabletToolKind::Pen),
            tablet_pointer_id(Some(pen), TabletToolKind::Eraser)
        );
        assert_ne!(
            tablet_pointer_id(Some(pen), TabletToolKind::Pen),
            tablet_pointer_id(Some(other), TabletToolKind::Pen)
        );
        let mut tracker = InputTracker::default();
        let moved = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerMoved {
                device_id: Some(pen),
                position: PhysicalPosition::new(4.0, 8.0),
                primary: true,
                source: PointerSource::TabletTool {
                    kind: TabletToolKind::Eraser,
                    data: TabletToolData::default(),
                },
            },
            1.0,
            None,
        )
        .expect("pen move");
        let InputPayload::Pointer(PointerInput {
            pointer_id,
            pointer_type,
            ..
        }) = moved
        else {
            panic!("expected pointer");
        };
        assert_eq!(pointer_type, PointerType::Pen);
        assert_eq!(
            pointer_id.0,
            tablet_pointer_id(Some(pen), TabletToolKind::Eraser)
        );
        assert_ne!(pointer_id.0, 1000);
    }

    #[test]
    fn wheel_preserves_line_delta_and_converts_pixels() {
        let mut tracker = InputTracker {
            cursor: (8.0, 16.0),
            ..InputTracker::default()
        };
        let line = map_input(
            &mut tracker,
            &WinitWindowEvent::MouseWheel {
                device_id: None,
                delta: MouseScrollDelta::LineDelta(1.0, -2.0),
                phase: TouchPhase::Moved,
            },
            2.0,
            None,
        )
        .expect("line wheel");
        assert_eq!(
            line,
            InputPayload::Wheel(WheelInput {
                pointer_id: PointerId(1),
                x: 8.0,
                y: 16.0,
                delta_x: 1.0,
                delta_y: -2.0,
                unit: WheelUnit::Lines,
                modifiers: Default::default(),
            })
        );

        let pixel = map_input(
            &mut tracker,
            &WinitWindowEvent::MouseWheel {
                device_id: None,
                delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(8.0, -4.0)),
                phase: TouchPhase::Moved,
            },
            2.0,
            None,
        )
        .expect("pixel wheel");
        let InputPayload::Wheel(WheelInput {
            delta_x,
            delta_y,
            unit,
            ..
        }) = pixel
        else {
            panic!("expected wheel");
        };
        assert_eq!((delta_x, delta_y), (4.0, -2.0));
        assert_eq!(unit, WheelUnit::Pixels);
    }

    #[test]
    fn first_touch_is_primary_and_uses_hosted_pointer_id() {
        let mut tracker = InputTracker::default();
        let start = map_input(
            &mut tracker,
            &WinitWindowEvent::PointerButton {
                device_id: None,
                state: ElementState::Pressed,
                position: PhysicalPosition::new(4.0, 8.0),
                primary: true,
                button: ButtonSource::Touch {
                    finger_id: FingerId::from_raw(3),
                    force: None,
                },
                is_macos_activation_click: false,
            },
            2.0,
            None,
        )
        .expect("touch start");
        let InputPayload::Pointer(PointerInput {
            phase,
            pointer_id,
            pointer_type,
            is_primary,
            x,
            y,
            buttons,
            ..
        }) = start
        else {
            panic!("expected pointer");
        };
        assert_eq!(phase, PointerPhase::Down);
        assert_eq!(pointer_id, PointerId(5));
        assert_eq!(pointer_type, PointerType::Touch);
        assert!(is_primary);
        assert_eq!((x, y), (2.0, 4.0));
        assert_eq!(buttons, 1);
    }

    #[test]
    fn ime_and_window_lifecycle_mapping_is_backend_neutral() {
        assert_eq!(
            platform_composition(&winit::event::Ime::Commit("你".into())),
            CompositionInput::Commit("你".into())
        );
        assert_eq!(
            platform_composition(&winit::event::Ime::Preedit("かな".into(), Some((3, 6)))),
            CompositionInput::Update {
                text: "かな".into(),
                selection: Some((3, 6)),
            }
        );
        assert_eq!(
            platform_composition(&winit::event::Ime::DeleteSurrounding {
                before_bytes: 3,
                after_bytes: 0,
            }),
            CompositionInput::DeleteSurrounding {
                before_bytes: 3,
                after_bytes: 0,
            }
        );
        assert_eq!(
            platform_window_event(
                &WinitWindowEvent::CloseRequested,
                WindowId::PRIMARY,
                geometry(),
            ),
            Some(WindowEvent::CloseRequested {
                id: WindowId::PRIMARY
            })
        );
        assert_eq!(
            platform_window_event(
                &WinitWindowEvent::Focused(true),
                WindowId::PRIMARY,
                geometry(),
            ),
            Some(WindowEvent::FocusChanged {
                id: WindowId::PRIMARY,
                focused: true,
            })
        );
        assert_eq!(
            platform_window_event(
                &WinitWindowEvent::Occluded(true),
                WindowId::PRIMARY,
                geometry(),
            ),
            Some(WindowEvent::VisibilityChanged {
                id: WindowId::PRIMARY,
                hidden: true,
            })
        );
        // Composition is input, routed and observed like any other.
        assert!(
            platform_window_event(
                &WinitWindowEvent::Ime(winit::event::Ime::Enabled),
                WindowId::PRIMARY,
                geometry(),
            )
            .is_none()
        );
        assert!(
            platform_window_event(
                &WinitWindowEvent::RedrawRequested,
                WindowId::PRIMARY,
                geometry(),
            )
            .is_none()
        );
    }

    #[test]
    fn file_drag_batches_hover_paths_and_emits_one_drop() {
        let mut tracker = InputTracker {
            cursor: (24.0, 48.0),
            ..InputTracker::default()
        };
        let transfer = winit::data_transfer::DataTransferId::from_raw(1);
        assert!(matches!(
            tracker.map_file_window_event(&WinitWindowEvent::DragEntered {
                id: transfer,
                position: None,
            },),
            Some(FileDragInput {
                kind: FileDragKind::Hover,
                ..
            })
        ));
        assert!(matches!(
            tracker.map_file_window_event(&WinitWindowEvent::DragDropped {
                id: transfer,
                proposed_action: None,
            },),
            Some(FileDragInput {
                kind: FileDragKind::Drop,
                ..
            })
        ));
        assert!(
            tracker
                .map_file_window_event(&WinitWindowEvent::DragDropped {
                    id: transfer,
                    proposed_action: None,
                },)
                .is_none()
        );

        let mut cancelled = InputTracker::default();
        cancelled.map_file_window_event(&WinitWindowEvent::DragEntered {
            id: transfer,
            position: None,
        });
        assert!(matches!(
            cancelled.map_file_window_event(&WinitWindowEvent::DragLeft { id: transfer },),
            Some(FileDragInput {
                kind: FileDragKind::Cancel,
                ..
            })
        ));
    }

    #[test]
    fn an_unreadable_release_ends_the_drag() {
        let transfer = winit::data_transfer::DataTransferId::from_raw(5);
        let mut tracker = InputTracker::default();
        tracker.begin_file_drag(transfer, None);
        // Hovering, not releasing: nothing to abandon.
        assert!(tracker.abandon_drop(transfer).is_none());
        tracker.wait_for_drop_data(transfer, winit::event_loop::AsyncRequestSerial::get());
        assert_eq!(
            tracker.abandon_drop(transfer),
            Some(FileDragInput::cancel())
        );
        assert!(tracker.abandon_drop(transfer).is_none());
    }

    #[test]
    fn file_drag_events_carry_the_held_modifiers() {
        // Without a system sample (Linux) the tracked state is reported.
        let transfer = winit::data_transfer::DataTransferId::from_raw(3);
        let mut tracker = InputTracker {
            modifiers: ModifiersState::CONTROL,
            ..InputTracker::default()
        };
        let expected = nana_window::keyboard_modifiers()
            .map(|keys| keys.control)
            .unwrap_or(true);
        let hovered = tracker.map_file_window_event(&WinitWindowEvent::DragEntered {
            id: transfer,
            position: None,
        });
        assert!(matches!(
            hovered,
            Some(FileDragInput { kind: FileDragKind::Hover, modifiers, .. }) if modifiers.control == expected
        ));
        let dropped = tracker.map_file_window_event(&WinitWindowEvent::DragDropped {
            id: transfer,
            proposed_action: None,
        });
        assert!(matches!(
            dropped,
            Some(FileDragInput { kind: FileDragKind::Drop, modifiers, .. }) if modifiers.control == expected
        ));
    }

    #[test]
    fn file_drag_ingests_fetched_paths_before_and_after_drop() {
        let transfer = winit::data_transfer::DataTransferId::from_raw(7);
        let paths = vec![std::path::PathBuf::from("/tmp/nana.txt")];
        let mut hover = InputTracker {
            cursor: (8.0, 16.0),
            ..InputTracker::default()
        };
        hover.begin_file_drag(transfer, None);
        assert_eq!(
            hover.ingest_file_paths(transfer, paths.clone()),
            Some(FileDragInput {
                kind: FileDragKind::Hover,
                paths: paths.clone(),
                position: Some((8.0, 16.0)),
                modifiers: InputModifiers::default(),
            })
        );
        assert_eq!(
            hover.map_file_window_event(&WinitWindowEvent::DragDropped {
                id: transfer,
                proposed_action: None,
            },),
            Some(FileDragInput {
                kind: FileDragKind::Drop,
                paths: paths.clone(),
                position: Some((8.0, 16.0)),
                modifiers: InputModifiers::default(),
            })
        );

        let mut delayed = InputTracker {
            modifiers: ModifiersState::CONTROL,
            ..InputTracker::default()
        };
        delayed.wait_for_drop_data(transfer, winit::event_loop::AsyncRequestSerial::get());
        // Released before the paths arrive: the drop keeps the keys it had.
        delayed.modifiers = ModifiersState::empty();
        let held = nana_window::keyboard_modifiers().map_or(
            InputModifiers {
                control: true,
                ..InputModifiers::default()
            },
            system_input_modifiers,
        );
        assert_eq!(
            delayed.ingest_file_paths(transfer, paths.clone()),
            Some(FileDragInput {
                kind: FileDragKind::Drop,
                paths,
                position: Some((0.0, 0.0)),
                modifiers: held,
            })
        );
        assert!(
            delayed
                .map_file_window_event(&WinitWindowEvent::DragDropped {
                    id: transfer,
                    proposed_action: None,
                },)
                .is_none()
        );
    }

    #[test]
    fn screen_position_falls_back_to_client_without_origin() {
        assert_eq!(screen_position(None, (3.0, 4.0)), (3.0, 4.0));
        assert_eq!(
            screen_position(
                Some(ScreenSpace {
                    origin: (10.0, 20.0),
                    client_ratio: 1.0,
                }),
                (3.0, 4.0)
            ),
            (13.0, 24.0)
        );
    }

    /// The origin is desktop-logical while the client point is in the window's
    /// own logical scale. On a mixed-DPI desktop those are different units, and
    /// adding them as they stand puts the screen position off by the ratio —
    /// which is exactly what a context menu or tooltip is placed with.
    #[test]
    fn a_client_point_is_converted_before_it_is_added_to_the_origin() {
        // A 2x window on a 1x desktop: 100 window-logical px span 200 of them.
        let space = ScreenSpace {
            origin: (400.0, 50.0),
            client_ratio: 2.0,
        };
        assert_eq!(screen_position(Some(space), (100.0, 25.0)), (600.0, 100.0));
        // And a 1x window on a 2x desktop the other way round.
        let space = ScreenSpace {
            origin: (400.0, 50.0),
            client_ratio: 0.5,
        };
        assert_eq!(screen_position(Some(space), (100.0, 24.0)), (450.0, 62.0));
    }

    #[test]
    fn window_commands_route_by_known_ids_without_a_surface() {
        let primary = WindowId::PRIMARY;
        let tool = WindowId(7);
        let known = [primary, tool];
        let settings = WindowDescriptor::new("tool");

        assert_eq!(
            route_window_command(
                &WindowCommand::Open {
                    id: tool,
                    settings: settings.clone(),
                },
                &known
            ),
            RoutedWindowCommand::Focus(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::Open {
                    id: WindowId(9),
                    settings,
                },
                &known
            ),
            RoutedWindowCommand::Open(WindowId(9))
        );
        assert_eq!(
            route_window_command(&WindowCommand::Close(primary), &known),
            RoutedWindowCommand::Close(primary)
        );
        assert_eq!(
            route_window_command(&WindowCommand::Close(tool), &known),
            RoutedWindowCommand::Close(tool)
        );
        assert_eq!(
            route_window_command(&WindowCommand::Close(WindowId(3)), &known),
            RoutedWindowCommand::Ignore
        );
        assert_eq!(
            route_window_command(&WindowCommand::Focus(tool), &known),
            RoutedWindowCommand::Focus(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetTitle {
                    id: tool,
                    title: "Aux".into(),
                },
                &known
            ),
            RoutedWindowCommand::SetTitle(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::Move {
                    id: tool,
                    position: (8.0, 16.0),
                },
                &known
            ),
            RoutedWindowCommand::Move(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetBounds {
                    id: primary,
                    position: (0.0, 0.0),
                    size: (100.0, 80.0),
                },
                &known
            ),
            RoutedWindowCommand::SetBounds(primary)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetFullscreen {
                    id: WindowId(3),
                    fullscreen: Some(nana_ui_platform::FullscreenRequest::default()),
                },
                &known
            ),
            RoutedWindowCommand::Ignore
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetMinimized {
                    id: tool,
                    minimized: true,
                },
                &known
            ),
            RoutedWindowCommand::SetMinimized(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetMaximized {
                    id: tool,
                    maximized: true,
                },
                &known
            ),
            RoutedWindowCommand::SetMaximized(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetAlwaysOnTop {
                    id: tool,
                    always_on_top: true,
                },
                &known
            ),
            RoutedWindowCommand::SetAlwaysOnTop(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetNativeWindowControlsVisible {
                    id: tool,
                    visible: false,
                    duration: std::time::Duration::from_millis(140),
                },
                &known
            ),
            RoutedWindowCommand::SetNativeWindowControlsVisible(tool)
        );
        assert_eq!(
            route_window_command(
                &WindowCommand::SetIcon {
                    id: tool,
                    icon: None,
                },
                &known
            ),
            RoutedWindowCommand::SetIcon(tool)
        );
        assert_eq!(
            route_window_command(&WindowCommand::SetApplicationIcon { icon: None }, &known),
            RoutedWindowCommand::SetApplicationIcon
        );
        assert_eq!(
            route_window_command(&WindowCommand::Drag(tool), &known),
            RoutedWindowCommand::Drag(tool)
        );
        assert_eq!(
            route_window_command(&WindowCommand::Drag(WindowId(3)), &known),
            RoutedWindowCommand::Ignore
        );
    }

    #[test]
    fn client_frame_resize_hits_edges_unless_caption_or_maximized() {
        use super::frame_resize_edge_for;
        use winit::cursor::CursorIcon;

        let mut settings = WindowDescriptor::new("Scene");
        let mut geometry = geometry();
        geometry.logical_size = (800.0, 600.0);
        assert_eq!(
            frame_resize_edge_for(&settings, &geometry, false, 2.0, 300.0),
            Some(WindowResizeEdge::West)
        );
        assert_eq!(
            frame_resize_edge_for(&settings, &geometry, false, 400.0, 300.0),
            None
        );
        settings.system_caption = true;
        assert!(frame_resize_edge_for(&settings, &geometry, false, 2.0, 300.0).is_none());
        settings.system_caption = false;
        settings.resizable = false;
        assert!(frame_resize_edge_for(&settings, &geometry, false, 2.0, 300.0).is_none());
        settings.resizable = true;
        geometry.maximized = true;
        assert!(frame_resize_edge_for(&settings, &geometry, false, 2.0, 300.0).is_none());
        geometry.maximized = false;
        assert!(frame_resize_edge_for(&settings, &geometry, true, 2.0, 300.0).is_none());
        assert_eq!(
            frame_edge_cursor(WindowResizeEdge::East),
            CursorIcon::EwResize
        );
        assert_eq!(
            frame_edge_cursor(WindowResizeEdge::South),
            CursorIcon::NsResize
        );
        assert_eq!(
            frame_edge_cursor(WindowResizeEdge::NorthEast),
            CursorIcon::NeswResize
        );
        assert_eq!(
            frame_edge_cursor(WindowResizeEdge::SouthEast),
            CursorIcon::NwseResize
        );
    }

    #[test]
    fn window_cursor_matches_css_cursor_spec() {
        use crate::WindowCursor;
        use winit::cursor::CursorIcon;
        assert_eq!(
            window_cursor_override(WindowCursor::Automatic),
            (None, None)
        );
        assert_eq!(
            window_cursor_override(WindowCursor::Help),
            (Some(CursorIcon::Help), None)
        );
        assert_eq!(
            window_cursor_override(WindowCursor::Progress),
            (Some(CursorIcon::Progress), None)
        );
        assert_eq!(
            window_cursor_override(WindowCursor::ZoomIn),
            (Some(CursorIcon::ZoomIn), None)
        );
        assert_eq!(
            window_cursor_override(WindowCursor::ZoomOut),
            (Some(CursorIcon::ZoomOut), None)
        );
        assert_eq!(
            window_cursor_override(WindowCursor::None),
            (Some(CursorIcon::Default), Some(false))
        );
    }

    #[test]
    fn redraw_requests_ignore_unknown_ids_and_cover_every_known_window() {
        let primary = WindowId::PRIMARY;
        let tool = WindowId(2);
        let known = [primary, tool];

        assert!(windows_to_redraw(RuntimeRedraw::None, &known).is_empty());
        assert_eq!(
            windows_to_redraw(RuntimeRedraw::Window(tool), &known),
            vec![tool]
        );
        assert!(windows_to_redraw(RuntimeRedraw::Window(WindowId(9)), &known).is_empty());
        assert_eq!(
            windows_to_redraw(RuntimeRedraw::All, &known),
            vec![primary, tool]
        );
    }

    #[test]
    fn image_target_index_replaces_and_shares_resource_keys() {
        use std::collections::{HashMap, HashSet};

        let first = WindowId::PRIMARY;
        let second = WindowId(2);
        let mut targets = HashMap::new();
        let mut window_keys = HashMap::new();
        replace_image_target_index(
            &mut targets,
            &mut window_keys,
            first,
            HashSet::from(["shared.png".to_string(), "old.png".to_string()]),
        );
        replace_image_target_index(
            &mut targets,
            &mut window_keys,
            second,
            HashSet::from(["shared.png".to_string(), "second.png".to_string()]),
        );
        assert_eq!(targets["shared.png"], HashSet::from([first, second]));

        replace_image_target_index(
            &mut targets,
            &mut window_keys,
            first,
            HashSet::from(["new.png".to_string()]),
        );
        assert!(!targets.contains_key("old.png"));
        assert_eq!(targets["shared.png"], HashSet::from([second]));
        assert_eq!(targets["new.png"], HashSet::from([first]));
    }

    #[test]
    fn image_scene_keys_include_all_url_backed_surface_sources() {
        use std::collections::HashSet;

        let surface = nana_ui_scene::QuadSurfacePaint {
            background_image: Some(nana_ui_core::BackgroundImage::url("background.png")),
            background_layers: vec![nana_ui_core::BackgroundImage::url("layer.png")],
            content_image: Some(nana_ui_core::BackgroundImage::url("content.png")),
            mask: Some(nana_ui_core::MaskImage::Url("mask.png".into())),
            border_image: Some(nana_ui_core::BorderImageSpec::from_source(
                nana_ui_core::BackgroundImage::url("border.png"),
            )),
            ..Default::default()
        };

        let mut keys = HashSet::new();
        surface_image_keys(&surface, &mut keys);
        assert_eq!(
            keys,
            HashSet::from([
                "background.png".to_string(),
                "layer.png".to_string(),
                "content.png".to_string(),
                "mask.png".to_string(),
                "border.png".to_string(),
            ])
        );
    }

    #[test]
    fn image_target_index_removes_closed_window_without_leaking_keys() {
        use std::collections::{HashMap, HashSet};

        let first = WindowId::PRIMARY;
        let second = WindowId(2);
        let mut targets = HashMap::new();
        let mut window_keys = HashMap::new();
        replace_image_target_index(
            &mut targets,
            &mut window_keys,
            first,
            HashSet::from(["shared.png".to_string()]),
        );
        replace_image_target_index(
            &mut targets,
            &mut window_keys,
            second,
            HashSet::from(["shared.png".to_string(), "only-second.png".to_string()]),
        );
        remove_image_target_index(&mut targets, &mut window_keys, second);
        assert_eq!(targets["shared.png"], HashSet::from([first]));
        assert!(!targets.contains_key("only-second.png"));
        assert!(!window_keys.contains_key(&second));
    }

    #[test]
    fn runtime_prevent_default_still_invokes_the_program_input_hook() {
        let update = scene_runtime_input_update(
            InputDisposition {
                handled: true,
                prevent_default: true,
            },
            WindowId::PRIMARY,
            Ok(RuntimeProgramUpdate::exit()),
        );
        assert!(update.exit);
        assert_eq!(update.redraw, RuntimeRedraw::Window(WindowId::PRIMARY));

        let update = scene_runtime_input_update(
            InputDisposition {
                handled: false,
                prevent_default: false,
            },
            WindowId::PRIMARY,
            Ok(RuntimeProgramUpdate::default()),
        );
        assert_eq!(update.redraw, RuntimeRedraw::None);
    }

    #[test]
    fn failed_program_input_degrades_without_panicking() {
        let update = scene_runtime_input_update(
            InputDisposition {
                handled: true,
                prevent_default: true,
            },
            WindowId::PRIMARY,
            Err(FrameworkError::InvalidAction),
        );
        // The failed handler's effect is dropped, but the Runtime's
        // prevent_default redraw still happens instead of a panic.
        assert_eq!(update.redraw, RuntimeRedraw::Window(WindowId::PRIMARY));
        assert!(!update.exit);
        assert!(update.window_commands.is_empty());
    }

    #[test]
    fn device_recovery_invalidates_cloned_program_host_textures_before_rebuild() {
        let registry = occupied_host_textures("live");
        struct FakeProgram {
            textures: HostTextureRegistry,
            rebuilt_len: std::cell::Cell<Option<usize>>,
        }
        impl FakeProgram {
            fn host_textures(&self, id: WindowId) -> Option<HostTextureRegistry> {
                match id {
                    WindowId::PRIMARY | WindowId(2) => Some(self.textures.clone()),
                    _ => None,
                }
            }

            fn rebuild_gpu(&self) {
                self.rebuilt_len.set(Some(self.textures.len()));
            }
        }

        let program = FakeProgram {
            textures: registry.clone(),
            rebuilt_len: std::cell::Cell::new(None),
        };
        let cleared =
            invalidate_program_host_textures([WindowId::PRIMARY, WindowId(2), WindowId(9)], |id| {
                program.host_textures(id)
            });
        program.rebuild_gpu();

        assert_eq!(cleared, 1);
        assert_eq!(program.rebuilt_len.get(), Some(0));
        assert!(registry.is_empty());
        assert_eq!(
            invalidate_program_host_textures([WindowId::PRIMARY, WindowId(2)], |id| program
                .host_textures(id)),
            0
        );
    }

    fn occupied_host_textures(slot: &str) -> HostTextureRegistry {
        let registry = HostTextureRegistry::new();
        registry.register(
            slot,
            HostTexture::new(1, 1, &crate::test_gpu::texture(1, 1)),
            8,
            8,
            HostTextureAlphaMode::Premultiplied,
        );
        registry
    }

    #[test]
    fn acknowledged_commands_route_missing_windows_for_failure_reports() {
        for id in [WindowId::PRIMARY, WindowId(20)] {
            assert_eq!(
                route_window_command(
                    &WindowCommand::SetSkipTaskbar {
                        id,
                        skip_taskbar: true,
                    },
                    &[WindowId::PRIMARY]
                ),
                RoutedWindowCommand::SetSkipTaskbar(id, true)
            );
            assert_eq!(
                route_window_command(
                    &WindowCommand::SetMousePassthrough { id, enabled: true },
                    &[WindowId::PRIMARY]
                ),
                RoutedWindowCommand::SetMousePassthrough(id, MousePassthroughMode::Passthrough)
            );
            assert_eq!(
                route_window_command(
                    &WindowCommand::SetMousePassthroughForward { id, enabled: true },
                    &[WindowId::PRIMARY]
                ),
                RoutedWindowCommand::SetMousePassthrough(id, MousePassthroughMode::Forward)
            );
            assert_eq!(
                route_window_command(
                    &WindowCommand::SetMousePassthroughForward { id, enabled: false },
                    &[WindowId::PRIMARY]
                ),
                RoutedWindowCommand::SetMousePassthrough(id, MousePassthroughMode::Off)
            );
        }
    }

    fn pointer_down_event() -> WinitWindowEvent {
        WinitWindowEvent::PointerButton {
            device_id: None,
            state: ElementState::Pressed,
            position: PhysicalPosition::new(20.0, 40.0),
            primary: true,
            button: ButtonSource::Mouse(MouseButton::Left),
            is_macos_activation_click: false,
        }
    }

    #[test]
    fn forward_passthrough_drops_down_until_hit_testing_recovers() {
        use super::forward_pointer_action;
        let down = pointer_down_event();
        assert_eq!(
            forward_pointer_action(MousePassthroughMode::Forward, true, true, &down),
            ForwardPointerAction::IgnoreUntilRecovered,
            "OS pointer, including Down, must not reach widgets while hit-testing is still off"
        );
        assert_eq!(
            forward_pointer_action(MousePassthroughMode::Forward, false, true, &down),
            ForwardPointerAction::Dispatch,
            "Down is delivered only after sampling recovered hit-testing over content"
        );
        assert_eq!(
            forward_pointer_action(MousePassthroughMode::Forward, false, false, &down),
            ForwardPointerAction::RestorePassthrough
        );
        assert_eq!(
            forward_pointer_action(MousePassthroughMode::Off, true, true, &down),
            ForwardPointerAction::Dispatch
        );
    }
}
