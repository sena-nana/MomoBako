use nana_ui_core::LogicalPoint;
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{InputPayload, PointerInput, PointerPhase, WindowId};
use nana_ui_runtime::{
    AccessibilityRole, AppContext, AppTitleBar, AppTitleBarControls, DocumentId, Entity, NodeKind,
};

pub use nana_ui_core::{WindowChrome, WindowChromeAction, WindowControlMode};

const DRAG_THRESHOLD: f32 = 4.0;

/// Input handled by [`WindowChromeState`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowChromeEvent {
    PointerMoved(LogicalPoint),
    PointerPressed,
    PointerReleased,
    PointerCancelled,
    Action(WindowChromeAction),
    PrepareWindow(WindowId),
    SyncMaximized(WindowId),
    WindowClosed(WindowId),
    MaximizedChanged { window: WindowId, maximized: bool },
}

/// Interaction state shared by title bar views and their window runtime.
#[derive(Debug, Clone)]
pub struct WindowChromeState {
    chrome: WindowChrome,
    window: Option<WindowId>,
    auto_bind: bool,
    cursor_position: Option<LogicalPoint>,
    drag_origin: Option<LogicalPoint>,
    maximized: bool,
}

impl WindowChromeState {
    /// Creates a state that binds to the first window opened by the host.
    pub fn new(chrome: WindowChrome) -> Self {
        Self {
            chrome,
            window: None,
            auto_bind: true,
            cursor_position: None,
            drag_origin: None,
            maximized: false,
        }
    }

    /// Creates a state bound to a specific host window.
    pub fn for_window(window: WindowId, chrome: WindowChrome) -> Self {
        let mut state = Self::new(chrome);
        state.bind(window);
        state
    }

    /// Binds this state to a specific host window.
    ///
    /// Binding explicitly disables automatic rebinding after the window closes.
    pub fn bind(&mut self, window: WindowId) {
        self.auto_bind = false;
        self.set_window(Some(window));
    }

    pub const fn window_id(&self) -> Option<WindowId> {
        self.window
    }

    pub const fn chrome(&self) -> WindowChrome {
        self.chrome
    }

    pub const fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// Synchronizes the visible maximize/restore state for a host-owned window.
    pub fn set_maximized(&mut self, maximized: bool) {
        self.maximized = maximized;
    }

    /// Reduces a title bar event and returns a runtime action when one is ready.
    pub fn update(&mut self, event: WindowChromeEvent) -> Option<WindowChromeAction> {
        match event {
            WindowChromeEvent::PointerMoved(position) => {
                self.cursor_position = Some(position);
                let origin = self.drag_origin?;
                if distance(origin, position) < DRAG_THRESHOLD {
                    return None;
                }
                self.drag_origin = None;
                Some(WindowChromeAction::Drag)
            }
            WindowChromeEvent::PointerPressed => {
                #[cfg(target_os = "macos")]
                {
                    Some(WindowChromeAction::Drag)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    self.drag_origin = self.cursor_position;
                    None
                }
            }
            WindowChromeEvent::PointerReleased | WindowChromeEvent::PointerCancelled => {
                self.drag_origin = None;
                None
            }
            WindowChromeEvent::Action(action) => {
                if action == WindowChromeAction::ToggleMaximize {
                    self.maximized = !self.maximized;
                }
                Some(action)
            }
            WindowChromeEvent::MaximizedChanged { window, maximized } => {
                if self.window == Some(window) {
                    self.set_maximized(maximized);
                }
                None
            }
            WindowChromeEvent::WindowClosed(window) => {
                if self.window == Some(window) {
                    self.set_window(None);
                }
                None
            }
            WindowChromeEvent::PrepareWindow(window) => {
                if self.window.is_none() && self.auto_bind {
                    self.set_window(Some(window));
                }
                None
            }
            WindowChromeEvent::SyncMaximized(_) => None,
        }
    }

    fn set_window(&mut self, window: Option<WindowId>) {
        self.window = window;
        self.cursor_position = None;
        self.drag_origin = None;
        self.maximized = false;
    }
}

impl Default for WindowChromeState {
    fn default() -> Self {
        Self::new(WindowChrome::platform_default())
    }
}

/// Tracks a title-bar pointer gesture and emits [`WindowChromeEvent`]s.
#[derive(Debug, Clone, Default)]
pub struct TitleBarDragTracker {
    pressed: bool,
    control: Option<WindowChromeAction>,
    drag_bar: Option<nana_ui_runtime::StableNodeId>,
}

