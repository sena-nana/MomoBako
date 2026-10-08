mod accessibility;
mod animation;
mod extraction;
mod focus_scope;
mod geometry;
mod hit_test;
mod input;
mod motion;
mod mutation;
mod overlay_index;
mod presentation;
mod scroll_bounds;
mod style;
mod text;
use hit_test::*;
pub(crate) use text::TextDisplayView;
pub(crate) use text::text_visual_key;
use text::*;

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
    mem::size_of,
    sync::Arc,
    time::Duration,
};

use nana_ui_core::{
    ControlSize, LayoutStyle, LengthSpec, PointerEventsSpec, PositionSpec, SemanticColorRole,
    SemanticPalette, StyleModelRef, SwitchControlPosition, ThemeMode, ThemeWorkCounters,
    icon_y_on_text_glyph_center,
};

#[cfg(feature = "calendar")]
use nana_ui_core::TooltipConfig;
#[cfg(feature = "graph-canvas")]
use nana_ui_core::{GraphPoint, GraphPortKind, GraphPortSide, GraphRect, GraphSize, cubic_point};

use crate::{
    AccessibilityDelta, AccessibilityNode, AccessibilityRole, AccessibilityState, AnimationFrame,
    AnimationId, AnimationSpec, ComponentTypeId, ComputedStyle, CustomRenderNode, EventListeners,
    EventRoute, ExtractedNode, ExtractedTextSpan, HighlightRequest, ImeComposition,
    InteractionState, LayoutBox, LayoutInput, MotionWorkCounters, MountState, MutationQueue,
    NodeStyle, OverlayHostState, PointerCaptureChange, ScrollMetrics, ScrollOffset, StandardVisual,
    TextContent, TextMetrics, TextPresentation, TextPresenter, TextShaper, TextVerticalAlignment,
    UiMutation, WorkCounters,
    animation::ActiveAnimation,
    components::{
        EmptyStateTextPresentation, ModalTextPresentation, TextColorSwatchSpan,
        TextEditorRenderOptions, TextGitGutterMark, TextGitMark, TextGitMarkKind,
        TextInputPresentation, TextMatchMark, TextMatchMarker, TextMatchSpan, TextOverlayMetrics,
        TextSwatchMark, TextWhitespaceKind, TextWhitespaceMark,
    },
    schedule::{DirtyMask, SystemWork, push_work},
    store::{Hierarchy, NodeRecord, NodeStore, ResolvedStyle, intern_empty_children},
    text_editing::clamp_boundary,
};
/// Stable external node identity. Zero is reserved so missing/default IDs
/// cannot accidentally address a live node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StableNodeId(u64);

impl StableNodeId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Hashing for keys that are already integers: node ids, slot numbers, a hash
/// something else computed.
///
/// The default hasher is SipHash — a keyed MAC, which is the right default for
/// a map whose keys come off the wire and a poor one for a counter nobody
/// outside the process can choose. Scene paint asks such maps several times per
/// primitive per frame; a sampling profile of a transform animation put SipHash
/// alone at a tenth of the painter's time.
///
/// The mixing step is rustc's: rotate, xor the word in, multiply by an odd
/// constant. Ids are a dense counter, so the work is spreading them, not
/// hiding them.
#[derive(Debug, Default, Clone, Copy)]
pub struct IdHasher(u64);

impl IdHasher {
    const SEED: u64 = 0x517c_c1b7_2722_0a95;

    fn add(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(Self::SEED);
    }
}

impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.add(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }
}

/// What a map or set keyed by integers hashes with. See [`IdHasher`].
pub type BuildIdHasher = std::hash::BuildHasherDefault<IdHasher>;

/// [`HashMap`] keyed by node, hashed by [`IdHasher`].
pub type NodeMap<V> = HashMap<StableNodeId, V, BuildIdHasher>;

/// [`HashSet`] of nodes, hashed by [`IdHasher`].
pub type NodeSet = HashSet<StableNodeId, BuildIdHasher>;

fn sorted_unique(mut ids: Vec<StableNodeId>) -> Vec<StableNodeId> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// See [`UiWorld::is_scroll_container`].
fn scroll_container(layout: &nana_ui_core::LayoutStyle, visual: Option<&StandardVisual>) -> bool {
    layout.overflow_x.scrolls()
        || layout.overflow_y.scrolls()
        || matches!(visual, Some(StandardVisual::Scrollbar { .. }))
}

/// Deepest retained tree the frame pipeline accepts.
///
/// Style resolution walks ancestors, layout and hit-test walk descendants, and
/// paint walks the scene: all recursive. Tree shape comes from application or JS
/// input and is not trustworthy, so the bound is enforced once where the tree is
/// written. Real UIs nest one to two orders of magnitude below this.
pub const MAX_TREE_DEPTH: usize = 512;

/// Retired node IDs, stored as coalesced inclusive runs.
///
/// Retirement is permanent: a stale handle must never alias a node created
/// later. Both ID allocators ([`crate::AppContext`] and the Vue tree) are
/// strictly monotonic, so churning a list retires consecutive IDs. Keeping runs
/// instead of one entry per ID bounds the ledger by the number of gaps in the
/// allocation stream rather than by every node ever destroyed, with identical
/// membership semantics.
#[derive(Debug, Default)]
struct RetiredIds {
    /// Sorted, disjoint, and never adjacent: `(start, end)` inclusive.
    runs: Vec<(u64, u64)>,
    len: usize,
}

