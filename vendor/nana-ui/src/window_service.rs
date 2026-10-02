//! Thread-safe window requests. Native objects remain on the host thread.
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{
    DisplayInfo, FullscreenRequest, MousePassthroughMode, WindowId, WindowResizeEdge,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex, mpsc},
    task::{Context, Poll, Waker},
    thread::ThreadId,
};

pub use nana_ui_platform::{WindowDescriptor, WindowLevel, WindowSurfacePreference};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    HostStopped,
    /// The host has not yet processed its bounded request backlog.
    QueueFull,
    WindowClosed,
    InvalidParameter(String),
    Unsupported(String),
    InitializationFailed(String),
    OperationFailed(String),
    HostThreadWait,
}
impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WindowError {}

struct Completion<T> {
    value: Option<Result<T, WindowError>>,
    waker: Option<Waker>,
}
struct Shared<T> {
    state: Mutex<Completion<T>>,
    ready: Condvar,
}
/// A request may be awaited anywhere, or waited on from a worker thread.
#[must_use = "observe the result to detect rejected window operations"]
pub struct WindowRequest<T> {
    shared: Arc<Shared<T>>,
    host_thread: ThreadId,
}
pub(crate) struct Reply<T>(Arc<Shared<T>>, bool);
impl<T> Reply<T> {
    pub(crate) fn finish(mut self, value: Result<T, WindowError>) {
        self.1 = true;
        let waker = {
            let mut state = self.0.state.lock().unwrap();
            state.value = Some(value);
            state.waker.take()
        };
        self.0.ready.notify_all();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let waker = {
            let mut state = self.0.state.lock().unwrap();
            if state.value.is_none() {
                state.value = Some(Err(WindowError::HostStopped));
            }
            state.waker.take()
        };
        self.0.ready.notify_all();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
impl<T> WindowRequest<T> {
    fn pair(host_thread: ThreadId) -> (Self, Reply<T>) {
        let shared = Arc::new(Shared {
            state: Mutex::new(Completion {
                value: None,
                waker: None,
            }),
            ready: Condvar::new(),
        });
        (
            Self {
                shared: shared.clone(),
                host_thread,
            },
            Reply(shared, false),
        )
    }
    pub fn wait(self) -> Result<T, WindowError> {
        if std::thread::current().id() == self.host_thread {
            return Err(WindowError::HostThreadWait);
        }
        let mut state = self.shared.state.lock().unwrap();
        loop {
            if let Some(value) = state.value.take() {
                return value;
            }
            state = self.shared.ready.wait(state).unwrap();
        }
    }
    pub fn try_take(&mut self) -> Option<Result<T, WindowError>> {
        self.shared.state.lock().unwrap().value.take()
    }
}
impl<T> Future for WindowRequest<T> {
    type Output = Result<T, WindowError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(value) = state.value.take() {
            Poll::Ready(value)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCursor {
    /// Restore Runtime/CSS cursor selection.
    Automatic,
    Default,
    Pointer,
    Text,
    Move,
    Grab,
    Grabbing,
    NotAllowed,
    Crosshair,
    Help,
    Wait,
    Progress,
    ZoomIn,
    ZoomOut,
    /// Hide the system cursor. Does not load a custom image.
    None,
}

#[expect(
    clippy::large_enum_variant,
    reason = "boxing `Command` touches every platform host's match arms"
)]
pub(crate) enum Control {
    Command(WindowCommand),
    Visible(bool),
    Size((f64, f64)),
    MinSize(Option<(f64, f64)>),
    MaxSize(Option<(f64, f64)>),
    Resizable(bool),
    Level(WindowLevel),
    Cursor(WindowCursor),
    CursorVisible(bool),
    ContentProtected(bool),
    Redraw,
    Resize(WindowResizeEdge),
}
pub(crate) type NativeCallback =
    Box<dyn for<'a> FnOnce(Result<raw_window_handle::WindowHandle<'a>, WindowError>) + Send>;

pub(crate) enum Request {
    Material(
        WindowId,
        u64,
        crate::MaterialEffect,
        Reply<crate::MaterialOutcome>,
    ),
    Displays(Reply<Vec<DisplayInfo>>),
    Create(WindowDescriptor, Reply<WindowHandle>),
    Control(WindowId, u64, Control, Reply<()>),
    Native(WindowId, u64, NativeCallback),
}
impl Request {
    fn reject(self, error: WindowError) {
        match self {
            Self::Material(_, _, _, reply) => reply.finish(Err(error)),
            Self::Displays(reply) => reply.finish(Err(error)),
            Self::Create(_, reply) => reply.finish(Err(error)),
            Self::Control(_, _, _, reply) => reply.finish(Err(error)),
            Self::Native(_, _, callback) => {
                callback(Err(error));
            }
        }
    }
}
#[derive(Clone)]
pub struct WindowService {
    sender: mpsc::SyncSender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
    host_thread: ThreadId,
    identities: Arc<Mutex<(u64, std::collections::HashMap<WindowId, u64>)>>,
}
impl WindowService {
    pub(crate) fn channel(wake: Arc<dyn Fn() + Send + Sync>) -> (Self, mpsc::Receiver<Request>) {
        let (sender, receiver) = mpsc::sync_channel(1024);
        (
            Self {
                sender,
                wake,
                host_thread: std::thread::current().id(),
                identities: Arc::new(Mutex::new((0, std::collections::HashMap::new()))),
            },
            receiver,
        )
    }
    fn submit(&self, request: Request) {
        match self.sender.try_send(request) {
            Ok(()) => (self.wake)(),
            Err(mpsc::TrySendError::Full(request)) => request.reject(WindowError::QueueFull),
            Err(mpsc::TrySendError::Disconnected(request)) => {
                request.reject(WindowError::HostStopped)
            }
        }
    }
    pub fn create_window(&self, descriptor: WindowDescriptor) -> WindowRequest<WindowHandle> {
        let (request, reply) = WindowRequest::pair(self.host_thread);
        if let Err(error) = validate_descriptor(&descriptor) {
            reply.finish(Err(error));
            return request;
        }
        self.submit(Request::Create(descriptor, reply));
        request
    }
    /// Displays connected now, enumerated on the window thread.
    pub fn displays(&self) -> WindowRequest<Vec<DisplayInfo>> {
        let (request, reply) = WindowRequest::pair(self.host_thread);
        self.submit(Request::Displays(reply));
        request
    }
    pub(crate) fn register(&self, id: WindowId) {
        let mut identities = self.identities.lock().unwrap();
        identities.0 = identities
            .0
            .checked_add(1)
            .expect("window generations exhausted");
        let generation = identities.0;
        identities.1.insert(id, generation);
    }
    pub(crate) fn unregister(&self, id: WindowId) {
        self.identities.lock().unwrap().1.remove(&id);
    }
    pub(crate) fn is_current(&self, id: WindowId, generation: u64) -> bool {
        self.identities.lock().unwrap().1.get(&id) == Some(&generation)
    }
    pub(crate) fn handle(&self, id: WindowId) -> WindowHandle {
        let generation = self
            .identities
            .lock()
            .unwrap()
            .1
            .get(&id)
            .copied()
            .unwrap_or(0);
        WindowHandle {
            id,
            generation,
            service: self.clone(),
        }
    }
}
#[derive(Clone)]
pub struct WindowHandle {
    id: WindowId,
    generation: u64,
    service: WindowService,
}
impl WindowHandle {
    /// Runs on the window thread. The borrowed native handle is valid only
    /// during the callback; do not retain raw pointers or use them after return.
    pub fn with_native_handle<R: Send + 'static>(
        &self,
        callback: impl for<'a> FnOnce(raw_window_handle::WindowHandle<'a>) -> R + Send + 'static,
    ) -> WindowRequest<R> {
        let (request, reply) = WindowRequest::pair(self.service.host_thread);
        let native = Box::new(
            move |handle: Result<raw_window_handle::WindowHandle<'_>, WindowError>| {
                reply.finish(handle.map(callback))
            },
        );
        self.service
            .submit(Request::Native(self.id, self.generation, native));
        request
    }

    pub fn capture(&self) -> WindowCapture {
        WindowCapture(self.clone())
    }
    pub fn effects(&self) -> WindowEffects {
        WindowEffects(self.clone())
    }
    pub fn set_icon(&self, icon: Option<nana_ui_platform::WindowIcon>) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetIcon {
            id: self.id,
            icon,
        }))
    }
    pub fn set_menu_bar(&self, bar: Option<nana_ui_core::MenuBar>) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetMenuBar {
            id: self.id,
            bar,
        }))
    }
    pub fn open_file_dialog(&self, request: nana_ui_core::FileDialogRequest) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::OpenFileDialog {
            id: self.id,
            request,
        }))
    }
    pub fn focus(&self) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::Focus(self.id)))
    }
    pub fn set_minimized(&self, minimized: bool) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetMinimized {
            id: self.id,
            minimized,
        }))
    }
    pub fn set_maximized(&self, maximized: bool) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetMaximized {
            id: self.id,
            maximized,
        }))
    }
    pub fn id(&self) -> WindowId {
        self.id
    }
    fn control(&self, control: Control) -> WindowRequest<()> {
        let (request, reply) = WindowRequest::pair(self.service.host_thread);
        self.service
            .submit(Request::Control(self.id, self.generation, control, reply));
        request
    }
    pub fn set_title(&self, title: impl Into<String>) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetTitle {
            id: self.id,
            title: title.into(),
        }))
    }
    pub fn set_visible(&self, visible: bool) -> WindowRequest<()> {
        self.control(Control::Visible(visible))
    }
    pub fn set_size(&self, size: (f64, f64)) -> WindowRequest<()> {
        self.control(Control::Size(size))
    }
    pub fn set_min_size(&self, size: Option<(f64, f64)>) -> WindowRequest<()> {
        self.control(Control::MinSize(size))
    }
    pub fn set_max_size(&self, size: Option<(f64, f64)>) -> WindowRequest<()> {
        self.control(Control::MaxSize(size))
    }
    pub fn set_position(&self, position: (f32, f32)) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::Move {
            id: self.id,
            position,
        }))
    }
    pub fn set_resizable(&self, resizable: bool) -> WindowRequest<()> {
        self.control(Control::Resizable(resizable))
    }
    /// `None` leaves fullscreen. A display that is not connected fails with
    /// `InvalidParameter` and leaves the window unchanged; the effective mode
    /// arrives as `WindowEvent::ModeChanged`.
    pub fn set_fullscreen(&self, fullscreen: Option<FullscreenRequest>) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetFullscreen {
            id: self.id,
            fullscreen,
        }))
    }
    pub fn set_window_level(&self, level: WindowLevel) -> WindowRequest<()> {
        self.control(Control::Level(level))
    }
    /// Show or hide the taskbar entry. Platforms without a per-window entry
    /// fail with `Unsupported`; the effective state also arrives as
    /// `WindowEvent::SkipTaskbarChanged`.
    pub fn set_skip_taskbar(&self, skip_taskbar: bool) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetSkipTaskbar {
            id: self.id,
            skip_taskbar,
        }))
    }
    pub fn set_cursor(&self, cursor: WindowCursor) -> WindowRequest<()> {
        self.control(Control::Cursor(cursor))
    }
    pub fn set_cursor_visible(&self, visible: bool) -> WindowRequest<()> {
        self.control(Control::CursorVisible(visible))
    }
    pub fn request_redraw(&self) -> WindowRequest<()> {
        self.control(Control::Redraw)
    }
    /// Starts a native drag from the press being handled; the host then ends the
    /// gesture with a pointer `Cancel`. macOS requires a left-button press event.
    pub fn begin_drag(&self) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::Drag(self.id)))
    }
    pub fn begin_resize(&self, edge: WindowResizeEdge) -> WindowRequest<()> {
        self.control(Control::Resize(edge))
    }
    pub fn set_mouse_passthrough(&self, enabled: bool) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::SetMousePassthrough {
            id: self.id,
            enabled,
        }))
    }
    /// Host-owned Forward passthrough: sample the global pointer and recover
    /// hit-testing over opaque or interactive content. Widgets never see HWND.
    pub fn set_mouse_passthrough_forward(&self, enabled: bool) -> WindowRequest<()> {
        self.control(Control::Command(
            WindowCommand::SetMousePassthroughForward {
                id: self.id,
                enabled,
            },
        ))
    }
    pub fn set_mouse_passthrough_mode(&self, mode: MousePassthroughMode) -> WindowRequest<()> {
        match mode {
            MousePassthroughMode::Off => self.set_mouse_passthrough(false),
            MousePassthroughMode::Passthrough => self.set_mouse_passthrough(true),
            MousePassthroughMode::Forward => self.set_mouse_passthrough_forward(true),
        }
    }
    pub(crate) fn service(&self) -> &WindowService {
        &self.service
    }
    pub fn close(&self) -> WindowRequest<()> {
        self.control(Control::Command(WindowCommand::Close(self.id)))
    }
}
pub(crate) fn validate_descriptor(descriptor: &WindowDescriptor) -> Result<(), WindowError> {
    validate_size(descriptor.initial_size)?;
    validate_size(descriptor.minimum_size)?;
    if let Some(key) = descriptor.persist_key.as_deref() {
        nana_ui_core::RestorationKey::new(key).map_err(|error| {
            WindowError::InvalidParameter(format!("persist_key is invalid: {error}"))
        })?;
    }
    if descriptor
        .initial_position
        .is_some_and(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return Err(WindowError::InvalidParameter(
            "position must be finite".into(),
        ));
    }
    if descriptor.modal && descriptor.parent.is_none() {
        return Err(WindowError::InvalidParameter(
            "modal window requires a parent".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_size(size: (f64, f64)) -> Result<(), WindowError> {
    if !size.0.is_finite() || !size.1.is_finite() || size.0 <= 0.0 || size.1 <= 0.0 {
        Err(WindowError::InvalidParameter(
            "size must be finite and positive".into(),
        ))
    } else {
        Ok(())
    }
}

/// Platform appearance controls. Results describe the effect actually applied.
#[derive(Clone)]
pub struct WindowEffects(WindowHandle);
impl WindowEffects {
    pub fn set_material(
        &self,
        effect: crate::MaterialEffect,
    ) -> WindowRequest<crate::MaterialOutcome> {
        let (request, reply) = WindowRequest::pair(self.0.service.host_thread);
        self.0.service.submit(Request::Material(
            self.0.id,
            self.0.generation,
            effect,
            reply,
        ));
        request
    }
    pub fn set_mouse_passthrough(&self, enabled: bool) -> WindowRequest<()> {
        self.0.set_mouse_passthrough(enabled)
    }
    pub fn set_mouse_passthrough_forward(&self, enabled: bool) -> WindowRequest<()> {
        self.0.set_mouse_passthrough_forward(enabled)
    }
    pub fn set_mouse_passthrough_mode(&self, mode: MousePassthroughMode) -> WindowRequest<()> {
        self.0.set_mouse_passthrough_mode(mode)
    }
}

/// Native capture policy requests. Operating-system protection is best effort;
/// it does not guarantee exclusion from every capture mechanism.
#[derive(Clone)]
pub struct WindowCapture(WindowHandle);
impl WindowCapture {
    /// Requests native content protection on macOS/Windows. Other backends
    /// return Unsupported rather than silently claiming protection.
    pub fn set_protected(&self, protected: bool) -> WindowRequest<()> {
        self.0.control(Control::ContentProtected(protected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_creation_is_rejected_before_queueing_or_waking_the_host() {
        let (service, rx) =
            WindowService::channel(Arc::new(|| panic!("invalid request woke host")));
        for descriptor in [
            WindowDescriptor {
                modal: true,
                ..Default::default()
            },
            WindowDescriptor {
                initial_size: (f64::NAN, 1.0),
                ..Default::default()
            },
            WindowDescriptor {
                minimum_size: (0.0, 1.0),
                ..Default::default()
            },
            WindowDescriptor {
                initial_position: Some((f64::INFINITY, 0.0)),
                ..Default::default()
            },
            WindowDescriptor {
                persist_key: Some(String::new()),
                ..Default::default()
            },
            WindowDescriptor {
                persist_key: Some("main\0window".into()),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                service.create_window(descriptor).try_take(),
                Some(Err(WindowError::InvalidParameter(_)))
            ));
        }
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }
    #[test]
    fn saturated_queue_rejects_without_blocking_and_recovers_after_draining() {
        let (service, rx) = WindowService::channel(Arc::new(|| {}));
        let handle = service.handle(WindowId(7));
        let mut pending = Vec::new();
        loop {
            let mut request = handle.request_redraw();
            if let Some(result) = request.try_take() {
                assert_eq!(result, Err(WindowError::QueueFull));
                break;
            }
            pending.push(request);
            assert!(pending.len() < 10000, "request backlog must be bounded");
        }
        assert!(matches!(
            service
                .create_window(WindowDescriptor::default())
                .try_take(),
            Some(Err(WindowError::QueueFull))
        ));
        assert_eq!(
            handle
                .effects()
                .set_material(crate::MaterialEffect::Solid)
                .try_take(),
            Some(Err(WindowError::QueueFull))
        );
        assert_eq!(
            handle
                .with_native_handle(|_| panic!("rejected native callback must not run"))
                .try_take(),
            Some(Err(WindowError::QueueFull))
        );
        let Request::Control(_, _, _, reply) = rx.try_recv().unwrap() else {
            panic!("expected control");
        };
        reply.finish(Ok(()));
        assert_eq!(pending[0].try_take(), Some(Ok(())));
        let mut accepted = handle.request_redraw();
        assert!(accepted.try_take().is_none());
        drop(rx);
        assert_eq!(accepted.try_take(), Some(Err(WindowError::HostStopped)));
    }
    #[test]
    fn worker_control_is_queued_and_completed_by_host() {
        let (service, rx) = WindowService::channel(Arc::new(|| {}));
        let handle = service.handle(WindowId(7));
        let worker = std::thread::spawn(move || handle.set_title("worker").wait());
        let Request::Control(id, _, Control::Command(WindowCommand::SetTitle { title, .. }), reply) =
            rx.recv().unwrap()
        else {
            panic!("unexpected request")
        };
        assert_eq!(id, WindowId(7));
        assert_eq!(title, "worker");
        reply.finish(Ok(()));
        assert_eq!(worker.join().unwrap(), Ok(()));
    }
    #[test]
    fn displays_and_targeted_fullscreen_are_answered_by_the_host() {
        let (service, rx) = WindowService::channel(Arc::new(|| {}));
        let target = FullscreenRequest {
            display: Some(nana_ui_platform::DisplayId(9)),
            ..Default::default()
        };
        let mut fullscreen = service.handle(WindowId(2)).set_fullscreen(Some(target));
        let Request::Control(
            _,
            _,
            Control::Command(WindowCommand::SetFullscreen {
                fullscreen: Some(queued),
                ..
            }),
            reply,
        ) = rx.try_recv().unwrap()
        else {
            panic!("unexpected request")
        };
        assert_eq!(queued, target);
        reply.finish(Err(WindowError::InvalidParameter(
            "display is not connected".into(),
        )));
        assert!(matches!(
            fullscreen.try_take(),
            Some(Err(WindowError::InvalidParameter(_)))
        ));
        let mut displays = service.displays();
        let Request::Displays(reply) = rx.try_recv().unwrap() else {
            panic!("unexpected request")
        };
        reply.finish(Ok(Vec::new()));
        assert_eq!(displays.try_take(), Some(Ok(Vec::new())));
        drop(rx);
        assert_eq!(
            service.displays().try_take(),
            Some(Err(WindowError::HostStopped))
        );
    }
    #[test]
    fn stopping_host_resolves_pending_and_future_requests() {
        let (service, rx) = WindowService::channel(Arc::new(|| {}));
        let mut pending = service.handle(WindowId(1)).close();
        drop(rx);
        assert_eq!(pending.try_take(), Some(Err(WindowError::HostStopped)));
        let mut later = service.handle(WindowId(1)).close();
        assert_eq!(later.try_take(), Some(Err(WindowError::HostStopped)));
    }
    #[test]
    fn reopening_backend_identity_does_not_revive_old_handles() {
        let (service, _rx) = WindowService::channel(Arc::new(|| {}));
        let id = WindowId(3);
        service.register(id);
        let old = service.handle(id);
        service.unregister(id);
        service.register(id);
        let new = service.handle(id);
        assert!(!service.is_current(id, old.generation));
        assert!(service.is_current(id, new.generation));
    }
    #[test]
    fn future_is_woken_on_host_shutdown() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Wake(AtomicBool);
        impl std::task::Wake for Wake {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let wake = Arc::new(Wake(AtomicBool::new(false)));
        let waker = Waker::from(wake.clone());
        let mut context = Context::from_waker(&waker);
        let (service, rx) = WindowService::channel(Arc::new(|| {}));
        let mut request = service.handle(WindowId(3)).close();
        assert!(Pin::new(&mut request).poll(&mut context).is_pending());
        drop(rx);
        assert!(wake.0.load(Ordering::SeqCst));
        assert_eq!(
            Pin::new(&mut request).poll(&mut context),
            Poll::Ready(Err(WindowError::HostStopped))
        );
    }
    #[test]
    fn host_thread_cannot_deadlock_waiting_for_itself() {
        let (service, _rx) = WindowService::channel(Arc::new(|| {}));
        assert_eq!(
            service.handle(WindowId(1)).close().wait(),
            Err(WindowError::HostThreadWait)
        );
    }
}
