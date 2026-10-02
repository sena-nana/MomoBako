//! CPU-side raster cache, separate from any device's atlas.
//!
//! The split is the point. The atlas is device memory and dies with its
//! device; this cache is plain bytes and survives one. A second window, a
//! second device, or a device that was lost and recreated re-*uploads* from
//! here — it never re-shapes, and re-rasterizes only what this cache's budget
//! made it drop.
//!
//! Results are handed out as `Arc<GlyphImage>`, which is also the lifetime
//! contract: an upload already queued keeps its pixels alive after the entry
//! that produced them has been evicted, so eviction can never race a pending
//! upload or leave the queue pointing at freed bytes.

use std::collections::HashMap;
use std::sync::Arc;

use super::glyph::GlyphRasterKey;
use super::raster::{GlyphImage, GlyphRasterRequest, GlyphRasterizer};

/// Entry ceiling. A dense CJK page is a few thousand distinct glyphs; the cap
/// is above that so a real document is resident, not so far above that a
/// corpus of unique glyphs can hold the process hostage.
const DEFAULT_ENTRY_CAP: usize = 8192;
/// Byte ceiling, which is the binding one for large text: 16 MiB is roughly
/// 4000 glyphs of 64×64 coverage.
const DEFAULT_BYTE_BUDGET: usize = 16 * 1024 * 1024;
/// Eviction drops to this fraction of both ceilings in one pass, so the scan
/// that picks victims is amortized over many inserts rather than run per
/// glyph.
const EVICT_TO: f32 = 0.85;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GlyphRasterCounters {
    /// Every lookup, including the repeats within one frame that dedup into a
    /// hit. `requests - hits` is what reached the backend boundary.
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub rasterized: u64,
    pub evictions: u64,
}

struct CachedGlyph {
    /// `None` when the backend could not produce the glyph at all. Cached so a
    /// missing face is asked about once, not once per frame.
    image: Option<Arc<GlyphImage>>,
    bytes: usize,
    last_used: u64,
}

pub(super) struct GlyphRasterCache {
    entries: HashMap<GlyphRasterKey, CachedGlyph>,
    bytes: usize,
    entry_cap: usize,
    byte_budget: usize,
    clock: u64,
    /// Content epoch. Bumped by [`Self::invalidate`]; an atlas entry records
    /// the epoch its pixels came from, so a wholesale invalidation (the font
    /// set changed under everything) is detectable downstream instead of
    /// leaving the atlas holding bitmaps from a face set that no longer
    /// exists.
    generation: u64,
    counters: GlyphRasterCounters,
}

impl Default for GlyphRasterCache {
    fn default() -> Self {
        Self::with_budget(DEFAULT_ENTRY_CAP, DEFAULT_BYTE_BUDGET)
    }
}