impl RetiredIds {
    /// Index of the run containing `value`, or the insertion point.
    fn locate(&self, value: u64) -> Result<usize, usize> {
        self.runs.binary_search_by(|(start, end)| {
            if value < *start {
                std::cmp::Ordering::Greater
            } else if value > *end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
    }

    fn contains(&self, id: StableNodeId) -> bool {
        self.locate(id.get()).is_ok()
    }

    fn insert(&mut self, id: StableNodeId) {
        let value = id.get();
        let Err(index) = self.locate(value) else {
            return;
        };
        let joins_previous = index > 0 && self.runs[index - 1].1.checked_add(1) == Some(value);
        let joins_next =
            index < self.runs.len() && Some(self.runs[index].0) == value.checked_add(1);
        match (joins_previous, joins_next) {
            (true, true) => {
                self.runs[index - 1].1 = self.runs[index].1;
                self.runs.remove(index);
            }
            (true, false) => self.runs[index - 1].1 = value,
            (false, true) => self.runs[index].0 = value,
            (false, false) => self.runs.insert(index, (value, value)),
        }
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    /// Stored runs. This is the ledger's real memory cost.
    fn runs(&self) -> usize {
        self.runs.len()
    }
}

/// Stable document/window ownership boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentId(u64);

impl DocumentId {
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Element { tag: String },
    Text,
    Comment,
}

fn menu_surface_open(visual: Option<&StandardVisual>) -> Option<bool> {
    match visual {
        Some(StandardVisual::MenuSurface { open, .. }) => Some(*open),
        _ => None,
    }
}

/// Whether a menu surface with `visual` hides its child `child`: a closed
/// surface hides its items, never the content that draws its trigger.
pub(crate) fn closed_menu_hides(visual: Option<&StandardVisual>, child: StableNodeId) -> bool {
    match visual {
        Some(StandardVisual::MenuSurface {
            open: false,
            overlay,
            ..
        }) => overlay.and_then(|overlay| overlay.trigger_content) != Some(child),
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct HitEntry {
    id: StableNodeId,
    /// Shared authoritative child sequence at projection time. Pointer identity
    /// validates cached sibling ordinals without searching a wide parent.
    source_children: Arc<Vec<StableNodeId>>,
    layout: LayoutBox,
    transform: [f32; 6],
    persp: [f32; 2],
    /// Clips applied to this node's own hit (and therefore its subtree).
    self_clips: Vec<(LayoutBox, [f32; 6])>,
    /// Extra clips applied to descendants only (overflow / visual frames).
    child_clips: Vec<(LayoutBox, [f32; 6])>,
    /// Shape clips from `paint.clip_path`, kept in node-local coordinates so
    /// presentation transforms can be sampled at hit-test time.
    path_clips: Vec<crate::world::hit_test::HitPathClip>,
    z_index: i32,
    order: usize,
    hittable: bool,
    menu: Option<LayoutBox>,
    /// A painter that decides which points of the box hit (Issue #217).
    painter: Option<Arc<PainterHit>>,
    children: Vec<HitEntry>,
}

/// A node's latest recording, shared with its hit index entry so a
/// re-record — on a state, focus, theme or font change, whichever path
/// asked for it — reaches hit testing without the index being rebuilt.
type SharedRecording = Arc<std::sync::RwLock<Arc<crate::PaintRecording>>>;

/// One node's last recording and what it was recorded against.
#[derive(Debug)]
struct PaintCacheEntry {
    key: crate::custom_paint::PaintCacheKey,
    recording: Arc<crate::PaintRecording>,
    /// The same recording, for the hit index.
    latest: SharedRecording,
    /// What the painter's text was measured against — the node's resolved
    /// style and the host's font backend — when it measured any. Only then
    /// does a font or backend change re-record it.
    measured_text: Option<(
        Arc<crate::ComputedStyle>,
        Option<crate::text_node::TextBackendEpoch>,
    )>,
}

/// What a painted node's hit test asks: the painter's own answer, else the
/// recording's outline when the painter asked for that.
#[derive(Debug)]
struct PainterHit {
    painter: crate::NodePainter,
    recording: Option<SharedRecording>,
}

/// Per-pass cache of ancestor-chain answers. Extraction and hit-index share
/// most of each chain, so one walk fills every node on it.
#[derive(Default)]
struct AncestorMemo {
    live: HashMap<StableNodeId, bool>,
    stacking: HashMap<StableNodeId, i32>,
    /// Paint color filled this extract pass after a palette epoch change.
    color: HashMap<StableNodeId, [f32; 4]>,
    chain: Vec<StableNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSnapshot {
    pub id: StableNodeId,
    pub document: DocumentId,
    pub kind: NodeKind,
    pub parent: Option<StableNodeId>,
    pub children: Vec<StableNodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitReport {
    pub generation: u64,
    pub mutations: usize,
    pub created: usize,
    pub inserted: usize,
    pub detached: usize,
    pub reparented: usize,
    pub despawned: usize,
}

impl CommitReport {
    /// A commit of `mutations` that changed nothing.
    pub(crate) const fn unchanged(generation: u64, mutations: usize) -> Self {
        Self {
            generation,
            mutations,
            created: 0,
            inserted: 0,
            detached: 0,
            reparented: 0,
            despawned: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiWorldError {
    DuplicateNode(StableNodeId),
    RetiredNode(StableNodeId),
    MissingNode(StableNodeId),
    CrossDocument {
        parent: StableNodeId,
        child: StableNodeId,
    },
    FocusDocument {
        document: DocumentId,
        target: StableNodeId,
    },
    PointerDocument {
        document: DocumentId,
        target: StableNodeId,
    },
    Cycle {
        parent: StableNodeId,
        child: StableNodeId,
    },
    /// Parenting `child` under `parent` would exceed [`MAX_TREE_DEPTH`]. Style
    /// resolution, layout, hit-test and paint all recurse over the retained
    /// tree, so the depth bound is enforced where the tree is written rather
    /// than re-checked in every walk.
    TreeTooDeep {
        parent: StableNodeId,
        child: StableNodeId,
        depth: usize,
    },
    InvalidBefore {
        parent: StableNodeId,
        before: StableNodeId,
    },
    InvalidStyle(StableNodeId),
    InvalidText(StableNodeId),
    InvalidLayout(StableNodeId),
    InvalidScrollOffset(StableNodeId),
    InvalidScrollMetrics(StableNodeId),
    InvalidIme(StableNodeId),
    InvalidCustomRender(StableNodeId),
    InvalidEventListener(StableNodeId),
    InvalidStandardVisual(StableNodeId),
    InvalidOverlayHost(StableNodeId),
    NotFocusable(StableNodeId),
    NotPointerInteractive(StableNodeId),
    NotFocused(StableNodeId),
    PointerCaptureMismatch {
        pointer_id: u64,
        target: StableNodeId,
    },
    InvalidAnimation(AnimationId),
    MissingAnimation(AnimationId),
    InvalidTextInput(StableNodeId),
    MissingTextInput(StableNodeId),
    InvalidHighlightRequest(StableNodeId),
    InvalidPresenter,
    DuplicatePresenter(String),
}

impl fmt::Display for UiWorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode(id) => write!(formatter, "node {} already exists", id.get()),
            Self::RetiredNode(id) => write!(formatter, "node {} was retired", id.get()),
            Self::MissingNode(id) => write!(formatter, "node {} does not exist", id.get()),
            Self::CrossDocument { parent, child } => write!(
                formatter,
                "cannot parent node {} under node {} from another document",
                child.get(),
                parent.get()
            ),
            Self::FocusDocument { document, target } => write!(
                formatter,
                "node {} does not belong to document {}",
                target.get(),
                document.get()
            ),
            Self::PointerDocument { document, target } => write!(
                formatter,
                "pointer target {} does not belong to document {}",
                target.get(),
                document.get()
            ),
            Self::Cycle { parent, child } => write!(
                formatter,
                "parenting node {} under node {} would create a cycle",
                child.get(),
                parent.get()
            ),
            Self::TreeTooDeep {
                parent,
                child,
                depth,
            } => write!(
                formatter,
                "parenting node {} under node {} would reach depth {depth}, past the {MAX_TREE_DEPTH} limit",
                child.get(),
                parent.get()
            ),
            Self::InvalidBefore { parent, before } => write!(
                formatter,
                "node {} is not a child of parent {}",
                before.get(),
                parent.get()
            ),
            Self::InvalidStyle(id) => write!(formatter, "node {} has an invalid style", id.get()),
            Self::InvalidText(id) => {
                write!(formatter, "node {} has invalid text metrics", id.get())
            }
            Self::InvalidLayout(id) => {
                write!(formatter, "node {} has an invalid layout box", id.get())
            }
            Self::InvalidScrollOffset(id) => {
                write!(formatter, "node {} has an invalid scroll offset", id.get())
            }
            Self::InvalidScrollMetrics(id) => {
                write!(formatter, "node {} has invalid scroll metrics", id.get())
            }
            Self::InvalidIme(id) => write!(formatter, "node {} has an invalid IME range", id.get()),
            Self::InvalidCustomRender(id) => {
                write!(
                    formatter,
                    "node {} has invalid custom render content",
                    id.get()
                )
            }
            Self::InvalidEventListener(id) => {
                write!(
                    formatter,
                    "node {} has an invalid event listener name",
                    id.get()
                )
            }
            Self::InvalidStandardVisual(id) => {
                write!(
                    formatter,
                    "node {} has invalid standard visual state",
                    id.get()
                )
            }
            Self::InvalidOverlayHost(id) => {
                write!(formatter, "node {} has an invalid active overlay", id.get())
            }
            Self::NotFocusable(id) => write!(formatter, "node {} cannot receive focus", id.get()),
            Self::NotPointerInteractive(id) => {
                write!(formatter, "node {} cannot receive pointer input", id.get())
            }
            Self::NotFocused(id) => write!(formatter, "node {} is not focused", id.get()),
            Self::PointerCaptureMismatch { pointer_id, target } => write!(
                formatter,
                "pointer {pointer_id} is not captured by node {}",
                target.get()
            ),
            Self::InvalidAnimation(id) => write!(formatter, "animation {} is invalid", id.get()),
            Self::MissingAnimation(id) => {
                write!(formatter, "animation {} is not active", id.get())
            }
            Self::InvalidTextInput(id) => {
                write!(formatter, "node {} has invalid text input state", id.get())
            }
            Self::MissingTextInput(id) => {
                write!(formatter, "node {} has no text input state", id.get())
            }
            Self::InvalidHighlightRequest(id) => {
                write!(
                    formatter,
                    "node {} has an invalid highlight request",
                    id.get()
                )
            }
            Self::InvalidPresenter => formatter.write_str("presenter name must not be empty"),
            Self::DuplicatePresenter(name) => {
                write!(formatter, "presenter `{name}` is already registered")
            }
        }
    }
}

impl std::error::Error for UiWorldError {}

#[derive(Debug, Clone)]
struct PlannedNode {
    document: DocumentId,
    parent: Option<StableNodeId>,
    children: Vec<StableNodeId>,
}

/// The sole authoritative retained identity and hierarchy store.
/// Which device the input being routed came from, for `:focus-visible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputModality {
    Keyboard,
    Pointer,
}

pub struct UiWorld {
    input: input::WorldInputState,
    nodes: NodeStore,
    retired: RetiredIds,
    dirty_entities: HashSet<StableNodeId, BuildIdHasher>,
    /// Monotonic, non-consuming invalidation epoch. Input routing uses this
    /// to report whether dispatch scheduled work without scanning or draining
    /// the retained work queues.
    pending_work_revision: u64,
    hit_test_index: HashMap<DocumentId, HitIndex, BuildIdHasher>,
    /// Scroll deltas awaiting the in-place hit-index patch (see
    /// `UiMutation::SetScrollOffset`). Drained by the frame driver.
    scroll_hit_updates: Vec<(StableNodeId, [f32; 2])>,
    /// Input changes that cannot be represented by scroll translation alone.
    non_scroll_hit_dirty: HashSet<StableNodeId, BuildIdHasher>,
    scroll_content_bounds: RefCell<scroll_bounds::ContentBoundsIndex>,
    /// Every node styled `overflow: auto | scroll` (what a `ScrollView`
    /// projects too). Despawned ids are dropped when a commit re-measures.
    scroll_containers: NodeSet,
    /// Scroll containers whose style changed in the commit being applied,
    /// or that stopped scrolling.
    scroll_restyled: Vec<StableNodeId>,
    /// The commit being applied wrote a box or moved a node.
    scroll_layout_touched: bool,
    /// Offsets the commit being applied set, as requested.
    scroll_requested: Vec<(StableNodeId, ScrollOffset)>,
    /// Scroll containers whose offset that re-measure clamped, for the
    /// framework to announce. Drained by `take_scroll_reclamped`.
    scroll_reclamped: HashSet<StableNodeId, BuildIdHasher>,
    /// Scroll containers whose viewport size a re-measure changed since the
    /// last [`Self::take_scroll_resized`].
    scroll_resized: HashSet<StableNodeId, BuildIdHasher>,
    /// Last recording of each custom-painted node, keyed by what the painter
    /// promised decides its output (Issue #217). Extraction reads through it,
    /// so an unchanged node is never re-recorded. Only painted nodes have an
    /// entry; despawn drops it.
    paint_recordings: RefCell<crate::NodeMap<PaintCacheEntry>>,
    /// The `nana-text` engine the host last shaped with, for painters that
    /// measure text while recording. `None` until a host shapes through one;
    /// painters then measure with the em-based fallback.
    paint_text_engine: Option<nana_text::SharedTextEngine>,
    /// Painters set on nodes apart from their style
    /// ([`crate::UiMutation::SetPainter`]). Empty in a tree nobody paints.
    painter_overrides: crate::NodeMap<crate::NodePainter>,
    pending_render_removals: Vec<StableNodeId>,
    pending_accessibility_removals: Vec<StableNodeId>,
    animations: HashMap<AnimationId, ActiveAnimation>,
    /// Running width / height / padding / margin tracks by target, in start
    /// order: what a node's layout overlay reads instead of every animation.
    layout_length_tracks: HashMap<StableNodeId, Vec<AnimationId>, BuildIdHasher>,
    pub(crate) animation_now: Duration,
    presentation: nana_ui_core::motion::PresentationStore,
    /// Input / a11y / focus queries increment this. Idle `advance_animations`
    /// must not, including compositor-only overlays.
    presentation_query_samples: Cell<usize>,
    /// `presentation_query_samples` at the start of the last
    /// [`Self::advance_animations`]. Inspector "samples/frame" is the delta.
    presentation_samples_at_advance: Cell<usize>,
    /// Last `advance_animations` attribution (mutations / layout / style /
    /// extract). Track classification is recomputed live.
    last_motion_frame: MotionWorkCounters,
    compositor_layer_requests: HashSet<StableNodeId, BuildIdHasher>,
    motion_descriptors: nana_ui_core::motion::MotionDescriptorStore,
    /// Mutation-scoped Finished/Cancelled not yet observed. A later successful
    /// commit closes this batch so park-cancel does not leak onto an idle
    /// `advance_animations`. Deadline completions still go out on that wake.
    pending_animation_events: Vec<crate::AnimationEvent>,
    surface_motion: HashMap<StableNodeId, motion::SurfaceMotion, BuildIdHasher>,
    closing_surfaces: HashSet<StableNodeId, BuildIdHasher>,
    hover_transitions: HashMap<StableNodeId, style::HoverTransition, BuildIdHasher>,
    animation_deadlines: BTreeSet<(Duration, AnimationId)>,
    /// The installed design system. This is the authority; `style_model` below
    /// is its hot slice, cached so per-node resolution does not chase an `Arc`
    /// and copy a kilobyte to read one colour.
    theme: Arc<nana_ui_core::CompiledTheme>,
    style_model: StyleModelRef,
    generation: u64,
    /// Cursor declarations changed since the last system-work drain.
    cursor_style_dirty: bool,
    presenters: HashMap<String, Box<dyn TextPresenter>>,
    spawned_since_drain: usize,
    despawned_since_drain: usize,
    last_counters: WorkCounters,
    frame_counters: WorkCounters,
    frame_extracted_nodes: usize,
    frame_extracted_spans: usize,
    accumulating_frame: bool,
    /// Layout/document-order allocs recorded from `&self` hot paths.
    pending_hot_allocations: Cell<usize>,
    pending_hot_allocated_bytes: Cell<usize>,
    text_layout_cache: crate::text_layout_cache::TextLayoutCache,
    glyph_cache: crate::GlyphCache,
    /// The backend plain text last resolved against. A different one on a
    /// later pass means every resolved text node is stale.
    text_backend: Option<crate::text_node::TextBackendEpoch>,
    /// Whether `text_backend` changed — its first install included —
    /// since the framework last asked, for geometry a component spends at
    /// projection time from a measurement.
    text_backend_changed: bool,
    /// Text work of the last pass, or of the last frame that ran one.
    text_work: nana_text::TextWorkCounters,
    /// Text work of the frame being accumulated.
    text_frame_work: nana_text::TextWorkCounters,
    /// Editable work (#96) committed since the last text pass: edits,
    /// caret- and selection-only changes, composition updates. Reported with
    /// the pass that lays those changes out.
    pending_edit_work: nana_text::TextWorkCounters,
    /// Nodes style resolution turned visible since the last scheduled text
    /// pass, which re-resolves them alongside its own work.
    text_shown: Vec<StableNodeId>,
    /// Theme/style work (Issue #101) of the last style pass or theme install,
    /// or of the last frame that ran one.
    theme_work: ThemeWorkCounters,
    /// Theme/style work of the frame being accumulated.
    theme_frame_work: ThemeWorkCounters,
    /// Token-authority reads recorded from the `&self` palette paths. Folded
    /// into `theme_work` by the pass that made them. Counted in its own
    /// `Cell` rather than through a pending `ThemeWorkCounters`: this is the
    /// hottest observation on the style path — once per role a resolve reads
    /// — and a whole-struct read-modify-write per read is measurable.
    pending_theme_reads: Cell<usize>,
    /// `LayoutStyle` copies recorded from the style-write paths, folded on
    /// the same boundary. Separate from the reads because a write is rare
    /// and the two are never observed together.
    pending_layout_copies: Cell<usize>,
    /// Recently written layouts, shared by nodes whose layouts are equal.
    layouts: style::LayoutInterner,
    /// Live Confirm modal frames. Extract, a11y, and hit-test skip ancestor
    /// confirm walks when this is zero.
    confirm_modals: usize,
    /// Live EmptyState + ModalFrame visuals that clip descendants.
    clip_visuals: usize,
    /// Nodes with an authored `z-index`, or an open triggered menu overlay
    /// whose children inject z-index. Stacking walks skip when this is zero.
    z_index_nodes: usize,
    /// Live open triggered menus (Popover, ActionMenu, HoverCard). Their
    /// surfaces take the pointer at the content's level, above the page, so
    /// hit testing looks them up here instead of walking the tree.
    triggered_overlays: HashSet<StableNodeId, BuildIdHasher>,
    /// Live nodes whose box resolves against the viewport (`position: fixed`,
    /// `vw` / `vh`). A resize dirties this set together with document roots
    /// instead of discarding the retained layout cache.
    viewport_basis_nodes: usize,
    viewport_basis: HashMap<DocumentId, HashSet<StableNodeId>, BuildIdHasher>,
    /// Nodes that accept a drop, and what they accept. A sparse index rather
    /// than a field on every node: almost no tree has drop targets.
    drop_targets: HashMap<StableNodeId, nana_ui_core::DropAccepts, BuildIdHasher>,
    /// Innermost file-drop hover target, if any. Scene paints overlay chrome.
    drop_hover: Option<(StableNodeId, nana_ui_core::DropEffect)>,
    /// Document-level text selection (not a second TextInput). One range per document.
    document_text_selections: HashMap<DocumentId, crate::DocumentTextSelection, BuildIdHasher>,
    /// Viewport each document was last laid out against.
    ///
    /// Geometry projection runs on `&UiWorld` with no window context, but
    /// overlay surfaces that the framework places itself (the `Select` menu)
    /// have to fold back inside the window near its edges. Layout is the one
    /// place that already knows the viewport, so it records it here.
    document_viewports: HashMap<DocumentId, crate::LayoutViewport, BuildIdHasher>,
    /// Last applied presence flags per entity, so park/remove/despawn can
    /// decrement without double-counting.
    presence_flags: HashMap<StableNodeId, PresenceFlags, BuildIdHasher>,
    /// Subtree roots detached by Remove or Park. Mounted document/scene roots
    /// are created with no parent and are not in this set.
    detached: HashSet<StableNodeId, BuildIdHasher>,
    /// The `detached` roots that are still `Mounted` (a `Detach`, not a
    /// park). A parked root's whole subtree is `Parked`, so `is_mounted`
    /// already answers presence under it; only these need an ancestor walk.
    detached_mounted: HashSet<StableNodeId, BuildIdHasher>,
    /// Live roots per document: `parent.is_none()` and [`Self::presence_live`].
    live_document_roots: HashMap<DocumentId, BTreeSet<StableNodeId>, BuildIdHasher>,
    /// Nodes carrying an `OverlayHostState` component. Overlay bookkeeping walks
    /// this index instead of every entity, so clearing references from a removed
    /// node costs the host count rather than the world size.
    overlay_host_nodes: HashSet<StableNodeId, BuildIdHasher>,
    overlay_hosts_by_document: HashMap<DocumentId, HashSet<StableNodeId>, BuildIdHasher>,
    /// Live nodes grouped by the component that created them.
    ///
    /// Several per-pointer-event paths need "every X in this document" -- the
    /// split-handle slop probe, the calendar heatmap hover release -- and each
    /// answered by walking `document_order` and filtering, which is a
    /// full-document walk per event for a tree that usually contains none of
    /// the thing being looked for.
    ///
    /// It lives beside `component_type` rather than in `AppContext` so there is
    /// one authority: every path that gives a node a component type does it
    /// through `SetComponentType`, including the semantic-binding path that
    /// never touches `stamp_component_type`.
    nodes_by_component: HashMap<ComponentTypeId, HashSet<StableNodeId>>,
    overlay_dependents: HashMap<StableNodeId, HashSet<StableNodeId>, BuildIdHasher>,
    /// `control -> label`: the node whose text names a control that has no
    /// label of its own ([`MutationQueue::set_labelled_by`]).
    labelled_by: HashMap<StableNodeId, StableNodeId, BuildIdHasher>,
    /// `label -> controls` it names: projected again when its text changes,
    /// and let go of with it.
    labels: HashMap<StableNodeId, HashSet<StableNodeId>, BuildIdHasher>,
    /// Nodes visited by mutation validation since the last drain, summed over
    /// every commit the next frame will consume. Validation must scale with the
    /// batch, not the retained world; this is the sentinel for that invariant.
    validation_nodes_scanned: usize,
    /// Bumped on `SetTheme`; extract skips a second palette walk when it matches.
    palette_epoch: u64,
    /// Parents whose child list changed since the last drain (insert, detach,
    /// despawn). Consumers take the list once per commit to schedule opt-in
    /// component reprojections; see `ComponentView::wants_child_reproject`.
    structural_change_parents: Vec<StableNodeId>,
    /// minimap 行长单条缓存（原始值 → 每逻辑行非空白字符数）。存于
    /// `RefCell` 供 `&self` 的 presentation 构建路径读写。
    minimap_line_lengths_cache: RefCell<Option<(crate::TextValue, Vec<u32>)>>,
    /// 括号配对着色单条缓存（原始值 → 配对/未配对 span 表）。值未变
    /// （纯光标/选区同步）时复用上一次 O(n) 单趟栈扫描结果。存于
    /// `RefCell` 供 `&self` 的 presentation 构建路径读写。
    bracket_color_spans_cache: RefCell<Option<(crate::TextValue, Arc<[(usize, usize, usize)]>)>>,
    presentation_transform_documents_cache: RefCell<Option<(u64, u64, HashSet<DocumentId>)>>,
    /// Queries answered by the hit index; see [`Self::hit_test_queries`].
    hit_test_queries: Cell<u64>,
    /// Reused by `hit_test` when it has to order candidates by paint.
    hit_scratch: RefCell<hit_test::HitScratch>,
}

impl Default for UiWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl UiWorld {
    pub(crate) fn take_window_cursor_dirty(&mut self) -> bool {
        std::mem::take(&mut self.cursor_style_dirty)
    }

    pub fn new() -> Self {
        Self {
            input: input::WorldInputState::default(),
            nodes: NodeStore::new(),
            retired: RetiredIds::default(),
            dirty_entities: HashSet::default(),
            pending_work_revision: 0,
            hit_test_index: HashMap::default(),
            scroll_hit_updates: Vec::new(),
            non_scroll_hit_dirty: HashSet::default(),
            scroll_content_bounds: RefCell::new(scroll_bounds::ContentBoundsIndex::default()),
            scroll_containers: NodeSet::default(),
            scroll_restyled: Vec::new(),
            scroll_layout_touched: false,
            scroll_requested: Vec::new(),
            scroll_reclamped: HashSet::default(),
            scroll_resized: HashSet::default(),
            paint_recordings: RefCell::new(crate::NodeMap::default()),
            paint_text_engine: None,
            painter_overrides: crate::NodeMap::default(),
            pending_render_removals: Vec::new(),
            pending_accessibility_removals: Vec::new(),
            animations: HashMap::new(),
            layout_length_tracks: HashMap::default(),
            animation_now: Duration::ZERO,
            presentation: nana_ui_core::motion::PresentationStore::new(),
            presentation_query_samples: Cell::new(0),
            presentation_samples_at_advance: Cell::new(0),
            last_motion_frame: MotionWorkCounters::default(),
            compositor_layer_requests: HashSet::default(),
            motion_descriptors: nana_ui_core::motion::MotionDescriptorStore::new(),
            pending_animation_events: Vec::new(),
            surface_motion: HashMap::default(),
            closing_surfaces: HashSet::default(),
            hover_transitions: HashMap::default(),
            animation_deadlines: BTreeSet::new(),
            theme: nana_ui_core::builtin_theme_arc(ThemeMode::default()),
            style_model: StyleModelRef::default(),
            generation: 0,
            cursor_style_dirty: false,
            presenters: HashMap::new(),
            spawned_since_drain: 0,
            despawned_since_drain: 0,
            last_counters: WorkCounters::default(),
            frame_counters: WorkCounters::default(),
            frame_extracted_nodes: 0,
            frame_extracted_spans: 0,
            accumulating_frame: false,
            pending_hot_allocations: Cell::new(0),
            pending_hot_allocated_bytes: Cell::new(0),
            text_layout_cache: crate::text_layout_cache::TextLayoutCache::default(),
            glyph_cache: crate::GlyphCache::default(),
            text_backend: None,
            text_backend_changed: false,
            text_work: nana_text::TextWorkCounters::default(),
            text_frame_work: nana_text::TextWorkCounters::default(),
            pending_edit_work: nana_text::TextWorkCounters::default(),
            text_shown: Vec::new(),
            theme_work: ThemeWorkCounters::default(),
            theme_frame_work: ThemeWorkCounters::default(),
            pending_theme_reads: Cell::new(0),
            pending_layout_copies: Cell::new(0),
            layouts: style::LayoutInterner::default(),
            confirm_modals: 0,
            clip_visuals: 0,
            z_index_nodes: 0,
            triggered_overlays: HashSet::default(),
            viewport_basis_nodes: 0,
            viewport_basis: HashMap::default(),
            document_viewports: HashMap::default(),
            drop_targets: HashMap::default(),
            drop_hover: None,
            document_text_selections: HashMap::default(),
            presence_flags: HashMap::default(),
            detached: HashSet::default(),
            detached_mounted: HashSet::default(),
            live_document_roots: HashMap::default(),
            overlay_host_nodes: HashSet::default(),
            overlay_hosts_by_document: HashMap::default(),
            nodes_by_component: HashMap::new(),
            overlay_dependents: HashMap::default(),
            labelled_by: HashMap::default(),
            labels: HashMap::default(),
            validation_nodes_scanned: 0,
            palette_epoch: 1,
            structural_change_parents: Vec::new(),
            minimap_line_lengths_cache: RefCell::new(None),
            bracket_color_spans_cache: RefCell::new(None),
            presentation_transform_documents_cache: RefCell::new(None),
            hit_test_queries: Cell::new(0),
            hit_scratch: RefCell::new(hit_test::HitScratch::default()),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn contains(&self, id: StableNodeId) -> bool {
        self.nodes.contains(id)
    }

    pub(crate) fn mark_layout(&mut self, id: StableNodeId) {
        if self.nodes.contains(id) {
            let _ = self.mark(id, crate::schedule::DirtyMask::LAYOUT);
        }
    }

    /// Whether the node is `Mounted`. Parked is not; Detach stays mounted but
    /// is omitted from the live document until inserted.
    pub fn is_mounted(&self, id: StableNodeId) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|node| node.mount == MountState::Mounted)
    }

    pub fn mount_state(&self, id: StableNodeId) -> Option<MountState> {
        self.nodes.get(id).map(|node| node.mount)
    }

    pub fn is_retired(&self, id: StableNodeId) -> bool {
        self.retired.contains(id)
    }

    /// Total IDs retired over this world's lifetime.
    pub fn retired_ids(&self) -> usize {
        self.retired.len()
    }

    /// Coalesced runs backing the retired ledger. Sequential allocation keeps
    /// this near-constant while [`Self::retired_ids`] grows with churn.
    pub fn retired_id_runs(&self) -> usize {
        self.retired.runs()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether any node still owes system work, i.e. whether the next flush has
    /// anything to do. [`Self::take_system_work`] drains this.
    ///
    /// A host that wrote into a document and wants to know whether that write
    /// actually changed anything asks here: an update that projects no
    /// mutations leaves nothing dirty. Answering it from the outside otherwise
    /// means re-deriving per-field diffs the commit already computed.
    pub fn has_pending_work(&self) -> bool {
        // The removal queues are drained by the same call and are refilled
        // independently of the dirty set when a frame does not settle, so
        // reporting only dirty nodes would answer "clean" while work waits.
        !self.dirty_entities.is_empty()
            || !self.pending_accessibility_removals.is_empty()
            || !self.pending_render_removals.is_empty()
    }

    /// Monotonic revision of work scheduled since world creation. Reading it
    /// is O(1) and has no effect on the frame's pending queues.
    pub fn pending_work_revision(&self) -> u64 {
        self.pending_work_revision
    }

    /// Algorithm-level counters from the last non-empty drain, or the current
    /// frame accumulator while a product flush is running. An idle
    /// [`Self::take_system_work`] does not replace this snapshot.
    pub fn last_work_counters(&self) -> WorkCounters {
        let mut counters = self.last_counters;
        counters.record_hot_path_allocation(
            self.pending_hot_allocations.get(),
            self.pending_hot_allocated_bytes.get(),
        );
        counters
    }

    /// Start a multi-pass frame accumulator. Idle drains still leave the
    /// previous snapshot in place until a non-empty pass runs.
    pub fn begin_frame_counters(&mut self) {
        self.commit_pending_hot_allocs();
        self.commit_pending_theme_work();
        self.frame_counters = WorkCounters::default();
        self.text_frame_work = nana_text::TextWorkCounters::default();
        self.theme_frame_work = ThemeWorkCounters::default();
        self.frame_extracted_nodes = 0;
        self.frame_extracted_spans = 0;
        self.accumulating_frame = true;
    }

    pub fn end_frame_counters(&mut self) {
        self.accumulating_frame = false;
    }

    /// Record extract output onto the last drained work counters. Draw batches
    /// and GPU upload bytes are omitted; extraction does not measure them.
    pub fn record_extract(&mut self, extracted: &[ExtractedNode]) {
        let spans = extracted.iter().map(|node| node.text_spans.len()).sum();
        if self.accumulating_frame {
            self.frame_extracted_nodes = self.frame_extracted_nodes.saturating_add(extracted.len());
            self.frame_extracted_spans = self.frame_extracted_spans.saturating_add(spans);
            self.last_counters.render_nodes_extracted = self.frame_extracted_nodes;
            self.last_counters.extracted_text_spans = self.frame_extracted_spans;
        } else {
            self.last_counters.render_nodes_extracted = extracted.len();
            self.last_counters.extracted_text_spans = spans;
        }
    }

    /// Observe a CPU hot-path heap event from a `&self` path (layout inputs,
    /// document order). Folded into [`Self::last_work_counters`].
    pub fn record_hot_path_allocation(&self, count: usize, bytes: usize) {
        if count == 0 && bytes == 0 {
            return;
        }
        self.pending_hot_allocations
            .set(self.pending_hot_allocations.get().saturating_add(count));
        self.pending_hot_allocated_bytes
            .set(self.pending_hot_allocated_bytes.get().saturating_add(bytes));
    }

    fn commit_pending_hot_allocs(&mut self) {
        let count = self.pending_hot_allocations.replace(0);
        let bytes = self.pending_hot_allocated_bytes.replace(0);
        self.last_counters.record_hot_path_allocation(count, bytes);
        if self.accumulating_frame {
            self.frame_counters.record_hot_path_allocation(count, bytes);
        }
    }

    /// Text work (Issue #95) of the last shaping pass, or of every pass of
    /// the last frame [`Self::begin_frame_counters`] accumulated that ran one: nodes
    /// considered, skipped on revision alone, shaped, and what the passes
    /// cloned, hashed and looked up to get there.
    pub fn last_text_work_counters(&self) -> nana_text::TextWorkCounters {
        self.text_work
    }

    /// Theme/style work (Issue #101 §4) of the last style pass or theme
    /// install, or of every pass of the last frame
    /// [`Self::begin_frame_counters`] accumulated that ran one: nodes
    /// considered, resolved and skipped, token reads, and what the pass cost
    /// Layout, Text and Paint.
    ///
    /// An idle frame leaves the previous snapshot in place, like
    /// [`Self::last_work_counters`].
    ///
    /// One theme change is more than one event — the install marks the world,
    /// the drain schedules it, the style pass resolves it — so measure it
    /// inside [`Self::begin_frame_counters`] / [`Self::end_frame_counters`].
    /// Outside that accumulator each event replaces the last, exactly as
    /// [`Self::last_text_work_counters`] does.
    pub fn last_theme_work_counters(&self) -> ThemeWorkCounters {
        let mut counters = self.theme_work;
        counters.accumulate(self.pending_theme_work());
        counters
    }

    /// The theme/style work observed since the last fold, as counters.
    fn pending_theme_work(&self) -> ThemeWorkCounters {
        let mut pending = ThemeWorkCounters::default();
        pending.record_theme_reads(self.pending_theme_reads.get());
        pending.record_layout_copy(
            self.pending_layout_copies.get(),
            self.pending_layout_copies
                .get()
                .saturating_mul(size_of::<nana_ui_core::LayoutStyle>()),
        );
        pending
    }

    /// Take that work, leaving nothing behind for the next window.
    fn take_pending_theme_work(&self) -> ThemeWorkCounters {
        let pending = self.pending_theme_work();
        self.pending_theme_reads.set(0);
        self.pending_layout_copies.set(0);
        pending
    }

    /// Observe one read of the token authority from a `&self` palette path.
    fn record_theme_read(&self) {
        self.pending_theme_reads
            .set(self.pending_theme_reads.get().saturating_add(1));
    }

    /// Observe the `LayoutStyle` a style write had to copy to hold the
    /// resolved value beside the authored intent. It is the largest heap
    /// event on the style path and would otherwise go unwatched: it happens
    /// under `Arc::make_mut`, which no allocator counter sees.
    fn record_resolved_layout_copy(&self) {
        self.pending_layout_copies
            .set(self.pending_layout_copies.get().saturating_add(1));
    }

    /// Close the pending work onto the window it was observed in, so an event
    /// from before [`Self::begin_frame_counters`] is not charged to the frame
    /// that follows it.
    fn commit_pending_theme_work(&mut self) {
        let pending = self.take_pending_theme_work();
        if pending == ThemeWorkCounters::default() {
            return;
        }
        self.theme_work.accumulate(pending);
        if self.accumulating_frame {
            self.theme_frame_work.accumulate(pending);
        }
    }

    fn record_theme_work(&mut self, mut work: ThemeWorkCounters) {
        work.accumulate(self.take_pending_theme_work());
        if self.accumulating_frame {
            self.theme_frame_work.accumulate(work);
            self.theme_work = self.theme_frame_work;
        } else {
            self.theme_work = work;
        }
    }

    fn record_text_work(&mut self, mut work: nana_text::TextWorkCounters) {
        work.accumulate(std::mem::take(&mut self.pending_edit_work));
        if self.accumulating_frame {
            // Published as the frame goes, so an idle frame (no text pass)
            // leaves the last frame that had one in place, as
            // `last_work_counters` does.
            self.text_frame_work.accumulate(work);
            self.text_work = self.text_frame_work;
        } else {
            self.text_work = work;
        }
    }

    fn bump_last_counters(&mut self, update: impl Fn(&mut WorkCounters)) {
        update(&mut self.last_counters);
        if self.accumulating_frame {
            update(&mut self.frame_counters);
        }
    }

    fn record_id_list_alloc(&self, len: usize) {
        if len == 0 {
            return;
        }
        self.record_hot_path_allocation(1, len.saturating_mul(size_of::<StableNodeId>()));
    }

    fn record_string_clone(&self, len: usize) {
        if len == 0 {
            return;
        }
        self.record_hot_path_allocation(1, len);
    }

    pub fn theme_mode(&self) -> ThemeMode {
        self.style_model.theme_mode
    }

    pub fn theme_metrics(&self) -> nana_ui_core::ThemeMetrics {
        self.style_model.metrics
    }

    pub fn style_model(&self) -> StyleModelRef {
        self.style_model
    }

    /// The installed design system.
    ///
    /// Returned by reference: a [`CompiledTheme`](nana_ui_core::CompiledTheme)
    /// is around a kilobyte, and handing one out by value on a read path is
    /// the mistake Issue #101 §1.5 measured with `LayoutStyle`. Callers that
    /// only want a colour or a metric take [`Self::style_model`] instead.
    pub fn theme(&self) -> &nana_ui_core::CompiledTheme {
        &self.theme
    }

    /// Drain dirty components into deterministic system work. Calling this on
    /// an unchanged world returns an empty work set and performs no scheduling.
    pub fn take_system_work(&mut self) -> SystemWork {
        let mut ids = std::mem::take(&mut self.dirty_entities)
            .into_iter()
            .collect::<Vec<_>>();
        ids.sort_unstable();
        let dirty_len = ids.len();
        let mut work = SystemWork {
            generation: self.generation,
            style: Vec::new(),
            state: Vec::new(),
            text: Vec::new(),
            layout: Vec::new(),
            transform: Vec::new(),
            input_hit_test: Vec::new(),
            focus_ime: Vec::new(),
            accessibility: Vec::new(),
            accessibility_removals: sorted_unique(std::mem::take(
                &mut self.pending_accessibility_removals,
            )),
            render_extraction: Vec::new(),
            render_removals: sorted_unique(std::mem::take(&mut self.pending_render_removals)),
            entities_total: self.nodes.len(),
            entities_changed: 0,
            entities_spawned: std::mem::take(&mut self.spawned_since_drain),
            entities_despawned: std::mem::take(&mut self.despawned_since_drain),
            input_targets: self.live_input_target_count(),
            render_nodes_changed: 0,
            render_nodes_extracted: 0,
            extracted_text_spans: 0,
            allocations: 0,
            allocated_bytes: 0,
            text_shaped_runs: 0,
            text_layout_cache_hits: 0,
            text_layout_cache_misses: 0,
            text_wrap_layouts: 0,
            glyph_cache_hits: None,
            glyph_cache_misses: None,
            cache_eviction: None,
            // A drain always follows the commits it drains, so 0 here is an
            // observed "validated nothing", not a missing measurement.
            validation_nodes_scanned: Some(std::mem::take(&mut self.validation_nodes_scanned)),
        };
        work.render_removals.sort_unstable();
        work.accessibility_removals.sort_unstable();
        for id in ids {
            let bits = self.record_mut(id).dirty.take();
            if !self.presence_live(id) {
                continue;
            }
            let has_text = matches!(self.record(id).kind.as_ref(), NodeKind::Text)
                || !self.record(id).text.value.is_empty()
                || matches!(
                    self.nodes.visual(id),
                    Some(StandardVisual::EmptyState { .. })
                        | Some(StandardVisual::ModalFrame { .. })
                );
            let bits = if has_text {
                bits
            } else {
                bits & !DirtyMask::TEXT
            };
            if bits != 0 {
                work.entities_changed += 1;
            }
            push_work(&mut work, id, bits);
        }
        work.render_nodes_changed = work.render_extraction.len();
        work.render_nodes_extracted = work.render_extraction.len();
        let mut drain_allocs = 0usize;
        let mut drain_bytes = 0usize;
        let mut bump_list = |len: usize| {
            if len > 0 {
                drain_allocs = drain_allocs.saturating_add(1);
                drain_bytes =
                    drain_bytes.saturating_add(len.saturating_mul(size_of::<StableNodeId>()));
            }
        };
        bump_list(dirty_len);
        bump_list(work.style.len());
        bump_list(work.state.len());
        bump_list(work.text.len());
        bump_list(work.layout.len());
        bump_list(work.transform.len());
        bump_list(work.input_hit_test.len());
        bump_list(work.focus_ime.len());
        bump_list(work.accessibility.len());
        bump_list(work.accessibility_removals.len());
        bump_list(work.render_extraction.len());
        bump_list(work.render_removals.len());
        work.record_hot_path_allocation(drain_allocs, drain_bytes);
        if !work.is_empty() {
            self.pending_hot_allocations.set(0);
            self.pending_hot_allocated_bytes.set(0);
            let mut counters = work.counters();
            // Extracted node/span fields are filled by [`Self::record_extract`]
            // on the product path, not by the planned render list.
            counters.render_nodes_extracted = 0;
            counters.extracted_text_spans = 0;
            if self.accumulating_frame {
                self.frame_counters.accumulate(counters);
                self.last_counters = self.frame_counters;
                self.last_counters.render_nodes_extracted = self.frame_extracted_nodes;
                self.last_counters.extracted_text_spans = self.frame_extracted_spans;
            } else {
                self.last_counters = counters;
            }
        } else {
            self.commit_pending_hot_allocs();
        }
        work
    }

    /// Count UI frames this world would emit over `ticks` host attempts with no
    /// external vsync. A frame is a non-empty dirty drain ([`Self::take_system_work`]).
    /// Empty drains do not count. Elapsed time and `idle_schedule_ms` are not frames.
    pub fn scheduled_ui_frames(&mut self, ticks: usize) -> usize {
        let mut frames = 0;
        for _ in 0..ticks {
            if self.take_system_work().is_empty() {
                continue;
            }
            frames += 1;
        }
        frames
    }

    /// Restore drained work after a frame-system failure. Derived writes are
    /// idempotent, so retrying the complete transaction is safer than losing
    /// accessibility or render invalidations from an earlier pass.
    pub fn restore_system_work(&mut self, work: SystemWork) {
        for (ids, bit) in [
            (work.style, DirtyMask::STYLE),
            (work.state, DirtyMask::STATE),
            (work.text, DirtyMask::TEXT),
            (work.layout, DirtyMask::LAYOUT),
            (work.transform, DirtyMask::TRANSFORM),
            (work.input_hit_test, DirtyMask::INPUT),
            (work.focus_ime, DirtyMask::FOCUS_IME),
            (work.accessibility, DirtyMask::ACCESSIBILITY),
            (work.render_extraction, DirtyMask::RENDER),
        ] {
            for id in ids {
                if !self.nodes.contains(id) {
                    continue;
                };
                let changed = self
                    .nodes
                    .get_mut(id)
                    .map(|n| &mut n.dirty)
                    .expect("retained node must have dirty state")
                    .insert(bit);
                if changed {
                    self.dirty_entities.insert(id);
                    self.pending_work_revision = self.pending_work_revision.saturating_add(1);
                }
            }
        }
        self.pending_accessibility_removals
            .extend(work.accessibility_removals);
        self.pending_accessibility_removals.sort_unstable();
        self.pending_accessibility_removals.dedup();
        self.pending_render_removals.extend(work.render_removals);
        self.pending_render_removals.sort_unstable();
        self.pending_render_removals.dedup();
        self.spawned_since_drain = self
            .spawned_since_drain
            .saturating_add(work.entities_spawned);
        self.despawned_since_drain = self
            .despawned_since_drain
            .saturating_add(work.entities_despawned);
    }

    pub fn node(&self, id: StableNodeId) -> Option<NodeSnapshot> {
        let node = self.nodes.get(id)?;
        Some(NodeSnapshot {
            id,
            document: node.document,
            kind: node.kind.as_ref().clone(),
            parent: node.hierarchy.parent,
            children: node.hierarchy.children.as_ref().clone(),
        })
    }

    pub fn focused(&self, document: DocumentId) -> Option<StableNodeId> {
        self.input.focused.get(&document).copied()
    }

    /// Record which kind of device the event being routed came from.
    ///
    /// Hosts call this from their one input entry point. The modality is read
    /// again each time focus is written, so a click makes exactly the focus it
    /// causes invisible — not every focus after it.
    pub fn note_input_modality(&mut self, document: DocumentId, modality: InputModality) {
        match modality {
            InputModality::Pointer => self.input.pointer_modality.insert(document),
            InputModality::Keyboard => self.input.pointer_modality.remove(&document),
        };
    }

    /// The focused node, when focus should also be *shown*.
    ///
    /// Same answer as [`Self::focused`] except right after a pointer press put
    /// it there. Paint asks this one; hit-testing, IME and the accessibility
    /// tree keep asking [`Self::focused`], because focus is still focus — only
    /// its indicator is conditional.
    pub fn focus_visible(&self, document: DocumentId) -> Option<StableNodeId> {
        if self.input.focus_from_pointer.contains(&document) {
            return None;
        }
        self.focused(document)
    }

    pub fn focused_text_input(
        &self,
        document: DocumentId,
    ) -> Option<(StableNodeId, crate::TextInputView<'_>)> {
        let id = self.focused(document)?;
        Some((id, self.text_input(id)?))
    }

    pub fn text(&self, id: StableNodeId) -> Option<&str> {
        self.nodes
            .get(id)
            .map(|n| &n.text)
            .map(|text| text.value.as_str())
    }

    pub fn document_text_selection(
        &self,
        document: DocumentId,
    ) -> Option<&crate::DocumentTextSelection> {
        self.document_text_selections.get(&document)
    }

    pub fn set_document_text_selection(
        &mut self,
        document: DocumentId,
        selection: Option<crate::DocumentTextSelection>,
    ) {
        if self.document_text_selections.get(&document) == selection.as_ref() {
            return;
        }
        if let Some(previous) = self.document_text_selections.remove(&document)
            && self.contains(previous.node)
        {
            let _ = self.mark(previous.node, DirtyMask::RENDER);
        }
        if let Some(selection) = selection {
            if self.contains(selection.node) {
                let _ = self.mark(selection.node, DirtyMask::RENDER);
            }
            self.document_text_selections.insert(document, selection);
        }
    }

    pub(crate) fn drop_invalid_document_text_selections(&mut self) {
        let stale: Vec<DocumentId> = self
            .document_text_selections
            .iter()
            .filter_map(|(&document, selection)| {
                let keep = self.contains(selection.node)
                    && self
                        .computed_style(selection.node)
                        .is_some_and(|style| style.user_select.allows_document_select());
                (!keep).then_some(document)
            })
            .collect();
        for document in stale {
            self.set_document_text_selection(document, None);
        }
    }

    pub fn layout_box(&self, id: StableNodeId) -> Option<LayoutBox> {
        self.nodes.get(id).map(|node| node.layout)
    }

    /// Where `id` is scrolled: the offset last set, except that a multiline
    /// editor reports where it is drawn — that request clamped to its value
    /// and, while focused, moved to show the caret.
    pub fn scroll_offset(&self, id: StableNodeId) -> Option<ScrollOffset> {
        let node = self.nodes.get(id)?;
        if node.accessibility.multiline
            && let Some(frame) = self.editor_frame(id)
        {
            return Some(frame.scroll_offset());
        }
        Some(node.scroll_offset)
    }

    /// The scrolling area a scroll offset is clamped to. A multiline text
    /// editor's follows its shaped value, so it is always current; every
    /// other container's is the one last published for it.
    /// The offset last set on `id`, which a multiline editor is drawn from.
    pub(crate) fn scroll_request(&self, id: StableNodeId) -> Option<ScrollOffset> {
        self.nodes.get(id).map(|node| node.scroll_offset)
    }

    pub fn scroll_metrics(&self, id: StableNodeId) -> Option<ScrollMetrics> {
        self.text_scroll_metrics(id)
            .or_else(|| self.nodes.scroll_metrics(id).copied())
    }

    /// A box styled `overflow: auto | scroll`, or a `ScrollView` (its
    /// scrollbar visual) whatever chrome restyled its overflow — a workspace
    /// region borrowing it as its surface, say.
    pub(crate) fn is_scroll_container(&self, id: StableNodeId) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|record| scroll_container(&record.style.layout, self.nodes.visual(id)))
    }

    /// The scrolling area of `id`'s laid-out box over its descendants' boxes.
    pub(crate) fn layout_scroll_metrics(&self, id: StableNodeId) -> Option<ScrollMetrics> {
        let viewport = self.layout_box(id)?;
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            return None;
        }
        let extent = self.scroll_content_extent(id);
        Some(ScrollMetrics::scrolling_area(
            viewport,
            [extent.left, extent.top, extent.right, extent.bottom],
            self.scroll_far_start_axes(id),
        ))
    }

    /// Which page axes of scroll container `id` start at the right / bottom
    /// (`[horizontal, vertical]`), where its scroll origin then sits: an RTL
    /// inline axis, `vertical-rl`'s block axis, a `*-reverse` flex main axis.
    pub fn scroll_far_start_axes(&self, id: StableNodeId) -> [bool; 2] {
        crate::layout_engine::far_start_axes(self, id)
    }

    /// Clamp to the scrolling area. A container nothing has published
    /// metrics for yet is measured here, so its range is never guessed; one
    /// with no laid-out box has nothing to scroll past its origin.
    pub fn clamp_scroll_offset(&self, id: StableNodeId, offset: ScrollOffset) -> ScrollOffset {
        let measured = || {
            self.is_scroll_container(id)
                .then(|| self.layout_scroll_metrics(id))
                .flatten()
        };
        match self.scroll_metrics(id).or_else(measured) {
            Some(metrics) => metrics.clamp(offset),
            None => ScrollOffset {
                x: offset.x.max(0.0),
                y: offset.y.max(0.0),
            },
        }
    }

    pub fn node_style(&self, id: StableNodeId) -> Option<&NodeStyle> {
        self.nodes.get(id).map(|node| &node.style)
    }

    pub fn computed_style(&self, id: StableNodeId) -> Option<&ComputedStyle> {
        self.nodes.get(id).map(|node| node.resolved.0.as_ref())
    }

    /// Whether the text backend changed since the last call, and clears it.
    pub(crate) fn take_text_backend_changed(&mut self) -> bool {
        std::mem::take(&mut self.text_backend_changed)
    }

    /// Measures chrome text painted on `id` through the engine that shaped
    /// this world, or by estimate before any engine has.
    pub(crate) fn chrome_text_measure(
        &self,
        id: StableNodeId,
    ) -> crate::text_width::ChromeTextMeasure<'_> {
        crate::text_width::ChromeTextMeasure::new(
            self.paint_text_engine.as_ref(),
            self.computed_style(id),
        )
    }

    /// Whether a mounted node is visible through every retained overlay branch.
    /// Dirty computed styles are derived from the local hierarchy instead of
    /// treating the previous frame's visibility as current authority.
    pub fn is_overlay_reachable(&self, id: StableNodeId) -> bool {
        let mut visibility = None;
        let mut child = id;
        let mut current = Some(id);
        while let Some(candidate) = current {
            // Visibility inherits from the nearest declaration; a visible child
            // may override a hidden parent. Read authored styles while dirty.
            visibility = visibility.or_else(|| {
                self.node_style(candidate)
                    .and_then(|style| style.layout.paint.visibility)
            });
            if !self.presence_live(candidate)
                || !self.menu_branch_open(candidate)
                || self
                    .node_style(candidate)
                    .is_some_and(|style| style.layout.omits_box())
            {
                return false;
            }
            let parent = self.parent_id(candidate);
            if let Some(parent) = parent {
                if self
                    .overlay_host(parent)
                    .is_some_and(|state| state.active != Some(child))
                {
                    return false;
                }
                child = parent;
            }
            current = parent;
        }
        visibility != Some(nana_ui_core::VisibilitySpec::Hidden)
    }

    pub fn interaction(&self, id: StableNodeId) -> Option<InteractionState> {
        self.nodes.get(id).map(|node| node.interaction)
    }

    /// The editor's committed text and selections, read from its session
    /// without a copy.
    pub fn text_input(&self, id: StableNodeId) -> Option<crate::TextInputView<'_>> {
        self.nodes.text_input(id)
    }

    pub fn text_input_presentation(&self, id: StableNodeId) -> Option<&TextInputPresentation> {
        self.nodes.text_input_presentation(id)
    }

    pub fn highlight_request(&self, id: StableNodeId) -> Option<&HighlightRequest> {
        self.nodes.highlight(id)
    }

    pub fn text_presentation(&self, id: StableNodeId) -> Option<&TextPresentation> {
        self.nodes.text_presentation(id)
    }

    pub fn has_presenter(&self, name: &str) -> bool {
        self.presenters.contains_key(name)
    }

    /// Install a named text presenter. Matching [`HighlightRequest`] nodes are
    /// marked dirty so the next TEXT system can derive spans.
    pub fn register_presenter(
        &mut self,
        presenter: Box<dyn TextPresenter>,
    ) -> Result<(), UiWorldError> {
        let name = presenter.name().trim();
        if name.is_empty() {
            return Err(UiWorldError::InvalidPresenter);
        }
        if self.presenters.contains_key(name) {
            return Err(UiWorldError::DuplicatePresenter(name.to_owned()));
        }
        let name = name.to_owned();
        self.presenters.insert(name.clone(), presenter);
        let ids = self.nodes.keys().collect::<Vec<_>>();
        for id in ids {
            if self
                .nodes
                .highlight(id)
                .is_some_and(|request| request.presenter.as_ref() == name)
            {
                self.mark(id, DirtyMask::TEXT | DirtyMask::RENDER);
            }
        }
        Ok(())
    }

    /// Derive [`TextPresentation`] for scheduled text nodes. Committed text
    /// only; IME preedit is ignored here and omitted from extraction.
    pub fn resolve_presentations(&mut self, ids: &[StableNodeId]) -> Result<(), UiWorldError> {
        for &id in ids {
            if !self.contains(id) {
                return Err(UiWorldError::MissingNode(id));
            }
            let Some(request) = self.nodes.highlight(id).cloned() else {
                if self.nodes.text_presentation(id).is_some() {
                    self.nodes.set_text_presentation(id, None);
                }
                continue;
            };
            let text = self.committed_presentation_text(id);
            let source = crate::presentation::presentation_source(&text, &request);
            if self
                .nodes
                .text_presentation(id)
                .is_some_and(|presentation| presentation.source == source)
            {
                continue;
            }
            // 基础层先出，语义 overlay 在 presenter 结果之后、sanitize 之前
            // 合并（overlay 段优先，重叠处丢基础层）；presenter 未注册时
            // 基础层为空，overlay 仍单独生效（宿主喂数据、框架只渲染）。
            let base = self
                .presenters
                .get(request.presenter.as_ref())
                .map(|presenter| presenter.present(&text, &request))
                .unwrap_or_default();
            let spans = match request.overlay.as_ref() {
                Some(overlay) => crate::presentation::sanitize_spans(
                    &text,
                    crate::presentation::merge_overlay_spans(base, overlay),
                ),
                None => crate::presentation::sanitize_spans(&text, base),
            };
            self.nodes
                .set_text_presentation(id, Some(TextPresentation { spans, source }));
        }
        Ok(())
    }

    fn committed_presentation_text(&self, id: StableNodeId) -> crate::TextValue {
        self.nodes
            .text_input(id)
            .map(|state| state.value_shared())
            .unwrap_or_else(|| self.record(id).text.value.clone())
    }

    pub fn text_metrics(&self, id: StableNodeId) -> Option<TextMetrics> {
        self.nodes.get(id).map(|node| node.text_metrics)
    }

    /// The width a plain text that wrapped to its box takes unwrapped, when
    /// that is wider than the lines it wrapped to: what it asks of layout
    /// once its box may grow. `None` for text that did not wrap.
    pub(crate) fn text_natural_width(&self, id: StableNodeId) -> Option<f32> {
        self.nodes.text_natural_width(id).copied()
    }

    /// The editor's IME state: its preedit, or an empty one while an IME is
    /// attached with nothing composed.
    pub fn ime(&self, id: StableNodeId) -> Option<crate::ImeView<'_>> {
        self.nodes.ime(id)
    }

    pub fn custom_render(&self, id: StableNodeId) -> Option<&CustomRenderNode> {
        self.nodes.custom_render(id)
    }

    pub fn has_event(&self, id: StableNodeId, event: &str) -> bool {
        self.event_listeners(id)
            .is_some_and(|listeners| listeners.contains(event))
    }

    pub fn event_listeners(&self, id: StableNodeId) -> Option<&EventListeners> {
        self.nodes.event_listeners(id)
    }

    pub fn component_type(&self, id: StableNodeId) -> Option<&ComponentTypeId> {
        self.nodes.component_type(id)
    }

    /// Live nodes of one component type in `document`, in no particular order.
    ///
    /// Callers that used to walk `document_order` and filter cost the number of
    /// that component instead of the world size.
    pub fn nodes_of_component(
        &self,
        document: DocumentId,
        component: &str,
    ) -> impl Iterator<Item = StableNodeId> + '_ {
        self.nodes_by_component
            .get(component)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |id| self.document_of(*id) == Some(document))
    }

    /// Keep [`Self::nodes_by_component`] in step with a node's component type.
    fn reindex_component(&mut self, id: StableNodeId, next: Option<&ComponentTypeId>) {
        if let Some(previous) = self.nodes.component_type(id).cloned()
            && let Some(nodes) = self.nodes_by_component.get_mut(&previous)
        {
            nodes.remove(&id);
            if nodes.is_empty() {
                self.nodes_by_component.remove(&previous);
            }
        }
        if let Some(next) = next {
            self.nodes_by_component
                .entry(next.clone())
                .or_default()
                .insert(id);
        }
    }

    fn note_structural_change(&mut self, parent: StableNodeId) {
        if self.structural_change_parents.last() != Some(&parent) {
            self.structural_change_parents.push(parent);
        }
    }

    /// Parents whose child list changed since the last drain, deduplicated in
    /// ascending order. Consumers must drain per commit so scheduled
    /// reprojections observe the post-mutation tree.
    pub fn take_structural_change_parents(&mut self) -> Vec<StableNodeId> {
        let mut parents = std::mem::take(&mut self.structural_change_parents);
        parents.sort_unstable();
        parents.dedup();
        parents
    }

    pub fn event_targets(&self, document: DocumentId) -> HashSet<(u64, String)> {
        self.document_order(document)
            .into_iter()
            .flat_map(|id| {
                self.event_listeners(id)
                    .into_iter()
                    .flat_map(move |listeners| {
                        listeners
                            .iter()
                            .map(move |event| (id.get(), event.to_string()))
                    })
            })
            .collect()
    }

    pub fn standard_visual(&self, id: StableNodeId) -> Option<StandardVisual> {
        self.nodes.visual(id).cloned()
    }

    /// [`Self::standard_visual`] without the copy, for projections that
    /// compare before they write.
    pub(crate) fn standard_visual_ref(&self, id: StableNodeId) -> Option<&StandardVisual> {
        self.nodes.visual(id)
    }

    pub fn component_geometry(&self, id: StableNodeId) -> Option<crate::ComponentGeometry> {
        let visual = self.nodes.visual(id)?;
        let style = self.nodes.get(id)?.resolved.0.as_ref();
        self.derive_component_geometry(id, visual, style)
    }

    pub(crate) fn component_content_box(&self, id: StableNodeId) -> Option<LayoutBox> {
        let node = self.nodes.get(id)?;
        let bounds = node.layout;
        let padding = self.used_layout_padding(id);
        let border = node.style.layout.resolved_border_width();
        Some(LayoutBox {
            x: bounds.x + border + padding.left,
            y: bounds.y + border + padding.top,
            width: (bounds.width - border * 2.0 - padding.left - padding.right).max(0.0),
            height: (bounds.height - border * 2.0 - padding.top - padding.bottom).max(0.0),
        })
    }

    pub fn accessibility(&self, id: StableNodeId) -> Option<&AccessibilityState> {
        self.nodes.get(id).map(|node| &node.accessibility)
    }

    /// The node whose text names `id` when it has no label of its own; see
    /// [`MutationQueue::set_labelled_by`].
    pub fn labelled_by(&self, id: StableNodeId) -> Option<StableNodeId> {
        self.labelled_by.get(&id).copied()
    }

    /// Relate `id` to the node that names it, or drop the relation.
    fn set_labelled_by(&mut self, id: StableNodeId, label: Option<StableNodeId>) {
        if let Some(previous) = self.labelled_by.remove(&id)
            && let Some(controls) = self.labels.get_mut(&previous)
        {
            controls.remove(&id);
            if controls.is_empty() {
                self.labels.remove(&previous);
            }
        }
        if let Some(label) = label {
            self.labelled_by.insert(id, label);
            self.labels.entry(label).or_default().insert(id);
        }
    }

    /// Drop every naming relation `id` is an end of: it is gone.
    fn forget_labelled_by(&mut self, id: StableNodeId) {
        self.set_labelled_by(id, None);
        if let Some(controls) = self.labels.remove(&id) {
            for control in controls {
                self.labelled_by.remove(&control);
                if self.nodes.contains(control) {
                    self.mark(control, DirtyMask::ACCESSIBILITY);
                }
            }
        }
    }

    pub fn overlay_host(&self, id: StableNodeId) -> Option<OverlayHostState> {
        self.nodes.overlay_host(id).copied()
    }

    /// Nodes that carry an `OverlayHostState`. Overlay validation iterates this
    /// instead of the entity index so cost tracks host count, not world size.
    pub(crate) fn overlay_host_ids(
        &self,
        document: DocumentId,
    ) -> impl Iterator<Item = StableNodeId> + '_ {
        self.overlay_hosts_by_document
            .get(&document)
            .into_iter()
            .flatten()
            .copied()
    }

    /// Drop focus and composition when dirty visual or interaction state makes
    /// the focused node ineligible.
    pub fn reconcile_focus(&mut self, ids: &[StableNodeId]) {
        let dirty = ids.iter().copied().collect::<HashSet<_>>();
        let invalid_focus = self
            .input
            .focused
            .iter()
            .filter_map(|(&document, &id)| {
                let invalid = dirty.contains(&id)
                    && (!self.record(id).resolved.0.visible
                        || !self.record(id).interaction.focusable);
                invalid.then_some((document, id))
            })
            .collect::<Vec<_>>();
        for (document, id) in invalid_focus {
            self.input.focused.remove(&document);
            self.remove_ime(id);
            self.mark_focus_changed(id);
        }
    }

    pub fn layout_inputs(&self, ids: &[StableNodeId]) -> Result<Vec<LayoutInput>, UiWorldError> {
        if !ids.is_empty() {
            self.record_hot_path_allocation(1, ids.len().saturating_mul(size_of::<LayoutInput>()));
        }
        ids.iter().map(|&id| self.layout_input(id)).collect()
    }

    /// Project a single layout input without allocating a batch container.
    pub(crate) fn layout_input(&self, id: StableNodeId) -> Result<LayoutInput, UiWorldError> {
        let record = self.nodes.get(id).ok_or(UiWorldError::MissingNode(id))?;
        let has_text =
            matches!(record.kind.as_ref(), NodeKind::Text) || !record.text.value.is_empty();
        let writing = record_writing(record);
        Ok(LayoutInput {
            id,
            parent: record.hierarchy.parent,
            children: Arc::clone(&record.hierarchy.children),
            style: self.motion_layout(id, &self.effective_layout_style(id)),
            writing,
            containing_writing: record_containing_writing(record),
            text_metrics: has_text.then_some(record.text_metrics),
            modal: self.nodes.visual(id).and_then(|visual| {
                let StandardVisual::ModalFrame { kind, slots, .. } = visual else {
                    return None;
                };
                let presentation = self.nodes.modal_text(id).copied().unwrap_or_default();
                Some(crate::ModalLayoutInput {
                    kind: *kind,
                    slots: slots.clone(),
                    title: presentation.title,
                    description: presentation.description,
                    body_text: presentation.body,
                })
            }),
        })
    }

    pub(crate) fn write_layout_padding(
        &mut self,
        id: StableNodeId,
        padding: nana_ui_core::PaddingSpec,
    ) -> bool {
        let record = self.record_mut(id);
        let changed = record.layout_padding != Some(padding);
        record.layout_padding = Some(padding);
        if changed {
            self.nodes
                .invalidate_text(id, crate::text_node::TextDirty::CONSTRAINT);
        }
        changed
    }

    /// Record which page axes layout placed `id`'s children from the far
    /// end. Returns whether that changed, which moves the scroll origin.
    pub(crate) fn write_layout_far_start(&mut self, id: StableNodeId, far: [bool; 2]) -> bool {
        let record = self.record_mut(id);
        let changed = record.layout_far_start != Some(far);
        record.layout_far_start = Some(far);
        changed
    }

    pub(crate) fn layout_far_start(&self, id: StableNodeId) -> Option<[bool; 2]> {
        self.nodes.get(id)?.layout_far_start
    }

    /// Padding resolved by the layout pass, including its containing block and font.
    pub(crate) fn used_layout_padding(&self, id: StableNodeId) -> nana_ui_core::PaddingSpec {
        let record = self.record(id);
        record.layout_padding.unwrap_or_else(|| {
            // Intent (radius / control height / padding) lives on
            // `resolved_layout`. Falling back to the authored box would
            // report 0 for every control that named its inset instead of
            // spending the token.
            record
                .resolved_layout
                .resolved_padding_against(Some(record.layout.width))
        })
    }

    /// Layout-facing style without assembling a [`LayoutInput`].
    ///
    /// Parked, detached, and inactive-overlay nodes match [`Self::layout_inputs`]:
    /// the returned style reports [`nana_ui_core::LayoutStyle::omits_box`].
    pub(crate) fn layout_style(&self, id: StableNodeId) -> Option<Arc<nana_ui_core::LayoutStyle>> {
        self.contains(id).then(|| self.effective_layout_style(id))
    }

    /// True when `effective_layout_style` for `parent`'s children reduces to
    /// each child's own `style.layout`, with no adjustment derived from an
    /// ancestor.
    ///
    /// The scoped layout engine reuses a container's cached child placement
    /// when nothing in the change closure altered it. That is only sound if a
    /// child's layout style cannot change without the child itself being
    /// marked LAYOUT-dirty -- and `set_style` guarantees exactly that (it
    /// marks the subtree when layout semantics change). The two escapes are
    /// the adjustments below, each derived from an ancestor and so able to
    /// move without touching the child:
    ///
    /// - `overlay_branch_active`, when the parent is an overlay host;
    /// - `menu_branch_open` / `parent_triggered_overlay`, when the parent is a
    ///   menu surface.
    ///
    /// `presence_live` is ancestor-derived as well, but it never differs
    /// between a parent and its children: only an unlinked root is ever
    /// `detached`, a child in `parent`'s list shares its mount state, and a
    /// subtree that comes back is inserted with all of it marked dirty. A
    /// parked node elsewhere in the world does not disable the plans.
    ///
    /// Logical edges landed in an inherited writing context are derived from
    /// an ancestor too, but they do not escape: changing an ancestor's
    /// `writing-mode` or `direction` marks its whole subtree LAYOUT-dirty.
    ///
    /// Reporting false is always safe: it only costs the caller its fast path.
    pub(crate) fn children_layout_style_is_local(&self, parent: StableNodeId) -> bool {
        (self.overlay_host_nodes.is_empty() || self.overlay_host(parent).is_none())
            && !matches!(
                self.nodes.visual(parent),
                Some(StandardVisual::MenuSurface { .. })
            )
    }

    fn effective_layout_style(&self, id: StableNodeId) -> Arc<nana_ui_core::LayoutStyle> {
        let record = self.record(id);
        let mut style = Arc::clone(&record.resolved_layout);
        if style.omits_box()
            || !self.presence_live(id)
            || !self.overlay_branch_active(id)
            || !self.menu_branch_open(id)
        {
            Arc::make_mut(&mut style).hidden = true;
            return style;
        }
        // A box that inherits its writing mode or direction resolved its
        // logical edges against its own declarations; land them against the
        // context it actually lays out in. The context comparison goes first:
        // it reads the record and two style fields, where the logical edges
        // sit on three other cache lines of a 4.8 KB style.
        let writing = record_writing(record);
        if writing != style.writing_context() && style.has_logical_box_edges() {
            Arc::make_mut(&mut style).resolve_logical_box_edges_in(writing);
        }
        if let Some(overlay) = self.parent_triggered_overlay(id) {
            apply_triggered_overlay(Arc::make_mut(&mut style), overlay);
        }
        style
    }

    /// The content of an open triggered menu (Popover, ActionMenu,
    /// HoverCard) as layout, hit testing and paint all see it: a
    /// viewport-fixed surface above the page. Extraction reads the resolved
    /// style, so it folds this in the same way `effective_layout_style` does.
    pub(super) fn triggered_overlay_layout(
        &self,
        id: StableNodeId,
        layout: &mut Arc<nana_ui_core::LayoutStyle>,
    ) {
        // Open triggered menus count among the z-index nodes; with none of
        // those there is no overlay content to find.
        if self.z_index_nodes == 0 {
            return;
        }
        if let Some(overlay) = self.parent_triggered_overlay(id) {
            apply_triggered_overlay(Arc::make_mut(layout), overlay);
        }
    }

    fn parent_triggered_overlay(&self, id: StableNodeId) -> Option<crate::TriggeredMenuOverlay> {
        let parent = self.record(id).hierarchy.parent?;
        match self.nodes.visual(parent)? {
            StandardVisual::MenuSurface {
                open: true,
                overlay: Some(overlay),
                ..
            } if overlay.trigger_content != Some(id) => Some(*overlay),
            _ => None,
        }
    }

    fn record(&self, id: StableNodeId) -> &NodeRecord {
        self.nodes
            .get(id)
            .expect("entity must have runtime component")
    }

    fn record_mut(&mut self, id: StableNodeId) -> &mut NodeRecord {
        self.nodes
            .get_mut(id)
            .expect("entity must have runtime component")
    }

    /// Records a change to a node's authored text.
    fn invalidate_text_content(&mut self, id: StableNodeId) {
        self.nodes
            .invalidate_text(id, crate::text_node::TextDirty::CONTENT);
        if self.record(id).text.value.is_empty() {
            // Emptied text has nothing to draw, and a non-Text element without
            // text never reaches a text pass that could release it later.
            self.nodes.release_text_layout(id);
        }
    }

    /// The writing mode and direction `id` lays out in. See
    /// [`record_writing`].
    pub(crate) fn layout_writing(&self, id: StableNodeId) -> nana_ui_core::WritingContext {
        self.nodes
            .get(id)
            .map_or_else(Default::default, record_writing)
    }

    /// The writing context of `id`'s containing block: its parent's (its own
    /// for a root). `id`'s percentage margins and paddings resolve against
    /// that block's inline size (CSS Box Model §5), so a node that sets a
    /// writing mode orthogonal to its parent's still resolves on the parent's
    /// inline axis.
    pub(crate) fn containing_writing(&self, id: StableNodeId) -> nana_ui_core::WritingContext {
        self.nodes
            .get(id)
            .map_or_else(Default::default, record_containing_writing)
    }

    /// The node's parent, without copying the node as [`Self::node`] does.
    pub fn parent_id(&self, id: StableNodeId) -> Option<StableNodeId> {
        self.nodes.get(id)?.hierarchy.parent
    }

    fn live_input_target_count(&self) -> usize {
        let mut ids = HashSet::new();
        ids.extend(self.input.focused.values().copied());
        ids.extend(self.input.pointer_hover.values().copied());
        ids.extend(self.input.pointer_press.values().copied());
        ids.extend(self.input.pointer_captures.values().copied());
        ids.len()
    }

    pub(crate) fn presence_live(&self, id: StableNodeId) -> bool {
        self.presence_live_memo(id, &mut AncestorMemo::default())
    }

    fn presence_live_memo(&self, id: StableNodeId, memo: &mut AncestorMemo) -> bool {
        if !self.is_mounted(id) {
            return false;
        }
        // Every detached root is parked, and a mounted node cannot sit under
        // a parked one: nothing to walk.
        if self.detached_mounted.is_empty() {
            debug_assert!(
                self.presence_live_walk(id),
                "{id:?} sits under a detached root"
            );
            return true;
        }
        memo.chain.clear();
        let mut current = Some(id);
        let mut live = true;
        while let Some(node) = current {
            if let Some(&known) = memo.live.get(&node) {
                live = known;
                break;
            }
            memo.chain.push(node);
            if self.detached.contains(&node) {
                live = false;
                break;
            }
            current = self.parent_id(node);
        }
        for node in memo.chain.drain(..) {
            memo.live.insert(node, live);
        }
        live
    }

    /// `presence_live` by the ancestor walk alone, to check the shortcut.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn presence_live_walk(&self, id: StableNodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if self.detached.contains(&node) {
                return false;
            }
            current = self.parent_id(node);
        }
        true
    }

    fn presence_flags_of(&self, id: StableNodeId) -> PresenceFlags {
        let visual = self.nodes.visual(id);
        let record = self.nodes.get(id);
        debug_assert!(
            record.is_none_or(|record| record.layout_depends_on_viewport
                == record.style.layout.depends_on_viewport()),
            "{id:?}'s cached viewport dependency went stale"
        );
        PresenceFlags {
            confirm: is_confirm_modal(visual),
            clip: is_clip_visual(visual),
            z_index: record.is_some_and(|record| record.style.layout.z_index.is_some())
                || is_triggered_menu_overlay(visual),
            triggered: is_triggered_menu_overlay(visual),
            viewport: record.is_some_and(|record| record.layout_depends_on_viewport),
        }
    }

    fn apply_presence_flags(&mut self, id: Option<StableNodeId>, next: PresenceFlags) {
        let previous = id
            .and_then(|id| self.presence_flags.get(&id).copied())
            .unwrap_or(PresenceFlags::NONE);
        if previous == next {
            return;
        }
        self.note_presence_counts(previous.confirm, next.confirm, previous.clip, next.clip);
        self.note_z_index_presence(previous.z_index, next.z_index);
        bump_presence(
            &mut self.viewport_basis_nodes,
            previous.viewport,
            next.viewport,
        );
        if let Some(id) = id
            && let Some(document) = self.document_of(id)
        {
            if next.viewport {
                self.viewport_basis.entry(document).or_default().insert(id);
            } else if let Some(ids) = self.viewport_basis.get_mut(&document) {
                ids.remove(&id);
                if ids.is_empty() {
                    self.viewport_basis.remove(&document);
                }
            }
        }
        if let Some(id) = id
            && previous.triggered != next.triggered
        {
            if next.triggered {
                self.triggered_overlays.insert(id);
            } else {
                self.triggered_overlays.remove(&id);
            }
        }
        if let Some(id) = id {
            if next == PresenceFlags::NONE {
                self.presence_flags.remove(&id);
            } else {
                self.presence_flags.insert(id, next);
            }
        }
    }

    fn sync_node_presence(&mut self, id: StableNodeId) {
        if !self.nodes.contains(id) {
            return;
        }
        let next = if self.presence_live(id) {
            self.presence_flags_of(id)
        } else {
            PresenceFlags::NONE
        };
        self.apply_presence_flags(Some(id), next);
    }

    fn sync_subtree_presence(&mut self, root: StableNodeId) {
        for id in self.subtree_ids(root) {
            self.sync_node_presence(id);
        }
    }

    pub fn uses_viewport_basis(&self) -> bool {
        self.viewport_basis_nodes != 0
    }

    pub fn viewport_basis_ids(&self) -> impl Iterator<Item = StableNodeId> + '_ {
        self.viewport_basis
            .values()
            .flat_map(|ids| ids.iter().copied())
    }

    /// Mounted viewport-dependent nodes in one document, without scanning other documents.
    /// Declares that `id` accepts drops, replacing any previous declaration.
    pub(crate) fn set_drop_target(&mut self, id: StableNodeId, accepts: nana_ui_core::DropAccepts) {
        self.drop_targets.insert(id, accepts);
    }

    /// Stops `id` accepting drops. Returns whether it was a target.
    pub(crate) fn clear_drop_target(&mut self, id: StableNodeId) -> bool {
        self.drop_targets.remove(&id).is_some()
    }

    /// Every node currently registered as a drop target.
    pub(crate) fn drop_target_ids(&self) -> impl Iterator<Item = StableNodeId> + '_ {
        self.drop_targets.keys().copied()
    }

    pub(crate) fn drop_target(&self, id: StableNodeId) -> Option<&nana_ui_core::DropAccepts> {
        self.drop_targets.get(&id)
    }

    pub(crate) fn drop_hover(&self) -> Option<(StableNodeId, nana_ui_core::DropEffect)> {
        self.drop_hover
    }

    pub(crate) fn set_drop_hover(
        &mut self,
        hover: Option<(StableNodeId, nana_ui_core::DropEffect)>,
    ) -> bool {
        if self.drop_hover == hover {
            return false;
        }
        if let Some((id, _)) = self.drop_hover {
            self.mark(id, DirtyMask::RENDER);
        }
        if let Some((id, _)) = hover {
            self.mark(id, DirtyMask::RENDER);
        }
        self.drop_hover = hover;
        true
    }

    /// Records the viewport a document was laid out against.
    pub(crate) fn set_document_viewport(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
    ) {
        self.document_viewports.insert(document, viewport);
    }

    /// Viewport `document` was last laid out against.
    pub(crate) fn document_viewport(&self, document: DocumentId) -> Option<crate::LayoutViewport> {
        self.document_viewports.get(&document).copied()
    }

    /// Forgets a document's viewport once it holds no roots.
    pub(crate) fn clear_document_viewport(&mut self, document: DocumentId) {
        self.document_viewports.remove(&document);
    }

    /// Viewport of the document owning `id`, if it has been laid out.
    pub(crate) fn document_viewport_of(&self, id: StableNodeId) -> Option<crate::LayoutViewport> {
        let document = self.node(id)?.document;
        self.document_viewports.get(&document).copied()
    }

    pub fn viewport_basis_ids_for(
        &self,
        document: DocumentId,
    ) -> impl Iterator<Item = StableNodeId> + '_ {
        self.viewport_basis
            .get(&document)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
    }

    pub(crate) fn document_of(&self, id: StableNodeId) -> Option<DocumentId> {
        self.nodes.get(id).map(|node| node.document)
    }

    fn note_z_index_presence(&mut self, was_present: bool, now_present: bool) {
        bump_presence(&mut self.z_index_nodes, was_present, now_present);
    }

    fn note_presence_counts(
        &mut self,
        was_confirm: bool,
        now_confirm: bool,
        was_clip: bool,
        now_clip: bool,
    ) {
        bump_presence(&mut self.confirm_modals, was_confirm, now_confirm);
        bump_presence(&mut self.clip_visuals, was_clip, now_clip);
    }

    fn forget_visual_presence(&mut self, id: StableNodeId) {
        self.apply_presence_flags(Some(id), PresenceFlags::NONE);
    }

    pub fn is_descendant_or_self(&self, id: StableNodeId, ancestor: StableNodeId) -> bool {
        let mut current = Some(id);
        while let Some(candidate) = current {
            if candidate == ancestor {
                return true;
            }
            current = self.parent_id(candidate);
        }
        false
    }

    fn confirm_action_effect(&self, id: StableNodeId) -> Option<(bool, bool, bool)> {
        if self.confirm_modals == 0 {
            return None;
        }
        let mut current = self.parent_id(id);
        while let Some(ancestor) = current {
            if let Some(StandardVisual::ModalFrame {
                kind: crate::ModalSurfaceKind::Confirm(_),
                busy,
                danger,
                slots,
                ..
            }) = self.nodes.visual(ancestor)
            {
                let close = slots
                    .close_action
                    .is_some_and(|root| self.is_descendant_or_self(id, root));
                let action = slots
                    .actions
                    .iter()
                    .copied()
                    .find(|root| self.is_descendant_or_self(id, *root));
                if close || action.is_some() {
                    return Some((*busy, *danger, action == slots.actions.last().copied()));
                }
            }
            current = self.parent_id(ancestor);
        }
        None
    }

    fn validate_pointer_target(
        &self,
        document: DocumentId,
        target: StableNodeId,
    ) -> Result<(), UiWorldError> {
        let node = self
            .nodes
            .get(target)
            .ok_or(UiWorldError::MissingNode(target))?;
        if node.document != document {
            return Err(UiWorldError::PointerDocument { document, target });
        }
        if !self.is_mounted(target) {
            return Err(UiWorldError::NotPointerInteractive(target));
        }
        if !node.interaction.pointer_events || !self.used_pointer_events(target).hittable() {
            return Err(UiWorldError::NotPointerInteractive(target));
        }
        if !node.resolved.0.pointer_events.hittable() {
            return Err(UiWorldError::NotPointerInteractive(target));
        }
        Ok(())
    }

    fn used_pointer_events(&self, id: StableNodeId) -> PointerEventsSpec {
        let mut current = Some(id);
        while let Some(node) = current {
            if let Some(specified) = self.record(node).style.layout.pointer_events {
                return specified;
            }
            current = self.parent_id(node);
        }
        PointerEventsSpec::Auto
    }

    fn parent_used_pointer_events(&self, id: StableNodeId) -> PointerEventsSpec {
        self.parent_id(id)
            .map(|parent| self.used_pointer_events(parent))
            .unwrap_or(PointerEventsSpec::Auto)
    }

    fn clear_hover_for_pointer_events_none(&mut self, root: StableNodeId) {
        let mut stack = vec![root];
        let mut cleared = Vec::new();
        while let Some(id) = stack.pop() {
            if !self.used_pointer_events(id).hittable() {
                let had_hover = self
                    .input
                    .pointer_hover
                    .values()
                    .any(|target| target == &id);
                let had_press = self
                    .input
                    .pointer_press
                    .values()
                    .any(|target| target == &id);
                self.input.pointer_hover.retain(|_, target| target != &id);
                self.input.pointer_press.retain(|_, target| target != &id);
                if had_hover || had_press {
                    cleared.push(id);
                }
            }
            stack.extend(self.record(id).hierarchy.children.iter().copied());
        }
        if !cleared.is_empty() {
            self.generation = self.generation.wrapping_add(1);
            for id in cleared {
                self.mark_interaction_style(id);
            }
        }
    }

    /// What a node that just lost focus needs: its composition cancelled and
    /// its focused state and style redrawn.
    pub(super) fn release_focus(&mut self, old: StableNodeId) {
        self.remove_ime(old);
        self.mark(old, DirtyMask::STATE);
        if !self.record(old).style.interaction.focused.is_empty() {
            self.mark(old, DirtyMask::STYLE | DirtyMask::RENDER);
        }
        self.mark_focus_changed(old);
    }

    /// Focus left the editor, or it is going away: an unfinished composition
    /// is cancelled and an attached IME detached. The editor presentation was
    /// built with the preedit spliced in, so it is re-derived rather than left
    /// drawing text that is no longer there.
    fn remove_ime(&mut self, id: StableNodeId) {
        let Some(editor) = self.nodes.editor_mut(id) else {
            return;
        };
        if editor.ime().is_none() {
            return;
        }
        editor.session.blur();
        editor.empty_preedit = None;
        let _ = editor.session.take_work();
        self.pending_edit_work.composition_updates += 1;
        self.nodes
            .invalidate_text(id, crate::text_node::TextDirty::EDIT_STATE);
        self.mark(id, DirtyMask::TEXT | DirtyMask::RENDER);
    }

    fn clear_overlay_references(&mut self, removed: StableNodeId) {
        self.clear_overlay_references_for(&[removed]);
    }

    /// Drop references through the reverse index; unrelated hosts are untouched.
    fn clear_overlay_references_for(&mut self, removed: &[StableNodeId]) {
        if self.overlay_host_nodes.is_empty() || removed.is_empty() {
            return;
        }
        let removed = removed.iter().copied().collect::<HashSet<_>>();
        let hosts = removed
            .iter()
            .filter_map(|id| self.overlay_dependents.get(id))
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        // The overlays closing here: usually the removed root alone.
        let closing = hosts
            .iter()
            .filter(|host| !removed.contains(host))
            .filter_map(|host| self.nodes.overlay_host(*host)?.active)
            .filter(|active| removed.contains(active))
            .collect::<Vec<_>>();
        // Overlays opened from inside it, hosted elsewhere (a dropdown's
        // listbox hosted outside the popover it opened from), and those
        // opened from inside them in turn: focus in any of them belongs to
        // the closing overlay too. The openers are still in the tree here: a
        // despawn takes the root first, and parking keeps the nodes.
        let mut opened_from_closing = Vec::new();
        if !closing.is_empty() {
            let open = self
                .overlay_host_nodes
                .iter()
                .filter(|host| !removed.contains(host))
                .filter_map(|host| self.nodes.overlay_host(*host))
                .filter_map(|state| {
                    let active = state.active.filter(|active| !removed.contains(active))?;
                    Some((active, state.restore_focus?))
                })
                .collect::<Vec<_>>();
            let mut frontier = closing;
            while let Some(parent) = frontier.pop() {
                for &(active, opener) in &open {
                    if opened_from_closing.contains(&active) {
                        continue;
                    }
                    // Inside the overlay it was opened from; one elsewhere in
                    // the removal (a parked subtree) is unrelated.
                    if self.contains(opener) && self.is_descendant_or_self(opener, parent) {
                        opened_from_closing.push(active);
                        frontier.push(active);
                    }
                }
            }
        }
        // A host inside `removed` goes with it and is skipped.
        let updates = hosts
            .into_iter()
            .filter_map(|host| {
                (!removed.contains(&host))
                    .then(|| self.nodes.overlay_host(host).copied())
                    .flatten()
                    .and_then(|mut state| {
                        let previous = state;
                        let closed = state.active.filter(|active| removed.contains(active));
                        let restore_focus = closed.zip(state.restore_focus);
                        if closed.is_some() {
                            state.active = None;
                            state.restore_focus = None;
                        }
                        if state
                            .restore_focus
                            .is_some_and(|target| removed.contains(&target))
                        {
                            state.restore_focus = None;
                        }
                        (state != previous).then_some((host, state, restore_focus))
                    })
            })
            .collect::<Vec<_>>();
        for (host, state, restore_focus) in updates {
            self.write_overlay_host(host, Some(state));
            self.mark(host, DirtyMask::ACCESSIBILITY);
            let document = self.record(host).document;
            let Some((overlay, restore_focus)) = restore_focus else {
                continue;
            };
            // Focus goes back unless the user moved it to another node, which
            // keeps it, with whatever composition it has going. It is still
            // on the overlay or inside it when a despawn removes the root
            // first, or in an overlay opened from it. An active modal above
            // still keeps it where it is (checked below). A document with no focus gets the opener back too: the
            // removal may have dropped it, or the framework may have (a
            // disabled or hidden editor), and a keyboard or screen-reader user
            // left with nothing focused is worse off than one whose cleared
            // focus comes back to the control that opened the overlay.
            let focused = self.input.focused.get(&document).copied();
            let focus_left = focused.is_none_or(|focused| {
                removed.contains(&focused)
                    || !self.contains(focused)
                    || self.is_descendant_or_self(focused, overlay)
                    || opened_from_closing
                        .iter()
                        .any(|nested| self.is_descendant_or_self(focused, *nested))
            });
            if focus_left
                && self.contains(restore_focus)
                && self.is_mounted(restore_focus)
                && self.record(restore_focus).document == document
                && self.record(restore_focus).interaction.focusable
                && self.record(restore_focus).resolved.0.visible
                && self.active_modal_allows_focus_now(document, restore_focus)
            {
                // The node focus leaves, if it is still here, loses it as a
                // RequestFocus would take it: composition ended, style redrawn.
                if let Some(old) =
                    focused.filter(|old| *old != restore_focus && self.contains(*old))
                {
                    self.release_focus(old);
                }
                self.input.focused.insert(document, restore_focus);
                self.mark_focus_changed(restore_focus);
            }
        }
    }

    fn active_modal_allows_focus_now(&self, document: DocumentId, target: StableNodeId) -> bool {
        let order = self.document_order(document);
        let top = order
            .iter()
            .enumerate()
            .filter_map(|(host_order, host)| {
                let state = self.overlay_host(*host)?;
                let active = state.active?;
                let node = self.node(active)?;
                if node.parent != Some(*host)
                    || self.surface_closed(active)
                    || !self.is_overlay_reachable(active)
                    || !self
                        .accessibility(active)
                        .is_some_and(|accessibility| accessibility.modal)
                {
                    return None;
                }
                let z = self
                    .node_style(active)
                    .and_then(|style| style.layout.z_index)
                    .unwrap_or_default();
                let active_order = order
                    .iter()
                    .position(|candidate| *candidate == active)
                    .unwrap_or(host_order);
                Some((z, active_order, active))
            })
            .max_by_key(|(z, active_order, _)| (*z, *active_order));
        top.is_none_or(|(_, _, active)| self.has_ancestor_now(target, active))
    }

    fn has_ancestor_now(&self, mut id: StableNodeId, candidate: StableNodeId) -> bool {
        let mut visited = HashSet::new();
        loop {
            if id == candidate {
                return true;
            }
            if !visited.insert(id) {
                return false;
            }
            let Some(parent) = self.parent_id(id) else {
                return false;
            };
            id = parent;
        }
    }

    fn overlay_branch_active(&self, id: StableNodeId) -> bool {
        if self.overlay_host_nodes.is_empty() {
            return true;
        }
        let Some(parent) = self.record(id).hierarchy.parent else {
            return true;
        };
        self.overlay_host(parent)
            .is_none_or(|state| state.active == Some(id))
    }

    /// A closed menu keeps its items in the tree but out of the frame. They
    /// would otherwise stretch the in-flow trigger they hang under.
    fn menu_branch_open(&self, id: StableNodeId) -> bool {
        let Some(parent) = self.record(id).hierarchy.parent else {
            return true;
        };
        if !self.nodes.contains(parent) {
            return true;
        };
        !closed_menu_hides(self.nodes.visual(parent), id)
    }

    /// Whether a live retained root currently belongs to this document.
    /// Host service requests use this as the document lifetime authority.
    pub fn has_document(&self, document: DocumentId) -> bool {
        self.live_document_roots
            .get(&document)
            .is_some_and(|roots| !roots.is_empty())
    }

    pub(crate) fn has_document_roots(&self, document: DocumentId) -> bool {
        self.has_document(document)
    }

    pub(crate) fn document_roots(&self, document: DocumentId) -> Vec<StableNodeId> {
        let mut roots = self
            .live_document_roots
            .get(&document)
            .map(|set| set.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        roots.sort_unstable();
        roots
    }

    fn refresh_root_membership(&mut self, id: StableNodeId) {
        if !self.nodes.contains(id) {
            return;
        }
        let node = self.record(id);
        let document = node.document;
        let parent = node.hierarchy.parent;
        let live_root = parent.is_none() && self.presence_live(id);
        if live_root {
            self.live_document_roots
                .entry(document)
                .or_default()
                .insert(id);
            return;
        }
        self.remove_document_root(document, id);
    }

    fn remove_document_root(&mut self, document: DocumentId, id: StableNodeId) {
        let empty = self
            .live_document_roots
            .get_mut(&document)
            .is_some_and(|roots| {
                roots.remove(&id);
                roots.is_empty()
            });
        if empty {
            self.live_document_roots.remove(&document);
        }
    }

    pub fn document_order(&self, document: DocumentId) -> Vec<StableNodeId> {
        let roots = self.document_roots(document);
        let mut order = Vec::new();
        let mut stack = roots.into_iter().rev().collect::<Vec<_>>();
        while let Some(id) = stack.pop() {
            order.push(id);
            stack.extend(self.record(id).hierarchy.children.iter().rev().copied());
        }
        self.record_id_list_alloc(order.len());
        order
    }

    fn hierarchy_mut(&mut self, id: StableNodeId) -> &mut Hierarchy {
        &mut self.record_mut(id).hierarchy
    }

    fn mark(&mut self, id: StableNodeId, bits: u16) -> bool {
        if bits & DirtyMask::INPUT != 0 {
            self.non_scroll_hit_dirty.insert(id);
        }
        self.mark_scroll_compatible(id, bits)
    }

    /// Record dirtiness without claiming that hit membership or geometry changed.
    fn mark_scroll_compatible(&mut self, id: StableNodeId, bits: u16) -> bool {
        let changed = self.record_mut(id).dirty.insert(bits);
        if changed {
            self.dirty_entities.insert(id);
            self.pending_work_revision = self.pending_work_revision.saturating_add(1);
        }
        changed
    }

    fn mark_subtree(&mut self, root: StableNodeId, bits: u16) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self.node(id).expect("hierarchy node must exist").children;
            stack.extend(children.iter().rev().copied());
            let _ = self.mark(id, bits);
        }
    }

    fn subtree_ids(&self, root: StableNodeId) -> Vec<StableNodeId> {
        let mut ids = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self.node(id).expect("hierarchy node must exist").children;
            stack.extend(children.iter().rev().copied());
            ids.push(id);
        }
        ids
    }

    fn set_subtree_mount_state(&mut self, root: StableNodeId, state: MountState) {
        let ids = self.subtree_ids(root);
        for id in &ids {
            self.record_mut(*id).mount = state;
        }
        if state == MountState::Mounted {
            self.mark_subtree(root, DirtyMask::ALL);
        }
    }

    fn unlink_from_parent(&mut self, id: StableNodeId) -> bool {
        let Some(parent) = self.node(id).expect("validated node must exist").parent else {
            return false;
        };
        let hierarchy = self.hierarchy_mut(parent);
        Arc::make_mut(&mut hierarchy.children).retain(|child| *child != id);
        intern_empty_children(&mut hierarchy.children);
        let _hierarchy = hierarchy;
        self.hierarchy_mut(id).parent = None;
        self.mark_ancestors(
            parent,
            DirtyMask::LAYOUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
        );
        self.note_structural_change(parent);
        true
    }

    fn leave_live_document(&mut self, root: StableNodeId) {
        let subtree = self.subtree_ids(root);
        self.retire_subtree_from_document(&subtree);
        self.detached.insert(root);
        if self.mount_state(root) == Some(MountState::Parked) {
            self.detached_mounted.remove(&root);
        } else {
            self.detached_mounted.insert(root);
        }
        self.sync_subtree_presence(root);
    }

    fn retire_subtree_from_document(&mut self, subtree: &[StableNodeId]) {
        let parked = subtree.iter().copied().collect::<HashSet<_>>();
        for &id in subtree {
            let document = self.record(id).document;
            if self.input.focused.get(&document) == Some(&id) {
                self.input.focused.remove(&document);
            }
            self.remove_ime(id);
            if let Some(index) = self.hit_test_index.get_mut(&document) {
                retain_hit_tree(index, id);
            }
            self.pending_render_removals.push(id);
            self.pending_accessibility_removals.push(id);
        }

        let released = self
            .input
            .pointer_captures
            .iter()
            .filter_map(|(&(document, pointer_id), &target)| {
                parked
                    .contains(&target)
                    .then_some((document, pointer_id, target))
            })
            .collect::<Vec<_>>();
        for (document, pointer_id, target) in released {
            self.input.pointer_captures.remove(&(document, pointer_id));
            self.input
                .pending_pointer_capture_changes
                .push(PointerCaptureChange {
                    pointer_id,
                    target,
                    captured: false,
                });
        }
        self.input
            .pointer_hover
            .retain(|_, target| !parked.contains(target));
        self.input
            .pointer_press
            .retain(|_, target| !parked.contains(target));
        self.drop_non_overlay_animations_for_parked(&parked);

        for &id in subtree {
            self.surface_motion.remove(&id);
            self.closing_surfaces.remove(&id);
            self.hover_transitions.remove(&id);
            if self.overlay_host(id).is_some() {
                self.write_overlay_host(id, Some(OverlayHostState::default()));
            }
        }
        self.clear_overlay_references_for(subtree);
        // Sorted and deduplicated once, when system work takes them: doing
        // it here, once per removed subtree, made dropping n rows O(n² log n).
    }

    pub(crate) fn layout_isolated(&self, id: StableNodeId) -> bool {
        self.node_style(id).is_some_and(|node| {
            let style=&node.layout;
            style.layout_isolation
                && matches!(style.width, Some(nana_ui_core::LengthSpec::Px(value)) if value.is_finite() && value>=0.0)
                && matches!(style.height, Some(nana_ui_core::LengthSpec::Px(value)) if value.is_finite() && value>=0.0)
                && style.position==nana_ui_core::PositionSpec::Static
                && style.float==nana_ui_core::FloatSpec::None
                && !style.display.is_some_and(nana_ui_core::DisplaySpec::is_grid_container)
                && style.min_width.is_none() && style.max_width.is_none()
                && style.min_height.is_none() && style.max_height.is_none()
        })
    }

    fn mark_ancestors(&mut self, start: StableNodeId, mut bits: u16) {
        let mut current = Some(start);
        while let Some(id) = current {
            current = self
                .identity_and_parent(id)
                .expect("hierarchy node must exist")
                .1;
            if !self.mark(id, bits) {
                break;
            }
            if self.layout_isolated(id) {
                bits &= !(DirtyMask::LAYOUT | DirtyMask::RENDER);
                if bits == 0 {
                    break;
                }
            }
        }
    }

    fn propagate_layout_from_node(&mut self, id: StableNodeId) {
        self.mark(id, DirtyMask::LAYOUT | DirtyMask::RENDER);
        if let Some(parent) = self.parent_id(id) {
            self.mark_ancestors(parent, DirtyMask::LAYOUT | DirtyMask::RENDER);
        }
    }
}