impl TitleBarDragTracker {
    pub fn events(
        &mut self,
        context: &AppContext,
        document: DocumentId,
        event: &InputPayload,
    ) -> Vec<WindowChromeEvent> {
        if self.pressed
            && self.control.is_none()
            && self
                .drag_bar
                .is_none_or(|bar| !title_bar_drag_enabled(context, bar))
        {
            self.pressed = false;
            self.drag_bar = None;
            return vec![WindowChromeEvent::PointerCancelled];
        }
        let InputPayload::Pointer(pointer) = event else {
            return Vec::new();
        };
        match pointer {
            PointerInput {
                phase: PointerPhase::Down,
                button: 0,
                is_primary: true,
                activation_click,
                x,
                y,
                ..
            } => {
                let hit = title_bar_pointer_hit(context, document, *x, *y);
                if *activation_click && matches!(hit, TitleBarHit::Control(_)) {
                    return Vec::new();
                }
                match hit {
                    TitleBarHit::None => Vec::new(),
                    TitleBarHit::Control(action) => {
                        self.pressed = true;
                        self.control = Some(action);
                        Vec::new()
                    }
                    TitleBarHit::Drag(bar) => {
                        self.pressed = true;
                        self.control = None;
                        self.drag_bar = Some(bar);
                        vec![
                            WindowChromeEvent::PointerMoved(LogicalPoint::new(*x, *y)),
                            WindowChromeEvent::PointerPressed,
                        ]
                    }
                }
            }
            PointerInput {
                phase: PointerPhase::Move,
                x,
                y,
                ..
            } if self.pressed && self.control.is_none() => {
                vec![WindowChromeEvent::PointerMoved(LogicalPoint::new(*x, *y))]
            }
            PointerInput {
                phase: PointerPhase::Up,
                button: 0,
                x,
                y,
                ..
            } if self.pressed => {
                self.pressed = false;
                if let Some(action) = self.control.take() {
                    if title_bar_pointer_hit(context, document, *x, *y)
                        == TitleBarHit::Control(action)
                    {
                        vec![WindowChromeEvent::Action(action)]
                    } else {
                        Vec::new()
                    }
                } else {
                    vec![WindowChromeEvent::PointerReleased]
                }
            }
            PointerInput {
                phase: PointerPhase::Cancel,
                ..
            } if self.pressed => {
                self.pressed = false;
                self.control = None;
                vec![WindowChromeEvent::PointerCancelled]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleBarHit {
    None,
    Drag(nana_ui_runtime::StableNodeId),
    Control(WindowChromeAction),
}

/// True when `(x, y)` is blank custom-title-bar chrome (not traffic lights or buttons).
#[cfg(test)]
fn title_bar_drag_hit(context: &AppContext, document: DocumentId, x: f32, y: f32) -> bool {
    matches!(
        title_bar_pointer_hit(context, document, x, y),
        TitleBarHit::Drag(_)
    )
}

/// True when `(x, y)` is a custom Minimize / Maximize / Close control.
///
/// Client-frame resize must skip the actual button bounds. Padding around
/// the rounded buttons remains available to the title bar and frame.
pub fn title_bar_hits_window_control(
    context: &AppContext,
    document: DocumentId,
    x: f32,
    y: f32,
) -> bool {
    matches!(
        title_bar_pointer_hit(context, document, x, y),
        TitleBarHit::Control(_)
    )
}

fn title_bar_pointer_hit(
    context: &AppContext,
    document: DocumentId,
    x: f32,
    y: f32,
) -> TitleBarHit {
    let mut current = context.pointer_target(document, x, y);
    let mut control = None;
    while let Some(id) = current {
        if control.is_none() {
            control = window_control_action(context, id);
        }
        if is_app_title_bar(context, id) {
            if let Some(action) = control {
                return TitleBarHit::Control(action);
            }
            return if title_bar_drag_enabled(context, id) {
                TitleBarHit::Drag(id)
            } else {
                TitleBarHit::None
            };
        }
        // Native buttons keep their own clicks; the bar must not drag there.
        if (is_title_bar_control(context, id) || is_native_window_controls(context, id))
            && control.is_none()
        {
            return TitleBarHit::None;
        }
        current = context.world().node(id).and_then(|node| node.parent);
    }
    TitleBarHit::None
}

/// The native window-button placeholder of `document`, with its layout box
/// while it is shown. `hint` is the placeholder found last time; the document
/// is searched only when it no longer is one.
///
/// Any [`AppTitleBarControls`] with `native` set marks the spot, wherever it
/// is mounted — a title bar's controls slot is only the usual home. A window
/// has one set of native buttons, so the first such node in document order
/// wins.
#[cfg(feature = "hosted")]
pub(crate) fn native_window_controls(
    context: &AppContext,
    document: DocumentId,
    hint: Option<nana_ui_runtime::StableNodeId>,
) -> Option<(
    nana_ui_runtime::StableNodeId,
    Option<nana_ui_runtime::LayoutBox>,
)> {
    let mounted = |id| is_native_window_controls(context, id) && context.world().is_mounted(id);
    let id = hint.filter(|&id| mounted(id)).or_else(|| {
        context
            .world()
            .document_order(document)
            .into_iter()
            .find(|&id| mounted(id))
    })?;
    let bounds = context
        .world()
        .layout_box(id)
        .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0 && !hidden_in_tree(context, id));
    Some((id, bounds))
}

fn is_native_window_controls(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> bool {
    context
        .read(
            Entity::<AppTitleBarControls>::from_stable_id(id),
            |controls| controls.native,
        )
        .unwrap_or(false)
}

fn hidden_in_tree(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> bool {
    let mut current = Some(id);
    while let Some(id) = current {
        if context
            .world()
            .node_style(id)
            .is_some_and(|style| style.layout.hidden)
        {
            return true;
        }
        current = context.world().node(id).and_then(|node| node.parent);
    }
    false
}

/// Maps a custom-title-bar chrome action to host window commands.
pub fn window_commands_for_chrome_action(
    id: WindowId,
    action: WindowChromeAction,
    maximized: bool,
) -> Vec<WindowCommand> {
    match action {
        WindowChromeAction::Drag => vec![WindowCommand::Drag(id)],
        WindowChromeAction::Minimize => vec![WindowCommand::SetMinimized {
            id,
            minimized: true,
        }],
        WindowChromeAction::ToggleMaximize => {
            vec![WindowCommand::SetMaximized { id, maximized }]
        }
        WindowChromeAction::Close => vec![WindowCommand::Close(id)],
    }
}

/// Reduces title-bar pointer input through [`TitleBarDragTracker`] and [`WindowChromeState`].
pub fn apply_title_bar_pointer(
    state: &mut WindowChromeState,
    tracker: &mut TitleBarDragTracker,
    context: &AppContext,
    document: DocumentId,
    event: &InputPayload,
) -> Option<WindowChromeAction> {
    let mut action = None;
    for chrome_event in tracker.events(context, document, event) {
        action = state.update(chrome_event).or(action);
    }
    action
}

fn title_bar_drag_enabled(context: &AppContext, bar: nana_ui_runtime::StableNodeId) -> bool {
    if !context.world().is_mounted(bar)
        || context
            .read(Entity::<AppTitleBar>::from_stable_id(bar), |bar| {
                !bar.drag_enabled
            })
            .unwrap_or(false)
    {
        return false;
    }
    !hidden_in_tree(context, bar)
}

fn is_app_title_bar(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> bool {
    context
        .read(Entity::<AppTitleBar>::from_stable_id(id), |_| ())
        .is_ok()
        || context.world().node(id).is_some_and(|node| {
            matches!(
                &node.kind,
                NodeKind::Element { tag }
                    if tag == "app-title-bar" || tag == "nana-app-title-bar"
            )
        })
}

fn window_control_action(
    context: &AppContext,
    id: nana_ui_runtime::StableNodeId,
) -> Option<WindowChromeAction> {
    let parent = context.world().node(id).and_then(|node| node.parent)?;
    if !is_title_bar_controls(context, parent) {
        return None;
    }
    let children = context.world().node(parent)?.children;
    match children.iter().position(|child| *child == id) {
        Some(0) => Some(WindowChromeAction::Minimize),
        Some(1) => Some(WindowChromeAction::ToggleMaximize),
        Some(2) => Some(WindowChromeAction::Close),
        _ => None,
    }
}

fn is_title_bar_controls(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> bool {
    context
        .read(Entity::<AppTitleBarControls>::from_stable_id(id), |_| ())
        .is_ok()
        || context.world().node(id).is_some_and(|node| {
            matches!(
                &node.kind,
                NodeKind::Element { tag } if tag == "app-title-bar-controls"
            )
        })
}

fn is_title_bar_control(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> bool {
    context.world().accessibility(id).is_some_and(|state| {
        // Editable text keeps press-drag for caret and selection; a window
        // drag from inside an embedded input would break both.
        matches!(
            state.role,
            AccessibilityRole::Button | AccessibilityRole::TextInput
        )
    })
}

fn distance(from: LogicalPoint, to: LogicalPoint) -> f32 {
    (to.x - from.x).hypot(to.y - from.y)
}

#[cfg(test)]
mod tests {
    use super::{
        WindowChrome, WindowChromeAction, WindowChromeEvent, WindowChromeState, WindowControlMode,
    };
    use nana_ui_core::LogicalPoint;
    use nana_ui_platform::{InputPayload, PointerInput, PointerPhase, WindowId};

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn drag_starts_once_only_after_crossing_the_threshold() {
        let mut state = WindowChromeState::default();

        assert_eq!(
            state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(
                10.0, 10.0
            ))),
            None
        );
        assert_eq!(state.update(WindowChromeEvent::PointerPressed), None);
        assert_eq!(
            state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(
                12.0, 11.0
            ))),
            None
        );
        assert_eq!(
            state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(
                15.0, 10.0
            ))),
            Some(WindowChromeAction::Drag)
        );
        assert_eq!(
            state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(
                20.0, 10.0
            ))),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_delegates_drag_threshold_to_appkit_on_blank_press() {
        let mut state = WindowChromeState::default();

        assert_eq!(
            state.update(WindowChromeEvent::PointerPressed),
            Some(WindowChromeAction::Drag)
        );
    }

    #[test]
    fn release_and_cancel_clear_a_pending_drag() {
        for end in [
            WindowChromeEvent::PointerReleased,
            WindowChromeEvent::PointerCancelled,
        ] {
            let mut state = WindowChromeState::default();
            state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(4.0, 4.0)));
            state.update(WindowChromeEvent::PointerPressed);
            state.update(end);

            assert_eq!(
                state.update(WindowChromeEvent::PointerMoved(LogicalPoint::new(
                    20.0, 20.0
                ))),
                None
            );
        }
    }

