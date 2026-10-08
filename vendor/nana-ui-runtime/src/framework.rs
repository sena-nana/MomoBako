/// Evaluate `$body` with `$C` naming the component type behind a plain
/// editor kind, so each generic editor operation dispatches in one place.
#[cfg(feature = "charts")]
mod charts;
mod choice;
mod events;
mod frame;
mod input;
pub use input::{
    HeadlessInput, InputBindError, InputCounters, InputRouteError, InputRouteOutcome, RoutedEvent,
};
mod hooks;
mod keyboard;
mod lifecycle;
mod modal;
mod registry;
pub(crate) use hooks::{ChoiceHooks, DockHooks, NavigateHooks, WorkspaceHooks, hooked};
pub use hooks::{TypeBehavior, TypeHooks};
pub(crate) use lifecycle::lifecycle_hooks;
pub(crate) use text_edit::Editable;
pub use text_edit::EditableHooks;
mod scroll;
mod selection;
pub use selection::FormValidity;
#[cfg(feature = "rich-text")]
mod rich_text_input;
mod text_area_resize;
mod text_input;
mod value_input;
mod virtualize;
mod virtualize_retained;
use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    future::Future,
    hash::Hash,
    marker::PhantomData,
    ops::Range,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_core::Stream;
use nana_ui_core::{
    ActionId, ActionPickerNavigation, CommandPaletteEvent, ContextPredicate, KeyContext,
    LengthSpec, ThemeMode, TooltipConfig, TooltipPlacement, VirtualListLayout,
    VirtualListMaterializationError, VirtualListMaterializer, VirtualListWindow,
    VirtualTableLayout, VirtualTableMaterializer, VirtualTableWindow, VirtualTreeLayout,
    WorkspaceMutation,
};

#[cfg(test)]
use crate::Dialog;
use crate::{
    AccessibilityAction, AccessibilityActionRequest, ActionMenu, ActionMenuItem, Activate,
    AnimationFrame, Button, Checkbox, Chip, CodeEditing, CommandPalette, ComponentView,
    ContextMenu, ContextMenuEvent, DocumentId, Dropdown, EmptyState, FileDropEvent, FormField,
    FrameProfile, FrameProfiler, FrameStage, HoverCard, IconButton, LabeledValue, List, ListItem,
    ListItemSlots, ModalSlots, ModalSurface, MountState, MutationQueue, NodeKind, NumberChanged,
    NumberInput, OverlayChanged, OverlayHost, Popover, PopoverClosed, PopoverToggled, Progress,
    ProgressCancelled, RangeAdjustment, RangeChanged, RangeDragging, RangeField, RangeInput,
    RovingFocusIntent, ScrollAxes, ScrollChanged, ScrollLaidOut, ScrollMetrics, ScrollOffset,
    ScrollView, ScrollViewportChanged, SearchDropdown, SearchDropdownEvent, SecondaryPress,
    SegmentedControl, SegmentedOption, SegmentedSelectionRequested, Select,
    SettingsCollapsibleCard, SidebarFooterButton, SidebarRow, SidebarSection, SizeChanged,
    StableNodeId, Switch, Table, TableCell, TableRow, Tabs, TextArea, TextChanged, TextClamped,
    TextInput, TextInputState, TextPresenter, TextSelection, ToggleChanged, Tooltip, TreeView,
    UiWorld, UiWorldError, Workspace, XYPad, XYPadDragState, XYPadEvent,
    component_registry::{
        ComponentBindKind, ComponentBindRequest, ComponentRegistry, ComponentTypeId,
        RegisterableComponent, SemanticSpec, alias_entry, registerable_entry, tag_entry,
    },
};

mod assemble;
mod build;
pub(crate) use assemble::AssembledKey;
mod document_text;
mod overlay;
mod reactive;
pub(crate) mod text_edit;
mod text_history;
pub use assemble::AssemblyScope;
pub(crate) use assemble::reconcile_child_order;
pub(crate) use build::UiBuilder;
pub(crate) use overlay::overlay_kind_for_role;
pub use overlay::{
    ActiveRuntimeOverlay, OverlayKey, OverlayPointerDecision, OverlayPointerPhase,
    RuntimeOverlayKind,
};
pub use reactive::MountedView;
pub(crate) use reactive::{bound_view, stage_bound_node};
pub use text_edit::{TextDeleteKind, TextFindScope};
pub use text_history::TextEditOrigin;

const MAX_EVENTS_PER_UPDATE: usize = 16_384;

/// Whether this test run makes [`AppContext::new`] typed; tests that need
/// tag-based binding skip themselves when it does.
#[cfg(test)]
pub(crate) fn typed_builtins_under_test() -> bool {
    std::env::var_os("NANA_TEST_TYPED_BUILTINS").is_some()
}
/// Joins assembly keys into a path ([`AppContext::assembly_path`]). Keys may
/// not contain it, so every keyed node stays reachable by path.
pub const ASSEMBLY_PATH_SEPARATOR: &str = "/";

/// Whether `key` may name a keyed child: non-empty and free of the path
/// separator.
pub(crate) fn valid_assembly_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(ASSEMBLY_PATH_SEPARATOR)
}
pub(crate) const COMPONENT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub trait View: Send + 'static {}

impl<T: Send + 'static> View for T {}

pub(crate) trait EditableText: ComponentView {
    type Change: Send + 'static;
    /// Which plain editor this is; `None` for a composite's search field.
    const PLAIN: Option<text_edit::TextEditorKind> = None;
    fn accepts_input(&self) -> bool;
    fn accepts_selection(&self) -> bool {
        self.accepts_input()
    }
    /// Replace the text of every active selection (single cursor replaces its
    /// own selection; multiple cursors each receive an insertion).
    fn replace_selection(&mut self, text: &str) -> bool;
    fn text_atoms(&self) -> &[crate::TextAtomSpan] {
        &[]
    }
    /// Whether a whole-value edit may land. `TextInput` enforces its length
    /// limit here; editors without a limit accept every value.
    fn accepts_edit_value(&self, _value: &str) -> bool {
        true
    }
    /// The committed number behind the text, for an editor that keeps one
    /// (`NumberInput`). The undo journal records it with the text.
    fn committed_number(&self) -> Option<f64> {
        None
    }
    /// Put back a committed number the undo journal recorded, announcing the
    /// move in the editor's own terms.
    fn restore_committed_number(&mut self, _number: f64, _cx: &mut ViewContext<'_, Self>)
    where
        Self: Sized,
    {
    }
    /// IME commit path: replace only the primary selection's text. This is
    /// the documented multi-cursor IME restriction — composition commits to
    /// the primary cursor alone and other cursors survive via offset
    /// remapping.
    fn commit_ime_text(&mut self, text: &str) -> bool {
        self.state_mut().replace_primary_selection(text)
    }
    /// `composing`: a preedit stands in for the selection, which is kept.
    fn delete_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        self.state_mut()
            .delete_ime_surrounding(before_bytes, after_bytes, composing)
    }
    fn state(&self) -> &TextInputState;
    fn state_mut(&mut self) -> &mut TextInputState;
    fn change(&self) -> Self::Change;
    /// Whether the value lays out across multiple lines.
    fn is_multiline(&self) -> bool {
        false
    }
    /// Code-editor behaviors when the component opted in.
    fn code_editing(&self) -> Option<&CodeEditing> {
        None
    }
    fn set_value(&mut self, value: String) -> bool {
        if self.state().value == value {
            return false;
        }
        self.state_mut().replace_value(value);
        true
    }
}

fn text_changed(state: &TextInputState) -> TextChanged {
    TextChanged {
        value: state.value.clone(),
        selection: state.selection,
    }
}

/// Build a scroll offset that moves one axis and holds the other.
fn scroll_offset_on(axis: nana_ui_core::ScrollbarAxis, offset: f32, hold: f32) -> ScrollOffset {
    match axis {
        nana_ui_core::ScrollbarAxis::Horizontal => ScrollOffset { x: offset, y: hold },
        nana_ui_core::ScrollbarAxis::Vertical => ScrollOffset { x: hold, y: offset },
    }
}

impl EditableText for TextInput {
    type Change = TextChanged;
    const PLAIN: Option<text_edit::TextEditorKind> = Some(text_edit::TextEditorKind::Field);

    fn accepts_input(&self) -> bool {
        !self.disabled && !self.loading && !self.read_only
    }
    fn accepts_selection(&self) -> bool {
        !self.disabled && !self.loading
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        self.replace_selection(text)
    }

    fn commit_ime_text(&mut self, text: &str) -> bool {
        self.commit_limited_ime(text)
    }

    fn accepts_edit_value(&self, value: &str) -> bool {
        TextInput::accepts_edit_value(self, value)
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> TextChanged {
        text_changed(&self.state)
    }
}

impl EditableText for NumberInput {
    type Change = TextChanged;
    const PLAIN: Option<text_edit::TextEditorKind> = Some(text_edit::TextEditorKind::Number);

    fn accepts_input(&self) -> bool {
        self.accepts_input()
    }
    /// A read-only field's draft still selects, so its text can be copied.
    fn accepts_selection(&self) -> bool {
        !self.disabled
    }

    fn committed_number(&self) -> Option<f64> {
        Some(self.value())
    }

    fn restore_committed_number(&mut self, number: f64, cx: &mut ViewContext<'_, Self>) {
        if self.restore_value(number) {
            cx.emit(NumberChanged {
                value: self.value(),
            });
        }
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        number_text(text).is_some_and(|text| self.state.replace_selection(&text))
    }

    fn commit_ime_text(&mut self, text: &str) -> bool {
        number_text(text).is_some_and(|text| self.state.replace_primary_selection(&text))
    }

    /// Whole-value edits (find and replace, transforms) are refused when
    /// they would put a control character in the draft.
    fn accepts_edit_value(&self, value: &str) -> bool {
        !value.chars().any(char::is_control)
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> TextChanged {
        text_changed(&self.state)
    }
}

/// What of `text` a number draft takes. A number never holds control
/// characters: the newline or tab a host reports with Enter or Tab, an IME
/// commit ending in one, or a pasted cell's line break is dropped and the
/// rest kept. `None` when nothing but control characters was offered.
fn number_text(text: &str) -> Option<std::borrow::Cow<'_, str>> {
    if !text.chars().any(char::is_control) {
        return Some(std::borrow::Cow::Borrowed(text));
    }
    let kept: String = text
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    (!kept.is_empty()).then_some(std::borrow::Cow::Owned(kept))
}

impl EditableText for TextArea {
    type Change = TextChanged;
    const PLAIN: Option<text_edit::TextEditorKind> = Some(text_edit::TextEditorKind::Area);

    fn accepts_input(&self) -> bool {
        !self.disabled && !self.read_only
    }
    fn accepts_selection(&self) -> bool {
        !self.disabled
    }

    fn is_multiline(&self) -> bool {
        true
    }

    fn code_editing(&self) -> Option<&CodeEditing> {
        self.code_editing.as_ref()
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        self.replace_selection(text)
    }

    fn text_atoms(&self) -> &[crate::TextAtomSpan] {
        &self.atom_spans
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> TextChanged {
        text_changed(&self.state)
    }
}

impl EditableText for SearchDropdown {
    type Change = SearchDropdownEvent;

    fn accepts_input(&self) -> bool {
        self.opened && !self.inactive()
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        self.replace_selection(text)
    }

    fn commit_ime_text(&mut self, text: &str) -> bool {
        // Composite search surfaces are single-selection and keep `query`
        // synchronized through their own replace path.
        self.replace_selection(text)
    }

    fn delete_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        self.delete_surrounding(before_bytes, after_bytes, composing)
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> SearchDropdownEvent {
        SearchDropdownEvent::Search(self.query.clone())
    }

    fn set_value(&mut self, value: String) -> bool {
        if self.query == value {
            return false;
        }
        let _ = self.set_query(value);
        true
    }
}

impl EditableText for ContextMenu {
    type Change = ContextMenuEvent;

    fn accepts_input(&self) -> bool {
        self.open && self.searchable
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        if !self.state.replace_selection(text) {
            return false;
        }
        self.sync_query_from_state();
        true
    }

    fn commit_ime_text(&mut self, text: &str) -> bool {
        // Searchable menus keep `query` synchronized with the committed
        // state through their own replace path.
        self.replace_selection(text)
    }

    fn delete_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        if !self
            .state
            .delete_ime_surrounding(before_bytes, after_bytes, composing)
        {
            return false;
        }
        self.sync_query_from_state();
        true
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> ContextMenuEvent {
        ContextMenuEvent::Search(Arc::clone(&self.query))
    }

    fn set_value(&mut self, value: String) -> bool {
        if self.query.as_ref() == value {
            return false;
        }
        self.set_query(value);
        true
    }
}

impl EditableText for CommandPalette {
    type Change = CommandPaletteEvent;

    fn accepts_input(&self) -> bool {
        true
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        self.replace_selection(text)
    }

    fn commit_ime_text(&mut self, text: &str) -> bool {
        // Composite palettes are single-selection and keep `query`
        // synchronized through their own replace path.
        self.replace_selection(text)
    }

    fn delete_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        self.delete_surrounding(before_bytes, after_bytes, composing)
    }

    fn state(&self) -> &TextInputState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut TextInputState {
        &mut self.state
    }

    fn change(&self) -> CommandPaletteEvent {
        CommandPaletteEvent::Search(self.query.clone())
    }

