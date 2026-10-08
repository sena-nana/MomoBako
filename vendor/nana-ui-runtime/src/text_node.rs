//! Retained text state per node, and the dirty graph that decides what a
//! mutation costs the text pipeline (Issue #95).
//!
//! A text node does not carry a single `dirty = true`. Every mutation that can
//! touch text names *what* changed as a [`TextDirty`] class, and
//! [`TextDirty::work`] is the one place that says which text work that class
//! implies:
//!
//! ```text
//! CONTENT / FONT / SHAPE_STYLE -> shape + layout + scene text geometry
//! CONSTRAINT                    -> layout + scene text geometry
//! EDIT_STATE                    -> editor overlay / caret
//! PAINT                         -> scene paint only
//! TRANSFORM / OPACITY           -> compositor only
//! ```
//!
//! [`TextNodeState::invalidate`] turns a class into revision bumps, and a
//! resolved node remembers the revisions it was resolved at
//! ([`TextStamp`]). Deciding that a node needs no text work is therefore a
//! comparison of a few integers — before its text is cloned, hashed or looked
//! up — and a paint or compositor class cannot reach a shaping or layout
//! revision because it does not bump one.
//!
//! Today the text passes consume the shape and layout half of the graph.
//! Scene extraction still re-extracts a render-dirty node whole, so the paint,
//! geometry, overlay and compositor work classes (and the `paint` / `edit`
//! revisions) are the contract a renderer drawing retained layouts (#97) and
//! the editable path (#96) key on, not yet a finer extraction.

use std::sync::Arc;

use nana_text::{
    TextConstraints as NanaTextConstraints, TextEngineEpoch, TextKind, TextLayout, TextLayoutId,
    TextSource, TextStyle as NanaTextStyle,
};
use nana_ui_core::{LineHeightSpec, TextAlignSpec};

use crate::{ComputedStyle, TextHorizontalAlignment, TextMetrics, TextShapeConstraints};

/// What changed about a text node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextDirty(u8);

impl TextDirty {
    pub const NONE: Self = Self(0);
    /// The authored text.
    pub const CONTENT: Self = Self(1 << 0);
    /// The font set the text resolves against: a face registered, replaced or
    /// removed, or a fallback policy change.
    pub const FONT: Self = Self(1 << 1);
    /// A style input to shaping: family, size, weight, slant, letter spacing,
    /// features, variation axes, kerning, direction.
    pub const SHAPE_STYLE: Self = Self(1 << 2);
    /// An input to line layout only: the container box, wrapping, clamping,
    /// alignment, line height, word/line breaking, writing mode.
    pub const CONSTRAINT: Self = Self(1 << 3);
    /// Caret, selection or IME composition of an editable node.
    pub const EDIT_STATE: Self = Self(1 << 4);
    /// Colour, text shadow, decoration colour, selection colour.
    pub const PAINT: Self = Self(1 << 5);
    /// The node's transform.
    pub const TRANSFORM: Self = Self(1 << 6);
    /// The node's opacity.
    pub const OPACITY: Self = Self(1 << 7);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The text work this class implies. The whole dependency graph.
    pub const fn work(self) -> TextWork {
        let mut work = TextWork::NONE.0;
        if self.intersects(Self::CONTENT.union(Self::FONT).union(Self::SHAPE_STYLE)) {
            work |= TextWork::SHAPE.0 | TextWork::LAYOUT.0 | TextWork::SCENE_GEOMETRY.0;
        }
        if self.intersects(Self::CONSTRAINT) {
            work |= TextWork::LAYOUT.0 | TextWork::SCENE_GEOMETRY.0;
        }
        if self.intersects(Self::EDIT_STATE) {
            work |= TextWork::EDITOR_OVERLAY.0;
        }
        if self.intersects(Self::PAINT) {
            work |= TextWork::SCENE_PAINT.0;
        }
        if self.intersects(Self::TRANSFORM.union(Self::OPACITY)) {
            work |= TextWork::COMPOSITOR.0;
        }
        TextWork(work)
    }
}

