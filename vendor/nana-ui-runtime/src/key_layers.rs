//! Backend-neutral shortcut capture and keymap resolution.
//!
//! Host-facing command-palette `KeyStroke` / `Keymap` / `ActionRegistry` live in
//! `nana-ui::command`. This module uses [`CapturedStroke`] and a thin
//! enabled-state registry — not a second command palette. The input router
//! maps a canonical key press (or a Vue `KeyboardEvent`) into [`KeyInput`];
//! this file itself stays free of platform types.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use nana_ui_core::{ActionId, ContextPredicate, KeyContext, LengthSpec};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, MutationQueue,
    NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

/// Modifier bits for a backend-neutral key event.
///
/// Fields match `nana_ui_input::InputModifiers` 1:1:
/// - `alt` ← `InputModifiers::alt`
/// - `control` ← `InputModifiers::control`
/// - `meta` ← `InputModifiers::meta` / `nana_ui::command::KeyModifiers::logo`
/// - `shift` ← `InputModifiers::shift`
///
/// This crate does not depend on platform types; hosts copy the bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct KeyModifiers {
    pub alt: bool,
    pub control: bool,
    pub meta: bool,
    pub shift: bool,
}

impl KeyModifiers {
    pub const fn primary() -> Self {
        if cfg!(target_os = "macos") {
            Self {
                meta: true,
                ..Self::empty()
            }
        } else {
            Self {
                control: true,
                ..Self::empty()
            }
        }
    }

    pub const fn empty() -> Self {
        Self {
            alt: false,
            control: false,
            meta: false,
            shift: false,
        }
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn from_flags(control: bool, alt: bool, shift: bool, meta: bool) -> Self {
        Self {
            alt,
            control,
            meta,
            shift,
        }
    }
}

/// Host-normalized key press or release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub pressed: bool,
    pub key: Arc<str>,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

impl KeyInput {
    /// Host-normalized keyboard event from platform fields.
    ///
    /// Map `nana_ui_input::InputEvent::Keyboard` as
    /// `(pressed, key, modifiers.alt, modifiers.control, modifiers.shift,
    /// modifiers.meta, repeat)`. Vue `KeyboardEvent` uses the same flag names
    /// (`altKey`/`ctrlKey`/`shiftKey`/`metaKey`).
    pub fn new(
        pressed: bool,
        key: &str,
        alt: bool,
        control: bool,
        shift: bool,
        meta: bool,
        repeat: bool,
    ) -> Self {
        Self {
            pressed,
            key: Arc::from(key),
            modifiers: KeyModifiers {
                alt,
                control,
                meta,
                shift,
            },
            repeat,
        }
    }

    pub fn press(key: impl Into<Arc<str>>, modifiers: KeyModifiers) -> Self {
        Self {
            pressed: true,
            key: key.into(),
            modifiers,
            repeat: false,
        }
    }

    pub fn with_repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }
}

/// Chord key plus modifiers. Hosts copy these bits from platform input.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapturedStroke {
    pub key: Arc<str>,
    pub modifiers: KeyModifiers,
}

impl CapturedStroke {
    pub fn new(key: impl Into<Arc<str>>, modifiers: KeyModifiers) -> Self {
        Self {
            key: normalize_key_name(key.into().as_ref()),
            modifiers,
        }
    }

    fn from_input(event: &KeyInput) -> Option<Self> {
        let stroke = Self::new(Arc::clone(&event.key), event.modifiers);
        (!stroke.key.is_empty()).then_some(stroke)
    }

    /// Platform-conventional rendering, e.g. `Ctrl+Shift+P` or `⌘P`.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.meta {
            parts.push(if cfg!(target_os = "macos") {
                "⌘".to_owned()
            } else {
                "Super".to_owned()
            });
        }
        let key = if self.key.chars().count() == 1 {
            self.key.to_uppercase()
        } else {
            self.key.to_string()
        };
        parts.push(key);
        parts.join("+")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCaptureEvent {
    Captured(CapturedStroke),
    Cleared,
    Cancelled,
}

