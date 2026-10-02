//! `NanaRenderer::text`: the renderer's own glyph subsystem.
//!
//! ```text
//! shaped paragraph
//!         ↓  resolve
//!     NanaGlyphRun / PlacedGlyph        glyph.rs
//!         ↓  GlyphRasterKey
//!     GlyphRasterCache ── GlyphRasterizer   raster_cache.rs / raster.rs
//!         ↓  GlyphImage
//!     GlyphAtlasManager ── GlyphUploadQueue atlas.rs / upload.rs
//!         ↓  GlyphAtlasEntryId
//!     TextPipeline ──────────────────────── pipeline.rs
//!         ↓
//!        WGPU
//! ```
//!
//! Nothing below the resolver knows what laid the paragraph out. Since #99
//! that is `nana-text`: this module asks the process-wide engine for an
//! immutable [`nana_text::TextLayout`] and resolves it into glyph runs. No
//! cosmic-text type reaches the paint path.
//!
//! Three lifetimes meet here and are deliberately not the same:
//!
//! - **Shaped paragraphs** are CPU state of one painter, keyed by content and
//!   style. A repaint of unchanged text reuses them.
//! - **Glyph bitmaps** are CPU state too, keyed by face instance, size,
//!   subpixel bin and synthesis — never by color, opacity or transform.
//! - **Atlas placements** are device state, shared by every window on one
//!   device and reachable only through a generational handle, so eviction and
//!   compaction cannot be observed as the wrong glyph.

mod atlas;
mod entry;
mod gamma;
mod glyph;
mod pipeline;
mod raster;
mod raster_cache;
#[cfg(windows)]
mod raster_dwrite;
mod upload;

use nana_text::{TextEngine as _, TextLayout};
use nana_ui_core::{LineHeightSpec, TextAlignSpec};
use nana_ui_runtime::{TextHorizontalAlignment, TextShaping, TextVerticalAlignment};
use nana_ui_scene::{SceneTextOpenType, SceneTextSpan};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use self::atlas::{AtlasPageKind, GlyphAtlasLimits, GlyphAtlasManager};
pub(in crate::scene_paint) use self::entry::EntryKey;
use self::entry::{
    EntrySegment, EntryStore, RunSlots, SegmentBuilder, SlotArena, VACANT_INDEX, range_keeps,
};
use self::glyph::{GlyphRenderMode, GlyphSynthesis, NanaGlyphBuffer, PlacedGlyph, size_bits};
use self::pipeline::{
    ArenaWrite, CONTENT_COLOR, CONTENT_MASK, DrawSegment, FrameUpload, GlyphInstance, TextGpu,
    TextPresentationGpu, TextRunGpu, TextTargetGpu,
};
/// The glyph rasterizer this platform draws with: DirectWrite on Windows,
/// so text there matches every native app; swash elsewhere.
#[cfg(windows)]
type PlatformRasterizer = self::raster_dwrite::DWriteGlyphRasterizer;
#[cfg(not(windows))]
type PlatformRasterizer = self::raster::SwashGlyphRasterizer;
use self::raster_cache::GlyphRasterCache;
use self::upload::GlyphUploadQueue;

use super::clip::{self, LogicalRect};
use super::color::{linear_from_srgb8, to_rgba8};
use crate::PhysicalRect;

const SHAPE_CACHE_CAP: usize = 512;
/// Hard ceiling on shaped paragraphs, whatever the view asks for.
const SHAPE_CACHE_MAX: usize = 32_768;
/// Frames a paragraph stays un-evictable after it was last drawn. Two, so a
/// painter alternating between two windows holds both of their text.
const PIN_FRAMES: u64 = 2;
/// How far along the LRU order one insert looks for a paragraph nothing wants.
/// The front is the oldest; if the oldest this many are all still on screen,
/// so is everything behind them.
const EVICT_PROBES: usize = 64;
/// What a caller passes when it cannot say whether the primitive changed.
///
/// Such a frame assembles the shape key and hashes the paragraph, which is
/// what every frame used to do. It never matches a retained entry, including
/// one built by another untracked call.
pub(super) const UNTRACKED_REVISION: u64 = u64::MAX;

/// What a presentation row is derived from: the paint transform, its
/// perspective, the fragment clip and the device scale.
type PresentationInputs = ([f32; 6], [f32; 2], clip::FragmentClip, u32);

/// Frames between retirement sweeps, and how long an entry survives without
/// being drawn. A tab switch that flips back and forth must not pay for
/// either direction.
const RETIRE_INTERVAL: u64 = 64;
const RETIRE_AFTER_FRAMES: u64 = 240;

/// The raster factor a transform's scale earns a paragraph: half-octave steps
/// from 1/4 (step -4) to 4 (step 4), step 0 the device scale. Shrinking earns
/// smaller bitmaps as magnifying earns larger ones, so a glyph's edge stays
/// about one device pixel either way; bilinear sampling a bitmap twice its
/// quad's size would halve the edge and alias thin stems. A press or a
/// zoom-in from 0.9 stays within step 0.
fn raster_factor(step: i8) -> f32 {
    2f32.powf(f32::from(step) * 0.5)
}
/// How far past the midpoint between two steps a magnification must travel
/// before the step an entry holds gives way, in steps. A zoom that settles
/// near a boundary does not flip between two bitmaps as it jitters, and a zoom
/// animation rasterizes each step it passes once rather than once per frame.
const RASTER_HYSTERESIS: f32 = 0.25;
/// Largest em a raster step may ask for, in raster px. Past this a magnified
/// glyph is not worth an atlas region the size of a tile.
const MAX_RASTER_EM_PX: f32 = 256.0;

/// The raster step for text under `affine`, given the step it held.
///
/// The device scale is not an input: it is the DPI policy, applied whatever
/// this says. This is the *scene* scale policy — separate, bucketed and
/// hysteretic, so a transform animation cannot open a raster size per frame.
fn raster_step(affine: [f32; 6], held: Option<i8>, em_px: f32) -> i8 {
    let [a, b, c, d, _, _] = affine;
    let magnification = (a * a + b * b).max(c * c + d * d).sqrt();
    let ceiling = if em_px > 0.0 && em_px.is_finite() {
        ((2.0 * (MAX_RASTER_EM_PX / em_px).log2()).floor()).clamp(0.0, 4.0) as i8
    } else {
        0
    };
    if !magnification.is_finite() || magnification <= 0.0 {
        return held.unwrap_or(0).min(ceiling);
    }
    let wanted = 2.0 * magnification.log2();
    let step = match held {
        Some(held) if (wanted - f32::from(held)).abs() <= 0.5 + RASTER_HYSTERESIS => held,
        _ => wanted.round().clamp(-4.0, 4.0) as i8,
    };
    step.min(ceiling)
}

/// The counters Issue #97 asks the text path to answer with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextGlyphCounters {
    pub glyph_resolve_requests: u64,
    pub glyph_rasterized: u64,
    pub glyph_raster_cache_hit: u64,
    pub glyph_raster_cache_miss: u64,
    pub glyph_raster_cache_evict: u64,
    pub glyph_raster_cache_bytes: u64,
    pub glyph_atlas_hit: u64,
    pub glyph_atlas_miss: u64,
    pub glyph_atlas_evict: u64,
    pub glyph_atlas_pages: u32,
    pub glyph_atlas_bytes: u64,
    /// Live glyph area per mille of page area; the complement is gutters plus
    /// fragmentation.
    pub glyph_atlas_occupancy_permille: u32,
    pub glyph_upload_regions: u64,
    pub glyph_upload_bytes: u64,
    pub atlas_relocations: u64,
    pub atlas_stale_handle_rejects: u64,
    pub text_pipeline_draws: u64,
    /// Index slots those draws span: the quads the vertex stage runs, gap and
    /// slack slots included, which cull without reading an instance but are
    /// still run.
    pub text_index_slots_drawn: u64,
    /// Retained `TextGpuEntry` count and its lifecycle.
    pub text_gpu_entries_active: u64,
    pub text_gpu_entries_created: u64,
    pub text_gpu_entries_destroyed: u64,
    /// Entries answered from the store rather than resolved again.
    pub text_gpu_entries_reused: u64,
    /// Live instances the entries hold.
    pub text_gpu_entry_glyphs: u64,
    /// Entries whose glyphs had to be resolved from a shaped paragraph.
    pub text_instance_rebuilds: u64,
    /// Entries repaired in place: an atlas relocation or a new run index,
    /// never a reshape or a rasterize.
    pub text_instance_patches: u64,
    pub text_instance_upload_bytes: u64,
    /// The draw-order index table: four bytes per slot of an entry whose
    /// range was placed or whose block moved. A frame whose draw order and
    /// blocks stood still writes none.
    pub text_index_upload_bytes: u64,
    /// The run and presentation tables, which is where a move, a fade or a
    /// recolor lands instead of in the instances.
    pub text_presentation_upload_bytes: u64,
    pub text_prepare_nodes_considered: u64,
    /// Nodes that reached a draw without resolving a glyph.
    pub text_prepare_nodes_skipped: u64,
    /// Nodes that could not reach a pixel, so they cost no entry and no draw.
    pub text_prepare_nodes_culled: u64,
    /// Paragraphs resolved straight from the Runtime's retained layout.
    pub text_retained_layouts_drawn: u64,
}

struct ShapeEntry {
    key: ShapeKey,
    layout: Arc<TextLayout>,
    /// Frame this paragraph was last asked for.
    last_used: u64,
}

/// Laid-out paragraphs keyed by [`ShapeKeyRef::hash64`].
///
/// A handle table, not a second layout authority: the values are `Arc`s to
/// layouts the engine's own cache produced, so what this bounds is how many
/// the painter keeps *reachable*, not how many exist.
///
/// The map is keyed by the hash rather than by an owned key so a repaint of
/// unchanged text looks up without copying the string, the family name, or the
/// rich-span list. The stored key still decides the hit, so a hash collision
/// between two different texts reshapes instead of painting the wrong glyphs.
#[derive(Default)]
struct ShapeCache {
    /// Keyed by the shape hash, hashed by [`nana_ui_runtime::IdHasher`]:
    /// running SipHash over a word that is already a hash is work for nothing.
    entries: HashMap<u64, ShapeEntry, nana_ui_runtime::BuildIdHasher>,
    order: VecDeque<u64>,
    frame: u64,
    hits: usize,
    misses: usize,
    evictions: usize,
}

impl ShapeCache {
    fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    fn get(&mut self, hash: u64, key: &ShapeKeyRef<'_>) -> Option<&Arc<TextLayout>> {
        let frame = self.frame;
        match self.entries.get_mut(&hash) {
            Some(entry) if entry.key.matches(key) => {
                entry.last_used = frame;
                self.hits += 1;
                Some(&entry.layout)
            }
            _ => {
                self.misses += 1;
                None
            }
        }
    }

    /// Whether the cache still holds `hash`, counted and aged as a hit.
    ///
    /// The caller that uses this already knows the paragraph is the one it
    /// resolved from — the primitive has not been rewritten since — so the key
    /// comparison would only re-derive an answer it has.
    fn holds(&mut self, hash: u64) -> bool {
        let frame = self.frame;
        match self.entries.get_mut(&hash) {
            Some(entry) => {
                entry.last_used = frame;
                self.hits += 1;
                true
            }
            None => {
                self.misses += 1;
                false
            }
        }
    }

    fn layout(&self, hash: u64) -> Option<&Arc<TextLayout>> {
        self.entries.get(&hash).map(|entry| &entry.layout)
    }

    /// Drop every shaped paragraph. For the case where they are not merely
    /// cold but no longer meaningful, i.e. the face set changed under them.
    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Store a shaped paragraph, evicting the coldest paragraph nothing on
    /// screen still wants.
    ///
    /// A paragraph a recent frame asked for is never the victim. A view with
    /// more distinct strings on it than [`SHAPE_CACHE_CAP`] would otherwise
    /// evict the row it is about to draw to make room for the next one, and
    /// reshape every one of them on every frame — the cache would turn a
    /// steady frame into the most expensive kind there is. So the nominal
    /// capacity is a floor the cache shrinks back to once the view does, not a
    /// ceiling it enforces against the view in front of it.
    ///
    /// "Recent" rather than "this frame" because two windows on one painter
    /// take turns: what window A drew last frame is still on screen while
    /// window B is drawing, and the cache has to hold both.
    fn insert(&mut self, hash: u64, key: ShapeKey, layout: Arc<TextLayout>) {
        let frame = self.frame;
        let pinned = frame.saturating_sub(PIN_FRAMES);
        let mut probes = 0;
        while self.entries.len() >= SHAPE_CACHE_CAP && probes < EVICT_PROBES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            probes += 1;
            match self.entries.get(&oldest) {
                Some(entry) if entry.last_used >= pinned => {
                    // Still on a screen. Put it back and look further along.
                    self.order.push_back(oldest);
                }
                Some(_) => {
                    self.entries.remove(&oldest);
                    self.evictions += 1;
                    probes = 0;
                }
                None => probes = 0,
            }
        }
        // A hard ceiling, in case a single view really does hold this much
        // text: the alternative is a cache that remembers every paragraph a
        // session ever drew.
        while self.entries.len() >= SHAPE_CACHE_MAX {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if self.entries.remove(&oldest).is_some() {
                self.evictions += 1;
            }
        }
        if self
            .entries
            .insert(
                hash,
                ShapeEntry {
                    key,
                    layout,
                    last_used: frame,
                },
            )
            .is_none()
        {
            self.order.push_back(hash);
        }
    }
}

/// Marks a paragraph identity as a Runtime layout handle rather than a hash of
/// this painter's own shape key.
///
/// The two share one `u64` — [`TextGpuEntry::layout`] — and a value from one
/// must never read as a value from the other, because what it gates is whether
/// a set of resolved glyphs may be drawn again. The hash space is halved
/// instead: handles set this bit, [`ShapeKeyRef::hash64`] clears it.
const PARAGRAPH_IS_RETAINED: u64 = 1 << 63;

/// One Runtime layout handle as a paragraph identity.
///
/// Exact, not hashed: the handle is two `u32`s and a generational slot is
/// never reissued at the same generation, so packing them *is* the identity.
/// The index is masked to 31 bits, which a store of two billion live layouts
/// would be needed to reach.
fn retained_paragraph_id(id: nana_text::TextLayoutId) -> u64 {
    PARAGRAPH_IS_RETAINED | (u64::from(id.index() & 0x7fff_ffff) << 32) | u64::from(id.generation())
}

/// FNV-1a over the shape key.
///
/// Not a general-purpose hasher and deliberately not the default one: this
/// runs once per text node per frame over the whole string, and it is the
/// single largest item left in the text path. What guards against a collision
/// is [`ShapeKey::matches`], which decides the hit — a collision costs a
/// reshape, never the wrong glyphs.
#[derive(Default)]
struct ShapeHasher(u64);

impl Hasher for ShapeHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut state = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for byte in bytes {
            state ^= u64::from(*byte);
            state = state.wrapping_mul(0x1000_0000_01b3);
        }
        self.0 = state;
    }
}

/// Everything that determines the laid-out paragraph.
///
/// Position and color are not here. Since #99 that includes *rich span*
/// colors: `nana-text` lays text out without knowing what paints it, so two
/// spellings of the same string in different colors are one layout and the
/// colors are resolved onto glyphs afterwards.
struct ShapeKey {
    content: String,
    family: Option<String>,
    weight: Option<u16>,
    font_size_bits: u32,
    line_height_bits: u32,
    wrap: bool,
    wrap_break: nana_ui_core::TextWrapBreak,
    italic: bool,
    ellipsis: bool,
    max_lines: Option<u16>,
    letter_spacing_bits: u32,
    word_break: u8,
    line_break: u8,
    kerning: u8,
    features: Vec<nana_ui_core::FontFeatureSetting>,
    variations: Vec<nana_ui_core::FontVariationSetting>,
    width_bits: u32,
    height_bits: u32,
    align: u8,
    direction: u8,
    writing_mode: u8,
    text_orientation: u8,
    preserve_lines: bool,
    font_features: Vec<nana_ui_core::FontFeatureSetting>,
}

/// Borrowed form of [`ShapeKey`] built straight from the scene primitive.
///
/// Hashing and comparison live here so a frame can answer "already shaped?"
/// without owning any of it.
struct ShapeKeyRef<'a> {
    content: &'a str,
    family: Option<&'a str>,
    weight: Option<u16>,
    font_size_bits: u32,
    line_height_bits: u32,
    wrap: bool,
    wrap_break: nana_ui_core::TextWrapBreak,
    italic: bool,
    ellipsis: bool,
    max_lines: Option<u16>,
    letter_spacing_bits: u32,
    word_break: u8,
    line_break: u8,
    kerning: u8,
    features: &'a [nana_ui_core::FontFeatureSetting],
    variations: &'a [nana_ui_core::FontVariationSetting],
    width_bits: u32,
    height_bits: u32,
    align: u8,
    direction: u8,
    writing_mode: u8,
    text_orientation: u8,
    preserve_lines: bool,
    font_features: &'a [nana_ui_core::FontFeatureSetting],
}

impl ShapeKeyRef<'_> {
    fn hash64(&self) -> u64 {
        let mut hasher = ShapeHasher::default();
        self.content.hash(&mut hasher);
        self.family.hash(&mut hasher);
        self.weight.hash(&mut hasher);
        self.font_size_bits.hash(&mut hasher);
        self.line_height_bits.hash(&mut hasher);
        self.wrap.hash(&mut hasher);
        self.wrap_break.hash(&mut hasher);
        self.italic.hash(&mut hasher);
        self.ellipsis.hash(&mut hasher);
        self.max_lines.hash(&mut hasher);
        self.letter_spacing_bits.hash(&mut hasher);
        self.word_break.hash(&mut hasher);
        self.line_break.hash(&mut hasher);
        self.kerning.hash(&mut hasher);
        self.features.hash(&mut hasher);
        self.variations.hash(&mut hasher);
        self.width_bits.hash(&mut hasher);
        self.height_bits.hash(&mut hasher);
        self.align.hash(&mut hasher);
        self.font_features.hash(&mut hasher);
        self.direction.hash(&mut hasher);
        self.writing_mode.hash(&mut hasher);
        self.text_orientation.hash(&mut hasher);
        self.preserve_lines.hash(&mut hasher);
        // The top bit belongs to [`PARAGRAPH_IS_RETAINED`], so a hash can
        // never be mistaken for a Runtime handle. What this costs is one bit
        // of a hash whose collisions are already caught by
        // [`ShapeKey::matches`] and cost a relayout, never wrong glyphs.
        hasher.finish() & !PARAGRAPH_IS_RETAINED
    }

    fn to_owned_key(&self) -> ShapeKey {
        ShapeKey {
            content: self.content.to_owned(),
            family: self.family.map(str::to_owned),
            weight: self.weight,
            font_size_bits: self.font_size_bits,
            line_height_bits: self.line_height_bits,
            wrap: self.wrap,
            wrap_break: self.wrap_break,
            italic: self.italic,
            ellipsis: self.ellipsis,
            max_lines: self.max_lines,
            letter_spacing_bits: self.letter_spacing_bits,
            word_break: self.word_break,
            line_break: self.line_break,
            kerning: self.kerning,
            features: self.features.to_vec(),
            variations: self.variations.to_vec(),
            width_bits: self.width_bits,
            height_bits: self.height_bits,
            align: self.align,
            font_features: self.font_features.to_vec(),
            direction: self.direction,
            writing_mode: self.writing_mode,
            text_orientation: self.text_orientation,
            preserve_lines: self.preserve_lines,
        }
    }
}

impl ShapeKey {
    fn matches(&self, other: &ShapeKeyRef<'_>) -> bool {
        self.content == other.content
            && self.family.as_deref() == other.family
            && self.weight == other.weight
            && self.font_size_bits == other.font_size_bits
            && self.line_height_bits == other.line_height_bits
            && self.wrap == other.wrap
            && self.wrap_break == other.wrap_break
            && self.italic == other.italic
            && self.ellipsis == other.ellipsis
            && self.max_lines == other.max_lines
            && self.letter_spacing_bits == other.letter_spacing_bits
            && self.word_break == other.word_break
            && self.line_break == other.line_break
            && self.kerning == other.kerning
            && self.features == other.features
            && self.variations == other.variations
            && self.width_bits == other.width_bits
            && self.height_bits == other.height_bits
            && self.align == other.align
            && self.font_features == other.font_features
            && self.direction == other.direction
            && self.writing_mode == other.writing_mode
            && self.text_orientation == other.text_orientation
            && self.preserve_lines == other.preserve_lines
    }
}

/// How a paragraph reaches the screen.
///
/// Deliberately everything the retained instances do *not* carry: where the
/// text sits, what color it paints, how opaque it is and what transform it is
/// under. An animation that only touches these writes one 48-byte row.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RunPresentation {
    /// Whole physical pixels the entry's instances are relative to.
    origin: [f32; 2],
    /// Linear RGB with its own alpha; opacity is applied separately so a fade
    /// never has to be folded into a color a glyph was resolved with.
    color: [f32; 4],
    opacity: f32,
    flags: u32,
    presentation: u32,
    /// Physical px per logical px the entry's instances are in.
    raster: f32,
}

/// One text draw command's head, or one run folded into an earlier one.
struct TextRun {
    entry: u32,
    presentation: RunPresentation,
    /// Next run in the same draw command. `NO_RUN` ends the chain.
    next: u32,
    /// Chain tail, so folding another run in is O(1).
    last: u32,
    /// Set once this run belongs to an earlier run's command.
    folded: bool,
    /// Filled by [`TextPipeline::flush_runs`] for the chain head.
    segments: Range<u32>,
}

const NO_RUN: u32 = u32::MAX;

pub(super) struct PreparedText {
    pub index: usize,
    /// Local-space rectangle the glyphs can cover, `bounds` overflow included.
    pub ink: LogicalRect,
}

/// Per-target text state: the arena this target's glyphs are drawn from, the
/// entries that own ranges in it, and the draw commands that name them.
///
/// The atlas, the raster cache and the shaped paragraphs are **not** here.
/// They belong to the device context, so a second window on the same device
/// reuses every glyph the first one faulted in rather than filling a second
/// atlas with the same shell chrome.
pub(super) struct TextPipelineTarget {
    gpu: TextTargetGpu,
    entries: EntryStore,
    runs: Vec<TextRun>,
    live_runs: usize,
    flushed: usize,
    segments: Vec<DrawSegment>,
    /// Indexed by [`entry::TextGpuEntry::slot`], not by draw order, and kept
    /// between frames: a paragraph that did not move, recolor or fade leaves
    /// its row alone even when the frame around it changed completely.
    run_table: Vec<TextRunGpu>,
    /// Rows that differ from what the GPU holds, as one span. Two rows far
    /// apart cost the rows between them, which is cheaper than a write per row
    /// and far cheaper than comparing the whole table.
    run_dirty: Option<Range<u32>>,
    run_slots: RunSlots,
    presentations: Vec<TextPresentationGpu>,
    uploaded_presentations: Vec<TextPresentationGpu>,
    presentation_index: HashMap<[u32; 40], u32>,
    /// The presentation the label before this one asked for, by the values it
    /// was derived from. See [`TextPipeline::presentation_index`].
    last_presentation: Option<(PresentationInputs, u32)>,
    /// Where each entry's block sits in this target's instance buffer. The
    /// bytes come straight from the entry that owns the block; only the
    /// offsets live here.
    arena: SlotArena,
    /// Where each entry's range sits in the draw-order index table. What
    /// batches is adjacency here, not in the arena.
    order: SlotArena,
    /// Blocks whose arena bytes are no longer what the GPU holds, coalesced
    /// into as few writes as their offsets allow.
    writes: Vec<ArenaWrite>,
    staging: Vec<GlyphInstance>,
    /// Index ranges that were placed or whose block moved, likewise.
    index_writes: Vec<ArenaWrite>,
    index_staging: Vec<u32>,
    /// Scratch for the blocks and ranges a flush is about to place.
    fresh_blocks: Vec<u32>,
    fresh_ranges: Vec<u32>,
    /// Which entry owns the index range starting at each offset, so a range
    /// that grows can find the ones after it.
    order_owners: OrderOwners,
    /// Drawn entries whose block outgrew their range, found by the first walk
    /// so growing them in place does not walk every entry a second time.
    outgrown: Vec<u32>,
    /// Entries this frame leaves undrawn, sorted, because with them the
    /// frame's glyphs would not fit the device's storage binding.
    skipped: Vec<u32>,
    /// Whether the last order repack left gaps, and what has moved in the
    /// order since: ranges placed, and how many of those because their
    /// paragraph grew. What decides whether the next repack leaves gaps.
    order_gapped: bool,
    order_moves: u32,
    growth_moves: u32,
    /// Frames this target drew in a row without a range outgrowing its
    /// place. Past [`SETTLE_FRAMES`] the text has stopped growing, and a
    /// layout with gaps is repacked without them.
    quiet_frames: u32,
    physical_size: [u32; 2],
    frame: u64,
    frame_gpu_allocations: usize,
    /// Everything but the entry lifecycle, which the store counts itself.
    counters: TargetCounters,
    /// What this target's instance and index buffers hold, rebuilt from the
    /// writes the frame sent them. See [`TextPipeline::audit_draw_order`].
    #[cfg(test)]
    shadow: GpuShadow,
}

/// A CPU copy of one target's two GPU buffers, kept only in tests.
#[cfg(test)]
#[derive(Default)]
struct GpuShadow {
    instances: Vec<GlyphInstance>,
    indices: Vec<u32>,
}

/// The monotonic counters one render target accumulates.
///
/// Kept per target because that is where the work happens, and summed across
/// every live target — plus the ones already closed — when read, so a window
/// that is not the one painted last still shows up in the totals and closing
/// it does not make them run backwards.
#[derive(Clone, Copy, Default)]
struct TargetCounters {
    instance_rebuilds: u64,
    instance_patches: u64,
    instance_upload_bytes: u64,
    index_upload_bytes: u64,
    presentation_upload_bytes: u64,
    nodes_considered: u64,
    nodes_skipped: u64,
    nodes_culled: u64,
    entries_created: u64,
    entries_destroyed: u64,
    entries_reused: u64,
}

impl TargetCounters {
    fn add(&mut self, other: Self) {
        self.instance_rebuilds += other.instance_rebuilds;
        self.instance_patches += other.instance_patches;
        self.instance_upload_bytes += other.instance_upload_bytes;
        self.index_upload_bytes += other.index_upload_bytes;
        self.presentation_upload_bytes += other.presentation_upload_bytes;
        self.nodes_considered += other.nodes_considered;
        self.nodes_skipped += other.nodes_skipped;
        self.nodes_culled += other.nodes_culled;
        self.entries_created += other.entries_created;
        self.entries_destroyed += other.entries_destroyed;
        self.entries_reused += other.entries_reused;
    }
}

impl TextPipelineTarget {
    fn counters(&self) -> TargetCounters {
        let entries = self.entries.counters();
        TargetCounters {
            entries_created: entries.created,
            entries_destroyed: entries.destroyed,
            entries_reused: entries.reused,
            ..self.counters
        }
    }

    /// Forget every range of the draw order, and with them the gaps and what
    /// moved since the last repack: the next layout has not left anything
    /// room, and nothing placed before it says whether text is growing.
    fn forget_order(&mut self) {
        self.order.reset();
        self.entries.invalidate_order();
        self.order_owners.clear(None);
        self.order_gapped = false;
        self.order_moves = 0;
        self.growth_moves = 0;
    }

