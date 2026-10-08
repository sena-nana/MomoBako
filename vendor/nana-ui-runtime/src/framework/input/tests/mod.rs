//! Routing tests: canonical events through the one input path.
//!
//! [`TestInput`] keeps the shape most fixtures were written in, dispatching
//! one gesture into one document, but every gesture becomes canonical events
//! stamped and routed exactly as a headless host routes them.

use std::borrow::Cow;
use std::time::Duration;

use nana_ui_input::{
    CommittedText, CompositionInput, HeadlessHostServices, InputDisposition, InputModifiers,
    InputPayload, KeyInput, KeyState, LogicalKey, PhysicalKey, PointerId, PointerInput,
    PointerPhase, PointerType, WheelInput, WheelUnit,
};

use super::*;

/// A pointer sample, spelled with every field the way fixtures set them.
macro_rules! pointer_fixture {
    ($($field:tt)*) => {
        Gesture::from(PointerFixture { $($field)* })
    };
}

/// A key press or release and, for a press, the text it types.
macro_rules! key_fixture {
    ($($field:tt)*) => {
        Gesture::from(KeyFixture { $($field)* })
    };
}

/// A wheel delta at a point.
macro_rules! wheel_fixture {
    ($($field:tt)*) => {
        Gesture::from(WheelFixture { $($field)* })
    };
}

mod dispatch;
mod hover_card;
mod modal_handles;
mod popover_trigger;
mod router;
mod terminal;

/// One thing a fixture does to a document, as a host would deliver it.
#[derive(Debug, Clone)]
enum Gesture {
    Event(InputPayload),
    /// A key press and the text it types: the key, then the text naming it.
    Key {
        key: KeyInput,
        text: Option<String>,
    },
}

impl From<InputPayload> for Gesture {
    fn from(payload: InputPayload) -> Self {
        Self::Event(payload)
    }
}

struct PointerFixture {
    phase: PointerPhase,
    pointer_id: u64,
    pointer_type: PointerType,
    x: f32,
    y: f32,
    screen_x: f32,
    screen_y: f32,
    button: i16,
    buttons: u16,
    pressure: f32,
    tangential_pressure: f32,
    tilt_x: i16,
    tilt_y: i16,
    twist: u16,
    is_primary: bool,
    activation_click: bool,
    modifiers: InputModifiers,
}

impl From<PointerFixture> for Gesture {
    fn from(fixture: PointerFixture) -> Self {
        Self::Event(InputPayload::Pointer(PointerInput {
            phase: fixture.phase,
            pointer_id: PointerId(fixture.pointer_id),
            pointer_type: fixture.pointer_type,
            x: fixture.x,
            y: fixture.y,
            screen_x: fixture.screen_x,
            screen_y: fixture.screen_y,
            button: fixture.button,
            buttons: fixture.buttons,
            pressure: fixture.pressure,
            tangential_pressure: fixture.tangential_pressure,
            tilt_x: fixture.tilt_x,
            tilt_y: fixture.tilt_y,
            twist: fixture.twist,
            is_primary: fixture.is_primary,
            activation_click: fixture.activation_click,
            modifiers: fixture.modifiers,
        }))
    }
}

struct KeyFixture {
    pressed: bool,
    key: String,
    text: Option<String>,
    code: String,
    repeat: bool,
    modifiers: InputModifiers,
}

impl From<KeyFixture> for Gesture {
    fn from(fixture: KeyFixture) -> Self {
        let key = KeyInput {
            physical: PhysicalKey(Cow::Owned(fixture.code)),
            logical: LogicalKey(Cow::Owned(fixture.key)),
            state: if fixture.pressed {
                KeyState::Pressed
            } else {
                KeyState::Released
            },
            repeat: fixture.repeat,
            modifiers: fixture.modifiers,
        };
        if fixture.pressed {
            Self::Key {
                key,
                text: fixture.text,
            }
        } else {
            Self::Event(InputPayload::Key(key))
        }
    }
}

struct WheelFixture {
    x: f32,
    y: f32,
    delta_x: f32,
    delta_y: f32,
    line_delta: bool,
    modifiers: InputModifiers,
}

