use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nana_ui::runtime::view::widget;
use nana_ui::runtime::{Activate, Button, DocumentId, FrameworkError, List, RuntimeDocument, Text};
use nana_ui::{
    RoutedInput, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate, RuntimeRedraw,
    ThemeMode, WindowDescriptor, run_runtime,
};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowEvent, WindowId, WindowRole};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Message {
    OpenWindow,
}

struct Smoke {
    windows: BTreeMap<WindowId, SmokeWindow>,
    next_window: u64,
    open: Arc<AtomicBool>,
}

struct SmokeWindow {
    document: RuntimeDocument,
}

impl Smoke {
    fn open_settings(number: usize) -> WindowDescriptor {
        let offset = 64.0 * number.saturating_sub(1) as f64;
        WindowDescriptor {
            title: format!("NanaUI Window {number}"),
            initial_size: (640.0, 420.0),
            minimum_size: (480.0, 320.0),
            initial_position: Some((120.0 + offset, 120.0 + offset)),
            role: if number == 1 {
                WindowRole::Main
            } else {
                WindowRole::Tool
            },
            parent: (number > 1).then_some(WindowId::PRIMARY),
            system_caption: true,
            ..WindowDescriptor::default()
        }
    }

    fn mount_window(
        id: WindowId,
        number: usize,
        open: &Arc<AtomicBool>,
    ) -> Result<SmokeWindow, FrameworkError> {
        let document_id = DocumentId::new(id.0 + 1).expect("window document");
        let mut document = RuntimeDocument::new(document_id);
        let pending = Arc::clone(open);
        document.context_mut().mount_view_root(document_id, || {
            widget(List::new().label("Window chrome")).children((
                widget(Text::new("NANA NanaUI Window")),
                widget(Text::new(format!("窗口 {number}"))),
                widget(Button::new("新建窗口")).on(move |_: &Activate| {
                    pending.store(true, Ordering::SeqCst);
                }),
            ))
        })?;
        Ok(SmokeWindow { document })
    }

    fn open_next(&mut self) -> RuntimeProgramUpdate {
        let number = self.next_window as usize;
        self.next_window = self.next_window.saturating_add(1);
        let id = if number == 1 {
            WindowId::PRIMARY
        } else {
            WindowId(number as u64)
        };
        let window = Self::mount_window(id, number, &self.open).expect("window document");
        self.windows.insert(id, window);
        if id == WindowId::PRIMARY {
            RuntimeProgramUpdate::redraw(id)
        } else {
            RuntimeProgramUpdate {
                redraw: RuntimeRedraw::All,
                window_commands: vec![WindowCommand::Open {
                    id,
                    settings: Self::open_settings(number),
                }],
                exit: false,
            }
        }
    }
}

impl RuntimeProgram for Smoke {
    type Message = Message;
    type Error = Infallible;

    fn initialize(
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), Self::Error> {
        let mut smoke = Self {
            windows: BTreeMap::new(),
            next_window: 1,
            open: Arc::new(AtomicBool::new(false)),
        };
        let _ = smoke.open_next();
        Ok((smoke, vec![Message::OpenWindow]))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, nana_ui::DocumentAccessError> {
        let document = { self.windows.get(&id).map(|window| &window.document) };
        Ok(document.map(f))
    }

    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, nana_ui::DocumentAccessError> {
        let document = { self.windows.get_mut(&id).map(|window| &mut window.document) };
        Ok(document.map(f))
    }

    fn update(
        &mut self,
        message: Self::Message,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::OpenWindow => self.open_next(),
        }
    }

    fn theme_mode(&self) -> ThemeMode {
        ThemeMode::Dark
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::CloseRequested { id } if id == WindowId::PRIMARY => {
                RuntimeProgramUpdate::exit()
            }
            WindowEvent::CloseRequested { id } => RuntimeProgramUpdate {
                redraw: RuntimeRedraw::All,
                window_commands: vec![WindowCommand::Close(id)],
                exit: false,
            },
            WindowEvent::Closed { id } => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    RuntimeProgramUpdate::exit()
                } else {
                    RuntimeProgramUpdate::default()
                }
            }
            _ => RuntimeProgramUpdate::default(),
        }
    }

    fn input_event(
        &mut self,
        _id: WindowId,
        input: RoutedInput<'_>,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        let _event = input.event;
        if self.open.swap(false, Ordering::SeqCst) {
            Ok(self.open_next())
        } else {
            Ok(RuntimeProgramUpdate::default())
        }
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    run_runtime::<Smoke>(
        WindowDescriptor::new("NanaUI Window 1")
            .initial_size(640.0, 420.0)
            .minimum_size(480.0, 320.0)
            .system_caption(true),
    )
}
