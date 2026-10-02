//! Native two-window acceptance probe. Commands on stdin: lock, unlock, forward, forward-off,
//! taskbar-show, taskbar-hide, hide, show, close, quit.
//! Run through the Scene host; inspect output plus native pointer/compositor behavior.
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DocumentId, FrameworkError, RuntimeDocument, Stack, Text};
use nana_ui::{
    DocumentAccessError, MaterialEffect, RoutedInput, RuntimeProgram, RuntimeProgramContext,
    RuntimeProgramUpdate, ThemeMode, WindowDescriptor, WindowHandle, run_runtime,
};
use nana_ui_core::LengthSpec;
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{
    InputPayload, PointerInput, PointerPhase, WindowEvent, WindowId, WindowRole,
};
use std::{
    convert::Infallible,
    io::{self, BufRead, Write},
};

const OVERLAY: WindowId = WindowId(1);
const OPAQUE_HIT: (f32, f32, f32, f32) = (24.0, 24.0, 140.0, 80.0);
#[derive(Clone)]
enum Message {
    Lock(bool),
    Forward(bool),
    SkipTaskbar(bool),
    Visible(bool),
    Close,
    Fail,
    Quit,
}
struct Probe {
    primary: RuntimeDocument,
    overlay: RuntimeDocument,
    overlay_window: Option<WindowHandle>,
}
fn report(value: serde_json::Value) {
    println!("{value}");
    io::stdout().flush().unwrap();
}
fn commands(window_commands: Vec<WindowCommand>) -> RuntimeProgramUpdate {
    RuntimeProgramUpdate {
        window_commands,
        ..RuntimeProgramUpdate::redraw_all()
    }
}
impl RuntimeProgram for Probe {
    type Message = Message;
    type Error = Infallible;
    fn initialize(
        context: &RuntimeProgramContext<Message>,
    ) -> Result<(Self, Vec<Message>), Infallible> {
        let primary_id = DocumentId::new(1).unwrap();
        let mut primary = RuntimeDocument::new(primary_id);
        primary
            .context_mut()
            .mount_view_root(primary_id, || {
                // The native-composition reference: a plain green fill.
                widget(Stack::fill_column(0.0).with_layout(|layout| {
                    layout.background = Some([0.0, 1.0, 0.0, 1.0]);
                }))
            })
            .unwrap();
        let id = DocumentId::new(2).unwrap();
        let mut overlay = RuntimeDocument::new(id);
        overlay
            .context_mut()
            .mount_view_root(id, || {
                widget(Stack::fill_column(0.0).align(nana_ui_core::AlignSpec::Start)).children(
                    // The opaque hit target.
                    widget(
                        Stack::column(8.0)
                            .width(LengthSpec::Px(OPAQUE_HIT.2))
                            .height(LengthSpec::Px(OPAQUE_HIT.3))
                            .hittable()
                            .with_layout(|layout| {
                                layout.margin_left = Some(LengthSpec::Px(OPAQUE_HIT.0));
                                layout.margin_top = Some(LengthSpec::Px(OPAQUE_HIT.1));
                                layout.background = Some([0.85, 0.15, 0.2, 1.0]);
                            }),
                    )
                    .children(widget(Text::new("Native overlay pointer target"))),
                )
            })
            .unwrap();
        let context = context.clone();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                let message = match line.trim() {
                    "lock" => Message::Lock(true),
                    "unlock" => Message::Lock(false),
                    "forward" => Message::Forward(true),
                    "forward-off" => Message::Forward(false),
                    "taskbar-show" => Message::SkipTaskbar(false),
                    "taskbar-hide" => Message::SkipTaskbar(true),
                    "hide" => Message::Visible(false),
                    "show" => Message::Visible(true),
                    "close" => Message::Close,
                    "fail" => Message::Fail,
                    "quit" => Message::Quit,
                    _ => continue,
                };
                context.dispatch(message);
            }
        });
        Ok((
            Self {
                primary,
                overlay,
                overlay_window: None,
            },
            vec![],
        ))
    }
    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(match id {
            WindowId::PRIMARY => Some(f(&self.primary)),
            OVERLAY => Some(f(&self.overlay)),
            _ => None,
        })
    }
    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(match id {
            WindowId::PRIMARY => Some(f(&mut self.primary)),
            OVERLAY => Some(f(&mut self.overlay)),
            _ => None,
        })
    }
    fn update(
        &mut self,
        message: Message,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        match message {
            Message::Lock(enabled) => commands(vec![WindowCommand::SetMousePassthrough {
                id: OVERLAY,
                enabled,
            }]),
            Message::Forward(enabled) => {
                commands(vec![WindowCommand::SetMousePassthroughForward {
                    id: OVERLAY,
                    enabled,
                }])
            }
            Message::SkipTaskbar(skip_taskbar) => commands(vec![WindowCommand::SetSkipTaskbar {
                id: OVERLAY,
                skip_taskbar,
            }]),
            Message::Visible(visible) => {
                if let Some(window) = &self.overlay_window {
                    // The host thread cannot wait; the change shows up natively.
                    let _request = window.set_visible(visible);
                }
                RuntimeProgramUpdate::default()
            }
            Message::Close => commands(vec![WindowCommand::Close(OVERLAY)]),
            Message::Fail => {
                let mut settings = WindowDescriptor::new("invalid modal");
                settings.modal = true;
                commands(vec![WindowCommand::Open {
                    id: WindowId(2),
                    settings,
                }])
            }
            Message::Quit => RuntimeProgramUpdate::exit(),
        }
    }
    fn theme_mode(&self) -> ThemeMode {
        ThemeMode::Dark
    }
    fn window_material_mode_for(&self, id: WindowId) -> MaterialEffect {
        if id == OVERLAY {
            MaterialEffect::Transparent
        } else {
            MaterialEffect::Solid
        }
    }
    fn appearance_backdrop_opacity_for(&self, _: WindowId) -> f32 {
        0.0
    }
    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready {
                id: WindowId::PRIMARY,
                ..
            } => {
                let mut settings = WindowDescriptor::new("NanaUI overlay probe layer")
                    .initial_size(420.0, 300.0)
                    .minimum_size(280.0, 240.0);
                settings.initial_position = Some((200.0, 200.0));
                settings.transparent = true;
                // A click-through overlay layer, not a card: no desktop shadow.
                settings.shadow = nana_ui::WindowShadow::None;
                settings.always_on_top = true;
                settings.focus_on_show = false;
                settings.constrain_to_work_area = true;
                settings.skip_taskbar = true;
                settings.role = WindowRole::Tool;
                assert_eq!(context.material().effect, MaterialEffect::Solid);
                report(serde_json::json!({"event":"primary_ready"}));
                commands(vec![WindowCommand::Open {
                    id: OVERLAY,
                    settings,
                }])
            }
            WindowEvent::Ready {
                id: OVERLAY,
                geometry,
            } => {
                assert_eq!(context.material().effect, MaterialEffect::Transparent);
                self.overlay_window = Some(context.window());
                assert_ne!(
                    context.surface_alpha_mode(),
                    nana_ui::SurfaceAlphaMode::Opaque
                );
                report(serde_json::json!({
                    "event": "ready",
                    "window": OVERLAY.0,
                    "scale": geometry.scale_factor,
                    "hit": {
                        "x": OPAQUE_HIT.0,
                        "y": OPAQUE_HIT.1,
                        "w": OPAQUE_HIT.2,
                        "h": OPAQUE_HIT.3
                    }
                }));
                commands(vec![WindowCommand::SetMousePassthrough {
                    id: WindowId(99),
                    enabled: true,
                }])
            }
            WindowEvent::MousePassthroughChanged {
                id,
                enabled,
                result,
            } => {
                if id == WindowId(99) {
                    assert!(result.is_err());
                } else {
                    assert!(result.is_ok(), "{result:?}");
                }
                report(
                    serde_json::json!({"event":"passthrough", "window":id.0, "enabled":enabled, "success":result.is_ok()}),
                );
                if id == OVERLAY && !enabled && result.is_ok() {
                    commands(vec![WindowCommand::Focus(OVERLAY)])
                } else {
                    RuntimeProgramUpdate::default()
                }
            }
            WindowEvent::SkipTaskbarChanged {
                id,
                skip_taskbar,
                result,
            } => {
                report(
                    serde_json::json!({"event":"skip_taskbar", "window":id.0, "skip":skip_taskbar, "success":result.is_ok(), "error":result.err()}),
                );
                RuntimeProgramUpdate::default()
            }
            WindowEvent::Closed { id } => {
                report(serde_json::json!({"event":"closed", "window":id.0}));
                RuntimeProgramUpdate::default()
            }
            WindowEvent::OpenFailed {
                id: WindowId(2), ..
            } => {
                report(serde_json::json!({"event":"open_failed", "window":2}));
                RuntimeProgramUpdate::default()
            }
            WindowEvent::OpenFailed { error, .. } => panic!("{error}"),
            WindowEvent::CloseRequested {
                id: WindowId::PRIMARY,
            } => RuntimeProgramUpdate::exit(),
            WindowEvent::CloseRequested { id } => commands(vec![WindowCommand::Close(id)]),
            _ => RuntimeProgramUpdate::default(),
        }
    }
    fn input_event(
        &mut self,
        id: WindowId,
        input: RoutedInput<'_>,
        _: &RuntimeProgramContext<Message>,
    ) -> Result<RuntimeProgramUpdate, FrameworkError> {
        let event = input.event;
        if let InputPayload::Pointer(PointerInput {
            phase: PointerPhase::Down,
            x,
            y,
            ..
        }) = &event.payload
        {
            report(serde_json::json!({"event":"pointer_down", "window":id.0, "x":x, "y":y}));
        }
        Ok(RuntimeProgramUpdate::default())
    }
}
fn main() -> Result<(), nana_ui::HostedRunError> {
    let mut settings = WindowDescriptor::new("NanaUI overlay probe base")
        .initial_size(800.0, 600.0)
        .minimum_size(600.0, 400.0);
    settings.initial_position = Some((100.0, 100.0));
    run_runtime::<Probe>(settings)
}
