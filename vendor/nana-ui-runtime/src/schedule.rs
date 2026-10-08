use nana_ui_core::WorkCounters;

use crate::{ExtractedNode, StableNodeId};

/// Per-entity invalidation mask. Widened from `u8` because Issue #8 §6.2 STATE
/// and TRANSFORM are independent bits; packing them into the last unused `u8`
/// lane would leave no room for TRANSFORM.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DirtyMask(u16);

impl DirtyMask {
    pub(crate) const STYLE: u16 = 1 << 0;
    pub(crate) const TEXT: u16 = 1 << 1;
    pub(crate) const LAYOUT: u16 = 1 << 2;
    pub(crate) const INPUT: u16 = 1 << 3;
    pub(crate) const FOCUS_IME: u16 = 1 << 4;
    /// Render extraction and paint. PAINT is not an independent bit; paint-only
    /// mutations (hover color, opacity) set RENDER without LAYOUT.
    pub(crate) const RENDER: u16 = 1 << 5;
    pub(crate) const ACCESSIBILITY: u16 = 1 << 6;
    /// Hover / press / focus / interaction authority. Does not imply STYLE.
    pub(crate) const STATE: u16 = 1 << 7;
    /// Paint transform (and unsupported transform diagnostics). Does not imply
    /// STYLE or LAYOUT. RENDER/INPUT are set only when extract or hit-test
    /// consume the matrix.
    pub(crate) const TRANSFORM: u16 = 1 << 8;
    pub(crate) const ALL: u16 = Self::STYLE
        | Self::TEXT
        | Self::LAYOUT
        | Self::INPUT
        | Self::FOCUS_IME
        | Self::RENDER
        | Self::ACCESSIBILITY
        | Self::STATE
        | Self::TRANSFORM;

    pub(crate) const fn all() -> Self {
        Self(Self::ALL)
    }

    pub(crate) fn insert(&mut self, bits: u16) -> bool {
        let before = self.0;
        self.0 |= bits;
        self.0 != before
    }

    pub(crate) fn take(&mut self) -> u16 {
        std::mem::take(&mut self.0)
    }

    /// Whether every bit in `bits` is set.
    pub(crate) fn has(&self, bits: u16) -> bool {
        self.0 & bits == bits
    }
}

