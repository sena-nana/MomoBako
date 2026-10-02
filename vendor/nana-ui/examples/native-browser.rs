//! Run with `--features hosted,bundled-fonts`. An optional URL opens in the content pane.
use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{
    Activate, BrowserView, Button, DocumentId, Entity, RuntimeDocument, Stack, Text,
};
use nana_ui::{
    BrowserCommand, BrowserEvent, BrowserPolicy, DocumentAccessError, NativeBrowserEvent,
    NativeBrowserRequest, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate, ThemeMode,
    WindowDescriptor, run_runtime,
};
use nana_ui_platform::WindowId;
use std::io::Write;

#[derive(Clone)]
enum Message {
    Command(BrowserCommand),
    Toggle,
    CaptureAndClose,
}

struct BrowserExample {
    document: RuntimeDocument,
    view: Entity<BrowserView>,
    status: Entity<Text>,
    revision: u64,
    command: BrowserCommand,
    visible: bool,
    restore_url: String,
    close_after_present: bool,
    capture_completed: bool,
    presented: u64,
}

impl RuntimeProgram for BrowserExample {
    type Message = Message;
    type Error = String;

    fn initialize(
        _: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), String> {
        let id = DocumentId::new(1).unwrap();
        let first_url = std::env::var("NANA_BROWSER_FIRST_URL")
            .unwrap_or_else(|_| "https://example.com".into());
        let second_url = std::env::var("NANA_BROWSER_SECOND_URL")
            .unwrap_or_else(|_| "https://www.iana.org/domains/reserved".into());
        let mut document = RuntimeDocument::new(id);
        document
            .context_mut()
            .set_theme(ThemeMode::Light)
            .map_err(|error| error.to_string())?;
        let (_, (view, status)) = document
            .context_mut()
            .mount_view_root(id, || {
                let status = entity_ref::<Text>();
                let view = entity_ref::<BrowserView>();
                let toolbar = widget(Stack::bar(8.0)).with(|toolbar| {
                    for (label, command) in [
                        (
                            "示例页",
                            Message::Command(BrowserCommand::Navigate(first_url.clone())),
                        ),
                        (
                            "说明页",
                            Message::Command(BrowserCommand::Navigate(second_url.clone())),
                        ),
                        ("后退", Message::Command(BrowserCommand::Back)),
                        ("前进", Message::Command(BrowserCommand::Forward)),
                        ("重新加载", Message::Command(BrowserCommand::Reload)),
                        ("停止", Message::Command(BrowserCommand::Stop)),
                        ("显示／隐藏", Message::Toggle),
                        ("聚焦网页", Message::Command(BrowserCommand::Focus)),
                        ("截图", Message::Command(BrowserCommand::Capture)),
                        ("截图并关闭", Message::CaptureAndClose),
                    ] {
                        toolbar.add(widget(Button::new(label)).on_cx(
                            move |_, _: &Activate, cx| cx.dispatch_program(command.clone()),
                        ));
                    }
                });
                let root = widget(Stack::fill_column(8.0).padding(12.0)).children((
                    toolbar,
                    widget(Text::new("正在打开")).entity_ref(status),
                    widget(BrowserView::new("main")).entity_ref(view),
                ));
                with_refs(root, (view, status))
            })
            .map_err(|error| error.to_string())?;
        let url = std::env::args().nth(1).unwrap_or(first_url);
        Ok((
            Self {
                document,
                view,
                status,
                revision: 1,
                restore_url: url.clone(),
                command: BrowserCommand::Navigate(url),
                visible: true,
                close_after_present: false,
                capture_completed: false,
                presented: 0,
            },
            Vec::new(),
        ))
    }
    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY).then(|| f(&self.document)))
    }
    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY).then(|| f(&mut self.document)))
    }
    fn update(
        &mut self,
        message: Self::Message,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::Command(command) => {
                self.command = command;
                self.revision += 1;
                self.capture_completed = false;
            }
            Message::Toggle => self.visible = !self.visible,
            Message::CaptureAndClose => {
                self.command = BrowserCommand::Capture;
                self.revision += 1;
                self.capture_completed = false;
                self.close_after_present = true;
            }
        }
        report(format_args!(
            "command revision={} visible={} {:?}",
            self.revision, self.visible, self.command
        ));
        RuntimeProgramUpdate::redraw(WindowId::PRIMARY)
    }
    fn native_browser_requests(&self, id: WindowId) -> Vec<NativeBrowserRequest> {
        if id != WindowId::PRIMARY {
            return Vec::new();
        }
        vec![NativeBrowserRequest {
            id: "main".into(),
            node: self.view.stable_id(),
            policy: BrowserPolicy { allow_web: true },
            restore_url: self.restore_url.clone(),
            visible: self.visible,
            revision: self.revision,
            command: Some(self.command.clone()),
        }]
    }
    fn native_browser_event(
        &mut self,
        _: WindowId,
        event: NativeBrowserEvent,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        report(format_args!(
            "event revision={} {:?}",
            event.revision,
            match &event.event {
                BrowserEvent::State(state) => format!("state {state:?}"),
                BrowserEvent::Captured(bytes) => format!("captured bytes={}", bytes.len()),
                BrowserEvent::CaptureFailed(error) => format!("capture-failed {error}"),
            }
        ));
        if let BrowserEvent::State(state) = &event.event
            && !state.url.is_empty()
        {
            self.restore_url = state.url.clone();
        }
        let value = match event.event {
            BrowserEvent::State(state) => format!(
                "{} · {}{}",
                state.title,
                state.url,
                state
                    .error
                    .map(|error| format!(" · {error}"))
                    .unwrap_or_default()
            ),
            BrowserEvent::Captured(bytes) => {
                self.capture_completed = true;
                if let Some(path) = std::env::var_os("NANA_BROWSER_CAPTURE_OUTPUT") {
                    match std::fs::write(path, bytes) {
                        Ok(()) => "截图已保存".into(),
                        Err(error) => error.to_string(),
                    }
                } else {
                    "已截取网页".into()
                }
            }
            BrowserEvent::CaptureFailed(error) => error,
        };
        eprintln!("{value}");
        let _ = self
            .document
            .context_mut()
            .update_component(self.status, |text, _| text.value = value);
        RuntimeProgramUpdate::redraw(WindowId::PRIMARY)
    }
    fn theme_mode(&self) -> ThemeMode {
        ThemeMode::Light
    }
    fn window_frame_presented(
        &mut self,
        window: WindowId,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        self.presented += 1;
        if self.presented == 1 || self.close_after_present {
            report(format_args!(
                "presented window={} count={} revision={}",
                window.0, self.presented, self.revision
            ));
        }
        if self.close_after_present {
            report(format_args!(
                "close-after-capture revision={} completed={}",
                self.revision, self.capture_completed
            ));
            return RuntimeProgramUpdate::exit();
        }
        RuntimeProgramUpdate::default()
    }
}

fn report(message: std::fmt::Arguments<'_>) {
    eprintln!("{message}");
    if let Some(path) = std::env::var_os("NANA_BROWSER_EVENT_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    run_runtime::<BrowserExample>(
        WindowDescriptor::new("NanaUI Browser")
            .initial_size(1200.0, 760.0)
            .system_caption(true),
    )
}