/// Records one shortcut while the application has enabled recording.
///
/// Policy object that also projects a small chrome leaf for snapshots.
/// Modifier-only presses stay pending; Tab is left to focus traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCaptureLayer {
    pub recording: bool,
    pub pending_modifiers: KeyModifiers,
}

impl Default for KeyCaptureLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyCaptureLayer {
    pub fn new() -> Self {
        Self {
            recording: false,
            pending_modifiers: KeyModifiers::empty(),
        }
    }

    pub fn recording(mut self, recording: bool) -> Self {
        self.set_recording(recording);
        self
    }

    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
        if !recording {
            self.pending_modifiers = KeyModifiers::empty();
        }
    }

    /// True when a press must not reach content or the application keymap.
    pub fn should_consume(&self, event: &KeyInput) -> bool {
        self.recording && event.pressed && !is_named(event.key.as_ref(), "Tab")
    }

    pub fn handle_key(&mut self, event: &KeyInput) -> Option<KeyCaptureEvent> {
        if !self.recording {
            self.pending_modifiers = KeyModifiers::empty();
            return None;
        }
        if !event.pressed {
            if is_modifier_key(event.key.as_ref()) {
                self.pending_modifiers = event.modifiers;
            }
            return None;
        }
        if event.repeat {
            return None;
        }
        if is_named(event.key.as_ref(), "Tab") {
            return None;
        }
        if is_named(event.key.as_ref(), "Escape") {
            self.pending_modifiers = KeyModifiers::empty();
            return Some(KeyCaptureEvent::Cancelled);
        }
        if is_named(event.key.as_ref(), "Delete") || is_named(event.key.as_ref(), "Backspace") {
            self.pending_modifiers = KeyModifiers::empty();
            return Some(KeyCaptureEvent::Cleared);
        }
        if is_modifier_key(event.key.as_ref()) {
            self.pending_modifiers = event.modifiers;
            return None;
        }
        let stroke = CapturedStroke::from_input(event)?;
        self.pending_modifiers = KeyModifiers::empty();
        Some(KeyCaptureEvent::Captured(stroke))
    }
}

impl ComponentView for KeyCaptureLayer {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "key-capture-layer".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::KeyCaptureLayer {
            recording: self.recording,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &layer_chrome_style(),
            InteractionState {
                pointer_events: false,
                focusable: self.recording,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from(if self.recording {
                    "Recording shortcut"
                } else {
                    "Shortcut capture"
                })),
                ..AccessibilityState::default()
            },
        );
    }
}

/// One registered action: its stable id, when it is available, and the copy a
/// command palette shows for it.
///
/// This is the single action record. The keymap needs `id` / `enabled` / `when`;
/// a palette additionally reads `label` / `category` / `keywords`. A host that
/// only binds keys leaves the copy empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub enabled: bool,
    pub when: ContextPredicate,
    /// Human-readable name. Empty for actions that never reach a palette.
    pub label: String,
    pub category: Option<String>,
    pub keywords: Vec<String>,
}

impl ActionDescriptor {
    pub fn new(id: impl Into<ActionId>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            when: ContextPredicate::always(),
            label: String::new(),
            category: None,
            keywords: Vec::new(),
        }
    }

    /// Action carrying palette copy.
    pub fn labeled(id: impl Into<ActionId>, label: impl Into<String>) -> Self {
        Self::new(id).label(label)
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn when(mut self, when: ContextPredicate) -> Self {
        self.when = when;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionRegistryError {
    EmptyId,
    Duplicate { id: ActionId },
}

impl fmt::Display for ActionRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => formatter.write_str("action id must not be empty"),
            Self::Duplicate { id } => write!(formatter, "action `{id}` is already registered"),
        }
    }
}

impl std::error::Error for ActionRegistryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredAction {
    descriptor: ActionDescriptor,
    /// Registration order, so a palette lists actions the way the host declared
    /// them rather than alphabetically by id.
    order: u64,
}