    #[test]
    fn maximize_action_and_runtime_sync_update_visible_state() {
        let mut state = WindowChromeState::default();

        assert_eq!(
            state.update(WindowChromeEvent::Action(
                WindowChromeAction::ToggleMaximize
            )),
            Some(WindowChromeAction::ToggleMaximize)
        );
        assert!(state.is_maximized());

        state.set_maximized(false);
        assert!(!state.is_maximized());
    }

    #[test]
    fn explicit_states_bind_and_ignore_stale_windows() {
        let window_a = WindowId(1);
        let window_b = WindowId(2);
        let mut state_a = WindowChromeState::for_window(window_a, WindowChrome::platform_default());
        let mut state_b = WindowChromeState::for_window(window_b, WindowChrome::platform_default());

        assert_eq!(
            state_a.update(WindowChromeEvent::Action(WindowChromeAction::Minimize)),
            Some(WindowChromeAction::Minimize)
        );
        assert_eq!(
            state_b.update(WindowChromeEvent::Action(WindowChromeAction::Close)),
            Some(WindowChromeAction::Close)
        );

        state_a.update(WindowChromeEvent::WindowClosed(window_a));
        assert_eq!(state_a.window_id(), None);
        state_a.update(WindowChromeEvent::PrepareWindow(window_b));
        assert_eq!(state_a.window_id(), None);

        state_a.bind(window_b);
        state_a.update(WindowChromeEvent::MaximizedChanged {
            window: window_a,
            maximized: true,
        });
        assert!(!state_a.is_maximized());
        assert_eq!(
            state_a.update(WindowChromeEvent::Action(
                WindowChromeAction::ToggleMaximize
            )),
            Some(WindowChromeAction::ToggleMaximize)
        );
        assert!(state_a.is_maximized());
    }

