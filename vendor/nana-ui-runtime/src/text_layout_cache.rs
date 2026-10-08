//! Runtime-owned text layout cache (Issue #8 §3.5 / §11.4).
//!
//! Lookup/insert are the only hit/miss sources for `text_layout_cache_*`.
//! This is not a glyph atlas; per-glyph advances live on [`crate::GlyphCache`].

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use nana_ui_core::LineHeightSpec;

use crate::{ComputedStyle, TextContent, TextMetrics, TextShapeConstraints, TextShaping};

/// Bounded FIFO cache. Eviction is a real event, not a stand-in for glyph trim.
const DEFAULT_CAP: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextLayoutKey {
    text: Arc<str>,
    font_size_bits: u32,
    font_weight: u16,
    font_family: Option<Arc<str>>,
    line_height: Option<(u8, u32)>,
    letter_spacing_bits: u32,
    font_features: Vec<nana_ui_core::FontFeatureSetting>,
    font_variations: Vec<nana_ui_core::FontVariationSetting>,
    font_kerning: nana_ui_core::FontKerningSpec,
    word_break: nana_ui_core::WordBreakSpec,
    line_break: nana_ui_core::LineBreakSpec,
    writing_mode: nana_ui_core::WritingModeSpec,
    text_orientation: nana_ui_core::TextOrientationSpec,
    wrap: bool,
    ellipsis: bool,
    max_lines: Option<u16>,
    preserve_lines: bool,
    wrap_break: nana_ui_core::TextWrapBreak,
    italic: bool,
    shaping: u8,
    max_width_bits: Option<u32>,
    max_height_bits: Option<u32>,
    /// The host's font generation: metrics measured against a font set that
    /// has since changed must miss rather than answer for the new one.
    font_generation: u64,
}

impl TextLayoutKey {
    pub(crate) fn new(
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
        font_generation: u64,
    ) -> Self {
        Self {
            font_generation,
            text: Arc::from(text.value.as_str()),
            font_size_bits: style.font_size.to_bits(),
            font_weight: style.font_weight.unwrap_or(0),
            font_family: style.font_family.clone(),
            line_height: style.line_height.map(|spec| match spec {
                LineHeightSpec::Relative(value) => (0, value.to_bits()),
                LineHeightSpec::Absolute(value) => (1, value.to_bits()),
            }),
            letter_spacing_bits: style.letter_spacing.to_bits(),
            font_features: style.font_features.clone(),
            font_variations: style.font_variations.clone(),
            font_kerning: style.font_kerning,
            word_break: style.word_break,
            line_break: style.line_break,
            writing_mode: style.writing_mode,
            text_orientation: style.text_orientation,
            wrap: constraints.wrap,
            ellipsis: constraints.ellipsis,
            max_lines: constraints.max_lines,
            preserve_lines: constraints.preserve_lines,
            wrap_break: constraints.wrap_break,
            italic: style.italic,
            shaping: match constraints.shaping {
                TextShaping::Auto => 0,
                TextShaping::Advanced => 1,
            },
            max_width_bits: constraints.max_width.map(f32::to_bits),
            max_height_bits: constraints.max_height.map(f32::to_bits),
        }
    }
}

#[derive(Debug)]
pub(crate) struct TextLayoutCache {
    entries: HashMap<TextLayoutKey, TextMetrics>,
    order: VecDeque<TextLayoutKey>,
    cap: usize,
    hits: usize,
    misses: usize,
    evictions: usize,
}

impl Default for TextLayoutCache {
    fn default() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }
}

impl TextLayoutCache {
    pub(crate) fn with_cap(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    /// Drops every entry, keeping the counters.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Record a hit when the key is present. A miss is recorded on [`Self::insert`].
    pub(crate) fn lookup(&mut self, key: &TextLayoutKey) -> Option<TextMetrics> {
        let metrics = self.entries.get(key).copied()?;
        self.hits = self.hits.saturating_add(1);
        Some(metrics)
    }

    #[allow(clippy::map_entry)]
    pub(crate) fn insert(&mut self, key: TextLayoutKey, metrics: TextMetrics) {
        self.misses = self.misses.saturating_add(1);
        if self.entries.contains_key(&key) {
            self.entries.insert(key, metrics);
            return;
        }
        while self.entries.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
                self.evictions = self.evictions.saturating_add(1);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, metrics);
    }

    pub(crate) fn take_counters(&mut self) -> (usize, usize, usize) {
        let stats = (self.hits, self.misses, self.evictions);
        self.hits = 0;
        self.misses = 0;
        self.evictions = 0;
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextContent;

    fn key(text: &str, width: Option<f32>) -> TextLayoutKey {
        TextLayoutKey::new(
            &TextContent { value: text.into() },
            &ComputedStyle::default(),
            TextShapeConstraints {
                max_width: width,
                wrap: width.is_some(),
                ..TextShapeConstraints::default()
            },
            0,
        )
    }

    #[test]
    fn lookup_insert_records_miss_then_hit_and_fifo_eviction() {
        let mut cache = TextLayoutCache::with_cap(2);
        let first = key("a", None);
        let second = key("b", None);
        let third = key("c", None);
        let metrics = TextMetrics {
            width: 1.0,
            height: 1.0,
            ascent: None,
        };

        assert!(cache.lookup(&first).is_none());
        cache.insert(first.clone(), metrics);
        assert_eq!(cache.lookup(&first), Some(metrics));
        cache.insert(second.clone(), metrics);
        cache.insert(third, metrics);
        assert!(cache.lookup(&first).is_none());
        assert_eq!(cache.lookup(&second), Some(metrics));

        let (hits, misses, evictions) = cache.take_counters();
        assert_eq!(hits, 2);
        assert_eq!(misses, 3);
        assert_eq!(evictions, 1);
    }
}
