//! `TextGpuEntry`: one text node's glyphs, kept between frames.
//!
//! Everything above this module produces a paragraph's glyphs from scratch:
//! shape it, resolve every glyph, rasterize what is missing, place it in the
//! atlas. That work is proportional to the text on screen and it used to
//! happen on every frame that rebatched — which is every frame with any
//! animation in it, anywhere.
//!
//! An entry is the answer: the resolved instances of one node, held in a slab
//! and handed back whenever the facts they were derived from still hold.
//!
//! ```text
//! layout unchanged  ── the laid-out paragraph is the same one
//! colors unchanged  ── its rich spans still paint what they painted
//! phase unchanged   ── the bitmaps were rasterized for this sub-pixel offset
//! scale unchanged   ── they were placed at this device scale and raster step
//! fonts unchanged   ── the face set still issues these face ids
//! atlas unchanged   ── the rectangles are still these glyphs'
//! ```
//!
//! All but the last are compared in `prepare`; a mismatch rebuilds that one
//! entry and nothing else. The atlas is repaired rather than rebuilt: the
//! entry keeps one atlas handle per glyph, so a relocation only has to re-read
//! rectangles.
//!
//! What an entry deliberately does **not** hold is where its text sits, what
//! *solid* color it paints, how opaque it is or what transform it is under.
//! Those are the run and presentation rows of [`super::pipeline`], which is
//! why moving, fading or recoloring plain text never touches a single
//! instance. Rich spans are the exception, and the one they prove: a span
//! paints something the run row cannot say, so its bytes are baked into the
//! instances and `colors` is what notices when they change.

use std::collections::HashMap;

use super::atlas::GlyphAtlasEntryId;
use super::glyph::GlyphRenderMode;
use super::pipeline::{DrawSegment, GlyphInstance};

/// Which draw of which node an entry belongs to.
///
/// `slot` is the scene's own `PrimitiveId` slot: one node can draw several
/// paragraphs — a checkbox's tick beside its label, an input's placeholder
/// beside its value — and they are not each other's.
///
/// `pass` separates a label's text shadows from the label: they resolve the
/// same paragraph at different offsets and colors, so they are different
/// entries over the same shaped buffer and the same glyph bitmaps.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::scene_paint) struct EntryKey {
    pub node: u64,
    pub slot: u64,
    pub pass: u32,
}

/// A page pair and the span of one entry's instances that samples it.
/// `first` is relative to the entry's block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EntrySegment {
    pub mask_page: u32,
    pub color_page: u32,
    pub first: u32,
    pub count: u32,
}

/// One text node's retained GPU geometry. See the module docs.
pub(super) struct TextGpuEntry {
    /// Hash of the shaped paragraph these glyphs were resolved from.
    pub layout: u64,
    /// What these glyphs were painted with, when that is not the run row's
    /// colour alone: a fingerprint of the rich spans and the colour they were
    /// resolved against. Zero for solid text, whose every glyph inherits the
    /// run row and is therefore recoloured without touching an instance.
    ///
    /// Its own field because the *layout* is not keyed by colour any more:
    /// since #99 two spellings of one string in different colours are the same
    /// paragraph, and only this tells their glyphs apart.
    pub colors: u64,
    /// Sub-pixel phase of the run origin the bitmaps were rasterized for.
    pub phase: [u32; 2],
    /// Font-set generation the face ids were issued under.
    pub font_generation: u64,
    /// Grayscale or subpixel: the bitmaps differ, so a paragraph that moves
    /// into an offscreen group, or out of one, is resolved again.
    pub mode: GlyphRenderMode,
    /// The scene rebuild that wrote the primitive these glyphs were resolved
    /// from, and the device scale they were resolved at. Together with
    /// `font_generation` they are the cheap half of `layout`: if none of them
    /// moved, the paragraph and its box are the ones this entry already holds,
    /// so the shape key does not have to be assembled and hashed to find that
    /// out. `u64::MAX` is "no primitive said", which never matches.
    pub revision: u64,
    /// Physical px per logical px the glyphs were resolved at: the device
    /// scale times [`Self::raster_step`]'s factor.
    pub scale_bits: u32,
    /// The half-octave scale step a transform above this text earned it,
    /// negative when it shrinks, held between frames so a zoom that hovers
    /// near a step boundary does not rasterize the paragraph again every time
    /// it crosses it.
    pub raster_step: i8,
    /// What the paragraph measured: the widest line and the height the lines
    /// laid out to, in **logical** px — the layout is laid out there and the
    /// device scale only reaches the glyph coordinates. A pure function of the
    /// layout, which `revision` already pins, so a steady frame reads it here
    /// instead of walking the lines again.
    pub measured: [f32; 2],
    /// Atlas placement epoch the rectangles were read at.
    pub atlas_epoch: u64,
    /// Start of this entry's block in the store.
    pub block: u32,
    /// Slots the block spans. `capacity - glyphs` of them are vacant, so a
    /// paragraph that gains or loses a few glyphs keeps its arena offset and
    /// the paragraphs after it are not rewritten.
    pub capacity: u32,
    /// Live instances, the first of the block.
    pub glyphs: u32,
    pub segments: Vec<EntrySegment>,
    /// Where the block was last written in the arena, and how many slots it
    /// claimed there. `arena_generation` is the arena layout those are true
    /// for; anything older has to be placed again.
    pub arena_offset: u32,
    pub arena_capacity: u32,
    pub arena_generation: Option<u64>,
    /// The same three for the entry's range of the draw-order index table:
    /// one `u32` per slot of the block, naming that slot's place in the arena.
    /// A draw spans index ranges, not arena ranges, so where the block sits in
    /// the arena no longer decides what it batches with.
    pub order_offset: u32,
    pub order_capacity: u32,
    pub order_generation: Option<u64>,
    /// The index range no longer names the block: the block moved or changed
    /// size, or the range itself was shifted along. Rewritten when the entry
    /// is next drawn, even if the range stays where it is.
    pub indices_stale: bool,
    /// The run row this entry's instances name. Allocated once and kept for
    /// the entry's whole life, so a frame that only changes which paragraphs
    /// are on screen does not renumber the ones that stayed — renumbering
    /// would mean rewriting every instance of every one of them.
    pub slot: Option<u32>,
    /// The slot the instances currently carry, which lags `slot` only between
    /// a rebuild and the flush that binds it.
    pub run: u32,
    pub last_used: u64,
    /// A rectangle this entry named could not be repaired, so its glyphs are
    /// no longer all the ones its paragraph resolves to. It draws what it has
    /// this frame and is resolved again on the next.
    pub damaged: bool,
}