/// Every action the application registers: the keymap reads `enabled` / `when`,
/// a command palette reads the copy and calls [`Self::search`]. One registry,
/// not one per consumer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionRegistry {
    actions: BTreeMap<ActionId, RegisteredAction>,
    next_order: u64,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        mut descriptor: ActionDescriptor,
    ) -> Result<(), ActionRegistryError> {
        descriptor.id = ActionId::new(descriptor.id.as_str().trim());
        if descriptor.id.as_str().is_empty() {
            return Err(ActionRegistryError::EmptyId);
        }
        if self.actions.contains_key(&descriptor.id) {
            return Err(ActionRegistryError::Duplicate { id: descriptor.id });
        }
        let order = self.next_order;
        self.next_order += 1;
        self.actions.insert(
            descriptor.id.clone(),
            RegisteredAction { descriptor, order },
        );
        Ok(())
    }

    pub fn unregister(&mut self, id: &ActionId) -> Option<ActionDescriptor> {
        self.actions.remove(id).map(|action| action.descriptor)
    }

    pub fn set_enabled(&mut self, id: &ActionId, enabled: bool) -> bool {
        let Some(action) = self.actions.get_mut(id) else {
            return false;
        };
        action.descriptor.enabled = enabled;
        true
    }

    pub fn get(&self, id: &ActionId) -> Option<&ActionDescriptor> {
        self.actions.get(id).map(|action| &action.descriptor)
    }

    pub fn is_available(&self, id: &ActionId, context: &KeyContext) -> bool {
        self.get(id)
            .is_some_and(|action| action.enabled && action.when.matches(context))
    }

    /// Enabled actions whose `when` matches, in registration order.
    pub fn available(&self, context: &KeyContext) -> Vec<&ActionDescriptor> {
        let mut actions = self
            .actions
            .values()
            .filter(|action| action.descriptor.enabled && action.descriptor.when.matches(context))
            .collect::<Vec<_>>();
        actions.sort_by_key(|action| action.order);
        actions
            .into_iter()
            .map(|action| &action.descriptor)
            .collect()
    }

    /// Fuzzy-matches `query` against label, id, category and keywords.
    /// Best match first; ties keep registration order.
    pub fn search<'a>(&'a self, query: &str, context: &KeyContext) -> Vec<ActionMatch<'a>> {
        let query = query.trim().to_lowercase();
        let mut matches = self
            .actions
            .values()
            .filter(|action| action.descriptor.enabled && action.descriptor.when.matches(context))
            .filter_map(|action| {
                action_score(&action.descriptor, &query).map(|score| ActionMatch {
                    action: &action.descriptor,
                    score,
                    order: action.order,
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|matched| (matched.score, matched.order));
        matches
    }
}

/// One search hit from [`ActionRegistry::search`]; lower `score` is better.
#[derive(Debug, Clone, Copy)]
pub struct ActionMatch<'a> {
    pub action: &'a ActionDescriptor,
    pub score: u32,
    order: u64,
}