/// The writing mode and direction a node lays out in: its own
/// `writing-mode` / `direction`, or else its parent's — inherited, as CSS
/// inherits them.
///
/// The layout style carries only what a node declares; the record carries
/// what it inherits ([`NodeRecord::inherited_writing`], written when
/// styles resolve). Reading the declared value first keeps a node that
/// sets its own writing mode right even before its style has been
/// resolved.
fn record_writing(record: &NodeRecord) -> nana_ui_core::WritingContext {
    let declared = &record.resolved_layout;
    let inherited = record.inherited_writing;
    nana_ui_core::WritingContext::used(
        declared.writing_mode.unwrap_or(inherited.mode),
        declared.dir.unwrap_or(inherited.direction),
        declared
            .text_orientation
            .unwrap_or(record.inherited_orientation),
    )
}

/// [`UiWorld::containing_writing`] for a record already in hand: what it
/// inherits is its parent's writing context. A root is its own containing
/// block's frame.
fn record_containing_writing(record: &NodeRecord) -> nana_ui_core::WritingContext {
    if record.hierarchy.parent.is_some() {
        let parent = record.inherited_writing;
        nana_ui_core::WritingContext::used(
            parent.mode,
            parent.direction,
            record.inherited_orientation,
        )
    } else {
        record_writing(record)
    }
}