    #[test]
    fn automatic_state_binds_first_window_and_rebinds_after_it_closes() {
        let window_a = WindowId(1);
        let window_b = WindowId(2);
        let mut state = WindowChromeState::default();

        assert_eq!(state.window_id(), None);
        state.update(WindowChromeEvent::PrepareWindow(window_a));
        assert_eq!(state.window_id(), Some(window_a));
        state.update(WindowChromeEvent::PrepareWindow(window_b));
        assert_eq!(state.window_id(), Some(window_a));

        state.update(WindowChromeEvent::WindowClosed(window_a));
        assert_eq!(state.window_id(), None);
        state.update(WindowChromeEvent::PrepareWindow(window_b));
        assert_eq!(state.window_id(), Some(window_b));
    }

    #[test]
    fn control_actions_are_forwarded_without_rewriting_their_meaning() {
        let mut state = WindowChromeState::default();

        for action in [
            WindowChromeAction::Drag,
            WindowChromeAction::Minimize,
            WindowChromeAction::Close,
        ] {
            assert_eq!(
                state.update(WindowChromeEvent::Action(action)),
                Some(action)
            );
        }
    }

    #[test]
    fn chrome_normalizes_insets_and_exposes_control_mode() {
        let chrome = WindowChrome::new(WindowControlMode::NativeTrailing, f32::NAN, -10.0);
        assert_eq!(chrome.leading_inset, 0.0);
        assert_eq!(chrome.trailing_inset, 0.0);
        assert!(!chrome.uses_custom_controls());
        assert!(WindowChrome::custom().uses_custom_controls());
    }

