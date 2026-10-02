use std::convert::Infallible;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{
    Activate, Button, DocumentId, Entity, FrameworkError, GpuTextureView, List, RuntimeDocument,
    Text,
};
use nana_ui::{
    ButtonKind, CopyOutcome, DEFAULT_CAPACITY, FrameBinding, FrameExchange, FrameExchangeStats,
    FrameInbox, GpuContext, HostTextureAlphaMode, HostTextureRegistry, HostedRunError, RoutedInput,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate, ThemeMode, WindowDescriptor,
    WindowHandle, run_runtime,
};
use nana_ui_platform::{WindowEvent, WindowId};

use crate::panel::{DemoPanel, Message};
use crate::performance::StartupProbe;
use crate::scene::SharedScene;

const PREVIEW_SLOT: &str = "preview";
const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

static STARTED_AT: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy)]
struct ProducerFrame {
    size: (u32, u32),
    background: nana_ui::Color,
    accent: nana_ui::Color,
    revision: u32,
}

struct PreviewProducer {
    inbox: FrameInbox<u64>,
    stats: Arc<Mutex<FrameExchangeStats>>,
    commands: mpsc::Sender<ProducerFrame>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl PreviewProducer {
    fn spawn(
        gpu: &GpuContext,
        window: WindowHandle,
        initial: ProducerFrame,
        continuous: bool,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(FrameExchangeStats::default()));
        let notify = {
            let window = window.clone();
            Arc::new(move || {
                drop(window.request_redraw());
            })
        };
        let mut exchange = FrameExchange::new(gpu, DEFAULT_CAPACITY, 0, notify);
        let inbox = exchange.inbox();
        let gpu = gpu.clone();
        let stop_thread = Arc::clone(&stop);
        let stats_thread = Arc::clone(&stats);
        let join = thread::Builder::new()
            .name("hosted-gpu-demo-producer".into())
            .spawn(move || {
                let mut frame = initial;
                let device = gpu.wgpu().device();
                let mut scene = SharedScene::new(
                    device,
                    gpu.wgpu().queue(),
                    SURFACE_FORMAT,
                    [frame.background, frame.accent],
                    frame.revision,
                    frame.size,
                );
                let mut dirty = true;
                while !stop_thread.load(Ordering::Acquire) {
                    while let Ok(next) = rx.try_recv() {
                        frame = next;
                        dirty = true;
                    }
                    if dirty || continuous {
                        scene.resize(device, SURFACE_FORMAT, frame.size.0, frame.size.1);
                        {
                            // Raw queue writes off the window thread hold the
                            // submission guard, like every submit does.
                            let _submission = gpu.wgpu().lock_submission();
                            scene.update(
                                gpu.wgpu().queue(),
                                frame.background,
                                frame.accent,
                                frame.revision,
                            );
                        }
                        let mut producer = gpu.begin_frame("hosted gpu demo producer");
                        scene.render(producer.wgpu_encoder());
                        producer.submit();
                        match exchange.copy_from_wgpu(scene.texture(), 0) {
                            CopyOutcome::Submitted | CopyOutcome::PoolFull => {}
                            CopyOutcome::EmptySource
                            | CopyOutcome::IncompatibleSource
                            | CopyOutcome::DeviceMismatch => {}
                        }
                        dirty = false;
                    }
                    exchange.poll();
                    if let Ok(mut published) = stats_thread.lock() {
                        *published = exchange.stats();
                    }
                    match rx.recv_timeout(Duration::from_millis(8)) {
                        Ok(next) => {
                            frame = next;
                            dirty = true;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("hosted gpu demo producer");
        Self {
            inbox,
            stats,
            commands,
            stop,
            join: Some(join),
        }
    }

    fn submit(&self, frame: ProducerFrame) {
        let _ = self.commands.send(frame);
    }

    fn stats(&self) -> FrameExchangeStats {
        self.stats.lock().map(|stats| *stats).unwrap_or_default()
    }
}

impl Drop for PreviewProducer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn run(started_at: Instant) -> Result<(), HostedRunError> {
    let _ = STARTED_AT.set(started_at);
    let mut settings = WindowDescriptor::new("NanaUI Hosted GPU Demo")
        .initial_size(1100.0, 720.0)
        .minimum_size(760.0, 520.0)
        .system_caption(true);
    settings.transparent = true;
    run_runtime::<DemoProgram>(settings)
}

struct DemoProgram {
    panel: DemoPanel,
    producer: PreviewProducer,
    binding: FrameBinding<u64>,
    document: RuntimeDocument,
    version: Entity<Text>,
    theme_button: Entity<Button>,
    textures: HostTextureRegistry,
    startup: StartupProbe,
    size: (u32, u32),
    stats_printed: bool,
}

impl DemoProgram {
    fn mount(
        context: &RuntimeProgramContext<Message>,
        panel: DemoPanel,
        size: (u32, u32),
        startup: StartupProbe,
    ) -> Result<Self, FrameworkError> {
        let document_id = DocumentId::new(1).expect("hosted gpu document");
        let mut document = RuntimeDocument::new(document_id);
        let (_, (version, theme_button)) =
            document.context_mut().mount_view_root(document_id, || {
                let version = entity_ref::<Text>();
                let theme_button = entity_ref::<Button>();
                let root = widget(List::new().label("Hosted GPU")).children((
                    widget(Text::new("NANA 实时预览")),
                    widget(Button::new(panel.theme_label()).kind(ButtonKind::Text))
                        .entity_ref(theme_button)
                        .on_cx(move |_button, _event: &Activate, cx| {
                            cx.dispatch_program(Message::ToggleTheme);
                        }),
                    widget(GpuTextureView::new(PREVIEW_SLOT)),
                    widget(Text::new(panel.version_label())).entity_ref(version),
                    widget(Button::new("刷新预览").kind(ButtonKind::Primary)).on_cx(
                        move |_button, _event: &Activate, cx| {
                            cx.dispatch_program(Message::Refresh);
                        },
                    ),
                ));
                with_refs(root, (version, theme_button))
            })?;

        let textures = HostTextureRegistry::new();
        let binding = FrameBinding::new(
            context.gpu(),
            textures.slot(PREVIEW_SLOT),
            HostTextureAlphaMode::Opaque,
        );
        let producer = PreviewProducer::spawn(
            context.gpu(),
            context.window(),
            Self::frame(&panel, size),
            startup.continuous_preview(),
        );
        Ok(Self {
            panel,
            producer,
            binding,
            document,
            version,
            theme_button,
            textures,
            startup,
            size,
            stats_printed: false,
        })
    }

    fn frame(panel: &DemoPanel, size: (u32, u32)) -> ProducerFrame {
        let colors = panel.palette();
        ProducerFrame {
            size,
            background: colors.background,
            accent: colors.accent_strong,
            revision: panel.revision(),
        }
    }

    fn apply(&mut self, message: Message) {
        self.panel.update(message);
        self.producer.submit(Self::frame(&self.panel, self.size));
        let _ = self
            .document
            .context_mut()
            .update_component(self.version, |text, _| {
                text.value = self.panel.version_label();
            });
        let _ = self
            .document
            .context_mut()
            .update_component(self.theme_button, |button, _| {
                button.label = self.panel.theme_label().to_owned();
            });
    }
}

impl RuntimeProgram for DemoProgram {
    type Message = Message;
    type Error = Infallible;

    fn initialize(
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(Self, Vec<Self::Message>), Self::Error> {
        let panel = DemoPanel::default();
        let size = context.geometry().physical_size;
        let started_at = STARTED_AT.get().copied().unwrap_or_else(Instant::now);
        let program = Self::mount(context, panel, size, StartupProbe::new(started_at))
            .expect("hosted gpu document");
        Ok((program, Vec::new()))
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

    fn frame_demand(&self, _id: WindowId) -> nana_ui::FrameDemand {
        self.startup.demand()
    }

    fn theme_mode(&self) -> ThemeMode {
        self.panel.theme_mode()
    }

    fn host_textures(&self, _id: WindowId) -> Option<HostTextureRegistry> {
        Some(self.textures.clone())
    }

    fn prepare_window_frame(
        &mut self,
        _id: WindowId,
        _context: &RuntimeProgramContext<Self::Message>,
    ) {
        self.binding.prepare(Some(&self.producer.inbox), |_| true);
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        _context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready { geometry, .. } | WindowEvent::Resized { geometry, .. } => {
                self.size = geometry.physical_size;
                self.producer.submit(Self::frame(&self.panel, self.size));
                RuntimeProgramUpdate::redraw_all()
            }
            WindowEvent::CloseRequested { .. } => RuntimeProgramUpdate::exit(),
            _ => RuntimeProgramUpdate::default(),
        }
    }

    fn rebuild_gpu(&mut self, context: &RuntimeProgramContext<Self::Message>) {
        self.binding = FrameBinding::new(
            context.gpu(),
            self.textures.slot(PREVIEW_SLOT),
            HostTextureAlphaMode::Opaque,
        );
        self.producer = PreviewProducer::spawn(
            context.gpu(),
            context.window(),
            Self::frame(&self.panel, self.size),
            self.startup.continuous_preview(),
        );
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

    fn window_frame_presented(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let stats = self.producer.stats();
        if !self.stats_printed && self.binding.token().is_some() {
            self.stats_printed = true;
            println!(
                "frame_exchange submitted={} published={} superseded={} pool_full={} occupied={}/{}",
                stats.submitted,
                stats.published,
                stats.superseded,
                stats.pool_full,
                stats.occupied,
                stats.occupied_high_water
            );
        }
        if self.startup.record_frame(context, Some(stats)) {
            return RuntimeProgramUpdate::exit();
        }
        if self.binding.presented(Some(&self.producer.inbox), |_| true) {
            RuntimeProgramUpdate::redraw(id)
        } else {
            RuntimeProgramUpdate::default()
        }
    }
}