const IDENTITY_AFFINE: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PresenceFlags {
    confirm: bool,
    clip: bool,
    z_index: bool,
    /// An open triggered menu, whose surface hangs above the page.
    triggered: bool,
    /// Style resolves against the viewport (`position: fixed`, `vw` / `vh`).
    viewport: bool,
}

impl PresenceFlags {
    const NONE: Self = Self {
        confirm: false,
        clip: false,
        z_index: false,
        triggered: false,
        viewport: false,
    };
}

fn bump_presence(count: &mut usize, was_present: bool, now_present: bool) {
    if was_present == now_present {
        return;
    }
    if now_present {
        *count = count.saturating_add(1);
    } else {
        *count = count.saturating_sub(1);
    }
}

fn is_confirm_modal(visual: Option<&StandardVisual>) -> bool {
    matches!(
        visual,
        Some(StandardVisual::ModalFrame {
            kind: crate::ModalSurfaceKind::Confirm(_),
            ..
        })
    )
}

fn is_clip_visual(visual: Option<&StandardVisual>) -> bool {
    matches!(
        visual,
        Some(StandardVisual::EmptyState { .. } | StandardVisual::ModalFrame { .. })
    )
}

fn apply_triggered_overlay(
    layout: &mut nana_ui_core::LayoutStyle,
    overlay: crate::TriggeredMenuOverlay,
) {
    layout.position = PositionSpec::Fixed;
    layout.width = Some(LengthSpec::Px(
        (overlay.width - overlay.padding * 2.0).max(0.0),
    ));
    layout.z_index = Some(crate::popover::MENU_OVERLAY_Z_INDEX);
}