    #[test]
    fn native_control_hit_excludes_leading_traffic_light_inset() {
        use nana_ui_core::LogicalRect;
        let chrome = WindowChrome::native_leading(78.0);
        let bar = LogicalRect::new(0.0, 0.0, 800.0, 36.0);
        assert!(chrome.native_control_hit(bar, 12.0, 18.0));
        assert!(!chrome.native_control_hit(bar, 90.0, 18.0));
        assert!(!chrome.native_control_hit(bar, 12.0, 40.0));
        assert!(!WindowChrome::custom().native_control_hit(bar, 12.0, 18.0));
    }

    #[cfg(not(target_os = "macos"))]
    fn with_activation_click(mut event: InputPayload) -> InputPayload {
        if let InputPayload::Pointer(pointer) = &mut event {
            pointer.activation_click = true;
        }
        event
    }

    fn pointer_at(phase: PointerPhase, x: f32, y: f32) -> InputPayload {
        InputPayload::Pointer(PointerInput {
            buttons: u16::from(phase != PointerPhase::Up),
            pressure: 0.0,
            ..PointerInput::mouse(phase, x, y)
        })
    }

    fn pointer_down(x: f32, y: f32) -> InputPayload {
        pointer_at(PointerPhase::Down, x, y)
    }

    fn pointer_move(x: f32, y: f32) -> InputPayload {
        pointer_at(PointerPhase::Move, x, y)
    }

    #[cfg(not(target_os = "macos"))]
    fn pointer_up(x: f32, y: f32) -> InputPayload {
        pointer_at(PointerPhase::Up, x, y)
    }

    fn title_bar_document() -> (
        nana_ui_runtime::AppContext,
        nana_ui_runtime::DocumentId,
        nana_ui_runtime::Entity<nana_ui_runtime::AppTitleBar>,
        nana_ui_runtime::Entity<nana_ui_runtime::IconButton>,
    ) {
        use nana_ui_core::{ControlSize, Icon};
        use nana_ui_runtime::{AppContext, AppTitleBar, DocumentId, IconButton, LayoutViewport};

        let document = DocumentId::new(1).unwrap();
        let mut context = AppContext::new();
        let button = context
            .create_component(
                document,
                IconButton::new(Icon::Sidebar, "toggle").size(ControlSize::Small),
            )
            .unwrap();
        let bar = context
            .create_component(
                document,
                AppTitleBar::new("Nana").leading(button.stable_id()),
            )
            .unwrap();
        context.append_child(bar, button).unwrap();
        context
            .layout_document(document, LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);
        (context, document, bar, button)
    }

