//! Live heap per retained node: allocations minus frees, counted per thread
//! so parallel tests do not disturb each other.
//!
//! A `LayoutStyle` is about 2 KB. Nodes whose components build equal layouts
//! share one (`ComponentView::share_layouts`, the world's recent-layout
//! cache), so a thousand equal buttons hold one layout, not two thousand.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use nana_ui_runtime::view::{column, widget};
use nana_ui_runtime::{AppContext, Button, ComponentView, DocumentId, Stack, Text, TextInput};

struct Counting;

thread_local! {
    static LIVE: Cell<i64> = const { Cell::new(0) };
}

fn add(bytes: i64) {
    let _ = LIVE.try_with(|live| live.set(live.get() + bytes));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add(layout.size() as i64);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        add(layout.size() as i64);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        add(new_size as i64 - layout.size() as i64);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        add(-(layout.size() as i64));
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Live bytes per node after building `n` nodes from `make`.
fn live_per_node<C: ComponentView>(n: usize, make: impl Fn(usize) -> C + Copy + 'static) -> f64 {
    let document = DocumentId::new(1).unwrap();
    let mut cx = AppContext::new();
    // Grow the context's tables and the process-wide defaults first.
    cx.mount_view_root(document, || {
        column().children((0..8).map(|i| widget(make(i))).collect::<Vec<_>>())
    })
    .unwrap();
    let before = LIVE.with(Cell::get);
    cx.mount_view_root(document, || {
        column().children((0..n).map(|i| widget(make(i))).collect::<Vec<_>>())
    })
    .unwrap();
    let per_node = (LIVE.with(Cell::get) - before) as f64 / n as f64;
    drop(cx);
    per_node
}

#[test]
fn equal_nodes_share_their_layouts() {
    const N: usize = 2_000;
    let text = live_per_node(N, |i| Text::new(format!("行 {i}")));
    let stack = live_per_node(N, |_| Stack::column(0.0));
    let button = live_per_node(N, |i| Button::new(format!("按钮 {i}")));
    let input = live_per_node(N, |i| TextInput::new(format!("值 {i}")));
    eprintln!(
        "live bytes per node: text {text:.0}, stack {stack:.0}, button {button:.0}, \
         text input {input:.0}"
    );
    // Each would be at least one ~2 KB layout above these if every node
    // kept its own (a button kept three: its own, its projection's and its
    // resolved one).
    assert!(stack < 2_400.0, "{stack:.0} B per stack");
    assert!(button < 3_000.0, "{button:.0} B per button");
    assert!(input < 4_400.0, "{input:.0} B per text input");
}