fn is_triggered_menu_overlay(visual: Option<&StandardVisual>) -> bool {
    matches!(
        visual,
        Some(StandardVisual::MenuSurface {
            open: true,
            overlay: Some(_),
            ..
        })
    )
}

fn text_intrinsic_changed(previous: TextMetrics, next: TextMetrics) -> bool {
    previous.width != next.width || previous.height != next.height
}

fn intersect_layout_boxes(left: LayoutBox, right: LayoutBox) -> Option<LayoutBox> {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let right_edge = (left.x + left.width).min(right.x + right.width);
    let bottom_edge = (left.y + left.height).min(right.y + right.height);
    (right_edge > x && bottom_edge > y).then_some(LayoutBox {
        x,
        y,
        width: right_edge - x,
        height: bottom_edge - y,
    })
}

/// Drain/layout shaper adapter. `UiWorld::shape_text` and
/// `shape_text_for_layout` construct this around the host `TextShaper`
/// (`MeasureTextShaper`, `NanaTextShaper`, …). It is not a test-only wrapper:
/// empty-state / modal / presentation helpers also call `self.shape` on it.
fn initial_interaction(kind: &NodeKind) -> InteractionState {
    match kind {
        NodeKind::Text | NodeKind::Comment => InteractionState {
            pointer_events: false,
            focusable: false,
        },
        NodeKind::Document | NodeKind::Element { .. } => InteractionState::default(),
    }
}

