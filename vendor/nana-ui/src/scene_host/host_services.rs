//! A native window as an input source: its endpoint and stamps, and the
//! host services the Runtime drives while routing its input. The IME is
//! applied straight to the window, the clipboard is the process's one
//! backend, and the cursor the Runtime asks for is composed here with the
//! window's own frame edges and the program's override.

use std::sync::OnceLock;

use nana_ui_platform::{
    EndpointGeneration, HostServiceError, HostServices, InputEndpoint, InputSequencer,
    InputSourceId, SharedClipboardHost, TextInputContext, TextInputPurpose,
    default_shared_clipboard, read_shared_clipboard, write_shared_clipboard,
};
use winit::cursor::CursorIcon;
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeHint, ImePurpose, ImeRequest, ImeRequestData,
    ImeRequestError, ImeSurroundingText,
};

/// Events one window may hold between drains. Moves and wheel merge, so
/// this bounds bursts of transitions.
const ENDPOINT_EVENTS: usize = 1024;
const ENDPOINT_PAYLOAD_BYTES: usize = 1024 * 1024;

/// The process's clipboard: one system resource, opened once.
pub(super) fn process_clipboard() -> SharedClipboardHost {
    static CLIPBOARD: OnceLock<SharedClipboardHost> = OnceLock::new();
    CLIPBOARD.get_or_init(default_shared_clipboard).clone()
}

/// One window's input source.
#[derive(Debug)]
pub(super) struct WindowInputSource {
    pub(super) endpoint: InputEndpoint,
    pub(super) sequencer: InputSequencer,
    /// The cursor the Runtime last asked for.
    pub(super) runtime_cursor: nana_ui_platform::CursorIcon,
    /// Icon and visibility last set on the native window.
    pub(super) applied_cursor: Option<(CursorIcon, bool)>,
    /// Text-input state last applied to the native IME.
    pub(super) ime: Option<TextInputContext>,
}

impl WindowInputSource {
    pub(super) fn new(source: InputSourceId, generation: EndpointGeneration) -> Self {
        Self {
            endpoint: InputEndpoint::new(ENDPOINT_EVENTS, ENDPOINT_PAYLOAD_BYTES),
            sequencer: InputSequencer::new(source, generation),
            runtime_cursor: nana_ui_platform::CursorIcon::Default,
            applied_cursor: None,
            ime: None,
        }
    }
}

/// The host services of one window for one drain.
pub(super) struct NativeWindowServices<'a> {
    pub(super) window: &'a dyn winit::window::Window,
    pub(super) clipboard: &'a SharedClipboardHost,
    pub(super) runtime_cursor: &'a mut nana_ui_platform::CursorIcon,
    pub(super) ime: &'a mut Option<TextInputContext>,
    /// Set when the Runtime's cursor changed; the host composes and applies
    /// it once the drain is over.
    pub(super) cursor_changed: bool,
}

impl<'a> NativeWindowServices<'a> {
    /// Services over `source`'s slots, leaving its endpoint free to drain.
    pub(super) fn of(
        window: &'a dyn winit::window::Window,
        clipboard: &'a SharedClipboardHost,
        source: &'a mut WindowInputSource,
    ) -> (
        Self,
        &'a mut InputEndpoint,
        InputSourceId,
        EndpointGeneration,
    ) {
        let input_id = source.sequencer.source();
        let generation = source.sequencer.generation();
        (
            Self {
                window,
                clipboard,
                runtime_cursor: &mut source.runtime_cursor,
                ime: &mut source.ime,
                cursor_changed: false,
            },
            &mut source.endpoint,
            input_id,
            generation,
        )
    }
}

impl HostServices for NativeWindowServices<'_> {
    fn set_cursor(&mut self, cursor: nana_ui_platform::CursorIcon) {
        *self.runtime_cursor = cursor;
        self.cursor_changed = true;
    }

    fn set_text_input(&mut self, state: Option<&TextInputContext>) {
        // Follow the focused editable field, not NSWindow key status: gating
        // on focus disables the IME while its own candidate panel is key.
        apply_text_input(self.window, ime_apply(self.ime.as_ref(), state));
        *self.ime = state.cloned();
    }

    fn read_clipboard(&mut self) -> Result<Option<String>, HostServiceError> {
        read_shared_clipboard(self.clipboard)
    }

    fn write_clipboard(&mut self, text: &str) -> Result<(), HostServiceError> {
        write_shared_clipboard(self.clipboard, text)
    }
}

/// The native icon for the Runtime's cursor, and whether it shows.
pub(super) fn native_cursor(cursor: nana_ui_platform::CursorIcon) -> (CursorIcon, bool) {
    use nana_ui_platform::CursorIcon as Runtime;
    match cursor {
        Runtime::Default => (CursorIcon::Default, true),
        Runtime::Pointer => (CursorIcon::Pointer, true),
        Runtime::Text => (CursorIcon::Text, true),
        Runtime::Move => (CursorIcon::Move, true),
        Runtime::Grab => (CursorIcon::Grab, true),
        Runtime::Grabbing => (CursorIcon::Grabbing, true),
        Runtime::NotAllowed => (CursorIcon::NotAllowed, true),
        Runtime::Crosshair => (CursorIcon::Crosshair, true),
        Runtime::Help => (CursorIcon::Help, true),
        Runtime::Wait => (CursorIcon::Wait, true),
        Runtime::Progress => (CursorIcon::Progress, true),
        Runtime::ZoomIn => (CursorIcon::ZoomIn, true),
        Runtime::ZoomOut => (CursorIcon::ZoomOut, true),
        Runtime::EwResize => (CursorIcon::EwResize, true),
        Runtime::NsResize => (CursorIcon::NsResize, true),
        Runtime::Hidden => (CursorIcon::Default, false),
    }
}

