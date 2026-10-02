//! Two-phase startup (Issue #225): an Early Splash while the device is
//! requested, an ordinary loading page once the program is ready, then the
//! main screen when the (simulated) business work finishes.
//!
//! Run it plainly to watch it. `--probe` makes it an acceptance check: it
//! exits by itself a second after the handoff, prints the startup record as
//! one JSON line, and fails if the record breaks the contract.
//!
//! ```text
//! --no-splash               no Early Splash
//! --animation=NAME          none | fade | pulse | rotate (default fade)
//! --bad-logo                a logo that is not a PNG
//! --packaged-logo[=URL]     read the logo from the package's early-splash pack
//!                           (default nana://res/splash-logo.png); from
//!                           `examples/assets` when run from `target/`
//! --defer=MS                keep the splash until MS after UiReady, then take over
//! --cancel-first            with --defer: request, cancel, then request again
//! --app-ms=MS               simulated business initialization (default 1500)
//! --first-frame-ms=MS       block the event thread MS in the first prepare
//! --exit-before-handoff     exit from UiReady without ever taking over
//! --hidden                  start hidden (a tray start: no splash), show later
//! --probe                   exit after the handoff and check the record
//! ```
//!
//! The host's fault injection applies too: `NANA_STARTUP_GPU_DELAY_MS=N`
//! delays the device request, `NANA_STARTUP_GPU_FAIL=1` fails it.

use std::collections::HashMap;
use std::time::Duration;

use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{Entity, FrameworkError, List, Text};
use nana_ui::{
    ApplicationState, ApplicationWindow, NanaApplication, RuntimeApplication,
    RuntimeProgramContext, RuntimeProgramUpdate, SplashAnimation, SplashLogo, SplashSpec,
    StartupPhase, StartupStatus, StartupTakeover, WindowDescriptor,
};
use nana_ui_platform::WindowId;

static LOGO: &[u8] = include_bytes!("assets/splash-logo.png");
static NOT_A_LOGO: &[u8] = b"this is not a PNG";
/// The same PNG, as the packaged build's early-splash pack holds it.
const PACKAGED_LOGO: &str = "nana://res/splash-logo.png";

#[derive(Clone, Default)]
struct Options {
    splash: bool,
    animation: Option<SplashAnimation>,
    bad_logo: bool,
    packaged_logo: Option<&'static str>,
    defer: Option<u64>,
    cancel_first: bool,
    app_ms: u64,
    first_frame_ms: u64,
    exit_before_handoff: bool,
    hidden: bool,
    probe: bool,
}

fn options() -> Options {
    let mut options = Options {
        splash: true,
        app_ms: 1500,
        ..Options::default()
    };
    let millis = |value: &str| value.parse::<u64>().expect("milliseconds");
    for argument in std::env::args().skip(1) {
        let (flag, value) = argument
            .split_once('=')
            .map_or((argument.as_str(), ""), |(flag, value)| (flag, value));
        match flag {
            "--no-splash" => options.splash = false,
            "--animation" => {
                options.animation = Some(match value {
                    "none" => SplashAnimation::None,
                    "fade" => SplashAnimation::FadeIn,
                    "pulse" => SplashAnimation::Pulse,
                    "rotate" => SplashAnimation::Rotate,
                    other => panic!("unknown animation {other}"),
                });
            }
            "--bad-logo" => options.bad_logo = true,
            "--packaged-logo" => {
                options.packaged_logo = Some(if value.is_empty() {
                    PACKAGED_LOGO
                } else {
                    // A process-lifetime option, like the embedded logo.
                    Box::leak(value.to_owned().into_boxed_str())
                });
            }
            "--defer" => options.defer = Some(millis(value)),
            "--cancel-first" => options.cancel_first = true,
            "--app-ms" => options.app_ms = millis(value),
            "--first-frame-ms" => options.first_frame_ms = millis(value),
            "--exit-before-handoff" => options.exit_before_handoff = true,
            "--hidden" => options.hidden = true,
            "--probe" => options.probe = true,
            other => panic!("unknown argument {other}"),
        }
    }
    options
}

