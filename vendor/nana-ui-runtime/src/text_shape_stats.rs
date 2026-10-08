//! Zero-work TextShape attribution for tests and `feature = "benchmark"`.
//! Not `WorkCounters`: those stay the scheduled TEXT / cache hit-miss fields.

use std::cell::Cell;
#[cfg(feature = "benchmark")]
use std::time::Instant;

thread_local! {
    static SCOPE_NODES: Cell<usize> = const { Cell::new(0) };
    static NONEMPTY_TEXT_NODES: Cell<usize> = const { Cell::new(0) };
    static STRING_CLONES: Cell<usize> = const { Cell::new(0) };
    static STRING_CLONE_BYTES: Cell<usize> = const { Cell::new(0) };
    static KEY_BUILDS: Cell<usize> = const { Cell::new(0) };
    static CACHE_LOOKUPS: Cell<usize> = const { Cell::new(0) };
    static SKIPPED_UNCHANGED: Cell<usize> = const { Cell::new(0) };
    static BRACKET_RESCANS: Cell<usize> = const { Cell::new(0) };
    static CLONE_NS: Cell<u64> = const { Cell::new(0) };
    static KEY_NS: Cell<u64> = const { Cell::new(0) };
    static LOOKUP_NS: Cell<u64> = const { Cell::new(0) };
    static INNER_SHAPE_NS: Cell<u64> = const { Cell::new(0) };
}

/// One TextShape pass (or the sum of `shape_text` + layout-scoped reshape
/// in a single frame, if the caller does not reset between them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextShapePassStats {
    pub scope_nodes: usize,
    pub nonempty_text_nodes: usize,
    pub string_clones: usize,
    pub string_clone_bytes: usize,
    pub key_builds: usize,
    pub cache_lookups: usize,
    pub skipped_unchanged: usize,
    /// Whole-document bracket stack scans. An edit that touches no bracket
    /// character must not need one (`world::text::bracket_color_spans_cached`).
    pub bracket_rescans: usize,
    pub clone_ns: u64,
    pub key_ns: u64,
    pub lookup_ns: u64,
    pub inner_shape_ns: u64,
}

pub fn reset() {
    SCOPE_NODES.with(|cell| cell.set(0));
    NONEMPTY_TEXT_NODES.with(|cell| cell.set(0));
    STRING_CLONES.with(|cell| cell.set(0));
    STRING_CLONE_BYTES.with(|cell| cell.set(0));
    KEY_BUILDS.with(|cell| cell.set(0));
    CACHE_LOOKUPS.with(|cell| cell.set(0));
    SKIPPED_UNCHANGED.with(|cell| cell.set(0));
    BRACKET_RESCANS.with(|cell| cell.set(0));
    CLONE_NS.with(|cell| cell.set(0));
    KEY_NS.with(|cell| cell.set(0));
    LOOKUP_NS.with(|cell| cell.set(0));
    INNER_SHAPE_NS.with(|cell| cell.set(0));
}

pub fn snapshot() -> TextShapePassStats {
    TextShapePassStats {
        scope_nodes: SCOPE_NODES.with(Cell::get),
        nonempty_text_nodes: NONEMPTY_TEXT_NODES.with(Cell::get),
        string_clones: STRING_CLONES.with(Cell::get),
        string_clone_bytes: STRING_CLONE_BYTES.with(Cell::get),
        key_builds: KEY_BUILDS.with(Cell::get),
        cache_lookups: CACHE_LOOKUPS.with(Cell::get),
        skipped_unchanged: SKIPPED_UNCHANGED.with(Cell::get),
        bracket_rescans: BRACKET_RESCANS.with(Cell::get),
        clone_ns: CLONE_NS.with(Cell::get),
        key_ns: KEY_NS.with(Cell::get),
        lookup_ns: LOOKUP_NS.with(Cell::get),
        inner_shape_ns: INNER_SHAPE_NS.with(Cell::get),
    }
}

pub(crate) fn note_scope(nodes: usize) {
    SCOPE_NODES.with(|cell| cell.set(cell.get().saturating_add(nodes)));
}

pub(crate) fn note_nonempty() {
    NONEMPTY_TEXT_NODES.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn note_clone(bytes: usize) {
    STRING_CLONES.with(|cell| cell.set(cell.get().saturating_add(1)));
    STRING_CLONE_BYTES.with(|cell| cell.set(cell.get().saturating_add(bytes)));
}

pub(crate) fn note_key_build() {
    KEY_BUILDS.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn note_lookup() {
    CACHE_LOOKUPS.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn note_skipped_unchanged() {
    SKIPPED_UNCHANGED.with(|cell| cell.set(cell.get().saturating_add(1)));
}

pub(crate) fn note_bracket_rescan() {
    BRACKET_RESCANS.with(|cell| cell.set(cell.get().saturating_add(1)));
}

#[inline]
pub(crate) fn timed_clone<T>(work: impl FnOnce() -> T) -> T {
    timed(&CLONE_NS, work)
}

#[inline]
pub(crate) fn timed_key<T>(work: impl FnOnce() -> T) -> T {
    timed(&KEY_NS, work)
}

#[inline]
pub(crate) fn timed_lookup<T>(work: impl FnOnce() -> T) -> T {
    timed(&LOOKUP_NS, work)
}

#[inline]
pub(crate) fn timed_inner_shape<T>(work: impl FnOnce() -> T) -> T {
    timed(&INNER_SHAPE_NS, work)
}

#[cfg(feature = "benchmark")]
#[inline]
fn timed<T>(acc: &'static std::thread::LocalKey<Cell<u64>>, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = work();
    acc.with(|cell| {
        cell.set(
            cell.get()
                .saturating_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)),
        );
    });
    value
}

#[cfg(not(feature = "benchmark"))]
#[inline]
fn timed<T>(_acc: &'static std::thread::LocalKey<Cell<u64>>, work: impl FnOnce() -> T) -> T {
    work()
}
