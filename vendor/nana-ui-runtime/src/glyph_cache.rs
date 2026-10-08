//! Runtime-owned per-glyph metrics cache (Issue #8 §3.5 / §11.4).
//!
//! Lookup/insert are the only hit/miss sources for `glyph_cache_*`. This is not
//! [`TextLayoutCache`] (whole-string layout) and not a GPU atlas. Hosts without
//! a glyph backend must not consult this cache, so counters stay `None`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::ComputedStyle;

/// Bounded FIFO cache. Eviction is real; it is not exported as `cache_eviction`
/// (that field stays `TextLayoutCache` FIFO).
const DEFAULT_CAP: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    font_size_bits: u32,
    font_weight: u16,
    font_family: Option<Arc<str>>,
    letter_spacing_bits: u32,
    italic: bool,
    font_features: Vec<nana_ui_core::FontFeatureSetting>,
    font_variations: Vec<nana_ui_core::FontVariationSetting>,
    font_kerning: nana_ui_core::FontKerningSpec,
}

impl GlyphKey {
    fn new(ch: char, style: &ComputedStyle) -> Self {
        Self {
            ch,
            font_size_bits: style.font_size.to_bits(),
            font_weight: style.font_weight.unwrap_or(0),
            font_family: style.font_family.clone(),
            letter_spacing_bits: style.letter_spacing.to_bits(),
            italic: style.italic,
            font_features: style.font_features.clone(),
            font_variations: style.font_variations.clone(),
            font_kerning: style.font_kerning,
        }
    }
}

/// Per-glyph advance cache for a `TextShaper::shape_cached` backend that has no
/// text engine of its own (the em-width `MeasureTextShaper` of tests and
/// benchmarks). The `nana-text` shaper does not consult it (#99): the engine's
/// layout cache already answers a repeated string, and a per-character answer
/// cannot carry what a layout does (the first line's ascent, kerning).
#[derive(Debug)]
pub struct GlyphCache {
    entries: HashMap<GlyphKey, f32>,
    order: VecDeque<GlyphKey>,
    cap: usize,
    hits: usize,
    misses: usize,
    consulted: bool,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }
}

impl GlyphCache {
    pub fn with_cap(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
            hits: 0,
            misses: 0,
            consulted: false,
        }
    }

    /// Drops every advance, keeping the counters: the font set they were
    /// measured against is gone.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Read without counting. Used to decide a single-glyph fast path.
    pub fn peek(&self, ch: char, style: &ComputedStyle) -> Option<f32> {
        self.entries.get(&GlyphKey::new(ch, style)).copied()
    }

    /// Record a hit when the glyph is present. A miss is recorded on [`Self::insert`].
    pub fn lookup(&mut self, ch: char, style: &ComputedStyle) -> Option<f32> {
        self.consulted = true;
        let advance = self.entries.get(&GlyphKey::new(ch, style)).copied()?;
        self.hits = self.hits.saturating_add(1);
        Some(advance)
    }

    #[allow(clippy::map_entry)]
    pub fn insert(&mut self, ch: char, style: &ComputedStyle, advance: f32) {
        self.consulted = true;
        self.misses = self.misses.saturating_add(1);
        let key = GlyphKey::new(ch, style);
        if self.entries.contains_key(&key) {
            self.entries.insert(key, advance);
            return;
        }
        while self.entries.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, advance);
    }

    /// Lookup, inserting `advance` on a miss.
    pub fn lookup_or_insert(&mut self, ch: char, style: &ComputedStyle, advance: f32) -> f32 {
        if let Some(cached) = self.lookup(ch, style) {
            return cached;
        }
        self.insert(ch, style, advance);
        advance
    }

    /// `None` when this shaping pass never looked up or inserted a glyph.
    pub(crate) fn take_counters(&mut self) -> Option<(usize, usize)> {
        if !self.consulted {
            return None;
        }
        let stats = (self.hits, self.misses);
        self.hits = 0;
        self.misses = 0;
        self.consulted = false;
        Some(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_insert_records_miss_then_hit() {
        let mut cache = GlyphCache::with_cap(2);
        let style = ComputedStyle::default();
        assert!(cache.peek('a', &style).is_none());
        assert!(cache.lookup('a', &style).is_none());
        cache.insert('a', &style, 8.0);
        assert_eq!(cache.lookup('a', &style), Some(8.0));
        cache.insert('b', &style, 9.0);
        cache.insert('c', &style, 10.0);
        assert!(cache.peek('a', &style).is_none());
        assert_eq!(cache.lookup('b', &style), Some(9.0));

        let (hits, misses) = cache.take_counters().expect("cache was consulted");
        assert_eq!(hits, 2);
        assert_eq!(misses, 3);
        assert!(cache.take_counters().is_none());
    }

    #[test]
    fn lookup_or_insert_is_the_hit_miss_pair() {
        let mut cache = GlyphCache::with_cap(8);
        let style = ComputedStyle::default();
        assert_eq!(cache.lookup_or_insert('x', &style, 7.0), 7.0);
        assert_eq!(cache.lookup_or_insert('x', &style, 99.0), 7.0);
        let (hits, misses) = cache.take_counters().expect("cache was consulted");
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }

    #[test]
    fn letter_spacing_is_part_of_the_glyph_key() {
        let mut cache = GlyphCache::with_cap(8);
        let tight = ComputedStyle {
            font_size: 16.0,
            letter_spacing: 0.0,
            ..ComputedStyle::default()
        };
        let tracked = ComputedStyle {
            font_size: 16.0,
            letter_spacing: 0.5,
            ..ComputedStyle::default()
        };
        cache.insert('a', &tight, 8.0);
        assert!(cache.peek('a', &tracked).is_none());
        cache.insert('a', &tracked, 8.5);
        assert_eq!(cache.peek('a', &tight), Some(8.0));
        assert_eq!(cache.peek('a', &tracked), Some(8.5));
    }

    #[test]
    fn font_features_are_part_of_the_glyph_key() {
        let mut cache = GlyphCache::with_cap(8);
        let liga_off = ComputedStyle {
            font_size: 16.0,
            font_features: vec![nana_ui_core::FontFeatureSetting::new(*b"liga", 0)],
            ..ComputedStyle::default()
        };
        let liga_on = ComputedStyle {
            font_size: 16.0,
            font_features: vec![nana_ui_core::FontFeatureSetting::new(*b"liga", 1)],
            ..ComputedStyle::default()
        };
        cache.insert('f', &liga_off, 8.0);
        assert!(cache.peek('f', &liga_on).is_none());
    }
}
