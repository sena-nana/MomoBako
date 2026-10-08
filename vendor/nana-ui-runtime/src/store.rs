//! Dense node table plus sparse side maps. Replaces `bevy_ecs::World` inside
//! [`crate::UiWorld`]. Public identity stays [`crate::StableNodeId`].

use hashbrown::HashMap;
use std::sync::{Arc, LazyLock};

use crate::ComputedStyle;
use crate::component_registry::ComponentTypeId;
use crate::components::{
    AccessibilityState, CustomRenderNode, EmptyStateTextPresentation, EventListeners, ImeView,
    InteractionState, LayoutBox, ModalTextPresentation, MountState, NodeStyle, OverlayHostState,
    ScrollMetrics, ScrollOffset, StandardVisual, TextCodeFold, TextContent, TextInputPresentation,
    TextInputView, TextMetrics, TextSnippetSession,
};
use crate::presentation::{HighlightRequest, TextPresentation};
use crate::schedule::DirtyMask;
use crate::world::{DocumentId, NodeKind, StableNodeId};

static EMPTY_CHILDREN: LazyLock<Arc<Vec<StableNodeId>>> = LazyLock::new(|| Arc::new(Vec::new()));
static INTERNED_KIND_DOCUMENT: LazyLock<Arc<NodeKind>> =
    LazyLock::new(|| Arc::new(NodeKind::Document));
static INTERNED_KIND_TEXT: LazyLock<Arc<NodeKind>> = LazyLock::new(|| Arc::new(NodeKind::Text));
static INTERNED_KIND_COMMENT: LazyLock<Arc<NodeKind>> =
    LazyLock::new(|| Arc::new(NodeKind::Comment));
static INTERNED_KIND_DIV: LazyLock<Arc<NodeKind>> =
    LazyLock::new(|| Arc::new(NodeKind::Element { tag: "div".into() }));
static INTERNED_DEFAULT_STYLE: LazyLock<Arc<ComputedStyle>> =
    LazyLock::new(|| Arc::new(ComputedStyle::default()));

fn intern_kind(kind: &NodeKind) -> Arc<NodeKind> {
    match kind {
        NodeKind::Document => Arc::clone(&INTERNED_KIND_DOCUMENT),
        NodeKind::Text => Arc::clone(&INTERNED_KIND_TEXT),
        NodeKind::Comment => Arc::clone(&INTERNED_KIND_COMMENT),
        NodeKind::Element { tag } if tag == "div" => Arc::clone(&INTERNED_KIND_DIV),
        _ => Arc::new(kind.clone()),
    }
}

pub(crate) fn intern_empty_children(children: &mut Arc<Vec<StableNodeId>>) {
    if children.is_empty() {
        *children = Arc::clone(&EMPTY_CHILDREN);
    }
}

