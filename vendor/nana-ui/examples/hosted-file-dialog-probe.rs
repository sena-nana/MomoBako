//! Native acceptance: stdin accepts file, files, folder, folders, save,
//! duplicate, busy, and quit. The frame counter keeps updating behind a sheet.
//! Completions and actual presentations are printed for verification.
use nana_ui::runtime::{
    Activate, Button, DocumentId, Entity, FrameworkError, RuntimeDocument, Stack, Text,
};
use nana_ui::{
    DocumentAccessError, FileDialogKind, FileDialogRequest, FileFilter, RuntimeProgram,
    RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor, run_runtime,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowEvent, WindowId};
use std::io::{self, BufRead, Write};

#[derive(Clone)]
enum Message {
    Command(String),
    Tick,
}
struct Probe {
    document: RuntimeDocument,
    counter: Entity<Text>,
    ticks: u64,
    presented: u64,
    request_id: u64,
    active_request: Option<u64>,
    close_at: Option<u64>,
}
impl RuntimeProgram for Probe {
    type Message = Message;
    type Error = FrameworkError;
    fn initialize(
        context: &RuntimeProgramContext<Message>,
    ) -> Result<(Self, Vec<Message>), Self::Error> {
        let id = DocumentId::new(1).unwrap();
        let mut document = RuntimeDocument::new(id);
        let cx = document.context_mut();
        cx.set_theme(nana_ui::ThemeMode::Light)?;
        let root = cx.create_component(id, Stack::fill_column(16.0))?;
        let counter = cx.create_detached_component(id, Text::new("0"))?;
        cx.append_child(root, counter)?;
        for (label, command) in [
            ("选择文件", "file"),
            ("选择多个文件", "files"),
            ("选择目录", "folder"),
            ("选择多个目录", "folders"),
            ("保存文件", "save"),
            ("并发测试", "concurrent"),
            ("关闭测试", "close-test"),
        ] {
            let button = cx.create_detached_component(id, Button::new(label))?;
            cx.append_child(root, button)?;
            cx.on(button, move |_, _: &Activate, cx| {
                cx.dispatch_program_all(Message::Command(command.into()))
            })?;
        }
        let commands = context.clone();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                commands.dispatch(Message::Command(line));
            }
        });
        let ticker = context.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                ticker.dispatch(Message::Tick);
            }
        });
        Ok((
            Self {
                document,
                counter,
                ticks: 0,
                presented: 0,
                request_id: 0,
                active_request: None,
                close_at: None,
            },
            vec![],
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
        message: Message,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        let command = match message {
            Message::Tick => {
                self.ticks += 1;
                if self.close_at.is_some_and(|tick| self.ticks >= tick) {
                    return RuntimeProgramUpdate::exit();
                }
                let _ = self
                    .document
                    .context_mut()
                    .update_component(self.counter, |text, _| text.value = self.ticks.to_string());
                return RuntimeProgramUpdate::redraw_all();
            }
            Message::Command(command) => command,
        };
        let kind = match command.trim() {
            "quit" => return RuntimeProgramUpdate::exit(),
            "file" | "duplicate" | "busy" | "concurrent" | "close-test" => FileDialogKind::OpenFile,
            "files" => FileDialogKind::OpenFiles,
            "folder" => FileDialogKind::PickFolder,
            "folders" => FileDialogKind::PickFolders,
            "save" => FileDialogKind::SaveFile,
            _ => return RuntimeProgramUpdate::default(),
        };
        if command.trim() != "duplicate" {
            self.request_id += 1;
        }
        let id = if command.trim() == "duplicate" {
            self.active_request.unwrap_or(self.request_id)
        } else {
            self.request_id
        };
        self.active_request.get_or_insert(id);
        let request = FileDialogRequest::new(id, kind)
            .title("选择文件")
            .directory(
                std::env::var_os("NANA_FILE_DIALOG_DIRECTORY")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir),
            )
            .file_name("nana-dialog-selection.txt")
            .filters([FileFilter::new("文本", ["txt"])]);
        report(format_args!("request {id} {kind:?}"));
        let _ = io::stdout().flush();
        let mut requests = vec![request.clone()];
        if command.trim() == "concurrent" {
            requests.push(request.clone());
            self.request_id += 1;
            let mut busy = request;
            busy.id = self.request_id;
            requests.push(busy);
        }
        if command.trim() == "close-test" {
            self.close_at = Some(self.ticks + 30);
        }
        RuntimeProgramUpdate {
            window_commands: requests
                .into_iter()
                .map(|request| WindowCommand::OpenFileDialog {
                    id: WindowId::PRIMARY,
                    request,
                })
                .collect(),
            ..Default::default()
        }
    }
    fn theme_mode(&self) -> nana_ui::ThemeMode {
        nana_ui::ThemeMode::Light
    }
    fn window_event(
        &mut self,
        event: WindowEvent,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::FileDialogCompleted { id, result } => {
                report(format_args!("completed window={} result={result:?}", id.0));
                if self.active_request == Some(result.id) {
                    self.active_request = None;
                }
            }
            WindowEvent::FileDialogRejected {
                id,
                request_id,
                error,
            } => report(format_args!(
                "rejected window={} request={request_id} error={error:?}",
                id.0
            )),
            WindowEvent::CloseRequested { .. } => return RuntimeProgramUpdate::exit(),
            _ => return RuntimeProgramUpdate::default(),
        }
        let _ = io::stdout().flush();
        RuntimeProgramUpdate::default()
    }
    fn window_frame_presented(
        &mut self,
        _: WindowId,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        self.presented += 1;
        if self.presented.is_multiple_of(10) {
            report(format_args!(
                "presented {} ticks {}",
                self.presented, self.ticks
            ));
            let _ = io::stdout().flush();
        }
        RuntimeProgramUpdate::default()
    }
}
fn report(message: std::fmt::Arguments<'_>) {
    println!("{message}");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(
        std::env::var_os("NANA_FILE_DIALOG_EVENT_LOG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("nana-hosted-file-dialog-probe.log")),
    ) {
        let _ = writeln!(file, "{message}");
    }
}
fn main() -> Result<(), nana_ui::HostedRunError> {
    run_runtime::<Probe>(
        WindowDescriptor::new("NanaUI File Dialog Probe").initial_size(480.0, 580.0),
    )
}