    fn new(gpu: TextTargetGpu) -> Self {
        Self {
            gpu,
            entries: EntryStore::default(),
            runs: Vec::new(),
            live_runs: 0,
            flushed: 0,
            segments: Vec::new(),
            run_table: Vec::new(),
            run_dirty: None,
            run_slots: RunSlots::default(),
            presentations: Vec::new(),
            uploaded_presentations: Vec::new(),
            presentation_index: HashMap::new(),
            last_presentation: None,
            arena: SlotArena::default(),
            order: SlotArena::default(),
            writes: Vec::new(),
            staging: Vec::new(),
            index_writes: Vec::new(),
            index_staging: Vec::new(),
            fresh_blocks: Vec::new(),
            fresh_ranges: Vec::new(),
            order_owners: OrderOwners::default(),
            outgrown: Vec::new(),
            skipped: Vec::new(),
            order_gapped: false,
            order_moves: 0,
            growth_moves: 0,
            quiet_frames: 0,
            physical_size: [0; 2],
            frame: 0,
            frame_gpu_allocations: 0,
            counters: TargetCounters::default(),
            #[cfg(test)]
            shadow: GpuShadow::default(),
        }
    }
}

/// The order of a panel's color subpixels, left to right.
///
/// [`SubpixelOrder::system`] reports it on Windows. A host that paints onto
/// a surface of its own passes it to
/// [`SceneWgpuPainter::set_subpixel_text`](crate::SceneWgpuPainter::set_subpixel_text).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubpixelOrder {
    Rgb,
    Bgr,
}

impl SubpixelOrder {
    /// The order the system draws its own text with, when it draws it with
    /// subpixel coverage: ClearType's panel order on Windows while the user
    /// has it on, `None` everywhere else. Read once per process.
    pub fn system() -> Option<Self> {
        #[cfg(windows)]
        {
            static ORDER: std::sync::OnceLock<Option<SubpixelOrder>> = std::sync::OnceLock::new();
            *ORDER.get_or_init(raster_dwrite::system_subpixel_order)
        }
        #[cfg(not(windows))]
        None
    }
}

pub(super) struct TextPipeline {
    /// The `nana-text` engine this painter lays paragraphs out through, and
    /// the one whose faces its rasterizer scales. One per process, so a
    /// paragraph shaped for one window is already shaped for the next.
    engine: nana_text::SharedTextEngine,
    rasterizer: PlatformRasterizer,
    raster: GlyphRasterCache,
    atlas: GlyphAtlasManager,
    uploads: GlyphUploadQueue,
    gpu: TextGpu,
    /// Shaped paragraphs reused across frames. Shaping is the dominant CPU
    /// cost of a text-heavy frame and identical text+style+box repeats on
    /// every repaint (scroll, hover, unrelated animations), so the shaped
    /// `Buffer` is cached and only glyph placement is redone per frame.
    shape_cache: ShapeCache,
    /// Resolver scratch, reused so a paragraph's runs cost no allocation.
    resolved: NanaGlyphBuffer,
    target: TextPipelineTarget,
    /// The font-set generation this painter's caches were filled at. A
    /// `@font-face` registration reissues faces, so both the shaped paragraphs
    /// and the glyph bitmaps stop meaning what they meant.
    font_generation: u64,
    resolve_requests: u64,
    /// Paragraphs drawn straight from the Runtime's retained layout, i.e. not
    /// laid out a second time here. The counter exists so "measurement and
    /// paint are the same paragraph" is a number a gate can read rather than a
    /// claim in a comment.
    retained_layouts_drawn: u64,
    draws: Cell<u64>,
    slots_drawn: Cell<u64>,
    /// What closed render targets did before they closed.
    closed: TargetCounters,
    /// What upright text on an opaque backdrop is resolved as: `Mask`
    /// unless the host turned subpixel text on and the device can draw it.
    subpixel: GlyphRenderMode,
}