#[derive(Clone)]
pub(crate) struct Hierarchy {
    pub parent: Option<StableNodeId>,
    pub children: Arc<Vec<StableNodeId>>,
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self {
            parent: None,
            children: Arc::clone(&EMPTY_CHILDREN),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedStyle(pub Arc<ComputedStyle>, pub u64);

impl ResolvedStyle {
    fn interned_default() -> Self {
        Self(Arc::clone(&INTERNED_DEFAULT_STYLE), 0)
    }
}

/// The process-wide default a root node inherits from. Sharing this avoids
/// building a fresh `ComputedStyle` per root during style resolution.
pub(crate) fn interned_default_style() -> Arc<ComputedStyle> {
    Arc::clone(&INTERNED_DEFAULT_STYLE)
}

pub(crate) struct NodeRecord {
    pub document: DocumentId,
    pub kind: Arc<NodeKind>,
    pub hierarchy: Hierarchy,
    pub mount: MountState,
    pub style: NodeStyle,
    /// [`Self::style`]'s layout with the node's design intent already applied
    /// (radius tier → `border_radius`), resolved against the installed
    /// `ThemeMetrics`.
    ///
    /// Resolved on write — when the style is set and when a theme install
    /// changes the metrics — rather than on every read. `LayoutStyle` is 4.8 KB,
    /// so resolving it inside extraction meant an `Arc::make_mut` copy per
    /// control per frame; a palette switch over 1k controls cost three times
    /// what it should. Resolving on write pays that once per change and hands
    /// every reader an `Arc` it can clone.
    ///
    /// [`Self::style`] stays exactly as authored, because that is what
    /// projection diffs against.
    pub resolved_layout: Arc<nana_ui_core::LayoutStyle>,
    pub resolved: ResolvedStyle,
    /// Whether [`Self::style`]'s layout reads the viewport, kept with it so
    /// presence bookkeeping after a visual or text write does not rescan
    /// the layout's lengths.
    pub layout_depends_on_viewport: bool,
    /// The writing mode, direction and text orientation this node inherits:
    /// its parent's computed values, as last resolved. Kept beside the
    /// record, not only inside the parent's `ComputedStyle`, so layout reads a
    /// node's writing context and its containing block's without a parent
    /// lookup or an `Arc` deref.
    pub inherited_writing: nana_ui_core::WritingContext,
    pub inherited_orientation: nana_ui_core::TextOrientationSpec,
    pub text: TextContent,
    pub text_metrics: TextMetrics,
    pub layout: LayoutBox,
    pub layout_padding: Option<nana_ui_core::PaddingSpec>,
    /// Page axes the last layout placed the children from the far end.
    pub layout_far_start: Option<[bool; 2]>,
    pub scroll_offset: ScrollOffset,
    pub interaction: InteractionState,
    pub accessibility: AccessibilityState,
    pub dirty: DirtyMask,
}

impl NodeRecord {
    pub fn new(document: DocumentId, kind: &NodeKind, interaction: InteractionState) -> Self {
        Self {
            document,
            kind: intern_kind(kind),
            hierarchy: Hierarchy::default(),
            mount: MountState::default(),
            style: NodeStyle::default(),
            resolved_layout: NodeStyle::default().layout,
            resolved: ResolvedStyle::interned_default(),
            layout_depends_on_viewport: false,
            inherited_writing: nana_ui_core::WritingContext::default(),
            inherited_orientation: nana_ui_core::TextOrientationSpec::Mixed,
            text: TextContent::default(),
            text_metrics: TextMetrics::default(),
            layout: LayoutBox::default(),
            layout_padding: None,
            layout_far_start: None,
            scroll_offset: ScrollOffset::default(),
            interaction,
            accessibility: AccessibilityState::default(),
            dirty: DirtyMask::all(),
        }
    }
}

macro_rules! sparse {
    ($field:ident, $ty:ty, $get:ident, $set:ident) => {
        pub fn $get(&self, id: StableNodeId) -> Option<&$ty> {
            self.$field.get(&id)
        }

        pub fn $set(&mut self, id: StableNodeId, value: Option<$ty>) {
            if let Some(value) = value {
                self.$field.insert(id, value);
            } else {
                self.$field.remove(&id);
            }
        }
    };
}

/// 代码折叠视图状态（仅喂过折叠区间的编辑器节点持有条目）。
#[derive(Clone, Debug, Default)]
pub(crate) struct TextFoldViewState {
    /// 宿主上一次喂入的折叠区间（用于重喂时的漂移匹配）。
    pub offered: Arc<[TextCodeFold]>,
    /// 当前折叠态的区间（`offered` 的子集，按 `start` 排序）。
    pub collapsed: Vec<TextCodeFold>,
}

/// 补全弹层会话状态（仅候选会话活跃的编辑器节点持有条目）。
///
/// 候选列表由宿主喂入（过滤是宿主的职责），组件只维护键盘选中与滚动：
/// 喂入相同列表不重置选中态（包括 Esc 关闭态）；喂入不同列表时视为新
/// 会话（选中归零、重新打开）；喂入空列表整个条目移除（零分配待机）。
#[derive(Clone, Debug)]
pub(crate) struct TextCompletionViewState {
    pub items: Arc<[crate::TextCompletion]>,
    pub selected: usize,
    /// 第一条可见候选的绝对下标。
    pub scroll: usize,
    /// Esc 关闭标记：会话数据保留（宿主重喂相同列表不复活弹层），
    /// 换新列表或宿主撤空重喂后清除。
    pub dismissed: bool,
}

/// hover 文档浮窗状态（仅宿主喂入 hover 的编辑器节点持有条目）。
#[derive(Clone, Debug)]
pub(crate) struct TextHoverViewState {
    pub doc: crate::TextHover,
    /// 正文滚过的逻辑行数。
    pub scroll: usize,
    /// 由诊断 span 指针命中写入。离开 span 时框架撤掉；宿主喂入的
    /// 符号 hover 把此位置为 false。
    pub diagnostic: bool,
}

/// One text editor, stored once: the session is the only copy of its
/// committed text, its selections and its IME composition (Issue #182).
#[derive(Debug)]
pub(crate) struct EditorRecord {
    pub session: nana_text::EditSession,
    /// An IME attached with nothing composed: the preedit a platform reports
    /// as empty between keystrokes, or when composition just ended and the
    /// commit is still on its way. Not a composition — the session has none,
    /// and caret motion and display do not treat it as one — but it is still
    /// the IME's turn: a submit waits for its commit or its end. Carries the
    /// IME's (empty) selection so the state reads back as it was written.
    pub empty_preedit: Option<Option<(usize, usize)>>,
    /// What the editor draws, built from the session and the overlays on it
    /// once per change of those, not once per consumer and pass.
    pub display: std::cell::RefCell<EditorDisplayMemo>,
}

/// Memoized display pieces of one editor, each with the inputs it was built
/// from. The keys are stamps and revisions, not dirty bits: a stale entry
/// cannot survive a change it did not see.
#[derive(Debug, Default)]
pub(crate) struct EditorDisplayMemo {
    /// The fold / inlay display view.
    pub view: Option<(DisplayViewKey, Option<crate::world::TextDisplayView>)>,
    /// The committed text with the preedit spliced in, stamped.
    pub composed: Option<((nana_text::TextStamp, u64), crate::TextValue)>,
}

/// What a fold / inlay view is a function of.
#[derive(Debug, Clone)]
pub(crate) struct DisplayViewKey {
    pub text: nana_text::TextStamp,
    pub collapsed: Vec<crate::TextCodeFold>,
    /// The inlay set, by identity: a new feed is a new `Arc`. Held, not
    /// just its address: a freed feed's address can come back for the next
    /// one, and a key that did not keep it alive would then match it.
    pub inlays: Option<Arc<[crate::TextInlay]>>,
    pub composing: bool,
}

impl PartialEq for DisplayViewKey {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.composing == other.composing
            && self.collapsed == other.collapsed
            && match (&self.inlays, &other.inlays) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl EditorRecord {
    pub fn new(session: nana_text::EditSession) -> Self {
        Self {
            session,
            empty_preedit: None,
            display: Default::default(),
        }
    }

    pub fn ime(&self) -> Option<ImeView<'_>> {
        if let Some(composition) = self.session.composition() {
            return Some(ImeView {
                text: &composition.text,
                selection: composition
                    .selection
                    .as_ref()
                    .map(|range| (range.start, range.end)),
            });
        }
        self.empty_preedit.map(|selection| ImeView {
            text: "",
            selection,
        })
    }
}

#[derive(Default)]
pub(crate) struct NodeStore {
    nodes: HashMap<StableNodeId, NodeRecord>,
    visuals: HashMap<StableNodeId, StandardVisual>,
    custom_render: HashMap<StableNodeId, CustomRenderNode>,
    event_listeners: HashMap<StableNodeId, EventListeners>,
    component_types: HashMap<StableNodeId, ComponentTypeId>,
    overlay_hosts: HashMap<StableNodeId, OverlayHostState>,
    scroll_metrics: HashMap<StableNodeId, ScrollMetrics>,
    /// Every text editor's session: its committed text, selections and IME
    /// composition, stored once (Issue #182).
    editors: HashMap<StableNodeId, EditorRecord>,
    highlights: HashMap<StableNodeId, HighlightRequest>,
    text_presentations: HashMap<StableNodeId, TextPresentation>,
    text_input_presentations: HashMap<StableNodeId, TextInputPresentation>,
    empty_state_text: HashMap<StableNodeId, EmptyStateTextPresentation>,
    modal_text: HashMap<StableNodeId, ModalTextPresentation>,
    /// 代码折叠视图状态（仅喂过折叠区间的编辑器节点持有条目）。
    text_fold_views: HashMap<StableNodeId, TextFoldViewState>,
    /// 行内提示（inlay）集（仅喂过非空 inlay 的编辑器节点持有条目；
    /// 世界校验后按 `(offset, label)` 排序去重）。
    text_inlays: HashMap<StableNodeId, Arc<[crate::TextInlay]>>,
    /// 活跃 snippet 会话（仅会话进行中的编辑器持有条目）。
    text_snippets: HashMap<StableNodeId, TextSnippetSession>,
    /// 补全弹层会话（仅候选会话活跃的编辑器持有条目）。
    text_completions: HashMap<StableNodeId, TextCompletionViewState>,
    /// hover 文档浮窗（仅宿主喂入 hover 的编辑器持有条目）。
    text_hovers: HashMap<StableNodeId, TextHoverViewState>,
    /// 签名帮助浮窗（仅宿主喂入签名的编辑器持有条目）。
    text_signatures: HashMap<StableNodeId, crate::TextSignatureHelp>,
    /// minimap 视口钉住（仅显式导航过的多行编辑器持有条目）。
    text_viewport_pins: HashMap<StableNodeId, ScrollOffset>,
    /// 拖拽移动选中文本的落点指示线（仅拖拽态编辑器持有条目；文本空间
    /// 矩形）。框架侧拖拽状态机写入，提取层翻译为节点空间图元。
    text_drop_indicators: HashMap<StableNodeId, LayoutBox>,
    /// The width a plain text that wrapped to its box would take on its
    /// own lines, unwrapped (its max-content width). Only text that wrapped
    /// holds an entry: its metrics are the lines it wrapped to, and layout
    /// needs this to give it more room once its box may grow.
    text_natural_widths: HashMap<StableNodeId, f32>,
    /// Revisions, resolution stamp and retained layout handle of every node's
    /// text (Issue #95), one entry per node. Kept apart from the dense records
    /// on purpose: a large relayout scope decides "no text work" for every
    /// candidate, and a table of small entries keeps that scan in cache where
    /// the full records would not.
    text_nodes: HashMap<StableNodeId, crate::text_node::TextNodeState>,
    /// Layouts retained by plain text nodes, addressed by
    /// [`crate::text_node::TextNodeState::layout`]. Released with the node.
    text_layouts: nana_text::TextLayoutStore,
}

/// minimap 视口钉住：显式视口导航（minimap 点击/拖动）写入的滚动偏移。
/// 几何层在「钉住值 == 请求滚动」期间跳过光标 reveal（视口停在用户导航
/// 到的位置）；光标移动后由 shape 趟清除，reveal 恢复权威。
pub(crate) type TextViewportPin = ScrollOffset;

impl NodeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn contains(&self, id: StableNodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn get(&self, id: StableNodeId) -> Option<&NodeRecord> {
        self.nodes.get(&id)
    }

    pub fn get_mut(&mut self, id: StableNodeId) -> Option<&mut NodeRecord> {
        self.nodes.get_mut(&id)
    }

    pub fn keys(&self) -> impl Iterator<Item = StableNodeId> + '_ {
        self.nodes.keys().copied()
    }

    /// 持有 hover 浮窗状态的节点（稀疏表直接迭代，命中测试路由用）。
    pub fn text_hover_ids(&self) -> impl Iterator<Item = StableNodeId> + '_ {
        self.text_hovers.keys().copied()
    }

