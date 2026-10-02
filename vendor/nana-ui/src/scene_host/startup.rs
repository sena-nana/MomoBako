//! The standalone host's startup (Issue #225): the window, its device request
//! on a thread of its own and its splash first, then the program and the
//! handoff.
//!
//! The ordering rules live in [`StartupCoordinator`], kept off the window and
//! GPU calls so each of them can be tested on a machine with no display. The
//! host feeds it what happened — the program became ready, a takeover was
//! asked for or withdrawn, a frame was presented — and asks it two questions:
//! may the primary window present yet, and does this presented frame end the
//! splash. Frames that were skipped, retried or failed never reach it: the
//! host only reports a frame after its `present` succeeded.
//!
//! Everything that only matters until the handoff lives in [`ActiveStartup`]
//! and is dropped with it; afterwards the host keeps nothing but the record.

use std::sync::atomic::{AtomicBool, Ordering};

use nana_diagnostics::framework::host;
use nana_window::{NativeSplash, SplashHandoff};

use super::*;
use crate::hosted_context::{AcquiredDevice, DeviceRequest, PendingPrimarySurface};
use crate::presentation::ResolvedSurfaceTarget;
use crate::startup::{
    SplashOutcome, SplashSkip, StartupError, StartupHandle, StartupOptions, StartupPhase,
    StartupRequest, StartupStatus, StartupTakeover, StartupTicket,
};

/// How long the Windows handoff waits between checks that the takeover
/// frame's GPU work has completed. Only while that one frame is in flight.
const LATCH_POLL: Duration = Duration::from_millis(2);

/// The milestones `host.startup_phase` reports, by their stable number.
#[derive(Clone, Copy)]
#[repr(u64)]
pub(super) enum StartupMark {
    Entry = 0,
    SplashCommitted = 1,
    UiReady = 2,
    TakeoverRequested = 3,
    TakeoverFrame = 4,
    HandedOff = 5,
    SplashReleased = 6,
}