fn action_score(action: &ActionDescriptor, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let candidates = std::iter::once(action.label.as_str())
        .chain(std::iter::once(action.id.as_str()))
        .chain(action.category.as_deref())
        .chain(action.keywords.iter().map(String::as_str))
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    query
        .split_whitespace()
        .map(|term| {
            candidates
                .iter()
                .filter_map(|candidate| fuzzy_score(candidate, term))
                .min()
        })
        .try_fold(0_u32, |total, score| {
            score.map(|score| total.saturating_add(score))
        })
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<u32> {
    if candidate == query {
        return Some(0);
    }
    if candidate.starts_with(query) {
        return Some(
            10 + candidate
                .chars()
                .count()
                .saturating_sub(query.chars().count()) as u32,
        );
    }
    if let Some(position) = candidate.find(query) {
        return Some(100 + position as u32);
    }
    let mut candidate_chars = candidate.chars().enumerate();
    let mut last = 0_usize;
    let mut gap = 0_usize;
    for query_char in query.chars() {
        let (index, _) =
            candidate_chars.find(|(_, candidate_char)| *candidate_char == query_char)?;
        gap = gap.saturating_add(index.saturating_sub(last));
        last = index.saturating_add(1);
    }
    Some(200 + gap as u32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub action: ActionId,
    pub sequence: Vec<CapturedStroke>,
    pub when: ContextPredicate,
}

impl KeyBinding {
    pub fn new(action: impl Into<ActionId>, stroke: CapturedStroke) -> Self {
        Self {
            action: action.into(),
            sequence: vec![stroke],
            when: ContextPredicate::always(),
        }
    }

    pub fn sequence(
        action: impl Into<ActionId>,
        sequence: impl IntoIterator<Item = CapturedStroke>,
    ) -> Self {
        Self {
            action: action.into(),
            sequence: sequence.into_iter().collect(),
            when: ContextPredicate::always(),
        }
    }

    pub fn when(mut self, when: ContextPredicate) -> Self {
        self.when = when;
        self
    }
    /// The whole sequence rendered for display, e.g. `Ctrl+K Ctrl+S`.
    pub fn display(&self) -> String {
        self.sequence
            .iter()
            .map(CapturedStroke::display)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapState {
    pending: Vec<CapturedStroke>,
}

impl KeymapState {
    pub fn pending(&self) -> &[CapturedStroke] {
        &self.pending
    }

    pub fn cancel(&mut self) {
        self.pending.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapMatch {
    Dispatch(ActionId),
    Pending,
    NoMatch,
}

impl KeymapMatch {
    pub fn consumed(&self) -> bool {
        !matches!(self, Self::NoMatch)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
}

impl Keymap {
    pub fn new(bindings: impl IntoIterator<Item = KeyBinding>) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }

    /// Shortcut a command palette shows beside `action`, if one is bound and
    /// currently available. The last matching binding wins, so a later
    /// registration overrides an earlier default.
    pub fn binding_label(
        &self,
        action: &ActionId,
        context: &KeyContext,
        registry: &ActionRegistry,
    ) -> Option<String> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| {
                binding.action == *action
                    && !binding.sequence.is_empty()
                    && binding.when.matches(context)
                    && registry.is_available(action, context)
            })
            .map(KeyBinding::display)
    }

    pub fn push(&mut self, binding: KeyBinding) {
        self.bindings.push(binding);
    }

    pub fn resolve(
        &self,
        state: &mut KeymapState,
        stroke: CapturedStroke,
        context: &KeyContext,
        registry: &ActionRegistry,
    ) -> KeymapMatch {
        let had_pending = !state.pending.is_empty();
        state.pending.push(stroke.clone());
        let matched = self.resolve_pending(state, context, registry);
        if matched == KeymapMatch::NoMatch && had_pending {
            state.pending.clear();
            state.pending.push(stroke);
            return self.resolve_pending(state, context, registry);
        }
        matched
    }

    fn resolve_pending(
        &self,
        state: &mut KeymapState,
        context: &KeyContext,
        registry: &ActionRegistry,
    ) -> KeymapMatch {
        let candidates = self.bindings.iter().rev().filter(|binding| {
            binding.sequence.starts_with(&state.pending)
                && binding.when.matches(context)
                && registry.is_available(&binding.action, context)
        });
        let mut has_prefix = false;
        for binding in candidates {
            if binding.sequence.len() == state.pending.len() {
                let action = binding.action.clone();
                state.pending.clear();
                return KeymapMatch::Dispatch(action);
            }
            has_prefix = true;
        }
        if has_prefix {
            KeymapMatch::Pending
        } else {
            state.pending.clear();
            KeymapMatch::NoMatch
        }
    }
}

/// Resolves application key bindings against context and enabled actions.
///
/// Chord prefix state. A hit is consumed; a miss is `None` so content can
/// handle the key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapLayer {
    pub keymap: Keymap,
    pub context: KeyContext,
    pub registry: ActionRegistry,
    state: KeymapState,
}

impl KeymapLayer {
    pub fn new(keymap: Keymap, context: KeyContext, registry: ActionRegistry) -> Self {
        Self {
            keymap,
            context,
            registry,
            state: KeymapState::default(),
        }
    }

    pub fn pending(&self) -> &[CapturedStroke] {
        self.state.pending()
    }

    pub fn cancel(&mut self) {
        self.state.cancel();
    }

    /// Resolve including chord [`KeymapMatch::Pending`].
    pub fn resolve_key(&mut self, event: &KeyInput) -> KeymapMatch {
        if !event.pressed {
            return KeymapMatch::NoMatch;
        }
        let Some(stroke) = CapturedStroke::from_input(event) else {
            return KeymapMatch::NoMatch;
        };
        self.keymap
            .resolve(&mut self.state, stroke, &self.context, &self.registry)
    }

    pub fn handle_key(&mut self, event: &KeyInput) -> Option<ActionId> {
        match self.resolve_key(event) {
            KeymapMatch::Dispatch(action) => Some(action),
            KeymapMatch::Pending | KeymapMatch::NoMatch => None,
        }
    }
}

impl ComponentView for KeymapLayer {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "keymap-layer".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::KeymapLayer;
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &layer_chrome_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                ..AccessibilityState::default()
            },
        );
    }
}