/// Deterministic per-system work produced from entity dirty components.
///
/// PAINT is folded into `RENDER`. STATE and TRANSFORM are independent lists so
/// hover/transform invalidation is not counted as style or layout. Mapping onto
/// Issue #8 dirty bits lives on [`nana_ui_core::WorkCounters`]; STATE/TRANSFORM
/// follow `FOCUS_IME` and stay on this type, not WorkCounters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemWork {
    pub generation: u64,
    pub style: Vec<StableNodeId>,
    /// Hover, press, focus, and `SetInteraction` targets. Empty style is valid.
    /// Observability only: no frame stage consumes it and it does not make the
    /// drain non-empty. See [`Self::is_empty`].
    pub state: Vec<StableNodeId>,
    pub text: Vec<StableNodeId>,
    pub layout: Vec<StableNodeId>,
    /// Paint-transform targets. Empty layout is valid; extract still uses RENDER.
    /// Observability only, like [`Self::state`]. See [`Self::is_empty`].
    pub transform: Vec<StableNodeId>,
    pub input_hit_test: Vec<StableNodeId>,
    pub focus_ime: Vec<StableNodeId>,
    pub accessibility: Vec<StableNodeId>,
    pub accessibility_removals: Vec<StableNodeId>,
    pub render_extraction: Vec<StableNodeId>,
    pub render_removals: Vec<StableNodeId>,
    /// Live retained entity count at drain time.
    pub entities_total: usize,
    /// Mounted entities that contributed dirty bits this drain.
    pub entities_changed: usize,
    /// Entities created since the previous drain.
    pub entities_spawned: usize,
    /// Entities despawned since the previous drain.
    pub entities_despawned: usize,
    /// Unique live pointer hover, press, capture, and focus nodes.
    pub input_targets: usize,
    /// Nodes whose render extraction was invalidated this drain. Not overwritten
    /// by [`Self::record_extract`].
    pub render_nodes_changed: usize,
    /// Nodes named for render extraction. Overwritten by
    /// [`Self::record_extract`] with the number actually produced.
    pub render_nodes_extracted: usize,
    /// Theme-resolved text spans after [`Self::record_extract`]. Zero until then.
    pub extracted_text_spans: usize,
    /// Observed CPU hot-path heap events while draining dirty lists.
    pub allocations: usize,
    /// Payload bytes of those drain-list events.
    pub allocated_bytes: usize,
    /// `TextShaper::shape` invocations. Zero until Runtime records shaping.
    pub text_shaped_runs: usize,
    /// `TextLayoutCache` lookup hits. Zero until shaping records the cache.
    pub text_layout_cache_hits: usize,
    /// `TextLayoutCache` inserts after a miss. Zero until shaping.
    pub text_layout_cache_misses: usize,
    /// Shape calls that requested wrapping. Zero until shaping.
    pub text_wrap_layouts: usize,
    /// `GlyphCache` lookup hits. `None` until a glyph backend consults it.
    pub glyph_cache_hits: Option<usize>,
    /// `GlyphCache` inserts after a miss. `None` until consulted.
    pub glyph_cache_misses: Option<usize>,
    /// `TextLayoutCache` evictions. `None` until a shaping pass consults it.
    pub cache_eviction: Option<usize>,
    /// Retained nodes visited validating the mutation batches behind this drain.
    /// `None` until a commit reports one.
    pub validation_nodes_scanned: Option<usize>,
}

impl SystemWork {
    /// Whether any frame stage has work. `state` and `transform` are excluded:
    /// no stage consumes them, so counting them would schedule a settle pass
    /// that produces nothing. When either invalidation has downstream effect it
    /// arrives with the bit of the stage that does the work — interaction styling
    /// sets STYLE, focus sets FOCUS_IME, and a paint transform sets INPUT and
    /// RENDER — so a frame is still scheduled through that bit.
    pub fn is_empty(&self) -> bool {
        self.style.is_empty()
            && self.text.is_empty()
            && self.layout.is_empty()
            && self.input_hit_test.is_empty()
            && self.focus_ime.is_empty()
            && self.accessibility.is_empty()
            && self.accessibility_removals.is_empty()
            && self.render_extraction.is_empty()
            && self.render_removals.is_empty()
    }

    /// Algorithm-level snapshot for Performance Contract assertions.
    pub fn counters(&self) -> WorkCounters {
        WorkCounters {
            entities_total: self.entities_total,
            entities_changed: self.entities_changed,
            entities_spawned: self.entities_spawned,
            entities_despawned: self.entities_despawned,
            style_processed: self.style.len(),
            text_shaped: self.text.len(),
            layout_nodes: self.layout.len(),
            hit_test_candidates: self.input_hit_test.len(),
            input_targets: self.input_targets,
            accessibility_nodes_updated: self.accessibility.len(),
            render_nodes_changed: self.render_nodes_changed,
            render_nodes_extracted: self.render_nodes_extracted,
            extracted_text_spans: self.extracted_text_spans,
            allocations: self.allocations,
            allocated_bytes: self.allocated_bytes,
            text_shaped_runs: self.text_shaped_runs,
            text_layout_cache_hits: self.text_layout_cache_hits,
            text_layout_cache_misses: self.text_layout_cache_misses,
            text_wrap_layouts: self.text_wrap_layouts,
            glyph_cache_hits: self.glyph_cache_hits,
            glyph_cache_misses: self.glyph_cache_misses,
            cache_eviction: self.cache_eviction,
            batch_rebuilds: None,
            draw_batches: None,
            draw_calls: None,
            gpu_upload_bytes: None,
            gpu_buffer_reallocations: None,
            validation_nodes_scanned: self.validation_nodes_scanned,
            hit_test_nodes_rebuilt: None,
        }
    }