impl TextGpuEntry {
    /// Whether the glyphs this entry holds are still the ones `layout` at
    /// `phase`, `colors` and `scale_bits` would resolve to under `fonts`.
    ///
    /// The scale is its own term because the layout is not keyed by it: since
    /// #99 a paragraph is laid out in logical px and the device scale is
    /// applied to its coordinates at resolve time, so one layout hash stands
    /// for two sets of glyphs at 1x and 2x.
    ///
    /// Deliberately does not consider the atlas: a relocation is repaired by
    /// re-reading rectangles through the handles, which costs no shaping, no
    /// rasterizing and no atlas traffic.
    pub(super) fn valid(
        &self,
        layout: u64,
        colors: u64,
        phase: [u32; 2],
        scale_bits: u32,
        fonts: u64,
        mode: GlyphRenderMode,
    ) -> bool {
        !self.damaged
            && self.mode == mode
            && self.layout == layout
            && self.colors == colors
            && self.phase == phase
            && self.scale_bits == scale_bits
            && self.font_generation == fonts
    }
}

/// Slots a block of `glyphs` is given: an eighth of slack, at least four,
/// rounded up to a size class.
///
/// The slack is what lets typing a character into a field, a counter ticking
/// from 9 to 10 or a label gaining a word stay inside the block it already
/// owns (see [`EntryStore::begin_build`]), so the only bytes that move are
/// that paragraph's. The classes — four per doubling — are what let a block
/// one paragraph gave back be taken by the next one of about that length:
/// the store's free lists are keyed by exact capacity, and without classes a
/// list whose labels vary in length would only ever grow.
pub(super) fn capacity_for(glyphs: u32) -> u32 {
    let need = glyphs.saturating_add((glyphs / 8).max(4));
    if need <= 8 {
        return 8;
    }
    round_to_class(need, 4)
}

/// `n` rounded up to one of `per_doubling` sizes between each power of two
/// and the next.
fn round_to_class(n: u32, per_doubling: u32) -> u32 {
    let step = (1u32 << (31 - n.leading_zeros())) / per_doubling;
    n.div_ceil(step).saturating_mul(step)
}

/// Whether a block of `capacity` slots may keep holding `glyphs`.
///
/// Growing within it is the point of the slack. Shrinking only happens when
/// the block is more than twice what the paragraph needs *and* that is a real
/// amount of memory. A block that changes class costs more than its own
/// bytes: its index range changes length too, so it lands away from its
/// neighbours in the draw order, the draw it belonged to splits, and enough
/// splits rewrite the whole index table. A cell whose text keeps changing
/// length therefore settles at the largest class it has needed rather than
/// moving on every change; a paragraph that lost most of a page of text still
/// gives the space back.
fn block_fits(capacity: u32, glyphs: u32) -> bool {
    glyphs <= capacity
        && (capacity <= capacity_for(glyphs).saturating_mul(2) || capacity - glyphs <= SHRINK_SLACK)
}

/// What an index slot holds when no instance is behind it: past a range's
/// block, or in a gap left for ranges to grow into.
pub(super) const VACANT_INDEX: u32 = u32::MAX;

/// Whether an index range of `held` slots may keep serving a block of
/// `capacity`: it holds the block, and not so much more that the spare quads
/// cost more than moving would.
pub(super) fn range_keeps(held: u32, capacity: u32) -> bool {
    held >= capacity && held - capacity <= capacity.max(SHRINK_SLACK)
}

/// Vacant slots a block may carry before shrinking is worth moving it: a
/// kilobyte and a half of instances.
const SHRINK_SLACK: u32 = 64;

/// The entries, their instances and the atlas handles behind them.
///
/// Instances and handles are parallel slabs: one allocation covers both, and a
/// glyph's handle is at the same index as the instance it produced, which is
/// what makes repairing a relocated rectangle a straight walk.
#[derive(Default)]
pub(super) struct EntryStore {
    entries: Vec<Option<TextGpuEntry>>,
    /// Hashed by [`nana_ui_runtime::IdHasher`]: the key is three integers a
    /// steady frame looks up once per paragraph, and SipHash over them was a
    /// measurable share of the painter.
    index: HashMap<EntryKey, u32, nana_ui_runtime::BuildIdHasher>,
    vacant: Vec<u32>,
    instances: Vec<GlyphInstance>,
    handles: Vec<GlyphAtlasEntryId>,
    /// Free blocks by capacity. Blocks are only ever reused at their own
    /// capacity, so the slab never has to coalesce or compact.
    free: HashMap<u32, Vec<u32>>,
    live_glyphs: u64,
    created: u64,
    destroyed: u64,
    reused: u64,
}

impl EntryStore {
    pub(super) fn get(&self, id: u32) -> Option<&TextGpuEntry> {
        self.entries.get(id as usize)?.as_ref()
    }

    pub(super) fn get_mut(&mut self, id: u32) -> Option<&mut TextGpuEntry> {
        self.entries.get_mut(id as usize)?.as_mut()
    }

    pub(super) fn lookup(&self, key: EntryKey) -> Option<u32> {
        self.index.get(&key).copied()
    }

    /// Record that an entry answered a frame without being resolved again.
    pub(super) fn note_reuse(&mut self) {
        self.reused += 1;
    }

    pub(super) fn counters(&self) -> EntryCounters {
        EntryCounters {
            active: self.index.len() as u64,
            created: self.created,
            destroyed: self.destroyed,
            reused: self.reused,
            glyphs: self.live_glyphs,
        }
    }