impl std::ops::BitOr for TextDirty {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl std::ops::BitOrAssign for TextDirty {
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}

/// Work a [`TextDirty`] class implies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextWork(u8);

impl TextWork {
    pub const NONE: Self = Self(0);
    /// Shape the text again (a shape cache may still answer it).
    pub const SHAPE: Self = Self(1 << 0);
    /// Lay the shaped runs out again.
    pub const LAYOUT: Self = Self(1 << 1);
    /// Re-extract the node's text geometry into the scene.
    pub const SCENE_GEOMETRY: Self = Self(1 << 2);
    /// Rebuild the editor overlay: caret, selection, composition underline.
    pub const EDITOR_OVERLAY: Self = Self(1 << 3);
    /// Re-extract the node's text paint (colour) only.
    pub const SCENE_PAINT: Self = Self(1 << 4);
    /// Compositor presentation only.
    pub const COMPOSITOR: Self = Self(1 << 5);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Per-node revisions, one per class that can make resolved text stale.
///
/// There is deliberately no transform or opacity revision: those classes
/// imply no text work, so nothing a resolved stamp compares can move.
///
/// `u32` keeps the retained record small: the per-node "no text work" check
/// runs over every candidate of a scope, and it is the record's size, not the
/// comparison, that such a scan pays for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextRevisions {
    pub content: u32,
    pub shape: u32,
    pub constraint: u32,
    pub paint: u32,
    pub edit: u32,
}

/// Which backend a node was resolved by, and against which font set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextBackendEpoch {
    /// The host's [`crate::TextShaper::shape`], by shaper type, at the host's
    /// font generation. Another shaper is another measurement. The type is
    /// kept as a hash of its name, read once per pass: every candidate
    /// compares this epoch, and a name would be a string comparison each.
    Host { shaper: u64, font_generation: u64 },
    /// A `nana-text` engine.
    Engine(TextEngineEpoch),
}

/// A plain text node's retained `nana-text` layout as extraction hands it to
/// the scene: the node's generational handle, and the immutable layout it
/// named when extracted.
///
/// Renderer-neutral IR, not a shaping backend's buffer. Two values are equal
/// only when they are the same handle to the same layout, so a relayout is a
/// scene change and an unchanged layout is not.
#[derive(Debug, Clone)]
pub struct RetainedTextLayout {
    pub id: TextLayoutId,
    pub layout: Arc<TextLayout>,
}

impl PartialEq for RetainedTextLayout {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.layout, &other.layout)
    }
}

/// The revisions and backend a node's current metrics (and layout) were
/// resolved at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextStamp {
    content: u32,
    shape: u32,
    constraint: u32,
    backend: TextBackendEpoch,
    /// Whether the node counts as a text node (it has text, or is a `Text`
    /// node) when resolved. The content revision is part of the stamp, so
    /// while the stamp is current this is still true.
    pub text_node: bool,
    /// Whether the node's text was measured, even as an empty line box, as
    /// opposed to a box without text resolved to nothing. Only a measured node
    /// can hold metrics a new font set changes.
    pub measured: bool,
    /// The constraints and alignment the revisions stood for, for a measured
    /// node. Only compared by the debug check that a revision bump was not
    /// missed.
    #[cfg(debug_assertions)]
    pub constraints: Option<(TextShapeConstraints, TextHorizontalAlignment)>,
}

/// Retained text state of one node.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextNodeState {
    pub revisions: TextRevisions,
    /// The shared source a `nana-text` engine lays out, and the content
    /// revision it was built at. Built once per content revision, only for a
    /// node an engine actually resolves; boxed so a node that never is pays a
    /// pointer, not a source.
    source: Option<Box<(u32, TextSource)>>,
    stamp: Option<TextStamp>,
    /// The layout this node retains in its world's layout store, or null.
    pub layout: TextLayoutId,
}

impl TextNodeState {
    /// Applies `dirty` as revision bumps, through [`TextDirty::work`]: a
    /// revision moves exactly when its work is implied. Classes that imply no
    /// text work bump nothing, so they cannot make a resolved node stale.
    pub fn invalidate(&mut self, dirty: TextDirty) {
        let work = dirty.work();
        let revisions = &mut self.revisions;
        if dirty.intersects(TextDirty::CONTENT) {
            revisions.content = revisions.content.wrapping_add(1);
        }
        if work.intersects(TextWork::SHAPE) {
            revisions.shape = revisions.shape.wrapping_add(1);
        }
        if work.intersects(TextWork::LAYOUT) {
            revisions.constraint = revisions.constraint.wrapping_add(1);
        }
        if work.intersects(TextWork::SCENE_PAINT) {
            revisions.paint = revisions.paint.wrapping_add(1);
        }
        if work.intersects(TextWork::EDITOR_OVERLAY) {
            revisions.edit = revisions.edit.wrapping_add(1);
        }
    }