    fn set_value(&mut self, value: String) -> bool {
        if self.query == value {
            return false;
        }
        let _ = self.set_query(value);
        true
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Entity<V: View> {
    id: StableNodeId,
    marker: PhantomData<fn() -> V>,
}

/// A typed handle is also the node's stable id.
///
/// Slots on composites (`ListItemSlots`, `DesktopShell`'s regions, ...) hold
/// `StableNodeId` because a slot is heterogeneous — a leading slot may carry an
/// icon, an avatar or a thumbnail. This conversion lets callers hand over an
/// `Entity` directly instead of spelling `.stable_id()` at every assignment.
impl<V: View> From<Entity<V>> for StableNodeId {
    fn from(entity: Entity<V>) -> Self {
        entity.id
    }
}

impl<V: View> Copy for Entity<V> {}

impl<V: View> Clone for Entity<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V: View> Entity<V> {
    pub const fn from_stable_id(id: StableNodeId) -> Self {
        Self {
            id,
            marker: PhantomData,
        }
    }

    pub const fn stable_id(self) -> StableNodeId {
        self.id
    }
}

type BoxedEvent = (StableNodeId, TypeId, Box<dyn Any + Send>);
type ProgramMessage = Box<dyn Any + Send>;
type ErasedEventHandler = Box<
    dyn FnMut(
            &mut dyn Any,
            &dyn Any,
            &mut MutationQueue,
            &mut VecDeque<BoxedEvent>,
            &mut Vec<ProgramMessage>,
            Duration,
        ) -> bool
        + Send,
>;
struct EventHandler {
    key: Option<String>,
    observer: StableNodeId,
    callback: ErasedEventHandler,
}
type ActionHandler = Box<dyn FnMut(&mut AppContext) -> Result<(), FrameworkError> + Send>;

struct RegisteredAction {
    when: ContextPredicate,
    handler: ActionHandler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadingComponent {
    Button,
    Switch,
    Card,
}

#[derive(Debug, Clone, Copy)]
struct TooltipLifecycle {
    overlay: StableNodeId,
    show_at: Option<Duration>,
    open: bool,
}

/// Timer state for a hover-triggered surface ([`crate::HoverCard`]). The
/// surface's own node is the lifecycle key; no overlay node is allocated —
/// the card content rides the trigger's subtree.
#[derive(Debug, Clone, Copy, Default)]
struct HoverCardLifecycle {
    preserve_editor_focus: bool,
    restore_focus: Option<StableNodeId>,
    show_at: Option<Duration>,
    close_at: Option<Duration>,
    open: bool,
}

#[derive(Default)]
struct ComponentLifecycle {
    now: Duration,
    text_area_resizes: HashMap<(DocumentId, u64), StableNodeId, crate::BuildIdHasher>,
    #[cfg(feature = "rich-text")]
    rich_text_presses: HashMap<(DocumentId, u64), StableNodeId, crate::BuildIdHasher>,
    pointer_positions: HashMap<(DocumentId, u64), (f32, f32), crate::BuildIdHasher>,
    tooltips: HashMap<StableNodeId, TooltipLifecycle, crate::BuildIdHasher>,
    #[cfg(feature = "charts")]
    chart_tooltips: HashMap<StableNodeId, StableNodeId, crate::BuildIdHasher>,
    hover_cards: HashMap<StableNodeId, HoverCardLifecycle, crate::BuildIdHasher>,
    loading: HashMap<StableNodeId, LoadingComponent, crate::BuildIdHasher>,
    overlay_pointer_sequences: HashSet<(DocumentId, u64), crate::BuildIdHasher>,
    overlay_outside_presses: HashMap<(DocumentId, u64), (StableNodeId, u64), crate::BuildIdHasher>,
    overlay_activation_tokens: HashMap<StableNodeId, u64, crate::BuildIdHasher>,
    next_overlay_activation_token: u64,
    split_hover_probe_last: HashMap<DocumentId, Duration, crate::BuildIdHasher>,
    /// Per `PaneTree`, the boxes it owns, addressed by its own split / pane id.
    pane_tree_slots: HashMap<StableNodeId, HashMap<Arc<str>, StableNodeId>, crate::BuildIdHasher>,
}

impl ComponentLifecycle {
    fn begin_split_hover_probe(&mut self, document: DocumentId, now: Duration) -> bool {
        const HOVER_PROBE_INTERVAL: Duration = Duration::from_millis(8);
        if let Some(last) = self.split_hover_probe_last.get(&document)
            && now.saturating_sub(*last) < HOVER_PROBE_INTERVAL
        {
            return false;
        }
        self.split_hover_probe_last.insert(document, now);
        true
    }
}

type ActivationFn =
    Arc<dyn Fn(&mut AppContext, StableNodeId) -> Result<bool, FrameworkError> + Send + Sync>;

/// Emit [`SecondaryPress`] on a node whose concrete component type the caller
/// no longer knows. Registered per type when a component is created.
type SecondaryPressFn = Arc<
    dyn Fn(&mut AppContext, StableNodeId, SecondaryPress) -> Result<(), FrameworkError>
        + Send
        + Sync,
>;

/// Emit [`FileDropEvent`] on a node whose concrete component type the caller
/// no longer knows. Registered per type when a component is created.
type FileDropFn = Arc<
    dyn Fn(&mut AppContext, StableNodeId, FileDropEvent) -> Result<(), FrameworkError>
        + Send
        + Sync,
>;

/// Reproject a component view whose concrete type the scheduler no longer
/// knows, via the typed `update_component` pipeline captured at stamp time.
type ChildReprojectFn = fn(&mut AppContext, StableNodeId) -> Result<(), FrameworkError>;

#[derive(Default)]
pub struct ExtensionRegistrar {
    actions: HashMap<ActionId, RegisteredAction>,
    presenters: Vec<Box<dyn TextPresenter>>,
    components: ComponentRegistry,
    activations: HashMap<TypeId, ActivationFn>,
}

impl ExtensionRegistrar {
    pub fn register_action(
        &mut self,
        id: impl Into<ActionId>,
        when: ContextPredicate,
        handler: impl FnMut(&mut AppContext) -> Result<(), FrameworkError> + Send + 'static,
    ) -> Result<(), FrameworkError> {
        insert_action(&mut self.actions, id, when, handler)
    }

    pub fn register_presenter(
        &mut self,
        presenter: Box<dyn TextPresenter>,
    ) -> Result<(), FrameworkError> {
        let name = presenter.name().trim();
        if name.is_empty() {
            return Err(FrameworkError::InvalidPresenter);
        }
        if self
            .presenters
            .iter()
            .any(|existing| existing.name() == name)
        {
            return Err(FrameworkError::DuplicatePresenter(name.to_owned()));
        }
        self.presenters.push(presenter);
        Ok(())
    }

    pub fn register_component<C: RegisterableComponent>(&mut self) -> Result<(), FrameworkError> {
        let (entry, tags) = registerable_entry::<C>()?;
        self.components.insert_with_tags(entry, tags)
    }

    /// Register pointer/keyboard activation for `C`.
    ///
    /// Plugins call this from [`UiExtension::install`] instead of adding a
    /// type branch to [`AppContext::activate_node`].
    pub fn register_activation<C: View>(
        &mut self,
        handler: fn(&mut AppContext, Entity<C>) -> Result<bool, FrameworkError>,
    ) -> Result<(), FrameworkError> {
        let type_id = TypeId::of::<C>();
        if self.activations.contains_key(&type_id) {
            return Err(FrameworkError::DuplicateActivation);
        }
        self.activations.insert(
            type_id,
            Arc::new(move |context, id| handler(context, Entity::from_stable_id(id))),
        );
        Ok(())
    }

    pub fn register_tags(
        &mut self,
        type_id: &'static str,
        tags: &'static [&'static str],
    ) -> Result<(), FrameworkError> {
        let (entry, tags) = tag_entry(type_id, tags)?;
        self.components.insert_with_tags(entry, tags)
    }

    /// Register extra type ids / tags that bind through `C::from_semantic`.
    pub fn register_component_alias<C: RegisterableComponent>(
        &mut self,
        type_id: &'static str,
        tags: &'static [&'static str],
    ) -> Result<(), FrameworkError> {
        let (entry, tags) = alias_entry::<C>(type_id, tags)?;
        self.components.insert_with_tags(entry, tags)
    }

    /// Register an identity only (see [`BuiltinComponents::Typed`]).
    pub(crate) fn register_identity<C: RegisterableComponent>(
        &mut self,
        type_id: &'static str,
        rust_type: Option<TypeId>,
        tags: &'static [&'static str],
    ) -> Result<(), FrameworkError> {
        let (entry, tags) =
            crate::component_registry::identity_entry::<C>(type_id, rust_type, tags)?;
        self.components.insert_with_tags(entry, tags)
    }
}

pub struct ViewContext<'a, V: View> {
    entity: Entity<V>,
    mutations: &'a mut MutationQueue,
    events: &'a mut VecDeque<BoxedEvent>,
    program_messages: &'a mut Vec<ProgramMessage>,
    now: Duration,
    reassemble: bool,
}

impl<V: View> ViewContext<'_, V> {
    /// From an `observe` handler that changed what a composite derives its
    /// children from: run the observer's assembler once the update delivering
    /// this event commits, as an `update_component` write would. Elsewhere the
    /// update already reassembles its own component and this does nothing.
    pub fn reassemble(&mut self) {
        self.reassemble = true;
    }

    pub const fn entity(&self) -> Entity<V> {
        self.entity
    }

    pub fn mutations(&mut self) -> &mut MutationQueue {
        self.mutations
    }

    /// L3 motion handle for this view. Compiles to Motion IR.
    pub fn node(&mut self) -> crate::NodeMotion<'_> {
        self.mutations.node(self.entity.id, self.now)
    }

    pub fn transition(&mut self) -> crate::TransitionBuilder<'_> {
        self.node().transition()
    }

    pub fn emit<E: Send + 'static>(&mut self, event: E) {
        self.events
            .push_back((self.entity.id, TypeId::of::<E>(), Box::new(event)));
    }

    /// Queue a `RuntimeProgram::Message` for the Scene host, keeping only the
    /// latest value of each message type.
    ///
    /// The host delivers the queued messages on the next frame
    /// (`RuntimeProgram::update`), not during the current input handler.
    ///
    /// Coalescing is by Rust type, so it is only what you want when the message
    /// type carries a *state* that supersedes its predecessor — a resize, a
    /// "theme changed", a redraw request. An application whose messages are one
    /// `enum` (the idiomatic shape) has a single type for everything, so two
    /// dispatches in the same frame would collapse into one and the first user
    /// action would be silently lost. Use [`Self::dispatch_program_all`] there.
    pub fn dispatch_program<M: Send + 'static>(&mut self, message: M) {
        let type_id = TypeId::of::<M>();
        self.program_messages
            .retain(|queued| queued.as_ref().type_id() != type_id);
        self.program_messages.push(Box::new(message));
    }

    /// Queue a `RuntimeProgram::Message` without dropping earlier ones.
    ///
    /// Every dispatched message reaches `RuntimeProgram::update` next frame, in
    /// dispatch order. This is the right entry point when one message type
    /// carries distinct user actions — typically a single application `enum`,
    /// where [`Self::dispatch_program`] would keep only the last of them.
    pub fn dispatch_program_all<M: Send + 'static>(&mut self, message: M) {
        self.program_messages.push(Box::new(message));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameworkError {
    World(UiWorldError),
    /// A theme definition failed validation, so nothing was installed.
    ThemeCompile(nana_ui_core::ThemeCompileError),
    MissingView(StableNodeId),
    ViewType(StableNodeId),
    DuplicateAction(ActionId),
    InvalidAction,
    MissingAction(ActionId),
    ActionUnavailable(ActionId),
    DuplicateExtension(String),
    DuplicatePresenter(String),
    DuplicateComponentType(String),
    DuplicateComponentTag(String),
    MissingComponentType(String),
    InvalidComponentType,
    InvalidExtension,
    InvalidPresenter,
    InvalidInput,
    InvalidVirtualization,
    EventOverflow(StableNodeId),
    InvalidComponentValue(StableNodeId),
    InvalidComponentHierarchy {
        parent: StableNodeId,
        child: StableNodeId,
    },
    InvalidListItemSlots {
        item: StableNodeId,
        slot: Option<StableNodeId>,
    },
    InvalidFeedbackSlots {
        parent: StableNodeId,
        slot: Option<StableNodeId>,
    },
    InvalidModalSlots {
        parent: StableNodeId,
        slot: Option<StableNodeId>,
    },
    OverlayActivationTokenExhausted(StableNodeId),
    FrameDidNotSettle,
    DuplicateAssemblyKey {
        parent: StableNodeId,
        key: String,
    },
    DuplicateActivation,
}

impl fmt::Display for FrameworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::World(error) => error.fmt(formatter),
            Self::ThemeCompile(error) => error.fmt(formatter),
            Self::MissingView(id) => write!(formatter, "view {} does not exist", id.get()),
            Self::ViewType(id) => write!(formatter, "view {} has a different type", id.get()),
            Self::DuplicateAction(id) => write!(formatter, "action `{id}` is already registered"),
            Self::InvalidAction => formatter.write_str("action id must not be empty"),
            Self::MissingAction(id) => write!(formatter, "action `{id}` is not registered"),
            Self::ActionUnavailable(id) => {
                write!(formatter, "action `{id}` is unavailable in this context")
            }
            Self::DuplicateExtension(name) => {
                write!(formatter, "extension `{name}` is already installed")
            }
            Self::DuplicatePresenter(name) => {
                write!(formatter, "presenter `{name}` is already registered")
            }
            Self::DuplicateComponentType(name) => {
                write!(formatter, "component type `{name}` is already registered")
            }
            Self::DuplicateComponentTag(tag) => {
                write!(formatter, "component tag `{tag}` is already registered")
            }
            Self::MissingComponentType(name) => {
                write!(formatter, "component type `{name}` is not registered")
            }
            Self::InvalidComponentType => {
                formatter.write_str("component type id must not be empty")
            }
            Self::InvalidExtension => formatter.write_str("extension name must not be empty"),
            Self::InvalidPresenter => formatter.write_str("presenter name must not be empty"),
            Self::InvalidInput => formatter.write_str("input contains a non-finite coordinate"),
            Self::InvalidVirtualization => {
                formatter.write_str("virtualized component state is inconsistent")
            }
            Self::EventOverflow(id) => {
                write!(
                    formatter,
                    "view {} emitted too many events in one update",
                    id.get()
                )
            }
            Self::InvalidComponentValue(id) => {
                write!(
                    formatter,
                    "view {} received an invalid component value",
                    id.get()
                )
            }
            Self::InvalidComponentHierarchy { parent, child } => write!(
                formatter,
                "view {} is not a valid child of component {}",
                child.get(),
                parent.get()
            ),
            Self::InvalidListItemSlots { item, slot } => match slot {
                Some(slot) => write!(
                    formatter,
                    "view {} has an invalid list-item slot {}",
                    item.get(),
                    slot.get()
                ),
                None => write!(
                    formatter,
                    "view {} has duplicate list-item slots",
                    item.get()
                ),
            },
            Self::InvalidFeedbackSlots { parent, slot } => match slot {
                Some(slot) => write!(
                    formatter,
                    "view {} has an invalid feedback child slot {}",
                    parent.get(),
                    slot.get()
                ),
                None => write!(
                    formatter,
                    "view {} has duplicate feedback child slots",
                    parent.get()
                ),
            },
            Self::InvalidModalSlots { parent, slot } => match slot {
                Some(slot) => write!(
                    formatter,
                    "view {} has an invalid modal child slot {}",
                    parent.get(),
                    slot.get()
                ),
                None => write!(
                    formatter,
                    "view {} has duplicate modal child slots",
                    parent.get()
                ),
            },
            Self::OverlayActivationTokenExhausted(host) => write!(
                formatter,
                "overlay activation identity for host {} is exhausted",
                host.get()
            ),
            Self::FrameDidNotSettle => {
                formatter.write_str("runtime frame did not settle within the bounded pass limit")
            }
            Self::DuplicateAssemblyKey { parent, key } => write!(
                formatter,
                "assembly key `{key}` is duplicated under view {}",
                parent.get()
            ),
            Self::DuplicateActivation => {
                formatter.write_str("an activation handler is already registered for this type")
            }
        }
    }
}