    /// Each live glyph's instance with the atlas handle it was read through.
    #[cfg(test)]
    pub(super) fn live_glyphs(
        &self,
        entry: &TextGpuEntry,
    ) -> impl Iterator<Item = (&GlyphInstance, GlyphAtlasEntryId)> {
        let start = entry.block as usize;
        let end = start + entry.glyphs as usize;
        self.instances[start..end]
            .iter()
            .zip(self.handles[start..end].iter().copied())
    }

    /// The whole block, slack included: what a draw that spans it reads.
    pub(super) fn instances(&self, entry: &TextGpuEntry) -> &[GlyphInstance] {
        let start = entry.block as usize;
        &self.instances[start..start + entry.capacity as usize]
    }

    /// Point every instance of `id` at `run`. Returns whether anything moved.
    pub(super) fn bind_run(&mut self, id: u32, run: u32) -> bool {
        let Some(entry) = self.entries.get_mut(id as usize).and_then(Option::as_mut) else {
            return false;
        };
        if entry.run == run {
            return false;
        }
        entry.run = run;
        let start = entry.block as usize;
        let end = start + entry.capacity as usize;
        for instance in &mut self.instances[start..end] {
            *instance = instance.with_run(run);
        }
        true
    }

    /// Re-read every rectangle through its handle after the atlas moved.
    ///
    /// Returns `false` when a handle no longer names a live placement, which
    /// means this entry has to be resolved again before it is correct. The
    /// glyph is vacated for this frame rather than sampling whatever now owns
    /// its rectangle.
    pub(super) fn repair(
        &mut self,
        id: u32,
        epoch: u64,
        mut rect: impl FnMut(GlyphAtlasEntryId) -> Option<([u32; 2], [u32; 2])>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(id as usize).and_then(Option::as_mut) else {
            return false;
        };
        let start = entry.block as usize;
        let live = entry.glyphs as usize;
        entry.atlas_epoch = epoch;
        let mut intact = true;
        for offset in 0..live {
            let handle = self.handles[start + offset];
            match rect(handle) {
                Some((origin, size)) => {
                    self.instances[start + offset] =
                        self.instances[start + offset].with_placement(origin, size);
                }
                None => {
                    self.instances[start + offset] = self.instances[start + offset].vacated();
                    intact = false;
                }
            }
        }
        intact
    }

    /// Take the block `key` should build into, releasing whatever it held.
    ///
    /// `release` sees every atlas handle the old block referenced, so the
    /// entry's claim on those glyphs ends exactly when its instances do.
    pub(super) fn begin_build(
        &mut self,
        key: EntryKey,
        glyphs: u32,
        mut release: impl FnMut(GlyphAtlasEntryId),
    ) -> u32 {
        let capacity = capacity_for(glyphs);
        let id = match self.index.get(&key).copied() {
            Some(id) => {
                let entry = self.entries[id as usize]
                    .as_mut()
                    .expect("an indexed entry is live");
                self.live_glyphs -= u64::from(entry.glyphs);
                let start = entry.block as usize;
                // The claims the *old* block made, which is its glyph count,
                // not the new one: a paragraph rebuilt shorter would otherwise
                // keep claiming the glyphs it dropped for as long as it lives.
                let held = entry.glyphs.min(entry.capacity) as usize;
                let resize =
                    (!block_fits(entry.capacity, glyphs)).then_some((entry.block, entry.capacity));
                entry.glyphs = glyphs;
                entry.segments.clear();
                entry.damaged = false;
                for handle in &self.handles[start..start + held] {
                    release(*handle);
                }
                if let Some((block, released)) = resize {
                    self.free.entry(released).or_default().push(block);
                    let block = self.take_block(capacity);
                    let entry = self.entries[id as usize]
                        .as_mut()
                        .expect("an indexed entry is live");
                    entry.block = block;
                    entry.capacity = capacity;
                }
                id
            }
            None => {
                let block = self.take_block(capacity);
                let entry = TextGpuEntry {
                    layout: 0,
                    colors: 0,
                    phase: [0; 2],
                    font_generation: 0,
                    mode: GlyphRenderMode::Mask,
                    revision: u64::MAX,
                    scale_bits: 0,
                    raster_step: 0,
                    measured: [0.0; 2],
                    atlas_epoch: 0,
                    block,
                    capacity,
                    glyphs,
                    segments: Vec::new(),
                    arena_offset: 0,
                    arena_capacity: 0,
                    arena_generation: None,
                    order_offset: 0,
                    order_capacity: 0,
                    order_generation: None,
                    indices_stale: false,
                    slot: None,
                    run: u32::MAX,
                    last_used: 0,
                    damaged: false,
                };
                let id = match self.vacant.pop() {
                    Some(id) => {
                        self.entries[id as usize] = Some(entry);
                        id
                    }
                    None => {
                        self.entries.push(Some(entry));
                        (self.entries.len() - 1) as u32
                    }
                };
                self.index.insert(key, id);
                self.created += 1;
                id
            }
        };
        self.live_glyphs += u64::from(glyphs);
        id
    }

    /// Write one resolved glyph into the block being built.
    pub(super) fn push_glyph(
        &mut self,
        id: u32,
        offset: u32,
        handle: GlyphAtlasEntryId,
        instance: GlyphInstance,
    ) {
        let entry = self.entries[id as usize].as_ref().expect("building");
        let slot = entry.block as usize + offset as usize;
        self.instances[slot] = instance;
        self.handles[slot] = handle;
    }