pub(super) fn startup_event(mark: StartupMark, at: Duration) {
    nana_diagnostics::event!(
        host::STARTUP_PHASE,
        phase = mark as u64,
        elapsed_ns = u64::try_from(at.as_nanos()).unwrap_or(u64::MAX)
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupCoordinator {
    phase: StartupPhase,
    /// Current ticket generation. A cancel retires it.
    generation: u64,
    /// A native splash covers the primary window.
    splash: bool,
    /// The primary window's flush sequence when the takeover was asked for.
    /// Only a frame flushed after it shows the content the request named.
    requested_after: Option<u64>,
    /// The takeover frame has been presented; only the splash's removal is
    /// left. Nothing can be withdrawn any more.
    committed: bool,
}

impl StartupCoordinator {
    pub(super) const fn new(splash: bool) -> Self {
        Self {
            phase: StartupPhase::Starting,
            generation: 1,
            splash,
            requested_after: None,
            committed: false,
        }
    }

    pub(super) const fn phase(&self) -> StartupPhase {
        self.phase
    }

    pub(super) const fn ticket(&self) -> Option<StartupTicket> {
        match self.phase {
            StartupPhase::UiReady | StartupPhase::TakeoverRequested => Some(StartupTicket {
                generation: self.generation,
            }),
            StartupPhase::Starting | StartupPhase::HandedOff => None,
        }
    }

    /// The program exists. `flush` is the primary window's flush sequence
    /// now. Returns whether this also requested the takeover.
    pub(super) fn ui_ready(&mut self, policy: StartupTakeover, flush: u64) -> bool {
        if self.phase != StartupPhase::Starting {
            return false;
        }
        self.phase = StartupPhase::UiReady;
        policy == StartupTakeover::Immediate && self.take_over(flush)
    }

    /// Returns whether this was a new request. A repeated one keeps the first
    /// target: it named content that is already in every later frame.
    pub(super) fn request(
        &mut self,
        ticket: StartupTicket,
        flush: u64,
    ) -> Result<bool, StartupError> {
        self.check(ticket)?;
        Ok(self.take_over(flush))
    }

    fn take_over(&mut self, flush: u64) -> bool {
        if self.phase != StartupPhase::UiReady {
            return false;
        }
        self.phase = StartupPhase::TakeoverRequested;
        self.requested_after = Some(flush);
        true
    }

    pub(super) fn cancel(&mut self, ticket: StartupTicket) -> Result<(), StartupError> {
        self.check(ticket)?;
        self.generation += 1;
        self.phase = StartupPhase::UiReady;
        self.requested_after = None;
        Ok(())
    }

    fn check(&self, ticket: StartupTicket) -> Result<(), StartupError> {
        match self.phase {
            StartupPhase::HandedOff => Err(StartupError::AlreadyHandedOff),
            _ if self.committed => Err(StartupError::AlreadyHandedOff),
            StartupPhase::Starting => Err(StartupError::StaleTicket),
            _ if ticket.generation != self.generation => Err(StartupError::StaleTicket),
            _ => Ok(()),
        }
    }

    /// Whether `id` must not present yet. Only the primary window under a
    /// splash waits, and only until something asked to take over: until then
    /// nothing it drew could be seen, and the window is already on screen, so
    /// holding it back cannot keep it from ever presenting.
    pub(super) fn holds_presents(&self, id: WindowId) -> bool {
        self.splash
            && id == WindowId::PRIMARY
            && matches!(self.phase, StartupPhase::Starting | StartupPhase::UiReady)
    }

    /// Whether a presented frame of `id`, flushed at `flush`, ends the
    /// startup. Only a requested takeover ends it, splash or not, so a
    /// program that defers behaves the same on a platform without a splash:
    /// its ticket stays valid until it asks.
    pub(super) fn completes_with(&self, id: WindowId, flush: u64) -> bool {
        id == WindowId::PRIMARY
            && !self.committed
            && self.phase == StartupPhase::TakeoverRequested
            && self.requested_after.is_some_and(|after| flush > after)
    }

    /// The takeover frame was presented; the handoff finishes once the
    /// compositor has it. A cancel arriving meanwhile is refused.
    pub(super) fn frame_committed(&mut self) {
        self.committed = true;
    }

    /// The committed frame will never be confirmed (its device was
    /// replaced); the next presented frame decides instead.
    pub(super) fn reopen(&mut self) {
        self.committed = false;
    }
}

/// What `run_runtime_scene` hands the event loop before it has a window.
pub(super) struct LoadingStartup<Message> {
    pub(super) proxy: EventLoopProxy,
    pub(super) message_tx: Sender<Message>,
    pub(super) message_rx: Receiver<Message>,
    pub(super) settings: WindowDescriptor,
    pub(super) startup_failure: Arc<Mutex<Option<String>>>,
    pub(super) options: StartupOptions,
    pub(super) entry: Instant,
}

/// What the startup thread sends back: a device for the surface, and the
/// scene painter built on it, so the pipelines are not compiled on the event
/// thread either.
pub(super) struct DeviceStart {
    device: Result<AcquiredDevice, String>,
    painter: Option<SceneWgpuPainter>,
}

/// One presentation target being tried. Fields drop in declaration order:
/// the splash comes off before the window it covers goes.
struct Attempt {
    splash: Option<NativeSplash>,
    /// The primary is shown but held invisible behind the splash.
    held: bool,
    #[cfg(not(target_os = "android"))]
    accessibility: Option<HostedAccessibility>,
    surface: PendingPrimarySurface,
    provisional: PendingNativeWindow,
    window: Arc<dyn winit::window::Window>,
    requested_material: crate::MaterialEffect,
    applied_material: MaterialOutcome,
    device: Receiver<DeviceStart>,
}

/// The primary window exists — on screen with its splash, when there is one —
/// and its device is being requested off the event thread. The event loop
/// keeps turning meanwhile: closing the window cancels the startup.
pub(super) struct PendingStartup<Message> {
    channels: StartupChannels<Message>,
    settings: WindowDescriptor,
    store: SharedStore,
    options: StartupOptions,
    policy: crate::GpuBackendPolicy,
    startup_material_mode: crate::MaterialEffect,
    target: ResolvedSurfaceTarget,
    /// Why the composed target was given up, for a plain attempt that fails too.
    composed_error: Option<String>,
    handle: StartupHandle,
    longest_block: Duration,
    icons: Option<Receiver<SceneIcons>>,
    attempt: Option<Attempt>,
}

pub(super) enum StartupStep<Program: RuntimeProgram> {
    Ready(Box<WindowManager<Program>>),
    /// The composed target failed; the plain one is being tried.
    Retry(Box<PendingStartup<Program::Message>>),
}

impl<Message: Send + 'static> PendingStartup<Message> {
    pub(super) fn begin<Program: RuntimeProgram<Message = Message>>(
        event_loop: &dyn ActiveEventLoop,
        loading: LoadingStartup<Message>,
    ) -> Result<Self, String> {
        let LoadingStartup {
            proxy,
            message_tx,
            message_rx,
            mut settings,
            startup_failure,
            options,
            entry,
        } = loading;
        let store = prepare_primary_descriptor(&mut settings)?;
        let policy = Program::gpu_backend_policy();
        let mut bootstrap = gpu_bootstrap(policy, None);
        let startup_material_mode = Program::startup_window_material_mode();
        let target = primary_surface_target(&settings, policy, &bootstrap, startup_material_mode)?;
        let icons = spawn_icon_render(&settings, &proxy);
        let mut pending = Self {
            channels: StartupChannels {
                proxy,
                message_tx,
                message_rx,
                startup_failure,
            },
            settings,
            store,
            options,
            policy,
            startup_material_mode,
            target,
            composed_error: None,
            handle: StartupHandle::new(entry, SplashOutcome::Skipped(SplashSkip::NotConfigured)),
            longest_block: Duration::ZERO,
            icons,
            attempt: None,
        };
        // The probe's instance is narrowed to the composition backend, and
        // that narrowing is the process-wide policy taking effect. It is
        // dropped only once composition has been given up on.
        let instance = target
            .fallback
            .is_none()
            .then(|| bootstrap.take_instance())
            .flatten();
        pending.start(event_loop, instance)?;
        Ok(pending)
    }

    pub(super) fn startup_failure(&self) -> &Arc<Mutex<Option<String>>> {
        &self.channels.startup_failure
    }

    pub(super) fn owns(&self, id: winit::window::WindowId) -> bool {
        self.attempt
            .as_ref()
            .is_some_and(|attempt| attempt.window.id() == id)
    }

    pub(super) fn note_block(&mut self, elapsed: Duration) {
        self.longest_block = self.longest_block.max(elapsed);
    }

    /// The window's backing scale changed while the device is requested.
    pub(super) fn rescale_splash(&mut self, id: winit::window::WindowId, scale: f64) {
        if let Some(attempt) = self.attempt.as_mut()
            && attempt.window.id() == id
            && let Some(splash) = attempt.splash.as_mut()
        {
            splash.set_scale_factor(scale);
        }
    }

    /// The startup was cancelled. The device thread may still hold the
    /// window through its surface, so it is hidden here rather than left for
    /// the last reference to take down; the splash comes off with the drop.
    pub(super) fn cancel(self) {
        if let Some(attempt) = self.attempt.as_ref() {
            attempt.window.set_visible(false);
        }
    }

    /// The startup thread's result, once it has sent one.
    pub(super) fn take_device(&mut self) -> Option<DeviceStart> {
        match self.attempt.as_ref()?.device.try_recv() {
            Ok(start) => Some(start),
            Err(mpsc::TryRecvError::Empty) => None,
            // The thread ended without sending: it panicked.
            Err(mpsc::TryRecvError::Disconnected) => Some(DeviceStart {
                device: Err("the GPU startup thread stopped without a device".into()),
                painter: None,
            }),
        }
    }

    /// Starts the current target, falling back to the plain one while there
    /// is one to fall back to.
    fn start(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        mut instance: Option<wgpu::Instance>,
    ) -> Result<(), String> {
        loop {
            match self.start_attempt(event_loop, instance.take()) {
                Ok(()) => return Ok(()),
                Err(error) => self.fall_back(error)?,
            }
        }
    }

    /// Drops the current attempt — splash, window and all — and moves to the
    /// next target, or gives up with the whole story.
    fn fall_back(&mut self, error: String) -> Result<(), String> {
        self.attempt = None;
        self.target = next_primary_target(self.target, error, &mut self.composed_error)?;
        Ok(())
    }

    fn start_attempt(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        instance: Option<wgpu::Instance>,
    ) -> Result<(), String> {
        let target = self.target.resolved;
        let (window, provisional, requested_material, applied_material) = create_primary_window(
            event_loop,
            &self.settings,
            target,
            crate::ThemeMode::default(),
            self.startup_material_mode,
        )?;
        // The device request goes first: the splash then overlaps it instead
        // of delaying it. The splash sits above the surface whichever order
        // the two reach the compositor in.
        let (surface, request) = PendingPrimarySurface::begin(
            Arc::clone(&window),
            wgpu::Features::empty(),
            requested_material.wants_transparent_surface(),
            surface_mode_for(target),
            instance,
        )
        .map_err(|error| error.to_string())?;
        let device = spawn_device_request(request, self.channels.proxy.clone())?;
        // The accessibility adapter belongs to the window before it is first
        // shown, which with a splash is now.
        #[cfg(not(target_os = "android"))]
        let accessibility = Some(HostedAccessibility::new(
            Arc::clone(&window),
            true,
            window.scale_factor() as f32,
        ));
        let (splash, outcome) = self.show_splash(window.as_ref());
        let committed = splash.is_some().then(|| self.handle.elapsed());
        // The takeover frame has to be presented before the splash comes off,
        // and a hidden window never presents. So the primary is shown now but
        // held invisible to the user until the handoff releases it; where the
        // platform cannot hold it, it is simply shown behind the splash.
        let held = splash.is_some() && nana_window::set_presentation_hold(window.as_ref(), true);
        if splash.is_some() {
            let focus = !held && self.settings.focus_on_show;
            windows::set_native_visible(window.as_ref(), true, focus);
        }
        nana_diagnostics::event!(host::SPLASH_OUTCOME, outcome = outcome.code());
        if let Some(at) = committed {
            startup_event(StartupMark::SplashCommitted, at);
        }
        let work = splash.as_ref().map(NativeSplash::work).unwrap_or_default();
        self.handle.update(|status| {
            status.splash = outcome;
            status.timeline.splash_committed = committed;
            status.work.splash = work;
            status.work.devices_requested += 1;
        });
        self.attempt = Some(Attempt {
            splash,
            held,
            #[cfg(not(target_os = "android"))]
            accessibility,
            surface,
            provisional,
            window,
            requested_material,
            applied_material,
            device,
        });
        Ok(())
    }

    fn show_splash(
        &self,
        window: &dyn winit::window::Window,
    ) -> (Option<NativeSplash>, SplashOutcome) {
        let Some(spec) = self.options.splash else {
            return (None, SplashOutcome::Skipped(SplashSkip::NotConfigured));
        };
        if !self.settings.visible {
            return (None, SplashOutcome::Skipped(SplashSkip::HiddenStart));
        }
        // Before the logo is resolved: a platform without a splash reads
        // nothing for one.
        if !NativeSplash::platform_supported() {
            return (
                None,
                SplashOutcome::Skipped(SplashSkip::PlatformUnsupported),
            );
        }
        let png = match self.read_splash_logo(spec.logo) {
            Ok(png) => png,
            Err(failure) => return (None, SplashOutcome::Failed(failure)),
        };
        let theme = match window.theme() {
            Some(WinitTheme::Light) => crate::ThemeMode::Light,
            Some(WinitTheme::Dark) => crate::ThemeMode::Dark,
            None => crate::ThemeMode::default(),
        };
        let (red, green, blue, alpha) = theme.palette().background.to_u8_rgba();
        NativeSplash::show(
            window,
            &spec,
            &png,
            FallbackColor::rgba(red, green, blue, alpha),
            nana_window::system_reduced_motion().unwrap_or(false),
        )
    }

    /// The logo's PNG. A packaged one is one read of the package's
    /// `early-splash` pack, timed into the startup record; a supported native
    /// target shows a splash, so a startup reads it at most once.
    fn read_splash_logo(
        &self,
        logo: crate::SplashLogo,
    ) -> Result<std::borrow::Cow<'static, [u8]>, crate::SplashFailure> {
        let url = match logo.source() {
            crate::SplashLogoSource::Embedded(png) => return Ok(png.into()),
            crate::SplashLogoSource::Packaged(url) => url,
        };
        let started = Instant::now();
        #[cfg(feature = "packaged-resources")]
        let result = crate::packaged_resources::read_splash_logo(url).map(Into::into);
        #[cfg(not(feature = "packaged-resources"))]
        let result = Err(crate::SplashFailure::Package(
            crate::SplashPackageError::Unsupported,
        ));
        let elapsed = started.elapsed();
        nana_diagnostics::metric!(host::STARTUP_SPLASH_LOGO_READ_NS, elapsed);
        self.handle
            .update(|status| status.work.splash_logo_read = Some(elapsed));
        if let Err(crate::SplashFailure::Package(error)) = &result {
            nana_diagnostics::fault!(
                host::SPLASH_LOGO_FAILED,
                code = error.code();
                "Early Splash logo {url}: {error}"
            );
        }
        result
    }

    /// Binds the surface to the device the startup thread produced and runs
    /// the rest of the startup on this thread, or moves to the next target.
    pub(super) fn finish<Program: RuntimeProgram<Message = Message>>(
        mut self,
        event_loop: &dyn ActiveEventLoop,
        start: DeviceStart,
    ) -> Result<StartupStep<Program>, String> {
        let Attempt {
            splash,
            held,
            #[cfg(not(target_os = "android"))]
            accessibility,
            surface,
            provisional,
            window,
            requested_material,
            applied_material,
            device: _,
        } = self
            .attempt
            .take()
            .expect("a device result belongs to an attempt");
        let target = self.target;
        let bound = start
            .device
            .and_then(|device| surface.finish(device).map_err(|error| error.to_string()))
            .and_then(|context| composed_fault(target).map_or(Ok(context), Err));
        let context = match bound {
            Ok(context) => context,
            Err(error) => {
                drop(splash);
                drop(provisional);
                self.fall_back(error)?;
                self.start(event_loop, None)?;
                return Ok(StartupStep::Retry(Box::new(self)));
            }
        };
        provisional.keep();
        let (graphics, surface) = context.into_parts();
        let composition =
            settle_primary_target(target, self.policy, graphics.adapter_info().backend);
        let startup = HostStartup::new(self.handle, splash, held, self.longest_block);
        complete_startup::<Program>(
            event_loop,
            self.channels,
            self.settings,
            self.store,
            false,
            PrimaryStart {
                bootstrap: PrimaryBootstrap {
                    window,
                    graphics,
                    surface,
                    target,
                    composition,
                    requested_material,
                    applied_material,
                },
                #[cfg(not(target_os = "android"))]
                accessibility,
                painter: start.painter,
                icons: self.icons,
                startup,
            },
        )
        .map(|ready| StartupStep::Ready(Box::new(ready)))
    }
}