    #[test]
    fn transparent_title_bar_slots_drag_labels_but_not_business_buttons() {
        use super::title_bar_drag_hit;
        let (mut context, document, bar, button) = title_bar_document();
        let text = context
            .create_component(
                document,
                nana_ui_runtime::Text::new("Status").style(nana_ui_runtime::NodeStyle {
                    layout: std::sync::Arc::new(nana_ui_core::LayoutStyle {
                        width: Some(nana_ui_core::LengthSpec::Px(80.0)),
                        height: Some(nana_ui_core::LengthSpec::Px(20.0)),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();
        context
            .update_component(bar, |bar, _| {
                bar.transparent = true;
                bar.trailing = Some(text.stable_id());
            })
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document, nana_ui_runtime::LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let text_box = context.world().layout_box(text.stable_id()).unwrap();
        let button_box = context.world().layout_box(button.stable_id()).unwrap();
        assert!(title_bar_drag_hit(
            &context,
            document,
            text_box.x + text_box.width / 2.0,
            text_box.y + text_box.height / 2.0
        ));
        assert!(!title_bar_drag_hit(
            &context,
            document,
            button_box.x + button_box.width / 2.0,
            button_box.y + button_box.height / 2.0
        ));
        context
            .update_component(bar, |bar, _| bar.drag_enabled = false)
            .unwrap();
        assert!(!title_bar_drag_hit(
            &context,
            document,
            text_box.x + text_box.width / 2.0,
            text_box.y + text_box.height / 2.0
        ));
    }

    #[test]
    fn native_window_controls_placeholder_blocks_drag_and_reports_its_box() {
        use super::title_bar_drag_hit;
        let (mut context, document, bar, _) = title_bar_document();
        context
            .update_component(bar, |bar, _| {
                bar.native_controls = true;
                bar.show_window_controls = true;
            })
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();
        context
            .layout_document(document, nana_ui_runtime::LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let controls = context.read(bar, |bar| bar.controls).unwrap().unwrap();
        let placeholder = context.world().layout_box(controls).unwrap();
        let (x, y) = (
            placeholder.x + placeholder.width / 2.0,
            placeholder.y + placeholder.height / 2.0,
        );
        assert!(!title_bar_drag_hit(&context, document, x, y));
        assert!(title_bar_drag_hit(&context, document, 400.0, y));

        #[cfg(feature = "hosted")]
        {
            let found = super::native_window_controls(&context, document, None);
            assert_eq!(found, Some((controls, Some(placeholder))));
            context
                .update_component(bar, |bar, _| bar.show_window_controls = false)
                .unwrap();
            context
                .layout_document(document, nana_ui_runtime::LayoutViewport::new(800.0, 400.0))
                .unwrap();
            assert_eq!(
                super::native_window_controls(&context, document, Some(controls)),
                Some((controls, None))
            );
        }
    }

    #[cfg(feature = "hosted")]
    #[test]
    fn a_native_controls_placeholder_marks_the_spot_wherever_it_is_mounted() {
        use nana_ui_runtime::{AppContext, AppTitleBarControls, DocumentId, LayoutViewport};

        let document = DocumentId::new(7).unwrap();
        let mut context = AppContext::new();
        let mut style = nana_ui_runtime::NodeStyle::default();
        {
            let layout = std::sync::Arc::make_mut(&mut style.layout);
            layout.position = nana_ui_core::PositionSpec::Absolute;
            layout.offset_left = Some(nana_ui_core::LengthSpec::Px(240.0));
            layout.offset_top = Some(nana_ui_core::LengthSpec::Px(48.0));
        }
        let root = context
            .create_component(document, nana_ui_runtime::Stack::fill_column(0.0))
            .unwrap();
        let controls = context
            .create_detached_component(
                document,
                AppTitleBarControls::new(false).native(true).style(style),
            )
            .unwrap();
        context.append_child(root, controls).unwrap();
        context
            .layout_document(document, LayoutViewport::new(800.0, 400.0))
            .unwrap();

        let (found, bounds) = super::native_window_controls(&context, document, None)
            .expect("a mounted native placeholder is the target");
        assert_eq!(found, controls.stable_id());
        let bounds = bounds.expect("a shown placeholder reports its box");
        assert_eq!((bounds.x, bounds.y), (240.0, 48.0));
        assert!(bounds.width > 0.0 && bounds.height > 0.0);
    }

    #[test]
    fn disabling_or_hiding_title_bar_cancels_pending_drag() {
        use super::{TitleBarDragTracker, apply_title_bar_pointer, title_bar_drag_hit};
        for hidden in [false, true] {
            let (mut context, document, bar, _) = title_bar_document();
            let bounds = context.world().layout_box(bar.stable_id()).unwrap();
            let x = bounds.x + bounds.width - 24.0;
            let y = bounds.y + bounds.height / 2.0;
            let mut state = WindowChromeState::default();
            let mut tracker = TitleBarDragTracker::default();
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_down(x, y),
            );
            context
                .update_component(bar, |bar, _| {
                    if hidden {
                        std::sync::Arc::make_mut(&mut bar.style.layout).hidden = true;
                    } else {
                        bar.drag_enabled = false;
                    }
                })
                .unwrap();
            assert!(!title_bar_drag_hit(&context, document, x, y));
            assert_eq!(
                apply_title_bar_pointer(
                    &mut state,
                    &mut tracker,
                    &context,
                    document,
                    &pointer_move(x + 8.0, y)
                ),
                None
            );
            context
                .update_component(bar, |bar, _| {
                    bar.drag_enabled = true;
                    std::sync::Arc::make_mut(&mut bar.style.layout).hidden = false;
                })
                .unwrap();
            assert_eq!(
                apply_title_bar_pointer(
                    &mut state,
                    &mut tracker,
                    &context,
                    document,
                    &pointer_move(x + 16.0, y)
                ),
                None
            );
            assert!(title_bar_drag_hit(&context, document, x, y));
        }
    }

    #[test]
    fn blank_title_bar_pointer_starts_window_drag() {
        use super::{TitleBarDragTracker, apply_title_bar_pointer, title_bar_drag_hit};

        let (context, document, bar, button) = title_bar_document();
        let bounds = context.world().layout_box(bar.stable_id()).unwrap();
        let blank_x = bounds.x + bounds.width - 24.0;
        let blank_y = bounds.y + bounds.height / 2.0;
        assert!(title_bar_drag_hit(&context, document, blank_x, blank_y));

        let button_box = context.world().layout_box(button.stable_id()).unwrap();
        assert!(!title_bar_drag_hit(
            &context,
            document,
            button_box.x + button_box.width / 2.0,
            button_box.y + button_box.height / 2.0,
        ));

        let mut state = WindowChromeState::default();
        let mut tracker = TitleBarDragTracker::default();
        let pressed = apply_title_bar_pointer(
            &mut state,
            &mut tracker,
            &context,
            document,
            &pointer_down(blank_x, blank_y),
        );
        #[cfg(target_os = "macos")]
        assert_eq!(pressed, Some(WindowChromeAction::Drag));
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(pressed, None);
            assert_eq!(
                apply_title_bar_pointer(
                    &mut state,
                    &mut tracker,
                    &context,
                    document,
                    &pointer_move(blank_x + 8.0, blank_y),
                ),
                Some(WindowChromeAction::Drag)
            );
        }
    }

    #[test]
    fn text_input_inside_title_bar_keeps_press_drag_for_selection() {
        use super::{TitleBarDragTracker, apply_title_bar_pointer, title_bar_drag_hit};
        use nana_ui_runtime::{AppContext, AppTitleBar, DocumentId, LayoutViewport, TextInput};

        let document = DocumentId::new(1).unwrap();
        let mut context = AppContext::new();
        let input = context
            .create_component(document, TextInput::new("搜索"))
            .unwrap();
        let bar = context
            .create_component(document, AppTitleBar::new("Nana").center(input.stable_id()))
            .unwrap();
        assert!(context.assemble_app_title_bar(bar).unwrap());
        context
            .layout_document(document, LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);

        let input_box = context.world().layout_box(input.stable_id()).unwrap();
        let x = input_box.x + input_box.width / 2.0;
        let y = input_box.y + input_box.height / 2.0;
        assert!(!title_bar_drag_hit(&context, document, x, y));

        // 输入框子树内按下并拖过阈值不得产生拖窗动作;空白区仍要能拖。
        let blank_x = input_box.x - 12.0;
        let blank_y = input_box.y + input_box.height / 2.0;
        assert!(title_bar_drag_hit(&context, document, blank_x, blank_y));

        let mut state = WindowChromeState::default();
        let mut tracker = TitleBarDragTracker::default();
        assert_eq!(
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_down(x, y),
            ),
            None
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_move(x + 40.0, y),
            ),
            None
        );

        let pressed = apply_title_bar_pointer(
            &mut state,
            &mut tracker,
            &context,
            document,
            &pointer_down(blank_x, blank_y),
        );
        #[cfg(target_os = "macos")]
        assert_eq!(pressed, Some(WindowChromeAction::Drag));
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(pressed, None);
            assert_eq!(
                apply_title_bar_pointer(
                    &mut state,
                    &mut tracker,
                    &context,
                    document,
                    &pointer_move(blank_x + 8.0, blank_y),
                ),
                Some(WindowChromeAction::Drag)
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn window_control_pointer_emits_chrome_action_and_skips_drag() {
        use super::{TitleBarDragTracker, apply_title_bar_pointer, title_bar_drag_hit};
        use nana_ui_runtime::{AppContext, AppTitleBar, DocumentId, LayoutViewport};

        let document = DocumentId::new(1).unwrap();
        let mut context = AppContext::new();
        let bar = context
            .create_component(document, AppTitleBar::new("Nana"))
            .unwrap();
        assert!(context.assemble_app_title_bar(bar).unwrap());
        context
            .layout_document(document, LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);

        let controls = context
            .read(bar, |bar| bar.controls)
            .unwrap()
            .expect("assembled controls");
        let close = context.world().node(controls).unwrap().children[2];
        let close_box = context.world().layout_box(close).unwrap();
        let x = close_box.x + close_box.width / 2.0;
        let y = close_box.y + close_box.height / 2.0;
        assert!(!title_bar_drag_hit(&context, document, x, y));

        let mut state = WindowChromeState::default();
        let mut tracker = TitleBarDragTracker::default();
        assert_eq!(
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_down(x, y),
            ),
            None
        );
        assert_eq!(
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_move(x + 8.0, y),
            ),
            None
        );
        assert_eq!(
            apply_title_bar_pointer(
                &mut state,
                &mut tracker,
                &context,
                document,
                &pointer_up(x, y),
            ),
            Some(WindowChromeAction::Close)
        );

        let mut activation = TitleBarDragTracker::default();
        let mut activation_state = WindowChromeState::default();
        assert_eq!(
            apply_title_bar_pointer(
                &mut activation_state,
                &mut activation,
                &context,
                document,
                &with_activation_click(pointer_down(x, y)),
            ),
            None
        );
        assert_eq!(
            apply_title_bar_pointer(
                &mut activation_state,
                &mut activation,
                &context,
                document,
                &with_activation_click(pointer_up(x, y)),
            ),
            None
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn inset_close_button_owns_its_bounds_but_not_the_frame_corner() {
        use super::{title_bar_drag_hit, title_bar_hits_window_control};
        use nana_ui_core::{TITLE_BAR_HEIGHT, WINDOW_CONTROL_PADDING, WINDOW_CONTROL_WIDTH};
        use nana_ui_runtime::{AppContext, AppTitleBar, DocumentId, LayoutViewport};

        let document = DocumentId::new(1).unwrap();
        let mut context = AppContext::new();
        let bar = context
            .create_component(document, AppTitleBar::new("Nana"))
            .unwrap();
        assert!(context.assemble_app_title_bar(bar).unwrap());
        context
            .layout_document(document, LayoutViewport::new(800.0, 400.0))
            .unwrap();
        context.rebuild_hit_test(document);

        let controls = context
            .read(bar, |bar| bar.controls)
            .unwrap()
            .expect("assembled controls");
        let close = context.world().node(controls).unwrap().children[2];
        let close_box = context.world().layout_box(close).unwrap();
        assert_eq!(close_box.y, (TITLE_BAR_HEIGHT - WINDOW_CONTROL_WIDTH) / 2.0);
        assert_eq!(close_box.height, WINDOW_CONTROL_WIDTH);
        assert_eq!(close_box.width, WINDOW_CONTROL_WIDTH);
        assert_eq!(
            close_box.x + close_box.width,
            800.0 - WINDOW_CONTROL_PADDING
        );

        let corner_x = 800.0 - 1.0;
        let corner_y = 1.0;
        assert!(!title_bar_hits_window_control(
            &context, document, corner_x, corner_y
        ));
        assert!(title_bar_hits_window_control(
            &context,
            document,
            close_box.x + close_box.width / 2.0,
            close_box.y + close_box.height / 2.0
        ));
        assert!(!title_bar_drag_hit(
            &context,
            document,
            close_box.x + close_box.width / 2.0,
            close_box.y + close_box.height / 2.0
        ));
    }
}