    /// Close the block opened by [`Self::begin_build`] at `placed` live glyphs.
    ///
    /// Not every resolved glyph reaches a slot: a space rasterizes to nothing
    /// and an atlas that cannot hold one drops it. Everything past `placed` is
    /// vacated, because a draw that batches this block with its neighbour
    /// covers the slack too, and whatever the previous build left there would
    /// paint as a second copy of the old text.
    pub(super) fn finish_build(&mut self, id: u32, placed: u32) {
        let Some(entry) = self.entries.get_mut(id as usize).and_then(Option::as_mut) else {
            return;
        };
        self.live_glyphs -= u64::from(entry.glyphs);
        self.live_glyphs += u64::from(placed);
        entry.glyphs = placed;
        // The instances the run index has yet to be bound into.
        entry.run = u32::MAX;
        let start = entry.block as usize;
        let end = start + entry.capacity as usize;
        self.instances[start + placed as usize..end].fill(GlyphInstance::VACANT);
        self.handles[start + placed as usize..end].fill(GlyphAtlasEntryId::STALE);
    }

    /// Void every block's arena placement. What a repack starts with.
    pub(super) fn invalidate_arena(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.arena_generation = None;
        }
    }

    /// Void every index range. What a repack of the draw order starts with.
    pub(super) fn invalidate_order(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.order_generation = None;
        }
    }

    /// Drop every entry last used before `before`, giving back its glyphs, its
    /// run row, its arena block and its index range.
    pub(super) fn retire(
        &mut self,
        before: u64,
        mut release: impl FnMut(GlyphAtlasEntryId),
        mut release_slot: impl FnMut(u32),
        mut release_arena: impl FnMut(u64, u32, u32),
        mut release_order: impl FnMut(u64, u32, u32),
    ) {
        let stale = self
            .index
            .iter()
            .filter(|(_, id)| {
                self.entries[**id as usize]
                    .as_ref()
                    .is_none_or(|entry| entry.last_used < before)
            })
            .map(|(key, id)| (*key, *id))
            .collect::<Vec<_>>();
        for (key, id) in stale {
            self.index.remove(&key);
            let Some(entry) = self.entries[id as usize].take() else {
                continue;
            };
            let start = entry.block as usize;
            for handle in &self.handles[start..start + entry.glyphs as usize] {
                release(*handle);
            }
            self.live_glyphs -= u64::from(entry.glyphs);
            if let Some(slot) = entry.slot {
                release_slot(slot);
            }
            if let Some(generation) = entry.arena_generation {
                release_arena(generation, entry.arena_offset, entry.arena_capacity);
            }
            if let Some(generation) = entry.order_generation {
                release_order(generation, entry.order_offset, entry.order_capacity);
            }
            self.free
                .entry(entry.capacity)
                .or_default()
                .push(entry.block);
            self.vacant.push(id);
            self.destroyed += 1;
        }
        self.compact_if_sparse();
    }

    /// Close the holes retired entries left, once they are most of the slab.
    ///
    /// Free blocks are only ever reused at their own size class, so a panel
    /// of long paragraphs that closed leaves space no label will take. Without
    /// this the slab stays at the largest amount of text the session ever
    /// held. Only the store's own offsets move — an entry's arena placement
    /// and its GPU bytes are untouched, because the block's contents are.
    fn compact_if_sparse(&mut self) {
        let free: usize = self
            .free
            .iter()
            .map(|(capacity, blocks)| *capacity as usize * blocks.len())
            .sum();
        if self.instances.len() < MIN_COMPACT_SLOTS || free * 2 < self.instances.len() {
            return;
        }
        let live = self.instances.len() - free;
        let mut instances = Vec::with_capacity(live);
        let mut handles = Vec::with_capacity(live);
        for entry in self.entries.iter_mut().flatten() {
            let start = entry.block as usize;
            let end = start + entry.capacity as usize;
            entry.block = instances.len() as u32;
            instances.extend_from_slice(&self.instances[start..end]);
            handles.extend_from_slice(&self.handles[start..end]);
        }
        self.instances = instances;
        self.handles = handles;
        self.free.clear();
    }

    /// Drop everything. For a font-set change, where no entry means what it
    /// meant.
    pub(super) fn clear(&mut self, mut release: impl FnMut(GlyphAtlasEntryId)) {
        for entry in self.entries.iter_mut().filter_map(Option::take) {
            let start = entry.block as usize;
            for handle in &self.handles[start..start + entry.glyphs as usize] {
                release(*handle);
            }
            self.destroyed += 1;
        }
        self.entries.clear();
        self.index.clear();
        self.vacant.clear();
        self.instances.clear();
        self.handles.clear();
        self.free.clear();
        self.live_glyphs = 0;
    }

    fn take_block(&mut self, capacity: u32) -> u32 {
        if let Some(block) = self.free.get_mut(&capacity).and_then(Vec::pop) {
            return block;
        }
        let block = self.instances.len() as u32;
        self.instances.resize(
            self.instances.len() + capacity as usize,
            GlyphInstance::VACANT,
        );
        self.handles.resize(
            self.handles.len() + capacity as usize,
            GlyphAtlasEntryId::STALE,
        );
        block
    }
}

/// Slab size below which holes are left alone: moving a few thousand
/// instances to save a few thousand is not worth the copy.
const MIN_COMPACT_SLOTS: usize = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EntryCounters {
    pub active: u64,
    pub created: u64,
    pub destroyed: u64,
    pub reused: u64,
    pub glyphs: u64,
}

/// A draw command's spans of the index table, assembled from the entries under
/// it.
///
/// Segments coalesce across entries: two labels whose index ranges ended up
/// adjacent and whose glyphs came from the same pages are one draw, however different
/// their colors, positions or transforms are — those live in the run rows the
/// instances name, not in the batch key.
pub(super) struct SegmentBuilder {
    open: Option<DrawSegment>,
    placeholders: [u32; 2],
}

impl SegmentBuilder {
    pub(super) fn new(placeholders: [u32; 2]) -> Self {
        Self {
            open: None,
            placeholders,
        }
    }