impl TextPipeline {
    #[cfg(test)]
    pub(super) fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        Self::with_atlas_limits_policy(device, format, GlyphAtlasLimits::default(), None)
    }

    pub(super) fn new_with_policy(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        policy: &nana_gpu::GpuDeviceState,
    ) -> Self {
        Self::with_atlas_limits_policy(device, format, GlyphAtlasLimits::default(), Some(policy))
    }

    #[cfg(test)]
    fn with_atlas_limits(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        limits: GlyphAtlasLimits,
    ) -> Self {
        Self::with_atlas_limits_policy(device, format, limits, None)
    }

    fn with_atlas_limits_policy(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        limits: GlyphAtlasLimits,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Self {
        let raster = GlyphRasterCache::default();
        let atlas = GlyphAtlasManager::new(device, raster.generation(), limits, policy);
        let gpu = TextGpu::new_with_policy(device, format, &atlas, policy);
        let target = TextPipelineTarget::new(gpu.new_target(device));
        Self {
            engine: crate::text_engine::nana_text_engine(),
            rasterizer: PlatformRasterizer::new(crate::text_engine::nana_text_engine()),
            raster,
            atlas,
            uploads: GlyphUploadQueue::default(),
            gpu,
            shape_cache: ShapeCache::default(),
            resolved: NanaGlyphBuffer::default(),
            target,
            font_generation: crate::text_engine::engine_font_generation(),
            resolve_requests: 0,
            retained_layouts_drawn: 0,
            draws: Cell::new(0),
            slots_drawn: Cell::new(0),
            closed: TargetCounters::default(),
            subpixel: GlyphRenderMode::Mask,
        }
    }

    /// Resolve upright text on an opaque backdrop with per-subpixel coverage
    /// for a panel of this order, or stop. A device that cannot blend two
    /// sources keeps grayscale whatever is asked. Returns whether the mode
    /// changed.
    ///
    /// Entries resolved under the other mode fail their validity check and
    /// are resolved again on their next frame, so the switch needs no sweep.
    pub(super) fn set_subpixel(
        &mut self,
        device: &wgpu::Device,
        order: Option<SubpixelOrder>,
    ) -> bool {
        let mode = match order {
            Some(_) if !TextGpu::supports_subpixel(device) => GlyphRenderMode::Mask,
            Some(SubpixelOrder::Rgb) => GlyphRenderMode::SubpixelRgb,
            Some(SubpixelOrder::Bgr) => GlyphRenderMode::SubpixelBgr,
            None => GlyphRenderMode::Mask,
        };
        if mode == self.subpixel {
            return false;
        }
        self.gpu.set_subpixel(device, mode != GlyphRenderMode::Mask);
        self.subpixel = mode;
        true
    }

    pub(super) fn begin_frame(&mut self, physical_size: [u32; 2]) {
        let generation = crate::text_engine::engine_font_generation();
        if generation != self.font_generation {
            // Faces were added, replaced or removed. Shaped paragraphs named
            // the old face set and glyph bitmaps were scaled from it, so both
            // are dropped rather than left to be keyed around — and with them
            // every entry, whose instances were resolved through those ids.
            self.font_generation = generation;
            self.shape_cache.clear();
            self.raster.invalidate();
            self.drop_every_entry();
        }
        // Evicted variation ids are never reissued, so what is keyed by one
        // goes cold instead of wrong, and ages out of the raster cache.
        let _ = self.rasterizer.begin_frame();
        self.atlas.begin_frame(self.raster.generation());
        self.shape_cache.begin_frame();
        let target = &mut self.target;
        target.frame = target.frame.wrapping_add(1);
        target.live_runs = 0;
        target.flushed = 0;
        target.segments.clear();
        // What the GPU holds is last frame's table, so it becomes the
        // comparison baseline by swapping rather than by copying.
        std::mem::swap(
            &mut target.presentations,
            &mut target.uploaded_presentations,
        );
        target.presentations.clear();
        target.presentation_index.clear();
        target.last_presentation = None;
        target.writes.clear();
        target.staging.clear();
        target.index_writes.clear();
        target.index_staging.clear();
        target.frame_gpu_allocations = 0;
        target.physical_size = physical_size;
        // Entries a shell stopped drawing — a closed panel, a scrolled-away
        // row — keep their block and their claim on the glyphs in it. Retiring
        // them on a schedule rather than on every frame keeps a tab switch
        // that flips back and forth from paying for either direction.
        if target.frame.is_multiple_of(RETIRE_INTERVAL) {
            let atlas = &mut self.atlas;
            let horizon = target.frame.saturating_sub(RETIRE_AFTER_FRAMES);
            let TextPipelineTarget {
                entries,
                run_slots,
                arena,
                order,
                order_owners,
                ..
            } = target;
            entries.retire(
                horizon,
                |handle| atlas.release(handle),
                |slot| run_slots.release(slot),
                |generation, offset, capacity| arena.release(generation, offset, capacity),
                |generation, offset, capacity| {
                    if generation == order.generation() {
                        order_owners.remove(offset);
                    }
                    order.release(generation, offset, capacity)
                },
            );
        }
    }

    /// Drop every retained entry and everything it held.
    ///
    /// The blocks and run rows go back too: without that, the arena and the
    /// run table would grow past every `@font-face` registration a session
    /// sees, because the entries that owned them are gone and can no longer
    /// give them back.
    fn drop_every_entry(&mut self) {
        let atlas = &mut self.atlas;
        self.target.entries.clear(|handle| atlas.release(handle));
        self.target.arena.reset();
        self.target.forget_order();
        self.target.run_slots.reset();
        self.target.run_table.clear();
        self.target.run_dirty = None;
    }

    /// Bumped whenever a placement moved or died. A render target that kept
    /// draw commands from an earlier frame must rebuild them when this
    /// changes: its instances name rectangles that are no longer that glyph's.
    pub(super) fn placement_epoch(&self) -> u64 {
        self.atlas.placement_epoch()
    }

    /// Shape-cache counters for tests: (hits, misses, evictions). None until
    /// consulted, mirroring the Runtime text cache contract.
    pub(super) fn shape_cache_stats(&self) -> (usize, usize, usize) {
        (
            self.shape_cache.hits,
            self.shape_cache.misses,
            self.shape_cache.evictions,
        )
    }

    /// Counters for the target being painted alone. The painter asks
    /// [`Self::glyph_counters_across`] instead; this is what a test that owns
    /// one target reads.
    #[cfg(test)]
    pub(super) fn glyph_counters(&self) -> TextGlyphCounters {
        self.glyph_counters_across(std::iter::empty())
    }

    /// Counters over every render target: the one being painted, `others`,
    /// and the ones already closed. The device-wide half — raster cache,
    /// atlas, uploads — is shared anyway; the retained half lives per target
    /// and is summed, so a second window's frames are not invisible.
    pub(super) fn glyph_counters_across<'a>(
        &'a self,
        others: impl Iterator<Item = &'a TextPipelineTarget>,
    ) -> TextGlyphCounters {
        let raster = self.raster.counters();
        let atlas = self.atlas.counters();
        let uploads = self.uploads.counters();
        let mut targets = self.closed;
        let (mut active, mut glyphs) = (0, 0);
        for target in others.chain(std::iter::once(&self.target)) {
            targets.add(target.counters());
            let entries = target.entries.counters();
            active += entries.active;
            glyphs += entries.glyphs;
        }
        TextGlyphCounters {
            glyph_resolve_requests: self.resolve_requests,
            glyph_rasterized: raster.rasterized,
            glyph_raster_cache_hit: raster.hits,
            glyph_raster_cache_miss: raster.misses,
            glyph_raster_cache_evict: raster.evictions,
            glyph_raster_cache_bytes: self.raster.bytes() as u64,
            glyph_atlas_hit: atlas.hits,
            glyph_atlas_miss: atlas.misses,
            glyph_atlas_evict: atlas.evictions,
            glyph_atlas_pages: atlas.pages,
            glyph_atlas_bytes: atlas.bytes,
            glyph_atlas_occupancy_permille: atlas.occupancy_permille,
            glyph_upload_regions: uploads.regions,
            glyph_upload_bytes: uploads.bytes,
            atlas_relocations: atlas.relocations,
            atlas_stale_handle_rejects: atlas.stale_handle_rejects,
            text_pipeline_draws: self.draws.get(),
            text_index_slots_drawn: self.slots_drawn.get(),
            text_gpu_entries_active: active,
            text_gpu_entries_created: targets.entries_created,
            text_gpu_entries_destroyed: targets.entries_destroyed,
            text_gpu_entries_reused: targets.entries_reused,
            text_gpu_entry_glyphs: glyphs,
            text_instance_rebuilds: targets.instance_rebuilds,
            text_instance_patches: targets.instance_patches,
            text_instance_upload_bytes: targets.instance_upload_bytes,
            text_index_upload_bytes: targets.index_upload_bytes,
            text_presentation_upload_bytes: targets.presentation_upload_bytes,
            text_prepare_nodes_considered: targets.nodes_considered,
            text_prepare_nodes_skipped: targets.nodes_skipped,
            text_prepare_nodes_culled: targets.nodes_culled,
            text_retained_layouts_drawn: self.retained_layouts_drawn,
        }
    }

    /// This target is drawn from last frame's batch: nothing was prepared,
    /// so nothing grew. Counted only when that batch draws text, as a flush
    /// is: a frame without any draws no gap.
    pub(super) fn note_reused_frame(&mut self) {
        if self.target.live_runs > 0 {
            self.target.quiet_frames = self.target.quiet_frames.saturating_add(1);
        }
    }

    /// Whether a flush this frame would give back the order's gaps. A frame
    /// that would reuse last frame's batch rebuilds it instead: an animation
    /// beside a table that stopped changing repaints every frame without
    /// flushing, and would otherwise draw the gaps for as long as it runs.
    ///
    /// Never while the batch draws no text: its flush would return before
    /// repacking anything, and every frame after would rebuild for nothing.
    pub(super) fn order_settle_due(&self) -> bool {
        let target = &self.target;
        target.order_gapped
            && target.live_runs > 0
            && target.quiet_frames.saturating_add(1) >= SETTLE_FRAMES
    }

    /// Mark the order as holding gaps, the way a repack around growing text
    /// leaves it, so a painter test can watch them be given back.
    #[cfg(test)]
    pub(super) fn assume_gapped_order(&mut self) {
        self.target.order_gapped = true;
        self.target.quiet_frames = 0;
    }

    #[cfg(test)]
    pub(super) fn order_gapped(&self) -> bool {
        self.target.order_gapped
    }

    /// GPU allocations this frame's text could not reuse.
    pub(super) fn take_frame_gpu_allocations(&mut self) -> usize {
        std::mem::take(&mut self.target.frame_gpu_allocations)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        bounds: LogicalRect,
        clip: LogicalRect,
        scale_factor: f32,
        content: &str,
        color: Option<[f32; 4]>,
        size: f32,
        weight: Option<u16>,
        family: Option<&str>,
        line_height: Option<LineHeightSpec>,
        wrap: bool,
        wrap_break: nana_ui_core::TextWrapBreak,
        italic: bool,
        ellipsis: bool,
        max_lines: Option<u16>,
        shaping: TextShaping,
        horizontal: TextHorizontalAlignment,
        vertical: TextVerticalAlignment,
        spans: &[SceneTextSpan],
        letter_spacing: f32,
        font_features: &[nana_ui_core::FontFeatureSetting],
        opentype: &SceneTextOpenType,
        affine: [f32; 6],
        persp: [f32; 2],
        fragment_clip: clip::FragmentClip,
        opacity: f32,
        paint_offset: [f32; 2],
        entry_key: EntryKey,
        revision: u64,
        text_layout: Option<&nana_ui_runtime::RetainedTextLayout>,
        // Nothing between this text and the window's own opaque surface:
        // not inside an offscreen group, whose transparent texture would
        // take subpixel coverage as colored alpha.
        opaque_backdrop: bool,
    ) -> Option<PreparedText> {
        if content.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let size = size.max(f32::MIN_POSITIVE);
        let line_height = match line_height {
            Some(LineHeightSpec::Relative(value)) => size * value,
            Some(LineHeightSpec::Absolute(value)) => value,
            None => size * 1.2,
        }
        .max(f32::MIN_POSITIVE);
        // Laid out in **logical** px, which is what makes a layout reusable
        // across device scales and identical to the one Runtime measured the
        // same node with. The device scale enters at resolve time, where it
        // picks the raster size and the sub-pixel bin; advances are linear in
        // the size, so scaling the result is the same text at a different
        // scale, and a DPI change re-rasterizes without laying anything out.
        let box_width = bounds.width.max(0.0);
        let box_height = bounds.height.max(line_height);
        // Opacity rides on the run, not on the color the glyphs were
        // resolved with: a fade must not be a reason to reshape rich text or
        // to rebuild a single instance.
        let default_color = color.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        // Built before the retained lookup because its fingerprint is half of
        // what decides that lookup: the layout is not keyed by colour, so for
        // rich text this is the only thing that can tell one paint from
        // another. Solid text costs an empty `Vec` and a zero.
        let colors = SpanColors::new(content, spans, default_color);
        // Every path shapes with the full OpenType machinery. The scene still
        // carries the old `Auto | Advanced` distinction; `nana-text` has no
        // reduced mode to select, and a reduced one changed advances enough to
        // wrap text that fit the Runtime content box.
        let (TextShaping::Auto | TextShaping::Advanced) = shaping;
        let align = match horizontal {
            TextHorizontalAlignment::Start => TextAlignSpec::Start,
            TextHorizontalAlignment::Center => TextAlignSpec::Center,
            TextHorizontalAlignment::End => TextAlignSpec::End,
        };
        // The layout Runtime *measured* this node with, when it measured into
        // the box this paints into.
        //
        // Taking it is the difference between measurement and paint being the
        // same paragraph and being two paragraphs that agree by construction:
        // every constraint the Runtime decided — wrap, ellipsis, max-lines,
        // `white-space`, direction — arrives already decided instead of being
        // rebuilt from the scene's description of it. It also costs one float
        // compare where laying out again costs a hash of the whole string.
        //
        // The one thing that has to hold is the alignment box: the glyphs are
        // placed inside the line budget — `max_width_px`, or `max_height_px`
        // for vertical text — and this paints them at the box's start edge,
        // so a node whose paint box is not the box it was measured into (a
        // switch with a trailing control, a list item whose content geometry
        // overrides the text box) falls back to laying out for itself rather
        // than drawing centred text off-centre.
        let retained_paragraph = text_layout
            .filter(|retained| {
                let layout = &retained.layout;
                let (budget, line_box) = if layout.is_vertical() {
                    (layout.constraints.max_height_px, bounds.height.max(0.0))
                } else {
                    (layout.constraints.max_width_px, box_width)
                };
                budget == Some(line_box)
            })
            .map(|retained| Arc::clone(&retained.layout));
        // Nothing the shape key is made of can have changed: the scene has not
        // rewritten this primitive since these glyphs were resolved, and
        // neither the device scale nor the font set has moved. Assembling the
        // key and hashing the paragraph would only prove that again, once per
        // label per frame.
        //
        // Rich text is included: since #99 the span colors are not part of the
        // layout key, and what a span paints is baked into the instances this
        // entry already holds under the same primitive revision.
        //
        // Text under a scaling transform is resolved at the scale it is seen,
        // in steps — see [`raster_step`] — so the scale an entry is compared
        // at is the raster one. A translation never scales, and costs no
        // lookup to find that out.
        let translation = clip::is_translation_projective(affine, persp);
        let step = if translation {
            0
        } else {
            let held = self
                .target
                .entries
                .lookup(entry_key)
                .and_then(|id| self.target.entries.get(id))
                .map(|entry| entry.raster_step);
            raster_step(affine, held, size * scale)
        };
        let raster = scale * raster_factor(step);
        let retained = self
            .target
            .entries
            .lookup(entry_key)
            .and_then(|id| Some((id, self.target.entries.get(id)?)))
            .filter(|(_, entry)| {
                !entry.damaged
                    && revision != UNTRACKED_REVISION
                    && entry.revision == revision
                    && entry.colors == colors.fingerprint
                    && entry.scale_bits == raster.to_bits()
                    && entry.font_generation == self.font_generation
            })
            .map(|(id, entry)| (id, entry.layout, entry.measured))
            // A paragraph the Runtime retains is kept alive by the scene, so
            // only a painter-owned one has to still be in the cache.
            .filter(|(_, hash, _)| {
                *hash & PARAGRAPH_IS_RETAINED != 0 || self.shape_cache.holds(*hash)
            });
        let hash = match (retained, &retained_paragraph) {
            (Some((_, hash, _)), _) => hash,
            // The Runtime's handle *is* the identity: two u32s of a
            // generational slot, never reissued at the same generation. No
            // string is read and nothing is hashed.
            (None, Some(paragraph)) => {
                self.retained_layouts_drawn += 1;
                retained_paragraph_id(paragraph.id)
            }
            (None, None) => {
                // Width, height and requested ellipsis uniquely determine the
                // result; the cache lookup happens before layout so a repaint
                // of unchanged text never reaches the engine.
                let key = ShapeKeyRef {
                    content,
                    family,
                    weight,
                    font_size_bits: size.to_bits(),
                    line_height_bits: line_height.to_bits(),
                    wrap,
                    wrap_break,
                    italic,
                    ellipsis,
                    max_lines,
                    letter_spacing_bits: letter_spacing.to_bits(),
                    word_break: opentype_disc(opentype.word_break),
                    line_break: opentype_line_disc(opentype.line_break),
                    kerning: opentype_kern_disc(opentype.kerning),
                    features: &opentype.features,
                    variations: &opentype.variations,
                    width_bits: box_width.to_bits(),
                    height_bits: box_height.to_bits(),
                    align: match horizontal {
                        TextHorizontalAlignment::Start => 0,
                        TextHorizontalAlignment::Center => 1,
                        TextHorizontalAlignment::End => 2,
                    },
                    direction: if opentype.direction.is_rtl() { 1 } else { 0 },
                    writing_mode: match opentype.writing_mode {
                        nana_ui_core::WritingModeSpec::HorizontalTb => 0,
                        nana_ui_core::WritingModeSpec::VerticalRl => 1,
                        nana_ui_core::WritingModeSpec::VerticalLr => 2,
                    },
                    text_orientation: match opentype.text_orientation {
                        nana_ui_core::TextOrientationSpec::Mixed => 0,
                        nana_ui_core::TextOrientationSpec::Upright => 1,
                        nana_ui_core::TextOrientationSpec::Sideways => 2,
                    },
                    preserve_lines: opentype.preserve_lines,
                    font_features,
                };
                let hash = key.hash64();
                if self.shape_cache.get(hash, &key).is_none() {
                    let layout = self.lay_out(
                        content,
                        family,
                        weight,
                        size,
                        line_height,
                        letter_spacing,
                        italic,
                        wrap,
                        wrap_break,
                        ellipsis,
                        max_lines,
                        box_width,
                        box_height,
                        align,
                        opentype,
                    );
                    self.shape_cache.insert(hash, key.to_owned_key(), layout);
                }
                hash
            }
        };
        // The widest line and the laid-out height are the layout's, in logical
        // px, and the layout is the one this entry was built from, so a steady
        // frame does not walk its lines again to find that out.
        let (measured_width, laid_out_height) = match (retained, &retained_paragraph) {
            (Some((_, _, measured)), _) => (measured[0], measured[1]),
            (None, Some(paragraph)) => measure(paragraph),
            (None, None) => {
                let layout = self.shape_cache.layout(hash).expect("laid out above");
                measure(layout)
            }
        };
        // Vertical text is aligned inside its own layout, down the column's
        // line budget, so the box origin is where it starts. A `vertical-rl`
        // paragraph is drawn right-aligned against its own column stack (see
        // `build_entry`), and the box adds whatever it has beyond that stack
        // on the left.
        let writing = nana_ui_core::WritingContext::new(opentype.writing_mode, opentype.direction);
        let mut aligned = if !writing.is_vertical() {
            text_box_origin(bounds, vertical, laid_out_height)
        } else if writing.block_reversed() {
            [bounds.x + bounds.width - measured_width, bounds.y]
        } else {
            [bounds.x, bounds.y]
        };
        aligned[0] += paint_offset[0];
        aligned[1] += paint_offset[1];
        // The line box's top lands on a whole pixel of the grid its glyphs are
        // placed on: the device grid for a translation, the entry's own raster
        // grid, before the homography, for anything else.
        //
        // The second is the same snap in the space a projected run is
        // resolved in, and that is what keeps an entry across the switch: at
        // an identity transform both put the paragraph at the same sub-pixel
        // phase, so a label whose container starts or stops turning, scaling
        // or pressing keeps its glyphs — its run row changes its flags and
        // nothing else. Unsnapped, every such transition rebuilt every label
        // under the container, and a press made the label jump by a fraction
        // of a pixel on its first frame.
        //
        // The snapped top is kept as the whole pixel it is. Carried back
        // through logical px and out again it comes back as 1.9999999 as
        // often as 2.0, and `floor` turns that into a line one pixel higher
        // with a phase of 0.9999999: a label that jumps a pixel on some
        // frames of a scroll and is re-resolved on each of them.
        let line_logical = laid_out_height;
        let top_px = if translation {
            let wy = clip::on_grid(aligned[1], affine[5], scale);
            let (top_px, _) =
                clip::snap_centered_origin(wy + line_logical * 0.5, line_logical, scale);
            aligned[1] += top_px / scale - wy;
            top_px
        } else {
            let (top_px, _) =
                clip::snap_centered_origin(aligned[1] + line_logical * 0.5, line_logical, raster);
            aligned[1] = top_px / raster;
            top_px
        };
        if fragment_clip == clip::FragmentClip::REJECT {
            return None;
        }
        // What the glyphs can actually cover, in the same local space as
        // `bounds`. Not the content box: `overflow: visible` text (a fixed
        // height holding three lines, `wrap: false` in a narrow box) paints
        // outside it, and the caller uses this to decide whether reordering a
        // batch would cross this text.
        //
        // The laid-out box is the union of the line boxes. Ink leaves it
        // vertically exactly when the requested line height is shorter than
        // what the face needs, so pad by that shortfall against a 1.25em
        // natural height — nothing at a normal line height, a few pixels at
        // `line-height: 1`. Horizontally the pad covers side bearings, which
        // an italic or a swash can push past the advance box.
        let pad_y = (size * 1.25 - line_height).max(0.0);
        let pad_x = size * 0.25;
        let ink = LogicalRect::from_xywh(
            aligned[0] - pad_x,
            aligned[1] - pad_y,
            bounds.width.max(measured_width) + pad_x * 2.0,
            laid_out_height + pad_y * 2.0,
        );
        // Where the paragraph starts, in the whole pixels its run row carries
        // and the sub-pixel phase its glyphs are rasterized for. A translated
        // run's whole pixels are relative to the translation's own, which the
        // presentation row carries: a scroll that lands on whole pixels then
        // moves the one row every label under it shares, and not a run.
        //
        // The phase is the paragraph's own position's alone. The painter
        // draws a translation on whole device pixels (`clip::snap_translation`,
        // #223), so a scroll by any amount, fraction or not, keeps every
        // label's glyphs; what the translation adds past its whole pixels is
        // the ulp `k / scale * scale` can come back with, and that is no phase.
        // Vertically there is no phase at all: the line box top is a whole
        // pixel.
        let (x, y) = if translation {
            let [_, ty] = pipeline::whole_translation(affine, scale);
            (aligned[0] * scale, top_px - ty)
        } else {
            (aligned[0] * raster, top_px)
        };
        let whole = [x.floor(), y];
        let phase = [(x - whole[0]).to_bits(), 0f32.to_bits()];
        // An axis-aligned run is clipped by the batch's scissor. Rotated or
        // projective text carries the same homography as Quad, applied per
        // glyph corner in the vertex stage, and a rounded or polygonal clip
        // needs the fragment test the scissor cannot express — neither of
        // which is a reason to resolve the paragraph differently.
        let mut flags = 0;
        if !translation {
            // The corners no longer land on the texel grid, so nearest
            // sampling would alias them.
            flags |= pipeline::RUN_PROJECT | pipeline::RUN_LINEAR;
        }
        if fragment_clip != clip::FragmentClip::PASS {
            flags |= pipeline::RUN_CLIP;
        }
        // Text that cannot reach a pixel costs nothing: no entry, no run, no
        // draw. The same predicate the scissor would apply, one rectangle at a
        // time instead of one glyph at a time.
        // `ink` is in the node's own space and `clip` is in paint space, so
        // the homography has to be applied before they can be compared — a
        // scrolled viewport folds its offset into `affine`, and comparing the
        // two spaces directly would cull exactly the text that scrolled into
        // view.
        let reachable = transformed_ink(ink, affine, persp)
            .intersection(clip)
            .is_some();
        self.target.counters.nodes_considered += 1;
        if !reachable {
            self.target.counters.nodes_culled += 1;
            return None;
        }
        // Subpixel coverage only where the blend lands on the surface it was
        // rasterized for: an upright run, straight onto the window.
        let mode = if translation && opaque_backdrop {
            self.subpixel
        } else {
            GlyphRenderMode::Mask
        };
        let index = self.target.live_runs;
        if index == self.target.runs.len() {
            self.target.runs.push(TextRun {
                entry: 0,
                presentation: RunPresentation {
                    origin: [0.0; 2],
                    color: [0.0; 4],
                    opacity: 1.0,
                    flags: 0,
                    presentation: 0,
                    raster: 1.0,
                },
                next: NO_RUN,
                last: index as u32,
                folded: false,
                segments: 0..0,
            });
        }
        let epoch = self.placement_epoch();
        let reusable = retained
            .map(|(id, _, _)| id)
            .or_else(|| self.target.entries.lookup(entry_key))
            .filter(|id| {
                self.target.entries.get(*id).is_some_and(|entry| {
                    entry.valid(
                        hash,
                        colors.fingerprint,
                        phase,
                        raster.to_bits(),
                        self.font_generation,
                        mode,
                    )
                })
            })
            .filter(|id| {
                // The atlas moved since this entry read its rectangles. Repair
                // them through the handles it kept — no shaping, no
                // rasterizing, no atlas traffic. Repairing here rather than at
                // flush is what lets a handle the atlas has reissued be
                // *resolved* again in the same frame instead of drawing a
                // paragraph with holes in it for one frame.
                let Self { atlas, target, .. } = self;
                let stale = target
                    .entries
                    .get(*id)
                    .is_some_and(|entry| entry.atlas_epoch != epoch);
                if !stale {
                    return true;
                }
                target.counters.instance_patches += 1;
                target.entries.repair(*id, epoch, |handle| {
                    atlas.entry(handle).map(|entry| (entry.origin, entry.size))
                })
            });
        let entry = match reusable {
            Some(id) => {
                // The paragraph, its sub-pixel phase and the face set are all
                // the ones these instances were resolved from. Nothing below
                // this line reads a glyph.
                self.target.entries.note_reuse();
                self.target.counters.nodes_skipped += 1;
                id
            }
            None => self.build_entry(
                device,
                entry_key,
                hash,
                phase,
                (raster, step),
                mode,
                default_color,
                &colors,
                revision,
                retained_paragraph.as_deref(),
            )?,
        };
        let frame = self.target.frame;
        if let Some(entry) = self.target.entries.get_mut(entry) {
            entry.last_used = frame;
            entry.measured = [measured_width, laid_out_height];
        }
        // After the entry, so a paragraph that resolves to nothing does not
        // leave a row in the table nobody names.
        let presentation = self.presentation_index(affine, persp, fragment_clip, scale);
        let run = &mut self.target.runs[index];
        run.entry = entry;
        run.presentation = RunPresentation {
            origin: whole,
            color: run_color(default_color),
            opacity: opacity.clamp(0.0, 1.0),
            flags,
            presentation,
            raster,
        };
        run.next = NO_RUN;
        run.last = index as u32;
        run.folded = false;
        run.segments = 0..0;
        self.target.live_runs += 1;
        Some(PreparedText { index, ink })
    }

    /// The row `affine`/`persp`/`clip` present through, adding it if this
    /// frame has not seen that combination yet.
    fn presentation_index(
        &mut self,
        affine: [f32; 6],
        persp: [f32; 2],
        fragment_clip: clip::FragmentClip,
        scale: f32,
    ) -> u32 {
        // A shell's labels nearly all share one transform and one clip. Ask
        // that question of the four values the row is derived from, not of the
        // row: building it is a clip inversion and a hundred and sixty bytes,
        // and hashing it is forty words, to find out they are the ones the
        // label before this one already had.
        let inputs = (affine, persp, fragment_clip, scale.to_bits());
        if let Some((held, index)) = self.target.last_presentation
            && held == inputs
        {
            return index;
        }
        let row = TextPresentationGpu::new(
            affine,
            persp,
            &fragment_clip.for_physical_pixels(scale),
            scale,
        );
        let bits = row.to_bits();
        if let Some(last) = self.target.presentations.last()
            && *last == row
        {
            let index = (self.target.presentations.len() - 1) as u32;
            self.target.last_presentation = Some((inputs, index));
            return index;
        }
        if let Some(index) = self.target.presentation_index.get(&bits) {
            let index = *index;
            self.target.last_presentation = Some((inputs, index));
            return index;
        }
        let index = self.target.presentations.len() as u32;
        self.target.presentations.push(row);
        self.target.presentation_index.insert(bits, index);
        self.target.last_presentation = Some((inputs, index));
        index
    }

    /// Lay one paragraph out through the process-wide `nana-text` engine.
    ///
    /// Everything is passed in **logical** px and the layout scale is left at
    /// 1, so the result does not depend on the device scale: the same layout
    /// serves 1x and 2x, and it is the same one Runtime measured this node
    /// with. The scale is applied to the *positions* when the glyphs are
    /// resolved.
    #[allow(clippy::too_many_arguments)]
    fn lay_out(
        &mut self,
        content: &str,
        family: Option<&str>,
        weight: Option<u16>,
        font_size_px: f32,
        line_height_px: f32,
        letter_spacing_px: f32,
        italic: bool,
        wrap: bool,
        wrap_break: nana_ui_core::TextWrapBreak,
        ellipsis: bool,
        max_lines: Option<u16>,
        max_width_px: f32,
        max_height_px: f32,
        align: TextAlignSpec,
        opentype: &SceneTextOpenType,
    ) -> Arc<TextLayout> {
        let source = nana_text::TextSource::new(content);
        let style = nana_text::TextStyle {
            // The same family rule Runtime measured with, from the same
            // function: a named family that says `mono` falls back to
            // `monospace`. Two spellings of it would be two font selections
            // for one node.
            font_family: family.map(nana_ui_runtime::nana_font_family),
            font_size_px,
            font_weight: weight.unwrap_or(400),
            italic,
            line_height: Some(LineHeightSpec::Absolute(line_height_px)),
            // A non-finite tracking would poison every advance after it.
            letter_spacing_px: if letter_spacing_px.is_finite() {
                letter_spacing_px
            } else {
                0.0
            },
            features: opentype.features.clone(),
            variations: opentype.variations.clone(),
            kerning: opentype.kerning,
        };
        // The box dimension lines stack along — the height, or the width of
        // vertical text — is a truncation budget, so it only goes to the
        // engine when truncation was asked for: the same rule
        // `nana_text_constraints` applies to the layout Runtime keeps, so the
        // fallback here and the handle cannot disagree about it. A box too
        // short is an overflow the scissor clips, not a shorter paragraph.
        let vertical = opentype.writing_mode.is_vertical();
        let constraints = nana_text::TextConstraints {
            max_width_px: (!vertical || ellipsis).then_some(max_width_px),
            max_height_px: (vertical || ellipsis).then_some(max_height_px),
            wrap: wrap.then_some(wrap_break),
            word_break: opentype.word_break,
            line_break: opentype.line_break,
            max_lines,
            ellipsis,
            // What the Runtime measured this node with. `white-space: normal`
            // folds an authored newline into a space, and measuring one line
            // while painting two is the whole reason this rides on the scene
            // rather than being assumed here.
            preserve_lines: opentype.preserve_lines,
            base_direction: opentype.direction,
            align,
            writing_mode: opentype.writing_mode,
            text_orientation: opentype.text_orientation,
            scale: nana_text::TextScale {
                px_per_logical: 1.0,
            },
            ..nana_text::TextConstraints::default()
        };
        let kind = if wrap {
            nana_text::TextKind::Paragraph
        } else {
            nana_text::TextKind::Label
        };
        let mut counters = nana_text::TextWorkCounters::default();
        let engine = Arc::clone(&self.engine);
        let mut engine = crate::text_engine::lock_engine(&engine);
        engine.layout(kind, &source, &style, &constraints, &mut counters)
    }

    /// Turn one shaped paragraph into placed, atlas-resident glyphs, and keep
    /// them.
    ///
    /// This is the only function that knows how the paragraph was laid out.
    /// Everything it stores is in the renderer's own terms: an instance per
    /// glyph in the run's own space, and the atlas handle it was read from so
    /// a later relocation can be repaired instead of re-resolved.
    #[allow(clippy::too_many_arguments)]
    fn build_entry(
        &mut self,
        device: &wgpu::Device,
        key: EntryKey,
        hash: u64,
        phase: [u32; 2],
        // Logical-to-raster px, applied to the layout's coordinates here and
        // nowhere else: it is not in the layout key, so 1x and 2x resolve the
        // same paragraph into different glyphs. The device scale times the
        // raster step, which is kept on the entry for the next frame's
        // hysteresis.
        (scale, step): (f32, i8),
        mode: GlyphRenderMode,
        default_color: [f32; 4],
        colors: &SpanColors,
        revision: u64,
        // The Runtime's own layout for this node, when it has one. `None`
        // means the painter laid this paragraph out itself and it is in the
        // shape cache under `hash`.
        paragraph: Option<&TextLayout>,
    ) -> Option<u32> {
        let origin = [f32::from_bits(phase[0]), f32::from_bits(phase[1])];
        let Self {
            shape_cache,
            resolved,
            rasterizer,
            font_generation,
            ..
        } = self;
        let layout = match paragraph {
            Some(paragraph) => paragraph,
            None => shape_cache.layout(hash).expect("laid out above"),
        };
        resolved.clear();
        let generation = *font_generation as u32;
        if layout.is_vertical() {
            resolve_vertical(
                layout, rasterizer, resolved, colors, generation, scale, origin,
            );
        }
        for line in layout.lines.iter().filter(|_| !layout.is_vertical()) {
            // The baseline is snapped whole so it cannot land between texels;
            // the sub-pixel phase of the paragraph's origin is what the glyph
            // bitmaps were rasterized for and stays fractional.
            let baseline = (line.metrics.baseline_y_px * scale).round();
            for run in layout.line_runs(line) {
                // A run the font layer could not instantiate has no face to
                // scale, so there is nothing to draw for it. Its advances are
                // still in the line, which is why the layout box does not move.
                let Some(instance) = run.instance.as_ref() else {
                    continue;
                };
                let (font, variation, synthesis) = rasterizer.intern_instance(instance);
                let size = size_bits(run.font_size_px * scale);
                let mut pen = run.origin_x_px;
                for glyph in &run.glyphs {
                    let x = (pen + glyph.offset_x_px) * scale + origin[0];
                    // `offset_y_px` is the shaper's, positive up; screen y
                    // grows down. `floor`, not `trunc`: rounding toward zero
                    // would snap text above the origin the other way and shift
                    // its baseline by a pixel as it scrolls past y = 0.
                    let y = (origin[1] - glyph.offset_y_px * scale).floor() + baseline;
                    pen += glyph.advance_px;
                    resolved.push(
                        font,
                        generation,
                        variation,
                        size,
                        synthesis,
                        mode,
                        colors.color_at(glyph.cluster as usize),
                        PlacedGlyph {
                            glyph: glyph.glyph_id,
                            x,
                            y,
                        },
                    );
                }
            }
        }
        if resolved.is_empty() {
            return None;
        }
        let Self {
            resolved,
            rasterizer,
            raster,
            atlas,
            uploads,
            target,
            resolve_requests,
            ..
        } = self;
        let pages_before = atlas.page_count();
        // Read before the first placement, not after the last. Faulting a
        // glyph in can repack the atlas, and a repack moves glyphs this same
        // build already wrote instances for; stamping the entry with the
        // epoch from *after* that would declare those rectangles current and
        // nothing would ever repair them. Stamped with this one, the flush
        // re-reads them through the handles.
        let epoch = atlas.placement_epoch();
        // A glyph the atlas could not place this frame. The entry draws what
        // it has and asks again next frame, when the frame that crowded it
        // out may be gone; without this it would be missing for good, since
        // nothing else about the paragraph would change.
        let mut unplaced = false;
        let inherited = pipeline::pack_srgb(default_color);
        let placeholders = [
            atlas.placeholder_page(AtlasPageKind::Mask),
            atlas.placeholder_page(AtlasPageKind::Color),
        ];
        let id = target
            .entries
            .begin_build(key, resolved.glyphs.len() as u32, |handle| {
                atlas.release(handle)
            });
        target.counters.instance_rebuilds += 1;
        let mut placed = 0u32;
        let mut segments: Vec<EntrySegment> = Vec::new();
        for run in &resolved.runs {
            // The run's color is the same for every glyph under it, so it is
            // packed once rather than per glyph — and compared once against
            // the paragraph's own color, because a glyph that paints it can
            // inherit the run row instead of carrying four bytes that a
            // recolor would then have to rewrite.
            let color = pipeline::pack_srgb(run.color);
            let own = if color == inherited {
                0
            } else {
                pipeline::INSTANCE_OWN_COLOR
            };
            for glyph in resolved.glyphs_of(run) {
                *resolve_requests += 1;
                let (raster_key, pen) = run.raster_key(glyph);
                let (handle, placement) = match atlas.lookup(&raster_key) {
                    Some(placed) => placed,
                    None => {
                        let Some(image) = raster.get_or_rasterize(rasterizer, raster_key) else {
                            continue;
                        };
                        match atlas.insert(device, raster_key, &image, raster, uploads) {
                            Some(placed) => placed,
                            None => {
                                // Waiting for room, not too big for any page:
                                // one that can never fit would only fail again,
                                // and rebuild its paragraph every frame doing so.
                                unplaced |= atlas.could_hold(&image);
                                continue;
                            }
                        }
                    }
                };
                atlas.retain(handle);
                let content = match placement.kind {
                    AtlasPageKind::Mask => CONTENT_MASK,
                    AtlasPageKind::Color if placement.subpixel => {
                        CONTENT_COLOR | pipeline::INSTANCE_SUBPIXEL
                    }
                    AtlasPageKind::Color => CONTENT_COLOR,
                };
                let (mask_page, color_page) = match placement.kind {
                    AtlasPageKind::Mask => (placement.page, placeholders[1]),
                    AtlasPageKind::Color => (placeholders[0], placement.page),
                };
                push_entry_segment(&mut segments, placeholders, mask_page, color_page, placed);
                target.entries.push_glyph(
                    id,
                    placed,
                    handle,
                    GlyphInstance::new(
                        [pen[0] + placement.left, pen[1] - placement.top],
                        placement.size,
                        placement.origin,
                        color,
                        content | own,
                    ),
                );
                placed += 1;
            }
        }
        target.frame_gpu_allocations += atlas.page_count() - pages_before;
        target.entries.finish_build(id, placed);
        let fonts = self.font_generation;
        let entry = self.target.entries.get_mut(id).expect("just built");
        entry.layout = hash;
        entry.colors = colors.fingerprint;
        entry.phase = phase;
        entry.font_generation = fonts;
        entry.mode = mode;
        entry.revision = revision;
        entry.scale_bits = scale.to_bits();
        entry.raster_step = step;
        entry.atlas_epoch = epoch;
        entry.segments = segments;
        entry.run = NO_RUN;
        entry.damaged = unplaced;
        if placed == 0 {
            return None;
        }
        Some(id)
    }

    /// Fold the run just opened by `next` into `previous`, so both draw as one
    /// command. Returns `false` when the two cannot share a run and the caller
    /// must keep `next` as its own command.
    ///
    /// Mirrors [`super::push_icon`] / [`super::push_quad`]: only runs that are
    /// already neighbours in document order merge, and glyph order inside the
    /// merged command is placement order, so a text shadow still paints under
    /// the text it belongs to.
    ///
    /// Color, position, opacity and transform are **not** part of this
    /// decision. Each instance names its own run row, so two paragraphs that
    /// present completely differently still draw as one command as long as
    /// their glyphs come from compatible atlas pages.
    pub(super) fn can_merge_runs(&self, previous: &PreparedText, next: &PreparedText) -> bool {
        // `next` must be the run just opened, so folding it away is an append.
        next.index + 1 == self.target.live_runs
            && previous.index < next.index
            && previous.index >= self.target.flushed
            && self
                .target
                .runs
                .get(previous.index)
                .is_some_and(|run| !run.folded)
    }

    pub(super) fn merge_runs(&mut self, previous: &PreparedText, next: &PreparedText) {
        debug_assert!(self.can_merge_runs(previous, next));
        let tail = self.target.runs[previous.index].last;
        self.target.runs[tail as usize].next = next.index as u32;
        self.target.runs[previous.index].last = next.index as u32;
        self.target.runs[next.index].folded = true;
    }

    /// Give every still-open run an arena block, an index range and a run
    /// row, and turn the entries under it into draw segments. Must run before
    /// `upload` and before `draw`.
    ///
    /// This is where retention pays: an entry whose block and range are
    /// already placed, under the run index it already names, is passed over
    /// without reading a single instance or writing a single index. An entry
    /// that outgrew its block moves in the arena and writes its own bytes
    /// there; what its neighbours pay for it is at most a rewrite of the index
    /// table, never of their instances.
    pub(super) fn flush_runs(&mut self) {
        if self.target.flushed >= self.target.live_runs {
            return;
        }
        let Self {
            atlas, target, gpu, ..
        } = self;
        // The table persists, and a row is only written when it actually
        // changed, so an unchanged frame neither copies it nor compares it.
        target.run_dirty = None;
        let rows = target.run_table.len();
        target
            .run_table
            .resize(target.run_slots.len(), TextRunGpu::VACANT);
        if target.run_table.len() > rows {
            target.run_dirty = Some(rows as u32..target.run_table.len() as u32);
        }
        target.segments.clear();
        target.writes.clear();
        target.staging.clear();
        target.index_writes.clear();
        target.index_staging.clear();
        // What the frame needs, and what of it the arena and the order do not
        // already hold, so a repack happens instead of running out of room.
        let mut outgrown = std::mem::take(&mut target.outgrown);
        let mut fresh = std::mem::take(&mut target.fresh_blocks);
        let mut fresh_order = std::mem::take(&mut target.fresh_ranges);
        target.skipped.clear();
        let limit = gpu.instance_slots();
        target.arena.set_limit(limit);
        let mut total = Self::measure_runs(target, &mut fresh, &mut fresh_order, &mut outgrown);
        if total > limit {
            // More glyphs than this device can bind as one storage buffer —
            // a whole log file set as one paragraph. The largest paragraphs
            // are left out until the rest fits with room to spare:
            // everything else on screen still draws, and a frame that would
            // have failed validation says so instead.
            let asked = total;
            Self::skip_largest(target, total, limit);
            total = Self::measure_runs(target, &mut fresh, &mut fresh_order, &mut outgrown);
            use nana_diagnostics::framework::text;
            static EXCEEDED: nana_diagnostics::Throttle = nana_diagnostics::Throttle::new();
            nana_diagnostics::metric!(text::INSTANCE_LIMIT_FRAMES);
            if nana_diagnostics::enabled(text::INSTANCE_LIMIT_EXCEEDED.severity)
                && EXCEEDED.allow(std::time::Duration::from_secs(10))
            {
                nana_diagnostics::event!(
                    text::INSTANCE_LIMIT_EXCEEDED,
                    slots = u64::from(asked),
                    limit = u64::from(limit),
                    skipped = target.skipped.len() as u64
                );
            }
        }
        // A range outgrowing its place is text still growing. Once it has
        // stopped for long enough, gaps left for it are quads every frame
        // runs for nothing, and the order is repacked without them.
        target.quiet_frames = if outgrown.is_empty() {
            target.quiet_frames.saturating_add(1)
        } else {
            0
        };
        let settled = target.quiet_frames >= SETTLE_FRAMES;
        // Independent: the arena repacks only to make room or close holes,
        // and then rewrites instances; the order repacks when fragmentation
        // costs too many draws, or to give back gaps, and then rewrites only
        // indices. Never twice in a frame: one decision covers both reasons.
        let arena_repack = target.arena.should_repack(total, &fresh);
        let order_repack =
            target.order.should_repack(total, &fresh_order) || (target.order_gapped && settled);
        target.fresh_blocks = fresh;
        target.fresh_ranges = fresh_order;
        if arena_repack {
            target.arena.repack(total);
            target.entries.invalidate_arena();
        }
        if order_repack {
            target.order.repack(total);
            target.entries.invalidate_order();
            // Most of what moved since the last repack moved because its
            // paragraph grew: this text keeps changing length, so the new
            // layout leaves room for it to grow where it is. Text that only
            // comes and goes gets none — a gap would not keep it from
            // splitting the draws, and every gap slot is a quad the vertex
            // stage still runs. Nor does text that grew once but has since
            // stood still.
            target.order_gapped = !settled && target.growth_moves * 2 > target.order_moves;
            // Only a layout with gaps grows ranges in place, which is the
            // one thing that asks who owns an offset.
            target
                .order_owners
                .clear(target.order_gapped.then(|| target.order.capacity()));
            target.growth_moves = 0;
            target.order_moves = 0;
        } else if target.order_gapped && !outgrown.is_empty() {
            Self::grow_ranges_in_place(target, &outgrown);
        }
        target.outgrown = outgrown;
        let epoch = atlas.placement_epoch();
        let placeholders = [
            atlas.placeholder_page(AtlasPageKind::Mask),
            atlas.placeholder_page(AtlasPageKind::Color),
        ];
        let generation = target.arena.generation();
        let order_generation = target.order.generation();
        let mut breaks = 0u32;
        let mut group_start = 0u32;
        for index in 0..target.live_runs {
            if target.runs[index].folded {
                continue;
            }
            let first_segment = target.segments.len() as u32;
            let mut builder = SegmentBuilder::new(placeholders);
            let mut member = index as u32;
            let mut previous_end = None;
            loop {
                let entry_id = target.runs[member as usize].entry;
                let slot = match target.entries.get(entry_id).and_then(|entry| entry.slot) {
                    Some(slot) => slot,
                    None => {
                        let slot = target.run_slots.alloc();
                        if let Some(entry) = target.entries.get_mut(entry_id) {
                            entry.slot = Some(slot);
                        }
                        slot
                    }
                };
                let row = target.runs[member as usize].presentation.to_gpu();
                if target.run_table.len() <= slot as usize {
                    target
                        .run_table
                        .resize(slot as usize + 1, TextRunGpu::VACANT);
                }
                if target.run_table[slot as usize] != row {
                    target.run_table[slot as usize] = row;
                    target.run_dirty = Some(match target.run_dirty.take() {
                        Some(range) => range.start.min(slot)..range.end.max(slot + 1),
                        None => slot..slot + 1,
                    });
                }
                let Some(entry) = target.entries.get(entry_id) else {
                    break;
                };
                if target.skipped.binary_search(&entry_id).is_ok() {
                    let next = target.runs[member as usize].next;
                    if next == NO_RUN {
                        break;
                    }
                    member = next;
                    continue;
                }
                let capacity = entry.capacity;
                let mut dirty = entry.atlas_epoch != epoch;
                let placed =
                    entry.arena_generation == Some(generation) && entry.arena_capacity == capacity;
                if !placed {
                    if entry.arena_generation == Some(generation) {
                        let (offset, held) = (entry.arena_offset, entry.arena_capacity);
                        target.arena.release(generation, offset, held);
                    }
                    let offset = target.arena.alloc(capacity);
                    let entry = target.entries.get_mut(entry_id).expect("looked up above");
                    entry.indices_stale |=
                        entry.arena_offset != offset || entry.arena_capacity != capacity;
                    entry.arena_offset = offset;
                    entry.arena_capacity = capacity;
                    entry.arena_generation = Some(generation);
                    dirty = true;
                }
                if entry_needs_repair(&target.entries, entry_id, epoch) {
                    // A glyph moved inside the atlas while a later paragraph
                    // was faulting its own in. Rectangles are re-read through
                    // the handles the entry kept; nothing is reshaped,
                    // re-rasterized or re-uploaded.
                    let intact = target.entries.repair(entry_id, epoch, |handle| {
                        atlas.entry(handle).map(|entry| (entry.origin, entry.size))
                    });
                    target.counters.instance_patches += 1;
                    if !intact && let Some(entry) = target.entries.get_mut(entry_id) {
                        entry.damaged = true;
                    }
                }
                if target.entries.bind_run(entry_id, slot) {
                    target.counters.instance_patches += 1;
                    dirty = true;
                }
                let entry = target.entries.get(entry_id).expect("looked up above");
                let offset = entry.arena_offset;
                if dirty {
                    stage_write(&mut target.writes, target.staging.len(), offset, capacity);
                    let block = target.entries.instances(entry);
                    target.staging.extend_from_slice(block);
                }
                // Exactly the block's size. Room to grow here would let a
                // paragraph lengthen without moving in the draw order, but
                // every spare index is a quad the vertex stage still runs —
                // and on the GPU that costs more than the draws and the
                // index rewrites it would save.
                let ordered = entry.order_generation == Some(order_generation)
                    && range_keeps(entry.order_capacity, capacity);
                if !ordered {
                    if !order_repack {
                        target.order_moves += 1;
                    }
                    if entry.order_generation == Some(order_generation) {
                        let (at, held) = (entry.order_offset, entry.order_capacity);
                        target.order.release(order_generation, at, held);
                        target.order_owners.remove(at);
                        if capacity > held {
                            target.growth_moves += 1;
                        }
                    }
                    let at = target.order.alloc(capacity);
                    target.order_owners.insert(at, entry_id);
                    let entry = target.entries.get_mut(entry_id).expect("looked up above");
                    entry.order_offset = at;
                    entry.order_capacity = capacity;
                    entry.order_generation = Some(order_generation);
                    if order_repack
                        && target.order_gapped
                        && at + capacity - group_start >= GAP_EVERY
                        && let Some(gap) = target.order.reserve_gap(GAP_SLOTS)
                    {
                        stage_write(
                            &mut target.index_writes,
                            target.index_staging.len(),
                            gap,
                            GAP_SLOTS,
                        );
                        target
                            .index_staging
                            .extend(std::iter::repeat_n(VACANT_INDEX, GAP_SLOTS as usize));
                        group_start = gap + GAP_SLOTS;
                    }
                }
                let entry = target.entries.get_mut(entry_id).expect("looked up above");
                let order = entry.order_offset;
                let span = entry.order_capacity;
                // The range names the block slot for slot, slack included:
                // a vacant slot's instance covers nothing, so a draw that
                // spans it paints nothing there.
                if !ordered || entry.indices_stale {
                    entry.indices_stale = false;
                    stage_write(
                        &mut target.index_writes,
                        target.index_staging.len(),
                        order,
                        span,
                    );
                    target.index_staging.extend(offset..offset + capacity);
                    target.index_staging.extend(std::iter::repeat_n(
                        VACANT_INDEX,
                        (span - capacity) as usize,
                    ));
                }
                let entry = target.entries.get(entry_id).expect("looked up above");
                let adjacent = previous_end
                    .is_none_or(|end| end == order || target.order.clean_between(end, order));
                if !adjacent {
                    breaks += 1;
                }
                for segment in &entry.segments {
                    builder.push(&mut target.segments, *segment, order, adjacent);
                }
                previous_end = Some(order + span);
                let next = target.runs[member as usize].next;
                if next == NO_RUN {
                    break;
                }
                member = next;
            }
            builder.finish(&mut target.segments);
            target.runs[index].segments = first_segment..target.segments.len() as u32;
        }
        target.order.note_breaks(breaks);
        target.flushed = target.live_runs;
        #[cfg(test)]
        self.audit_placements();
    }

    /// What the frame's drawn entries need: their slots in total, the blocks
    /// and ranges the arena and the order do not already hold, and the ranges
    /// whose block outgrew them. Entries in `target.skipped` are not drawn.
    fn measure_runs(
        target: &mut TextPipelineTarget,
        fresh: &mut Vec<u32>,
        fresh_order: &mut Vec<u32>,
        outgrown: &mut Vec<u32>,
    ) -> u32 {
        fresh.clear();
        fresh_order.clear();
        outgrown.clear();
        let mut total = 0u32;
        Self::walk_runs(target, |target, _, entry_id| {
            let Some(entry) = target.entries.get(entry_id) else {
                return;
            };
            if target.skipped.binary_search(&entry_id).is_ok() {
                return;
            }
            let capacity = entry.capacity;
            total = total.saturating_add(capacity);
            if entry.arena_generation != Some(target.arena.generation())
                || entry.arena_capacity != capacity
            {
                fresh.push(capacity);
            }
            let keeps = range_keeps(entry.order_capacity, capacity);
            if entry.order_generation != Some(target.order.generation()) || !keeps {
                fresh_order.push(capacity);
                if entry.order_generation == Some(target.order.generation())
                    && capacity > entry.order_capacity
                {
                    outgrown.push(entry_id);
                }
            }
        });
        total
    }

    /// Leave out the largest drawn entries until the rest need at most two
    /// thirds of `limit` slots. Fills `target.skipped`, sorted.
    ///
    /// Not just until they fit: an arena pinned at the limit has no room for
    /// the next block that moves, and would repack — rewrite every instance —
    /// on every frame. With a third free it repacks as rarely as any other.
    fn skip_largest(target: &mut TextPipelineTarget, total: u32, limit: u32) {
        // Slots per entry, counting an entry two commands draw twice, as the
        // total did.
        let mut drawn: HashMap<u32, u32> = HashMap::new();
        Self::walk_runs(target, |target, _, entry_id| {
            if let Some(entry) = target.entries.get(entry_id) {
                let slots = drawn.entry(entry_id).or_default();
                *slots = slots.saturating_add(entry.capacity);
            }
        });
        let mut largest = drawn.into_iter().collect::<Vec<_>>();
        largest.sort_unstable_by_key(|&(entry, slots)| (std::cmp::Reverse(slots), entry));
        let room = limit - limit / 3;
        let mut left = total;
        for (entry, slots) in largest {
            if left <= room {
                break;
            }
            left = left.saturating_sub(slots);
            target.skipped.push(entry);
        }
        target.skipped.sort_unstable();
    }

    /// Let every drawn range whose block outgrew it grow where it is: into
    /// the clean gap right after it, or by shifting the ranges between it and
    /// the next gap along by as much. A range that cannot is left to the walk,
    /// which places it elsewhere.
    ///
    /// Before the walk, so the offsets the walk turns into draws are final. A
    /// range that moves here has its indices rewritten by the walk if it is
    /// drawn this frame, and whenever it is next drawn otherwise.
    fn grow_ranges_in_place(target: &mut TextPipelineTarget, outgrown: &[u32]) {
        const WINDOW: u32 = 4096;
        'entries: for &entry_id in outgrown {
            let Some(entry) = target.entries.get(entry_id) else {
                continue;
            };
            // Listed twice when two commands draw it; the first grew it.
            if entry.capacity <= entry.order_capacity {
                continue;
            }
            let grow = entry.capacity - entry.order_capacity;
            let after = entry.order_offset + entry.order_capacity;
            let mut chain = Vec::new();
            let mut at = after;
            let gap = loop {
                let Some(owner) = target.order_owners.get(at) else {
                    match target.order.free_at(at) {
                        Some((len, true)) if len >= grow => break at,
                        _ => continue 'entries,
                    }
                };
                let Some(next) = target.entries.get(owner) else {
                    continue 'entries;
                };
                chain.push(owner);
                at += next.order_capacity;
                if at - after > WINDOW {
                    continue 'entries;
                }
            };
            target.order.grow_into(gap, grow);
            target.growth_moves += 1;
            target.order_moves += 1;
            for owner in chain.into_iter().rev() {
                let moved = target.entries.get_mut(owner).expect("owned");
                target.order_owners.remove(moved.order_offset);
                moved.order_offset += grow;
                moved.indices_stale = true;
                target.order_owners.insert(moved.order_offset, owner);
            }
            let entry = target.entries.get_mut(entry_id).expect("looked up above");
            entry.order_capacity = entry.capacity;
            entry.indices_stale = true;
        }
    }

    /// Every rectangle a drawn entry samples is the one its handle names now.
    ///
    /// Recomputed from the atlas rather than trusted from the epoch stamps,
    /// so every test that draws text also tests the rule that decides when a
    /// retained instance has to be repaired: an entry stamped current while
    /// holding a rectangle the atlas has since moved would sample some other
    /// glyph, and no counter would say so.
    #[cfg(test)]
    fn audit_placements(&self) {
        let target = &self.target;
        for run in &target.runs[..target.live_runs] {
            let Some(entry) = target.entries.get(run.entry) else {
                continue;
            };
            if target.skipped.binary_search(&run.entry).is_ok() {
                continue;
            }
            for (index, (instance, handle)) in target.entries.live_glyphs(entry).enumerate() {
                let Some(drawn) = instance.placement() else {
                    continue;
                };
                let current = self
                    .atlas
                    .entry(handle)
                    .map(|placed| (placed.origin, placed.size));
                assert_eq!(
                    Some(drawn),
                    current,
                    "glyph {index} of entry {} samples a rectangle its handle no \
                     longer names (entry epoch {}, atlas epoch {})",
                    run.entry,
                    entry.atlas_epoch,
                    self.placement_epoch(),
                );
            }
        }
    }

    /// Visit every run of every open command, in draw order.
    fn walk_runs(
        target: &mut TextPipelineTarget,
        mut visit: impl FnMut(&mut TextPipelineTarget, usize, u32),
    ) {
        for index in 0..target.live_runs {
            if target.runs[index].folded {
                continue;
            }
            let mut member = index as u32;
            loop {
                let entry = target.runs[member as usize].entry;
                visit(target, index, entry);
                let next = target.runs[member as usize].next;
                if next == NO_RUN {
                    break;
                }
                member = next;
            }
        }
    }

    /// [`Self::upload_with`] in an encoder of its own, submitted at once:
    /// for tests that draw without a painter around them.
    #[cfg(test)]
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui.scene.text.upload"),
        });
        self.upload_with(device, queue, &mut encoder, work);
        queue.submit([encoder.finish()]);
    }

    /// Write this frame's atlas regions, arena blocks, index ranges and
    /// presentation tables. Blocks and ranges are copied in `encoder`, so it
    /// must be the one the passes that draw them are recorded in, or one
    /// submitted before it.
    pub(super) fn upload_with(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        self.uploads.flush(queue, &self.atlas, work);
        // Bind groups are created here, where the atlas is still `&mut`, so
        // `draw` only has to look one up. Nearly every frame's segments name
        // the same pair, so the pair is only looked up when it changes.
        let Self { atlas, target, .. } = self;
        let mut last = None;
        for segment in target.segments.iter() {
            let pair = (segment.mask_page, segment.color_page);
            if last != Some(pair) {
                last = Some(pair);
                atlas.bind_group(device, pair.0, pair.1);
            }
        }
        let bytes = self.gpu.upload(
            device,
            queue,
            encoder,
            &mut target.gpu,
            &FrameUpload {
                physical_size: target.physical_size,
                arena_capacity: target.arena.capacity(),
                writes: &target.writes,
                staging: &target.staging,
                order_capacity: target.order.capacity(),
                index_writes: &target.index_writes,
                index_staging: &target.index_staging,
                runs: &target.run_table,
                run_dirty: target.run_dirty.clone(),
                presentations: &target.presentations,
                uploaded_presentations: &target.uploaded_presentations,
            },
            work,
        );
        if bytes.lost {
            // Nothing this frame placed reached the GPU. Forget every
            // placement, so the next frame places and writes everything it
            // draws again instead of trusting buffers that never got it.
            target.arena.reset();
            target.entries.invalidate_arena();
            target.forget_order();
        }
        target.counters.instance_upload_bytes += bytes.instances as u64;
        target.counters.index_upload_bytes += bytes.indices as u64;
        target.counters.presentation_upload_bytes += bytes.presentation as u64;
        target.frame_gpu_allocations += target.gpu.take_allocations();
        #[cfg(test)]
        self.audit_draw_order();
    }

    /// Every index a draw spans resolves, through what the GPU buffers hold,
    /// to the instance its entry holds, and every glyph an entry holds is under
    /// a draw.
    ///
    /// Checked against a copy of the two buffers rebuilt from the writes this
    /// target actually sent, so every test that draws text also tests the
    /// rules that decide when a block or a range is written: an entry that
    /// moved without its indices following, or a range reused while a draw
    /// still spans it, would draw another paragraph's glyphs, and no counter
    /// would say so.
    #[cfg(test)]
    fn audit_draw_order(&mut self) {
        let target = &mut self.target;
        let shadow = &mut target.shadow;
        // A replaced buffer starts zeroed, which is what a vacant glyph is.
        let capacity = target.arena.capacity() as usize;
        if capacity != 0 && capacity != shadow.instances.len() {
            shadow.instances = vec![GlyphInstance::VACANT; capacity];
        }
        let capacity = target.order.capacity() as usize;
        if capacity != 0 && capacity != shadow.indices.len() {
            shadow.indices = vec![0; capacity];
        }
        for write in &target.writes {
            let staged = &target.staging[write.staged.start as usize..write.staged.end as usize];
            shadow.instances[write.offset as usize..][..staged.len()].copy_from_slice(staged);
        }
        for write in &target.index_writes {
            let staged =
                &target.index_staging[write.staged.start as usize..write.staged.end as usize];
            shadow.indices[write.offset as usize..][..staged.len()].copy_from_slice(staged);
        }
        for head in 0..target.live_runs {
            if target.runs[head].folded {
                continue;
            }
            let mut expected = HashMap::new();
            let mut glyphs = Vec::new();
            let mut member = head as u32;
            loop {
                let run = &target.runs[member as usize];
                if let Some(entry) = target
                    .entries
                    .get(run.entry)
                    .filter(|_| target.skipped.binary_search(&run.entry).is_err())
                {
                    let block = target.entries.instances(entry);
                    assert!(
                        entry.order_capacity >= entry.capacity,
                        "entry {}'s index range is smaller than its block",
                        run.entry
                    );
                    for index in 0..entry.order_capacity {
                        let want = block.get(index as usize).copied();
                        let previous =
                            expected.insert(entry.order_offset + index, (run.entry, want));
                        assert!(
                            previous.is_none(),
                            "two entries of one command share index {}",
                            entry.order_offset + index
                        );
                    }
                    for segment in &entry.segments {
                        glyphs.extend(
                            (segment.first..segment.first + segment.count)
                                .map(|index| entry.order_offset + index),
                        );
                    }
                }
                if run.next == NO_RUN {
                    break;
                }
                member = run.next;
            }
            let run = &target.runs[head];
            let drawn = &target.segments[run.segments.start as usize..run.segments.end as usize];
            for segment in drawn {
                for position in segment.first..segment.first + segment.count {
                    let slot = shadow.indices[position as usize];
                    let Some((entry, want)) = expected.get(&position).copied() else {
                        assert!(
                            target.order.clean_at(position),
                            "a draw spans index {position}, which no entry of its command \
                             owns and no gap covers"
                        );
                        assert_eq!(slot, VACANT_INDEX, "gap index {position} must be vacant");
                        continue;
                    };
                    match want {
                        None => assert_eq!(
                            slot, VACANT_INDEX,
                            "index {position} runs past entry {entry}'s block and must be vacant"
                        ),
                        Some(want) => assert_eq!(
                            shadow.instances.get(slot as usize).copied(),
                            Some(want),
                            "index {position} of entry {entry} names slot {slot}, which does \
                             not hold the glyph the entry holds there"
                        ),
                    }
                }
            }
            for position in glyphs {
                assert!(
                    drawn
                        .iter()
                        .any(|segment| (segment.first..segment.first + segment.count)
                            .contains(&position)),
                    "index {position} holds a glyph no draw spans"
                );
            }
        }
    }

    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        prepared: &PreparedText,
        scissor: PhysicalRect,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let Some(run) = self
            .target
            .runs
            .get(prepared.index)
            .filter(|_| prepared.index < self.target.live_runs)
        else {
            return;
        };
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        let start = run.segments.start as usize;
        let end = (run.segments.end as usize).min(self.target.segments.len());
        let mut drawn = 0u64;
        let mut slots = 0u64;
        for segment in &self.target.segments[start.min(end)..end] {
            let Some(bind_group) = self
                .atlas
                .cached_bind_group(segment.mask_page, segment.color_page)
            else {
                continue;
            };
            self.gpu
                .draw_segment(pass, &self.target.gpu, segment, bind_group);
            drawn += 1;
            slots += u64::from(segment.count);
        }
        self.draws.set(self.draws.get() + drawn);
        self.slots_drawn.set(self.slots_drawn.get() + slots);
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            for _ in 0..drawn {
                work.record_draw_call();
            }
        }
    }

    /// A window or viewport closed. Its entries' claims on atlas glyphs end
    /// here — the glyphs stay placed for the windows still open, they just
    /// stop being counted as in use by one that is gone — and its counters
    /// are kept so the totals do not run backwards.
    pub(super) fn close_target(&mut self, mut target: TextPipelineTarget) {
        let atlas = &mut self.atlas;
        target.entries.clear(|handle| atlas.release(handle));
        self.closed.add(target.counters());
    }

    pub(super) fn swap_target(
        &mut self,
        target: &mut Option<TextPipelineTarget>,
        device: &wgpu::Device,
    ) {
        let target =
            target.get_or_insert_with(|| TextPipelineTarget::new(self.gpu.new_target(device)));
        std::mem::swap(&mut self.target, target);
    }
}