thread_local! {
    static OPTIONS: Options = options();
}

fn with_options<R>(read: impl FnOnce(&Options) -> R) -> R {
    OPTIONS.with(read)
}

enum Message {
    /// Simulated business initialization progress, in percent.
    Progress(u32),
    Loaded,
    TakeOver,
    ExitNow,
    Show,
    /// Starts the idle window the probe checks.
    Idle,
    Finish,
}

struct Screen {
    title: Entity<Text>,
    detail: Entity<Text>,
}

struct Demo {
    screens: HashMap<WindowId, Screen>,
    first_prepare: bool,
    handed_off: bool,
    loaded: bool,
    /// Frames drawn in the probe's idle second, after the handoff and the
    /// business work are both over.
    idle_frames: Option<u64>,
    /// The ticket a `--cancel-first` run retired.
    retired: Option<nana_ui::StartupTicket>,
}

/// Delivers `message` after `delay`, from a timer thread.
fn after(
    context: &RuntimeProgramContext<Message>,
    delay: Duration,
    message: impl FnOnce() -> Message + Send + 'static,
) {
    let context = context.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        context.dispatch(message());
    });
}

impl Demo {
    /// Once the handoff and the business work are both over and their last
    /// frame has had time to draw, nothing may draw for a second: no startup
    /// timer, splash or animation is left to wake the window.
    fn start_idle_check(&self, context: &RuntimeProgramContext<Message>) {
        if !(self.handed_off && self.loaded) || !with_options(|options| options.probe) {
            return;
        }
        after(context, Duration::from_millis(300), || Message::Idle);
        after(context, Duration::from_millis(1300), || Message::Finish);
    }
}

impl ApplicationState for Demo {
    type Message = Message;
    type Error = FrameworkError;