    pub fn insert(&mut self, id: StableNodeId, record: NodeRecord) {
        self.text_nodes
            .insert(id, crate::text_node::TextNodeState::default());
        self.nodes.insert(id, record);
    }

    /// Drop the dense record and every sparse entry. The only despawn path.
    pub fn remove(&mut self, id: StableNodeId) -> Option<NodeRecord> {
        let record = self.nodes.remove(&id)?;
        self.visuals.remove(&id);
        self.custom_render.remove(&id);
        self.event_listeners.remove(&id);
        self.component_types.remove(&id);
        self.overlay_hosts.remove(&id);
        self.scroll_metrics.remove(&id);
        self.editors.remove(&id);
        self.highlights.remove(&id);
        self.text_presentations.remove(&id);
        self.text_input_presentations.remove(&id);
        self.empty_state_text.remove(&id);
        self.modal_text.remove(&id);
        self.text_fold_views.remove(&id);
        self.text_inlays.remove(&id);
        self.text_snippets.remove(&id);
        self.text_completions.remove(&id);
        self.text_hovers.remove(&id);
        self.text_signatures.remove(&id);
        self.text_viewport_pins.remove(&id);
        self.text_drop_indicators.remove(&id);
        self.text_natural_widths.remove(&id);
        if let Some(text) = self.text_nodes.remove(&id) {
            self.text_layouts.remove(text.layout);
        }
        Some(record)
    }