/// Where the order leaves a gap when it repacks around text that keeps
/// growing: after every [`GAP_EVERY`] slots, [`GAP_SLOTS`] of room. Measured
/// on ten thousand labels changing length (#224): a quarter of room per 512
/// slots took the draws from 90 to 14 and the index uploads to a third; an
/// eighth, or a group half the size, left two to three times the draws.
const GAP_EVERY: u32 = 512;
const GAP_SLOTS: u32 = 128;

/// Frames a target draws without a range outgrowing its place before a
/// layout with gaps is repacked without them (#230).
///
/// Counted in frames drawn, not in time, because that is what the gaps cost
/// by: ten thousand labels whose churn stopped drew 152 085 quads a frame
/// across their gaps and 136 045 without, and their GPU time went from 2.86
/// to 2.72 ms. The repack that gives the gaps back rewrites 544 KB of
/// indices, which did not stand out of the frame-to-frame noise, so a few
/// frames of gaps already pay for it — and for the repack that leaves gaps
/// again should the text resume growing. A second of frames at 60 Hz is
/// several times that, so text that pauses between bursts keeps its room.
pub(super) const SETTLE_FRAMES: u32 = 60;

/// Which entry's index range starts at each offset of the order. Flat, so
/// placing a whole frame's worth of ranges after a repack costs a store per
/// range rather than a tree insert.
///
/// Kept only while the order has gaps (`clear(Some(capacity))`); otherwise
/// nothing asks, and every call is free.
#[derive(Default)]
struct OrderOwners {
    kept: bool,
    owners: Vec<u32>,
}

impl OrderOwners {
    fn clear(&mut self, capacity: Option<u32>) {
        self.kept = capacity.is_some();
        self.owners.clear();
        self.owners.resize(capacity.unwrap_or(0) as usize, u32::MAX);
    }

    fn insert(&mut self, offset: u32, entry: u32) {
        if !self.kept {
            return;
        }
        let offset = offset as usize;
        if self.owners.len() <= offset {
            self.owners.resize(offset + 1, u32::MAX);
        }
        self.owners[offset] = entry;
    }

    fn remove(&mut self, offset: u32) {
        if let Some(owner) = self.owners.get_mut(offset as usize) {
            *owner = u32::MAX;
        }
    }

    fn get(&self, offset: u32) -> Option<u32> {
        self.owners
            .get(offset as usize)
            .copied()
            .filter(|owner| *owner != u32::MAX)
    }
}

/// Queue `len` slots at `offset`, staged from `staged` on: folded into the
/// last write when it ends exactly there, so a walk that places blocks one
/// after another turns into one write.
fn stage_write(writes: &mut Vec<ArenaWrite>, staged: usize, offset: u32, len: u32) {
    let staged = staged as u32;
    if let Some(last) = writes.last_mut()
        && last.offset + (last.staged.end - last.staged.start) == offset
    {
        last.staged.end += len;
        return;
    }
    writes.push(ArenaWrite {
        offset,
        staged: staged..staged + len,
    });
}

impl RunPresentation {
    fn to_gpu(self) -> TextRunGpu {
        TextRunGpu::new(
            self.origin,
            self.presentation,
            self.flags,
            self.color,
            self.opacity,
            self.raster,
        )
    }
}

/// Open or extend the segment an entry's glyph at `offset` belongs to.
///
/// A segment has to name a page of each kind because one bind group does, but
/// only the kinds it actually samples are constrained. A segment still holding
/// a placeholder has not sampled that kind yet, so the first glyph of it adopts
/// a page instead of splitting — which is what keeps one emoji in a line of
/// text free.
fn push_entry_segment(
    segments: &mut Vec<EntrySegment>,
    placeholders: [u32; 2],
    mask_page: u32,
    color_page: u32,
    offset: u32,
) {
    if let Some(open) = segments.last_mut() {
        let mask_ok = open.mask_page == mask_page
            || open.mask_page == placeholders[0]
            || mask_page == placeholders[0];
        let color_ok = open.color_page == color_page
            || open.color_page == placeholders[1]
            || color_page == placeholders[1];
        if mask_ok && color_ok {
            if mask_page != placeholders[0] {
                open.mask_page = mask_page;
            }
            if color_page != placeholders[1] {
                open.color_page = color_page;
            }
            open.count += 1;
            return;
        }
    }
    segments.push(EntrySegment {
        mask_page,
        color_page,
        first: offset,
        count: 1,
    });
}

/// What each byte of the content paints in.
///
/// Rich text is a color per byte range over one layout. Since #99 the layout
/// does not know about color at all, so this is what puts the two back
/// together — at resolve time, per glyph cluster, which is why a recolor is no
/// longer a reason to lay anything out again.
///
/// Node opacity is deliberately not folded in: it rides on the run row, so a
/// fade neither relayouts rich text nor rewrites a glyph.
struct SpanColors {
    /// Well-formed spans, in start order. Empty means solid `default`.
    spans: Vec<(Range<usize>, [f32; 4])>,
    default: [f32; 4],
    /// Identity of the whole mapping, for [`TextGpuEntry::colors`]. Zero for
    /// solid text, which is the case that may be recoloured without rebuilding
    /// anything.
    fingerprint: u64,
}

impl SpanColors {
    fn new(content: &str, spans: &[SceneTextSpan], default: [f32; 4]) -> Self {
        let mut ordered: Vec<&SceneTextSpan> = spans.iter().collect();
        // Overlap is resolved by start order below, so the order has to be the
        // one the rule assumes rather than the one the scene happened to write.
        ordered.sort_by_key(|span| (span.start, span.end));
        let mut kept: Vec<(Range<usize>, [f32; 4])> = Vec::new();
        for span in ordered {
            // A span the scene built against different bytes would recolor
            // whatever now sits at those offsets, so a range that is not a
            // char boundary of *this* content is dropped rather than snapped.
            if span.start >= span.end
                || span.end > content.len()
                || !content.is_char_boundary(span.start)
                || !content.is_char_boundary(span.end)
            {
                continue;
            }
            // Overlaps would make "which span wins" depend on search order.
            // Later spans lose, which is the order `Scene` writes them in.
            if kept.last().is_some_and(|(last, _)| last.end > span.start) {
                continue;
            }
            kept.push((span.start..span.end, span.color));
        }
        // The default is in the fingerprint because it decides which glyphs
        // carry their own colour: a span that happens to paint the default
        // inherits the run row, and would keep inheriting a *different* one
        // after a recolour if this did not notice.
        let fingerprint = if kept.is_empty() {
            0
        } else {
            let mut hasher = ShapeHasher::default();
            default.map(f32::to_bits).hash(&mut hasher);
            kept.len().hash(&mut hasher);
            for (range, color) in &kept {
                range.start.hash(&mut hasher);
                range.end.hash(&mut hasher);
                color.map(f32::to_bits).hash(&mut hasher);
            }
            // Zero means "solid"; a fingerprint that lands there would claim it.
            hasher.finish() | 1
        };
        Self {
            spans: kept,
            default,
            fingerprint,
        }
    }

    /// The color at a byte offset. Binary search rather than a cursor: an RTL
    /// run walks its clusters backwards, so offsets do not arrive in order.
    fn color_at(&self, byte: usize) -> [f32; 4] {
        if self.spans.is_empty() {
            return self.default;
        }
        let index = self.spans.partition_point(|(range, _)| range.start <= byte);
        match index.checked_sub(1).and_then(|i| self.spans.get(i)) {
            Some((range, color)) if range.end > byte => *color,
            _ => self.default,
        }
    }
}

/// Resolves a vertical paragraph's glyphs onto the page (#59).
///
/// Columns are placed against the paragraph's own column stack, not the box:
/// `vertical-rl` puts the first column at the stack's right edge and the
/// caller shifts the whole entry right by whatever the box adds. That keeps
/// the entry a function of the layout alone, so a box that only widened
/// reuses every instance instead of re-resolving them.
///
/// Each column's centre line is snapped whole, as a horizontal baseline is.
/// Upright glyphs hang from it by the offsets HarfRust reported relative to
/// their horizontal origin; a sideways run centres its em box on it and is
/// rasterized a quarter turn clockwise.
fn resolve_vertical(
    layout: &TextLayout,
    rasterizer: &mut PlatformRasterizer,
    resolved: &mut NanaGlyphBuffer,
    colors: &SpanColors,
    generation: u32,
    scale: f32,
    origin: [f32; 2],
) {
    let stack = layout.physical_size().0;
    for line in &layout.lines {
        let centre =
            (layout.physical_x_of_block(line.metrics.baseline_y_px, stack) * scale).round();
        for run in layout.line_runs(line) {
            let Some(instance) = run.instance.as_ref() else {
                continue;
            };
            let (font, variation, mut synthesis) = rasterizer.intern_instance(instance);
            let sideways = run.orientation == nana_text::RunOrientation::Sideways;
            // The sideways run's own baseline, placed so that its em box —
            // ascent to the right of it, descent to the left — is centred on
            // the column's centre line.
            let baseline = centre - (run.metrics.ascent_px - run.metrics.descent_px) * 0.5 * scale;
            if sideways {
                synthesis = synthesis.with(GlyphSynthesis::ROTATE_CW);
            }
            let size = size_bits(run.font_size_px * scale);
            let mut pen = run.origin_x_px;
            for glyph in &run.glyphs {
                let (x, y) = if sideways {
                    // Font y (up) is page x; font x is page y.
                    (
                        baseline + glyph.offset_y_px * scale + origin[0],
                        (pen + glyph.offset_x_px) * scale + origin[1],
                    )
                } else {
                    (
                        centre + glyph.offset_x_px * scale + origin[0],
                        (pen - glyph.offset_y_px) * scale + origin[1],
                    )
                };
                pen += glyph.advance_px;
                resolved.push(
                    font,
                    generation,
                    variation,
                    size,
                    synthesis,
                    GlyphRenderMode::Mask,
                    colors.color_at(glyph.cluster as usize),
                    PlacedGlyph {
                        glyph: glyph.glyph_id,
                        x,
                        y,
                    },
                );
            }
        }
    }
}

/// The page size of a paragraph: the widest line and the summed line boxes,
/// crossed over for vertical text.
fn measure(layout: &TextLayout) -> (f32, f32) {
    layout.physical_size()
}

fn text_box_origin(
    bounds: LogicalRect,
    vertical: TextVerticalAlignment,
    laid_out_height: f32,
) -> [f32; 2] {
    let top = match vertical {
        TextVerticalAlignment::Top => bounds.y,
        TextVerticalAlignment::Center => bounds.y + (bounds.height - laid_out_height) * 0.5,
        TextVerticalAlignment::Bottom => bounds.y + bounds.height - laid_out_height,
    };
    [bounds.x, top]
}

fn opentype_disc(word_break: nana_ui_core::WordBreakSpec) -> u8 {
    match word_break {
        nana_ui_core::WordBreakSpec::Normal => 0,
        nana_ui_core::WordBreakSpec::BreakAll => 1,
        nana_ui_core::WordBreakSpec::BreakWord => 2,
    }
}

fn opentype_line_disc(line_break: nana_ui_core::LineBreakSpec) -> u8 {
    match line_break {
        nana_ui_core::LineBreakSpec::Auto => 0,
        nana_ui_core::LineBreakSpec::Normal => 1,
        nana_ui_core::LineBreakSpec::Anywhere => 2,
    }
}

fn opentype_kern_disc(kerning: nana_ui_core::FontKerningSpec) -> u8 {
    match kerning {
        nana_ui_core::FontKerningSpec::Auto => 0,
        nana_ui_core::FontKerningSpec::Normal => 1,
        nana_ui_core::FontKerningSpec::None => 2,
    }
}

/// Whether `id`'s rectangles predate the atlas's current placement epoch.
fn entry_needs_repair(entries: &EntryStore, id: u32, epoch: u64) -> bool {
    entries
        .get(id)
        .is_some_and(|entry| entry.atlas_epoch != epoch)
}

/// The run row's color, quantized exactly as an instance's four bytes would
/// be.
///
/// A glyph inherits the row whenever its own packed color matches the
/// paragraph's, so the two must agree to the bit — otherwise a rich span that
/// happens to paint the default color would shift by a least significant bit
/// when it stopped carrying its own.
fn run_color(color: [f32; 4]) -> [f32; 4] {
    let [r, g, b, a] = to_rgba8(color);
    [
        linear_from_srgb8(r),
        linear_from_srgb8(g),
        linear_from_srgb8(b),
        f32::from(a) / 255.0,
    ]
}