fn validate_text_metrics(id: StableNodeId, metrics: TextMetrics) -> Result<(), UiWorldError> {
    if !metrics.width.is_finite()
        || !metrics.height.is_finite()
        || metrics.width < 0.0
        || metrics.height < 0.0
    {
        return Err(UiWorldError::InvalidText(id));
    }
    Ok(())
}

fn style_excluding_transform_and_cursor_eq(left: &NodeStyle, right: &NodeStyle) -> bool {
    left.foreground == right.foreground
        && left.background == right.background
        && left.border == right.border
        && left.interaction == right.interaction
        && left.text_horizontal_alignment == right.text_horizontal_alignment
        && left.text_vertical_alignment == right.text_vertical_alignment
        && left.painter == right.painter
        && (std::sync::Arc::ptr_eq(&left.layout, &right.layout)
            || layout_excluding_transform_and_cursor_eq(
                left.layout.as_ref(),
                right.layout.as_ref(),
            ))
}

fn layout_excluding_transform_and_cursor_eq(
    left: &nana_ui_core::LayoutStyle,
    right: &nana_ui_core::LayoutStyle,
) -> bool {
    let excluded_equal = left.transform == right.transform
        && left.transform_3d == right.transform_3d
        && left.unsupported_transform == right.unsupported_transform
        && left.transform_origin == right.transform_origin
        && left.transform_box == right.transform_box
        && left.css_perspective == right.css_perspective
        && left.preserve_3d == right.preserve_3d
        && left.cursor == right.cursor
        && left.user_select == right.user_select;
    if excluded_equal {
        return left == right;
    }
    // One copy, carrying the other side's excluded fields, instead of two
    // stripped copies.
    let mut left = left.clone();
    left.transform = right.transform;
    left.transform_3d = right.transform_3d;
    left.unsupported_transform = right.unsupported_transform.clone();
    left.transform_origin = right.transform_origin;
    left.transform_box = right.transform_box;
    left.css_perspective = right.css_perspective;
    left.preserve_3d = right.preserve_3d;
    left.cursor = right.cursor;
    left.user_select = right.user_select;
    left == *right
}