impl GlyphRasterCache {
    pub(super) fn with_budget(entry_cap: usize, byte_budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            entry_cap: entry_cap.max(1),
            byte_budget: byte_budget.max(1),
            clock: 0,
            generation: 1,
            counters: GlyphRasterCounters::default(),
        }
    }

    pub(super) fn counters(&self) -> GlyphRasterCounters {
        self.counters
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The bitmap for `key` if it is still cached, without counting a lookup
    /// or aging the entry.
    ///
    /// For the atlas, which asks whether it may relocate a glyph rather than
    /// whether it needs one; charging that to the hit rate would hide what the
    /// draw path actually did.
    pub(super) fn peek(&self, key: &GlyphRasterKey) -> Option<Arc<GlyphImage>> {
        self.entries.get(key).and_then(|entry| entry.image.clone())
    }

    /// Drop every bitmap and start a new epoch.
    ///
    /// For the case where the bytes are not merely cold but no longer
    /// meaningful — the font set changed, so the faces the keys name are gone.
    pub(super) fn invalidate(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    /// The glyph for `key`, rasterizing it once if this is the first ask.
    ///
    /// `None` means "nothing to draw", for both a blank glyph and a face the
    /// backend cannot scale; both answers are cached, so a repeat within the
    /// same frame or across frames is a hit and never a second backend call.
    pub(super) fn get_or_rasterize(
        &mut self,
        rasterizer: &mut impl GlyphRasterizer,
        key: GlyphRasterKey,
    ) -> Option<Arc<GlyphImage>> {
        self.counters.requests += 1;
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            self.counters.hits += 1;
            entry.last_used = self.clock;
            return entry.image.clone();
        }
        self.counters.misses += 1;
        let image = rasterizer.rasterize(&GlyphRasterRequest { key });
        self.counters.rasterized += 1;
        let image = image.filter(|image| !image.is_empty()).map(Arc::new);
        let bytes = image.as_ref().map_or(0, |image| image.byte_len());
        self.bytes += bytes;
        self.entries.insert(
            key,
            CachedGlyph {
                image: image.clone(),
                bytes,
                last_used: self.clock,
            },
        );
        self.evict_if_over_budget();
        image
    }

    /// Drop the coldest entries in one pass when either ceiling is exceeded.
    ///
    /// Batched down to [`EVICT_TO`] rather than evicting exactly one victim:
    /// picking a victim costs a scan of the whole table, and running that scan
    /// per inserted glyph is what would turn a large unique-glyph corpus into
    /// a quadratic frame.
    fn evict_if_over_budget(&mut self) {
        if self.entries.len() <= self.entry_cap && self.bytes <= self.byte_budget {
            return;
        }
        let entry_target = (self.entry_cap as f32 * EVICT_TO) as usize;
        let byte_target = (self.byte_budget as f32 * EVICT_TO) as usize;
        let mut ages: Vec<(u64, GlyphRasterKey)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.last_used, *key))
            .collect();
        ages.sort_unstable_by_key(|(age, _)| *age);
        for (_, key) in ages {
            if self.entries.len() <= entry_target && self.bytes <= byte_target {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
                self.counters.evictions += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::glyph::{
        GlyphFontId, GlyphRenderMode, GlyphSynthesis, GlyphVariationId, SubpixelBin,
    };
    use super::super::raster::GlyphImageFormat;
    use super::*;

    #[derive(Default)]
    struct CountingRasterizer {
        calls: usize,
        /// Glyph ids the backend refuses outright.
        missing: Vec<u32>,
    }

    impl GlyphRasterizer for CountingRasterizer {
        fn rasterize(&mut self, request: &GlyphRasterRequest) -> Option<GlyphImage> {
            self.calls += 1;
            if self.missing.contains(&request.key.glyph) {
                return None;
            }
            Some(GlyphImage {
                format: GlyphImageFormat::Mask,
                width: 4,
                height: 4,
                left: 0,
                top: 4,
                data: vec![255; 16],
            })
        }
    }

    fn key(glyph: u32) -> GlyphRasterKey {
        GlyphRasterKey {
            font: GlyphFontId(0),
            font_generation: 1,
            variation: GlyphVariationId(0),
            glyph,
            size_bits: 16f32.to_bits(),
            subpixel_x: SubpixelBin::default(),
            subpixel_y: SubpixelBin::default(),
            synthesis: GlyphSynthesis::NONE,
            mode: GlyphRenderMode::Mask,
        }
    }

    #[test]
    fn a_repeated_request_dedups_into_one_backend_call() {
        let mut cache = GlyphRasterCache::default();
        let mut rasterizer = CountingRasterizer::default();
        for _ in 0..5 {
            assert!(cache.get_or_rasterize(&mut rasterizer, key(7)).is_some());
        }
        assert_eq!(rasterizer.calls, 1, "four repeats must be cache hits");
        let counters = cache.counters();
        assert_eq!(counters.requests, 5);
        assert_eq!(counters.hits, 4);
        assert_eq!(counters.misses, 1);
        assert_eq!(counters.rasterized, 1);
    }

    #[test]
    fn a_glyph_the_backend_cannot_produce_is_asked_for_once() {
        let mut cache = GlyphRasterCache::default();
        let mut rasterizer = CountingRasterizer {
            missing: vec![9],
            ..CountingRasterizer::default()
        };
        assert!(cache.get_or_rasterize(&mut rasterizer, key(9)).is_none());
        assert!(cache.get_or_rasterize(&mut rasterizer, key(9)).is_none());
        assert_eq!(rasterizer.calls, 1);
        assert_eq!(cache.counters().hits, 1);
    }

    #[test]
    fn the_byte_budget_evicts_the_coldest_entries_and_keeps_the_hot_one() {
        // Four glyphs of 16 bytes fit; the fifth does not.
        let mut cache = GlyphRasterCache::with_budget(64, 64);
        let mut rasterizer = CountingRasterizer::default();
        for glyph in 0..4 {
            cache.get_or_rasterize(&mut rasterizer, key(glyph));
        }
        // Keep glyph 0 hot so it outranks the ones inserted after it.
        cache.get_or_rasterize(&mut rasterizer, key(0));
        cache.get_or_rasterize(&mut rasterizer, key(4));
        assert!(cache.bytes() <= 64, "the budget must hold after eviction");
        assert!(cache.counters().evictions > 0);
        let before = rasterizer.calls;
        cache.get_or_rasterize(&mut rasterizer, key(0));
        assert_eq!(
            rasterizer.calls, before,
            "the most recently used glyph must survive eviction"
        );
    }

    #[test]
    fn a_new_epoch_drops_every_bitmap_and_is_observable() {
        let mut cache = GlyphRasterCache::default();
        let mut rasterizer = CountingRasterizer::default();
        cache.get_or_rasterize(&mut rasterizer, key(1));
        let epoch = cache.generation();
        cache.invalidate();
        assert_ne!(cache.generation(), epoch);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes(), 0);
        cache.get_or_rasterize(&mut rasterizer, key(1));
        assert_eq!(rasterizer.calls, 2, "an invalidated glyph re-rasterizes");
    }

    #[test]
    fn an_evicted_bitmap_stays_alive_for_an_upload_that_already_holds_it() {
        let mut cache = GlyphRasterCache::with_budget(1, 16);
        let mut rasterizer = CountingRasterizer::default();
        let queued = cache
            .get_or_rasterize(&mut rasterizer, key(1))
            .expect("rasterized");
        cache.get_or_rasterize(&mut rasterizer, key(2));
        assert!(cache.counters().evictions > 0);
        assert_eq!(queued.data.len(), 16, "a queued upload outlives its entry");
    }
}
