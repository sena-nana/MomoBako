//! Two-phase startup (Issue #225).
//!
//! 1. **Early Splash.** Before the GPU device or the program exists, the host
//!    can put an embedded logo on the primary window and show it, with at most
//!    one animation preset the platform compositor runs by itself.
//! 2. **UiReady.** Once the window, surface, device, painter and text system
//!    can mount, lay out, dispatch and draw an ordinary document, the host
//!    calls [`RuntimeProgram::initialize`](crate::RuntimeProgram::initialize).
//!    That call *is* the signal: business state does not have to exist before
//!    it, and nothing heavy should happen inside it.
//!
//! The program then says which content may replace the logo — immediately, or
//! later through [`StartupHandle::take_over`] — and the host removes the splash
//! only once that content's first frame is on the compositor. Readiness and
//! handoff are separate on purpose: a program can mount a loading page, build
//! its main interface directly, or keep the logo up while it decides.
//!
//! Everything here is optional. Without [`StartupOptions::splash`] no native
//! layer, window, device or timer is created for it, and the window is shown
//! after `initialize` exactly as before.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use nana_window::{
    LogoInfo, MAX_LOGO_DECODED_BYTES, MAX_LOGO_EDGE, MAX_LOGO_ENCODED_BYTES, SplashAnimation,
    SplashAnimationOutcome, SplashBackground, SplashFailure, SplashLogo, SplashLogoError,
    SplashLogoSource, SplashOutcome, SplashPackageError, SplashSkip, SplashSpec,
    SplashStaticReason, SplashWork, validate_logo,
};

/// What an application asks of its startup, before anything else of it runs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StartupOptions {
    pub splash: Option<SplashSpec>,
}

impl StartupOptions {
    pub const fn with_splash(mut self, splash: SplashSpec) -> Self {
        self.splash = Some(splash);
        self
    }
}

/// Where one startup is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupPhase {
    /// The window and device are being created; the program does not exist.
    Starting,
    /// The program has been initialized and can build ordinary UI. The splash,
    /// if any, is still up.
    UiReady,
    /// Content has been named for takeover; the host is waiting for its frame.
    TakeoverRequested,
    /// The splash is gone (or there never was one) and the primary window has
    /// presented a frame of the program's own.
    HandedOff,
}

impl StartupPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::UiReady => "ui-ready",
            Self::TakeoverRequested => "takeover-requested",
            Self::HandedOff => "handed-off",
        }
    }
}

/// Names one takeover request. Cancelling a request retires its ticket, so a
/// completion that arrives afterwards — from a task started before the cancel,
/// say — is refused instead of taking over with content nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StartupTicket {
    pub(crate) generation: u64,
}

impl StartupTicket {
    /// The ticket's number, for front ends that pass it through a boundary
    /// (the Vue host hands it to JavaScript). Tickets compare by it.
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// Whether the program's first document may replace the splash as soon as it
/// has a frame. Read once, right after `initialize` returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StartupTakeover {
    #[default]
    Immediate,
    /// Keep the splash until [`StartupHandle::take_over`].
    Deferred,
}

/// When each startup milestone happened, measured from the host's entry
/// (`run_runtime`), not from process creation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartupTimeline {
    /// The splash was committed to the platform compositor and the window
    /// asked to show. This is the CPU side of the request: no platform used
    /// here reports when a layer is actually on screen.
    pub splash_committed: Option<Duration>,
    /// `RuntimeProgram::initialize` was called.
    pub ui_ready: Option<Duration>,
    pub takeover_requested: Option<Duration>,
    /// The frame that completed the takeover was presented.
    pub first_frame_submitted: Option<Duration>,
    /// The splash was removed in the same compositor transaction as that frame
    /// (macOS) or after the compositor latched it (Windows); without a splash,
    /// the same instant as `first_frame_submitted`.
    pub handoff_completed: Option<Duration>,
    /// Every native object the splash created has been released.
    pub splash_released: Option<Duration>,
}