impl std::error::Error for FrameworkError {}

impl From<UiWorldError> for FrameworkError {
    fn from(value: UiWorldError) -> Self {
        Self::World(value)
    }
}

/// How much of the built-in component machinery a context installs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuiltinComponents {
    /// Every built-in component can also be built from a tag and attributes
    /// (`bind_semantic`, the Vue path). Links every built-in component.
    #[default]
    Full,
    /// Built-in components are created from their Rust types only (`build`,
    /// `mount`, declarative views). Their ids still stamp nodes and resolve
    /// by tag, but nothing that builds one from a tag is linked, so a program
    /// links only the components it creates.
    Typed,
}

/// Owns typed view state while [`UiWorld`] remains the retained UI authority.
pub struct AppContext {
    world: UiWorld,
    views: HashMap<StableNodeId, Box<dyn Any + Send>, crate::BuildIdHasher>,
    /// Opt-in reproject entry points keyed by node, registered from
    /// [`ComponentView::wants_child_reproject`] when a component view is
    /// stamped. The stored function reprojects through the typed
    /// `update_component` pipeline.
    child_reproject_views: HashMap<StableNodeId, ChildReprojectFn, crate::BuildIdHasher>,
    /// Opt-in reproject when installed metrics change, registered from
    /// [`ComponentView::wants_metrics_reproject`].
    metrics_reproject_views: HashMap<StableNodeId, ChildReprojectFn, crate::BuildIdHasher>,
    /// Opt-in reproject when the installed component recipes change,
    /// registered from [`ComponentView::wants_recipe_reproject`].
    recipe_reproject_views: HashMap<StableNodeId, ChildReprojectFn, crate::BuildIdHasher>,
    /// Opt-in reproject when the host's text backend changes, registered from
    /// [`ComponentView::wants_text_backend_reproject`].
    text_backend_reproject_views: HashMap<StableNodeId, ChildReprojectFn, crate::BuildIdHasher>,
    /// Nodes queued for one child-structure reproject; deduplicated per drain.
    pending_child_reprojects: Vec<StableNodeId>,
    /// Guards reentrant drains while a reproject commits its own mutations.
    draining_child_reprojects: bool,
    event_handlers: HashMap<(StableNodeId, TypeId), Vec<EventHandler>, crate::BuildIdHasher>,
    key_handlers: HashMap<StableNodeId, keyboard::KeyHandler, crate::BuildIdHasher>,
    event_dependencies:
        HashMap<StableNodeId, HashSet<(StableNodeId, TypeId)>, crate::BuildIdHasher>,
    actions: HashMap<ActionId, RegisteredAction>,
    extensions: HashSet<String>,
    components: ComponentRegistry,
    activations: HashMap<TypeId, ActivationFn, crate::BuildIdHasher>,
    secondary_presses: HashMap<TypeId, SecondaryPressFn, crate::BuildIdHasher>,
    file_drops: HashMap<TypeId, FileDropFn, crate::BuildIdHasher>,
    /// [`reproject_erased`] per component type created through this context.
    reprojectors: HashMap<TypeId, ReprojectFn, crate::BuildIdHasher>,
    /// [`ComponentView::BEHAVIOR`] per component type created through this
    /// context.
    behaviors: HashMap<TypeId, hooks::ErasedBehavior, crate::BuildIdHasher>,
    /// ScrollViews to send [`ScrollLaidOut`] after the next layout pass.
    laid_out_notices: HashSet<StableNodeId, crate::BuildIdHasher>,
    /// Texts listening for [`TextClamped`], with the answer last sent.
    clamp_watchers: HashMap<StableNodeId, Option<bool>, crate::BuildIdHasher>,
    /// Nodes listening for [`SizeChanged`], with the size last sent.
    size_watchers: HashMap<StableNodeId, events::SizeWatch, crate::BuildIdHasher>,
    /// Router hooks installed by the types created so far.
    type_hooks: TypeHooks,
    assembled:
        HashMap<StableNodeId, HashMap<String, assemble::AssembledChild>, crate::BuildIdHasher>,
    /// Reverse of `assembled`: a keyed child's declared parent and key. The
    /// declared parent is identity; the world parent is placement, and the
    /// two differ only for nodes in `placed_assembled`.
    assembled_parent: HashMap<StableNodeId, (StableNodeId, String), crate::BuildIdHasher>,
    /// Keyed children [`Self::place_assembled`] moved out from under their
    /// declared parent. Reassembling that parent leaves them where they are;
    /// despawning it despawns them.
    placed_assembled: HashSet<StableNodeId, crate::BuildIdHasher>,
    /// Components whose assembler is running, so the `update_component` calls
    /// an assembler makes do not re-enter it.
    assembling: HashSet<StableNodeId, crate::BuildIdHasher>,
    component_lifecycle: ComponentLifecycle,
    next_id: u64,
    frame_profiler: FrameProfiler,
    last_profile: FrameProfile,
    profiling: bool,
    /// Cross-frame layout memo for [`Self::layout_document_scoped`].
    layout_cache: crate::RetainedLayoutCache,
    /// Nodes recomputed by the last layout pass (relayout + shape scope).
    last_layout_scope: Vec<StableNodeId>,
    /// Full (`force_full`) [`Self::layout_document`] calls on this context.
    layout_full_invocations: usize,
    /// Layout passes on this context, scoped and full.
    layout_invocations: usize,
    /// Cumulative wall time of the four `layout_document_observed` sub-stages
    /// (tooltip positioning, engine, writeback + commit, scroll metrics) over
    /// every layout pass since the last drain. Attribution inside the Layout
    /// stage, which the 13 `FrameStage`s report as one number.
    #[cfg(feature = "benchmark")]
    layout_substage_totals: [Duration; 4],
    program_messages: Vec<ProgramMessage>,
    text_edit: text_edit::TextEditSession,
    /// Undo journals, one per editor node.
    text_histories: text_history::TextHistories,
    /// Input sources bound to this context and what routing keeps per
    /// source.
    input: input::InputState,
    /// Bindings, keyed lists and scopes of views mounted with
    /// [`Self::mount_view`].
    reactive: reactive::ReactiveHost,
}

/// Bookkeeping for multi-click selection inside a text editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextPointerClick {
    pub pointer_id: u64,
    pub node: StableNodeId,
    pub at: std::time::Duration,
    pub x: f32,
    pub y: f32,
    pub count: u8,
}

/// Published list gate state for retained sync.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VirtualListPublished {
    range: Range<usize>,
    len: usize,
    total_extent: f32,
    activity: HashSet<StableNodeId>,
    fingerprint: u64,
}

/// Published table gate state for retained sync.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VirtualTablePublished {
    rows: Range<usize>,
    columns: Range<usize>,
    frozen_rows: Range<usize>,
    frozen_columns: Range<usize>,
    row_len: usize,
    column_len: usize,
    row_extent: f32,
    column_extent: f32,
    activity: HashSet<StableNodeId>,
    fingerprint: u64,
}

/// Application-owned mapping between visible data keys and retained component
/// entities. Positioned materialization also keeps explicitly retained and active
/// editing items, each under a bounded placement container.
#[derive(Debug)]
pub struct VirtualListItems<K, C: ComponentView> {
    materializer: VirtualListMaterializer<K>,
    entities: HashMap<K, Entity<C>>,
    containers: HashMap<K, Entity<crate::Stack>>,
    ime_owner: Option<StableNodeId>,
    published: Option<VirtualListPublished>,
    /// Rows size to their content; see [`Self::measured`].
    measured: bool,
    /// Rows were mounted or moved since the last measurement.
    pending_measure: bool,
}

/// Application-owned visible row/cell identities for a virtual Table. The
/// retained tree contains only the cross-product of the current row and column
/// windows.
#[derive(Debug)]
pub struct VirtualTableItems<R, C> {
    materializer: VirtualTableMaterializer<R, C>,
    rows: HashMap<R, Entity<TableRow>>,
    cells: HashMap<(R, C), Entity<TableCell>>,
    ime_owner: Option<StableNodeId>,
    published: Option<VirtualTablePublished>,
}

impl<R, C> Default for VirtualTableItems<R, C> {
    fn default() -> Self {
        Self {
            materializer: VirtualTableMaterializer::default(),
            rows: HashMap::new(),
            cells: HashMap::new(),
            ime_owner: None,
            published: None,
        }
    }
}

impl<R, C> VirtualTableItems<R, C>
where
    R: Clone + Eq + Hash,
    C: Clone + Eq + Hash,
{
    pub fn mounted_rows(&self) -> &[R] {
        self.materializer.mounted_rows()
    }

    pub fn mounted_columns(&self) -> &[C] {
        self.materializer.mounted_columns()
    }

    pub fn row_entity(&self, key: &R) -> Option<Entity<TableRow>> {
        self.rows.get(key).copied()
    }

    pub fn cell_entity(&self, row: &R, column: &C) -> Option<Entity<TableCell>> {
        self.cells.get(&(row.clone(), column.clone())).copied()
    }

    pub(crate) fn publish_table(
        &mut self,
        layout: &VirtualTableLayout,
        pane: &nana_ui_core::VirtualTableFrozenWindow,
        activity: HashSet<StableNodeId>,
        fingerprint: u64,
    ) {
        self.published = Some(VirtualTablePublished {
            rows: pane.rows.body.range.clone(),
            columns: pane.columns.body.range.clone(),
            frozen_rows: pane.rows.frozen.clone(),
            frozen_columns: pane.columns.frozen.clone(),
            row_len: layout.row_count(),
            column_len: layout.column_count(),
            row_extent: layout.row_layout().total_extent(),
            column_extent: layout.column_layout().total_extent(),
            activity,
            fingerprint,
        });
    }

    pub(crate) fn table_range_unchanged(
        &self,
        layout: &VirtualTableLayout,
        pane: &nana_ui_core::VirtualTableFrozenWindow,
        activity: &HashSet<StableNodeId>,
        fingerprint: u64,
    ) -> bool {
        self.published.as_ref().is_some_and(|published| {
            published.rows == pane.rows.body.range
                && published.columns == pane.columns.body.range
                && published.frozen_rows == pane.rows.frozen
                && published.frozen_columns == pane.columns.frozen
                && published.row_len == layout.row_count()
                && published.column_len == layout.column_count()
                && published.row_extent == layout.row_layout().total_extent()
                && published.column_extent == layout.column_layout().total_extent()
                && published.activity == *activity
                && published.fingerprint == fingerprint
        })
    }
}

/// Application-owned mapping between visible flattened tree keys and retained
/// row entities. Collapsed descendants are not kept in the Runtime tree.
#[derive(Debug)]
pub struct VirtualTreeItems<K, C: ComponentView> {
    items: VirtualListItems<K, C>,
}

impl<K, C: ComponentView> Default for VirtualTreeItems<K, C> {
    fn default() -> Self {
        Self {
            items: VirtualListItems::default(),
        }
    }
}

impl<K, C> VirtualTreeItems<K, C>
where
    K: Clone + Eq + Hash,
    C: ComponentView,
{
    pub fn mounted_keys(&self) -> &[K] {
        self.items.mounted_keys()
    }

    pub fn entity(&self, key: &K) -> Option<Entity<C>> {
        self.items.entity(key)
    }
}

impl<K, C: ComponentView> Default for VirtualListItems<K, C> {
    fn default() -> Self {
        Self {
            materializer: VirtualListMaterializer::default(),
            entities: HashMap::new(),
            containers: HashMap::new(),
            ime_owner: None,
            published: None,
            measured: false,
            pending_measure: false,
        }
    }
}