fn layer_chrome_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.min_height = Some(LengthSpec::Px(28.0));
    style
}

fn normalize_key_name(value: &str) -> Arc<str> {
    let value = value.trim();
    if value.chars().count() == 1 {
        Arc::from(value.to_lowercase())
    } else {
        Arc::from(value)
    }
}

fn is_named(key: &str, name: &str) -> bool {
    key.trim().eq_ignore_ascii_case(name)
}

fn is_modifier_key(key: &str) -> bool {
    [
        "Alt", "AltGraph", "Control", "Ctrl", "Shift", "Meta", "Hyper", "Super",
    ]
    .iter()
    .any(|name| is_named(key, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn ctrl() -> KeyModifiers {
        KeyModifiers {
            control: true,
            ..KeyModifiers::empty()
        }
    }

    fn press(key: &str, modifiers: KeyModifiers) -> KeyInput {
        KeyInput::press(key, modifiers)
    }

    #[test]
    fn key_input_maps_platform_modifier_fields() {
        let event = KeyInput::new(true, "K", true, true, true, true, true);
        assert_eq!(
            event,
            KeyInput {
                pressed: true,
                key: Arc::from("K"),
                modifiers: KeyModifiers {
                    alt: true,
                    control: true,
                    meta: true,
                    shift: true,
                },
                repeat: true,
            }
        );
        let release = KeyInput::new(false, "Escape", false, true, false, false, false);
        assert!(!release.pressed);
        assert_eq!(release.key.as_ref(), "Escape");
        assert_eq!(
            release.modifiers,
            KeyModifiers {
                control: true,
                ..KeyModifiers::empty()
            }
        );
        assert!(!release.repeat);
    }

    fn palette_registry() -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionDescriptor::new("workspace.palette"))
            .expect("action is valid");
        registry
    }

    #[test]
    fn modifier_only_does_not_emit() {
        let mut layer = KeyCaptureLayer::new().recording(true);
        assert_eq!(layer.handle_key(&press("Control", ctrl())), None);
        assert_eq!(layer.pending_modifiers, ctrl());
        assert!(layer.should_consume(&press("Control", ctrl())));
    }

    #[test]
    fn escape_cancels_delete_clears_and_tab_is_ignored() {
        let mut layer = KeyCaptureLayer::new().recording(true);
        assert_eq!(
            layer.handle_key(&press("Escape", KeyModifiers::empty())),
            Some(KeyCaptureEvent::Cancelled)
        );
        assert_eq!(
            layer.handle_key(&press("Delete", KeyModifiers::empty())),
            Some(KeyCaptureEvent::Cleared)
        );
        assert_eq!(
            layer.handle_key(&press("Backspace", KeyModifiers::empty())),
            Some(KeyCaptureEvent::Cleared)
        );
        assert_eq!(layer.handle_key(&press("Tab", KeyModifiers::empty())), None);
        assert!(!layer.should_consume(&press("Tab", KeyModifiers::empty())));
    }

    #[test]
    fn captures_ctrl_k_when_recording() {
        let mut layer = KeyCaptureLayer::new().recording(true);
        assert_eq!(
            layer.handle_key(&press("K", ctrl())),
            Some(KeyCaptureEvent::Captured(CapturedStroke::new("k", ctrl())))
        );

        layer.set_recording(false);
        assert_eq!(layer.handle_key(&press("K", ctrl())), None);
    }

    #[test]
    fn keymap_hits_bound_shortcut_and_misses_unbound() {
        let mut layer = KeymapLayer::new(
            Keymap::new([KeyBinding::new(
                "workspace.palette",
                CapturedStroke::new("k", ctrl()),
            )]),
            KeyContext::new(["workspace"]),
            palette_registry(),
        );
        assert_eq!(
            layer.handle_key(&press("k", ctrl())),
            Some(ActionId::new("workspace.palette"))
        );
        assert_eq!(
            layer.resolve_key(&press("k", ctrl())),
            KeymapMatch::Dispatch(ActionId::new("workspace.palette"))
        );
        assert_eq!(layer.handle_key(&press("x", KeyModifiers::empty())), None);
        assert_eq!(
            layer.resolve_key(&press("x", KeyModifiers::empty())),
            KeymapMatch::NoMatch
        );
    }

    #[test]
    fn disabled_action_is_not_dispatched() {
        let mut registry = palette_registry();
        registry.set_enabled(&ActionId::new("workspace.palette"), false);
        let mut layer = KeymapLayer::new(
            Keymap::new([KeyBinding::new(
                "workspace.palette",
                CapturedStroke::new("k", ctrl()),
            )]),
            KeyContext::default(),
            registry,
        );
        assert_eq!(layer.handle_key(&press("k", ctrl())), None);
        assert_eq!(layer.resolve_key(&press("k", ctrl())), KeymapMatch::NoMatch);
    }

    #[test]
    fn capture_layer_projects_recording_visual() {
        let mut context = AppContext::new();
        let idle = context
            .create_component(document(), KeyCaptureLayer::new())
            .unwrap();
        let idle_id = idle.stable_id();
        assert!(matches!(
            context.world().node(idle_id).map(|node| node.kind),
            Some(NodeKind::Element { tag }) if tag == "key-capture-layer"
        ));
        assert_eq!(
            context.world().standard_visual(idle_id),
            Some(StandardVisual::KeyCaptureLayer { recording: false })
        );
        assert_eq!(
            context.world().interaction(idle_id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        assert_eq!(
            context
                .world()
                .accessibility(idle_id)
                .and_then(|state| state.label.as_deref()),
            Some("Shortcut capture")
        );
        assert_eq!(
            context
                .world()
                .node_style(idle_id)
                .and_then(|style| style.layout.min_height),
            Some(LengthSpec::Px(28.0))
        );

        let recording = context
            .create_component(document(), KeyCaptureLayer::new().recording(true))
            .unwrap();
        let recording_id = recording.stable_id();
        assert_eq!(
            context.world().standard_visual(recording_id),
            Some(StandardVisual::KeyCaptureLayer { recording: true })
        );
        assert_eq!(
            context.world().interaction(recording_id),
            Some(InteractionState {
                pointer_events: false,
                focusable: true,
            })
        );
        assert_eq!(
            context
                .world()
                .accessibility(recording_id)
                .and_then(|state| state.label.as_deref()),
            Some("Recording shortcut")
        );
    }

    #[test]
    fn keymap_layer_projects_visual() {
        let mut context = AppContext::new();
        let layer = context
            .create_component(document(), KeymapLayer::default())
            .unwrap();
        let id = layer.stable_id();
        assert!(matches!(
            context.world().node(id).map(|node| node.kind),
            Some(NodeKind::Element { tag }) if tag == "keymap-layer"
        ));
        assert_eq!(
            context.world().standard_visual(id),
            Some(StandardVisual::KeymapLayer)
        );
        assert_eq!(
            context.world().interaction(id),
            Some(InteractionState {
                pointer_events: false,
                focusable: false,
            })
        );
        assert_eq!(
            context
                .world()
                .node_style(id)
                .and_then(|style| style.layout.min_height),
            Some(LengthSpec::Px(28.0))
        );
    }
}