    pub fn visual(&self, id: StableNodeId) -> Option<&StandardVisual> {
        self.visuals.get(&id)
    }

    /// Sets a node's visual. A visual change that moves what plain text
    /// resolution reads — which path the text takes, or the inset a leading
    /// indicator takes from its box — is a constraint change for its text;
    /// any other visual change (a sampled progress value) is not.
    pub fn set_visual(&mut self, id: StableNodeId, value: Option<StandardVisual>) {
        let before = crate::world::text_visual_key(self.visuals.get(&id));
        let after = crate::world::text_visual_key(value.as_ref());
        if let Some(value) = value {
            self.visuals.insert(id, value);
        } else {
            self.visuals.remove(&id);
        }
        if before != after {
            self.invalidate_text(id, crate::text_node::TextDirty::CONSTRAINT);
        }
    }
    sparse!(
        custom_render,
        CustomRenderNode,
        custom_render,
        set_custom_render
    );
    sparse!(
        event_listeners,
        EventListeners,
        event_listeners,
        set_event_listeners
    );
    sparse!(
        component_types,
        ComponentTypeId,
        component_type,
        set_component_type
    );
    sparse!(
        overlay_hosts,
        OverlayHostState,
        overlay_host,
        set_overlay_host
    );
    sparse!(
        scroll_metrics,
        ScrollMetrics,
        scroll_metrics,
        set_scroll_metrics
    );
    pub fn text_input(&self, id: StableNodeId) -> Option<TextInputView<'_>> {
        self.editors
            .get(&id)
            .map(|editor| TextInputView::of(&editor.session))
    }

    pub fn ime(&self, id: StableNodeId) -> Option<ImeView<'_>> {
        self.editors.get(&id)?.ime()
    }

    pub(crate) fn editor(&self, id: StableNodeId) -> Option<&EditorRecord> {
        self.editors.get(&id)
    }

    pub(crate) fn editor_mut(&mut self, id: StableNodeId) -> Option<&mut EditorRecord> {
        self.editors.get_mut(&id)
    }

    pub(crate) fn set_editor(&mut self, id: StableNodeId, editor: Option<EditorRecord>) {
        match editor {
            Some(editor) => {
                self.editors.insert(id, editor);
            }
            None => {
                self.editors.remove(&id);
            }
        }
    }
    sparse!(highlights, HighlightRequest, highlight, set_highlight);
    sparse!(
        text_presentations,
        TextPresentation,
        text_presentation,
        set_text_presentation
    );
    sparse!(
        text_input_presentations,
        TextInputPresentation,
        text_input_presentation,
        set_text_input_presentation
    );
    sparse!(
        empty_state_text,
        EmptyStateTextPresentation,
        empty_state_text,
        set_empty_state_text
    );
    sparse!(
        modal_text,
        ModalTextPresentation,
        modal_text,
        set_modal_text
    );
    sparse!(
        text_fold_views,
        TextFoldViewState,
        text_fold_view,
        set_text_fold_view
    );
    sparse!(
        text_inlays,
        Arc<[crate::TextInlay]>,
        text_inlays,
        set_text_inlays
    );
    sparse!(
        text_snippets,
        TextSnippetSession,
        text_snippet_session,
        set_text_snippet_session
    );
    sparse!(
        text_completions,
        TextCompletionViewState,
        text_completion_view,
        set_text_completion_view
    );
    sparse!(
        text_hovers,
        TextHoverViewState,
        text_hover_view,
        set_text_hover_view
    );
    sparse!(
        text_signatures,
        crate::TextSignatureHelp,
        text_signature,
        set_text_signature
    );
    sparse!(
        text_viewport_pins,
        ScrollOffset,
        text_viewport_pin,
        set_text_viewport_pin
    );
    sparse!(
        text_drop_indicators,
        LayoutBox,
        text_drop_indicator,
        set_text_drop_indicator
    );
    sparse!(
        text_natural_widths,
        f32,
        text_natural_width,
        set_text_natural_width
    );

    pub(crate) fn text_layouts(&self) -> &nana_text::TextLayoutStore {
        &self.text_layouts
    }

    pub(crate) fn text_node(&self, id: StableNodeId) -> Option<&crate::text_node::TextNodeState> {
        self.text_nodes.get(&id)
    }

    /// Records a change to `id`'s text. See [`crate::text_node::TextDirty`].
    pub(crate) fn invalidate_text(&mut self, id: StableNodeId, dirty: crate::text_node::TextDirty) {
        if let Some(text) = self.text_nodes.get_mut(&id) {
            text.invalidate(dirty);
        }
    }

    /// Stamps `id`'s text as resolved at its current revisions.
    pub(crate) fn mark_text_resolved(
        &mut self,
        id: StableNodeId,
        backend: crate::text_node::TextBackendEpoch,
        constraints: Option<crate::TextShapeConstraints>,
    ) {
        let Some(record) = self.nodes.get(&id) else {
            return;
        };
        let text_node = !record.text.value.is_empty()
            || matches!(record.kind.as_ref(), crate::world::NodeKind::Text);
        let constraints =
            constraints.map(|constraints| (constraints, record.style.text_horizontal_alignment));
        if let Some(text) = self.text_nodes.get_mut(&id) {
            text.mark_resolved(backend, constraints, text_node);
        }
    }

    /// The `nana-text` source for `id`'s current text, built once per content
    /// revision, and whether this call built it.
    pub(crate) fn text_source(
        &mut self,
        id: StableNodeId,
    ) -> Option<(&nana_text::TextSource, bool)> {
        let text = &self.nodes.get(&id)?.text;
        Some(self.text_nodes.get_mut(&id)?.source_for(&text.value))
    }

    /// Retains `layout` for `id`, replacing what the node held. A node handed
    /// the very layout it already holds keeps its handle. Returns whether the
    /// handle was kept.
    pub(crate) fn retain_text_layout(
        &mut self,
        id: StableNodeId,
        layout: Arc<nana_text::TextLayout>,
    ) -> bool {
        let Some(text) = self.text_nodes.get_mut(&id) else {
            return false;
        };
        if self
            .text_layouts
            .get(text.layout)
            .is_some_and(|current| Arc::ptr_eq(current, &layout))
        {
            return true;
        }
        self.text_layouts.remove(text.layout);
        text.layout = self.text_layouts.insert(layout);
        false
    }

    /// Drops the layout `id` retains, if any. O(1) when it holds none.
    pub(crate) fn release_text_layout(&mut self, id: StableNodeId) {
        if let Some(text) = self.text_nodes.get_mut(&id)
            && !text.layout.is_null()
        {
            let held = std::mem::take(&mut text.layout);
            self.text_layouts.remove(held);
            // The source copy served only the released layout.
            text.release_source();
        }
    }

    /// Every node whose text a new font set can change: text that was measured
    /// (an empty Text node's line box included), plus the visuals that measure text of their own every pass
    /// and so are never stamped.
    pub(crate) fn text_bearing_nodes(&self) -> impl Iterator<Item = StableNodeId> + '_ {
        let resolved = self
            .text_nodes
            .iter()
            .filter(|(_, text)| text.stamp().is_some_and(|stamp| stamp.measured))
            .map(|(id, _)| *id);
        let measuring = self
            .visuals
            .iter()
            .filter(|(_, visual)| measures_text_each_pass(visual))
            .map(|(id, _)| *id);
        resolved.chain(measuring)
    }

    pub(crate) fn text_completion_view_mut(
        &mut self,
        id: StableNodeId,
    ) -> Option<&mut TextCompletionViewState> {
        self.text_completions.get_mut(&id)
    }

    pub(crate) fn text_hover_view_mut(
        &mut self,
        id: StableNodeId,
    ) -> Option<&mut TextHoverViewState> {
        self.text_hovers.get_mut(&id)
    }

    pub(crate) fn diagnostic_hover_ids(&self) -> Vec<StableNodeId> {
        self.text_hovers
            .iter()
            .filter(|(_, state)| state.diagnostic)
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Visuals that measure text of their own every pass — intrinsic text, or
/// chrome sized to the text painted into it — and so are never stamped.
fn measures_text_each_pass(visual: &StandardVisual) -> bool {
    match visual {
        StandardVisual::EmptyState { .. }
        | StandardVisual::ModalFrame { .. }
        | StandardVisual::TextInput { .. }
        | StandardVisual::KeyCaptureLayer { .. }
        | StandardVisual::KeymapLayer
        | StandardVisual::ListItem { .. }
        | StandardVisual::LabeledValue { .. }
        | StandardVisual::ActionMenuItem { .. }
        | StandardVisual::CommandPalette { .. } => true,
        #[cfg(feature = "calendar")]
        StandardVisual::CalendarHeatmap { .. } => true,
        #[cfg(feature = "charts")]
        StandardVisual::StackedTimeSeriesChart { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InteractionState;
    use crate::world::{DocumentId, NodeKind, StableNodeId};

    fn node(id: u64) -> StableNodeId {
        StableNodeId::new(id).unwrap()
    }

    fn document(id: u64) -> DocumentId {
        DocumentId::new(id).unwrap()
    }

    #[test]
    fn remove_clears_sparse_side_maps() {
        let mut store = NodeStore::new();
        let id = node(2);
        store.insert(
            id,
            NodeRecord::new(
                document(1),
                &NodeKind::Text,
                InteractionState {
                    pointer_events: false,
                    focusable: false,
                },
            ),
        );
        store.set_scroll_metrics(
            id,
            Some(ScrollMetrics {
                viewport_width: 10.0,
                viewport_height: 10.0,
                content_width: 20.0,
                content_height: 20.0,
                origin_x: 0.0,
                origin_y: 0.0,
            }),
        );
        store.set_component_type(id, Some(ComponentTypeId::new("nana.button").unwrap()));
        store.set_editor(
            id,
            Some(EditorRecord {
                empty_preedit: Some(None),
                ..EditorRecord::new(nana_text::EditSession::new("a"))
            }),
        );
        assert!(store.scroll_metrics(id).is_some());
        assert!(store.remove(id).is_some());
        assert!(!store.contains(id));
        assert!(store.scroll_metrics(id).is_none());
        assert!(store.component_type(id).is_none());
        assert!(store.ime(id).is_none());
    }

    #[test]
    fn sparse_set_none_removes_entry() {
        let mut store = NodeStore::new();
        let id = node(3);
        store.insert(
            id,
            NodeRecord::new(
                document(1),
                &NodeKind::Element { tag: "div".into() },
                InteractionState::default(),
            ),
        );
        store.set_overlay_host(id, Some(OverlayHostState::default()));
        assert!(store.overlay_host(id).is_some());
        store.set_overlay_host(id, None);
        assert!(store.overlay_host(id).is_none());
        assert!(store.contains(id));
    }
}