/// Renders the macOS Dock icon — a padded, re-encoded PNG, the one icon that
/// is expensive — while the device is requested. Elsewhere the window
/// attributes already carry the (cached) icon and nothing is spawned.
fn spawn_icon_render(
    settings: &WindowDescriptor,
    proxy: &EventLoopProxy,
) -> Option<Receiver<SceneIcons>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let per_window = settings.icon.clone();
    let proxy = proxy.clone();
    std::thread::Builder::new()
        .name("nana-startup-icons".into())
        .spawn(move || {
            if sender
                .send(SceneIcons::render(per_window.as_ref(), true))
                .is_ok()
            {
                proxy.wake_up();
            }
        })
        .ok()
        .map(|_| receiver)
}

/// Requests the device on a thread of its own and wakes the event loop when
/// it has one. A platform device request cannot be cancelled once it has
/// started; a startup cancelled meanwhile simply never takes the result.
fn spawn_device_request(
    request: DeviceRequest,
    proxy: EventLoopProxy,
) -> Result<Receiver<DeviceStart>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("nana-startup-gpu".into())
        .spawn(move || {
            if let Some(delay) = std::env::var("NANA_STARTUP_GPU_DELAY_MS")
                .ok()
                .and_then(|value| value.parse().ok())
            {
                std::thread::sleep(Duration::from_millis(delay));
            }
            let device = if fault_flag("NANA_STARTUP_GPU_FAIL") {
                drop(request);
                Err("GPU initialization failure requested by NANA_STARTUP_GPU_FAIL".into())
            } else {
                pollster::block_on(request.acquire()).map_err(|error| error.to_string())
            };
            let painter = device.as_ref().ok().map(|device| {
                SceneWgpuPainter::new(
                    device.gpu(),
                    nana_gpu::__framework::format_from_wgpu(device.format()),
                )
            });
            if let Err(unsent) = sender.send(DeviceStart { device, painter }) {
                // The host went away. The surface holds a reference to its
                // window, which must not be released off the window's thread:
                // keep it, once, for the rest of the process.
                std::mem::forget(unsent);
                return;
            }
            proxy.wake_up();
        })
        .map_err(|error| format!("failed to start the GPU startup thread: {error}"))?;
    Ok(receiver)
}