    /// True when the node was resolved by `backend` at its current content,
    /// shape and constraint revisions. O(1), reads no text.
    pub fn is_current(&self, backend: TextBackendEpoch) -> bool {
        self.stamp.is_some_and(|stamp| {
            stamp.backend == backend
                && stamp.content == self.revisions.content
                && stamp.shape == self.revisions.shape
                && stamp.constraint == self.revisions.constraint
        })
    }

    pub fn stamp(&self) -> Option<&TextStamp> {
        self.stamp.as_ref()
    }

    /// Records that the node was resolved at its current revisions.
    pub fn mark_resolved(
        &mut self,
        backend: TextBackendEpoch,
        #[cfg_attr(not(debug_assertions), allow(unused_variables))] constraints: Option<(
            TextShapeConstraints,
            TextHorizontalAlignment,
        )>,
        text_node: bool,
    ) {
        let measured = constraints.is_some();
        self.stamp = Some(TextStamp {
            content: self.revisions.content,
            shape: self.revisions.shape,
            constraint: self.revisions.constraint,
            backend,
            text_node,
            measured,
            #[cfg(debug_assertions)]
            constraints,
        });
    }

    /// Drops the copy of the text built for the engine.
    pub fn release_source(&mut self) {
        self.source = None;
    }

    /// The source for the node's current content, building it only when the
    /// content revision moved since the last build. Returns whether it copied.
    pub fn source_for(&mut self, text: &str) -> (&TextSource, bool) {
        let revision = self.revisions.content;
        let stale = self.source.as_ref().is_none_or(|built| built.0 != revision);
        if stale {
            self.source = Some(Box::new((revision, TextSource::new(text))));
        }
        let (_, source) = self.source.as_deref().expect("built above");
        (source, stale)
    }
}

/// The style inputs to shaping and layout that a [`ComputedStyle`] change can
/// move, classified. Paint and presentation fields map to their own classes;
/// fields no text work reads map to nothing.
pub(crate) fn classify_computed_style_change(
    previous: &ComputedStyle,
    next: &ComputedStyle,
) -> TextDirty {
    let mut dirty = TextDirty::NONE;
    if previous.font_size.to_bits() != next.font_size.to_bits()
        || previous.font_weight != next.font_weight
        || previous.italic != next.italic
        || previous.font_family != next.font_family
        || previous.letter_spacing.to_bits() != next.letter_spacing.to_bits()
        || previous.font_features != next.font_features
        || previous.font_variations != next.font_variations
        || previous.font_kerning != next.font_kerning
        || previous.direction != next.direction
        || previous.text_orientation != next.text_orientation
    {
        dirty |= TextDirty::SHAPE_STYLE;
    }
    if previous.line_height != next.line_height
        || previous.word_break != next.word_break
        || previous.line_break != next.line_break
        || previous.writing_mode != next.writing_mode
    {
        dirty |= TextDirty::CONSTRAINT;
    }
    if previous.color != next.color
        || previous.foreground != next.foreground
        || previous.selection_background != next.selection_background
        || previous.selection_color != next.selection_color
    {
        dirty |= TextDirty::PAINT;
    }
    if previous.opacity.to_bits() != next.opacity.to_bits() {
        dirty |= TextDirty::OPACITY;
    }
    dirty
}

/// The `nana-text` kind a plain text node lays out as.
pub(crate) fn text_kind(constraints: &TextShapeConstraints) -> TextKind {
    if constraints.wrap || constraints.max_lines.is_some() || constraints.preserve_lines {
        TextKind::Paragraph
    } else {
        TextKind::Label
    }
}

/// The line height the host shaper has always used when none is declared.
const DEFAULT_LINE_HEIGHT: LineHeightSpec = LineHeightSpec::Relative(1.2);

/// A resolved [`ComputedStyle`] as a `nana-text` base style.
pub(crate) fn nana_text_style(style: &ComputedStyle) -> NanaTextStyle {
    NanaTextStyle {
        font_family: style.font_family.as_deref().map(nana_font_family),
        font_size_px: style.font_size.max(f32::MIN_POSITIVE),
        font_weight: style.font_weight.unwrap_or(400),
        italic: style.italic,
        line_height: Some(style.line_height.unwrap_or(DEFAULT_LINE_HEIGHT)),
        letter_spacing_px: if style.letter_spacing.is_finite() {
            style.letter_spacing
        } else {
            0.0
        },
        features: style.font_features.clone(),
        variations: style.font_variations.clone(),
        kerning: style.font_kerning,
    }
}