    fn initialize(context: &RuntimeProgramContext<Message>) -> Result<Self, FrameworkError> {
        // UiReady: the host can draw now. The business work runs on a thread
        // of its own and reports back through a clone of the context; the
        // loading page is what `build` mounts.
        let app_ms = with_options(|options| options.app_ms);
        let business = context.clone();
        std::thread::spawn(move || {
            let steps = 5u64;
            for step in 1..=steps {
                std::thread::sleep(Duration::from_millis(app_ms / steps));
                business.dispatch(if step == steps {
                    Message::Loaded
                } else {
                    Message::Progress((step * 100 / steps) as u32)
                });
            }
        });
        if let Some(defer) = with_options(|options| options.defer) {
            after(context, Duration::from_millis(defer), || Message::TakeOver);
        }
        if with_options(|options| options.exit_before_handoff) {
            after(context, Duration::from_millis(300), || Message::ExitNow);
        }
        if with_options(|options| options.hidden) {
            after(context, Duration::from_millis(800), || Message::Show);
        }
        Ok(Self {
            screens: HashMap::new(),
            first_prepare: true,
            handed_off: false,
            loaded: false,
            idle_frames: None,
            retired: None,
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Message>,
    ) -> Result<(), FrameworkError> {
        let document = window.document.document();
        let (_, (title, detail)) =
            window
                .document
                .context_mut()
                .mount_view_root(document, || {
                    let title = entity_ref::<Text>();
                    let detail = entity_ref::<Text>();
                    let loading = widget(List::new()).children((
                        widget(Text::new("正在加载")).entity_ref(title),
                        widget(Text::new("0%")).entity_ref(detail),
                    ));
                    with_refs(loading, (title, detail))
                })?;
        self.screens
            .insert(context.window_id(), Screen { title, detail });
        Ok(())
    }

    fn startup_takeover(&self) -> StartupTakeover {
        let deferred =
            with_options(|options| options.defer.is_some() || options.exit_before_handoff);
        if deferred {
            StartupTakeover::Deferred
        } else {
            StartupTakeover::Immediate
        }
    }

    fn startup_changed(
        &mut self,
        status: &StartupStatus,
        _windows: &mut HashMap<WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        if status.phase == StartupPhase::HandedOff && !self.handed_off {
            self.handed_off = true;
            self.start_idle_check(context);
        }
        RuntimeProgramUpdate::default()
    }

    fn prepare(&mut self, _window: &mut ApplicationWindow, _: &RuntimeProgramContext<Message>) {
        if std::mem::take(&mut self.first_prepare) {
            let block = with_options(|options| options.first_frame_ms);
            if block > 0 {
                std::thread::sleep(Duration::from_millis(block));
            }
        }
        if let Some(frames) = self.idle_frames.as_mut() {
            *frames += 1;
        }
    }

    fn update(
        &mut self,
        message: Message,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        let id = WindowId::PRIMARY;
        let (title, detail) = match message {
            Message::Progress(percent) => ("正在加载".to_owned(), format!("{percent}%")),
            Message::Loaded => {
                self.loaded = true;
                self.start_idle_check(context);
                ("已就绪".to_owned(), "主界面".to_owned())
            }
            Message::TakeOver => {
                let startup = context.startup();
                let ticket = startup.status().ticket.expect("UiReady has a ticket");
                if let Some(retired) = self.retired {
                    // The host has applied the cancel by now: the old ticket
                    // is refused, the new one is not.
                    assert_ne!(retired, ticket, "cancel did not retire the ticket");
                    assert_eq!(
                        startup.take_over(retired),
                        Err(nana_ui::StartupError::StaleTicket)
                    );
                }
                if with_options(|options| options.cancel_first) && self.retired.is_none() {
                    self.retired = Some(ticket);
                    startup.take_over(ticket).expect("first request");
                    startup.cancel_takeover(ticket).expect("cancel");
                    after(context, Duration::from_millis(200), || Message::TakeOver);
                } else {
                    startup.take_over(ticket).expect("takeover request");
                }
                return RuntimeProgramUpdate::default();
            }
            Message::ExitNow => return RuntimeProgramUpdate::exit(),
            Message::Show => {
                // A hidden start had no splash; the handoff is this window's
                // first frame once it is shown.
                if with_options(|options| options.splash) {
                    assert_eq!(
                        context.startup().status().splash,
                        nana_ui::SplashOutcome::Skipped(nana_ui::SplashSkip::HiddenStart)
                    );
                }
                drop(context.window().set_visible(true));
                return RuntimeProgramUpdate::default();
            }
            Message::Idle => {
                self.idle_frames = Some(0);
                return RuntimeProgramUpdate::default();
            }
            Message::Finish => {
                report(&context.startup().status(), self.idle_frames.unwrap_or(0));
                return RuntimeProgramUpdate::exit();
            }
        };
        if let (Some(window), Some(screen)) = (windows.get_mut(&id), self.screens.get(&id)) {
            let context = window.document.context_mut();
            let _ = context.update_component(screen.title, |text, _| text.value = title);
            let _ = context.update_component(screen.detail, |text, _| text.value = detail);
        }
        RuntimeProgramUpdate::redraw(id)
    }
}

fn millis(value: Option<Duration>) -> String {
    value.map_or_else(
        || "null".to_owned(),
        |value| format!("{:.3}", value.as_secs_f64() * 1e3),
    )
}

/// One JSON line with the startup record; the process exits non-zero when the
/// record breaks the contract.
fn report(status: &StartupStatus, idle_frames: u64) {
    let timeline = &status.timeline;
    let work = &status.work;
    println!(
        concat!(
            "{{\"phase\":\"{}\",\"splash\":\"{}\",",
            "\"splash_committed_ms\":{},\"ui_ready_ms\":{},\"takeover_requested_ms\":{},",
            "\"first_frame_submitted_ms\":{},\"handoff_completed_ms\":{},\"splash_released_ms\":{},",
            "\"longest_block_ms\":{:.3},\"devices_requested\":{},\"painters_created\":{},",
            "\"logo_decodes\":{},\"logo_uploads\":{},\"animation_submissions\":{},",
            "\"splash_commits\":{},\"splash_live_resources\":{},\"logo_source\":\"{}\",",
            "\"splash_logo_read_ms\":{},\"idle_frames\":{}}}"
        ),
        status.phase.label(),
        // The outcome's Debug form quotes pack names and reasons.
        format!("{:?}", status.splash)
            .replace('\\', "\\\\")
            .replace('"', "\\\""),
        millis(timeline.splash_committed),
        millis(timeline.ui_ready),
        millis(timeline.takeover_requested),
        millis(timeline.first_frame_submitted),
        millis(timeline.handoff_completed),
        millis(timeline.splash_released),
        work.longest_event_thread_block.as_secs_f64() * 1e3,
        work.devices_requested,
        work.painters_created,
        work.splash.logo_decodes,
        work.splash.logo_uploads,
        work.splash.animation_submissions,
        work.splash.commits,
        work.splash.live_resources,
        if with_options(|options| options.packaged_logo.is_some()) {
            "packaged"
        } else {
            "embedded"
        },
        millis(work.splash_logo_read),
        idle_frames,
    );
    let mut failures = Vec::new();
    if status.phase != StartupPhase::HandedOff {
        failures.push("startup did not hand off");
    }
    if work.devices_requested != 1 {
        failures.push("more than one device was requested");
    }
    if work.painters_created != 1 {
        failures.push("the primary painter was created more than once");
    }
    if work.splash.live_resources != 0 {
        failures.push("splash resources outlived the handoff");
    }
    if status.splash.is_shown() {
        let order = [
            timeline.splash_committed,
            timeline.ui_ready,
            timeline.takeover_requested,
            timeline.first_frame_submitted,
            timeline.handoff_completed,
            timeline.splash_released,
        ];
        if order.iter().any(Option::is_none) || order.windows(2).any(|pair| pair[0] > pair[1]) {
            failures.push("startup milestones are missing or out of order");
        }
        if work.splash.animation_submissions > 1 {
            failures.push("the splash animation was submitted more than once");
        }
    }
    // One read of the package for a packaged logo that was attempted, none
    // for an embedded one.
    let attempted = !matches!(status.splash, nana_ui::SplashOutcome::Skipped(_));
    let packaged = with_options(|options| options.packaged_logo.is_some());
    if work.splash_logo_read.is_some() != (packaged && attempted) {
        failures.push("the logo read does not match the logo source");
    }
    if idle_frames > 0 {
        failures.push("the window kept drawing after the handoff");
    }
    if !failures.is_empty() {
        eprintln!("startup-splash probe failed: {}", failures.join("; "));
        std::process::exit(1);
    }
}

fn main() {
    let options = with_options(Options::clone);
    // With the identity marker, so `nana-packager` can package the example.
    let mut builder = NanaApplication::builder(nana_ui_platform::application_identity!(
        id: "dev.nanaui.startup-splash",
        name: "NanaUI Startup",
        version: env!("CARGO_PKG_VERSION"),
    ));
    if options.splash {
        let logo = match options.packaged_logo {
            Some(url) => SplashLogo::packaged(url),
            None => SplashLogo::png(if options.bad_logo { NOT_A_LOGO } else { LOGO }),
        };
        let mut splash = SplashSpec::new(logo).with_logo_size(128.0, 128.0);
        if let Some(animation) = options.animation {
            splash = splash.with_animation(animation);
        }
        builder = builder.early_splash(splash);
    }
    // A packaged logo is read through the package mount: the manifest's
    // early-splash pack in a package, `examples/assets` from `target/`.
    // Without `packaged-resources` the splash reports `Unsupported`.
    #[cfg(feature = "packaged-resources")]
    if options.packaged_logo.is_some() {
        builder = builder.resource_packs(
            nana_ui::ResourcePackOptions::new()
                .loose_root(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assets")),
        );
    }
    let mut window = WindowDescriptor::new("NanaUI Startup").initial_size(640.0, 420.0);
    window.visible = !options.hidden;
    let result = builder.run::<RuntimeApplication<Demo>>(window);
    if let Err(error) = result {
        eprintln!("startup-splash: {error}");
        std::process::exit(2);
    }
}
