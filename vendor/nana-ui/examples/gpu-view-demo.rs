use std::convert::Infallible;

use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{
    Activate, Button, DocumentId, Entity, FrameworkError, GpuView, GpuViewMode, GpuViewPalette,
    List, RuntimeDocument, Text,
};
use nana_ui::{
    ButtonKind, RoutedInput, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
    ThemeMode, WindowDescriptor, run_runtime,
};
use nana_ui_platform::{WindowEvent, WindowId};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Message {
    Refresh,
    ToggleTheme,
}

struct GpuViewDemo {
    theme: ThemeMode,
    revision: u32,
    document: RuntimeDocument,
    preview: Entity<GpuView>,
    thumbnail: Entity<GpuView>,
    version: Entity<Text>,
    theme_button: Entity<Button>,
}

impl GpuViewDemo {
    fn palette(theme: ThemeMode, accent_strong: bool) -> GpuViewPalette {
        let colors = theme.palette();
        let accent = if accent_strong {
            colors.accent_strong
        } else {
            colors.accent
        };
        GpuViewPalette {
            background: [
                colors.background.r,
                colors.background.g,
                colors.background.b,
                colors.background.a,
            ],
            accent: [accent.r, accent.g, accent.b, accent.a],
        }
    }

    fn mount(theme: ThemeMode, revision: u32) -> Result<Self, FrameworkError> {
        let document_id = DocumentId::new(1).expect("gpu view document");
        let mut document = RuntimeDocument::new(document_id);
        let (_, (preview, thumbnail, version, theme_button)) =
            document.context_mut().mount_view_root(document_id, || {
                let preview = entity_ref::<GpuView>();
                let thumbnail = entity_ref::<GpuView>();
                let version = entity_ref::<Text>();
                let theme_button = entity_ref::<Button>();
                let root = widget(List::new().label("GPU View")).children((
                    widget(Text::new("NANA 实时预览")),
                    widget(Button::new(theme_label(theme)).kind(ButtonKind::Text))
                        .entity_ref(theme_button)
                        .on_cx(move |_button, _event: &Activate, cx| {
                            cx.dispatch_program(Message::ToggleTheme);
                        }),
                    widget(
                        GpuView::new(1)
                            .mode(GpuViewMode::Standalone)
                            .palette(Self::palette(theme, true))
                            .seed(revision as f32),
                    )
                    .entity_ref(preview),
                    widget(Text::new(format!("版本 {}", revision + 1))).entity_ref(version),
                    widget(
                        GpuView::new(2)
                            .mode(GpuViewMode::Inline)
                            .palette(Self::palette(theme, false))
                            .seed((revision.saturating_add(2)) as f32),
                    )
                    .entity_ref(thumbnail),
                    widget(Button::new("刷新预览").kind(ButtonKind::Primary)).on_cx(
                        move |_button, _event: &Activate, cx| {
                            cx.dispatch_program(Message::Refresh);
                        },
                    ),
                ));
                with_refs(root, (preview, thumbnail, version, theme_button))
            })?;

        Ok(Self {
            theme,
            revision,
            document,
            preview,
            thumbnail,
            version,
            theme_button,
        })
    }

    fn apply(&mut self, message: Message) {
        match message {
            Message::Refresh => self.revision = self.revision.saturating_add(1),
            Message::ToggleTheme => self.theme = self.theme.toggle(),
        }
        let theme = self.theme;
        let revision = self.revision;
        let _ = self
            .document
            .context_mut()
            .update_component(self.version, |text, _| {
                text.value = format!("版本 {}", revision + 1);
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
            .update_component(self.preview, |view, _| {
                view.palette = Self::palette(theme, true);
                view.seed = revision as f32;
                view.invalidate_content();
            });
        let _ = self
            .document
            .context_mut()
            .update_component(self.thumbnail, |view, _| {
                view.palette = Self::palette(theme, false);
                view.seed = (revision.saturating_add(2)) as f32;
                view.invalidate_content();
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

impl RuntimeProgram for GpuViewDemo {
    type Message = Message;
    type Error = Infallible;

    fn initialize(
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), Self::Error> {
        Ok((
            Self::mount(ThemeMode::Dark, 0).expect("gpu view document"),
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

    fn scene_gpu_renderers(&self, _id: WindowId) -> Option<nana_ui::SceneGpuRendererRegistry> {
        Some(nana_ui::default_scene_gpu_renderers())
    }

    fn theme_mode(&self) -> ThemeMode {
        self.theme
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match event {
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
        Ok(RuntimeProgramUpdate::redraw(id))
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    run_runtime::<GpuViewDemo>(
        WindowDescriptor::new("NanaUI GPU View Demo")
            .initial_size(1100.0, 720.0)
            .minimum_size(760.0, 520.0)
            .system_caption(true),
    )
}