    /// Record extract output. Draw batches and GPU upload bytes are not
    /// available from node extraction and are not fabricated here.
    pub fn record_extract(&mut self, extracted: &[ExtractedNode]) {
        self.render_nodes_extracted = extracted.len();
        self.extracted_text_spans = extracted.iter().map(|node| node.text_spans.len()).sum();
    }

    /// Record observed drain/layout/shape heap events. Does not invent GPU bytes.
    pub fn record_hot_path_allocation(&mut self, count: usize, bytes: usize) {
        if count == 0 && bytes == 0 {
            return;
        }
        self.allocations = self.allocations.saturating_add(count);
        self.allocated_bytes = self.allocated_bytes.saturating_add(bytes);
    }

    /// Record shaping-path stats after the host `TextShaper` ran.
    pub fn record_text_shape(
        &mut self,
        shaped_runs: usize,
        cache_hits: usize,
        cache_misses: usize,
        wrap_layouts: usize,
    ) {
        self.text_shaped_runs = self.text_shaped_runs.saturating_add(shaped_runs);
        self.text_layout_cache_hits = self.text_layout_cache_hits.saturating_add(cache_hits);
        self.text_layout_cache_misses = self.text_layout_cache_misses.saturating_add(cache_misses);
        self.text_wrap_layouts = self.text_wrap_layouts.saturating_add(wrap_layouts);
    }

    pub fn record_cache_eviction(&mut self, evictions: usize) {
        self.cache_eviction = Some(self.cache_eviction.unwrap_or(0).saturating_add(evictions));
    }

    pub fn record_glyph_cache(&mut self, hits: usize, misses: usize) {
        self.glyph_cache_hits = Some(self.glyph_cache_hits.unwrap_or(0).saturating_add(hits));
        self.glyph_cache_misses = Some(self.glyph_cache_misses.unwrap_or(0).saturating_add(misses));
    }
}