    /// `base` is where the entry's range of the index table starts, and
    /// `adjacent` says it starts exactly where the previous entry's range
    /// ended. Only then may a draw span the gap: what lies between two
    /// adjacent ranges is the slack their size classes left, whose indices
    /// name vacant instances that cover nothing, while what lies between two
    /// ranges the order placed apart is other paragraphs' glyphs.
    pub(super) fn push(
        &mut self,
        out: &mut Vec<DrawSegment>,
        segment: EntrySegment,
        base: u32,
        adjacent: bool,
    ) {
        let first = base + segment.first;
        let compatible = adjacent
            && self.open.as_ref().is_some_and(|open| {
                open.first + open.count <= first
                    && (open.mask_page == segment.mask_page
                        || open.mask_page == self.placeholders[0]
                        || segment.mask_page == self.placeholders[0])
                    && (open.color_page == segment.color_page
                        || open.color_page == self.placeholders[1]
                        || segment.color_page == self.placeholders[1])
            });
        if compatible {
            let open = self.open.as_mut().expect("compatible implies open");
            if segment.mask_page != self.placeholders[0] {
                open.mask_page = segment.mask_page;
            }
            if segment.color_page != self.placeholders[1] {
                open.color_page = segment.color_page;
            }
            open.count = first + segment.count - open.first;
            return;
        }
        if let Some(open) = self.open.take() {
            out.push(open);
        }
        self.open = Some(DrawSegment {
            mask_page: segment.mask_page,
            color_page: segment.color_page,
            first,
            count: segment.count,
        });
    }

    pub(super) fn finish(mut self, out: &mut Vec<DrawSegment>) {
        if let Some(open) = self.open.take() {
            out.push(open);
        }
    }
}

/// A slot allocator whose blocks keep their offset for as long as they live.
///
/// A target holds two of them, and together they are what lets draw order and
/// storage order be different things:
///
/// - the **arena** says where each entry's instances sit in the instance
///   buffer. A paragraph that changes costs its own bytes and nobody else's,
///   even when it outgrows its block: it is simply placed somewhere else.
///   Space given back merges with the space beside it, so the holes growing
///   paragraphs leave are taken by the next blocks instead of pushing the
///   arena towards a repack.
/// - the **order** says where each entry's range of the index table sits. The
///   table is what a draw spans — the vertex stage reads an arena slot from
///   it and the instance from there — so two entries batch when their
///   *ranges* are adjacent, wherever their instances are. A range that had to
///   be placed elsewhere costs its command one more draw, and when enough of
///   them have accumulated the order is repacked: the index table is written
///   again in draw order, four bytes a slot, and not one instance moves.
///   Where the text under it keeps growing, the repack leaves clean gaps (see
///   [`Extents`]) that ranges grow into instead of moving.
///
/// The generation is what makes that safe: a repack or a buffer replacement
/// bumps it, and a block placed under an older one is written again rather
/// than left naming a range that now holds another paragraph.
#[derive(Default)]
pub(super) struct SlotArena {
    /// Slots handed out, including the holes between them.
    len: u32,
    /// Slots the GPU buffer holds.
    capacity: u32,
    /// Slots inside live blocks.
    live: u32,
    /// Free space, merged with its neighbours as it is given back.
    free: Extents,
    /// Commands that had to open a second draw because two of their entries
    /// were not adjacent, last frame. Only the order counts them.
    breaks: u32,
    generation: u64,
    /// The most slots the GPU buffer may hold; see [`Self::set_limit`].
    limit: Option<u32>,
}

/// Extra draws fragmentation may cost before the order is repacked.
///
/// A repack rewrites every index, so what it costs grows with the text on
/// screen; an extra draw costs the same few microseconds however much text
/// there is. A shell with a few hundred glyphs is therefore repacked as soon
/// as it fragments at all, and one with fifty thousand tolerates a few dozen
/// extra draws rather than rewrite the table every frame.
fn break_budget(total: u32) -> u32 {
    BREAK_FLOOR.max(total / SLOTS_PER_BREAK)
}

const BREAK_FLOOR: u32 = 4;
/// Index slots whose rewrite is worth one draw fewer. Four kilobytes: when an
/// order repack rewrote instances too this was a sixth of it and still the
/// balance point, so the index table on its own sits well inside it.
const SLOTS_PER_BREAK: u32 = 1024;

/// Slots a fresh target's arena starts at.
const MIN_ARENA: u32 = 512;

/// `slots` rounded up to one of eight sizes per doubling, so a buffer that
/// tracks a slowly growing set is replaced a few times per doubling rather
/// than on every repack, and never holds more than an eighth it was not
/// asked for.
fn size_class(slots: u32) -> u32 {
    if slots <= 8 {
        return slots;
    }
    round_to_class(slots, 8)
}

impl SlotArena {
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Slots handed out, holes included.
    #[cfg(test)]
    pub(super) fn len(&self) -> u32 {
        self.len
    }

    /// Never size the buffer past `slots`. The caller keeps what a frame
    /// places within it; this only stops the room a repack adds on top from
    /// crossing it.
    pub(super) fn set_limit(&mut self, slots: u32) {
        self.limit = Some(slots);
    }

    pub(super) fn note_breaks(&mut self, breaks: u32) {
        self.breaks = breaks;
    }

    /// Whether this frame should lay every block out again.
    ///
    /// `fresh` is what the frame is about to ask for that it does not already
    /// hold, so a repack happens *instead of* running out of room rather than
    /// after it.
    pub(super) fn should_repack(&self, total: u32, fresh: &[u32]) -> bool {
        let wanted = fresh.iter().fold(0u32, |sum, len| sum.saturating_add(*len));
        self.breaks > break_budget(total)
            || (self.len.saturating_add(wanted) > self.capacity && !self.fits(fresh))
            || total > self.capacity
            // Half the arena is holes left by paragraphs that changed size or
            // went away. Nothing is wrong, but the buffer is twice the size it
            // needs to be and every new block lands further from its
            // neighbours.
            || (self.len > 64 && self.live * 2 < self.len)
    }