/// What startup cost, for the structural gates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartupWork {
    /// Longest single event-loop callback between host entry and handoff.
    pub longest_event_thread_block: Duration,
    /// GPU devices this startup requested: one per presentation target tried,
    /// so two only when a composed target failed and the plain one followed.
    pub devices_requested: usize,
    /// Scene painters created (one per surface format) before the handoff.
    pub painters_created: usize,
    /// Reading a [`SplashLogo::packaged`] logo out of the package: one read,
    /// on the event thread, before the window is shown. `None` for an
    /// embedded logo, and when no splash was attempted.
    pub splash_logo_read: Option<Duration>,
    pub splash: SplashWork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupStatus {
    pub phase: StartupPhase,
    pub splash: SplashOutcome,
    /// The ticket a takeover request has to carry now. `None` before UiReady
    /// and after handoff.
    pub ticket: Option<StartupTicket>,
    pub timeline: StartupTimeline,
    pub work: StartupWork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupError {
    /// The ticket was retired by a cancel, or the program is not UiReady yet.
    StaleTicket,
    AlreadyHandedOff,
    HostStopped,
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::StaleTicket => "startup ticket is stale",
            Self::AlreadyHandedOff => "startup has already handed off",
            Self::HostStopped => "startup host has stopped",
        })
    }
}

impl std::error::Error for StartupError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupRequest {
    TakeOver(StartupTicket),
    Cancel(StartupTicket),
}

pub(crate) struct StartupShared {
    entry: Instant,
    status: Mutex<StartupStatus>,
    requests: Mutex<Vec<StartupRequest>>,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// The one startup record of a host, readable from any thread at any time, so
/// a front end that attaches late asks for the state instead of waiting for an
/// event it may already have missed.
#[derive(Clone)]
pub struct StartupHandle {
    shared: Arc<StartupShared>,
}

impl std::fmt::Debug for StartupHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupHandle")
            .field("status", &self.status())
            .finish()
    }
}

impl StartupHandle {
    pub(crate) fn new(entry: Instant, splash: SplashOutcome) -> Self {
        Self {
            shared: Arc::new(StartupShared {
                entry,
                status: Mutex::new(StartupStatus {
                    phase: StartupPhase::Starting,
                    splash,
                    ticket: None,
                    timeline: StartupTimeline::default(),
                    work: StartupWork::default(),
                }),
                requests: Mutex::new(Vec::new()),
                wake: Mutex::new(None),
            }),
        }
    }

    /// A record no host drives: it stays `Starting`, and takeover requests
    /// are refused with [`StartupError::HostStopped`]. For code that runs a
    /// program without a host — tests, offscreen tools.
    pub fn detached() -> Self {
        Self::new(
            Instant::now(),
            SplashOutcome::Skipped(SplashSkip::NotConfigured),
        )
    }

    /// A record for a host with no startup of its own to report — an embedded
    /// runtime, whose embedder already put its window on screen.
    pub(crate) fn settled(splash: SplashOutcome) -> Self {
        let handle = Self::new(Instant::now(), splash);
        handle.update(|status| status.phase = StartupPhase::HandedOff);
        handle
    }

    pub fn status(&self) -> StartupStatus {
        self.shared
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// Asks the host to replace the splash with the program's current primary
    /// document once a frame of it is presented. Safe from any thread; the host
    /// applies it on its own.
    pub fn take_over(&self, ticket: StartupTicket) -> Result<(), StartupError> {
        self.check(ticket)?;
        self.push(StartupRequest::TakeOver(ticket))
    }

    /// Withdraws a takeover that has not completed and retires its ticket. The
    /// splash stays; a new request needs the new ticket from [`Self::status`].
    pub fn cancel_takeover(&self, ticket: StartupTicket) -> Result<(), StartupError> {
        self.check(ticket)?;
        self.push(StartupRequest::Cancel(ticket))
    }

    fn check(&self, ticket: StartupTicket) -> Result<(), StartupError> {
        let status = self.status();
        match (status.phase, status.ticket) {
            (StartupPhase::HandedOff, _) => Err(StartupError::AlreadyHandedOff),
            (_, Some(current)) if current == ticket => Ok(()),
            _ => Err(StartupError::StaleTicket),
        }
    }

    fn push(&self, request: StartupRequest) -> Result<(), StartupError> {
        let wake = self
            .shared
            .wake
            .lock()
            .ok()
            .and_then(|wake| wake.clone())
            .ok_or(StartupError::HostStopped)?;
        if let Ok(mut requests) = self.shared.requests.lock() {
            requests.push(request);
        }
        wake();
        Ok(())
    }

    pub(crate) fn set_wake(&self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        if let Ok(mut slot) = self.shared.wake.lock() {
            *slot = wake;
        }
    }

    pub(crate) fn take_requests(&self) -> Vec<StartupRequest> {
        self.shared
            .requests
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut StartupStatus)) {
        let mut status = self
            .shared
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        change(&mut status);
    }

    /// Time since the host's entry, for a timeline field.
    pub(crate) fn elapsed(&self) -> Duration {
        self.shared.entry.elapsed()
    }
}