fn layout_semantics_changed(
    previous: &nana_ui_core::LayoutStyle,
    next: &nana_ui_core::LayoutStyle,
) -> bool {
    previous.direction != next.direction
        || previous.dir != next.dir
        || previous.flex_reverse != next.flex_reverse
        || previous.order != next.order
        || previous.flex_wrap != next.flex_wrap
        || previous.display != next.display
        || previous.box_sizing != next.box_sizing
        || previous.position != next.position
        || previous.gap != next.gap
        || previous.row_gap != next.row_gap
        || previous.column_gap != next.column_gap
        || previous.padding != next.padding
        || previous.padding_top != next.padding_top
        || previous.padding_right != next.padding_right
        || previous.padding_bottom != next.padding_bottom
        || previous.padding_left != next.padding_left
        || previous.margin != next.margin
        || previous.margin_top != next.margin_top
        || previous.margin_right != next.margin_right
        || previous.margin_bottom != next.margin_bottom
        || previous.margin_left != next.margin_left
        || previous.offset_top != next.offset_top
        || previous.offset_right != next.offset_right
        || previous.offset_bottom != next.offset_bottom
        || previous.offset_left != next.offset_left
        || previous.width != next.width
        || previous.height != next.height
        || previous.min_width != next.min_width
        || previous.max_width != next.max_width
        || previous.min_height != next.min_height
        || previous.max_height != next.max_height
        || previous.allow_shrink != next.allow_shrink
        || previous.align_items != next.align_items
        || previous.align_self != next.align_self
        || previous.align_content != next.align_content
        || previous.justify_content != next.justify_content
        || previous.justify_items != next.justify_items
        || previous.justify_self != next.justify_self
        || previous.flex_grow != next.flex_grow
        || previous.flex_shrink != next.flex_shrink
        || previous.flex_basis != next.flex_basis
        || previous.overflow_x != next.overflow_x
        || previous.overflow_y != next.overflow_y
        || previous.text_overflow_ellipsis != next.text_overflow_ellipsis
        || previous.line_clamp != next.line_clamp
        || previous.white_space_nowrap != next.white_space_nowrap
        || previous.white_space != next.white_space
        || previous.word_break != next.word_break
        || previous.overflow_wrap != next.overflow_wrap
        || previous.aspect_ratio != next.aspect_ratio
        || previous.font_italic != next.font_italic
        || previous.text_align != next.text_align
        || previous.word_break != next.word_break
        || previous.line_break != next.line_break
        || previous.float != next.float
        || previous.clear != next.clear
        || previous.writing_mode != next.writing_mode
        || previous.grid_template_areas != next.grid_template_areas
        || previous.grid_column_line_names != next.grid_column_line_names
        || previous.grid_row_line_names != next.grid_row_line_names
        || previous.grid_columns != next.grid_columns
        || previous.grid_rows != next.grid_rows
        || previous.grid_columns_unsupported != next.grid_columns_unsupported
        || previous.grid_rows_unsupported != next.grid_rows_unsupported
        || previous.grid_auto_columns != next.grid_auto_columns
        || previous.grid_auto_rows != next.grid_auto_rows
        || previous.grid_auto_flow != next.grid_auto_flow
        || previous.grid_columns_repeat != next.grid_columns_repeat
        || previous.grid_rows_repeat != next.grid_rows_repeat
        || previous.grid_placement != next.grid_placement
        || previous.border_width != next.border_width
        || previous.border_top_width != next.border_top_width
        || previous.border_right_width != next.border_right_width
        || previous.border_bottom_width != next.border_bottom_width
        || previous.border_left_width != next.border_left_width
        || previous.border_style != next.border_style
        || previous.border_top_style != next.border_top_style
        || previous.border_right_style != next.border_right_style
        || previous.border_bottom_style != next.border_bottom_style
        || previous.border_left_style != next.border_left_style
}

#[cfg(test)]
mod tests;