impl From<WheelFixture> for Gesture {
    fn from(fixture: WheelFixture) -> Self {
        Self::Event(InputPayload::Wheel(WheelInput {
            pointer_id: PointerId(1),
            x: fixture.x,
            y: fixture.y,
            delta_x: fixture.delta_x,
            delta_y: fixture.delta_y,
            unit: if fixture.line_delta {
                WheelUnit::Lines
            } else {
                WheelUnit::Pixels
            },
            modifiers: fixture.modifiers,
        }))
    }
}

/// Dispatch gestures into whichever document a fixture names, through a
/// headless source bound to it. The clipboard is the headless host's.
#[derive(Default)]
struct TestInput {
    bound: Option<(DocumentId, HeadlessInput)>,
    services: HeadlessHostServices,
}

impl TestInput {
    fn with_clipboard(text: &str) -> Self {
        let mut input = Self::default();
        input.services.set_clipboard(Some(text.to_owned()));
        input
    }

    fn clipboard(&self) -> Option<&str> {
        self.services.clipboard()
    }

    fn set_clipboard(&mut self, text: &str) {
        self.services.set_clipboard(Some(text.to_owned()));
    }

    fn source(&mut self, context: &mut AppContext, document: DocumentId) -> &mut HeadlessInput {
        if self
            .bound
            .as_ref()
            .is_none_or(|(bound, _)| *bound != document)
        {
            self.bound = Some((document, HeadlessInput::bind(context, document)));
        }
        &mut self.bound.as_mut().expect("bound above").1
    }

    fn dispatch(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        gesture: &Gesture,
    ) -> Result<InputDisposition, FrameworkError> {
        self.dispatch_with_shaper(context, document, gesture, Duration::ZERO, None)
    }

    fn dispatch_at(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        gesture: &Gesture,
        now: Duration,
    ) -> Result<InputDisposition, FrameworkError> {
        self.dispatch_with_shaper(context, document, gesture, now, None)
    }

    /// Routes at `now`, or at the source's last time when a fixture steps
    /// back: routing rejects a timestamp that regresses.
    fn dispatch_with_shaper(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        gesture: &Gesture,
        now: Duration,
        mut shaper: Option<&mut dyn TextShaper>,
    ) -> Result<InputDisposition, FrameworkError> {
        let source = self.source(context, document);
        source.set_now(now.max(source.now()));
        match gesture {
            Gesture::Event(payload) => {
                let event = source.stamp(payload.clone());
                disposition(context.route_input(
                    &event,
                    &mut self.services,
                    reborrow_text_shaper(&mut shaper),
                ))
            }
            Gesture::Key { key, text } => {
                let key_event = source.stamp(InputPayload::Key(key.clone()));
                let sequence = key_event.metadata.sequence;
                let text_event = text.as_ref().filter(|text| !text.is_empty()).map(|text| {
                    source.stamp(InputPayload::Text(CommittedText {
                        text: text.clone(),
                        key: Some(sequence),
                    }))
                });
                let pressed = disposition(context.route_input(
                    &key_event,
                    &mut self.services,
                    reborrow_text_shaper(&mut shaper),
                ))?;
                let Some(text_event) = text_event else {
                    return Ok(pressed);
                };
                let typed = disposition(context.route_input(
                    &text_event,
                    &mut self.services,
                    reborrow_text_shaper(&mut shaper),
                ))?;
                Ok(InputDisposition {
                    handled: pressed.handled || typed.handled,
                    prevent_default: pressed.prevent_default || typed.prevent_default,
                })
            }
        }
    }

    fn dispatch_ime(
        &mut self,
        context: &mut AppContext,
        document: DocumentId,
        composition: &CompositionInput,
    ) -> Result<InputDisposition, FrameworkError> {
        self.dispatch(
            context,
            document,
            &Gesture::from(InputPayload::Composition(composition.clone())),
        )
    }
}

fn disposition(
    routed: Result<InputRouteOutcome, InputRouteError>,
) -> Result<InputDisposition, FrameworkError> {
    match routed {
        Ok(outcome) => Ok(outcome.disposition()),
        Err(InputRouteError::Dispatch(error)) => Err(error),
        Err(rejected) => panic!("routing rejected a fixture event: {rejected}"),
    }
}