    /// Whether `fresh` can be placed without growing past the buffer, taking
    /// holes the way [`Self::alloc`] would. Blocks the walk gives back first
    /// are not counted, so the answer errs towards a repack.
    fn fits(&self, fresh: &[u32]) -> bool {
        // The same best fit `alloc` makes, over the extents as they are plus
        // what this simulation has taken or split so far — never a copy of
        // the maps, which hold thousands of extents once holes are reused.
        let mut taken: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
        let mut split: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
        let mut len = self.len;
        for &want in fresh {
            let base = self
                .free
                .by_size
                .range((want, 0)..)
                .find(|extent| !taken.contains(extent))
                .copied();
            let rest = split.range((want, 0)..).next().copied();
            let pick = [
                base.map(|extent| (extent, false)),
                rest.map(|extent| (extent, true)),
            ]
            .into_iter()
            .flatten()
            .min();
            match pick {
                Some(((size, offset), from_split)) => {
                    if from_split {
                        split.remove(&(size, offset));
                    } else {
                        taken.insert((size, offset));
                    }
                    if size > want {
                        split.insert((size - want, offset + want));
                    }
                }
                None => {
                    len = len.saturating_add(want);
                    if len > self.capacity {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Leave `len` slots of room after everything placed so far: a gap the
    /// caller fills with [`VACANT_INDEX`], so a draw may run across it and a
    /// range just before it can grow into it. `None` when the buffer has no
    /// room for it.
    pub(super) fn reserve_gap(&mut self, len: u32) -> Option<u32> {
        if self.len.saturating_add(len) > self.capacity {
            return None;
        }
        let offset = self.len;
        self.len += len;
        self.free.insert(offset, len, true);
        Some(offset)
    }

    /// Let a range grow by `len` into the clean extent that starts at
    /// `offset`, right after it; [`Self::free_at`] said it is that big.
    pub(super) fn grow_into(&mut self, offset: u32, len: u32) {
        let took = self.free.take_front(offset, len);
        debug_assert!(took, "grew into a gap that was not there");
        self.live += len;
    }

    /// The free extent starting exactly at `offset`, and whether it is clean.
    pub(super) fn free_at(&self, offset: u32) -> Option<(u32, bool)> {
        self.free.at(offset)
    }

    /// Whether `start..end` is exactly one clean extent, which a draw may
    /// span.
    pub(super) fn clean_between(&self, start: u32, end: u32) -> bool {
        end > start && self.free.at(start) == Some((end - start, true))
    }

    #[cfg(test)]
    pub(super) fn clean_at(&self, position: u32) -> bool {
        self.free.clean_at(position)
    }

    /// Give back everything, keeping the buffer. For the case where no entry
    /// means what it meant and they are all dropped at once.
    pub(super) fn reset(&mut self) {
        self.len = 0;
        self.live = 0;
        self.free = Extents::default();
        self.breaks = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Start a repack: every offset handed out before this is void.
    ///
    /// The buffer is sized so that a third of it is free afterwards. A repack
    /// that only just fits what the frame holds is followed by another as
    /// soon as a few blocks move, and each one rewrites everything; with a
    /// third free, the next one waits until as many slots have moved as half
    /// of what is live, so what repacking costs per frame is about what moved.
    pub(super) fn repack(&mut self, total: u32) {
        self.reset();
        let roomy = total.saturating_add(total / 2);
        if roomy > self.capacity || total.saturating_mul(4) < self.capacity {
            // Shrinking happens here too: every block is about to be written
            // again anyway, so this is the one moment giving memory back
            // costs nothing extra. A quarter, not a half, so a set that
            // hovers near a size does not replace the buffer on every repack.
            self.capacity = size_class(roomy).max(MIN_ARENA);
        }
        if let Some(limit) = self.limit {
            self.capacity = self.capacity.min(limit.max(total));
        }
    }

    pub(super) fn alloc(&mut self, capacity: u32) -> u32 {
        self.live += capacity;
        if let Some(offset) = self.free.take(capacity) {
            return offset;
        }
        let offset = self.len;
        self.len += capacity;
        // Never reached: `should_repack` is asked before the walk with exactly
        // what the walk is about to allocate, and a repack is what makes room.
        // Growing here instead would replace the buffer after the frame had
        // already decided which blocks to write, and the ones it skipped would
        // not be on the new one. So the capacity only moves when the block
        // really does not fit — never to round up a buffer the repack sized
        // off a power of two.
        debug_assert!(
            self.len <= self.capacity,
            "the arena grew past the buffer the repack sized"
        );
        if self.len > self.capacity {
            self.capacity = size_class(self.len).max(MIN_ARENA);
        }
        offset
    }

    /// Give a block back. Ignored when it belongs to an older layout, whose
    /// offsets the repack already forgot.
    pub(super) fn release(&mut self, generation: u64, offset: u32, capacity: u32) {
        if generation != self.generation {
            return;
        }
        self.live = self.live.saturating_sub(capacity);
        self.free.give(offset, capacity, false);
        // Free space that reaches the end is not a hole, it is room.
        if let Some(end) = self.free.tail(self.len) {
            self.len = end;
        }
    }
}

/// Free space in a [`SlotArena`]: extents that merge when they touch.
///
/// An extent is *clean* when every index slot in it is known to hold
/// [`VACANT_INDEX`]: a draw may run straight across it, because the vertex
/// stage culls those slots without reading an instance. Only the order ever
/// makes clean extents; everything the arena gives back is not.
#[derive(Default)]
struct Extents {
    by_offset: std::collections::BTreeMap<u32, (u32, bool)>,
    by_size: std::collections::BTreeSet<(u32, u32)>,
}

impl Extents {
    /// Clean extents are kept out of the size index: they are room for the
    /// ranges beside them to grow into, not space for new ranges.
    fn insert(&mut self, offset: u32, len: u32, clean: bool) {
        self.by_offset.insert(offset, (len, clean));
        if !clean {
            self.by_size.insert((len, offset));
        }
    }

    fn remove(&mut self, offset: u32) -> Option<(u32, bool)> {
        let (len, clean) = self.by_offset.remove(&offset)?;
        if !clean {
            self.by_size.remove(&(len, offset));
        }
        Some((len, clean))
    }

    fn give(&mut self, mut offset: u32, mut len: u32, mut clean: bool) {
        if let Some((&before, &(before_len, _))) = self.by_offset.range(..offset).next_back()
            && before + before_len == offset
        {
            let (_, before_clean) = self.remove(before).expect("just found");
            offset = before;
            len += before_len;
            clean &= before_clean;
        }
        if let Some((after_len, after_clean)) = self.remove(offset + len) {
            len += after_len;
            clean &= after_clean;
        }
        self.insert(offset, len, clean);
    }

    /// The smallest extent that holds `len`, split so the rest stays free.
    fn take(&mut self, len: u32) -> Option<u32> {
        let &(size, offset) = self.by_size.range((len, 0)..).next()?;
        let (_, clean) = self.remove(offset).expect("indexed");
        if size > len {
            self.insert(offset + len, size - len, clean);
        }
        Some(offset)
    }

    /// Take the first `len` slots of the clean extent that starts at `offset`.
    fn take_front(&mut self, offset: u32, len: u32) -> bool {
        match self.by_offset.get(&offset) {
            Some(&(size, true)) if size >= len => {
                self.remove(offset);
                if size > len {
                    self.insert(offset + len, size - len, true);
                }
                true
            }
            _ => false,
        }
    }

    fn at(&self, offset: u32) -> Option<(u32, bool)> {
        self.by_offset.get(&offset).copied()
    }

    /// Whether `position` lies in a clean extent.
    #[cfg(test)]
    fn clean_at(&self, position: u32) -> bool {
        self.by_offset
            .range(..=position)
            .next_back()
            .is_some_and(|(&offset, &(len, clean))| clean && position < offset + len)
    }

    /// If free space runs up to `end`, where it starts; that much is given
    /// back to the end of the arena rather than kept as a hole.
    fn tail(&mut self, end: u32) -> Option<u32> {
        let (&offset, &(len, _)) = self.by_offset.range(..end).next_back()?;
        if offset + len != end {
            return None;
        }
        self.remove(offset);
        Some(offset)
    }
}

/// Row indices into the run table, handed out per entry and given back when it
/// retires.
#[derive(Default)]
pub(super) struct RunSlots {
    next: u32,
    free: Vec<u32>,
}

impl RunSlots {
    pub(super) fn alloc(&mut self) -> u32 {
        if let Some(slot) = self.free.pop() {
            return slot;
        }
        let slot = self.next;
        self.next += 1;
        slot
    }

    pub(super) fn release(&mut self, slot: u32) {
        self.free.push(slot);
    }

    /// Give back every row. For the case where every entry is dropped at once.
    pub(super) fn reset(&mut self) {
        self.next = 0;
        self.free.clear();
    }

    /// Rows the table must hold for every live slot to be addressable.
    pub(super) fn len(&self) -> usize {
        self.next as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(node: u64) -> EntryKey {
        EntryKey {
            node,
            slot: 0,
            pass: 0,
        }
    }

    /// Build `key` with `glyphs` live glyphs whose handles are `first..`.
    fn build(store: &mut EntryStore, key: EntryKey, glyphs: u32, first: u32) -> Vec<u32> {
        let mut released = Vec::new();
        let id = store.begin_build(key, glyphs, |handle| {
            released.push(handle);
        });
        for offset in 0..glyphs {
            store.push_glyph(
                id,
                offset,
                GlyphAtlasEntryId::for_test(first + offset),
                GlyphInstance::VACANT,
            );
        }
        store.finish_build(id, glyphs);
        released
            .into_iter()
            .map(|handle| {
                (0..u32::MAX)
                    .find(|index| GlyphAtlasEntryId::for_test(*index) == handle)
                    .expect("a test handle")
            })
            .collect()
    }

    #[test]
    fn a_rebuild_gives_back_every_claim_the_old_block_made() {
        let mut store = EntryStore::default();
        build(&mut store, key(1), 10, 100);
        let released = build(&mut store, key(1), 3, 200);
        assert_eq!(
            released,
            (100..110).collect::<Vec<_>>(),
            "a paragraph rebuilt shorter must release all ten glyphs it held, \
             or the atlas keeps counting claims nobody makes"
        );
        let released = build(&mut store, key(1), 12, 300);
        assert_eq!(
            released,
            (200..203).collect::<Vec<_>>(),
            "and rebuilt longer, only the three it held, never a vacant slot"
        );
    }

    #[test]
    fn a_paragraph_that_changes_length_within_its_slack_keeps_its_block() {
        let mut store = EntryStore::default();
        build(&mut store, key(1), 11, 0);
        let id = store.lookup(key(1)).expect("built");
        let (block, capacity) = {
            let entry = store.get(id).expect("live");
            (entry.block, entry.capacity)
        };
        for glyphs in [12, 10, capacity] {
            build(&mut store, key(1), glyphs, 0);
            let entry = store.get(id).expect("live");
            assert_eq!(
                (entry.block, entry.capacity),
                (block, capacity),
                "{glyphs} glyphs fit the block of {capacity} a paragraph of 11 was given"
            );
        }
        build(&mut store, key(1), capacity + 1, 0);
        let grown = {
            let entry = store.get(id).expect("live");
            (entry.block, entry.capacity)
        };
        assert!(
            grown.1 > capacity,
            "outgrowing it moves the paragraph to a bigger class"
        );
        build(&mut store, key(1), 1, 0);
        let entry = store.get(id).expect("live");
        assert_eq!(
            (entry.block, entry.capacity),
            grown,
            "a short label that shrinks again keeps its block: moving it would \
             cost more than the few slots it frees"
        );
        build(&mut store, key(1), 600, 0);
        build(&mut store, key(1), 20, 0);
        assert_eq!(
            store.get(id).expect("live").capacity,
            capacity_for(20),
            "but a paragraph that lost most of a page gives the space back"
        );
    }

    #[test]
    fn size_classes_are_monotonic_and_leave_the_slack_they_promise() {
        let mut previous = 0;
        for glyphs in 0..5000u32 {
            let capacity = capacity_for(glyphs);
            assert!(capacity >= glyphs + (glyphs / 8).max(4), "{glyphs}");
            assert!(capacity >= previous, "{glyphs}");
            // A class step is a quarter of the power of two below what was
            // asked for, so rounding up costs at most a quarter again.
            let need = glyphs + (glyphs / 8).max(4);
            assert!(
                capacity <= (need + need / 4).max(8),
                "{glyphs} -> {capacity}"
            );
            previous = capacity;
        }
    }

    #[test]
    fn retiring_most_of_the_slab_compacts_it_without_losing_a_survivor() {
        let mut store = EntryStore::default();
        for node in 0..400u64 {
            build(&mut store, key(node), 20, node as u32 * 100);
            let id = store.lookup(key(node)).expect("built");
            store.get_mut(id).expect("live").last_used = if node % 10 == 0 { 9 } else { 1 };
        }
        let before = store.instances.len();
        store.retire(5, |_| {}, |_| {}, |_, _, _| {}, |_, _, _| {});
        assert!(
            store.instances.len() * 4 < before,
            "nine in ten retired, so the slab must shrink: {before} -> {}",
            store.instances.len()
        );
        for node in (0..400u64).step_by(10) {
            let id = store.lookup(key(node)).expect("a survivor");
            let entry = store.get(id).expect("live");
            let handles = store
                .live_glyphs(entry)
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>();
            let expected = (0..20)
                .map(|offset| GlyphAtlasEntryId::for_test(node as u32 * 100 + offset))
                .collect::<Vec<_>>();
            assert_eq!(handles, expected, "node {node} still holds its own glyphs");
        }
    }

    #[test]
    fn space_two_neighbours_give_back_holds_a_block_of_both_their_sizes() {
        let mut arena = SlotArena::default();
        arena.repack(64);
        let [a, b, _c] = [arena.alloc(8), arena.alloc(8), arena.alloc(8)];
        let end = arena.len();
        arena.release(arena.generation(), a, 8);
        arena.release(arena.generation(), b, 8);
        assert_eq!(
            arena.alloc(16),
            a,
            "two holes side by side are one hole: a block of sixteen fits \
             where two of eight were, rather than going to the end"
        );
        assert_eq!(arena.len(), end);
    }

    #[test]
    fn space_given_back_at_the_end_is_room_not_a_hole() {
        let mut arena = SlotArena::default();
        arena.repack(64);
        let [_a, b, c] = [arena.alloc(8), arena.alloc(8), arena.alloc(8)];
        arena.release(arena.generation(), c, 8);
        assert_eq!(
            arena.len(),
            c,
            "the last block given back shortens the arena"
        );
        arena.release(arena.generation(), b, 8);
        assert_eq!(
            arena.len(),
            b,
            "and so does the one before it, once it is last"
        );
    }

    #[test]
    fn a_frame_the_room_check_lets_through_never_outgrows_the_buffer() {
        // Whatever holes a session leaves, `should_repack` saying no must
        // mean every block the frame asks for is placed inside the buffer:
        // growing it mid-walk loses the blocks written before (see
        // `filling_a_buffer_the_repack_sized_does_not_replace_it`).
        let mut seed = 0x9e37_79b9_u32;
        let mut next = move |bound: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % bound
        };
        let mut arena = SlotArena::default();
        arena.repack(4096);
        let mut live: Vec<(u32, u32)> = Vec::new();
        for _ in 0..400 {
            for _ in 0..next(12) {
                if !live.is_empty() {
                    let (offset, len) = live.swap_remove(next(live.len() as u32) as usize);
                    arena.release(arena.generation(), offset, len);
                }
            }
            let fresh = (0..1 + next(12))
                .map(|_| [8, 10, 12, 14, 16, 20, 24, 40][next(8) as usize])
                .collect::<Vec<u32>>();
            let total = live.iter().map(|(_, len)| len).sum::<u32>() + fresh.iter().sum::<u32>();
            if arena.should_repack(total, &fresh) {
                // What the frame draws is placed again from nothing.
                arena.repack(total);
                live.clear();
                live.extend(fresh.iter().map(|len| (arena.alloc(*len), *len)));
                continue;
            }
            let capacity = arena.capacity();
            for len in fresh {
                let offset = arena.alloc(len);
                assert!(
                    offset + len <= capacity && arena.capacity() == capacity,
                    "the room check let through a block that does not fit"
                );
                live.push((offset, len));
            }
        }
    }

    #[test]
    fn filling_a_buffer_the_repack_sized_does_not_replace_it() {
        let mut arena = SlotArena::default();
        arena.repack(376);
        let sized = arena.capacity();
        assert!(
            !sized.is_power_of_two(),
            "the case that matters: {sized} is a size class, not a power of two"
        );
        while arena.len() + 16 <= sized {
            arena.alloc(16);
            assert_eq!(
                arena.capacity(),
                sized,
                "a block that fits must not change the capacity: that replaces \
                 the GPU buffer mid-frame and loses every block written before"
            );
        }
    }

    #[test]
    fn a_repack_never_sizes_the_buffer_past_the_device_limit() {
        let mut arena = SlotArena::default();
        arena.set_limit(600);
        arena.repack(500);
        assert_eq!(
            arena.capacity(),
            600,
            "the third of room a repack adds stops at what the device binds"
        );
        arena.repack(100);
        assert!(arena.capacity() <= 600);
        assert!(arena.capacity() >= 100);
    }

    #[test]
    fn a_repack_after_the_text_went_away_gives_the_buffer_back() {
        let mut arena = SlotArena::default();
        arena.repack(40_000);
        let peak = arena.capacity();
        assert!(peak >= 40_000);
        arena.repack(peak / 3);
        assert_eq!(
            arena.capacity(),
            peak,
            "a third of it is not worth a new buffer"
        );
        arena.repack(300);
        assert!(
            arena.capacity() < peak / 8,
            "a list that closed must not keep its GPU memory: {}",
            arena.capacity()
        );
        assert!(arena.capacity() >= 300);
    }
}
