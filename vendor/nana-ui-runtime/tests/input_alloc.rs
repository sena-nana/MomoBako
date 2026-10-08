//! Steady pointer input allocates nothing on the routing path.
//!
//! The counting allocator counts per thread, so each test sees only its own
//! allocations. Each case warms up first (hit scratch, pointer
//! identity, hover and cursor slots), then counts a steady run of moves:
//!
//! - uncaptured moves across a flat column, hover changing every event;
//! - the same inside a scroll viewport, where hit testing orders candidates
//!   by paint;
//! - moves while a node holds the pointer's capture;
//! - moves that cross rows, changing hover on every event.
//!
//! Each uncaptured move also costs exactly one hit query and a captured one
//! none, read from the world's own count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use nana_ui_core::{LengthSpec, OverflowSpec};
use nana_ui_input::{InputPayload, PointerInput, PointerPhase};
use nana_ui_runtime::view::widget;
use nana_ui_runtime::{
    AppContext, DocumentId, HeadlessInput, LayoutViewport, MeasureTextShaper, MutationQueue,
    StableNodeId, Stack, Text,
};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

struct CountingAllocator;

fn record(pointer: *mut u8) {
    if !pointer.is_null() && COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

// SAFETY: every call forwards the caller's pointer and layout unchanged to
// System; accounting touches only allocation-free thread-local cells.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record(pointer);
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(pointer);
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        record(pointer);
        pointer
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const ROWS: usize = 60;
const ROW_HEIGHT: f32 = 20.0;
const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 300.0;

/// A column of rows, laid out and indexed; inside a scrolling box when
/// `scrolled`.
fn column(scrolled: bool) -> (AppContext, DocumentId) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    context
        .mount_view_root(document, || {
            let mut root = Stack::column(0.0);
            if scrolled {
                root = root.with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(WIDTH));
                    layout.height = Some(LengthSpec::Px(HEIGHT));
                    layout.overflow_y = OverflowSpec::Scroll;
                });
            }
            let rows = (0..ROWS).map(|row| {
                // A hittable row, so every move lands on a node and hover
                // changes as the pointer crosses rows.
                let row_stack = Stack::row(0.0).hittable().with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(WIDTH));
                    layout.height = Some(LengthSpec::Px(ROW_HEIGHT));
                });
                widget(row_stack).children(widget(Text::new(format!("Row {row}"))))
            });
            widget(root).children(rows.collect::<Vec<_>>())
        })
        .unwrap();
    let nodes = context.world().document_order(document);
    context.resolve_styles(&nodes).unwrap();
    context.shape_text(&nodes, &mut MeasureTextShaper).unwrap();
    context
        .layout_document(document, LayoutViewport::new(WIDTH, HEIGHT))
        .unwrap();
    context.rebuild_hit_test(document);
    (context, document)
}

fn row_center(row: usize) -> (f32, f32) {
    (WIDTH / 2.0, row as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0)
}

/// A point on `row`, `step` pixels along it.
fn on_row(row: usize, step: usize) -> (f32, f32) {
    (
        20.0 + (step % 200) as f32,
        row as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0,
    )
}

/// Where move `index` goes: along one row, or down a row every move.
fn target(index: usize, across: bool) -> (f32, f32) {
    if across {
        row_center(index % 12)
    } else {
        on_row(2, index)
    }
}

fn moves(
    input: &mut HeadlessInput,
    context: &mut AppContext,
    from: usize,
    count: usize,
    across: bool,
) {
    for index in from..from + count {
        let (x, y) = target(index, across);
        input
            .route(
                context,
                InputPayload::Pointer(PointerInput::mouse(PointerPhase::Move, x, y)),
            )
            .unwrap();
    }
}

fn counted(run: impl FnOnce()) -> u64 {
    ALLOCATIONS.with(|allocations| allocations.set(0));
    COUNTING.with(|counting| counting.set(true));
    run();
    COUNTING.with(|counting| counting.set(false));
    ALLOCATIONS.with(Cell::get)
}

/// Allocations and hit queries a run of `count` moves along one row costs,
/// after a warm-up.
fn steady_moves(context: &mut AppContext, document: DocumentId, count: usize) -> (u64, u64) {
    let mut input = HeadlessInput::bind(context, document);
    steady_moves_on(&mut input, context, count)
}

fn steady_moves_on(
    input: &mut HeadlessInput,
    context: &mut AppContext,
    count: usize,
) -> (u64, u64) {
    moves(input, context, 0, 48, false);
    let queries = context.world().hit_test_queries();
    let allocations = counted(|| moves(input, context, 48, count, false));
    (allocations, context.world().hit_test_queries() - queries)
}

#[test]
fn steady_pointer_moves_allocate_nothing_and_hit_test_once() {
    let (mut context, document) = column(false);
    assert_eq!(
        steady_moves(&mut context, document, 240),
        (0, 240),
        "uncaptured moves over a column: (allocations, hit queries)"
    );

    let (mut context, document) = column(true);
    assert_eq!(
        steady_moves(&mut context, document, 240),
        (0, 240),
        "uncaptured moves in a scroll viewport: (allocations, hit queries)"
    );

    let (mut context, document) = column(false);
    // Hold the row the pointer rests on, as a press on it would.
    let mut probe = HeadlessInput::bind(&mut context, document);
    let (x, y) = row_center(2);
    probe
        .route(
            &mut context,
            InputPayload::Pointer(PointerInput::mouse(PointerPhase::Move, x, y)),
        )
        .unwrap();
    let held = context
        .world()
        .pointer_hover(document, 1)
        .expect("the pointer rests on a row");
    let mut capture = MutationQueue::new();
    capture.capture_pointer(1, held);
    context.commit_mutations(capture).unwrap();
    assert_eq!(
        context.world().pointer_capture(document, 1),
        Some::<StableNodeId>(held)
    );
    assert_eq!(
        steady_moves_on(&mut probe, &mut context, 240),
        (0, 0),
        "captured moves: (allocations, hit queries)"
    );
}

/// Crossing rows changes hover on every move: hover state, the tooltip,
/// hover card and sidebar bookkeeping all run, and still allocate nothing.
#[test]
fn moves_that_change_hover_allocate_nothing() {
    let (mut context, document) = column(false);
    let mut input = HeadlessInput::bind(&mut context, document);
    moves(&mut input, &mut context, 0, 48, true);
    let hover_changes = context.input_counters().hover_changes;
    let allocations = counted(|| moves(&mut input, &mut context, 48, 240, true));
    assert_eq!(context.input_counters().hover_changes - hover_changes, 240);
    assert_eq!(allocations, 0);
}