pub(crate) fn push_work(work: &mut SystemWork, id: StableNodeId, bits: u16) {
    if bits & DirtyMask::STYLE != 0 {
        work.style.push(id);
    }
    if bits & DirtyMask::STATE != 0 {
        work.state.push(id);
    }
    if bits & DirtyMask::TEXT != 0 {
        work.text.push(id);
    }
    if bits & DirtyMask::LAYOUT != 0 {
        work.layout.push(id);
    }
    if bits & DirtyMask::TRANSFORM != 0 {
        work.transform.push(id);
    }
    if bits & DirtyMask::INPUT != 0 {
        work.input_hit_test.push(id);
    }
    if bits & DirtyMask::FOCUS_IME != 0 {
        work.focus_ime.push(id);
    }
    if bits & DirtyMask::ACCESSIBILITY != 0 {
        work.accessibility.push(id);
    }
    if bits & DirtyMask::RENDER != 0 {
        work.render_extraction.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMED_BITS: [(&str, u16); 9] = [
        ("STYLE", DirtyMask::STYLE),
        ("TEXT", DirtyMask::TEXT),
        ("LAYOUT", DirtyMask::LAYOUT),
        ("INPUT", DirtyMask::INPUT),
        ("FOCUS_IME", DirtyMask::FOCUS_IME),
        ("RENDER", DirtyMask::RENDER),
        ("ACCESSIBILITY", DirtyMask::ACCESSIBILITY),
        ("STATE", DirtyMask::STATE),
        ("TRANSFORM", DirtyMask::TRANSFORM),
    ];

    fn node(raw: u64) -> StableNodeId {
        StableNodeId::new(raw).expect("test node id must be non-zero")
    }

    /// Which of `work`'s lists hold `id`, so a mapping assertion names the lists
    /// rather than restating every field.
    fn lists_containing(work: &SystemWork, id: StableNodeId) -> Vec<&'static str> {
        [
            ("style", &work.style),
            ("state", &work.state),
            ("text", &work.text),
            ("layout", &work.layout),
            ("transform", &work.transform),
            ("input_hit_test", &work.input_hit_test),
            ("focus_ime", &work.focus_ime),
            ("accessibility", &work.accessibility),
            ("render_extraction", &work.render_extraction),
        ]
        .into_iter()
        .filter(|(_, ids)| ids.contains(&id))
        .map(|(name, _)| name)
        .collect()
    }

    #[test]
    fn every_named_dirty_bit_is_a_distinct_lane_inside_all() {
        for (index, (name, bit)) in NAMED_BITS.iter().enumerate() {
            assert_eq!(bit.count_ones(), 1, "{name} must occupy one lane");
            assert_eq!(bit & DirtyMask::ALL, *bit, "{name} must be inside ALL");
            for (other_name, other) in NAMED_BITS.iter().skip(index + 1) {
                assert_eq!(bit & other, 0, "{name} overlaps {other_name}");
            }
        }
        let union = NAMED_BITS.iter().fold(0, |acc, (_, bit)| acc | bit);
        assert_eq!(union, DirtyMask::ALL, "ALL must be exactly the named bits");
        assert!(DirtyMask::all().has(DirtyMask::ALL));
    }

    #[test]
    fn insert_reports_only_newly_added_bits_and_take_clears() {
        let mut mask = DirtyMask::default();
        assert!(!mask.has(DirtyMask::STYLE));
        assert!(mask.insert(DirtyMask::STYLE));
        // Re-inserting a set bit is not a change, which is what lets callers skip
        // re-marking an already-dirty entity.
        assert!(!mask.insert(DirtyMask::STYLE));
        assert!(mask.insert(DirtyMask::STYLE | DirtyMask::LAYOUT));
        assert!(!mask.insert(0));

        // `has` is all-of, not any-of.
        assert!(mask.has(DirtyMask::STYLE | DirtyMask::LAYOUT));
        assert!(!mask.has(DirtyMask::STYLE | DirtyMask::RENDER));

        assert_eq!(mask.take(), DirtyMask::STYLE | DirtyMask::LAYOUT);
        assert_eq!(mask.take(), 0);
        assert!(mask.insert(DirtyMask::STYLE));
    }

    #[test]
    fn each_dirty_bit_routes_to_exactly_one_work_list() {
        let expected = [
            (DirtyMask::STYLE, "style"),
            (DirtyMask::STATE, "state"),
            (DirtyMask::TEXT, "text"),
            (DirtyMask::LAYOUT, "layout"),
            (DirtyMask::TRANSFORM, "transform"),
            (DirtyMask::INPUT, "input_hit_test"),
            (DirtyMask::FOCUS_IME, "focus_ime"),
            (DirtyMask::ACCESSIBILITY, "accessibility"),
            (DirtyMask::RENDER, "render_extraction"),
        ];
        for (bit, list) in expected {
            let mut work = SystemWork::default();
            push_work(&mut work, node(1), bit);
            assert_eq!(lists_containing(&work, node(1)), vec![list]);
        }

        let mut none = SystemWork::default();
        push_work(&mut none, node(1), 0);
        assert!(lists_containing(&none, node(1)).is_empty());

        let mut every = SystemWork::default();
        push_work(&mut every, node(1), DirtyMask::ALL);
        assert_eq!(lists_containing(&every, node(1)).len(), NAMED_BITS.len());
    }

    #[test]
    fn push_work_preserves_drain_order_per_list() {
        let mut work = SystemWork::default();
        push_work(&mut work, node(3), DirtyMask::LAYOUT);
        push_work(&mut work, node(1), DirtyMask::LAYOUT | DirtyMask::STYLE);
        push_work(&mut work, node(2), DirtyMask::LAYOUT);
        assert_eq!(work.layout, vec![node(3), node(1), node(2)]);
        assert_eq!(work.style, vec![node(1)]);
    }

    #[test]
    fn only_bits_with_a_frame_stage_make_a_drain_non_empty() {
        assert!(SystemWork::default().is_empty());

        // STATE and TRANSFORM are observability only; a drain carrying just those
        // would be a settle pass that produces nothing.
        for bit in [DirtyMask::STATE, DirtyMask::TRANSFORM] {
            let mut work = SystemWork::default();
            push_work(&mut work, node(1), bit);
            assert!(work.is_empty());
            assert!(!lists_containing(&work, node(1)).is_empty());
        }

        for (name, bit) in NAMED_BITS {
            if bit == DirtyMask::STATE || bit == DirtyMask::TRANSFORM {
                continue;
            }
            let mut work = SystemWork::default();
            push_work(&mut work, node(1), bit);
            assert!(!work.is_empty(), "{name} must schedule a frame");
        }

        // Removals carry no dirty bit but still need a frame to unpublish nodes.
        let mut render = SystemWork::default();
        render.render_removals.push(node(1));
        assert!(!render.is_empty());
        let mut accessibility = SystemWork::default();
        accessibility.accessibility_removals.push(node(1));
        assert!(!accessibility.is_empty());
    }

    #[test]
    fn counters_report_list_lengths_without_inventing_gpu_numbers() {
        let mut work = SystemWork::default();
        push_work(&mut work, node(1), DirtyMask::ALL);
        push_work(&mut work, node(2), DirtyMask::LAYOUT | DirtyMask::STATE);
        work.render_nodes_changed = work.render_extraction.len();

        let counters = work.counters();
        assert_eq!(counters.style_processed, 1);
        assert_eq!(counters.layout_nodes, 2);
        assert_eq!(counters.text_shaped, 1);
        assert_eq!(counters.hit_test_candidates, 1);
        assert_eq!(counters.accessibility_nodes_updated, 1);
        assert_eq!(counters.render_nodes_changed, 1);
        // STATE and TRANSFORM stay on SystemWork and are not mapped into the
        // Performance Contract counters.
        assert_eq!(counters.draw_batches, None);
        assert_eq!(counters.draw_calls, None);
        assert_eq!(counters.gpu_upload_bytes, None);
        assert_eq!(counters.gpu_buffer_reallocations, None);
        assert_eq!(counters.batch_rebuilds, None);
    }

    #[test]
    fn recorded_stats_accumulate_and_ignore_empty_allocation_reports() {
        let mut work = SystemWork::default();
        work.record_hot_path_allocation(0, 0);
        assert_eq!((work.allocations, work.allocated_bytes), (0, 0));
        work.record_hot_path_allocation(2, 64);
        work.record_hot_path_allocation(1, 8);
        assert_eq!((work.allocations, work.allocated_bytes), (3, 72));

        work.record_text_shape(1, 2, 3, 4);
        work.record_text_shape(1, 1, 1, 1);
        assert_eq!(work.text_shaped_runs, 2);
        assert_eq!(work.text_layout_cache_hits, 3);
        assert_eq!(work.text_layout_cache_misses, 4);
        assert_eq!(work.text_wrap_layouts, 5);

        assert_eq!(work.glyph_cache_hits, None);
        work.record_glyph_cache(1, 0);
        work.record_glyph_cache(2, 1);
        assert_eq!(work.glyph_cache_hits, Some(3));
        assert_eq!(work.glyph_cache_misses, Some(1));

        assert_eq!(work.cache_eviction, None);
        work.record_cache_eviction(0);
        assert_eq!(work.cache_eviction, Some(0));
        work.record_cache_eviction(2);
        assert_eq!(work.cache_eviction, Some(2));
    }
}