/// The host's side of a startup once the program exists.
pub(super) struct HostStartup {
    /// The record programs read. Outlives the startup.
    pub(super) handle: StartupHandle,
    /// Everything that matters only until the handoff; `None` from then on.
    active: Option<Box<ActiveStartup>>,
    /// Longest event-loop callback so far, while callbacks are measured. The
    /// callback that performs the handoff — often the longest, with the first
    /// frame in it — still counts; measuring stops after it.
    longest_block: Option<Duration>,
}

struct ActiveStartup {
    coordinator: StartupCoordinator,
    splash: Option<NativeSplash>,
    /// Windows: set by the queue once the takeover frame's GPU work has
    /// completed; the splash comes off after the next compositor pass.
    latch: Option<Arc<AtomicBool>>,
    /// Flushes of the primary document so far.
    primary_flushes: u64,
    /// An `Immediate` takeover waiting for the startup messages: the frame
    /// that removes the splash has to show their effects too.
    auto_takeover: bool,
    /// The primary is held invisible (cloaked / zero alpha) until handoff.
    /// Occlusion reports for it mean nothing until then.
    held: bool,
}

impl HostStartup {
    fn new(
        handle: StartupHandle,
        splash: Option<NativeSplash>,
        held: bool,
        longest_block: Duration,
    ) -> Self {
        Self {
            handle,
            active: Some(Box::new(ActiveStartup {
                coordinator: StartupCoordinator::new(splash.is_some()),
                held,
                splash,
                latch: None,
                primary_flushes: 0,
                auto_takeover: false,
            })),
            longest_block: Some(longest_block),
        }
    }

