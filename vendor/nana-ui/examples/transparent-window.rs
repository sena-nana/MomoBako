use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{
    Activate, Button, DocumentId, Entity, FrameworkError, List, RuntimeDocument, Text,
};
use nana_ui::{
    ButtonKind, RoutedInput, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
    ThemeMode, WindowDescriptor, run_runtime,
};
use nana_ui_platform::{WindowEvent, WindowId};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Message {
    ToggleTheme,
    TogglePanel,
}

struct TransparentWindow {
    theme: ThemeMode,
    panel_visible: bool,
    document: RuntimeDocument,
    status: Entity<Text>,
    theme_button: Entity<Button>,
    panel_button: Entity<Button>,
    toggle_theme: Arc<AtomicBool>,
    toggle_panel: Arc<AtomicBool>,
}

impl TransparentWindow {
    fn mount(theme: ThemeMode, panel_visible: bool) -> Result<Self, FrameworkError> {
        let toggle_theme = Arc::new(AtomicBool::new(false));
        let toggle_panel = Arc::new(AtomicBool::new(false));
        let document_id = DocumentId::new(1).expect("transparent window document");
        let mut document = RuntimeDocument::new(document_id);
        let pending_theme = Arc::clone(&toggle_theme);
        let pending_panel = Arc::clone(&toggle_panel);
        let (_, (theme_button, panel_button, status)) =
            document.context_mut().mount_view_root(document_id, || {
                let theme_button = entity_ref::<Button>();
                let panel_button = entity_ref::<Button>();
                let status = entity_ref::<Text>();
                let root = widget(List::new().label("Transparent window")).children((
                    widget(Text::new("NANA 透明窗口预览")),
                    widget(Button::new(theme_label(theme)).kind(ButtonKind::Text))
                        .entity_ref(theme_button)
                        .on(move |_: &Activate| {
                            pending_theme.store(true, Ordering::SeqCst);
                        }),
                    widget(Text::new(status_copy(panel_visible))).entity_ref(status),
                    widget(Button::new(panel_label(panel_visible)).kind(ButtonKind::Primary))
                        .entity_ref(panel_button)
                        .on(move |_: &Activate| {
                            pending_panel.store(true, Ordering::SeqCst);
                        }),
                ));
                with_refs(root, (theme_button, panel_button, status))
            })?;

        Ok(Self {
            theme,
            panel_visible,
            document,
            status,
            theme_button,
            panel_button,
            toggle_theme,
            toggle_panel,
        })
    }

    fn apply(&mut self, message: Message) {
        match message {
            Message::ToggleTheme => self.theme = self.theme.toggle(),
            Message::TogglePanel => self.panel_visible = !self.panel_visible,
        }
        let theme = self.theme;
        let panel_visible = self.panel_visible;
        let _ = self
            .document
            .context_mut()
            .update_component(self.status, |text, _| {
                text.value = status_copy(panel_visible).to_owned();
            });
        let _ = self
            .document
            .context_mut()
            .update_component(self.theme_button, |button, _| {
                button.label = theme_label(theme).to_owned();
            });
        let _ = self
            .document
            .context_mut()
            .update_component(self.panel_button, |button, _| {
                button.label = panel_label(panel_visible).to_owned();
            });
    }
}

fn theme_label(theme: ThemeMode) -> &'static str {
    if theme == ThemeMode::Dark {
        "浅色"
    } else {
        "深色"
    }
}

fn panel_label(visible: bool) -> &'static str {
    if visible {
        "隐藏面板"
    } else {
        "显示面板"
    }
}

fn status_copy(visible: bool) -> &'static str {
    if visible {
        "内容保持清晰可见"
    } else {
        "面板已隐藏"
    }
}

impl RuntimeProgram for TransparentWindow {
    type Message = Message;
    type Error = Infallible;

    fn initialize(
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), Self::Error> {
        Ok((
            Self::mount(ThemeMode::Dark, true).expect("transparent window document"),
            Vec::new(),
        ))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, nana_ui::DocumentAccessError> {
        let document = { (id == WindowId::PRIMARY).then_some(&self.document) };
        Ok(document.map(f))
    }

    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, nana_ui::DocumentAccessError> {
        let document = { (id == WindowId::PRIMARY).then_some(&mut self.document) };
        Ok(document.map(f))
    }

    fn update(
        &mut self,
        message: Self::Message,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        self.apply(message);
        RuntimeProgramUpdate::redraw_all()
    }

    fn theme_mode(&self) -> ThemeMode {
        self.theme
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready { id, .. } => {
                eprintln!(
                    "nana window ready id={} material={:?} alpha={:?}",
                    id.0,
                    context.material(),
                    context.surface_alpha_mode()
                );
                RuntimeProgramUpdate::default()
            }
            WindowEvent::CloseRequested { .. } => RuntimeProgramUpdate::exit(),
            _ => RuntimeProgramUpdate::default(),
        }
    }

    fn input_event(
        &mut self,
        id: WindowId,
        input: RoutedInput<'_>,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        let _event = input.event;
        let mut changed = false;
        if self.toggle_theme.swap(false, Ordering::SeqCst) {
            self.apply(Message::ToggleTheme);
            changed = true;
        }
        if self.toggle_panel.swap(false, Ordering::SeqCst) {
            self.apply(Message::TogglePanel);
            changed = true;
        }
        Ok(if changed {
            RuntimeProgramUpdate::redraw_all()
        } else {
            RuntimeProgramUpdate::redraw(id)
        })
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    let mut settings = WindowDescriptor::new("NanaUI Transparent Window Demo")
        .initial_size(920.0, 620.0)
        .minimum_size(640.0, 420.0)
        .system_caption(true);
    settings.transparent = true;
    run_runtime::<TransparentWindow>(settings)
}