/// A family list with the generic the host shaper has always implied: a named
/// family that says `mono` falls back to `monospace`, anything else to
/// `sans-serif` (which `nana-text` appends itself).
///
/// Public because the *painter* has to ask the engine for the same layout this
/// node was measured with, and it builds its style from the scene rather than
/// from `ComputedStyle`. Two spellings of this rule would be two font
/// selections for one node.
pub fn nana_font_family(family: &str) -> Arc<str> {
    let lowered = family.to_ascii_lowercase();
    let has_generic = lowered.split(',').any(|name| {
        matches!(
            name.trim().trim_matches(|c| c == '"' || c == '\''),
            "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
        )
    });
    if !has_generic && lowered.contains("mono") {
        Arc::from(format!("{family}, monospace"))
    } else {
        Arc::from(family)
    }
}

/// A plain text node's shape constraints and authored alignment as
/// `nana-text` layout constraints.
pub(crate) fn nana_text_constraints(
    style: &ComputedStyle,
    constraints: &TextShapeConstraints,
    alignment: TextHorizontalAlignment,
) -> NanaTextConstraints {
    let mut nana = NanaTextConstraints {
        max_width_px: constraints.max_width,
        max_height_px: constraints.max_height,
        wrap: constraints.wrap.then_some(constraints.wrap_break),
        word_break: style.word_break,
        line_break: style.line_break,
        max_lines: constraints.max_lines,
        ellipsis: constraints.ellipsis,
        preserve_lines: constraints.preserve_lines,
        // The used direction: `text-orientation: upright` reads `ltr`.
        base_direction: style.writing_context().direction,
        align: match alignment {
            TextHorizontalAlignment::Start => TextAlignSpec::Start,
            TextHorizontalAlignment::Center => TextAlignSpec::Center,
            TextHorizontalAlignment::End => TextAlignSpec::End,
        },
        writing_mode: style.writing_mode,
        text_orientation: style.text_orientation,
        ..NanaTextConstraints::default()
    };
    // The box dimension lines *stack* along — the height, or the width of a
    // vertical paragraph — is a *truncation* budget: `nana-text` drops the
    // lines that do not fit it. A box too short for its text is an overflow,
    // not a shorter paragraph — CSS clips it at paint and the text is still
    // there to select and to hit-test. So the engine only gets it when
    // truncation was actually asked for; `max_lines` travels on its own and
    // clamps either way.
    let vertical = nana.wants_vertical_writing();
    let stacking = if vertical {
        &mut nana.max_width_px
    } else {
        &mut nana.max_height_px
    };
    if !constraints.ellipsis {
        *stacking = None;
    }
    nana
}

