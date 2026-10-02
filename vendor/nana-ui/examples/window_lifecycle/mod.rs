//! Native lifecycle acceptance; ordinary application code has no winit dependency.
use nana_ui::runtime::view::{IntoView, widget};
use nana_ui::runtime::{List, Text};
use nana_ui::{
    ApplicationState, ApplicationWindow, DisplayId, FullscreenMode, FullscreenRequest,
    RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor, WindowError, WindowHandle,
    WindowLevel,
};
use nana_ui_platform::{WindowId, WindowModeState};
use std::{
    collections::HashMap,
    sync::{Mutex, mpsc},
    time::Duration,
};

static RESULT: Mutex<Option<Result<(), String>>> = Mutex::new(None);
pub enum Message {
    Pump,
    Completed(Result<(), String>),
}

pub struct App {
    service: nana_ui::WindowService,
    worker: Option<std::thread::JoinHandle<()>>,
    exiting: bool,
    cleaned: std::collections::HashSet<WindowId>,
    /// Taken by the first `Pump`, which the host only delivers from its
    /// running event loop.
    looping: Option<mpsc::Sender<()>>,
    focused: mpsc::Sender<WindowId>,
    presented: mpsc::Sender<(WindowId, u64)>,
    resized: mpsc::Sender<(WindowId, (f32, f32))>,
    modes: mpsc::Sender<(WindowId, WindowModeState)>,
    tags: mpsc::Sender<(WindowId, Option<String>)>,
}
/// Application window kinds, told apart by `WindowDescriptor::tag` rather
/// than by the order of creation requests.
const CHARACTER: &str = "character";
const TRACKING: &str = "tracking";
const REJECTED: &str = "rejected";
pub fn descriptor(title: &str) -> WindowDescriptor {
    WindowDescriptor {
        title: title.into(),
        initial_size: (480.0, 320.0),
        minimum_size: (200.0, 120.0),
        system_caption: !std::env::args().any(|arg| arg == "--client-chrome"),
        ..Default::default()
    }
}
/// Every tag the application read for `handle`: once in `build`, once on `Ready`.
fn tagged(
    rx: &mpsc::Receiver<(WindowId, Option<String>)>,
    handle: &WindowHandle,
    expected: &str,
) -> Result<(), String> {
    let seen: Vec<_> = rx
        .try_iter()
        .filter(|(id, _)| *id == handle.id())
        .map(|(_, tag)| tag)
        .collect();
    if seen != [Some(expected.to_owned()), Some(expected.to_owned())] {
        return Err(format!("window {expected} read back tags {seen:?}"));
    }
    Ok(())
}
fn presented(
    rx: &mpsc::Receiver<(WindowId, u64)>,
    handle: &WindowHandle,
    generation: &mut Option<u64>,
) -> Result<(), String> {
    loop {
        let (id, gpu) = rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())?;
        if let Some(expected) = *generation {
            if expected != gpu {
                return Err("windows did not share GPU resources".into());
            }
        } else {
            *generation = Some(gpu);
        }
        if id == handle.id() {
            return Ok(());
        }
    }
}
impl ApplicationState for App {
    type Message = Message;
    type Error = String;
    fn initialize(context: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        let (tx, rx) = mpsc::channel();
        let (resized_tx, resized_rx) = mpsc::channel();
        let (focused_tx, focused_rx) = mpsc::channel();
        let (modes_tx, modes_rx) = mpsc::channel();
        let (tags_tx, tags_rx) = mpsc::channel();
        let (looping_tx, looping_rx) = mpsc::channel();
        if std::env::args().any(|arg| arg == "--probe-host-stop") {
            let service = context.windows().clone();
            let worker_service = service.clone();
            let (queued_tx, queued_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let pending = worker_service.create_window(descriptor("Pending at host stop"));
                queued_tx.send(()).unwrap();
                let result = if matches!(pending.wait(), Err(WindowError::HostStopped))
                    && matches!(
                        worker_service
                            .create_window(descriptor("After host stop"))
                            .wait(),
                        Err(WindowError::HostStopped)
                    ) {
                    Ok(())
                } else {
                    Err("host teardown did not resolve window requests".into())
                };
                *RESULT.lock().unwrap() = Some(result);
            });
            queued_rx
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?;
            return Ok(Self {
                service,
                worker: Some(worker),
                exiting: false,
                cleaned: Default::default(),
                looping: None,
                focused: focused_tx,
                presented: tx,
                resized: resized_tx,
                modes: modes_tx,
                tags: tags_tx,
            });
        }
        context.dispatch(Message::Pump);
        let service = context.windows().clone();
        let context = context.clone();
        std::thread::spawn(move || {
            let run = || -> Result<(), String> {
                let service = context.windows();
                let primary = context.window();
                let mut generation = None;
                // The host builds the scene painter once per device on its
                // window thread before it enters the loop; with FXC on a CPU
                // adapter in a debug build that is ~10 s, and the device-loss
                // probe pays it twice. That setup is the host's, so the first
                // frame is timed from the loop, not from this thread's start.
                looping_rx
                    .recv_timeout(Duration::from_secs(120))
                    .map_err(|error| format!("the host never entered its event loop: {error}"))?;
                presented(&rx, &primary, &mut generation)?;
                if !matches!(
                    service
                        .create_window(descriptor("Rejected").tag(REJECTED))
                        .wait(),
                    Err(WindowError::InitializationFailed(_))
                ) {
                    return Err("document creation failure was not rolled back".into());
                }
                let second = service
                    .create_window(descriptor("Character").tag(CHARACTER))
                    .wait()
                    .map_err(|e| e.to_string())?;
                tagged(&tags_rx, &second, CHARACTER)?;
                presented(&rx, &second, &mut generation)?;
                let third = service
                    .create_window(descriptor("Tracking").tag(TRACKING))
                    .wait()
                    .map_err(|e| e.to_string())?;
                tagged(&tags_rx, &third, TRACKING)?;
                presented(&rx, &third, &mut generation)?;
                second
                    .set_title("Updated on worker")
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_resizable(false)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_resizable(true)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_min_size(Some((240.0, 160.0)))
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_max_size(Some((800.0, 600.0)))
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_cursor(nana_ui::WindowCursor::Pointer)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_cursor_visible(false)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_cursor_visible(true)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_cursor(nana_ui::WindowCursor::Automatic)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_size((520.0, 360.0))
                    .wait()
                    .map_err(|e| e.to_string())?;
                loop {
                    let (id, size) = resized_rx
                        .recv_timeout(Duration::from_secs(20))
                        .map_err(|error| error.to_string())?;
                    if id == second.id() && size == (520.0, 360.0) {
                        break;
                    }
                }
                second
                    .effects()
                    .set_material(nana_ui::MaterialEffect::Solid)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_mouse_passthrough(true)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_mouse_passthrough(false)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_mouse_passthrough_forward(true)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second
                    .set_mouse_passthrough_forward(false)
                    .wait()
                    .map_err(|e| e.to_string())?;
                let display = service
                    .displays()
                    .wait()
                    .map_err(|e| e.to_string())?
                    .first()
                    .map(|display| display.id)
                    .ok_or("no display was enumerated")?;
                // Simple covers the named display without a macOS Space
                // switch, which the host environment often cannot complete.
                let on = |display| {
                    Some(FullscreenRequest {
                        mode: FullscreenMode::Simple,
                        display: Some(display),
                    })
                };
                if !matches!(
                    second.set_fullscreen(on(DisplayId(u128::MAX))).wait(),
                    Err(WindowError::InvalidParameter(_))
                ) {
                    return Err("fullscreen on a disconnected display was accepted".into());
                }
                second
                    .set_fullscreen(on(display))
                    .wait()
                    .map_err(|e| e.to_string())?;
                wait_for_mode(&modes_rx, second.id(), |mode| {
                    mode.fullscreen.is_some() && mode.display.is_none_or(|shown| shown == display)
                })?;
                second
                    .set_window_level(WindowLevel::AlwaysOnTop)
                    .wait()
                    .map_err(|e| e.to_string())?;
                wait_for_mode(&modes_rx, second.id(), |mode| {
                    mode.level == WindowLevel::AlwaysOnTop
                })?;
                second
                    .set_fullscreen(None)
                    .wait()
                    .map_err(|e| e.to_string())?;
                wait_for_mode(&modes_rx, second.id(), |mode| mode.fullscreen.is_none())?;
                second
                    .set_window_level(WindowLevel::Normal)
                    .wait()
                    .map_err(|e| e.to_string())?;
                for skip in [true, false] {
                    let outcome = second.set_skip_taskbar(skip).wait();
                    let reported = if cfg!(target_os = "windows") {
                        outcome.is_ok()
                    } else {
                        matches!(outcome, Err(WindowError::Unsupported(_)))
                    };
                    if !reported {
                        return Err(format!("skip taskbar {skip} reported {outcome:?}"));
                    }
                }
                let mut opens_fullscreen = descriptor("Opens fullscreen");
                opens_fullscreen.fullscreen = Some(FullscreenRequest {
                    mode: FullscreenMode::Simple,
                    display: None,
                });
                let opened = service
                    .create_window(opens_fullscreen)
                    .wait()
                    .map_err(|e| e.to_string())?;
                wait_for_mode(&modes_rx, opened.id(), |mode| mode.fullscreen.is_some())?;
                opened.close().wait().map_err(|e| e.to_string())?;
                second
                    .set_visible(false)
                    .wait()
                    .map_err(|e| e.to_string())?;
                second.set_visible(true).wait().map_err(|e| e.to_string())?;
                second
                    .with_native_handle(|handle| {
                        let _ = handle.as_raw();
                    })
                    .wait()
                    .map_err(|e| e.to_string())?;
                primary.close().wait().map_err(|e| e.to_string())?;
                third.request_redraw().wait().map_err(|e| e.to_string())?;
                presented(&rx, &third, &mut generation)?;
                let mut modal_descriptor = descriptor("Modal child");
                modal_descriptor.parent = Some(second.id());
                modal_descriptor.modal = true;
                let modal = service
                    .create_window(modal_descriptor.clone())
                    .wait()
                    .map_err(|e| e.to_string())?;
                presented(&rx, &modal, &mut generation)?;
                if !matches!(
                    service.create_window(modal_descriptor.clone()).wait(),
                    Err(WindowError::InitializationFailed(_))
                ) {
                    return Err("a second modal child bypassed the blocked parent".into());
                }
                modal_descriptor.title = "Nested modal child".into();
                modal_descriptor.parent = Some(modal.id());
                let nested = service
                    .create_window(modal_descriptor)
                    .wait()
                    .map_err(|e| e.to_string())?;
                presented(&rx, &nested, &mut generation)?;
                third.focus().wait().map_err(|e| e.to_string())?;
                wait_for_focus(&focused_rx, third.id())?;
                while focused_rx.try_recv().is_ok() {}
                second.focus().wait().map_err(|e| e.to_string())?;
                wait_for_focus(&focused_rx, nested.id())?;
                second.close().wait().map_err(|e| e.to_string())?;
                if second.request_redraw().wait() != Err(WindowError::WindowClosed) {
                    return Err("closed handle still accepted commands".into());
                }
                if nested.request_redraw().wait() != Err(WindowError::WindowClosed) {
                    return Err("closing the parent did not release the nested modal child".into());
                }
                if modal.request_redraw().wait() != Err(WindowError::WindowClosed) {
                    return Err("closing the parent did not release its modal child".into());
                }
                let replacement = service
                    .create_window(descriptor("Replacement"))
                    .wait()
                    .map_err(|e| e.to_string())?;
                if replacement.id() == second.id() {
                    return Err("window identity reused".into());
                }
                presented(&rx, &replacement, &mut generation)?;
                replacement.close().wait().map_err(|e| e.to_string())?;
                Ok(())
            };
            context.dispatch(Message::Completed(run()));
        });
        Ok(Self {
            service,
            worker: None,
            exiting: false,
            cleaned: Default::default(),
            looping: Some(looping_tx),
            focused: focused_tx,
            presented: tx,
            resized: resized_tx,
            modes: modes_tx,
            tags: tags_tx,
        })
    }
    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let tag = context.window_tag();
        let _ = self
            .tags
            .send((context.window_id(), tag.map(str::to_owned)));
        if tag == Some(REJECTED) {
            return Err("intentional document build failure".into());
        }
        let document = window.document.document();
        let id = context.window_id().0;
        window
            .document
            .context_mut()
            .mount_view_root(document, || match tag {
                Some(CHARACTER) => widget(List::new().label("Character"))
                    .children((
                        widget(Text::new(format!("Character {id}"))),
                        widget(Text::new("Model")),
                    ))
                    .into_any(),
                Some(TRACKING) => widget(List::new().label("Tracking"))
                    .children(widget(Text::new(format!("Tracking {id}"))))
                    .into_any(),
                _ => widget(List::new())
                    .children(widget(Text::new(format!("Window {id}"))))
                    .into_any(),
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    fn window_event(
        &mut self,
        event: &nana_ui_platform::WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if let nana_ui_platform::WindowEvent::Ready { id, .. } = event {
            let _ = self
                .tags
                .send((*id, context.window_tag().map(str::to_owned)));
        }
        if let nana_ui_platform::WindowEvent::Closed { id } = event {
            assert!(
                self.cleaned.contains(id),
                "Closed was delivered before application cleanup"
            );
        }
        if let nana_ui_platform::WindowEvent::Resized { id, geometry } = event {
            let _ = self.resized.send((*id, geometry.logical_size));
        }
        if let nana_ui_platform::WindowEvent::FocusChanged { id, focused: true } = event {
            let _ = self.focused.send(*id);
        }
        if let nana_ui_platform::WindowEvent::ModeChanged { id, mode } = event {
            let _ = self.modes.send((*id, *mode));
        }
        RuntimeProgramUpdate::default()
    }
    fn presented(
        &mut self,
        _window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let _ = self
            .presented
            .send((context.window_id(), context.gpu().generation().get()));
        RuntimeProgramUpdate::default()
    }
    fn window_closed(&mut self, id: WindowId) {
        assert!(self.cleaned.insert(id), "window cleanup ran twice");
        if self.exiting {
            let mut request = self
                .service
                .create_window(descriptor("Must not reopen during exit"));
            assert!(matches!(
                request.try_take(),
                Some(Err(WindowError::HostStopped))
            ));
        }
    }
    fn update(
        &mut self,
        result: Self::Message,
        _windows: &mut HashMap<WindowId, ApplicationWindow>,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let result = match result {
            Message::Pump => {
                if let Some(looping) = self.looping.take() {
                    let _ = looping.send(());
                }
                context.dispatch(Message::Pump);
                return RuntimeProgramUpdate::default();
            }
            Message::Completed(result) => result,
        };
        self.exiting = true;
        *RESULT.lock().unwrap() = Some(result);
        RuntimeProgramUpdate::exit()
    }
}
pub fn verify() {
    RESULT
        .lock()
        .unwrap()
        .take()
        .expect("lifecycle did not complete")
        .expect("lifecycle failed");
    println!(
        "Window lifecycle passed: three windows, tagged character and tracking documents, shared GPU, worker controls, display-targeted fullscreen and level reported by ModeChanged, primary close, stale handle, recreate and present."
    );
}

fn wait_for_mode(
    rx: &mpsc::Receiver<(WindowId, WindowModeState)>,
    expected: WindowId,
    reached: impl Fn(&WindowModeState) -> bool,
) -> Result<(), String> {
    let mut seen = Vec::new();
    loop {
        let (id, mode) = rx.recv_timeout(Duration::from_secs(20)).map_err(|_| {
            format!(
                "window {} never reported the expected mode; seen {seen:?}",
                expected.0
            )
        })?;
        if id == expected && reached(&mode) {
            return Ok(());
        }
        seen.push((id.0, mode));
    }
}

fn wait_for_focus(rx: &mpsc::Receiver<WindowId>, expected: WindowId) -> Result<(), String> {
    loop {
        let id = rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())?;
        if id == expected {
            return Ok(());
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.join().expect("lifecycle worker panicked");
        }
    }
}
