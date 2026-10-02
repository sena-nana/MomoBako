//! Native UI Automation probe. Stdin commands: show, hide, quit.
use nana_ui::runtime::{
    AccessibilityRole, AccessibilityState, CustomRenderNode, Entity, FrameworkError, GpuView,
    LayoutViewport, MeasureTextShaper, MutationQueue, NodeStyle, StableNodeId, Stack, Text,
    TextInput,
};
use nana_ui::{
    ApplicationState, ApplicationWindow, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, SceneResourceEncodeContext, SceneResourceProducer,
    SceneResourceProducerRegistry, WindowDescriptor, default_scene_gpu_renderers, run_runtime,
};
use nana_ui_core::{LayoutStyle, LengthSpec, VisibilitySpec};
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

enum Message {
    Visible(bool),
    Quit,
    RetryAdd,
    RetryEdit,
    RetryRecover,
    InitialRecover,
    CloseAuxiliary,
}

struct RetryProducer {
    phase: AtomicUsize,
    initial_recovery_requested: AtomicBool,
    context: RuntimeProgramContext<Message>,
}
impl std::fmt::Debug for RetryProducer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RetryProducer")
    }
}
impl SceneResourceProducer for RetryProducer {
    fn encode(
        &self,
        _: &CustomRenderNode,
        _: SceneResourceEncodeContext<'_>,
    ) -> Result<(), String> {
        let phase = self.phase.load(Ordering::SeqCst);
        let failed = match phase {
            1 | 101 => 1,
            2 | 102 => 2,
            4 | 104 => 4,
            _ => return Ok(()),
        };
        if self
            .phase
            .compare_exchange(failed, failed + 100, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            println!(
                "{}",
                serde_json::json!({"event":"producer_failed", "phase":failed})
            );
            io::stdout().flush().unwrap();
            if failed != 4 {
                self.context.dispatch(if failed == 1 {
                    Message::RetryEdit
                } else {
                    Message::RetryRecover
                });
            } else if self.initial_recovery_requested.load(Ordering::SeqCst) {
                self.context.dispatch(Message::InitialRecover);
            }
        }
        Err("injected resource encoding failure".into())
    }
    fn submitted(&self, _: &CustomRenderNode, _: &nana_ui::GpuSubmission) {
        println!(
            "{}",
            serde_json::json!({"event":"producer_submitted", "phase":self.phase.load(Ordering::SeqCst)})
        );
        io::stdout().flush().unwrap();
    }
}
struct Probe {
    target: WindowId,
    primary_editor: Option<StableNodeId>,
    primary_last: Option<String>,
    auxiliary_opened: bool,
    root: Option<StableNodeId>,
    container: Option<StableNodeId>,
    editor: Option<StableNodeId>,
    gpu: Option<StableNodeId>,
    style: NodeStyle,
    last: Option<(bool, String)>,
    retry: Option<Arc<RetryProducer>>,
    retry_started: bool,
    prepared_phase: Option<usize>,
    initial_failure: bool,
}
impl ApplicationState for Probe {
    type Message = Message;
    type Error = FrameworkError;
    fn initialize(context: &RuntimeProgramContext<Message>) -> Result<Self, Self::Error> {
        let initial_failure = std::env::args().any(|arg| arg == "--initial-failure");
        let target = if std::env::args().any(|arg| arg == "--auxiliary") {
            WindowId(1)
        } else {
            WindowId::PRIMARY
        };
        println!(
            "{}",
            serde_json::json!({"event":"adapter", "backend":format!("{:?}", context.gpu().capabilities().backend())})
        );
        let retry = std::env::args().any(|arg| arg == "--retry").then(|| {
            Arc::new(RetryProducer {
                phase: AtomicUsize::new(if initial_failure { 4 } else { 0 }),
                initial_recovery_requested: AtomicBool::new(false),
                context: context.clone(),
            })
        });
        let context = context.clone();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                eprintln!("probe command: {line:?}");
                let message = match line.trim_start_matches('\u{feff}').trim() {
                    "show" => Message::Visible(true),
                    "hide" => Message::Visible(false),
                    "quit" => Message::Quit,
                    "recover-initial" => Message::InitialRecover,
                    "close-auxiliary" => Message::CloseAuxiliary,
                    _ => continue,
                };
                context.dispatch(message);
            }
        });
        let mut layout = LayoutStyle::default();
        layout.paint.visibility = Some(VisibilitySpec::Hidden);
        Ok(Self {
            target,
            primary_editor: None,
            primary_last: None,
            auxiliary_opened: false,
            root: None,
            container: None,
            editor: None,
            gpu: None,
            style: NodeStyle {
                layout: Arc::new(layout),
                ..Default::default()
            },
            last: None,
            retry,
            retry_started: false,
            prepared_phase: None,
            initial_failure,
        })
    }
    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Message>,
    ) -> Result<(), Self::Error> {
        let document = window.document.document();
        let cx = window.document.context_mut();
        if context.window_id() != self.target {
            let editor = cx.create_component(
                document,
                TextInput::new("Primary value").label("Primary editor"),
            )?;
            self.primary_editor = Some(editor.stable_id());
            return Ok(());
        }
        let root = cx.create_component(document, Stack::column(0.0))?;
        let container =
            cx.create_component(document, Stack::column(0.0).style(self.style.clone()))?;
        let mut layout = LayoutStyle {
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(36.0)),
            color: Some([1.0, 1.0, 1.0, 1.0]),
            background: Some([0.1, 0.1, 0.1, 1.0]),
            ..Default::default()
        };
        layout.paint.visibility = Some(VisibilitySpec::Visible);
        let editor = cx.create_component(
            document,
            TextInput::new("Initial value")
                .label("Visible editor")
                .style(NodeStyle {
                    layout: Arc::new(layout),
                    ..Default::default()
                }),
        )?;
        let mut queue = MutationQueue::new();
        queue.insert(root.stable_id(), container.stable_id(), None);
        queue.insert(container.stable_id(), editor.stable_id(), None);
        queue.set_accessibility(
            container.stable_id(),
            AccessibilityState {
                role: AccessibilityRole::Dialog,
                label: Some("Container metadata".into()),
                ..Default::default()
            },
        );
        if let Some(producer) = &self.retry {
            let gpu = cx.create_component(
                document,
                GpuView::new(777).style(NodeStyle {
                    layout: Arc::new(LayoutStyle {
                        width: Some(LengthSpec::Px(16.0)),
                        height: Some(LengthSpec::Px(16.0)),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )?;
            queue.insert(root.stable_id(), gpu.stable_id(), None);
            self.gpu = Some(gpu.stable_id());
            let mut producers = SceneResourceProducerRegistry::new();
            producers.insert("777", producer.clone());
            window.producers = Some(producers);
            window.renderers = Some(default_scene_gpu_renderers());
        }
        cx.commit_mutations(queue)?;
        self.root = Some(root.stable_id());
        self.container = Some(container.stable_id());
        self.editor = Some(editor.stable_id());
        if self.initial_failure {
            // Exercise applications that prepare a document before attaching
            // it to a native host. This still is not a presented frame.
            window
                .document
                .flush(LayoutViewport::new(420.0, 180.0), &mut MeasureTextShaper)?;
        }
        Ok(())
    }
    fn update(
        &mut self,
        message: Message,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::CloseAuxiliary => RuntimeProgramUpdate {
                window_commands: vec![WindowCommand::Close(self.target)],
                ..Default::default()
            },
            Message::InitialRecover => {
                let retry = self.retry.as_ref().unwrap();
                retry
                    .initial_recovery_requested
                    .store(true, Ordering::SeqCst);
                if retry.phase.load(Ordering::SeqCst) == 104 {
                    retry.phase.store(0, Ordering::SeqCst);
                }
                RuntimeProgramUpdate::redraw(self.target)
            }
            Message::RetryAdd | Message::RetryEdit | Message::RetryRecover => {
                let window = windows.get_mut(&self.target).unwrap();
                let document = window.document.document();
                let cx = window.document.context_mut();
                let mut queue = MutationQueue::new();
                let phase = match message {
                    Message::RetryAdd => {
                        let added = cx
                            .create_component(document, Text::new("Recovered item"))
                            .unwrap();
                        queue.insert(self.root.unwrap(), added.stable_id(), None);
                        1
                    }
                    Message::RetryEdit => {
                        cx.update_component(
                            Entity::<TextInput>::from_stable_id(self.editor.unwrap()),
                            |editor, _| editor.label = Some("Updated editor".into()),
                        )
                        .unwrap();
                        2
                    }
                    _ => {
                        // A third, independent delta on the successful frame
                        // must not hide either unpresented transaction. An idle
                        // recovery could mask lost deltas via generation fallback.
                        queue.set_accessibility(
                            self.gpu.unwrap(),
                            AccessibilityState {
                                role: AccessibilityRole::Image,
                                label: Some("Recovered viewport".into()),
                                ..Default::default()
                            },
                        );
                        3
                    }
                };
                if phase != 2 {
                    cx.commit_mutations(queue).unwrap();
                }
                self.retry
                    .as_ref()
                    .unwrap()
                    .phase
                    .store(phase, Ordering::SeqCst);
                self.last = None;
                println!(
                    "{}",
                    serde_json::json!({"event":"retry_update", "phase":phase})
                );
                io::stdout().flush().unwrap();
                RuntimeProgramUpdate::redraw(self.target)
            }
            Message::Quit => RuntimeProgramUpdate::exit(),
            Message::Visible(visible) => {
                eprintln!("probe visibility: {visible}");
                Arc::make_mut(&mut self.style.layout).paint.visibility = Some(if visible {
                    VisibilitySpec::Visible
                } else {
                    VisibilitySpec::Hidden
                });
                self.last = None;
                if let (Some(window), Some(container)) =
                    (windows.get_mut(&self.target), self.container)
                {
                    let mut queue = MutationQueue::new();
                    queue.set_style(container, self.style.clone());
                    window
                        .document
                        .context_mut()
                        .commit_mutations(queue)
                        .unwrap();
                }
                RuntimeProgramUpdate::redraw_all()
            }
        }
    }
    fn prepare(&mut self, _: &mut ApplicationWindow, context: &RuntimeProgramContext<Message>) {
        if context.window_id() != self.target {
            return;
        }
        if let Some(retry) = &self.retry {
            let phase = retry.phase.load(Ordering::SeqCst);
            if self.prepared_phase != Some(phase) {
                println!(
                    "{}",
                    serde_json::json!({"event":"retry_prepare", "phase":phase})
                );
                io::stdout().flush().unwrap();
                self.prepared_phase = Some(phase);
            }
        }
    }
    fn presented(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        if context.window_id() != self.target {
            let value = window
                .document
                .context()
                .world()
                .text_input(self.primary_editor.unwrap())
                .unwrap()
                .value
                .to_owned();
            if self.primary_last.as_ref() != Some(&value) {
                println!(
                    "{}",
                    serde_json::json!({"event":"primary_state", "value":value})
                );
                io::stdout().flush().unwrap();
                self.primary_last = Some(value);
            }
            if !self.auxiliary_opened {
                self.auxiliary_opened = true;
                return RuntimeProgramUpdate {
                    window_commands: vec![WindowCommand::Open {
                        id: self.target,
                        settings: WindowDescriptor::new("NanaUI Auxiliary Probe")
                            .initial_size(420.0, 180.0)
                            .minimum_size(420.0, 180.0),
                    }],
                    ..Default::default()
                };
            }
            return RuntimeProgramUpdate::default();
        }
        if let Some(editor) = self.editor {
            let world = window.document.context().world();
            let current = (
                world.focused(window.document.document()) == Some(editor),
                world
                    .text_input(editor)
                    .map(|input| input.value.to_owned())
                    .unwrap_or_default(),
            );
            if self.last.as_ref() != Some(&current) {
                println!(
                    "{}",
                    serde_json::json!({"event":"state", "focused":current.0, "value":current.1,
                        "phase": self.retry.as_ref().map_or(0, |retry| retry.phase.load(Ordering::SeqCst)),
                        "accessibility": format!("{:?}", world.project_accessibility(window.document.document()))})
                );
                io::stdout().flush().unwrap();
                self.last = Some(current);
            }
        }
        if self.retry.is_some() && !self.retry_started {
            self.retry_started = true;
            context.dispatch(Message::RetryAdd);
        }
        RuntimeProgramUpdate::default()
    }
    fn window_closed(&mut self, id: WindowId) {
        println!(
            "{}",
            serde_json::json!({"event":"window_closed", "window":id.0})
        );
        io::stdout().flush().unwrap();
    }
}
fn main() -> Result<(), nana_ui::HostedRunError> {
    run_runtime::<RuntimeApplication<Probe>>(
        WindowDescriptor::new("NanaUI Accessibility Probe")
            .minimum_size(420.0, 180.0)
            .initial_size(420.0, 180.0),
    )
}