/// What to ask of the native IME to go from `previous` to `next`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ImeApply {
    None,
    Disable,
    Enable {
        capabilities: ImeCapabilities,
        data: ImeRequestData,
    },
    Replace {
        capabilities: ImeCapabilities,
        data: ImeRequestData,
    },
    Update(ImeRequestData),
}

fn ime_capabilities(state: &TextInputContext) -> ImeCapabilities {
    let mut capabilities = ImeCapabilities::new().with_hint_and_purpose();
    if state.cursor_area.is_some() {
        capabilities = capabilities.with_cursor_area();
    }
    if state.surrounding.is_some() {
        capabilities = capabilities.with_surrounding_text();
    }
    capabilities
}

fn ime_request_data(state: &TextInputContext) -> ImeRequestData {
    let purpose = match state.purpose {
        TextInputPurpose::Normal => ImePurpose::Normal,
        TextInputPurpose::Password => ImePurpose::Password,
        TextInputPurpose::Terminal => ImePurpose::Terminal,
    };
    let mut data = ImeRequestData::default().with_hint_and_purpose(ImeHint::NONE, purpose);
    if let Some(cursor) = state.cursor_area {
        data = data.with_cursor_area(
            winit::dpi::LogicalPosition::new(cursor.x, cursor.y + cursor.height).into(),
            winit::dpi::LogicalSize::new(cursor.width.max(1.0), cursor.height.max(1.0)).into(),
        );
    }
    if let Some(surrounding) = state.surrounding.as_ref().and_then(|surrounding| {
        ImeSurroundingText::new(
            surrounding.text.clone(),
            surrounding.cursor,
            surrounding.anchor,
        )
        .ok()
    }) {
        data = data.with_surrounding_text(surrounding);
    }
    data
}

pub(super) fn ime_apply(
    previous: Option<&TextInputContext>,
    next: Option<&TextInputContext>,
) -> ImeApply {
    let Some(next) = next else {
        return if previous.is_some() {
            ImeApply::Disable
        } else {
            ImeApply::None
        };
    };
    let capabilities = ime_capabilities(next);
    let data = ime_request_data(next);
    let Some(previous) = previous else {
        return ImeApply::Enable { capabilities, data };
    };
    if ime_capabilities(previous) != capabilities {
        ImeApply::Replace { capabilities, data }
    } else {
        ImeApply::Update(data)
    }
}

fn enable_ime(
    window: &dyn winit::window::Window,
    capabilities: ImeCapabilities,
    data: ImeRequestData,
) {
    let Some(enable) = ImeEnableRequest::new(capabilities, data.clone()) else {
        return;
    };
    if window.request_ime_update(ImeRequest::Enable(enable)) == Err(ImeRequestError::AlreadyEnabled)
    {
        let _ = window.request_ime_update(ImeRequest::Update(data));
    }
}

pub(super) fn apply_text_input(window: &dyn winit::window::Window, apply: ImeApply) {
    match apply {
        ImeApply::None => {}
        ImeApply::Disable => {
            let _ = window.request_ime_update(ImeRequest::Disable);
        }
        ImeApply::Enable { capabilities, data } => enable_ime(window, capabilities, data),
        ImeApply::Replace { capabilities, data } => {
            let _ = window.request_ime_update(ImeRequest::Disable);
            enable_ime(window, capabilities, data);
        }
        ImeApply::Update(data) => {
            let _ = window.request_ime_update(ImeRequest::Update(data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_core::LogicalRect;
    use nana_ui_platform::SurroundingText;

    fn state(cursor_area: bool, surrounding: bool) -> TextInputContext {
        TextInputContext {
            purpose: TextInputPurpose::Normal,
            cursor_area: cursor_area.then(|| LogicalRect::new(10.0, 20.0, 2.0, 18.0)),
            surrounding: surrounding.then(|| SurroundingText {
                text: "abc".into(),
                cursor: 3,
                anchor: 3,
            }),
        }
    }

    #[test]
    fn the_ime_enables_once_then_updates_its_caret() {
        let first = state(true, true);
        assert!(matches!(
            ime_apply(None, Some(&first)),
            ImeApply::Enable { .. }
        ));
        let moved = TextInputContext {
            cursor_area: Some(LogicalRect::new(30.0, 20.0, 2.0, 18.0)),
            ..first.clone()
        };
        assert!(matches!(
            ime_apply(Some(&first), Some(&moved)),
            ImeApply::Update(_)
        ));
    }

    #[test]
    fn a_new_capability_replaces_the_ime_session() {
        assert!(matches!(
            ime_apply(Some(&state(false, true)), Some(&state(true, true))),
            ImeApply::Replace { .. }
        ));
    }

    #[test]
    fn leaving_the_field_disables_the_ime() {
        assert_eq!(ime_apply(Some(&state(true, true)), None), ImeApply::Disable);
        assert_eq!(ime_apply(None, None), ImeApply::None);
    }

    #[test]
    fn hidden_runtime_cursor_hides_the_native_one() {
        assert_eq!(
            native_cursor(nana_ui_platform::CursorIcon::Hidden),
            (CursorIcon::Default, false)
        );
        assert_eq!(
            native_cursor(nana_ui_platform::CursorIcon::EwResize),
            (CursorIcon::EwResize, true)
        );
    }
}