/// The axis-aligned box `ink` covers once its node's homography is applied.
fn transformed_ink(ink: LogicalRect, affine: [f32; 6], persp: [f32; 2]) -> LogicalRect {
    clip::transformed_aabb_projective(ink, affine, persp)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn ellipsis_paint_keeps_exact_fit_and_truncates_only_narrow_boxes() {
        use nana_ui_runtime::{
            ComputedStyle, StableNodeId, TextContent, TextShapeConstraints, TextShaper,
        };

        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        pipeline.begin_frame([512, 64]);
        let mut shaper = crate::NanaTextShaper::default();
        let style = ComputedStyle {
            font_size: 12.0,
            font_weight: Some(600),
            ..ComputedStyle::default()
        };
        for (content, shaping) in [
            ("未命名 1", TextShaping::Advanced),
            ("shade", TextShaping::Advanced),
            ("Cost $0.30", TextShaping::Auto),
            ("Save memory", TextShaping::Auto),
        ] {
            let natural = shaper
                .shape(
                    StableNodeId::new(1).unwrap(),
                    &TextContent {
                        value: content.into(),
                    },
                    &style,
                    TextShapeConstraints {
                        wrap: false,
                        shaping,
                        ..TextShapeConstraints::default()
                    },
                )
                .width;
            // Revisit the exact-fit entry after inserting a truncated one.
            let mut full_glyphs = Vec::new();
            for (width, ellipsis) in [
                (natural, false),
                (natural, true),
                (natural * 0.5, true),
                (natural, true),
            ] {
                pipeline
                    .prepare(
                        &device,
                        LogicalRect::from_xywh(0.0, 0.0, width, 24.0),
                        LogicalRect::from_xywh(0.0, 0.0, 512.0, 64.0),
                        1.0,
                        content,
                        Some([1.0; 4]),
                        12.0,
                        Some(600),
                        None,
                        None,
                        false,
                        nana_ui_core::TextWrapBreak::Word,
                        false,
                        ellipsis,
                        None,
                        shaping,
                        TextHorizontalAlignment::Start,
                        TextVerticalAlignment::Top,
                        &[],
                        0.0,
                        &[],
                        &SceneTextOpenType::default(),
                        clip::IDENTITY_AFFINE,
                        [0.0; 2],
                        clip::FragmentClip::PASS,
                        1.0,
                        [0.0; 2],
                        EntryKey {
                            node: 1,
                            slot: 0,
                            pass: 0,
                        },
                        UNTRACKED_REVISION,
                        None,
                        true,
                    )
                    .expect("label must prepare");
                let entry = pipeline
                    .shape_cache
                    .entries
                    .values()
                    .find(|entry| {
                        entry.key.content == content
                            && entry.key.width_bits == width.to_bits()
                            && entry.key.ellipsis == ellipsis
                    })
                    .expect("prepared label must be cached at its own width");
                let painted_width = measure(&entry.layout).0;
                let painted_glyphs: Vec<_> = entry
                    .layout
                    .runs
                    .iter()
                    .flat_map(|run| {
                        run.glyphs
                            .iter()
                            .map(|glyph| (run.font, glyph.glyph_id, glyph.cluster))
                    })
                    .collect();
                if !ellipsis {
                    assert!(!painted_glyphs.is_empty());
                    full_glyphs = painted_glyphs;
                    continue;
                }
                if width == natural {
                    assert_eq!(
                        painted_glyphs, full_glyphs,
                        "exact-fit paint must retain every original glyph"
                    );
                    assert!(
                        (painted_width - natural).abs() < 0.01,
                        "exact-fit paint changed {content:?}: {painted_width} vs {natural}"
                    );
                } else {
                    assert_ne!(
                        painted_glyphs, full_glyphs,
                        "narrow paint must replace the text tail"
                    );
                    assert!(
                        painted_width <= width + 0.5 && painted_width < natural,
                        "narrow paint must truncate {content:?}"
                    );
                }
            }
        }
    }

    /// An animated font axis interns a new coordinate set every frame. Past
    /// the ceiling the coldest sets go, one by one: the glyph caches of every
    /// other text stay, a set in use keeps its id, and no id is reissued.
    #[test]
    fn variation_ids_are_bounded_by_evicting_the_coldest_sets() {
        use raster::VARIATION_CAP;

        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        pipeline.begin_frame([64, 64]);
        let Some(face) = ({
            let engine = crate::text_engine::lock_engine(&pipeline.engine);
            engine.fonts().faces().into_iter().next()
        }) else {
            return;
        };
        let intern = |pipeline: &mut TextPipeline, value: usize| {
            pipeline
                .rasterizer
                .intern_instance(&nana_text::font::FontInstanceKey {
                    font: face,
                    coords: vec![nana_text::font::AxisCoord {
                        tag: *b"wdth",
                        value: value as f32,
                    }]
                    .into(),
                    synthesis: nana_text::font::Synthesis::default(),
                })
                .1
        };
        let first = intern(&mut pipeline, 0);
        for value in 1..VARIATION_CAP {
            intern(&mut pipeline, value);
        }
        let epoch = pipeline.raster.generation();
        for _ in 0..4 {
            pipeline.begin_frame([64, 64]);
        }
        // The first set stays warm; everything else from that frame is cold.
        assert_eq!(intern(&mut pipeline, 0), first);
        let newest = intern(&mut pipeline, VARIATION_CAP);
        pipeline.begin_frame([64, 64]);
        assert!(pipeline.rasterizer.variation_count() <= VARIATION_CAP);
        assert_eq!(
            pipeline.raster.generation(),
            epoch,
            "no glyph cache starts over"
        );
        assert_eq!(intern(&mut pipeline, 0), first, "a set in use keeps its id");
        assert_eq!(intern(&mut pipeline, VARIATION_CAP), newest);
        let again = intern(&mut pipeline, 1);
        assert!(
            again.0 > newest.0,
            "an evicted set comes back under a new id"
        );
    }

    #[test]
    fn rtl_latin_in_wide_box_places_first_glyph_on_the_right() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        pipeline.begin_frame([256, 64]);
        let bounds = LogicalRect::from_xywh(0.0, 0.0, 200.0, 24.0);
        let clip = LogicalRect::from_xywh(0.0, 0.0, 200.0, 24.0);
        let opentype = SceneTextOpenType {
            direction: nana_ui_core::DirSpec::Rtl,
            ..SceneTextOpenType::default()
        };
        pipeline
            .prepare(
                &device,
                bounds,
                clip,
                1.0,
                "Hello",
                Some([1.0, 1.0, 1.0, 1.0]),
                16.0,
                None,
                None,
                None,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Advanced,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &opentype,
                clip::IDENTITY_AFFINE,
                [0.0, 0.0],
                clip::FragmentClip::PASS,
                1.0,
                [0.0, 0.0],
                EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                },
                UNTRACKED_REVISION,
                None,
                true,
            )
            .expect("rtl latin must prepare");
        let layout = pipeline
            .shape_cache
            .entries
            .values()
            .next()
            .map(|entry| Arc::clone(&entry.layout))
            .expect("paint must cache the laid-out paragraph");
        let glyph_x = layout
            .runs
            .iter()
            .flat_map(|run| run.glyph_cells())
            .map(|(left, _)| left)
            .fold(f32::INFINITY, f32::min);
        assert!(
            glyph_x.is_finite(),
            "rtl latin must lay out a content glyph"
        );
        assert!(
            glyph_x > 100.0,
            "first Latin glyph must sit on the right of a 200px RTL box, got {glyph_x}"
        );
    }

    #[test]
    fn text_box_origin_keeps_left_edge_and_centers_vertically() {
        let bounds = LogicalRect::from_xywh(10.0, 20.0, 100.0, 40.0);
        assert_eq!(
            text_box_origin(bounds, TextVerticalAlignment::Top, 12.0),
            [10.0, 20.0]
        );
        assert_eq!(
            text_box_origin(bounds, TextVerticalAlignment::Center, 12.0),
            [10.0, 34.0]
        );
        assert_eq!(
            text_box_origin(bounds, TextVerticalAlignment::Bottom, 12.0),
            [10.0, 48.0]
        );
    }

    #[test]
    fn perspective_text_foreshortens_the_far_edge() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut pipeline = TextPipeline::new(&device, &queue, format);
        let mat = nana_ui_core::PaintMat4::perspective(120.0)
            .expect("finite depth")
            .then(nana_ui_core::PaintMat4::rotate_y(45_f32.to_radians()))
            .around_origin(0.0, 0.0, 64.0, 64.0);
        let (affine, persp) = mat.planar_homography().expect("homography");
        let pixels = paint_block(
            &device,
            &queue,
            &mut pipeline,
            LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0),
            LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0),
            affine,
            persp,
            clip::FragmentClip::PASS,
        );
        let ink = ink_aabb(&pixels, 64, 64).expect("perspective text must paint");
        let column_height = |x: u32| {
            let mut top = None;
            let mut bottom = 0;
            for y in 0..64u32 {
                if inked(pixel(&pixels, 64, x, y)) {
                    top.get_or_insert(y);
                    bottom = y;
                }
            }
            top.map(|top| bottom - top + 1)
        };
        // The tallest column in each outer quarter of the ink: the outermost
        // columns alone are the edge's one-pixel ramp.
        let quarter = (ink.2 - ink.0) / 4;
        let near = (ink.0..=ink.0 + quarter)
            .filter_map(column_height)
            .max()
            .expect("a near column");
        let far = (ink.2 - quarter..=ink.2)
            .filter_map(column_height)
            .max()
            .expect("a far column");
        assert_ne!(
            near, far,
            "a perspective homography must foreshorten one edge of the text, \
             near={near} far={far}"
        );
    }

    #[test]
    fn rotated_text_prepare_vertices_are_not_the_unrotated_aabb() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut pipeline = TextPipeline::new(&device, &queue, format);
        let bounds = LogicalRect::from_xywh(8.0, 16.0, 48.0, 20.0);
        let clip = LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0);
        let identity = clip::IDENTITY_AFFINE;
        // x' = -y + 40, y' = x keeps a 90° rotation on-screen.
        let rot90 = [0.0, 1.0, -1.0, 0.0, 40.0, 0.0];
        let identity_pixels = paint_text(
            &device,
            &queue,
            &mut pipeline,
            bounds,
            clip,
            identity,
            [0.0, 0.0],
            clip::FragmentClip::PASS,
        );
        let rotated_pixels = paint_text(
            &device,
            &queue,
            &mut pipeline,
            bounds,
            clip,
            rot90,
            [0.0, 0.0],
            clip::FragmentClip::PASS,
        );
        let identity_ink = ink_aabb(&identity_pixels, 64, 64).expect("unrotated text must paint");
        let rotated_ink = ink_aabb(&rotated_pixels, 64, 64).expect("rotated text must paint");
        assert_ne!(
            identity_ink, rotated_ink,
            "rotated glyphs must not occupy the unrotated AABB"
        );
        let identity_w = identity_ink.2 - identity_ink.0 + 1;
        let identity_h = identity_ink.3 - identity_ink.1 + 1;
        let rotated_w = rotated_ink.2 - rotated_ink.0 + 1;
        let rotated_h = rotated_ink.3 - rotated_ink.1 + 1;
        assert!(
            identity_w > identity_h,
            "unrotated 'Hi' ink must be wide, got {identity_ink:?}"
        );
        assert!(
            rotated_h > rotated_w,
            "90° glyph quads must paint a tall AABB, not a translated wide run, got {rotated_ink:?}"
        );
    }

    #[test]
    fn rotated_clip_discards_affine_glyph_in_aabb_outside_rect() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut pipeline = TextPipeline::new(&device, &queue, format);
        let bounds = LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0);
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let clips = [nana_ui_scene::ClipRegion {
            bounds: nana_ui_scene::SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            transform: nana_ui_scene::AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    ..nana_ui_core::PaintTransform::default()
                }
                .around_center(16.0, 16.0, 32.0, 32.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        let origin = clip::PaintOrigin::from(clip::paint_origin([0.0, 0.0], [0.0, 0.0]));
        let aabb = clip::intersect_clips(
            LogicalRect::viewport([0.0, 0.0], [64.0, 64.0]),
            &clips,
            origin,
        )
        .unwrap();
        let frag = clip::fragment_clip(&clips, origin);
        let unclipped = paint_block(
            &device,
            &queue,
            &mut pipeline,
            bounds,
            LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0),
            clip::IDENTITY_AFFINE,
            [0.0, 0.0],
            clip::FragmentClip::PASS,
        );
        let clipped = paint_block(
            &device,
            &queue,
            &mut pipeline,
            bounds,
            aabb,
            clip::IDENTITY_AFFINE,
            [0.0, 0.0],
            frag,
        );
        let mut probe = None;
        for y in 0..64u32 {
            for x in 0..64u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if px >= aabb.x
                    && py >= aabb.y
                    && px < aabb.x + aabb.width
                    && py < aabb.y + aabb.height
                    && !clip::point_in_fragment_clip(px, py, frag)
                    && inked(pixel(&unclipped, 64, x, y))
                {
                    probe = Some((x, y));
                    break;
                }
            }
            if probe.is_some() {
                break;
            }
        }
        let (probe_x, probe_y) =
            probe.expect("unclipped glyphs must ink a pixel in AABB-outside-rotated-rect");
        let probe_clipped = pixel(&clipped, 64, probe_x, probe_y);
        let mut inside = false;
        for y in 0..64u32 {
            for x in 0..64u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if clip::point_in_fragment_clip(px, py, frag) && inked(pixel(&clipped, 64, x, y)) {
                    inside = true;
                    break;
                }
            }
            if inside {
                break;
            }
        }
        assert!(
            !inked(probe_clipped),
            "affine glyphs must discard AABB-outside-rotated-rect, pixel ({probe_x},{probe_y})={probe_clipped:?}"
        );
        assert!(inside, "rotated clip interior must still paint the glyph");
    }

    /// `white-space: normal` folds an authored newline into a space. Runtime
    /// measures that way, so the painter has to lay it out that way: a
    /// paragraph measured as one line and painted as two overflows its box.
    #[test]
    fn an_authored_newline_only_breaks_the_line_when_the_scene_says_it_does() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let laid_out = |pipeline: &mut TextPipeline, preserve_lines: bool| {
            let layout = pipeline.lay_out(
                "first\nsecond",
                None,
                None,
                16.0,
                20.0,
                0.0,
                false,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                None,
                400.0,
                64.0,
                TextAlignSpec::Start,
                &SceneTextOpenType {
                    preserve_lines,
                    ..SceneTextOpenType::default()
                },
            );
            layout.lines.len()
        };
        assert_eq!(
            laid_out(&mut pipeline, false),
            1,
            "`white-space: normal` folds the newline into a space"
        );
        assert_eq!(
            laid_out(&mut pipeline, true),
            2,
            "a preserved newline is a line break"
        );
    }

    /// Lay a paragraph out through the engine the way Runtime would, and hand
    /// it back as the scene's retained handle.
    fn runtime_layout(
        content: &str,
        box_width: f32,
        align: TextAlignSpec,
    ) -> nana_ui_runtime::RetainedTextLayout {
        let source = nana_text::TextSource::new(content);
        let style = nana_text::TextStyle {
            font_size_px: 16.0,
            line_height: Some(LineHeightSpec::Absolute(20.0)),
            ..nana_text::TextStyle::default()
        };
        let constraints = nana_text::TextConstraints {
            max_width_px: Some(box_width),
            align,
            preserve_lines: true,
            ..nana_text::TextConstraints::default()
        };
        let engine = crate::text_engine::nana_text_engine();
        let mut engine = crate::text_engine::lock_engine(&engine);
        let mut counters = nana_text::TextWorkCounters::default();
        let layout = nana_text::TextEngine::layout(
            &mut *engine,
            nana_text::TextKind::Label,
            &source,
            &style,
            &constraints,
            &mut counters,
        );
        nana_ui_runtime::RetainedTextLayout {
            id: layout.id,
            layout,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_with_layout(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        content: &str,
        box_width: f32,
        handle: Option<&nana_ui_runtime::RetainedTextLayout>,
    ) {
        pipeline.begin_frame([512, 64]);
        pipeline.prepare(
            device,
            LogicalRect::from_xywh(0.0, 0.0, box_width, 32.0),
            LogicalRect::from_xywh(0.0, 0.0, 512.0, 64.0),
            1.0,
            content,
            Some([1.0, 1.0, 1.0, 1.0]),
            16.0,
            None,
            None,
            Some(LineHeightSpec::Absolute(20.0)),
            false,
            nana_ui_core::TextWrapBreak::Word,
            false,
            false,
            None,
            TextShaping::Auto,
            TextHorizontalAlignment::Start,
            TextVerticalAlignment::Top,
            &[],
            0.0,
            &[],
            &SceneTextOpenType {
                preserve_lines: true,
                ..SceneTextOpenType::default()
            },
            clip::IDENTITY_AFFINE,
            [0.0; 2],
            clip::FragmentClip::PASS,
            1.0,
            [0.0; 2],
            EntryKey {
                node: 1,
                slot: 0,
                pass: 0,
            },
            UNTRACKED_REVISION,
            handle,
            true,
        );
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
    }

    /// The point of the handle: the paragraph Runtime measured is the one that
    /// is drawn. Nothing is laid out here, and no string is hashed to find
    /// that out.
    #[test]
    fn a_paragraph_the_runtime_retains_is_drawn_without_laying_it_out_again() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let handle = runtime_layout("Retained paragraph", 240.0, TextAlignSpec::Start);
        prepare_with_layout(
            &device,
            &queue,
            &mut pipeline,
            "Retained paragraph",
            240.0,
            Some(&handle),
        );
        let drawn = pipeline.glyph_counters();
        assert!(
            drawn.text_retained_layouts_drawn >= 1,
            "the handle is what identified the paragraph: {drawn:?}"
        );
        assert!(
            drawn.glyph_resolve_requests > 0,
            "and its glyphs were resolved from it"
        );
        let (hits, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            (hits, misses),
            (0, 0),
            "the painter's own paragraph cache is never consulted"
        );
    }

    /// The handle is only usable when the box it was measured into is the box
    /// being painted: a centred line inside a wider box would otherwise be
    /// drawn off-centre. A mismatch falls back rather than misplacing text.
    #[test]
    fn a_layout_measured_into_a_different_box_is_not_drawn_from_its_handle() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let handle = runtime_layout("Centred", 240.0, TextAlignSpec::Center);
        prepare_with_layout(
            &device,
            &queue,
            &mut pipeline,
            "Centred",
            180.0,
            Some(&handle),
        );
        let drawn = pipeline.glyph_counters();
        assert_eq!(
            drawn.text_retained_layouts_drawn, 0,
            "the alignment box is not the paint box, so the handle is refused"
        );
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(misses, 1, "and the painter laid the paragraph out itself");
    }

    /// Paint one rich-text label with the given spans, under a fixed revision.
    fn paint_rich(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        spans: &[SceneTextSpan],
    ) {
        pipeline.begin_frame([256, 64]);
        pipeline.prepare(
            device,
            LogicalRect::from_xywh(0.0, 0.0, 240.0, 32.0),
            LogicalRect::from_xywh(0.0, 0.0, 256.0, 64.0),
            1.0,
            "warm and cold",
            Some([1.0, 1.0, 1.0, 1.0]),
            16.0,
            None,
            None,
            None,
            false,
            nana_ui_core::TextWrapBreak::Word,
            false,
            false,
            None,
            TextShaping::Auto,
            TextHorizontalAlignment::Start,
            TextVerticalAlignment::Top,
            spans,
            0.0,
            &[],
            &SceneTextOpenType::default(),
            clip::IDENTITY_AFFINE,
            [0.0; 2],
            clip::FragmentClip::PASS,
            1.0,
            [0.0; 2],
            EntryKey {
                node: 1,
                slot: 0,
                pass: 0,
            },
            UNTRACKED_REVISION,
            None,
            true,
        );
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
    }

    /// A rich span's colour is not part of the layout any more, so the layout
    /// hash cannot tell two paints of one string apart. The entry has to.
    #[test]
    fn repainting_a_span_in_a_new_color_rebuilds_that_entry_without_relayout() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let span = |color: [f32; 4]| {
            vec![SceneTextSpan {
                start: 0,
                end: "warm".len(),
                color,
            }]
        };
        paint_rich(&device, &queue, &mut pipeline, &span([1.0, 0.0, 0.0, 1.0]));
        let warm = pipeline.glyph_counters();
        let (_, misses_before, _) = pipeline.shape_cache_stats();
        paint_rich(&device, &queue, &mut pipeline, &span([0.0, 0.0, 1.0, 1.0]));
        let recolored = pipeline.glyph_counters();
        let (_, misses_after, _) = pipeline.shape_cache_stats();
        assert_eq!(
            recolored.text_instance_rebuilds - warm.text_instance_rebuilds,
            1,
            "the span paints a different colour, so its instances are resolved again"
        );
        assert_eq!(
            misses_after, misses_before,
            "but colour is not part of the layout, so nothing is laid out again"
        );
        assert_eq!(
            recolored.glyph_rasterized, warm.glyph_rasterized,
            "nor rasterized: the bitmaps never depended on the colour"
        );

        // And repainting the same spans is still a steady frame.
        paint_rich(&device, &queue, &mut pipeline, &span([0.0, 0.0, 1.0, 1.0]));
        let steady = pipeline.glyph_counters();
        assert_eq!(
            steady.text_instance_rebuilds, recolored.text_instance_rebuilds,
            "an unchanged rich repaint resolves nothing"
        );
    }

    /// Prepare one label, flush it and upload, reporting the GPU work.
    #[allow(clippy::too_many_arguments)]
    fn prepare_label(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        content: &str,
        scale: f32,
    ) -> nana_ui_core::GpuWorkObservation {
        pipeline.begin_frame([256, 64]);
        pipeline
            .prepare(
                device,
                LogicalRect::from_xywh(0.0, 0.0, 240.0, 32.0),
                LogicalRect::from_xywh(0.0, 0.0, 256.0, 64.0),
                scale,
                content,
                Some([1.0, 1.0, 1.0, 1.0]),
                16.0,
                None,
                None,
                None,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Auto,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &SceneTextOpenType::default(),
                clip::IDENTITY_AFFINE,
                [0.0; 2],
                clip::FragmentClip::PASS,
                1.0,
                [0.0; 2],
                EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                },
                UNTRACKED_REVISION,
                None,
                true,
            )
            .expect("label must prepare");
        pipeline.flush_runs();
        let work = crate::gpu_work::GpuWorkSink::new();
        pipeline.upload(device, queue, Some(&work));
        work.snapshot()
    }

    /// Place one hand-made glyph and return its handle, so a batching test can
    /// mix mask and color glyphs without depending on which faces this machine
    /// happens to have.
    fn place(
        pipeline: &mut TextPipeline,
        device: &wgpu::Device,
        glyph: u32,
        format: raster::GlyphImageFormat,
    ) -> atlas::GlyphAtlasEntryId {
        let key = glyph::GlyphRasterKey {
            font: glyph::GlyphFontId(0),
            font_generation: 0,
            variation: glyph::GlyphVariationId(0),
            glyph,
            size_bits: 16f32.to_bits(),
            subpixel_x: glyph::SubpixelBin::default(),
            subpixel_y: glyph::SubpixelBin::default(),
            synthesis: glyph::GlyphSynthesis::NONE,
            mode: GlyphRenderMode::Mask,
        };
        let image = std::sync::Arc::new(raster::GlyphImage {
            format,
            width: 4,
            height: 6,
            left: 0,
            top: 6,
            data: vec![255; 4 * 6 * format.bytes_per_pixel()],
        });
        let TextPipeline {
            atlas,
            raster,
            uploads,
            ..
        } = pipeline;
        atlas
            .insert(device, key, &image, raster, uploads)
            .expect("an empty atlas must place one glyph")
            .0
    }

    /// Open a run over already-placed glyphs, the way `build_entry` would.
    fn hand_built_run(pipeline: &mut TextPipeline, handles: &[atlas::GlyphAtlasEntryId]) -> usize {
        let index = pipeline.target.live_runs;
        let node = index as u64 + 1;
        let placeholders = [
            pipeline.atlas.placeholder_page(AtlasPageKind::Mask),
            pipeline.atlas.placeholder_page(AtlasPageKind::Color),
        ];
        let key = EntryKey {
            node,
            slot: 0,
            pass: 0,
        };
        let atlas = &mut pipeline.atlas;
        let id = pipeline
            .target
            .entries
            .begin_build(key, handles.len() as u32, |handle| atlas.release(handle));
        let mut segments: Vec<EntrySegment> = Vec::new();
        for (offset, handle) in handles.iter().enumerate() {
            let placement = *pipeline.atlas.entry(*handle).expect("just placed");
            let content = match placement.kind {
                AtlasPageKind::Mask => CONTENT_MASK,
                AtlasPageKind::Color => CONTENT_COLOR,
            };
            let (mask_page, color_page) = match placement.kind {
                AtlasPageKind::Mask => (placement.page, placeholders[1]),
                AtlasPageKind::Color => (placeholders[0], placement.page),
            };
            push_entry_segment(
                &mut segments,
                placeholders,
                mask_page,
                color_page,
                offset as u32,
            );
            pipeline.atlas.retain(*handle);
            pipeline.target.entries.push_glyph(
                id,
                offset as u32,
                *handle,
                GlyphInstance::new(
                    [offset as i32 * 6, 0],
                    placement.size,
                    placement.origin,
                    pipeline::pack_srgb([1.0; 4]),
                    content,
                ),
            );
        }
        let epoch = pipeline.atlas.placement_epoch();
        let entry = pipeline.target.entries.get_mut(id).expect("just built");
        entry.atlas_epoch = epoch;
        entry.segments = segments;
        if index == pipeline.target.runs.len() {
            pipeline.target.runs.push(TextRun {
                entry: id,
                presentation: RunPresentation {
                    origin: [0.0; 2],
                    color: [1.0; 4],
                    opacity: 1.0,
                    flags: 0,
                    presentation: 0,
                    raster: 1.0,
                },
                next: NO_RUN,
                last: index as u32,
                folded: false,
                segments: 0..0,
            });
        }
        pipeline.target.runs[index].entry = id;
        pipeline.target.runs[index].next = NO_RUN;
        pipeline.target.runs[index].last = index as u32;
        pipeline.target.runs[index].folded = false;
        pipeline.target.live_runs += 1;
        index
    }

    #[test]
    fn a_color_glyph_between_mask_glyphs_still_draws_as_one_batch() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        pipeline.begin_frame([64, 64]);
        let handles = [
            place(&mut pipeline, &device, 1, raster::GlyphImageFormat::Mask),
            place(
                &mut pipeline,
                &device,
                2,
                raster::GlyphImageFormat::ColorRgba,
            ),
            place(&mut pipeline, &device, 3, raster::GlyphImageFormat::Mask),
        ];
        let run = hand_built_run(&mut pipeline, &handles);
        pipeline.flush_runs();
        assert_eq!(
            pipeline.target.runs[run].segments.len(),
            1,
            "a mask page and a color page fit one bind group, so an emoji in a \
             line of text must not split the batch"
        );
        let segment = pipeline.target.segments[0];
        assert_eq!(segment.count, 3);
        assert_ne!(
            segment.color_page,
            pipeline.atlas.placeholder_page(AtlasPageKind::Color),
            "the one segment must name the real color page, not the placeholder"
        );
        assert_ne!(
            segment.mask_page,
            pipeline.atlas.placeholder_page(AtlasPageKind::Mask),
            "and the real mask page"
        );
    }

    #[test]
    fn two_paragraphs_folded_into_one_command_draw_as_one_segment() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        pipeline.begin_frame([64, 64]);
        let first = [place(
            &mut pipeline,
            &device,
            1,
            raster::GlyphImageFormat::Mask,
        )];
        let second = [place(
            &mut pipeline,
            &device,
            2,
            raster::GlyphImageFormat::Mask,
        )];
        let a = hand_built_run(&mut pipeline, &first);
        let b = hand_built_run(&mut pipeline, &second);
        // Different origin, different color, different opacity: none of that
        // is a batch key any more, because each instance names its own run.
        pipeline.target.runs[b].presentation.origin = [40.0, 12.0];
        pipeline.target.runs[b].presentation.color = [1.0, 0.0, 0.0, 1.0];
        pipeline.target.runs[b].presentation.opacity = 0.5;
        let previous = PreparedText {
            index: a,
            ink: LogicalRect::from_xywh(0.0, 0.0, 1.0, 1.0),
        };
        let next = PreparedText {
            index: b,
            ink: LogicalRect::from_xywh(0.0, 0.0, 1.0, 1.0),
        };
        assert!(pipeline.can_merge_runs(&previous, &next));
        pipeline.merge_runs(&previous, &next);
        pipeline.flush_runs();
        assert_eq!(
            pipeline.target.runs[a].segments.len(),
            1,
            "two paragraphs that only present differently must stay one draw"
        );
        let a_capacity = pipeline
            .target
            .entries
            .get(pipeline.target.runs[a].entry)
            .expect("entry")
            .capacity;
        assert_eq!(
            pipeline.target.segments[0],
            pipeline::DrawSegment {
                mask_page: pipeline.target.segments[0].mask_page,
                color_page: pipeline.target.segments[0].color_page,
                first: 0,
                count: a_capacity + 1,
            },
            "the one draw spans both blocks, slack included"
        );
        assert_eq!(
            pipeline.target.run_table.len(),
            2,
            "but each keeps its own presentation row"
        );
    }

    /// One label's presentation, so a gate can change exactly one thing.
    #[derive(Clone, Copy)]
    struct Label<'a> {
        content: &'a str,
        key: EntryKey,
        color: [f32; 4],
        opacity: f32,
        affine: [f32; 6],
        top: f32,
        left: f32,
    }

    impl<'a> Label<'a> {
        fn new(content: &'a str, node: u64) -> Self {
            Self {
                content,
                key: EntryKey {
                    node,
                    slot: 0,
                    pass: 0,
                },
                color: [1.0, 1.0, 1.0, 1.0],
                opacity: 1.0,
                affine: clip::IDENTITY_AFFINE,
                top: 0.0,
                left: 0.0,
            }
        }
    }

    /// Prepare, flush and upload one frame of `labels`.
    fn text_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        labels: &[Label<'_>],
    ) {
        text_frame_at(device, queue, pipeline, labels, 1.0);
    }

    /// [`text_frame`] on a display of `scale` physical px per logical px.
    fn text_frame_at(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        labels: &[Label<'_>],
        scale: f32,
    ) {
        pipeline.begin_frame([512, 256]);
        for label in labels {
            pipeline.prepare(
                device,
                LogicalRect::from_xywh(label.left, label.top, 480.0, 32.0),
                LogicalRect::from_xywh(0.0, 0.0, 512.0, 256.0),
                scale,
                label.content,
                Some(label.color),
                16.0,
                None,
                None,
                None,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Auto,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &SceneTextOpenType::default(),
                label.affine,
                [0.0; 2],
                clip::FragmentClip::PASS,
                label.opacity,
                [0.0; 2],
                label.key,
                UNTRACKED_REVISION,
                None,
                true,
            );
        }
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
    }

    #[test]
    fn more_distinct_paragraphs_than_the_cache_holds_still_shape_once_each() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // One more label than the cache's nominal capacity, all on screen at
        // once, behind a label that changes every frame. That first insert is
        // what a fixed capacity answers by evicting the rows it is about to
        // draw — and then it reshapes every one of them, every frame.
        let contents = (0..SHAPE_CACHE_CAP + 1)
            .map(|index| format!("Distinct row {index}"))
            .collect::<Vec<_>>();
        let frame = |pipeline: &mut TextPipeline, tick: usize| {
            let ticking = format!("tick {tick}");
            let mut labels = vec![Label::new(ticking.as_str(), 1)];
            labels.extend(contents.iter().enumerate().map(|(index, content)| Label {
                top: index as f32 * 2.0,
                ..Label::new(content.as_str(), index as u64 + 2)
            }));
            text_frame(&device, &queue, pipeline, &labels);
        };
        frame(&mut pipeline, 0);
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            warm_misses,
            contents.len() + 1,
            "the first frame shapes each paragraph once"
        );
        for tick in 1..4 {
            frame(&mut pipeline, tick);
        }
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses - warm_misses,
            3,
            "only the label that really changed is reshaped; the cache has to \
             hold one frame's worth of text, which is a property of the view"
        );
    }

    #[test]
    fn a_face_set_change_gives_back_the_blocks_and_rows_its_entries_held() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let labels = (0..12)
            .map(|index| Label {
                top: index as f32 * 18.0,
                ..Label::new("Face set row", index + 1)
            })
            .collect::<Vec<_>>();
        text_frame(&device, &queue, &mut pipeline, &labels);
        let rows = pipeline.target.run_table.len();
        let slots = pipeline.target.arena.len();
        assert_eq!(rows, labels.len());
        assert!(slots > 0);
        // What `@font-face` does: every entry stops meaning what it meant.
        pipeline.drop_every_entry();
        text_frame(&device, &queue, &mut pipeline, &labels);
        assert_eq!(
            pipeline.target.run_table.len(),
            rows,
            "the rows the dropped entries held are handed out again, not added to"
        );
        assert_eq!(
            pipeline.target.arena.len(),
            slots,
            "and so are their arena blocks"
        );
        assert_eq!(pipeline.glyph_counters().text_gpu_entries_active, 12);
    }

    #[test]
    fn a_paragraph_that_stopped_being_drawn_is_eventually_given_back() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let keep = Label::new("Still here", 1);
        let gone = Label {
            top: 40.0,
            ..Label::new("Closed panel", 2)
        };
        text_frame(&device, &queue, &mut pipeline, &[keep, gone]);
        let warm = pipeline.glyph_counters();
        assert_eq!(warm.text_gpu_entries_active, 2);
        assert_eq!(warm.text_gpu_entries_destroyed, 0);
        // The panel closes. Long enough that a tab flipped back and forth
        // would have paid for neither direction, and then long enough that
        // this one is really gone.
        for _ in 0..RETIRE_AFTER_FRAMES + RETIRE_INTERVAL {
            text_frame(&device, &queue, &mut pipeline, &[keep]);
        }
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_gpu_entries_active, 1,
            "the entry of a paragraph nothing draws any more is given back"
        );
        assert_eq!(after.text_gpu_entries_destroyed, 1);
        assert!(
            after.text_gpu_entry_glyphs < warm.text_gpu_entry_glyphs,
            "and so are its instances"
        );
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "the paragraph that stayed was never resolved again"
        );
    }

    /// A shape key for `content` with everything else at its default.
    fn shape_key_ref(content: &str) -> ShapeKeyRef<'_> {
        ShapeKeyRef {
            content,
            family: None,
            weight: None,
            font_size_bits: 16f32.to_bits(),
            line_height_bits: 20f32.to_bits(),
            wrap: false,
            wrap_break: nana_ui_core::TextWrapBreak::Word,
            italic: false,
            ellipsis: false,
            max_lines: None,
            letter_spacing_bits: 0f32.to_bits(),
            word_break: 0,
            line_break: 0,
            kerning: 0,
            features: &[],
            variations: &[],
            width_bits: 100f32.to_bits(),
            height_bits: 20f32.to_bits(),
            align: 0,
            direction: 0,
            writing_mode: 0,
            text_orientation: 0,
            preserve_lines: false,
            font_features: &[],
        }
    }

    #[test]
    fn a_shape_hash_collision_reshapes_instead_of_painting_the_other_text() {
        // The hash finds the entry; the stored key decides the hit. A weak
        // hash may therefore cost a reshape, and may never paint the wrong
        // letters — which is the whole reason the key is kept at all.
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let layout = pipeline.lay_out(
            "first paragraph",
            None,
            None,
            16.0,
            20.0,
            0.0,
            false,
            false,
            nana_ui_core::TextWrapBreak::Word,
            false,
            None,
            100.0,
            20.0,
            TextAlignSpec::Start,
            &SceneTextOpenType::default(),
        );
        let collision = 0x5ca1_ab1e_u64;
        pipeline.shape_cache.insert(
            collision,
            shape_key_ref("first paragraph").to_owned_key(),
            layout,
        );
        let (hits, misses, _) = pipeline.shape_cache_stats();
        assert!(
            pipeline
                .shape_cache
                .get(collision, &shape_key_ref("first paragraph"))
                .is_some(),
            "its own key still hits"
        );
        assert!(
            pipeline
                .shape_cache
                .get(collision, &shape_key_ref("a different paragraph"))
                .is_none(),
            "another paragraph at the same hash must miss, not read that buffer"
        );
        let (after_hits, after_misses, _) = pipeline.shape_cache_stats();
        assert_eq!(after_hits, hits + 1);
        assert_eq!(after_misses, misses + 1);
    }

    #[test]
    fn two_windows_taking_turns_keep_each_others_paragraphs_shaped() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut second: Option<TextPipelineTarget> = None;
        // Each window alone fits the cache; together they do not. What window
        // A drew last frame is still on A's screen while B is drawing.
        let half = SHAPE_CACHE_CAP * 2 / 3;
        let window = |offset: usize| {
            (0..half)
                .map(|index| format!("Window {offset} row {index}"))
                .collect::<Vec<_>>()
        };
        let (left, right) = (window(0), window(1));
        let paint = |pipeline: &mut TextPipeline, contents: &[String]| {
            let labels = contents
                .iter()
                .enumerate()
                .map(|(index, content)| Label {
                    top: index as f32 * 2.0,
                    ..Label::new(content.as_str(), index as u64 + 1)
                })
                .collect::<Vec<_>>();
            text_frame(&device, &queue, pipeline, &labels);
        };
        paint(&mut pipeline, &left);
        pipeline.swap_target(&mut second, &device);
        paint(&mut pipeline, &right);
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        for _ in 0..3 {
            pipeline.swap_target(&mut second, &device);
            paint(&mut pipeline, &left);
            pipeline.swap_target(&mut second, &device);
            paint(&mut pipeline, &right);
        }
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses, warm_misses,
            "neither window may evict the other's text: both are on screen"
        );
    }

    #[test]
    fn a_scrolled_viewport_paints_the_text_its_offset_brought_into_view() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // The node sits below the viewport in its own space; the scroll offset
        // lives in the transform, exactly as `paint_transform` folds a scene
        // origin into it. Comparing the two spaces directly would drop it.
        let scrolled = [1.0, 0.0, 0.0, 1.0, 0.0, -120.0];
        let pixels = paint_text(
            &device,
            &queue,
            &mut pipeline,
            LogicalRect::from_xywh(4.0, 130.0, 48.0, 20.0),
            LogicalRect::from_xywh(0.0, 0.0, 64.0, 64.0),
            scrolled,
            [0.0, 0.0],
            clip::FragmentClip::PASS,
        );
        assert!(
            ink_aabb(&pixels, 64, 64).is_some(),
            "text the scroll offset brought into view must still paint"
        );
    }

    #[test]
    fn a_static_steady_frame_resolves_no_glyph_and_moves_no_instance() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let labels = (0..8)
            .map(|index| Label {
                top: index as f32 * 24.0,
                ..Label::new("Steady label", index + 1)
            })
            .collect::<Vec<_>>();
        text_frame(&device, &queue, &mut pipeline, &labels);
        let warm = pipeline.glyph_counters();
        for _ in 0..3 {
            text_frame(&device, &queue, &mut pipeline, &labels);
        }
        let steady = pipeline.glyph_counters();
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses, 1,
            "one paragraph is shaped once, however many nodes draw it"
        );
        assert_eq!(
            steady.glyph_resolve_requests, warm.glyph_resolve_requests,
            "a static frame must not resolve a glyph"
        );
        assert_eq!(
            steady.glyph_rasterized, warm.glyph_rasterized,
            "nor rasterize one"
        );
        assert_eq!(
            steady.glyph_upload_bytes, warm.glyph_upload_bytes,
            "nor upload an atlas region"
        );
        assert_eq!(
            steady.text_instance_rebuilds, warm.text_instance_rebuilds,
            "nor rebuild an entry"
        );
        assert_eq!(
            steady.text_instance_patches, warm.text_instance_patches,
            "nor patch one"
        );
        assert_eq!(
            steady.text_instance_upload_bytes, warm.text_instance_upload_bytes,
            "nor move an instance byte"
        );
        assert_eq!(
            steady.text_presentation_upload_bytes, warm.text_presentation_upload_bytes,
            "nor a presentation byte"
        );
        assert_eq!(steady.text_gpu_entries_active, 8);
        assert_eq!(
            steady.text_prepare_nodes_skipped - warm.text_prepare_nodes_skipped,
            24,
            "every node of every steady frame is answered by its entry"
        );
    }

    #[test]
    fn recoloring_and_fading_are_presentation_only() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let label = Label::new("Recolor me", 1);
        text_frame(&device, &queue, &mut pipeline, &[label]);
        let warm = pipeline.glyph_counters();
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        for (index, color) in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]
            .into_iter()
            .enumerate()
        {
            text_frame(
                &device,
                &queue,
                &mut pipeline,
                &[Label {
                    color,
                    opacity: 1.0 - index as f32 * 0.25,
                    ..label
                }],
            );
        }
        let after = pipeline.glyph_counters();
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(misses, warm_misses, "a color or a fade must not reshape");
        assert_eq!(
            after.glyph_resolve_requests, warm.glyph_resolve_requests,
            "nor resolve a glyph"
        );
        assert_eq!(
            after.glyph_rasterized, warm.glyph_rasterized,
            "nor rasterize one"
        );
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "nor rebuild the entry"
        );
        assert_eq!(
            after.text_instance_upload_bytes, warm.text_instance_upload_bytes,
            "nor move an instance byte"
        );
        assert!(
            after.text_presentation_upload_bytes > warm.text_presentation_upload_bytes,
            "the new color and opacity reach the GPU as run rows"
        );
    }

    #[test]
    fn a_transform_animation_does_not_rebuild_text_instances() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let label = Label::new("Spinning", 1);
        let rotated = |angle: f32| {
            let (sin, cos) = angle.sin_cos();
            [cos, sin, -sin, cos, 24.0, 24.0]
        };
        text_frame(
            &device,
            &queue,
            &mut pipeline,
            &[Label {
                affine: rotated(0.1),
                ..label
            }],
        );
        let warm = pipeline.glyph_counters();
        for step in 1..8 {
            text_frame(
                &device,
                &queue,
                &mut pipeline,
                &[Label {
                    affine: rotated(0.1 + step as f32 * 0.02),
                    ..label
                }],
            );
        }
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "a rotation is presentation: the glyphs it turns are the same ones"
        );
        assert_eq!(
            after.glyph_rasterized, warm.glyph_rasterized,
            "and no angle is a new bitmap"
        );
        assert_eq!(
            after.text_instance_upload_bytes, warm.text_instance_upload_bytes,
            "nor an instance byte"
        );
        assert!(
            after.text_presentation_upload_bytes > warm.text_presentation_upload_bytes,
            "each angle is a presentation row"
        );
    }

    #[test]
    fn labels_coming_and_going_do_not_renumber_the_ones_that_stayed() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let all = (0..12)
            .map(|index| Label {
                top: index as f32 * 18.0,
                ..Label::new("Row content", index + 1)
            })
            .collect::<Vec<_>>();
        text_frame(&device, &queue, &mut pipeline, &all);
        text_frame(&device, &queue, &mut pipeline, &all);
        let warm = pipeline.glyph_counters();
        // The first label leaves. If a run row were the draw-order index, every
        // remaining label would be renumbered and every instance of every one
        // of them would have to be rewritten.
        text_frame(&device, &queue, &mut pipeline, &all[1..]);
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "nothing was reshaped or re-resolved"
        );
        assert_eq!(
            after.text_instance_patches, warm.text_instance_patches,
            "and no surviving label had its run rebound"
        );
    }

    #[test]
    fn a_scene_scale_animation_rasterizes_each_step_it_passes_once_not_each_frame() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let label = Label::new("Zooming", 1);
        let scaled = |factor: f32| [factor, 0.0, 0.0, factor, 8.0, 8.0];
        text_frame(
            &device,
            &queue,
            &mut pipeline,
            &[Label {
                affine: scaled(1.0),
                ..label
            }],
        );
        let warm = pipeline.glyph_counters();
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        let glyphs = warm.glyph_rasterized;
        // Twenty-three frames from 1.0 to 2.1: a zoom animation that passes
        // two raster steps (√2 and 2) on its way.
        for step in 1..24 {
            text_frame(
                &device,
                &queue,
                &mut pipeline,
                &[Label {
                    affine: scaled(1.0 + step as f32 * 0.05),
                    ..label
                }],
            );
        }
        let after = pipeline.glyph_counters();
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses, warm_misses,
            "a scene scale must not lay the paragraph out again"
        );
        // The DPI scale decides the raster size frame by frame; a scene
        // transform only moves it in half-octave steps. Letting the two share
        // a policy is what turns a zoom into a bitmap per frame and a cache
        // that never stops growing.
        // Per step, at most what the first frame rasterized: sub-pixel bins
        // can merge two of a paragraph's glyphs at a larger size.
        let rasterized = after.glyph_rasterized - warm.glyph_rasterized;
        assert!(
            rasterized > glyphs && rasterized <= 2 * glyphs,
            "one raster size per step passed, not one per frame: {rasterized} \
             glyphs for two steps of a {glyphs}-glyph label"
        );
        assert_eq!(
            after.text_instance_rebuilds - warm.text_instance_rebuilds,
            2,
            "and one rebuild per step"
        );
    }

    #[test]
    fn a_whole_pixel_scroll_rewrites_one_shared_row_and_no_run() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let labels = |offset: f32| {
            (0..12)
                .map(|index| Label {
                    top: 3.3 + index as f32 * 17.6,
                    affine: [1.0, 0.0, 0.0, 1.0, 0.0, -offset],
                    ..Label::new("Row content", index + 1)
                })
                .collect::<Vec<_>>()
        };
        text_frame(&device, &queue, &mut pipeline, &labels(0.0));
        for offset in 1..6 {
            let warm = pipeline.glyph_counters();
            text_frame(&device, &queue, &mut pipeline, &labels(offset as f32 * 3.0));
            let after = pipeline.glyph_counters();
            assert_eq!(
                after.text_instance_rebuilds, warm.text_instance_rebuilds,
                "a whole-pixel scroll keeps every glyph"
            );
            let moved = after.text_presentation_upload_bytes - warm.text_presentation_upload_bytes;
            assert!(
                moved <= std::mem::size_of::<pipeline::TextPresentationGpu>() as u64,
                "and moves one presentation row, not twelve run rows: {moved} bytes"
            );
        }
    }

    #[test]
    fn a_whole_pixel_horizontal_scroll_keeps_every_glyph() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // Labels at fractional x, as flex layout leaves them. Scrolled by
        // whole pixels their glyphs are the same glyphs at the same phase, so
        // not one of them may be resolved again — however the float that
        // carries the scroll happens to round.
        let labels = |offset: f32| {
            (0..12)
                .map(|index| Label {
                    left: 0.709 + index as f32 * 38.766,
                    affine: [1.0, 0.0, 0.0, 1.0, -offset, 0.0],
                    ..Label::new("Row", index + 1)
                })
                .collect::<Vec<_>>()
        };
        text_frame(&device, &queue, &mut pipeline, &labels(0.0));
        let warm = pipeline.glyph_counters();
        for step in 1..40 {
            text_frame(&device, &queue, &mut pipeline, &labels(step as f32 * 7.0));
        }
        assert_eq!(
            pipeline.glyph_counters().text_instance_rebuilds,
            warm.text_instance_rebuilds,
            "a whole-pixel horizontal scroll must not re-resolve a label"
        );
    }

    #[test]
    fn a_snapped_translation_reads_as_its_whole_pixels() {
        // The CPU and the vertex stage place a translated run by these whole
        // pixels. At 110%, 120% and 175% a snapped translation comes back
        // through the scale an ulp low for some offsets; read as `floor`, the
        // label would sit a pixel left of its background on those frames.
        for scale in [1.1f32, 1.2, 1.75] {
            for step in 0..20_000 {
                let offset = step as f32 * -2.37;
                let affine =
                    clip::snap_translation([1.0, 0.0, 0.0, 1.0, offset, offset], [0.0; 2], scale);
                let pixels = (offset * scale).round();
                assert_eq!(
                    pipeline::whole_translation(affine, scale),
                    [pixels; 2],
                    "{offset} at {scale}x"
                );
            }
        }
    }

    #[test]
    fn a_fractional_vertical_scroll_keeps_every_glyph() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // A trackpad scrolls by fractions of a pixel. Vertically that is free:
        // a translated line box is snapped to whole pixels, so its glyphs are
        // at the same vertical phase wherever the scroll left it.
        let labels = |offset: f32| {
            (0..12)
                .map(|index| Label {
                    top: 3.3 + index as f32 * 17.6,
                    affine: [1.0, 0.0, 0.0, 1.0, 0.0, -offset],
                    ..Label::new("Row content", index + 1)
                })
                .collect::<Vec<_>>()
        };
        text_frame(&device, &queue, &mut pipeline, &labels(0.0));
        let warm = pipeline.glyph_counters();
        for step in 1..24 {
            text_frame(&device, &queue, &mut pipeline, &labels(step as f32 * 0.37));
        }
        assert_eq!(
            pipeline.glyph_counters().text_instance_rebuilds,
            warm.text_instance_rebuilds,
            "a fractional vertical scroll must not re-resolve a label"
        );
    }

    #[test]
    fn a_fractional_horizontal_scroll_keeps_every_glyph() {
        // What the painter hands text under a trackpad's horizontal scroll:
        // the translation snapped to whole device pixels (#223). At 110%, 120%
        // and 175% — not 125% or 150%, where it is exact — some of those come
        // back through the scale an ulp off the pixel, near the start of the
        // list and more often thousands of pixels into it.
        for scale in [1.0, 1.1, 1.2, 1.25, 1.5, 1.75] {
            for start in [0.0, 4096.0] {
                let (device, queue) = test_device();
                let mut pipeline =
                    TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
                let labels = |offset: f32| {
                    let affine = clip::snap_translation(
                        [1.0, 0.0, 0.0, 1.0, -(start + offset), 0.0],
                        [0.0; 2],
                        scale,
                    );
                    (0..12)
                        .map(|index| Label {
                            left: start + 0.709 + index as f32 * 38.766,
                            top: 3.3 + index as f32 * 17.6,
                            affine,
                            ..Label::new("Row", index + 1)
                        })
                        .collect::<Vec<_>>()
                };
                text_frame_at(&device, &queue, &mut pipeline, &labels(0.0), scale);
                let warm = pipeline.glyph_counters();
                for step in 1..40 {
                    text_frame_at(
                        &device,
                        &queue,
                        &mut pipeline,
                        &labels(step as f32 * 0.37),
                        scale,
                    );
                }
                let after = pipeline.glyph_counters();
                assert_eq!(
                    after.text_instance_rebuilds, warm.text_instance_rebuilds,
                    "a fractional horizontal scroll at {scale}x, {start} px in, \
                     must not re-resolve a label"
                );
                assert!(
                    after.text_presentation_upload_bytes > warm.text_presentation_upload_bytes,
                    "while the labels do move: the shared row is rewritten"
                );
            }
        }
    }

    #[test]
    fn a_container_that_starts_or_stops_turning_keeps_its_labels_glyphs() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let labels = |affine: [f32; 6]| {
            (0..12)
                .map(|index| Label {
                    top: 3.3 + index as f32 * 17.6,
                    affine,
                    ..Label::new("Row content", index + 1)
                })
                .collect::<Vec<_>>()
        };
        let turned = |degrees: f32| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [cos, sin, -sin, cos, 0.0, 0.0]
        };
        let pressed = |factor: f32| [factor, 0.0, 0.0, factor, 0.0, 0.0];
        text_frame(
            &device,
            &queue,
            &mut pipeline,
            &labels(clip::IDENTITY_AFFINE),
        );
        let warm = pipeline.glyph_counters();
        // A wobble through upright, and a press that springs back: the
        // container leaves the identity and returns to it on every pass.
        for affine in [
            turned(2.0),
            turned(-2.0),
            clip::IDENTITY_AFFINE,
            turned(3.0),
            pressed(0.97),
            pressed(0.985),
            clip::IDENTITY_AFFINE,
        ] {
            text_frame(&device, &queue, &mut pipeline, &labels(affine));
        }
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "entering or leaving a transform is presentation: the glyphs are \
             resolved at the same phase either side of it"
        );
        assert_eq!(after.glyph_rasterized, warm.glyph_rasterized);
    }

    #[test]
    fn a_container_with_a_fractional_offset_keeps_its_glyphs_as_it_turns() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // A container scrolled to a fraction of a pixel, starting and stopping
        // a turn. Upright, the painter draws its translation on whole pixels
        // (#223) and the labels' phase is their own position's; turned, the
        // projected path's phase is the same. The turn is presentation.
        let labels = |affine: [f32; 6]| {
            (0..12)
                .map(|index| Label {
                    top: 3.3 + index as f32 * 17.6,
                    left: 0.709,
                    affine,
                    ..Label::new("Row content", index + 1)
                })
                .collect::<Vec<_>>()
        };
        let upright = clip::snap_translation([1.0, 0.0, 0.0, 1.0, 10.3, 0.0], [0.0; 2], 1.0);
        let turned = |degrees: f32| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [cos, sin, -sin, cos, 10.3, 0.0]
        };
        text_frame(&device, &queue, &mut pipeline, &labels(upright));
        let warm = pipeline.glyph_counters();
        for affine in [turned(2.0), upright, turned(-2.0), upright] {
            text_frame(&device, &queue, &mut pipeline, &labels(affine));
        }
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "a fractionally scrolled container entering or leaving a turn keeps its labels"
        );
    }

    #[test]
    fn a_magnification_hovering_at_a_step_boundary_does_not_flip_bitmaps() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let label = Label::new("Hover", 1);
        // √(√2) is exactly half-way between the first two steps.
        let boundary = 2f32.powf(0.25);
        let scaled = |factor: f32| [factor, 0.0, 0.0, factor, 8.0, 8.0];
        text_frame(
            &device,
            &queue,
            &mut pipeline,
            &[Label {
                affine: scaled(boundary * 1.02),
                ..label
            }],
        );
        let warm = pipeline.glyph_counters();
        for frame in 0..20 {
            let jitter = if frame % 2 == 0 { 0.97 } else { 1.03 };
            text_frame(
                &device,
                &queue,
                &mut pipeline,
                &[Label {
                    affine: scaled(boundary * jitter),
                    ..label
                }],
            );
        }
        assert_eq!(
            pipeline.glyph_counters().text_instance_rebuilds,
            warm.text_instance_rebuilds,
            "a scale jittering across a step boundary keeps the step it holds"
        );
    }

    #[test]
    fn text_a_transform_scales_is_rasterized_at_the_size_it_is_seen() {
        let (device, queue) = test_device();
        // The same text scaled and upright at the size it is seen: 16 px under
        // `scale(2)` against 32 px, and 32 px under `scale(0.5)` against 16 px.
        // Drawn from bitmaps of its own size, a magnified glyph would be
        // blurred and a shrunk one's edge half a pixel with aliased stems.
        let canvas = LogicalRect::from_xywh(0.0, 0.0, 256.0, 96.0);
        let paint = |size: f32, factor: f32| {
            let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            paint_text_with(
                &device,
                &queue,
                &mut pipeline,
                ("Sharp", size),
                LogicalRect::from_xywh(0.0, 0.0, size * 7.5, size * 2.5),
                canvas,
                [factor, 0.0, 0.0, factor, 0.0, 0.0],
                [0.0; 2],
                clip::FragmentClip::PASS,
                [256, 96],
            )
        };
        for (size, factor) in [(16.0, 2.0), (32.0, 0.5)] {
            let (scaled, upright) = (paint(size, factor), paint(size * factor, 1.0));
            let (scaled, upright) = (scaled.as_chunks::<4>().0, upright.as_chunks::<4>().0);
            let differing = scaled
                .iter()
                .zip(upright)
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24))
                .count();
            let inked = upright.iter().filter(|pixel| inked(**pixel)).count();
            assert!(inked > 100, "the reference must paint something");
            assert!(
                differing * 20 < inked,
                "{size} px text under scale({factor}) must be the {} px glyphs: \
                 {differing} of {inked} inked pixels differ",
                size * factor
            );
        }
    }

    #[test]
    fn text_squeezed_along_one_axis_keeps_its_ink_at_any_sub_pixel_phase() {
        // `scale(1, 0.3)` rasterizes for the larger scale and shrinks the
        // other axis 3.3 times: one bilinear tap per pixel skips rows, and
        // which ones depends on the sub-pixel phase. Filtered over the
        // pixel's footprint, the ink a paragraph lays down does not.
        let (device, queue) = test_device();
        let canvas = LogicalRect::from_xywh(0.0, 0.0, 256.0, 96.0);
        let ink = |phase: f32| {
            let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            let pixels = paint_text_with(
                &device,
                &queue,
                &mut pipeline,
                ("Sharp lines", 24.0),
                LogicalRect::from_xywh(0.0, 0.0, 200.0, 40.0),
                canvas,
                [1.0, 0.0, 0.0, 0.3, 8.0, 16.0 + phase],
                [0.0; 2],
                clip::FragmentClip::PASS,
                [256, 96],
            );
            pixels
                .chunks(4)
                .map(|pixel| u64::from(pixel[1]))
                .sum::<u64>() as f32
        };
        let inks = [0.0, 0.25, 0.5, 0.75].map(ink);
        let mean = inks.iter().sum::<f32>() / 4.0;
        let spread = inks
            .iter()
            .fold(0.0f32, |spread, ink| spread.max((ink - mean).abs()))
            / mean;
        // Not zero: the gamma correction a glyph's coverage takes is not linear.
        assert!(
            spread < 0.05,
            "ink varies {:.1}% with the sub-pixel phase: {inks:?}",
            spread * 100.0
        );
    }

    #[test]
    fn raster_steps_are_half_octaves_held_with_hysteresis_and_capped_by_size() {
        let scaled = |factor: f32| [factor, 0.0, 0.0, factor, 0.0, 0.0];
        assert_eq!(raster_step(scaled(1.0), None, 16.0), 0);
        assert_eq!(raster_step(scaled(0.9), None, 16.0), 0, "a press stays");
        assert_eq!(raster_step(scaled(0.5), None, 16.0), -2, "shrinking steps");
        assert_eq!(raster_step(scaled(0.01), None, 16.0), -4, "down to 1/4");
        assert_eq!(raster_step(scaled(2.0), None, 16.0), 2);
        assert_eq!(raster_step(scaled(1.4), None, 16.0), 1);
        let turned = [0.0, 2.0, -2.0, 0.0, 0.0, 0.0];
        assert_eq!(
            raster_step(turned, None, 16.0),
            2,
            "a rotation keeps its scale"
        );
        assert_eq!(
            raster_step(scaled(1.25), Some(0), 16.0),
            0,
            "held below the step"
        );
        assert_eq!(raster_step(scaled(1.25), Some(1), 16.0), 1, "and above it");
        assert_eq!(
            raster_step(scaled(1.3), Some(0), 16.0),
            1,
            "until it is clearly past"
        );
        assert_eq!(
            raster_step(scaled(64.0), None, 16.0),
            4,
            "four times at most"
        );
        assert_eq!(
            raster_step(scaled(4.0), None, 128.0),
            2,
            "and never an em past what a tile is worth"
        );
        assert_eq!(raster_step(scaled(f32::NAN), Some(2), 16.0), 2);
    }

    #[test]
    fn a_dpi_change_reresolves_glyphs_without_laying_the_paragraph_out_again() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let paint = |pipeline: &mut TextPipeline, scale: f32| {
            pipeline.begin_frame([1024, 512]);
            pipeline.prepare(
                &device,
                LogicalRect::from_xywh(0.0, 0.0, 480.0, 32.0),
                LogicalRect::from_xywh(0.0, 0.0, 512.0, 256.0),
                scale,
                "DPI policy",
                Some([1.0; 4]),
                16.0,
                None,
                None,
                None,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Auto,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &SceneTextOpenType::default(),
                clip::IDENTITY_AFFINE,
                [0.0; 2],
                clip::FragmentClip::PASS,
                1.0,
                [0.0; 2],
                EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                },
                UNTRACKED_REVISION,
                None,
                true,
            );
            pipeline.flush_runs();
            pipeline.upload(&device, &queue, None);
        };
        paint(&mut pipeline, 1.0);
        paint(&mut pipeline, 2.0);
        let warm = pipeline.glyph_counters();
        paint(&mut pipeline, 1.0);
        paint(&mut pipeline, 2.0);
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.glyph_rasterized, warm.glyph_rasterized,
            "a DPI round trip must answer from the caches both scales filled"
        );
        assert_eq!(
            after.text_instance_rebuilds - warm.text_instance_rebuilds,
            2,
            "but each scale places its glyphs at its own pixels, so its own \
             entry content: the entry is keyed by the node and the two take turns"
        );
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses, 1,
            "the layout itself is scale-free: a DPI change re-resolves glyphs \
             and lays nothing out"
        );
    }

    #[test]
    fn moving_a_label_by_whole_pixels_keeps_its_glyphs() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let label = Label::new("Scrolling row", 1);
        text_frame(&device, &queue, &mut pipeline, &[label]);
        let warm = pipeline.glyph_counters();
        for step in 1..6 {
            text_frame(
                &device,
                &queue,
                &mut pipeline,
                &[Label {
                    top: step as f32 * 3.0,
                    ..label
                }],
            );
        }
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "a whole-pixel move leaves the sub-pixel phase alone, so the \
             bitmaps and the instances that name them still hold"
        );
        assert_eq!(
            after.text_instance_upload_bytes, warm.text_instance_upload_bytes,
            "and the origin the instances are relative to is a run row"
        );
    }

    #[test]
    fn editing_one_label_does_not_retransmit_the_others() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut labels = (0..16)
            .map(|index| Label {
                top: index as f32 * 14.0,
                ..Label::new("Row content", index + 1)
            })
            .collect::<Vec<_>>();
        text_frame(&device, &queue, &mut pipeline, &labels);
        text_frame(&device, &queue, &mut pipeline, &labels);
        let block = u64::from(entry::capacity_for(11))
            * std::mem::size_of::<pipeline::GlyphInstance>() as u64;
        // A row in the middle, so every later block would move if the arena
        // could not absorb the edit where it happened.
        for (content, blocks, note) in [
            ("New content", 1, "the same glyph count"),
            (
                "New contents",
                1,
                "one more glyph, which the block's slack absorbs",
            ),
        ] {
            let warm = pipeline.glyph_counters();
            labels[7].content = content;
            text_frame(&device, &queue, &mut pipeline, &labels);
            let after = pipeline.glyph_counters();
            assert_eq!(
                after.text_instance_rebuilds - warm.text_instance_rebuilds,
                1,
                "one label changed, so one entry is resolved again ({note})"
            );
            let moved = after.text_instance_upload_bytes - warm.text_instance_upload_bytes;
            assert!(
                moved > 0 && moved <= block * blocks,
                "only the changed paragraph's block may move ({note}): \
                 {moved} bytes against a block of {block} and a list of \
                 {} blocks",
                labels.len()
            );
        }
    }

    #[test]
    fn a_block_rebuilt_shorter_does_not_keep_drawing_the_glyphs_it_dropped() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let head = EntryKey {
            node: 7,
            slot: 0,
            pass: 0,
        };
        let tail = EntryKey {
            node: 8,
            slot: 0,
            pass: 0,
        };
        // Two paragraphs folded into one draw, so the command spans the slack
        // the first one's size class left. Whatever sits there is painted.
        let long = paint_labels(
            &device,
            &queue,
            &mut pipeline,
            &[("Force majeure", head, 0.0), ("tail", tail, 24.0)],
        );
        let short = paint_labels(
            &device,
            &queue,
            &mut pipeline,
            &[("F", head, 0.0), ("tail", tail, 24.0)],
        );
        let long_top = ink_aabb(&long[..], 256, 64).expect("the long label must paint");
        let short_top = ink_aabb(&short[..], 256, 64).expect("the short label must paint");
        assert!(
            short_top.2 < long_top.2,
            "a block rebuilt shorter must not keep painting the letters it \
             dropped: long={long_top:?} short={short_top:?}"
        );
    }

    fn row_key(node: u64) -> EntryKey {
        EntryKey {
            node,
            slot: 0,
            pass: 0,
        }
    }

    /// One frame of `rows` folded into one command, flushed and uploaded.
    /// Returns what it wrote: instance bytes, index bytes and draws.
    fn merged_rows(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        rows: &[&str],
    ) -> (u64, u64, usize) {
        let labels = rows
            .iter()
            .enumerate()
            .map(|(index, content)| (*content, row_key(index as u64 + 1), index as f32 * 14.0))
            .collect::<Vec<_>>();
        let warm = pipeline.glyph_counters();
        let height = (rows.len() as u32 * 14 + 32).max(512);
        prepare_merged(device, pipeline, &labels, [512, height]).expect("the rows draw");
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
        let after = pipeline.glyph_counters();
        (
            after.text_instance_upload_bytes - warm.text_instance_upload_bytes,
            after.text_index_upload_bytes - warm.text_index_upload_bytes,
            pipeline.target.segments.len(),
        )
    }

    fn row_entry(pipeline: &TextPipeline, node: u64) -> &entry::TextGpuEntry {
        let id = pipeline
            .target
            .entries
            .lookup(row_key(node))
            .expect("drawn");
        pipeline.target.entries.get(id).expect("live")
    }

    const INSTANCE: u64 = std::mem::size_of::<pipeline::GlyphInstance>() as u64;

    #[test]
    fn a_paragraph_that_outgrows_its_block_moves_alone() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut rows = vec!["Row content"; 16];
        merged_rows(&device, &queue, &mut pipeline, &rows);
        assert_eq!(
            merged_rows(&device, &queue, &mut pipeline, &rows),
            (0, 0, 1),
            "a steady frame writes nothing and is one draw"
        );
        let before = row_entry(&pipeline, 8).capacity;
        rows[7] = "Row content that grew well past its block";
        let (instances, indices, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let entry = row_entry(&pipeline, 8);
        assert!(entry.capacity > before, "the row must have changed class");
        assert_eq!(
            instances,
            u64::from(entry.capacity) * INSTANCE,
            "the row that grew writes its own block and nobody else's"
        );
        assert_eq!(
            indices,
            u64::from(entry.capacity) * 4,
            "and its own index range"
        );
        assert_eq!(
            draws, 3,
            "its range sits away from its neighbours: the rows before it, it, \
             the rows after it"
        );
        assert_eq!(
            merged_rows(&device, &queue, &mut pipeline, &rows),
            (0, 0, 3),
            "which is kept, not rewritten every frame, while it is within budget"
        );
    }

    #[test]
    fn an_arena_repack_rewrites_the_indices_of_the_blocks_it_moved_and_no_others() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut rows = vec!["Row content"; 16];
        merged_rows(&device, &queue, &mut pipeline, &rows);
        // Row 8 outgrows its block and is placed after every other one.
        rows[7] = "Row content, longer";
        let (_, _, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let before = (1..=16u64)
            .map(|node| {
                let entry = row_entry(&pipeline, node);
                (entry.arena_offset, entry.order_offset)
            })
            .collect::<Vec<_>>();
        // Whatever makes the arena repack — room, holes — it does so apart
        // from the draw order. Forcing it here is the one way to see that.
        pipeline.target.arena.note_breaks(u32::MAX);
        let (instances, indices, again) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let mut moved = 0;
        let mut total = 0;
        for (node, (arena, order)) in (1..=16u64).zip(before) {
            let entry = row_entry(&pipeline, node);
            total += entry.capacity;
            assert_eq!(
                entry.order_offset, order,
                "row {node}: an arena repack leaves the draw order alone"
            );
            if entry.arena_offset != arena {
                moved += entry.capacity;
            }
        }
        assert!(
            moved > 0,
            "the repack laid the blocks out in draw order again"
        );
        assert_eq!(
            instances,
            u64::from(total) * INSTANCE,
            "every block is written"
        );
        assert_eq!(
            indices,
            u64::from(moved) * 4,
            "and only the indices of the blocks that landed somewhere new"
        );
        assert_eq!(
            again, draws,
            "the draws follow the order, which did not change"
        );
    }

    #[test]
    fn repacking_the_draw_order_moves_no_instance() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut rows = vec!["Row content"; 16];
        merged_rows(&device, &queue, &mut pipeline, &rows);
        // Three rows apart from each other: two breaks each, past the budget
        // a list this short gets. Grown by a little, so the order still has
        // room for them and it is the breaks that repack it.
        for row in [3, 7, 11] {
            rows[row] = "Row content, longer";
        }
        let (instances, _, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        assert_eq!(draws, 7);
        let grown: u32 = [4, 8, 12]
            .into_iter()
            .map(|node| row_entry(&pipeline, node).capacity)
            .sum();
        assert_eq!(instances, u64::from(grown) * INSTANCE);
        let (instances, indices, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let total: u32 = (1..=16u64)
            .map(|node| row_entry(&pipeline, node).order_capacity)
            .sum();
        assert_eq!(draws, 1, "the frame after is one draw again");
        assert_eq!(
            indices,
            u64::from(total) * 4,
            "by rewriting the index table in draw order"
        );
        assert_eq!(
            instances, 0,
            "and not one instance: where a block sits no longer decides what it \
             batches with"
        );
    }

    #[test]
    fn text_that_keeps_growing_grows_in_place_once_the_order_has_left_it_room() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut rows = vec!["Row content"; 64];
        merged_rows(&device, &queue, &mut pipeline, &rows);
        assert!(
            !pipeline.target.order_gapped,
            "a first layout leaves no gaps"
        );
        // Four rows apart grow: eight breaks, past what a thousand slots may
        // cost, and every move a growth.
        for row in [5, 20, 35, 50] {
            rows[row] = "Row content, longer";
        }
        merged_rows(&device, &queue, &mut pipeline, &rows);
        let (_, _, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        assert!(
            pipeline.target.order_gapped,
            "what split the draws was text growing, so the repack left room"
        );
        assert_eq!(draws, 1, "and a draw runs straight across the gaps");
        // A row that has not grown before.
        let before = row_entry(&pipeline, 11).order_offset;
        rows[10] = "Row content, longer";
        let (instances, indices, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let entry = row_entry(&pipeline, 11);
        assert_eq!(entry.order_offset, before, "it grew where it was");
        assert_eq!(draws, 1, "so the draw did not split");
        assert_eq!(instances, u64::from(entry.capacity) * INSTANCE);
        assert!(
            indices <= u64::from(GAP_EVERY + entry.capacity) * 4,
            "and only it and the rows up to the next gap moved: {indices} bytes"
        );
    }

    #[test]
    fn text_that_comes_and_goes_is_not_left_gaps_it_would_not_use() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let frame = |pipeline: &mut TextPipeline, nodes: &[u64]| {
            let labels = nodes
                .iter()
                .enumerate()
                .map(|(index, node)| ("Row content", row_key(*node), index as f32 * 14.0))
                .collect::<Vec<_>>();
            prepare_merged(&device, pipeline, &labels, [512, 1024]).expect("the rows draw");
            pipeline.flush_runs();
            pipeline.upload(&device, &queue, None);
            pipeline.target.segments.len()
        };
        let mut nodes = (1..=64u64).collect::<Vec<_>>();
        frame(&mut pipeline, &nodes);
        for (at, node) in [(48, 204), (32, 203), (16, 202), (8, 201)] {
            nodes.insert(at, node);
        }
        frame(&mut pipeline, &nodes);
        assert_eq!(frame(&mut pipeline, &nodes), 1, "repacked");
        assert!(
            !pipeline.target.order_gapped,
            "rows arriving split the draws; room to grow would not have helped"
        );
    }

    /// Sixty-four rows folded into one command, four of them grown far enough
    /// apart that the order repacked with gaps. Returns the rows as drawn.
    fn rows_in_a_gapped_order(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
    ) -> Vec<&'static str> {
        let mut rows = vec!["Row content"; 64];
        merged_rows(device, queue, pipeline, &rows);
        for row in [5, 20, 35, 50] {
            rows[row] = "Row content, longer";
        }
        merged_rows(device, queue, pipeline, &rows);
        merged_rows(device, queue, pipeline, &rows);
        assert!(pipeline.target.order_gapped, "the order repacked with gaps");
        rows
    }

    /// Index slots this frame's draws span: the quads the vertex stage runs.
    fn spanned_slots(pipeline: &TextPipeline) -> u32 {
        pipeline
            .target
            .segments
            .iter()
            .map(|segment| segment.count)
            .sum()
    }

    #[test]
    fn text_that_stopped_growing_gives_its_gaps_back() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let rows = rows_in_a_gapped_order(&device, &queue, &mut pipeline);
        let gapped = spanned_slots(&pipeline);
        let generation = pipeline.target.order.generation();
        while pipeline.target.quiet_frames + 1 < SETTLE_FRAMES {
            assert_eq!(
                merged_rows(&device, &queue, &mut pipeline, &rows),
                (0, 0, 1),
                "a still frame before the text has stood still long enough \
                 writes nothing"
            );
        }
        assert_eq!(pipeline.target.order.generation(), generation);
        assert!(pipeline.target.order_gapped, "and keeps the gaps");
        let (instances, indices, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        let held: u32 = (1..=64u64)
            .map(|node| row_entry(&pipeline, node).order_capacity)
            .sum();
        let blocks: u32 = (1..=64u64)
            .map(|node| row_entry(&pipeline, node).capacity)
            .sum();
        assert_eq!(
            pipeline.target.order.generation(),
            generation + 1,
            "the frame the text has stood still for long enough repacks once"
        );
        assert!(!pipeline.target.order_gapped, "without gaps");
        assert_eq!(instances, 0, "moving no instance");
        assert_eq!(
            indices,
            u64::from(held) * 4,
            "and rewriting the index table, ranges only"
        );
        assert_eq!(held, blocks, "every range is its block again");
        assert_eq!(draws, 1, "still one draw");
        let packed = spanned_slots(&pipeline);
        assert!(packed <= held, "which spans no slot outside a range");
        assert!(
            packed < gapped,
            "and fewer quads than across the gaps: {packed} of {gapped}"
        );
        assert_eq!(
            merged_rows(&device, &queue, &mut pipeline, &rows),
            (0, 0, 1),
            "after which a still frame writes nothing again"
        );
    }

    #[test]
    fn text_that_grows_again_after_giving_its_gaps_back_gets_them_again() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut rows = rows_in_a_gapped_order(&device, &queue, &mut pipeline);
        while pipeline.target.order_gapped {
            merged_rows(&device, &queue, &mut pipeline, &rows);
        }
        // Grows again, somewhere else: the rows split the draws and the next
        // repack is, as before, down to text growing.
        for row in [10, 25, 40, 55] {
            rows[row] = "Row content, longer";
        }
        merged_rows(&device, &queue, &mut pipeline, &rows);
        let (_, _, draws) = merged_rows(&device, &queue, &mut pipeline, &rows);
        assert!(
            pipeline.target.order_gapped,
            "growing text gets its room back"
        );
        assert_eq!(draws, 1);
        // And keeps it for as long as it keeps growing: one row a frame,
        // each a little longer than the last time it grew.
        let texts = (0..24)
            .map(|len| format!("Row content {}", "x".repeat(4 + len * 3)))
            .collect::<Vec<_>>();
        let mut grown = vec![0usize; 64];
        let generation = pipeline.target.order.generation();
        let mut repacks = 0;
        for frame in 0..SETTLE_FRAMES as usize * 2 {
            let row = frame * 13 % 64;
            grown[row] += 1;
            rows[row] = texts[grown[row].min(texts.len() - 1)].as_str();
            let before = pipeline.target.order.generation();
            merged_rows(&device, &queue, &mut pipeline, &rows);
            if pipeline.target.order.generation() != before {
                repacks += 1;
            }
            assert!(
                pipeline.target.order_gapped,
                "frame {frame}: text that keeps growing keeps its gaps"
            );
        }
        assert!(
            repacks <= 4,
            "and repacks when its breaks are over budget, not every frame: \
             {repacks} repacks, generation {generation} -> {}",
            pipeline.target.order.generation()
        );
    }

    #[test]
    fn gaps_given_back_in_a_frame_already_repacking_repack_once() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let rows = rows_in_a_gapped_order(&device, &queue, &mut pipeline);
        pipeline.target.quiet_frames = SETTLE_FRAMES - 1;
        pipeline.target.order.note_breaks(u32::MAX);
        let generation = pipeline.target.order.generation();
        merged_rows(&device, &queue, &mut pipeline, &rows);
        assert_eq!(pipeline.target.order.generation(), generation + 1);
        assert!(
            !pipeline.target.order_gapped,
            "a repack over budget in text that has stopped growing leaves no gaps"
        );
    }

    #[test]
    fn dropping_every_entry_forgets_the_gaps() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        rows_in_a_gapped_order(&device, &queue, &mut pipeline);
        pipeline.drop_every_entry();
        assert!(
            !pipeline.target.order_gapped,
            "an order reset holds no gaps, so there are none to give back"
        );
        assert!(!pipeline.order_settle_due());
    }

    #[test]
    fn a_draw_does_not_run_across_the_space_a_paragraph_left() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let frame = |pipeline: &mut TextPipeline, rows: &[(u64, &'static str)]| {
            let labels = rows
                .iter()
                .enumerate()
                .map(|(index, (node, content))| (*content, row_key(*node), index as f32 * 14.0))
                .collect::<Vec<_>>();
            prepare_merged(&device, pipeline, &labels, [512, 512]).expect("the rows draw");
            pipeline.flush_runs();
            pipeline.upload(&device, &queue, None);
            pipeline.target.segments.len()
        };
        let rows = (1..=16u64)
            .map(|node| (node, "Row content"))
            .collect::<Vec<_>>();
        frame(&mut pipeline, &rows);
        // Row 8 outgrows its range, which is given back, and is now drawn
        // last: rows 7 and 9 are next to each other in the draw with the
        // space row 8 left between them in the table. That space still holds
        // row 8's old indices, so a draw spanning it would paint them.
        let mut reordered = rows
            .iter()
            .copied()
            .filter(|(node, _)| *node != 8)
            .collect::<Vec<_>>();
        reordered.push((8, "Row content, longer"));
        frame(&mut pipeline, &reordered);
        // The frame after: the space is free now, and rows 7 and 9 are still
        // drawn one after the other.
        assert_eq!(
            frame(&mut pipeline, &reordered),
            2,
            "rows 7 and 9 are two draws, not one across what row 8 left"
        );
    }

    #[test]
    fn two_paints_of_one_target_before_one_submit_each_draw_their_own_text() {
        let (device, queue) = test_device();
        let target = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 256,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let paint = |pipeline: &mut TextPipeline,
                     encoder: &mut wgpu::CommandEncoder,
                     texture: &wgpu::Texture,
                     text: &str| {
            let command = prepare_merged(&device, pipeline, &[(text, row_key(1), 0.0)], [256, 64]);
            pipeline.flush_runs();
            pipeline.upload_with(&device, &queue, encoder, None);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui text two paints"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let scissor = PhysicalRect {
                x: 0,
                y: 0,
                width: 256,
                height: 64,
            };
            pipeline.draw(&mut pass, &command.expect("drawn"), scissor, None);
        };
        let reference = |text: &str| {
            let mut fresh = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            paint_labels(&device, &queue, &mut fresh, &[(text, row_key(1), 0.0)])
        };
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // The same paragraph, rebuilt in between: the second paint rewrites
        // the block the first one's pass reads.
        let (first, second) = (target("first"), target("second"));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui text two paints"),
        });
        paint(&mut pipeline, &mut encoder, &first, "First");
        paint(&mut pipeline, &mut encoder, &second, "Second");
        let first_pixels = readback_rgba(&device, &queue, encoder, &first, 256, 64);
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui text two paints readback"),
        });
        let second_pixels = readback_rgba(&device, &queue, encoder, &second, 256, 64);
        assert!(
            first_pixels == reference("First"),
            "the first paint draws its own glyphs, not the ones the second \
             paint wrote before the encoder was submitted"
        );
        assert!(second_pixels == reference("Second"));
    }

    #[test]
    fn a_paragraph_that_moved_in_the_arena_paints_where_it_did() {
        let (device, queue) = test_device();
        let head = row_key(1);
        let middle = row_key(2);
        let tail = row_key(3);
        let frame = |middle_text| {
            [
                ("Head", head, 0.0),
                (middle_text, middle, 20.0),
                ("Tail", tail, 40.0),
            ]
        };
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        paint_labels(&device, &queue, &mut pipeline, &frame("ab"));
        // Out of its block, then out of the next one: moved in the arena and
        // in the draw order both times.
        for text in ["abcdefghijk", "abcdefghijklmnopqrst"] {
            let moved = paint_labels(&device, &queue, &mut pipeline, &frame(text));
            let mut fresh = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
            let expected = paint_labels(&device, &queue, &mut fresh, &frame(text));
            assert!(
                moved == expected,
                "a block that moved in the arena must paint exactly what a \
                 fresh one does ({text})"
            );
        }
    }

    #[test]
    fn an_eviction_recovers_the_entry_it_hit_without_reshaping_anything() {
        let (device, queue) = test_device();
        // One page that either paragraph fits in and the two together do not,
        // so each one's entry keeps losing its placements to the other. Fits,
        // not with room to spare: a paragraph fills most of the page, so the
        // holes the other one's evicted glyphs leave are the wrong shape and
        // only a repack places the last few.
        let mut pipeline = TextPipeline::with_atlas_limits(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            GlyphAtlasLimits {
                page_edge: 80,
                byte_budget: 80 * 80 + 8,
            },
        );
        const LOWER: &str = "abcdefghijklmnopqrstuvwxy";
        const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXY";
        let first = EntryKey {
            node: 1,
            slot: 0,
            pass: 0,
        };
        let second = EntryKey {
            node: 2,
            slot: 0,
            pass: 0,
        };
        let fresh = paint_labels(&device, &queue, &mut pipeline, &[(LOWER, first, 0.0)]);
        let fresh_ink = ink_aabb(&fresh, 256, 64).expect("the first paragraph paints");
        paint_labels(&device, &queue, &mut pipeline, &[(UPPER, second, 0.0)]);
        let warm = pipeline.glyph_counters();
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        let mut recovered = Vec::new();
        for _ in 0..4 {
            recovered = paint_labels(&device, &queue, &mut pipeline, &[(LOWER, first, 0.0)]);
            paint_labels(&device, &queue, &mut pipeline, &[(UPPER, second, 0.0)]);
        }
        let after = pipeline.glyph_counters();
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert!(
            after.glyph_atlas_evict > warm.glyph_atlas_evict,
            "the two paragraphs must really be evicting each other"
        );
        assert!(
            after.atlas_stale_handle_rejects > warm.atlas_stale_handle_rejects,
            "and the entry must really be meeting handles the eviction reissued"
        );
        assert_eq!(
            ink_aabb(&recovered, 256, 64),
            Some(fresh_ink),
            "the paragraph comes back whole: a rejected handle costs its glyphs, \
             never the wrong ones"
        );
        // The box only proves the ends are inked. A glyph written before a
        // repack that moved it would still ink the same box while sampling
        // whatever the repack put at its old rectangle.
        assert!(
            recovered == fresh,
            "every glyph samples its own rectangle, including the ones a repack \
             moved while the paragraph was still faulting glyphs in"
        );
        assert_eq!(
            misses, warm_misses,
            "an eviction costs an entry its glyphs, never its shaped paragraph"
        );
        assert!(
            after.text_instance_rebuilds > warm.text_instance_rebuilds,
            "the entry the eviction hit is resolved again, and only that one"
        );
    }

    #[test]
    fn a_glyph_the_atlas_had_no_room_for_is_placed_once_there_is() {
        let (device, queue) = test_device();
        // Either paragraph alone fills a bit over half the page, so after the
        // other one is evicted there is room to spare even for a packer that
        // strands some of it.
        let limits = GlyphAtlasLimits {
            page_edge: 96,
            byte_budget: 96 * 96 + 8,
        };
        const LOWER: &str = "abcdefghijklmnopqrstuvwxy";
        const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXY";
        let lower = EntryKey {
            node: 1,
            slot: 0,
            pass: 0,
        };
        let upper = EntryKey {
            node: 2,
            slot: 0,
            pass: 0,
        };
        let mut alone =
            TextPipeline::with_atlas_limits(&device, wgpu::TextureFormat::Rgba8Unorm, limits);
        let whole = ink_aabb(
            &paint_labels(&device, &queue, &mut alone, &[(UPPER, upper, 0.0)]),
            256,
            64,
        )
        .expect("the paragraph paints on its own");
        let mut pipeline =
            TextPipeline::with_atlas_limits(&device, wgpu::TextureFormat::Rgba8Unorm, limits);
        // Both at once: the page holds either paragraph and not the two, and
        // every glyph in it is this frame's, so nothing can be evicted to make
        // room. The second paragraph comes up short.
        let crowded = paint_labels(
            &device,
            &queue,
            &mut pipeline,
            &[(LOWER, lower, 0.0), (UPPER, upper, 32.0)],
        );
        let short = ink_aabb(&crowded[256 * 4 * 32..], 256, 32);
        assert_ne!(
            short.map(|(x, _, right, _)| (x, right)),
            Some((whole.0, whole.2)),
            "the page must really be too small for both for this to test anything"
        );
        let (_, warm_misses, _) = pipeline.shape_cache_stats();
        // The crowd is gone. Nothing about the paragraph changed, so only the
        // entry remembering that it is incomplete can bring the rest back.
        let recovered = paint_labels(&device, &queue, &mut pipeline, &[(UPPER, upper, 0.0)]);
        assert_eq!(
            ink_aabb(&recovered, 256, 64),
            Some(whole),
            "a paragraph the atlas could not finish must be finished once it can"
        );
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(misses, warm_misses, "without laying anything out again");
        let steady = pipeline.glyph_counters();
        paint_labels(&device, &queue, &mut pipeline, &[(UPPER, upper, 0.0)]);
        assert_eq!(
            pipeline.glyph_counters().text_instance_rebuilds,
            steady.text_instance_rebuilds,
            "and once whole it is retained like any other"
        );
    }

    #[test]
    fn a_glyph_too_big_for_any_page_does_not_rebuild_its_paragraph_every_frame() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::with_atlas_limits(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            GlyphAtlasLimits {
                page_edge: 64,
                byte_budget: 64 * 64 * 4 + 8,
            },
        );
        // 16 px at 6x is a glyph no 64 px page can hold, next frame or ever.
        // Waiting for room would rebuild the paragraph on every frame for a
        // glyph that is never coming.
        prepare_label(&device, &queue, &mut pipeline, "Wide", 6.0);
        let warm = pipeline.glyph_counters();
        for _ in 0..4 {
            prepare_label(&device, &queue, &mut pipeline, "Wide", 6.0);
        }
        assert_eq!(
            pipeline.glyph_counters().text_instance_rebuilds,
            warm.text_instance_rebuilds,
            "a glyph that can never be placed is not a reason to try again"
        );
    }

    #[test]
    fn evicting_a_glyph_moves_the_placement_epoch_that_gates_retained_commands() {
        let (device, queue) = test_device();
        // One page barely wider than a line of text, so a second paragraph of
        // different glyphs cannot coexist with the first.
        let mut pipeline = TextPipeline::with_atlas_limits(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            GlyphAtlasLimits {
                page_edge: 64,
                byte_budget: 64 * 64 + 8,
            },
        );
        prepare_label(&device, &queue, &mut pipeline, "abcdefghij", 1.0);
        let steady = pipeline.placement_epoch();
        prepare_label(&device, &queue, &mut pipeline, "abcdefghij", 1.0);
        assert_eq!(
            pipeline.placement_epoch(),
            steady,
            "a repaint that places nothing new must leave retained commands valid"
        );

        for label in [
            "KLMNOPQRST",
            "UVWXYZ0123",
            "456789!?@#",
            "\u{4e2d}\u{6587}\u{6e2c}\u{8a66}\u{6587}\u{5b57}",
        ] {
            prepare_label(&device, &queue, &mut pipeline, label, 1.0);
        }
        assert!(
            pipeline.glyph_counters().glyph_atlas_evict > 0,
            "the corpus must outgrow this page for the test to mean anything"
        );
        assert_ne!(
            pipeline.placement_epoch(),
            steady,
            "an eviction must invalidate commands built against the old placements"
        );
    }

    #[test]
    fn a_corpus_larger_than_the_atlas_keeps_placing_glyphs_and_never_samples_a_stale_one() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::with_atlas_limits(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            GlyphAtlasLimits {
                page_edge: 128,
                byte_budget: 128 * 128 + 8,
            },
        );
        // Far more distinct CJK glyphs than one small page holds, three times
        // over, so the atlas fills, evicts, repacks and refills.
        let corpus: Vec<String> = (0..48)
            .map(|line: u32| {
                (0..8)
                    .filter_map(|index| char::from_u32(0x4e00 + line * 8 + index))
                    .collect()
            })
            .collect();
        for _ in 0..3 {
            for label in &corpus {
                prepare_label(&device, &queue, &mut pipeline, label, 1.0);
            }
        }
        let counters = pipeline.glyph_counters();
        assert!(
            counters.glyph_atlas_evict > 0,
            "the corpus has to outgrow the page for this to test anything"
        );
        assert!(
            counters.glyph_atlas_bytes <= (128 * 128 + 8) as u64,
            "the atlas must stay inside its byte budget, got {} bytes",
            counters.glyph_atlas_bytes
        );
        assert!(
            counters.glyph_raster_cache_hit > 0,
            "a second pass over the corpus must be answered from the raster cache"
        );
        // Every glyph a frame placed is protected from that frame's own
        // eviction, so the flush that turns placements into instances must
        // never meet a handle that has gone stale.
        assert_eq!(
            counters.atlas_stale_handle_rejects, 0,
            "a frame must never build an instance from a placement it lost"
        );
        // One region per glyph faulted in, and not one more: a full page must
        // not repack — and therefore re-upload its whole live set — for every
        // glyph it cannot place.
        assert_eq!(
            counters.glyph_upload_regions, counters.glyph_atlas_miss,
            "a churning atlas must upload each placement once"
        );
    }

    #[test]
    fn a_second_window_on_one_device_reuses_the_first_windows_glyphs() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        prepare_label(&device, &queue, &mut pipeline, "Shared chrome", 1.0);
        let first = pipeline.glyph_counters();
        assert!(first.glyph_rasterized > 0);
        assert!(first.glyph_upload_regions > 0);

        // A second render target on the same device. Its own instance buffers
        // are separate; the atlas and the rasterized glyphs are not.
        let mut second = None;
        pipeline.swap_target(&mut second, &device);
        prepare_label(&device, &queue, &mut pipeline, "Shared chrome", 1.0);
        let shared = pipeline.glyph_counters();
        assert_eq!(
            shared.glyph_rasterized, first.glyph_rasterized,
            "a second window must not rasterize the first window's glyphs again"
        );
        assert_eq!(
            shared.glyph_upload_regions, first.glyph_upload_regions,
            "nor upload them to a second atlas"
        );
        assert_eq!(
            shared.glyph_atlas_hit,
            first.glyph_atlas_hit + first.glyph_atlas_miss,
            "every glyph of the second window must hit the shared atlas"
        );
        assert_eq!(shared.glyph_atlas_pages, first.glyph_atlas_pages);
    }

    #[test]
    fn closing_one_window_leaves_the_others_glyphs_placed() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut first = None;
        let mut second = None;

        pipeline.swap_target(&mut first, &device);
        prepare_label(&device, &queue, &mut pipeline, "Shell chrome", 1.0);
        pipeline.swap_target(&mut first, &device);

        pipeline.swap_target(&mut second, &device);
        prepare_label(&device, &queue, &mut pipeline, "Shell chrome", 1.0);
        let shared = pipeline.glyph_counters();
        pipeline.swap_target(&mut second, &device);

        // The first window closes. Its instance buffers go with it; the glyphs
        // it faulted into the shared atlas must not.
        drop(first);

        pipeline.swap_target(&mut second, &device);
        prepare_label(&device, &queue, &mut pipeline, "Shell chrome", 1.0);
        let after = pipeline.glyph_counters();
        assert_eq!(
            after.glyph_rasterized, shared.glyph_rasterized,
            "closing a window must not cost the surviving one a re-rasterization"
        );
        assert_eq!(
            after.glyph_upload_regions, shared.glyph_upload_regions,
            "nor a re-upload"
        );
        assert_eq!(
            after.glyph_atlas_evict, 0,
            "nor drop anything from the shared atlas"
        );
    }

    #[test]
    fn a_closed_window_gives_back_its_claims_on_shared_glyphs() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut first = None;
        let mut second = None;
        for window in [&mut first, &mut second] {
            pipeline.swap_target(window, &device);
            prepare_label(&device, &queue, &mut pipeline, "Shell chrome", 1.0);
            pipeline.swap_target(window, &device);
        }
        let handles = |pipeline: &TextPipeline, target: &TextPipelineTarget| {
            let id = target
                .entries
                .lookup(EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                })
                .expect("the label's entry");
            let entry = target.entries.get(id).expect("live");
            target
                .entries
                .live_glyphs(entry)
                .map(|(_, handle)| pipeline.atlas.claims(handle))
                .collect::<Vec<_>>()
        };
        let second_target = second.take().expect("second window");
        let shared = handles(&pipeline, &second_target);
        assert!(
            shared.iter().all(|claims| claims.is_some_and(|n| n >= 2)),
            "both windows claim the chrome's glyphs: {shared:?}"
        );
        // The first window closes. Its claims go with it — otherwise the
        // atlas would treat those glyphs as in use for the rest of the
        // session and evict everything else first.
        pipeline.close_target(first.take().expect("first window"));
        let after = handles(&pipeline, &second_target);
        // Both windows drew the same paragraph, so each held the same claims —
        // one per occurrence, and a letter that appears twice at the same
        // sub-pixel bin is one handle claimed twice. Closing one gives back
        // exactly half.
        for (before, now) in shared.iter().zip(&after) {
            assert_eq!(
                now.map(|n| n * 2),
                *before,
                "the closed window's claims, and only those, are released"
            );
        }
    }

    #[test]
    fn repainting_unchanged_text_uploads_neither_a_region_nor_an_instance() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        prepare_label(&device, &queue, &mut pipeline, "Steady label", 1.0);
        let warm = pipeline.glyph_counters();
        let work = prepare_label(&device, &queue, &mut pipeline, "Steady label", 1.0);
        let steady = pipeline.glyph_counters();
        assert_eq!(
            steady.glyph_upload_regions, warm.glyph_upload_regions,
            "an unchanged repaint must upload no atlas region"
        );
        assert_eq!(
            steady.glyph_rasterized, warm.glyph_rasterized,
            "and rasterize nothing"
        );
        assert_eq!(
            work.gpu_upload_bytes, 0,
            "the instances are byte-identical, so nothing reaches the queue"
        );
        assert_eq!(work.gpu_buffer_reallocations, 0);
    }

    #[test]
    fn a_scale_round_trip_rerasterizes_once_and_then_answers_from_cache() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        prepare_label(&device, &queue, &mut pipeline, "DPI probe", 1.0);
        let one = pipeline.glyph_counters();
        prepare_label(&device, &queue, &mut pipeline, "DPI probe", 2.0);
        let two = pipeline.glyph_counters();
        assert!(
            two.glyph_rasterized > one.glyph_rasterized,
            "a raster scale change must rasterize at the new size"
        );
        // Back again. The raster cache is keyed by size, not by "current
        // scale", so the first scale's bitmaps are still there and the atlas
        // still holds them.
        prepare_label(&device, &queue, &mut pipeline, "DPI probe", 1.0);
        let back = pipeline.glyph_counters();
        assert_eq!(
            back.glyph_rasterized, two.glyph_rasterized,
            "returning to a scale already drawn must not rasterize again"
        );
        assert_eq!(
            back.glyph_upload_regions, two.glyph_upload_regions,
            "nor re-upload"
        );
        let (_, misses, _) = pipeline.shape_cache_stats();
        assert_eq!(
            misses, 1,
            "the paragraph is laid out in logical px, so neither scale nor the \
             return to the first one is a second layout"
        );
    }

    /// Paint labels folded into one draw command and read the frame back.
    /// Prepare `labels` as one draw command, the way the painter folds
    /// consecutive text, on a `size` canvas. Neither flushed nor uploaded.
    fn prepare_merged(
        device: &wgpu::Device,
        pipeline: &mut TextPipeline,
        labels: &[(&str, EntryKey, f32)],
        size: [u32; 2],
    ) -> Option<PreparedText> {
        pipeline.begin_frame(size);
        let mut command: Option<PreparedText> = None;
        for (content, key, top) in labels {
            let Some(prepared) = prepare_row(device, pipeline, content, *key, *top, size) else {
                continue;
            };
            match command.as_ref() {
                Some(previous) if pipeline.can_merge_runs(previous, &prepared) => {
                    pipeline.merge_runs(previous, &prepared);
                }
                _ => command = Some(prepared),
            }
        }
        command
    }

    /// One label 240 px wide at `top`, prepared into the frame `size` opened.
    fn prepare_row(
        device: &wgpu::Device,
        pipeline: &mut TextPipeline,
        content: &str,
        key: EntryKey,
        top: f32,
        size: [u32; 2],
    ) -> Option<PreparedText> {
        pipeline.prepare(
            device,
            LogicalRect::from_xywh(0.0, top, 240.0, 32.0),
            LogicalRect::from_xywh(0.0, 0.0, size[0] as f32, size[1] as f32),
            1.0,
            content,
            Some([1.0, 1.0, 1.0, 1.0]),
            16.0,
            None,
            None,
            None,
            false,
            nana_ui_core::TextWrapBreak::Word,
            false,
            false,
            None,
            TextShaping::Auto,
            TextHorizontalAlignment::Start,
            TextVerticalAlignment::Top,
            &[],
            0.0,
            &[],
            &SceneTextOpenType::default(),
            clip::IDENTITY_AFFINE,
            [0.0; 2],
            clip::FragmentClip::PASS,
            1.0,
            [0.0; 2],
            key,
            UNTRACKED_REVISION,
            None,
            true,
        )
    }

    fn paint_labels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        labels: &[(&str, EntryKey, f32)],
    ) -> Vec<u8> {
        let command = prepare_merged(device, pipeline, labels, [256, 64]);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui text label prepare"),
        });
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui text label target"),
            size: wgpu::Extent3d {
                width: 256,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui text label pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(prepared) = command.as_ref() {
                pipeline.draw(
                    &mut pass,
                    prepared,
                    PhysicalRect {
                        x: 0,
                        y: 0,
                        width: 256,
                        height: 64,
                    },
                    None,
                );
            }
        }
        readback_rgba(device, queue, encoder, &texture, 256, 64)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        bounds: LogicalRect,
        clip: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        fragment_clip: clip::FragmentClip,
    ) -> Vec<u8> {
        paint_text_with(
            device,
            queue,
            pipeline,
            ("Hi", 16.0),
            bounds,
            clip,
            affine,
            persp,
            fragment_clip,
            [64, 64],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text_with(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        (content, size): (&str, f32),
        bounds: LogicalRect,
        clip: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        fragment_clip: clip::FragmentClip,
        canvas: [u32; 2],
    ) -> Vec<u8> {
        pipeline.begin_frame(canvas);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui text affine prepare"),
        });
        let opentype = SceneTextOpenType::default();
        let prepared = pipeline
            .prepare(
                device,
                bounds,
                clip,
                1.0,
                content,
                Some([1.0, 1.0, 1.0, 1.0]),
                size,
                None,
                None,
                None,
                false,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Auto,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &opentype,
                affine,
                persp,
                fragment_clip,
                1.0,
                [0.0, 0.0],
                EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                },
                UNTRACKED_REVISION,
                None,
                true,
            )
            .expect("text must prepare");
        // Placements are handles until the run is flushed; nothing is on the
        // GPU until the instances and the atlas regions are uploaded.
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui text affine target"),
            size: wgpu::Extent3d {
                width: canvas[0],
                height: canvas[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui text affine pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.draw(
                &mut pass,
                &prepared,
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: canvas[0],
                    height: canvas[1],
                },
                None,
            );
        }
        readback_rgba(device, queue, encoder, &texture, canvas[0], canvas[1])
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_block(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &mut TextPipeline,
        bounds: LogicalRect,
        clip: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        fragment_clip: clip::FragmentClip,
    ) -> Vec<u8> {
        pipeline.begin_frame([64, 64]);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui text clip prepare"),
        });
        let opentype = SceneTextOpenType::default();
        let prepared = pipeline
            .prepare(
                device,
                bounds,
                clip,
                1.0,
                "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH",
                Some([1.0, 1.0, 1.0, 1.0]),
                16.0,
                None,
                None,
                None,
                true,
                nana_ui_core::TextWrapBreak::Word,
                false,
                false,
                None,
                TextShaping::Auto,
                TextHorizontalAlignment::Start,
                TextVerticalAlignment::Top,
                &[],
                0.0,
                &[],
                &opentype,
                affine,
                persp,
                fragment_clip,
                1.0,
                [0.0, 0.0],
                EntryKey {
                    node: 1,
                    slot: 0,
                    pass: 0,
                },
                UNTRACKED_REVISION,
                None,
                true,
            )
            .expect("block text must prepare");
        pipeline.flush_runs();
        pipeline.upload(device, queue, None);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui text clip target"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui text clip pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.draw(
                &mut pass,
                &prepared,
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                },
                None,
            );
        }
        readback_rgba(device, queue, encoder, &texture, 64, 64)
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    }

    fn inked(color: [u8; 4]) -> bool {
        u16::from(color[0]) + u16::from(color[1]) + u16::from(color[2]) > 24
    }

    fn ink_aabb(pixels: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut found = false;
        for y in 0..height {
            for x in 0..width {
                let index = ((y * width + x) * 4) as usize;
                if u16::from(pixels[index])
                    + u16::from(pixels[index + 1])
                    + u16::from(pixels[index + 2])
                    > 24
                {
                    found = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        found.then_some((min_x, min_y, max_x, max_y))
    }

    fn readback_rgba(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let unpadded = width as usize * 4;
        let padded = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui text affine readback"),
            size: (padded * height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .expect("text affine readback poll");
        let mapped = slice
            .get_mapped_range()
            .expect("text affine readback must be mapped");
        let mut pixels = Vec::with_capacity(unpadded * height as usize);
        for row in mapped.chunks_exact(padded) {
            pixels.extend_from_slice(&row[..unpadded]);
        }
        pixels
    }

    #[test]
    fn glyphs_past_the_devices_storage_binding_leave_out_the_largest_paragraph() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        // A device whose storage binding holds 400 glyphs.
        pipeline.gpu.set_instance_slots(400);
        let log = "overflowing labels ".repeat(60);
        let mut labels = (0..12)
            .map(|row| ("Row content", row_key(row + 1), row as f32 * 14.0))
            .collect::<Vec<_>>();
        labels.insert(5, (log.as_str(), row_key(100), 70.0));
        let frame = |pipeline: &mut TextPipeline, labels: &[(&str, EntryKey, f32)]| {
            prepare_merged(&device, pipeline, labels, [512, 512]).expect("the rows draw");
            pipeline.flush_runs();
            // Binds the instance buffer whole: past the limit, validation
            // would fail here.
            pipeline.upload(&device, &queue, None);
        };
        frame(&mut pipeline, &labels);
        let log_entry = pipeline
            .target
            .entries
            .lookup(row_key(100))
            .expect("resolved");
        assert!(
            pipeline
                .target
                .entries
                .get(log_entry)
                .expect("live")
                .capacity
                > 400,
            "the case that matters: one paragraph larger than the device binds"
        );
        assert_eq!(pipeline.target.skipped, vec![log_entry]);
        assert!(
            pipeline.target.arena.capacity() <= 400,
            "the instance buffer stays inside the binding: {}",
            pipeline.target.arena.capacity()
        );
        // The audit has already checked that every other row is drawn from
        // its own glyphs. The one left out never took a range, so the rows
        // around it are still adjacent and still one draw.
        assert_eq!(pipeline.target.segments.len(), 1);
        labels.remove(5);
        frame(&mut pipeline, &labels);
        assert!(pipeline.target.skipped.is_empty());
        assert_eq!(pipeline.target.segments.len(), 1);
    }

    /// The instance and index buffers on the GPU hold, slot for slot, what
    /// the writes this target sent say they do — the shadow
    /// `audit_draw_order` checks every draw against. What that audit cannot
    /// see is whether the copies out of the staging ring, or out of a frame's
    /// one-off buffers, landed where they were meant to; this reads them back.
    fn assert_gpu_holds_the_shadow(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &TextPipeline,
        when: &str,
    ) {
        let (instances, indices) = pipeline.target.gpu.read_back(device, queue);
        let shadow = &pipeline.target.shadow;
        assert_eq!(
            instances.len(),
            shadow.instances.len(),
            "{when}: instance slots"
        );
        assert_eq!(indices.len(), shadow.indices.len(), "{when}: index slots");
        if let Some(slot) =
            (0..instances.len()).find(|&slot| instances[slot] != shadow.instances[slot])
        {
            panic!(
                "{when}: instance slot {slot} holds {:?}, the writes put {:?} there",
                instances[slot], shadow.instances[slot]
            );
        }
        if let Some(slot) = (0..indices.len()).find(|&slot| indices[slot] != shadow.indices[slot]) {
            panic!(
                "{when}: index slot {slot} holds {}, the writes put {} there",
                indices[slot], shadow.indices[slot]
            );
        }
    }

    #[test]
    fn a_frame_too_large_for_the_staging_ring_lands_whole() {
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let long = "a paragraph of several hundred glyphs ".repeat(16);
        let mut rows = (0..24).map(|_| long.clone()).collect::<Vec<_>>();
        let frame = |pipeline: &mut TextPipeline, rows: &[String]| {
            let labels = rows
                .iter()
                .enumerate()
                .map(|(row, content)| {
                    (content.as_str(), row_key(row as u64 + 1), row as f32 * 14.0)
                })
                .collect::<Vec<_>>();
            let before = pipeline.glyph_counters().text_instance_upload_bytes;
            prepare_merged(&device, pipeline, &labels, [512, 512]).expect("the rows draw");
            pipeline.flush_runs();
            pipeline.upload(&device, &queue, None);
            pipeline.glyph_counters().text_instance_upload_bytes - before
        };
        let written = frame(&mut pipeline, &rows);
        assert!(
            written > 256 * 1024,
            "the case that matters: more than a quarter of the largest ring ({written} B), \
             so the frame is staged in buffers of its own"
        );
        assert_gpu_holds_the_shadow(&device, &queue, &pipeline, "one-off staging");
        // And the frames after it, through the ring, on top of those bytes.
        for row in [3, 11, 17] {
            rows[row] = format!("{row} changed");
            frame(&mut pipeline, &rows);
            assert_gpu_holds_the_shadow(&device, &queue, &pipeline, "the ring");
        }
    }

    /// xorshift32, so a churn test replays the same frames on every run.
    struct Churn(u32);

    impl Churn {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 as usize % bound.max(1)
        }

        /// A label of anywhere from nothing to a few lines.
        fn text(&mut self) -> String {
            const WORDS: [&str; 10] = [
                "a",
                "Row",
                "cell",
                "12",
                "content",
                "ticked",
                "0.25",
                "overflowing",
                "x y",
                "labels",
            ];
            (0..self.below(12))
                .map(|_| WORDS[self.below(WORDS.len())])
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// Hundreds of frames of labels changing length, coming and going,
    /// swapping places, splitting across commands, drawn twice and dropped
    /// all at once, with both order layouts
    /// (packed, and gapped for text that keeps growing) in play.
    ///
    /// Every frame goes through `audit_placements` and `audit_draw_order`, so
    /// a range left naming a block that moved, a gap a draw ran across that
    /// was not vacant, or a buffer replaced under blocks the frame had already
    /// placed fails here. And #224's promise is checked on every frame the
    /// arena did not repack: the instance bytes written are the blocks of the
    /// paragraphs that were rebuilt, never their neighbours'.
    #[test]
    fn seeded_churn_keeps_every_draw_naming_its_own_glyphs() {
        const INITIAL: usize = 240;
        let (device, queue) = test_device();
        let mut pipeline = TextPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut churn = Churn(0x2545_f491);
        let mut next_node = 1u64;
        let mut new_row = |churn: &mut Churn| {
            next_node += 1;
            (next_node, churn.text())
        };
        let mut rows = (0..INITIAL)
            .map(|_| new_row(&mut churn))
            .collect::<Vec<_>>();
        let mut drawn_as: HashMap<u64, String> = HashMap::new();
        let mut layouts = [0u32; 2];
        let mut checked = 0;
        let mut settles = 0;
        for frame in 0..600u32 {
            // Text changing length, then standing still for long enough to
            // give its gaps back (#230), then scrolling.
            let phase = frame % 300;
            let still = (75..75 + SETTLE_FRAMES + 30).contains(&phase);
            let scrolling = phase >= 75 + SETTLE_FRAMES + 30;
            // A font registration: every entry dropped at once.
            if !still && churn.below(97) == 0 {
                pipeline.drop_every_entry();
                drawn_as.clear();
            }
            if scrolling {
                for _ in 0..churn.below(6) {
                    if rows.len() > INITIAL / 2 {
                        rows.remove(churn.below(rows.len()));
                    }
                }
                for _ in 0..churn.below(6) {
                    let at = churn.below(rows.len() + 1);
                    rows.insert(at, new_row(&mut churn));
                }
                if churn.below(4) == 0 {
                    let (a, b) = (churn.below(rows.len()), churn.below(rows.len()));
                    rows.swap(a, b);
                }
            } else if !still {
                for _ in 0..1 + churn.below(8) {
                    let at = churn.below(rows.len());
                    rows[at].1 = churn.text();
                }
            }
            let (start, len) = if scrolling {
                let len = rows.len() * 3 / 4;
                (churn.below(rows.len() - len + 1), len)
            } else {
                (0, rows.len())
            };
            let size = [512, len as u32 * 14 + 32];
            pipeline.begin_frame(size);
            let epoch = pipeline.placement_epoch();
            let arena_generation = pipeline.target.arena.generation();
            let mut rebuilt = Vec::new();
            let mut command: Option<PreparedText> = None;
            for (index, (node, content)) in rows[start..start + len].iter().enumerate() {
                let key = row_key(*node);
                // What may be written: a paragraph that changed, and one an
                // arena repack placed nowhere because it was off screen then.
                let placed = pipeline
                    .target
                    .entries
                    .lookup(key)
                    .and_then(|id| pipeline.target.entries.get(id))
                    .is_some_and(|entry| entry.arena_generation == Some(arena_generation));
                if !placed || drawn_as.get(node) != Some(content) {
                    rebuilt.push(key);
                }
                let top = index as f32 * 14.0;
                let Some(prepared) = prepare_row(&device, &mut pipeline, content, key, top, size)
                else {
                    continue;
                };
                drawn_as.insert(*node, content.clone());
                match command.as_ref() {
                    Some(previous)
                        if churn.below(24) != 0 && pipeline.can_merge_runs(previous, &prepared) =>
                    {
                        pipeline.merge_runs(previous, &prepared);
                    }
                    _ => command = Some(prepared),
                }
            }
            if churn.below(6) == 0 && len > 0 {
                // One row drawn a second time, by a command of its own.
                let at = start + churn.below(len);
                let (node, content) = &rows[at];
                let _ = prepare_row(&device, &mut pipeline, content, row_key(*node), 0.0, size);
            }
            let before = pipeline.glyph_counters().text_instance_upload_bytes;
            let gapped = pipeline.target.order_gapped;
            pipeline.flush_runs();
            pipeline.upload(&device, &queue, None);
            let written = pipeline.glyph_counters().text_instance_upload_bytes - before;
            if frame % 20 == 0 {
                assert_gpu_holds_the_shadow(&device, &queue, &pipeline, &format!("frame {frame}"));
            }
            layouts[usize::from(pipeline.target.order_gapped)] += 1;
            if still && gapped && !pipeline.target.order_gapped {
                settles += 1;
            }
            if pipeline.target.arena.generation() == arena_generation
                && pipeline.placement_epoch() == epoch
            {
                let own: u64 = rebuilt
                    .iter()
                    .filter_map(|key| pipeline.target.entries.lookup(*key))
                    .map(|id| u64::from(pipeline.target.entries.get(id).expect("live").capacity))
                    .sum::<u64>()
                    * INSTANCE;
                assert!(
                    written <= own,
                    "frame {frame}: {written} B of instances written, but the {} paragraphs \
                     rebuilt or placed hold {own}",
                    rebuilt.len()
                );
                checked += 1;
            }
        }
        assert!(
            layouts.iter().all(|frames| *frames > 0),
            "both order layouts must have been exercised: packed / gapped {layouts:?}"
        );
        assert!(checked > 300, "only {checked} frames kept their arena");
        assert!(
            settles > 0,
            "text that stood still never gave its gaps back"
        );
    }

    fn test_device() -> (wgpu::Device, wgpu::Queue) {
        crate::test_gpu::device()
    }
}

#[cfg(test)]
mod placement_size {
    #[test]
    fn a_retained_glyph_stays_twenty_four_bytes() {
        // One per glyph, held between frames and never rewritten while the
        // paragraph, its sub-pixel phase and its atlas placements stand still.
        assert_eq!(
            std::mem::size_of::<super::pipeline::GlyphInstance>(),
            24,
            "the retained glyph is the instance; growing it grows every \
             entry's block and the arena that mirrors it"
        );
        assert_eq!(std::mem::size_of::<super::pipeline::TextRunGpu>(), 48);
        assert_eq!(
            std::mem::size_of::<super::pipeline::TextPresentationGpu>(),
            160
        );
    }
}