impl<K, C> VirtualListItems<K, C>
where
    K: Clone + Eq + Hash,
    C: ComponentView,
{
    /// Items whose rows take their content's height instead of the layout
    /// extent. The layout's extents are estimates until
    /// [`AppContext::sync_virtual_list_measured_with`] reads each mounted row
    /// back from the last layout pass; rows whose content wraps, clamps or
    /// expands need no height model in the application.
    pub fn measured() -> Self {
        Self {
            measured: true,
            ..Self::default()
        }
    }

    /// Whether rows were mounted or repositioned since they were last
    /// measured. Request another frame while this holds so the measuring sync
    /// sees their laid-out height.
    pub fn pending_measure(&self) -> bool {
        self.pending_measure
    }

    pub fn mounted_keys(&self) -> &[K] {
        self.materializer.mounted()
    }

    pub fn entity(&self, key: &K) -> Option<Entity<C>> {
        self.entities.get(key).copied()
    }

    pub(crate) fn publish_list(
        &mut self,
        layout: &VirtualListLayout,
        window: &VirtualListWindow,
        activity: HashSet<StableNodeId>,
        fingerprint: u64,
    ) {
        self.published = Some(VirtualListPublished {
            range: window.range.clone(),
            len: layout.len(),
            total_extent: layout.total_extent(),
            activity,
            fingerprint,
        });
    }

    pub(crate) fn list_range_unchanged(
        &self,
        layout: &VirtualListLayout,
        window: &VirtualListWindow,
        activity: &HashSet<StableNodeId>,
        fingerprint: u64,
    ) -> bool {
        self.published.as_ref().is_some_and(|published| {
            published.range == window.range
                && published.len == layout.len()
                && published.total_extent == layout.total_extent()
                && published.activity == *activity
                && published.fingerprint == fingerprint
        })
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AppContext {
    pub fn new() -> Self {
        // `NANA_TEST_TYPED_BUILTINS=1 cargo test -p nana-ui-runtime` runs this
        // crate's suite on typed contexts: a type hook a creation path forgot
        // to install fails the test that needs it.
        #[cfg(test)]
        if typed_builtins_under_test() {
            return Self::typed();
        }
        Self::from_world(UiWorld::new())
    }

    /// Wrap an existing retained tree.
    ///
    /// Hosts that already own node identity (Vue) create nodes on a [`UiWorld`]
    /// then adopt it here. The component ID allocator still starts at 1 and
    /// skips live or retired IDs; do not treat allocated chrome IDs as host
    /// tree identities.
    pub fn from_world(world: UiWorld) -> Self {
        Self::with_builtins(world, &crate::builtin_components::NanaBuiltinComponents)
    }

    /// A context whose built-in components are created from their Rust types
    /// only ([`BuiltinComponents::Typed`]). Call this directly: choosing
    /// between it and [`Self::new`] at run time links both.
    pub fn typed() -> Self {
        Self::with_builtins(
            UiWorld::new(),
            &crate::builtin_components::NanaBuiltinIdentities,
        )
    }

    fn with_builtins(world: UiWorld, builtins: &impl UiExtension) -> Self {
        let mut context = Self {
            world,
            views: HashMap::default(),
            child_reproject_views: HashMap::default(),
            metrics_reproject_views: HashMap::default(),
            recipe_reproject_views: HashMap::default(),
            text_backend_reproject_views: HashMap::default(),
            pending_child_reprojects: Vec::new(),
            draining_child_reprojects: false,
            event_handlers: HashMap::default(),
            key_handlers: HashMap::default(),
            event_dependencies: HashMap::default(),
            actions: HashMap::new(),
            extensions: HashSet::new(),
            components: ComponentRegistry::default(),
            activations: HashMap::default(),
            secondary_presses: HashMap::default(),
            file_drops: HashMap::default(),
            reprojectors: HashMap::default(),
            behaviors: HashMap::default(),
            laid_out_notices: HashSet::default(),
            clamp_watchers: HashMap::default(),
            size_watchers: HashMap::default(),
            type_hooks: TypeHooks::default(),
            assembled: HashMap::default(),
            assembled_parent: HashMap::default(),
            placed_assembled: HashSet::default(),
            assembling: HashSet::default(),
            component_lifecycle: ComponentLifecycle::default(),
            next_id: 1,
            frame_profiler: FrameProfiler::new(),
            last_profile: FrameProfile::default(),
            profiling: false,
            layout_cache: crate::RetainedLayoutCache::default(),
            last_layout_scope: Vec::new(),
            layout_full_invocations: 0,
            layout_invocations: 0,
            #[cfg(feature = "benchmark")]
            layout_substage_totals: [Duration::ZERO; 4],
            program_messages: Vec::new(),
            text_edit: text_edit::TextEditSession::default(),
            text_histories: text_history::TextHistories::default(),
            input: input::InputState::default(),
            reactive: reactive::ReactiveHost::default(),
        };
        context
            .install(builtins)
            .expect("builtin component registry");
        #[cfg(feature = "syntax-highlighting")]
        {
            context
                .install(&crate::HighlightPresentation)
                .expect("default highlight presenter");
        }
        context
    }

    pub fn world(&self) -> &UiWorld {
        &self.world
    }

    /// Whether the last layout pass dropped lines of this text to honour its
    /// `line_clamp` or height: the content is longer than what is shown, so
    /// an "expand" affordance has something to reveal. `None` until the text
    /// has been laid out (or when it is empty or an editor's).
    pub fn text_truncated<C: ComponentView>(&self, text: Entity<C>) -> Option<bool> {
        self.world.text_layout(text.id).map(|(_, layout)| {
            layout
                .overflow
                .contains(nana_text::OverflowFlags::TRUNCATED_LINES)
        })
    }

    /// The key `child` was created under by [`Self::build`] or [`Self::mount`],
    /// when `parent` is the parent it was declared under.
    ///
    /// The assembly key is this framework's author-facing stable-identity
    /// contract — [`FrameworkError::DuplicateAssemblyKey`] names it — so reading
    /// it back gives tooling a handle that survives relayout, reconciliation
    /// and [`Self::place_assembled`]. Nodes created directly with
    /// `create_component` were never keyed and have none; callers must have a
    /// second way to address those.
    pub fn assembly_key(&self, parent: StableNodeId, child: StableNodeId) -> Option<&str> {
        self.assembled_parent
            .get(&child)
            .filter(|(declared, _)| *declared == parent)
            .map(|(_, key)| key.as_str())
    }

    /// `/`-joined assembly keys from the outermost keyed ancestor down to `id`.
    ///
    /// Keyed nodes contribute their key and continue at the parent they were
    /// declared under, wherever they are placed; unkeyed nodes continue at
    /// their world parent. `None` when no ancestor keyed this node.
    pub fn assembly_path(&self, id: StableNodeId) -> Option<String> {
        let mut segments = Vec::new();
        let mut cursor = Some(id);
        let mut steps = 0usize;
        while let Some(node) = cursor {
            cursor = match self.assembled_parent.get(&node) {
                Some((parent, key)) => {
                    segments.push(key.as_str());
                    Some(*parent)
                }
                None => self.world.parent_id(node),
            };
            // Declared and world parents are each acyclic; a malformed mix
            // must still not hang.
            steps += 1;
            if steps > self.world.len() + 1 {
                break;
            }
        }
        if segments.is_empty() {
            return None;
        }
        segments.reverse();
        Some(segments.join(ASSEMBLY_PATH_SEPARATOR))
    }

    /// The node `path` names below `root`: each `/`-separated segment is the
    /// assembly key of a child declared under the previous node. An empty
    /// path is `root` itself.
    pub fn resolve_assembly_path(&self, root: StableNodeId, path: &str) -> Option<StableNodeId> {
        if !self.world.contains(root) {
            return None;
        }
        if path.is_empty() {
            return Some(root);
        }
        path.split(ASSEMBLY_PATH_SEPARATOR)
            .try_fold(root, |parent, key| {
                self.assembled.get(&parent)?.get(key).map(|child| child.id)
            })
    }

    /// [`Self::resolve_assembly_path`] as a typed handle: fails with
    /// [`FrameworkError::MissingView`] naming `root` when nothing is at
    /// `path`, and with [`FrameworkError::ViewType`] when the node there is
    /// another component.
    pub fn resolve_assembly_entity<C: ComponentView>(
        &self,
        root: StableNodeId,
        path: &str,
    ) -> Result<Entity<C>, FrameworkError> {
        let id = self
            .resolve_assembly_path(root, path)
            .ok_or(FrameworkError::MissingView(root))?;
        let entity = Entity::from_stable_id(id);
        self.read(entity, |_| ())?;
        Ok(entity)
    }

    /// Move a keyed node to `parent` without changing its identity.
    ///
    /// The node keeps its key under the parent it was declared under, so
    /// [`Self::assembly_path`] and [`Self::assembled_child`] still find it, and
    /// reassembling that parent neither pulls it back nor duplicates it.
    /// Despawning the declared parent despawns the node wherever it is.
    /// Placing it back under its declared parent ends the arrangement.
    ///
    /// Unkeyed nodes, dead nodes or parents, cycles and cross-document moves
    /// fail before anything changes.
    pub fn place_assembled(
        &mut self,
        node: StableNodeId,
        parent: StableNodeId,
    ) -> Result<(), FrameworkError> {
        let declared = self
            .assembled_parent
            .get(&node)
            .map(|(declared, _)| *declared)
            .ok_or(FrameworkError::InvalidInput)?;
        let snapshot = self
            .world
            .node(node)
            .ok_or(FrameworkError::MissingView(node))?;
        let target = self
            .world
            .node(parent)
            .ok_or(FrameworkError::MissingView(parent))?;
        let cycle = std::iter::successors(Some(parent), |id| self.world.parent_id(*id))
            .any(|ancestor| ancestor == node);
        if cycle || target.document != snapshot.document {
            return Err(FrameworkError::InvalidComponentHierarchy {
                parent,
                child: node,
            });
        }
        if snapshot.parent != Some(parent) {
            let mut mutations = MutationQueue::new();
            mutations.insert(parent, node, None);
            self.commit_mutations(mutations)?;
        }
        if parent == declared {
            self.placed_assembled.remove(&node);
        } else {
            self.placed_assembled.insert(node);
        }
        Ok(())
    }

    /// Replace `parent`'s keyed children and keep the reverse index in step.
    pub(super) fn store_assembled(
        &mut self,
        parent: StableNodeId,
        slots: HashMap<String, assemble::AssembledChild>,
    ) {
        if let Some(previous) = self.assembled.remove(&parent) {
            for child in previous.values() {
                if self
                    .assembled_parent
                    .get(&child.id)
                    .is_some_and(|(declared, _)| *declared == parent)
                {
                    self.assembled_parent.remove(&child.id);
                }
            }
        }
        if slots.is_empty() {
            return;
        }
        for (key, child) in &slots {
            self.assembled_parent
                .insert(child.id, (parent, key.clone()));
        }
        self.assembled.insert(parent, slots);
    }

    /// The key `child` holds under `parent`, to put back with
    /// [`Self::rekey_assembled`] after another node took it.
    pub(crate) fn assembled_key(
        &self,
        parent: StableNodeId,
        child: StableNodeId,
    ) -> Option<AssembledKey> {
        let (declared, key) = self.assembled_parent.get(&child)?;
        let entry = self.assembled.get(declared)?.get(key)?;
        (*declared == parent && entry.id == child).then(|| AssembledKey {
            key: key.clone(),
            child: *entry,
        })
    }

    /// Key `key.child` under `parent` again, displacing whichever node holds
    /// that key now: a kept-alive branch shown again takes its key back from
    /// the branch it replaces.
    pub(crate) fn rekey_assembled(&mut self, parent: StableNodeId, key: AssembledKey) {
        if !self.world.contains(parent) || !self.world.contains(key.child.id) {
            return;
        }
        let slots = self.assembled.entry(parent).or_default();
        if let Some(previous) = slots.insert(key.key.clone(), key.child)
            && previous.id != key.child.id
            && self
                .assembled_parent
                .get(&previous.id)
                .is_some_and(|(declared, held)| *declared == parent && *held == key.key)
        {
            self.assembled_parent.remove(&previous.id);
        }
        self.assembled_parent
            .insert(key.child.id, (parent, key.key));
    }

    /// Whether `id` is a keyed child placed away from its declared parent.
    pub(super) fn is_placed_elsewhere(&self, id: StableNodeId) -> bool {
        self.placed_assembled.contains(&id)
    }

    /// Messages queued by [`ViewContext::dispatch_program`] since the last take.
    /// The Scene host drains these into `RuntimeProgram::update` on the next frame.
    pub fn take_program_messages(&mut self) -> Vec<Box<dyn Any + Send>> {
        std::mem::take(&mut self.program_messages)
    }

    pub fn has_program_messages(&self) -> bool {
        !self.program_messages.is_empty()
    }

    /// Resolve a retained node only when it currently owns the requested view type.
    pub fn view_entity<C: View>(&self, id: StableNodeId) -> Option<Entity<C>> {
        self.views
            .get(&id)
            .is_some_and(|view| view.is::<C>())
            .then(|| Entity::from_stable_id(id))
    }

    fn focused_editor<C: EditableText>(&self, document: DocumentId) -> Option<Entity<C>> {
        let (target, _) = self.world.focused_text_input(document)?;
        self.view_entity(target)
    }

    /// The focused text input's node and its editor type's editing table.
    fn focused_editable(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, &'static text_edit::EditableHooks)> {
        let (target, _) = self.world.focused_text_input(document)?;
        Some((target, self.editable_hooks(target)?))
    }

    /// Mutable Runtime access for compatibility hosts and frame systems that
    /// have not yet been expressed by the higher-level `AppContext` API.
    ///
    /// This is intentionally named as a low-level compatibility boundary: it
    /// must not be used by ordinary application code in place of
    /// `commit_mutations` and the component/event APIs.
    pub fn compat_world_mut(&mut self) -> &mut UiWorld {
        &mut self.world
    }

    /// Commit one validated batch without exposing mutable access to the
    /// retained authority. Compatibility adapters and frame drivers use this
    /// for layout writeback and platform state projection.
    pub fn commit_mutations(
        &mut self,
        mut mutations: MutationQueue,
    ) -> Result<crate::CommitReport, FrameworkError> {
        // Re-appending or re-parking what is already in place: the world
        // skips it, and there is no focus, surface or lifecycle to follow.
        // A structural no-op only matches nodes that exist in a consistent
        // relation, so there is nothing to validate either.
        if !mutations.is_empty() && self.world.is_noop_batch(&mutations) {
            self.collect_child_reprojects();
            self.drain_child_reprojects()?;
            return Ok(crate::CommitReport::unchanged(
                self.world.generation(),
                mutations.len(),
            ));
        }
        self.prepare_surface_closing(&mut mutations);
        self.despawn_placed_with_declared_parents(&mut mutations);
        let previous_focus = mutations
            .as_slice()
            .iter()
            .filter_map(|mutation| match mutation {
                crate::UiMutation::RequestFocus { document, .. } => Some(*document),
                crate::UiMutation::RestoreFocusWithin { root }
                | crate::UiMutation::ParkSubtree { root }
                | crate::UiMutation::DespawnSubtree { root }
                | crate::UiMutation::Detach { id: root }
                | crate::UiMutation::Insert { child: root, .. }
                | crate::UiMutation::SetInteraction { id: root, .. }
                | crate::UiMutation::SetStandardVisual { id: root, .. }
                | crate::UiMutation::SetStyle { id: root, .. }
                | crate::UiMutation::SetOverlayHost { host: root, .. }
                | crate::UiMutation::SetSurfaceOpen { id: root, .. } => {
                    self.world.document_of(*root)
                }
                _ => None,
            })
            .filter_map(|document| self.world.focused(document).map(|node| (document, node)))
            .collect::<HashMap<_, _>>();
        let hover_restore = self.hover_card_focus_before_commit(&mutations, &previous_focus);
        let retired_documents = mutations
            .as_slice()
            .iter()
            .filter_map(|mutation| match mutation {
                crate::UiMutation::ParkSubtree { root }
                | crate::UiMutation::DespawnSubtree { root }
                | crate::UiMutation::Detach { id: root } => self.world.document_of(*root),
                _ => None,
            })
            .collect::<HashSet<_>>();
        let despawned = mutations
            .as_slice()
            .iter()
            .filter_map(|mutation| match mutation {
                crate::UiMutation::DespawnSubtree { root } => Some(*root),
                _ => None,
            })
            .flat_map(|root| self.retained_subtree(root))
            .collect::<HashSet<_>>();
        let deleted_layout_nodes = despawned
            .iter()
            .filter_map(|&id| self.world.document_of(id).map(|document| (document, id)))
            .collect::<HashSet<_>>();
        let written_editors = self.text_histories_written_by(&mutations);
        let (report, parked, inserted) = self
            .world
            .commit_with_mount_lifecycle(mutations)
            .map_err(FrameworkError::from)?;
        self.verify_text_histories(written_editors);
        for (document, root, previous, restore) in hover_restore {
            // Other focus owners (including overlay and scope restoration) win.
            if self
                .world
                .focused(document)
                .is_some_and(|focus| focus != previous)
                || self.hover_card_focus_reachable(root, previous)
            {
                continue;
            }
            if let Some(restore) = restore.filter(|focus| {
                self.overlay_focus_candidate(document, *focus)
                    && self.world.is_overlay_reachable(*focus)
                    && !self.world.motion_blocks_input(*focus)
            }) {
                // Validate against the committed tree: the same batch may have
                // hidden, disabled or removed the previous editor.
                if self.focus_node(document, restore).is_err() {
                    self.clear_focus(document)?;
                }
            } else {
                self.clear_focus(document)?;
            }
        }
        for (document, previous) in previous_focus {
            if let Some(focus) = self.world.focused(document) {
                self.remember_hover_card_entry(document, previous, focus);
            }
            self.seal_blurred_editor_history(document, Some(previous));
        }
        for (document, id) in deleted_layout_nodes {
            self.layout_cache.remove_node(document, id);
        }
        // Views, handlers and assembly records of despawned nodes go with
        // them whichever path queued the despawn.
        if !despawned.is_empty() {
            self.forget_subtree(&despawned);
        }
        for document in retired_documents {
            self.release_empty_document_layout(document);
        }
        // A failed reprojection must not leave the rest unsuspended.
        let mut suspended = Ok(());
        for id in parked {
            suspended = suspended.and(self.suspend(id));
        }
        for id in inserted {
            if self.world.is_mounted(id) {
                self.resume_component_lifecycle(id)?;
            }
        }
        self.collect_child_reprojects();
        self.drain_child_reprojects()?;
        suspended?;
        Ok(report)
    }

    /// Queue opt-in parents whose child structure just changed for one
    /// reproject through the `update_component` pipeline.
    fn collect_child_reprojects(&mut self) {
        for parent in self.world.take_structural_change_parents() {
            if !self.child_reproject_views.contains_key(&parent)
                || self.pending_child_reprojects.contains(&parent)
            {
                continue;
            }
            self.pending_child_reprojects.push(parent);
        }
    }

    /// Run queued reprojects. Reentrant commits made by a reproject append to
    /// the queue and are consumed by the same drain; nodes whose view is
    /// temporarily absent (mid-update or not yet installed by a build) wait
    /// for the next commit instead.
    fn reproject_metrics_views(&mut self) -> Result<(), FrameworkError> {
        let ids: Vec<_> = self.metrics_reproject_views.keys().copied().collect();
        for id in ids {
            let Some(reproject) = self.metrics_reproject_views.get(&id).copied() else {
                continue;
            };
            reproject(self, id)?;
        }
        Ok(())
    }

    /// Reprojects [`ComponentView::wants_text_backend_reproject`] views if the
    /// last text pass saw a new backend. Returns whether any did, since their
    /// projection can move layout the pass already measured against.
    pub(crate) fn reproject_text_backend_views(&mut self) -> Result<bool, FrameworkError> {
        if !self.world.take_text_backend_changed() || self.text_backend_reproject_views.is_empty() {
            return Ok(false);
        }
        let ids: Vec<_> = self.text_backend_reproject_views.keys().copied().collect();
        for id in ids {
            let Some(reproject) = self.text_backend_reproject_views.get(&id).copied() else {
                continue;
            };
            reproject(self, id)?;
        }
        Ok(true)
    }

    /// Same shape as [`Self::reproject_metrics_views`], different trigger.
    fn reproject_recipe_views(&mut self) -> Result<(), FrameworkError> {
        let ids: Vec<_> = self.recipe_reproject_views.keys().copied().collect();
        for id in ids {
            let Some(reproject) = self.recipe_reproject_views.get(&id).copied() else {
                continue;
            };
            reproject(self, id)?;
        }
        Ok(())
    }

    fn drain_child_reprojects(&mut self) -> Result<(), FrameworkError> {
        if self.draining_child_reprojects {
            return Ok(());
        }
        self.draining_child_reprojects = true;
        let result = self.drain_child_reprojects_inner();
        self.draining_child_reprojects = false;
        result
    }

    fn drain_child_reprojects_inner(&mut self) -> Result<(), FrameworkError> {
        loop {
            let pending = std::mem::take(&mut self.pending_child_reprojects);
            if pending.is_empty() {
                return Ok(());
            }
            let mut deferred = Vec::new();
            let mut progressed = false;
            for id in pending {
                let Some(reproject) = self.child_reproject_views.get(&id).copied() else {
                    continue;
                };
                if !self.views.contains_key(&id) {
                    deferred.push(id);
                    continue;
                }
                progressed = true;
                reproject(self, id)?;
            }
            self.pending_child_reprojects = deferred;
            if !progressed {
                // Every entry is waiting on a view no reproject created; leave
                // them pending for a later drain instead of spinning forever.
                return Ok(());
            }
        }
    }

    pub fn focused_text_input(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, crate::TextInputView<'_>)> {
        self.world.focused_text_input(document)
    }

    /// Change the retained theme once and invalidate only computed paint.
    /// Re-applying the active mode defaults is a true no-op.
    ///
    /// Resets palette alphas. Hosts that apply window backdrop follow with
    /// [`Self::set_style_tokens`].
    pub fn set_theme(&mut self, mode: ThemeMode) -> Result<bool, FrameworkError> {
        self.install_theme(nana_ui_core::builtin_theme_arc(mode))
    }

    /// Compile, validate and install a design system.
    ///
    /// This is the primary theme entry point. A definition that fails
    /// validation — a negative radius, a font size of zero, a recipe slot the
    /// author forgot — is rejected here and nothing is installed. Issue #102
    /// §6 calls that fail-closed; the practical version is that a broken theme
    /// produces one error with a token name in it instead of a screen that is
    /// subtly wrong in one corner.
    pub fn set_theme_definition(
        &mut self,
        definition: &nana_ui_core::ThemeDefinition,
    ) -> Result<bool, FrameworkError> {
        let compiled = definition.compile().map_err(FrameworkError::ThemeCompile)?;
        self.install_theme(Arc::new(compiled))
    }

    /// Install an already-compiled design system.
    pub fn set_theme_tokens(
        &mut self,
        theme: Arc<nana_ui_core::CompiledTheme>,
    ) -> Result<bool, FrameworkError> {
        self.install_theme(theme)
    }

    /// Install Style Model tokens, including backdrop alphas on Surface /
    /// Background / Titlebar.
    ///
    /// A convenience over [`Self::set_theme_definition`] for hosts that only
    /// move colour and control metrics: the remaining categories come from the
    /// built-in definition for `mode`. It validates like any other install.
    pub fn set_style_tokens(
        &mut self,
        mode: ThemeMode,
        metrics: nana_ui_core::ThemeMetrics,
        palette: nana_ui_core::SemanticPalette,
        titlebar: nana_ui_core::SemanticColor,
    ) -> Result<bool, FrameworkError> {
        let definition = nana_ui_core::ThemeDefinition::for_mode(mode)
            .with_metrics(metrics)
            .with_palette(palette)
            .with_titlebar(Some(titlebar));
        self.set_theme_definition(&definition)
    }

    /// Commit a theme and reproject whatever the change can reach no other way.
    ///
    /// Two opt-in reprojections, deliberately separate. Metrics reprojection
    /// exists because some geometry is derived at projection time and no
    /// layout-intent field names it. Recipe reprojection exists because a
    /// component recipe decides *which role* a component authors onto its own
    /// node, and only the component writes that node. Keeping them apart is
    /// what stops a palette-only switch — the common case, and the one Issue
    /// #101 §4 measured at zero reprojections — from paying for either.
    fn install_theme(
        &mut self,
        theme: Arc<nana_ui_core::CompiledTheme>,
    ) -> Result<bool, FrameworkError> {
        if *self.world.theme() == *theme {
            return Ok(false);
        }
        let metrics_changed = self.world.theme_metrics() != theme.metrics();
        let recipes_changed = self.world.theme().recipes() != theme.recipes();
        let mut queue = MutationQueue::new();
        queue.set_theme_tokens(theme);
        self.world.commit(queue)?;
        if metrics_changed {
            self.reproject_metrics_views()?;
        }
        if recipes_changed {
            self.reproject_recipe_views()?;
        }
        Ok(true)
    }

    pub fn create_view<V: View>(
        &mut self,
        document: DocumentId,
        kind: NodeKind,
        view: V,
    ) -> Result<Entity<V>, FrameworkError> {
        let id = self.allocate_id();
        let mut queue = MutationQueue::new();
        queue.create(id, document, kind);
        self.world.commit(queue)?;
        self.views.insert(id, Box::new(view));
        Ok(Entity::from_stable_id(id))
    }

    /// Create and atomically project a backend-neutral Nana component.
    pub fn create_component<C: ComponentView>(
        &mut self,
        document: DocumentId,
        mut component: C,
    ) -> Result<Entity<C>, FrameworkError> {
        self.share_layouts(&mut component);
        let id = self.allocate_id();
        let mut queue = MutationQueue::new();
        queue.create(id, document, component.node_kind());
        component.project(id, &self.world, &mut queue);
        self.stamp_component_type::<C>(id, &mut queue);
        self.world.commit(queue)?;
        self.views.insert(id, Box::new(component));
        self.sync_component_lifecycle(id)?;
        Ok(Entity::from_stable_id(id))
    }

    /// Let `component` share its layouts with equal ones already written.
    pub(crate) fn share_layouts<C: ComponentView>(&mut self, component: &mut C) {
        component.share_layouts(&mut |layout| self.world.share_layout(layout));
    }

    /// Bind a typed component view onto an existing world node.
    ///
    /// The node must already exist — hosts such as Vue own identity and must
    /// not allocate IDs through [`Self::create_component`]. This replaces any
    /// previous view at `id`, projects `component` into the retained tree, and
    /// commits internally so [`Self::read`], [`Self::update_component`], and
    /// `assemble_*` can run immediately.
    pub fn bind_component<C: ComponentView>(
        &mut self,
        id: StableNodeId,
        mut component: C,
    ) -> Result<Entity<C>, FrameworkError> {
        self.share_layouts(&mut component);
        if !self.world.contains(id) {
            return Err(FrameworkError::MissingView(id));
        }
        let mut queue = MutationQueue::new();
        component.project(id, &self.world, &mut queue);
        self.stamp_component_type::<C>(id, &mut queue);
        self.commit_mutations(queue)?;
        self.views.insert(id, Box::new(component));
        self.sync_component_lifecycle(id)?;
        Ok(Entity::from_stable_id(id))
    }

    /// Create a component whose view and handlers are retained without making
    /// it a document root. Inserting the entity mounts the complete subtree.
    pub fn create_detached_component<C: ComponentView>(
        &mut self,
        document: DocumentId,
        mut component: C,
    ) -> Result<Entity<C>, FrameworkError> {
        self.share_layouts(&mut component);
        let id = self.allocate_id();
        let mut queue = MutationQueue::new();
        queue.create(id, document, component.node_kind());
        component.project(id, &self.world, &mut queue);
        self.stamp_component_type::<C>(id, &mut queue);
        queue.park_subtree(id);
        self.world.commit(queue)?;
        self.views.insert(id, Box::new(component));
        self.sync_component_lifecycle(id)?;
        Ok(Entity::from_stable_id(id))
    }

    /// Appends `children` to `parent` in order, like one [`Self::append_child`]
    /// each, unless they already end `parent`'s children in that order.
    ///
    /// Assemblers run on every write. Appending an ordered set again moves
    /// each child to the end in turn: it ends where it started, but every
    /// move invalidates layout up the ancestor chain.
    pub(crate) fn append_children(
        &mut self,
        parent: StableNodeId,
        children: &[StableNodeId],
    ) -> Result<(), FrameworkError> {
        let current = &self
            .world
            .node(parent)
            .ok_or(FrameworkError::MissingView(parent))?
            .children;
        if current.ends_with(children) {
            return Ok(());
        }
        let mut queue = MutationQueue::new();
        for child in children {
            queue.insert(parent, *child, None);
        }
        self.commit_mutations(queue)?;
        Ok(())
    }

    pub fn append_child<P: View, C: View>(
        &mut self,
        parent: Entity<P>,
        child: Entity<C>,
    ) -> Result<(), FrameworkError> {
        self.read(parent, |_| ())?;
        self.read(child, |_| ())?;
        let mut queue = MutationQueue::new();
        queue.insert(parent.id, child.id, None);
        self.commit_mutations(queue)?;
        Ok(())
    }

    /// Dispatch a semantic activation through the component's closure-event
    /// path. Disabled buttons do not emit or mutate retained state.
    pub fn activate_button(&mut self, entity: Entity<Button>) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |button| button.disabled || button.loading)
    }

    pub fn activate_chip(&mut self, entity: Entity<Chip>) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |chip| chip.disabled)
    }

    pub fn activate_icon_button(
        &mut self,
        entity: Entity<IconButton>,
    ) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |button| button.disabled)
    }

    pub fn activate_list_item(&mut self, entity: Entity<ListItem>) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |item| item.disabled)
    }

    pub fn activate_hover_card(
        &mut self,
        entity: Entity<HoverCard>,
    ) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |_| false)
    }

    pub fn activate_sidebar_row(
        &mut self,
        entity: Entity<SidebarRow>,
    ) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |row| row.disabled())
    }

    pub fn activate_sidebar_footer_button(
        &mut self,
        entity: Entity<SidebarFooterButton>,
    ) -> Result<bool, FrameworkError> {
        self.activate_component(entity, |button| button.disabled)
    }

    fn enclosing_sidebar_row(&self, id: StableNodeId) -> Option<Entity<SidebarRow>> {
        let mut current = Some(id);
        while let Some(id) = current {
            if self
                .views
                .get(&id)
                .is_some_and(|view| view.is::<SidebarRow>())
            {
                return Some(Entity::from_stable_id(id));
            }
            current = self.world.parent_id(id);
        }
        None
    }

    fn sync_sidebar_row_hover(
        &mut self,
        row: Option<Entity<SidebarRow>>,
        hovered: bool,
    ) -> Result<(), FrameworkError> {
        let Some(row) = row else {
            return Ok(());
        };
        if self.read(row, |row| row.hovered == hovered)? {
            return Ok(());
        }
        self.update_component(row, |row, _| {
            row.hovered = hovered;
        })?;
        Ok(())
    }

    /// Nearest hover-card ancestor: the trigger, its open card content, and
    /// any node in between all resolve to the same card.
    fn enclosing_hover_card(&self, id: StableNodeId) -> Option<StableNodeId> {
        let mut current = Some(id);
        while let Some(id) = current {
            if matches!(
                self.world.standard_visual(id),
                Some(crate::StandardVisual::MenuSurface {
                    kind: crate::MenuSurfaceKind::HoverCard,
                    ..
                })
            ) {
                return Some(id);
            }
            current = self.world.parent_id(id);
        }
        None
    }

    fn enclosing_sidebar_section(&self, id: StableNodeId) -> Option<Entity<SidebarSection>> {
        let mut current = Some(id);
        while let Some(id) = current {
            if self
                .views
                .get(&id)
                .is_some_and(|view| view.is::<SidebarSection>())
            {
                return Some(Entity::from_stable_id(id));
            }
            current = self.world.parent_id(id);
        }
        None
    }

    fn node_is_or_under(&self, root: Option<StableNodeId>, target: StableNodeId) -> bool {
        let Some(root) = root else {
            return false;
        };
        let mut current = Some(target);
        while let Some(id) = current {
            if id == root {
                return true;
            }
            current = self.world.parent_id(id);
        }
        false
    }

    fn sidebar_section_header_hovered(
        &self,
        section: Entity<SidebarSection>,
        hover: Option<StableNodeId>,
    ) -> bool {
        let Some(hover) = hover else {
            return false;
        };
        let Ok((header, body, tools, title_slot, count_slot, disclosure)) =
            self.read(section, |section| {
                (
                    section.header,
                    section.body,
                    section.tools,
                    section.title_slot,
                    section.count_slot,
                    section.disclosure,
                )
            })
        else {
            return false;
        };
        if self.node_is_or_under(body, hover) {
            return false;
        }
        hover == section.stable_id()
            || self.node_is_or_under(header, hover)
            || self.node_is_or_under(tools, hover)
            || self.node_is_or_under(title_slot, hover)
            || self.node_is_or_under(count_slot, hover)
            || self.node_is_or_under(disclosure, hover)
    }

    fn sync_sidebar_section_hover(
        &mut self,
        node: Option<StableNodeId>,
        hover: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        let Some(section) = node.and_then(|id| self.enclosing_sidebar_section(id)) else {
            return Ok(());
        };
        let hovered = self.sidebar_section_header_hovered(section, hover);
        if self.read(section, |section| section.header_hovered == hovered)? {
            return Ok(());
        }
        self.update_component(section, |section, _| {
            section.header_hovered = hovered;
        })?;
        Ok(())
    }

    pub fn activate_sidebar_section(
        &mut self,
        entity: Entity<SidebarSection>,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, |section| !section.collapsible || section.disabled)? {
            return Ok(false);
        }
        let now = self.component_lifecycle.now;
        let id = entity.id;
        self.update_component(entity, |section, cx| {
            let from = section.animation_progress;
            section.state.toggle(now);
            let to = if section.state.expanded() { 1.0 } else { 0.0 };
            if let Some(animation) =
                crate::component_animation_id(crate::component_animation_kinds::SIDEBAR, id)
            {
                cx.mutations().start_animation(
                    crate::AnimationSpec::new(
                        animation,
                        id,
                        now,
                        crate::SidebarSectionState::animation_duration(),
                        COMPONENT_FRAME_INTERVAL,
                        crate::Easing::EaseInOutCubic,
                    )
                    .with_range(
                        crate::MotionValue::Scalar(from),
                        crate::MotionTo::Value(crate::MotionValue::Scalar(to)),
                    )
                    .with_interrupt(crate::MotionInterrupt::Retarget),
                );
            }
            cx.emit(ToggleChanged {
                checked: section.state.expanded(),
            });
        })?;
        Ok(true)
    }

    pub fn activate_settings_collapsible_card(
        &mut self,
        entity: Entity<SettingsCollapsibleCard>,
    ) -> Result<bool, FrameworkError> {
        if self.read(entity, |card| card.disabled)? {
            return Ok(false);
        }
        self.update_component(entity, |card, cx| {
            card.expanded = !card.expanded;
            cx.emit(ToggleChanged {
                checked: card.expanded,
            });
        })?;
        Ok(true)
    }

    pub fn activate_action_menu_item(
        &mut self,
        entity: Entity<ActionMenuItem>,
    ) -> Result<bool, FrameworkError> {
        let activated = self.activate_component(entity, |item| item.disabled)?;
        if activated {
            self.close_owning_menu(entity.id)?;
        }
        Ok(activated)
    }

    /// Picking a command dismisses the menu that offered it, so the caller does
    /// not have to mirror the open state just to close it again.
    fn close_owning_menu(&mut self, item: StableNodeId) -> Result<(), FrameworkError> {
        let mut current = self.world.parent_id(item);
        while let Some(id) = current {
            if let Some(entity) = self.view_entity::<ActionMenu>(id) {
                if self.read(entity, |menu| menu.popover.open)? {
                    self.toggle_action_menu(entity)?;
                }
                return Ok(());
            }
            if let Some(entity) = self.view_entity::<Popover>(id) {
                if self.read(entity, |popover| popover.open)? {
                    self.toggle_popover(entity)?;
                }
                return Ok(());
            }
            current = self.world.parent_id(id);
        }
        Ok(())
    }

    pub fn activate_segmented_option(
        &mut self,
        entity: Entity<SegmentedOption>,
    ) -> Result<bool, FrameworkError> {
        let Some(parent) = self.world.parent_id(entity.id) else {
            return Ok(false);
        };
        if self
            .views
            .get(&parent)
            .is_some_and(|view| view.is::<SegmentedControl>())
        {
            return self.request_segmented_selection(Entity::from_stable_id(parent), entity);
        }
        if self
            .views
            .get(&parent)
            .is_some_and(|view| view.is::<Tabs>())
        {
            return self.activate_tabs_option(Entity::from_stable_id(parent), entity.id);
        }
        Ok(false)
    }

    /// Activate a retained component selected by hit testing without exposing
    /// its concrete Rust type to a platform adapter.
    pub fn activate_node(&mut self, id: StableNodeId) -> Result<bool, FrameworkError> {
        if let Some((root, close_action, busy, _)) = self.modal_action_context(id) {
            if busy {
                return Ok(false);
            }
            if close_action == Some(id) {
                let Some(host) = self.world.parent_id(root) else {
                    return Ok(false);
                };
                return self.request_dialog_close(
                    Entity::from_stable_id(host),
                    nana_ui_core::DialogCloseTrigger::CloseButton,
                );
            }
        }
        let handler = self
            .views
            .get(&id)
            .and_then(|view| self.activations.get(&view.as_ref().type_id()).cloned());
        match handler {
            Some(handler) => handler(self, id),
            None => Ok(false),
        }
    }

    pub fn is_range_field(&self, id: StableNodeId) -> bool {
        self.views
            .get(&id)
            .is_some_and(|view| view.is::<RangeField>())
    }

    pub fn is_xy_pad(&self, id: StableNodeId) -> bool {
        self.views.get(&id).is_some_and(|view| view.is::<XYPad>())
    }

    pub fn pointer_target(&self, document: DocumentId, x: f32, y: f32) -> Option<StableNodeId> {
        self.world.hit_test(document, x, y)
    }

    pub fn set_pointer_hover(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: Option<StableNodeId>,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        self.set_pointer_hover_at(document, pointer_id, target, self.component_lifecycle.now)
    }

    pub fn set_pointer_location(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        position: Option<(f32, f32)>,
    ) {
        let key = (document, pointer_id);
        if let Some(position) = position {
            self.component_lifecycle
                .pointer_positions
                .insert(key, position);
        } else {
            self.component_lifecycle.pointer_positions.remove(&key);
        }
    }

    /// Whether the document still has live retained roots. Host capability
    /// requests use the same UiWorld lifetime authority as Runtime routing.
    pub fn has_document(&self, document: DocumentId) -> bool {
        self.world.has_document(document)
    }

    /// Return the latest host-space pointer sample recorded for a document.
    pub fn pointer_position(&self, document: DocumentId, pointer_id: u64) -> Option<(f32, f32)> {
        self.component_lifecycle
            .pointer_positions
            .get(&(document, pointer_id))
            .copied()
    }

    pub fn set_pointer_hover_at(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: Option<StableNodeId>,
        now: Duration,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        self.component_lifecycle.now = now;
        self.world.animation_now = now;
        let previous = self.world.set_pointer_hover(document, pointer_id, target)?;
        if previous != target {
            if let Some(previous) = previous {
                self.leave_tooltip(previous)?;
            }
            if let Some(target) = target {
                self.enter_tooltip(target, now)?;
            }
            // Hover cards track the pointer across their whole subtree: the
            // trigger and the open card content resolve to the same card, so
            // moving between them never closes it.
            let previous_card = previous.and_then(|id| self.enclosing_hover_card(id));
            let next_card = target.and_then(|id| self.enclosing_hover_card(id));
            match (previous_card, next_card) {
                (Some(card), Some(same)) if card == same => {
                    if let Some(lifecycle) = self.component_lifecycle.hover_cards.get_mut(&card) {
                        lifecycle.close_at = None;
                    }
                }
                (previous_card, next_card) => {
                    if let Some(card) = previous_card {
                        self.leave_hover_card(card, now)?;
                    }
                    if let Some(card) = next_card {
                        self.enter_hover_card(card, now)?;
                    }
                }
            }
            let previous_section = previous
                .and_then(|id| self.enclosing_sidebar_section(id))
                .map(Entity::stable_id);
            let next_section = target
                .and_then(|id| self.enclosing_sidebar_section(id))
                .map(Entity::stable_id);
            if previous_section != next_section {
                self.sync_sidebar_section_hover(previous, target)?;
            }
            self.sync_sidebar_section_hover(target, target)?;
            self.sync_scroll_view_hover(previous, target)?;
            let previous_row = previous.and_then(|id| self.enclosing_sidebar_row(id));
            let next_row = target.and_then(|id| self.enclosing_sidebar_row(id));
            if previous_row.map(Entity::stable_id) != next_row.map(Entity::stable_id) {
                self.sync_sidebar_row_hover(previous_row, false)?;
            }
            self.sync_sidebar_row_hover(next_row, true)?;
        } else if let Some(target) = target {
            self.reposition_follow_cursor_tooltip(target)?;
        }
        #[cfg(feature = "charts")]
        self.sync_chart_hover(previous, target)?;
        Ok(previous)
    }

    pub fn icon_button_tooltip(
        &self,
        entity: Entity<IconButton>,
    ) -> Result<Option<Entity<Tooltip>>, FrameworkError> {
        self.read(entity, |_| ())?;
        Ok(self
            .component_lifecycle
            .tooltips
            .get(&entity.id)
            .map(|tooltip| Entity::from_stable_id(tooltip.overlay)))
    }

    pub fn press_pointer(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: StableNodeId,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        Ok(self.world.press_pointer(document, pointer_id, target)?)
    }

    pub fn release_pointer(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
    ) -> Option<StableNodeId> {
        self.world.release_pointer_press(document, pointer_id)
    }

    /// Revoke capture immediately during host lifecycle cancellation. This
    /// keeps cancellation separate from the component mutation queue while
    /// reusing UiWorld's capture-change publication.
    pub fn release_pointer_capture(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
    ) -> Option<StableNodeId> {
        self.world.release_pointer_capture(document, pointer_id)
    }

    pub fn focus_node(
        &mut self,
        document: DocumentId,
        target: StableNodeId,
    ) -> Result<bool, FrameworkError> {
        if !self
            .world
            .interaction(target)
            .is_some_and(|interaction| interaction.focusable)
            || self.confirm_busy_action_subtree(target)
        {
            return Ok(false);
        }
        // A numeric draft settles before focus leaves it, so a half-typed value
        // never survives as the visible text of an unfocused field.
        if self.world.focused(document) != Some(target) {
            self.settle_focused_number_input(document)?;
        }
        if self.is_segmented_option_node(target) {
            let Some(parent) = self.world.parent_id(target) else {
                return Ok(false);
            };
            if self
                .views
                .get(&parent)
                .is_some_and(|view| view.is::<SegmentedControl>())
            {
                let control = Entity::<SegmentedControl>::from_stable_id(parent);
                let mut next = self.read(control, Clone::clone)?;
                let target_changed = next.focus_target != Some(target);
                let focus_changed = self.world.focused(document) != Some(target);
                if !target_changed && !focus_changed {
                    return Ok(false);
                }
                next.focus_target = Some(target);
                let mut mutations = MutationQueue::new();
                next.project(parent, &self.world, &mut mutations);
                mutations.request_focus(document, Some(target));
                self.commit_mutations(mutations)?;
                self.views.insert(parent, Box::new(next));
                return Ok(true);
            }
            if self
                .views
                .get(&parent)
                .is_some_and(|view| view.is::<Tabs>())
            {
                let tabs = Entity::<Tabs>::from_stable_id(parent);
                let Some(value) = self.tabs_option_value(tabs, target)? else {
                    return Ok(false);
                };
                let mut next = self.read(tabs, Clone::clone)?;
                let target_changed = next.focus.as_ref() != Some(&value);
                let focus_changed = self.world.focused(document) != Some(target);
                if !target_changed && !focus_changed {
                    return Ok(false);
                }
                next.focus = Some(value);
                let mut mutations = MutationQueue::new();
                next.project(parent, &self.world, &mut mutations);
                mutations.request_focus(document, Some(target));
                self.commit_mutations(mutations)?;
                self.views.insert(parent, Box::new(next));
                return Ok(true);
            }
            return Ok(false);
        }
        if self.world.focused(document) == Some(target) {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.request_focus(document, Some(target));
        self.commit_mutations(mutations)?;
        Ok(true)
    }

    pub fn clear_focus(&mut self, document: DocumentId) -> Result<bool, FrameworkError> {
        let previous_focus = self.world.focused(document);
        if previous_focus.is_none() {
            return Ok(false);
        }
        self.settle_focused_number_input(document)?;
        let mut mutations = MutationQueue::new();
        mutations.request_focus(document, None);
        self.commit_mutations(mutations)?;
        Ok(true)
    }

    /// Apply an assistive-technology action through the same typed component
    /// mutations used by pointer and keyboard input.
    pub fn apply_accessibility_action(
        &mut self,
        document: DocumentId,
        request: AccessibilityActionRequest,
    ) -> Result<bool, FrameworkError> {
        if self.world.document_of(request.target) != Some(document) {
            return Ok(false);
        }
        match request.action {
            AccessibilityAction::Click => {
                if self.activate_node(request.target)? {
                    return Ok(true);
                }
                // Text fields have no Activate handler; Click means focus so
                // the IME / TalkBack editing session can attach.
                if self.view_entity::<TextInput>(request.target).is_some()
                    || self.view_entity::<TextArea>(request.target).is_some()
                    || self.view_entity::<NumberInput>(request.target).is_some()
                {
                    return self.focus_node(document, request.target);
                }
                Ok(false)
            }
            AccessibilityAction::Focus => self.focus_node(document, request.target),
            AccessibilityAction::SetValue(value) => {
                if let Some(entity) = self.view_entity::<TextInput>(request.target) {
                    return self.set_editable_value(entity, value);
                }
                if let Some(entity) = self.view_entity::<TextArea>(request.target) {
                    return self.set_editable_value(entity, value);
                }
                if let Some(entity) = self.view_entity::<NumberInput>(request.target) {
                    return self.set_number_text(entity, &value);
                }
                if let Some(entity) = self.view_entity::<SearchDropdown>(request.target) {
                    return self.set_editable_value(entity, value);
                }
                if let Some(entity) = self.view_entity::<CommandPalette>(request.target) {
                    return self.set_editable_value(entity, value);
                }
                if let Some(entity) = self.view_entity::<ContextMenu>(request.target) {
                    return self.set_editable_value(entity, value);
                }
                if self
                    .views
                    .get(&request.target)
                    .is_some_and(|view| view.is::<RangeField>())
                {
                    return value
                        .parse::<f64>()
                        .ok()
                        .map(|value| {
                            self.set_range_value(Entity::from_stable_id(request.target), value)
                        })
                        .transpose()
                        .map(|changed| changed.unwrap_or(false));
                }
                Ok(false)
            }
            AccessibilityAction::SetSelection(selection) => {
                if let Some(entity) = self.view_entity::<TextInput>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                if let Some(entity) = self.view_entity::<TextArea>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                if let Some(entity) = self.view_entity::<NumberInput>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                if let Some(entity) = self.view_entity::<SearchDropdown>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                if let Some(entity) = self.view_entity::<CommandPalette>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                if let Some(entity) = self.view_entity::<ContextMenu>(request.target) {
                    return self.set_editable_selection(entity, selection);
                }
                Ok(false)
            }
        }
    }

    fn activate_component<C: ComponentView>(
        &mut self,
        entity: Entity<C>,
        disabled: impl FnOnce(&C) -> bool,
    ) -> Result<bool, FrameworkError> {
        if let Some((root, close_action, busy, intent)) = self.modal_action_context(entity.id) {
            if busy {
                return Ok(false);
            }
            if close_action == Some(entity.id) {
                let Some(host) = self.world.parent_id(root) else {
                    return Ok(false);
                };
                return self.request_dialog_close(
                    Entity::from_stable_id(host),
                    nana_ui_core::DialogCloseTrigger::CloseButton,
                );
            }
            if let Some(intent) = intent {
                self.update_component(
                    Entity::<crate::ConfirmDialog>::from_stable_id(root),
                    |_confirm, cx| cx.emit(intent),
                )?;
                return Ok(true);
            }
        }
        if self.read(entity, disabled)? {
            return Ok(false);
        }
        self.update_component(entity, |_component, cx| cx.emit(Activate))?;
        Ok(true)
    }

    pub fn toggle_checkbox(&mut self, entity: Entity<Checkbox>) -> Result<bool, FrameworkError> {
        if self.read(entity, |checkbox| checkbox.disabled)? {
            return Ok(false);
        }
        self.update_component(entity, |checkbox, cx| {
            checkbox.checked = !checkbox.checked;
            cx.emit(ToggleChanged {
                checked: checkbox.checked,
            });
        })?;
        Ok(true)
    }

    pub fn toggle_switch(&mut self, entity: Entity<Switch>) -> Result<bool, FrameworkError> {
        if self.read(entity, |switch| switch.disabled || switch.loading)? {
            return Ok(false);
        }
        self.update_component(entity, |switch, cx| {
            switch.checked = !switch.checked;
            cx.emit(ToggleChanged {
                checked: switch.checked,
            });
        })?;
        Ok(true)
    }

    /// Route a backend-neutral table navigation intent from current focus.
    pub fn navigate_focused_table(
        &mut self,
        document: DocumentId,
        navigation: crate::TableNavigation,
        page_rows: usize,
    ) -> Result<bool, FrameworkError> {
        let mut current = self.world.focused(document);
        while let Some(id) = current {
            if self.views.get(&id).is_some_and(|view| view.is::<Table>()) {
                return self.navigate_table(Entity::from_stable_id(id), navigation, page_rows);
            }
            current = self.world.parent_id(id);
        }
        Ok(false)
    }

    /// Move table focus using backend-neutral navigation intent. The retained
    /// hierarchy is the row/cell authority; no parallel selection matrix is
    /// stored by the component or platform adapter.
    pub fn navigate_table(
        &mut self,
        table: Entity<Table>,
        navigation: crate::TableNavigation,
        page_rows: usize,
    ) -> Result<bool, FrameworkError> {
        self.read(table, |_| ())?;
        let table_node = self
            .world
            .node(table.id)
            .ok_or(FrameworkError::MissingView(table.id))?;
        let rows = table_node
            .children
            .iter()
            .filter_map(|row| {
                let row = self.world.node(*row)?;
                matches!(&row.kind, NodeKind::Element { tag } if tag == "tr").then(|| {
                    row.children
                        .into_iter()
                        .filter(|cell| {
                            self.world.node(*cell).is_some_and(|cell| {
                                matches!(&cell.kind, NodeKind::Element { tag } if tag == "td" || tag == "th")
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .filter(|cells| !cells.is_empty())
            .collect::<Vec<_>>();
        let column_count = rows.iter().map(Vec::len).max().unwrap_or_default();
        if rows.is_empty() || column_count == 0 {
            return Ok(false);
        }
        let focused = self.world.focused(table_node.document);
        let current = focused.and_then(|focused| {
            rows.iter().enumerate().find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| *cell == focused)
                    .map(|column| crate::TableCursor { row, column })
            })
        });
        let mut cursor = current.unwrap_or(crate::TableCursor { row: 0, column: 0 });
        let moved =
            current.is_none() || cursor.navigate(navigation, rows.len(), column_count, page_rows);
        if !moved {
            return Ok(false);
        }
        cursor.column = cursor.column.min(rows[cursor.row].len() - 1);
        let cell = rows[cursor.row][cursor.column];
        self.update_component(table, |_table, cx| {
            cx.mutations()
                .request_focus(table_node.document, Some(cell));
            cx.emit(crate::TableCellFocused {
                row: cursor.row,
                column: cursor.column,
                cell,
            });
        })?;
        Ok(true)
    }

    pub fn read<V: View, R>(
        &self,
        entity: Entity<V>,
        read: impl FnOnce(&V) -> R,
    ) -> Result<R, FrameworkError> {
        if !self.world.contains(entity.id) {
            return Err(FrameworkError::MissingView(entity.id));
        }
        let view = self
            .views
            .get(&entity.id)
            .ok_or(FrameworkError::MissingView(entity.id))?
            .downcast_ref::<V>()
            .ok_or(FrameworkError::ViewType(entity.id))?;
        Ok(read(view))
    }

    fn inherit_segmented_option_surface<T: 'static>(&self, id: StableNodeId, staged: &mut T) {
        if TypeId::of::<T>() != TypeId::of::<SegmentedOption>() {
            return;
        }
        let Some(parent) = self.world.parent_id(id) else {
            return;
        };
        // Tabs own their options directly, with the tab chrome.
        let parent = self.views.get(&parent);
        let surface = parent
            .and_then(|view| view.downcast_ref::<SegmentedControl>())
            .map(SegmentedControl::option_surface)
            .or_else(|| {
                parent
                    .and_then(|view| view.downcast_ref::<crate::Tabs>())
                    .map(crate::Tabs::option_surface)
            });
        let Some((size, chrome, fill)) = surface else {
            return;
        };
        // SAFETY: `TypeId` matched `SegmentedOption`.
        let option = unsafe { &mut *std::ptr::from_mut(staged).cast::<SegmentedOption>() };
        option.synchronize_surface(size, chrome, fill);
    }

    /// Update typed state, deliver closure events, then atomically commit all
    /// retained-tree mutations produced by the update.
    pub fn update<V: View, R>(
        &mut self,
        entity: Entity<V>,
        update: impl FnOnce(&mut V, &mut ViewContext<'_, V>) -> R,
    ) -> Result<R, FrameworkError> {
        self.update_inner(entity, update, |_view, _world, _mutations| {})
    }

    /// Replaces a component's application-owned props, keeping the interaction
    /// state the runtime owns.
    ///
    /// This is the entry point for the "rebuild the component from application
    /// state" pattern. Assigning over the component inside
    /// [`Self::update_component`] overwrites runtime-owned fields too, which is
    /// how a refresh ends up closing an open menu or resetting a caret; this
    /// routes through [`ComponentView::reconcile`], where each component
    /// decides what survives.
    pub fn set_component<C: ComponentView>(
        &mut self,
        entity: Entity<C>,
        next: C,
    ) -> Result<(), FrameworkError> {
        self.update_component(entity, |component, _| component.reconcile(next))
    }

    /// Update component state and project the final state after all closure
    /// events emitted by the update have been delivered.
    ///
    /// An update that leaves the component equal to what it was, emitted no
    /// event and queued no mutation that changes the tree is a no-op: it
    /// returns without projecting, committing, or running lifecycle and
    /// assemblers. Program messages it dispatched are still delivered. Rewriting
    /// every row of a list with the values it already has therefore costs a
    /// clone and a comparison per component, so applications do not need to
    /// fingerprint rows to skip unchanged ones. Use
    /// [`Self::reproject_component`] to project an unchanged component again
    /// against a world that moved under it.
    pub fn update_component<C: ComponentView, R>(
        &mut self,
        entity: Entity<C>,
        update: impl FnOnce(&mut C, &mut ViewContext<'_, C>) -> R,
    ) -> Result<R, FrameworkError> {
        self.update_component_inner(entity, update, Projection::IfChanged)
    }

    /// Project `entity` again and commit whatever changed, even though its own
    /// data did not.
    ///
    /// For state a component derives in `project` from the world rather than
    /// from its fields — child structure, installed metrics, recipes, the
    /// text backend. [`Self::update_component`] with an empty closure no
    /// longer does this.
    pub fn reproject_component<C: ComponentView>(
        &mut self,
        entity: Entity<C>,
    ) -> Result<(), FrameworkError> {
        self.update_component_inner(entity, |_, _| {}, Projection::Always)
    }

    fn update_component_inner<C: ComponentView, R>(
        &mut self,
        entity: Entity<C>,
        update: impl FnOnce(&mut C, &mut ViewContext<'_, C>) -> R,
        projection: Projection,
    ) -> Result<R, FrameworkError> {
        if !self.world.contains(entity.id) {
            return Err(FrameworkError::MissingView(entity.id));
        }
        let boxed = self
            .views
            .remove(&entity.id)
            .ok_or(FrameworkError::MissingView(entity.id))?;
        let Some(component) = boxed.downcast_ref::<C>() else {
            self.views.insert(entity.id, boxed);
            return Err(FrameworkError::ViewType(entity.id));
        };
        let mut staged = component.clone();
        let mut mutations = MutationQueue::new();
        let mut events = VecDeque::new();
        let mut program_messages = std::mem::take(&mut self.program_messages);
        let result = update(
            &mut staged,
            &mut ViewContext {
                entity,
                mutations: &mut mutations,
                events: &mut events,
                program_messages: &mut program_messages,
                now: self.component_lifecycle.now,
                reassemble: false,
            },
        );
        self.inherit_segmented_option_surface(entity.id, &mut staged);
        let mut projected = false;
        if projection == Projection::IfChanged
            && events.is_empty()
            && self.world.is_noop_batch(&mutations)
            && staged == *component
        {
            // `PartialEq` cannot see what these read, so project and look.
            if C::ALWAYS_REPROJECT {
                staged.project(entity.id, &self.world, &mut mutations);
                projected = true;
            }
            if !projected || self.world.is_noop_batch(&mutations) {
                self.program_messages = program_messages;
                self.views.insert(entity.id, boxed);
                return Ok(result);
            }
        }
        let delivered = self.deliver_events(
            entity.id,
            &mut staged,
            &mut mutations,
            &mut events,
            &mut program_messages,
        );
        self.program_messages = program_messages;
        if delivered.is_ok() && !projected {
            staged.project(entity.id, &self.world, &mut mutations);
        }
        let commit =
            delivered.and_then(|observers| self.commit_mutations(mutations).map(|_| observers));
        if commit.is_ok() {
            self.views.insert(entity.id, Box::new(staged));
        } else {
            self.views.insert(entity.id, boxed);
        }
        let observers = commit?;
        let suspended = if self.world.is_mounted(entity.id) {
            Ok(())
        } else {
            self.suspend(entity.id)
        };
        let own = suspended.and_then(|()| {
            if projection == Projection::ProjectOnly {
                return Ok(());
            }
            self.sync_component_lifecycle(entity.id)?;
            self.run_component_assembler(entity.id, TypeId::of::<C>())
        });
        // Observer handlers already changed their components; their chrome
        // follows even when this component's own follow-up fails.
        let observed = self.follow_up_observers(entity.id, observers);
        own.and(observed)?;
        Ok(result)
    }

    /// Projects the components observer handlers changed in place, and
    /// reconciles the children a composite derives from its own props.
    ///
    /// Observer handlers mutate their view directly rather than through a
    /// staged `update_component`, so nothing else projects that state: a
    /// later write that leaves the component equal takes the no-op path.
    ///
    /// Components like `Chip`, `PathField` or `DesktopShell` own child nodes
    /// that follow their fields. Requiring the application to remember a
    /// matching `assemble_*` after every write made stale chrome a silent
    /// failure, so the write itself drives it. Assemblers are idempotent and
    /// return early when nothing changed; the guard keeps the
    /// `update_component` calls they make from re-entering.
    fn follow_up_observers(
        &mut self,
        source: StableNodeId,
        observers: Vec<TouchedObserver>,
    ) -> Result<(), FrameworkError> {
        let mut outcome = Ok(());
        for observer in observers {
            if observer.id == source || !self.world.contains(observer.id) {
                continue;
            }
            let followed = match self.reprojector(observer.type_id) {
                Some(reproject) => reproject(
                    self,
                    observer.id,
                    if observer.reassemble {
                        Projection::Always
                    } else {
                        Projection::ProjectOnly
                    },
                ),
                None if observer.reassemble => {
                    self.run_component_assembler(observer.id, observer.type_id)
                }
                None => Ok(()),
            };
            outcome = outcome.and(followed);
        }
        outcome
    }

    /// Suspends a parked component's lifecycle and projects whatever that
    /// changed in its view.
    fn suspend(&mut self, id: StableNodeId) -> Result<(), FrameworkError> {
        if !self.suspend_component_lifecycle(id) {
            return Ok(());
        }
        self.reproject_view(id, Projection::ProjectOnly)
    }

    /// Projects the view at `id`, whatever its type.
    pub(super) fn reproject_view(
        &mut self,
        id: StableNodeId,
        projection: Projection,
    ) -> Result<(), FrameworkError> {
        let reproject = self
            .views
            .get(&id)
            .and_then(|view| self.reprojector(view.as_ref().type_id()));
        match reproject {
            Some(reproject) => reproject(self, id, projection),
            None => Ok(()),
        }
    }

    /// The type-erased [`Self::reproject_component`] for a component type,
    /// known once any instance of it was created or its type registered.
    fn reprojector(&self, type_id: TypeId) -> Option<ReprojectFn> {
        self.reprojectors.get(&type_id).copied().or_else(|| {
            self.components
                .get_by_rust(type_id)
                .and_then(|entry| entry.reproject)
        })
    }

    fn run_component_assembler(
        &mut self,
        id: StableNodeId,
        type_id: TypeId,
    ) -> Result<(), FrameworkError> {
        let assembler = self.behaviors.get(&type_id).and_then(|b| b.assembler);
        self.run_assembler(id, assembler)
    }

    /// [`TypeBehavior::slot_assembler`], for the view layer.
    pub(crate) fn run_slot_assembler(
        &mut self,
        id: StableNodeId,
        type_id: TypeId,
    ) -> Result<(), FrameworkError> {
        let assembler = self.behaviors.get(&type_id).and_then(|b| b.slot_assembler);
        self.run_assembler(id, assembler)
    }

    /// What assembles a composite a view just built: its slot assembler, or
    /// the assembler that otherwise runs only after a write.
    pub(crate) fn run_built_assembler(
        &mut self,
        id: StableNodeId,
        type_id: TypeId,
    ) -> Result<(), FrameworkError> {
        let behavior = self.behaviors.get(&type_id);
        let assembler = behavior.and_then(|b| b.slot_assembler.or(b.assembler));
        self.run_assembler(id, assembler)
    }

    fn run_assembler(
        &mut self,
        id: StableNodeId,
        assembler: Option<fn(&mut AppContext, StableNodeId) -> Result<bool, FrameworkError>>,
    ) -> Result<(), FrameworkError> {
        let Some(assembler) = assembler else {
            return Ok(());
        };
        if !self.world.contains(id) || !self.assembling.insert(id) {
            return Ok(());
        }
        let outcome = assembler(self, id);
        self.assembling.remove(&id);
        outcome.map(|_| ())
    }

    fn update_inner<V: View, R>(
        &mut self,
        entity: Entity<V>,
        update: impl FnOnce(&mut V, &mut ViewContext<'_, V>) -> R,
        project: impl FnOnce(&V, &UiWorld, &mut MutationQueue),
    ) -> Result<R, FrameworkError> {
        if !self.world.contains(entity.id) {
            return Err(FrameworkError::MissingView(entity.id));
        }
        let mut boxed = self
            .views
            .remove(&entity.id)
            .ok_or(FrameworkError::MissingView(entity.id))?;
        if !boxed.is::<V>() {
            self.views.insert(entity.id, boxed);
            return Err(FrameworkError::ViewType(entity.id));
        }
        let view = boxed
            .downcast_mut::<V>()
            .expect("view type was checked before update");
        let mut mutations = MutationQueue::new();
        let mut events = VecDeque::new();
        let mut program_messages = std::mem::take(&mut self.program_messages);
        let result = update(
            view,
            &mut ViewContext {
                entity,
                mutations: &mut mutations,
                events: &mut events,
                program_messages: &mut program_messages,
                now: self.component_lifecycle.now,
                reassemble: false,
            },
        );
        self.inherit_segmented_option_surface(entity.id, view);
        let delivered = self.deliver_events(
            entity.id,
            view,
            &mut mutations,
            &mut events,
            &mut program_messages,
        );
        self.program_messages = program_messages;
        if delivered.is_ok() {
            project(view, &self.world, &mut mutations);
        }
        let commit =
            delivered.and_then(|observers| self.commit_mutations(mutations).map(|_| observers));
        self.views.insert(entity.id, boxed);
        let observers = commit?;
        if !self.world.is_mounted(entity.id) {
            self.suspend(entity.id)?;
        }
        self.follow_up_observers(entity.id, observers)?;
        Ok(result)
    }

    pub fn remove_view<V: View>(&mut self, entity: Entity<V>) -> Result<V, FrameworkError> {
        self.read(entity, |_| ())?;
        let boxed = self
            .views
            .remove(&entity.id)
            .expect("validated view must remain present");
        self.despawn_node(entity.id)?;
        boxed
            .downcast::<V>()
            .map(|view| *view)
            .map_err(|_| FrameworkError::ViewType(entity.id))
    }

    pub(crate) fn attach_child(
        &mut self,
        parent: StableNodeId,
        child: StableNodeId,
    ) -> Result<(), FrameworkError> {
        if !self.world.contains(parent) {
            return Err(FrameworkError::MissingView(parent));
        }
        if !self.world.contains(child) {
            return Err(FrameworkError::MissingView(child));
        }
        let mut queue = MutationQueue::new();
        queue.insert(parent, child, None);
        self.commit_mutations(queue)?;
        Ok(())
    }

    pub(super) fn despawn_node(&mut self, id: StableNodeId) -> Result<(), FrameworkError> {
        if !self.world.contains(id) {
            return Err(FrameworkError::MissingView(id));
        }
        let mut subtree = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let snapshot = self
                .world
                .node(current)
                .ok_or(FrameworkError::MissingView(current))?;
            stack.extend(snapshot.children.iter().rev().copied());
            subtree.push(current);
        }
        let mut queue = MutationQueue::new();
        queue.despawn_subtree(id);
        self.commit_mutations(queue)?;
        let removed = subtree.iter().copied().collect::<HashSet<_>>();
        self.forget_subtree(&removed);
        Ok(())
    }

    fn release_empty_document_layout(&mut self, document: DocumentId) {
        if !self.world.has_document_roots(document) {
            self.layout_cache.remove_document(document);
            self.world.clear_document_viewport(document);
        }
    }

    /// A keyed child placed away from its declared parent is still that
    /// parent's: when the batch despawns the parent, it despawns the child
    /// too, and anything that child declared elsewhere in turn.
    fn despawn_placed_with_declared_parents(&self, mutations: &mut MutationQueue) {
        if self.placed_assembled.is_empty() {
            return;
        }
        let mut roots: HashSet<StableNodeId> = mutations
            .as_slice()
            .iter()
            .filter_map(|mutation| match mutation {
                crate::UiMutation::DespawnSubtree { root } => Some(*root),
                _ => None,
            })
            .collect();
        if roots.is_empty() {
            return;
        }
        let under = |roots: &HashSet<StableNodeId>, id: StableNodeId| {
            std::iter::successors(Some(id), |id| self.world.parent_id(*id))
                .any(|ancestor| roots.contains(&ancestor))
        };
        loop {
            let found: Vec<_> = self
                .placed_assembled
                .iter()
                .copied()
                .filter(|&node| !under(&roots, node))
                .filter(|node| {
                    self.assembled_parent
                        .get(node)
                        .is_some_and(|(declared, _)| under(&roots, *declared))
                })
                .collect();
            if found.is_empty() {
                return;
            }
            for node in found {
                roots.insert(node);
                mutations.despawn_subtree(node);
            }
        }
    }

    fn forget_subtree(&mut self, removed: &HashSet<StableNodeId>) {
        self.remove_event_handlers_for(removed);
        self.component_lifecycle
            .text_area_resizes
            .retain(|_, target| !removed.contains(target));
        #[cfg(feature = "rich-text")]
        self.component_lifecycle
            .rich_text_presses
            .retain(|_, target| !removed.contains(target));
        for id in removed {
            self.component_lifecycle.tooltips.remove(id);
            self.component_lifecycle.loading.remove(id);
            self.views.remove(id);
        }
        #[cfg(feature = "charts")]
        self.component_lifecycle
            .chart_tooltips
            .retain(|owner, tooltip| !removed.contains(owner) && !removed.contains(tooltip));
        self.child_reproject_views
            .retain(|id, _| !removed.contains(id));
        self.metrics_reproject_views
            .retain(|id, _| !removed.contains(id));
        self.recipe_reproject_views
            .retain(|id, _| !removed.contains(id));
        self.text_backend_reproject_views
            .retain(|id, _| !removed.contains(id));
        self.pending_child_reprojects
            .retain(|id| !removed.contains(id));
        self.component_lifecycle
            .tooltips
            .retain(|_, tooltip| !removed.contains(&tooltip.overlay));
        self.assembled.retain(|parent, slots| {
            if removed.contains(parent) {
                return false;
            }
            slots.retain(|_, child| !removed.contains(&child.id));
            true
        });
        self.assembled_parent
            .retain(|child, (parent, _)| !removed.contains(child) && !removed.contains(parent));
        self.placed_assembled.retain(|id| !removed.contains(id));
        self.forget_reactive(removed);
    }
}

pub trait UiExtension {
    fn name(&self) -> &'static str;
    fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError>;
}

fn normalized_action_id(id: impl Into<ActionId>) -> Result<ActionId, FrameworkError> {
    let id = id.into();
    let id = ActionId::new(id.as_str().trim());
    if id.as_str().is_empty() {
        Err(FrameworkError::InvalidAction)
    } else {
        Ok(id)
    }
}

fn insert_action(
    actions: &mut HashMap<ActionId, RegisteredAction>,
    id: impl Into<ActionId>,
    when: ContextPredicate,
    handler: impl FnMut(&mut AppContext) -> Result<(), FrameworkError> + Send + 'static,
) -> Result<(), FrameworkError> {
    let id = normalized_action_id(id)?;
    if actions.contains_key(&id) {
        return Err(FrameworkError::DuplicateAction(id));
    }
    actions.insert(
        id,
        RegisteredAction {
            when,
            handler: Box::new(handler),
        },
    );
    Ok(())
}

#[must_use = "tasks do nothing until a host executor runs them"]
pub struct Task<T> {
    future: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
}

impl<T> Task<T> {
    pub fn new(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self {
            future: Box::pin(future),
        }
    }

    pub fn ready(value: T) -> Self
    where
        T: Send + 'static,
    {
        Self::new(async move { value })
    }

    pub fn into_future(self) -> Pin<Box<dyn Future<Output = T> + Send + 'static>> {
        self.future
    }

    pub fn map<U: Send + 'static>(self, map: impl FnOnce(T) -> U + Send + 'static) -> Task<U>
    where
        T: Send + 'static,
    {
        Task::new(async move { map(self.future.await) })
    }
}

/// Wake-driven event stream. Platform adapters own execution and cancellation;
/// the core runtime does not create a competing executor.
#[must_use = "subscriptions do nothing until a host consumes their stream"]
pub struct Subscription<T> {
    id: String,
    stream: Pin<Box<dyn Stream<Item = T> + Send + 'static>>,
}

impl<T> Subscription<T> {
    pub fn new(id: impl Into<String>, stream: impl Stream<Item = T> + Send + 'static) -> Self {
        Self {
            id: id.into(),
            stream: Box::pin(stream),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn into_stream(self) -> Pin<Box<dyn Stream<Item = T> + Send + 'static>> {
        self.stream
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod retained_interaction_tests;

#[cfg(test)]
mod scroll_origin_tests;

/// A view an event handler ran on during another view's update.
pub(super) struct TouchedObserver {
    pub(super) id: StableNodeId,
    pub(super) type_id: TypeId,
    /// The handler asked for the observer's assembler through
    /// [`ViewContext::reassemble`].
    pub(super) reassemble: bool,
}

/// How [`AppContext::update_component`]'s implementation treats the result.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Projection {
    /// Skip everything when the update left the component unchanged.
    IfChanged,
    /// Project, commit, run lifecycle and the assembler regardless.
    Always,
    /// Project and commit regardless, nothing else: a view changed in place
    /// (by an observer handler, or by suspension) whose lifecycle and
    /// children follow only when asked for.
    ProjectOnly,
}

/// Projects a component of a type known only by `TypeId`.
pub(crate) type ReprojectFn =
    fn(&mut AppContext, StableNodeId, Projection) -> Result<(), FrameworkError>;

pub(crate) fn reproject_erased<C: ComponentView>(
    context: &mut AppContext,
    id: StableNodeId,
    projection: Projection,
) -> Result<(), FrameworkError> {
    context.update_component_inner(Entity::<C>::from_stable_id(id), |_, _| {}, projection)
}

/// [`AppContext::reproject_component`] for the per-node reproject hooks.
fn reproject_typed<C: ComponentView>(
    context: &mut AppContext,
    id: StableNodeId,
) -> Result<(), FrameworkError> {
    context.reproject_component(Entity::<C>::from_stable_id(id))
}

/// Menu surfaces: their open motion is only started for a mounted node.
pub(super) fn is_menu_surface(type_id: TypeId) -> bool {
    [
        TypeId::of::<crate::Popover>(),
        TypeId::of::<crate::ActionMenu>(),
        TypeId::of::<crate::AnchoredActionMenu>(),
        TypeId::of::<crate::ContextMenu>(),
    ]
    .contains(&type_id)
}

/// Types whose projection only starts timelines or surface motion for a
/// mounted node, so a remount has to project them again.
pub(super) fn reprojects_on_mount(type_id: TypeId) -> bool {
    type_id == TypeId::of::<TextArea>() || is_menu_surface(type_id)
}