    /// An embedded host's record: nothing to hand off, nothing to measure.
    pub(super) fn settled(outcome: SplashOutcome) -> Self {
        Self {
            handle: StartupHandle::settled(outcome),
            active: None,
            longest_block: None,
        }
    }

    /// A native splash was committed before the primary window was shown.
    pub(super) fn shown_early(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.coordinator.splash)
    }

    /// Whether event-loop callbacks are still timed.
    pub(super) const fn measures_blocks(&self) -> bool {
        self.longest_block.is_some()
    }

    /// `RuntimeProgram::initialize` is about to be called.
    pub(super) fn ui_ready_begins(&mut self) {
        if self.active.is_none() {
            return;
        }
        let at = self.handle.elapsed();
        startup_event(StartupMark::UiReady, at);
        self.handle
            .update(|status| status.timeline.ui_ready = Some(at));
    }

    fn publish(&self, change: impl FnOnce(&mut StartupStatus)) {
        let (phase, ticket) = self
            .active
            .as_ref()
            .map_or((StartupPhase::HandedOff, None), |active| {
                (active.coordinator.phase(), active.coordinator.ticket())
            });
        self.handle.update(|status| {
            status.phase = phase;
            status.ticket = ticket;
            change(status);
        });
    }

    /// Takes the splash off — as the handoff, or because the window is
    /// closing — and records its release.
    fn release_splash(&mut self, handoff: bool) {
        let Some(splash) = self.active.as_mut().and_then(|active| active.splash.take()) else {
            return;
        };
        let work = if handoff {
            splash.remove()
        } else {
            splash.discard()
        };
        let at = self.handle.elapsed();
        startup_event(StartupMark::SplashReleased, at);
        self.handle.update(|status| {
            status.timeline.splash_released = Some(at);
            status.work.splash = work;
        });
    }
}