/// The Runtime metrics contract read off a layout: the page size of its lines
/// ([`TextLayout::physical_size`] — the widest line and the summed line boxes,
/// crossed over for vertical text), and the first line's ascent.
///
/// A vertical layout reports no ascent: its lines hang from a central
/// baseline, and handing that to a box layout that aligns alphabetic
/// baselines would put a column's midpoint on a horizontal neighbour's text
/// line.
pub(crate) fn text_metrics_of_layout(layout: &TextLayout) -> TextMetrics {
    let (width, height) = layout.physical_size();
    let ascent = layout
        .lines
        .first()
        .filter(|_| !layout.is_vertical())
        .map(|line| (line.metrics.baseline_y_px - line.metrics.top_y_px).max(0.0))
        .filter(|ascent| ascent.is_finite());
    TextMetrics {
        width,
        height,
        ascent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [TextDirty; 8] = [
        TextDirty::CONTENT,
        TextDirty::FONT,
        TextDirty::SHAPE_STYLE,
        TextDirty::CONSTRAINT,
        TextDirty::EDIT_STATE,
        TextDirty::PAINT,
        TextDirty::TRANSFORM,
        TextDirty::OPACITY,
    ];

    #[test]
    fn the_dependency_graph_is_the_one_the_issue_draws() {
        let shape_class = TextWork::SHAPE;
        for class in [TextDirty::CONTENT, TextDirty::FONT, TextDirty::SHAPE_STYLE] {
            let work = class.work();
            assert!(work.contains(shape_class), "{class:?} must reshape");
            assert!(work.contains(TextWork::LAYOUT), "{class:?} must relayout");
            assert!(work.contains(TextWork::SCENE_GEOMETRY));
            assert!(!work.intersects(TextWork::COMPOSITOR));
        }
        let constraint = TextDirty::CONSTRAINT.work();
        assert!(constraint.contains(TextWork::LAYOUT));
        assert!(constraint.contains(TextWork::SCENE_GEOMETRY));
        assert!(
            !constraint.intersects(TextWork::SHAPE),
            "a constraint change lays the existing shaped runs out again"
        );
        assert_eq!(TextDirty::EDIT_STATE.work(), TextWork::EDITOR_OVERLAY);
        assert_eq!(TextDirty::PAINT.work(), TextWork::SCENE_PAINT);
        assert_eq!(TextDirty::TRANSFORM.work(), TextWork::COMPOSITOR);
        assert_eq!(TextDirty::OPACITY.work(), TextWork::COMPOSITOR);
        assert!(TextDirty::NONE.work().is_empty());
    }

    #[test]
    fn only_shaping_and_layout_classes_can_make_a_resolved_node_stale() {
        let backend = TextBackendEpoch::Host {
            shaper: 7,
            font_generation: 0,
        };
        for class in ALL {
            let mut node = TextNodeState::default();
            node.mark_resolved(backend, None, true);
            node.invalidate(class);
            let needs_text_work = class.work().intersects(TextWork::SHAPE.union_layout());
            assert_eq!(
                !node.is_current(backend),
                needs_text_work,
                "{class:?}: staleness must follow the dependency graph"
            );
        }
    }

    #[test]
    fn paint_and_compositor_classes_never_move_a_shaping_or_layout_revision() {
        for class in [
            TextDirty::PAINT,
            TextDirty::TRANSFORM,
            TextDirty::OPACITY,
            TextDirty::EDIT_STATE,
        ] {
            let mut node = TextNodeState::default();
            let before = node.revisions;
            node.invalidate(class);
            assert_eq!(node.revisions.content, before.content, "{class:?}");
            assert_eq!(node.revisions.shape, before.shape, "{class:?}");
            assert_eq!(node.revisions.constraint, before.constraint, "{class:?}");
        }
    }

    #[test]
    fn a_new_font_set_makes_a_resolved_node_stale_without_any_revision_bump() {
        let mut node = TextNodeState::default();
        node.mark_resolved(
            TextBackendEpoch::Host {
                shaper: 7,
                font_generation: 3,
            },
            None,
            true,
        );
        assert!(node.is_current(TextBackendEpoch::Host {
            shaper: 7,
            font_generation: 3,
        }));
        assert!(!node.is_current(TextBackendEpoch::Host {
            shaper: 7,
            font_generation: 4,
        }));
    }

    #[test]
    fn the_source_is_built_once_per_content_revision() {
        let mut node = TextNodeState::default();
        let (_, copied) = node.source_for("hello");
        assert!(copied);
        let (source, copied) = node.source_for("hello");
        assert!(!copied, "an unchanged revision reuses the built source");
        assert_eq!(source.text(), "hello");
        node.invalidate(TextDirty::CONTENT);
        let (source, copied) = node.source_for("bye");
        assert!(copied);
        assert_eq!(source.text(), "bye");
    }

    #[test]
    fn style_changes_are_classified_by_what_reads_them() {
        let base = ComputedStyle::default();
        let classify = |edit: fn(&mut ComputedStyle)| {
            let mut next = base.clone();
            edit(&mut next);
            classify_computed_style_change(&base, &next)
        };
        assert_eq!(
            classify(|s| s.color = Some([1.0, 0.0, 0.0, 1.0])),
            TextDirty::PAINT
        );
        assert_eq!(classify(|s| s.opacity = 0.5), TextDirty::OPACITY);
        assert_eq!(classify(|s| s.font_size = 20.0), TextDirty::SHAPE_STYLE);
        assert_eq!(
            classify(|s| s.font_weight = Some(700)),
            TextDirty::SHAPE_STYLE
        );
        assert_eq!(
            classify(
                |s| s.font_variations = vec![nana_ui_core::FontVariationSetting {
                    tag: *b"wdth",
                    value: 80.0,
                }]
            ),
            TextDirty::SHAPE_STYLE
        );
        assert_eq!(
            classify(|s| s.line_height = Some(LineHeightSpec::Relative(2.0))),
            TextDirty::CONSTRAINT
        );
        assert_eq!(classify(|s| s.cursor_specified = true), TextDirty::NONE);
    }

    #[test]
    fn a_named_mono_family_keeps_its_monospace_fallback() {
        assert_eq!(
            &*nana_font_family("JetBrains Mono"),
            "JetBrains Mono, monospace"
        );
        assert_eq!(&*nana_font_family("Inter"), "Inter");
        assert_eq!(&*nana_font_family("Fira Mono, serif"), "Fira Mono, serif");
    }

    impl TextWork {
        const fn union_layout(self) -> Self {
            Self(self.0 | Self::LAYOUT.0)
        }
    }
}