impl<Program: RuntimeProgram> WindowManager<Program> {
    /// `initialize` returned; the program's takeover policy is known.
    ///
    /// With startup messages still queued, an `Immediate` takeover is asked
    /// for once they have been applied ([`Self::drain_host_messages`]).
    pub(super) fn startup_ui_ready(&mut self, policy: StartupTakeover) {
        let waits = !self.startup_messages.is_empty();
        let Some(active) = self.startup.active.as_mut() else {
            return;
        };
        let policy = if policy == StartupTakeover::Immediate && waits {
            active.auto_takeover = true;
            StartupTakeover::Deferred
        } else {
            policy
        };
        if active.coordinator.ui_ready(policy, active.primary_flushes) {
            self.takeover_requested();
        } else {
            self.startup.publish(|_| {});
        }
    }

    /// Whether `id` must not present yet; see [`StartupCoordinator::holds_presents`].
    pub(super) fn startup_holds(&self, id: WindowId) -> bool {
        self.startup
            .active
            .as_ref()
            .is_some_and(|active| active.coordinator.holds_presents(id))
    }

    /// A settled flush of `id` is about to be presented. Returns whether this
    /// frame ends the startup, and if so readies the window's presentation for
    /// the splash's handoff.
    pub(super) fn prepare_startup_frame(&mut self, id: WindowId) -> bool {
        let Some(active) = self.startup.active.as_mut() else {
            return false;
        };
        if id == WindowId::PRIMARY {
            active.primary_flushes += 1;
        }
        if !active
            .coordinator
            .completes_with(id, active.primary_flushes)
        {
            return false;
        }
        // The drawable and the splash's removal have to land in one Core
        // Animation commit, which only a transaction-mode present gives. It
        // has to be set before the drawable is acquired; the pin is released
        // by the ordinary idle unpin once the turn is over.
        #[cfg(target_os = "macos")]
        if active
            .splash
            .as_ref()
            .is_some_and(|splash| splash.handoff() == SplashHandoff::SameTransaction)
        {
            self.pin_present_transaction(id);
        }
        true
    }

    /// The frame [`Self::prepare_startup_frame`] picked was presented.
    pub(super) fn startup_frame_presented(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(active) = self.startup.active.as_mut() else {
            return;
        };
        active.coordinator.frame_committed();
        let flush_after_gpu = active
            .splash
            .as_ref()
            .is_some_and(|splash| splash.handoff() == SplashHandoff::AfterCompositorFlush);
        let at = self.startup.handle.elapsed();
        startup_event(StartupMark::TakeoverFrame, at);
        self.startup
            .handle
            .update(|status| status.timeline.first_frame_submitted = Some(at));
        if !flush_after_gpu {
            self.startup_handed_off(event_loop);
            return;
        }
        let latch = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&latch);
        let proxy = self.proxy.clone();
        self.graphics
            .gpu()
            .raw_queue()
            .on_submitted_work_done(move || {
                done.store(true, Ordering::Release);
                proxy.wake_up();
            });
        if let Some(active) = self.startup.active.as_mut() {
            active.latch = Some(latch);
        }
    }

    /// Windows: finishes the handoff once the takeover frame's GPU work is
    /// done. Returns when to look again while it is not.
    pub(super) fn poll_startup_latch(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
    ) -> Option<Instant> {
        let latch = self.startup.active.as_ref()?.latch.as_ref()?;
        if !latch.load(Ordering::Acquire) {
            let _ = self.graphics.gpu().raw_device().poll(wgpu::PollType::Poll);
        }
        if latch.load(Ordering::Acquire) {
            self.startup_handed_off(event_loop);
            None
        } else {
            Some(Instant::now() + LATCH_POLL)
        }
    }

    fn startup_handed_off(&mut self, event_loop: &dyn ActiveEventLoop) {
        let at = self.startup.handle.elapsed();
        self.release_primary_hold();
        self.startup.release_splash(true);
        self.startup.active = None;
        // The primary is visible now: its shadow companion can show.
        self.apply_window_shadow(WindowId::PRIMARY);
        startup_event(StartupMark::HandedOff, at);
        self.startup
            .publish(|status| status.timeline.handoff_completed = Some(at));
        self.notify_startup_changed(event_loop);
    }

    /// Puts a primary held behind the splash on screen and gives it the focus
    /// its descriptor asks for. Idempotent.
    fn release_primary_hold(&mut self) {
        let held = self
            .startup
            .active
            .as_mut()
            .is_some_and(|active| std::mem::take(&mut active.held));
        if held && let Some(window) = self.window(WindowId::PRIMARY) {
            nana_window::set_presentation_hold(window.as_ref(), false);
            windows::set_native_visible(
                window.as_ref(),
                self.settings.visible,
                self.settings.focus_on_show,
            );
        }
    }

    /// Whether the primary is presenting behind the splash, invisible to the
    /// user. Its occlusion state is not the user's view until the handoff.
    pub(super) fn startup_presentation_held(&self, id: WindowId) -> bool {
        id == WindowId::PRIMARY
            && self
                .startup
                .active
                .as_ref()
                .is_some_and(|active| active.held)
    }

    /// With the startup messages applied, asks for the `Immediate` takeover
    /// they held back.
    pub(super) fn startup_messages_applied(&mut self) {
        let Some(active) = self.startup.active.as_mut() else {
            return;
        };
        if std::mem::take(&mut active.auto_takeover)
            && let Some(ticket) = active.coordinator.ticket()
            && active
                .coordinator
                .request(ticket, active.primary_flushes)
                .unwrap_or(false)
        {
            self.takeover_requested();
        }
    }

    /// The coordinator accepted a takeover: record it and draw the frame.
    fn takeover_requested(&mut self) {
        let at = self.startup.handle.elapsed();
        startup_event(StartupMark::TakeoverRequested, at);
        self.startup
            .publish(|status| status.timeline.takeover_requested = Some(at));
        self.request_redraw(WindowId::PRIMARY);
    }

    /// The device was replaced. A Windows handoff waiting for the old queue
    /// to confirm the takeover frame would wait forever: the next frame, on
    /// the new device, takes over instead.
    pub(super) fn reset_startup_latch(&mut self) {
        if let Some(active) = self.startup.active.as_mut()
            && active.latch.take().is_some()
        {
            active.coordinator.reopen();
        }
    }

    /// Applies takeover requests made through [`crate::StartupHandle`].
    pub(super) fn process_startup_requests(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.startup.active.is_none() {
            return;
        }
        for request in self.startup.handle.take_requests() {
            let Some(active) = self.startup.active.as_mut() else {
                return;
            };
            let applied = match request {
                StartupRequest::TakeOver(ticket) => {
                    active.coordinator.request(ticket, active.primary_flushes)
                }
                StartupRequest::Cancel(ticket) => active.coordinator.cancel(ticket).map(|()| true),
            };
            if applied != Ok(true) {
                continue;
            }
            // The program asked or withdrew itself; nothing is asked on its
            // behalf any more.
            active.auto_takeover = false;
            match request {
                StartupRequest::TakeOver(_) => self.takeover_requested(),
                StartupRequest::Cancel(_) => self
                    .startup
                    .publish(|status| status.timeline.takeover_requested = None),
            }
            self.notify_startup_changed(event_loop);
        }
    }

    fn notify_startup_changed(&mut self, event_loop: &dyn ActiveEventLoop) {
        let status = self.startup.handle.status();
        let update = self.program.startup_changed(&status, &self.context());
        self.apply_update(event_loop, update, None);
    }

    /// The primary window is closing: nothing is left for the splash to
    /// cover, and it has to come off before the window goes.
    pub(super) fn release_startup_splash(&mut self) {
        self.release_primary_hold();
        if let Some(active) = self.startup.active.as_mut() {
            active.latch = None;
        }
        self.startup.release_splash(false);
    }

    /// The primary window's backing scale changed while its splash is up.
    pub(super) fn rescale_startup_splash(&mut self, id: WindowId, scale: f64) {
        if id == WindowId::PRIMARY
            && let Some(splash) = self
                .startup
                .active
                .as_mut()
                .and_then(|active| active.splash.as_mut())
        {
            splash.set_scale_factor(scale);
        }
    }

    /// Records one timed event-loop callback. The first one after the handoff
    /// finalizes the measurement.
    pub(super) fn note_startup_block(&mut self, elapsed: Duration) {
        let Some(longest) = self.startup.longest_block.as_mut() else {
            return;
        };
        if elapsed > *longest {
            *longest = elapsed;
        }
        let longest = *longest;
        if self.startup.active.is_none() {
            self.startup.longest_block = None;
            nana_diagnostics::metric!(host::STARTUP_LONGEST_BLOCK_NS, longest);
        }
        self.startup
            .handle
            .update(|status| status.work.longest_event_thread_block = longest);
    }

    /// Applies the primary window's icons once the startup thread has
    /// rendered them.
    pub(super) fn apply_pending_icons(&mut self) {
        let Some(icons) = self.pending_icons.as_ref() else {
            return;
        };
        let rendered = match icons.try_recv() {
            Ok(rendered) => Some(rendered),
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        self.pending_icons = None;
        let Some(window) = self.window(WindowId::PRIMARY) else {
            return;
        };
        match rendered {
            Some(rendered) => rendered.apply(window.as_ref()),
            None => apply_scene_window_icon(window.as_ref(), self.settings.icon.as_ref(), true),
        }
    }

    /// Counts a painter created for the startup (the startup thread's, or one
    /// built on the event thread before the handoff).
    pub(super) fn note_startup_painter(&self) {
        if self.startup.active.is_some() {
            self.startup
                .handle
                .update(|status| status.work.painters_created += 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECONDARY: WindowId = WindowId(7);

    fn ready(policy: StartupTakeover) -> StartupCoordinator {
        let mut startup = StartupCoordinator::new(true);
        startup.ui_ready(policy, 0);
        startup
    }

    #[test]
    fn nothing_can_be_requested_before_the_program_exists() {
        let mut startup = StartupCoordinator::new(true);
        assert_eq!(startup.ticket(), None);
        assert_eq!(
            startup.request(StartupTicket { generation: 1 }, 0),
            Err(StartupError::StaleTicket)
        );
        assert!(startup.holds_presents(WindowId::PRIMARY));
        assert!(!startup.completes_with(WindowId::PRIMARY, 9));
    }

    #[test]
    fn immediate_takeover_waits_for_a_frame_flushed_after_ready() {
        let mut startup = StartupCoordinator::new(true);
        assert!(startup.ui_ready(StartupTakeover::Immediate, 3));
        assert_eq!(startup.phase(), StartupPhase::TakeoverRequested);
        assert!(!startup.holds_presents(WindowId::PRIMARY));
        // A frame flushed before the request shows older content.
        assert!(!startup.completes_with(WindowId::PRIMARY, 3));
        assert!(startup.completes_with(WindowId::PRIMARY, 4));
    }

    #[test]
    fn deferred_takeover_keeps_the_primary_window_behind_the_splash() {
        let mut startup = ready(StartupTakeover::Deferred);
        assert_eq!(startup.phase(), StartupPhase::UiReady);
        assert!(startup.holds_presents(WindowId::PRIMARY));
        assert!(!startup.holds_presents(SECONDARY));
        assert!(!startup.completes_with(WindowId::PRIMARY, 50));
        let ticket = startup.ticket().unwrap();
        startup.request(ticket, 50).unwrap();
        assert!(!startup.holds_presents(WindowId::PRIMARY));
        assert!(!startup.completes_with(WindowId::PRIMARY, 50));
        assert!(startup.completes_with(WindowId::PRIMARY, 51));
    }

    #[test]
    fn other_windows_never_complete_the_takeover() {
        let startup = ready(StartupTakeover::Immediate);
        assert!(!startup.completes_with(SECONDARY, 10));
    }

    #[test]
    fn once_the_takeover_frame_is_committed_nothing_can_be_withdrawn() {
        let mut startup = ready(StartupTakeover::Immediate);
        let ticket = startup.ticket().unwrap();
        startup.frame_committed();
        assert_eq!(startup.cancel(ticket), Err(StartupError::AlreadyHandedOff));
        assert!(!startup.completes_with(WindowId::PRIMARY, 5));
        assert_eq!(startup.phase(), StartupPhase::TakeoverRequested);
    }

    #[test]
    fn a_cancel_retires_the_ticket_and_late_requests_are_refused() {
        let mut startup = ready(StartupTakeover::Deferred);
        let old = startup.ticket().unwrap();
        startup.request(old, 5).unwrap();
        startup.cancel(old).unwrap();
        assert_eq!(startup.phase(), StartupPhase::UiReady);
        assert!(startup.holds_presents(WindowId::PRIMARY));
        // A completion that was already in flight when the cancel happened.
        assert_eq!(startup.request(old, 9), Err(StartupError::StaleTicket));
        assert_eq!(startup.cancel(old), Err(StartupError::StaleTicket));
        assert!(!startup.completes_with(WindowId::PRIMARY, 9));
        let fresh = startup.ticket().unwrap();
        assert_ne!(fresh, old);
        startup.request(fresh, 9).unwrap();
        assert!(startup.completes_with(WindowId::PRIMARY, 10));
    }

    #[test]
    fn a_committed_frame_whose_device_went_away_reopens_the_takeover() {
        let mut startup = ready(StartupTakeover::Immediate);
        startup.frame_committed();
        assert!(!startup.completes_with(WindowId::PRIMARY, 5));
        startup.reopen();
        assert!(startup.completes_with(WindowId::PRIMARY, 5));
    }

    #[test]
    fn a_repeated_request_keeps_the_first_target() {
        let mut startup = ready(StartupTakeover::Deferred);
        let ticket = startup.ticket().unwrap();
        startup.request(ticket, 5).unwrap();
        startup.request(ticket, 8).unwrap();
        assert!(startup.completes_with(WindowId::PRIMARY, 6));
    }

    #[test]
    fn without_a_splash_nothing_is_held_but_a_deferral_still_waits_for_its_request() {
        let mut startup = StartupCoordinator::new(false);
        assert!(!startup.holds_presents(WindowId::PRIMARY));
        startup.ui_ready(StartupTakeover::Deferred, 0);
        assert!(!startup.holds_presents(WindowId::PRIMARY));
        // Frames present, but the program has not asked: its ticket stays
        // valid, exactly as on a platform with a splash.
        assert!(!startup.completes_with(WindowId::PRIMARY, 1));
        let ticket = startup.ticket().expect("deferred program keeps its ticket");
        startup.request(ticket, 3).unwrap();
        assert!(startup.completes_with(WindowId::PRIMARY, 4));
    }

    #[test]
    fn ready_is_delivered_once() {
        let mut startup = StartupCoordinator::new(true);
        assert!(startup.ui_ready(StartupTakeover::Immediate, 0));
        assert!(!startup.ui_ready(StartupTakeover::Immediate, 4));
        assert!(startup.completes_with(WindowId::PRIMARY, 1));
    }
}
