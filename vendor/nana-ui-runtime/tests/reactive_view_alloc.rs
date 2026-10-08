//! Heap cost of the declarative view layer, counted per thread so parallel
//! tests do not disturb each other.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use nana_ui_runtime::view::{IntoView, Signal, button, column, signal, text, watch_effect};
use nana_ui_runtime::{AppContext, DocumentId, StableNodeId, Stack};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

fn count(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    let _ = BYTES.try_with(|total| total.set(total.get() + bytes as u64));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations and bytes `f` makes on this thread.
fn measure<R>(f: impl FnOnce() -> R) -> (u64, u64, R) {
    let (a0, b0) = (ALLOCATIONS.with(Cell::get), BYTES.with(Cell::get));
    let result = f();
    (
        ALLOCATIONS.with(Cell::get) - a0,
        BYTES.with(Cell::get) - b0,
        result,
    )
}

fn setup() -> (AppContext, StableNodeId) {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    (cx, parent.stable_id())
}

#[test]
fn rerunning_an_effect_with_unchanged_dependencies_allocates_nothing() {
    let (mut cx, parent) = setup();
    let source = Cell::new(None);
    cx.mount_view(parent, || {
        let count = signal(0u64);
        source.set(Some(count));
        watch_effect(move || {
            count.get();
        });
        text("x")
    })
    .unwrap();
    let count = source.get().unwrap();
    // Warm the queue and pools once.
    count.set(1);
    cx.flush_reactive().unwrap();
    let (allocations, _, ()) = measure(|| {
        count.set(2);
        cx.flush_reactive().unwrap();
    });
    assert_eq!(allocations, 0);
}

fn bound_rows(n: usize, bound: bool) -> impl IntoView {
    let label = signal(String::from("行"));
    column().children(
        (0..n)
            .map(|_| {
                if bound {
                    text(label).into_any()
                } else {
                    text("行").into_any()
                }
            })
            .collect::<Vec<_>>(),
    )
}

/// Reported, not asserted to the byte: the numbers are the memory figures
/// the prototype gate asks for.
#[test]
fn per_binding_cost_against_constant_nodes() {
    const N: usize = 5_000;
    let (mut constant_cx, constant_parent) = setup();
    let (constant_allocs, constant_bytes, _) = measure(|| {
        constant_cx
            .mount_view(constant_parent, || bound_rows(N, false))
            .unwrap()
    });
    let (mut bound_cx, bound_parent) = setup();
    let (bound_allocs, bound_bytes, _) = measure(|| {
        bound_cx
            .mount_view(bound_parent, || bound_rows(N, true))
            .unwrap()
    });
    let extra_allocs = (bound_allocs - constant_allocs) as f64 / N as f64;
    let extra_bytes = (bound_bytes - constant_bytes) as f64 / N as f64;
    let base_bytes = constant_bytes as f64 / N as f64;
    eprintln!(
        "reactive-view mount, {N} text nodes: constant {base_bytes:.0} B/node \
         ({:.1} allocs); direct-bound adds {extra_bytes:.0} B and {extra_allocs:.1} allocs per node",
        constant_allocs as f64 / N as f64
    );
    // A direct binding carries no closure: a node pays for its effect slot,
    // its boxed binding set and one binding record, never per-read garbage.
    assert!(
        extra_allocs < 4.5,
        "{extra_allocs} extra allocations per bound node"
    );
}

fn one_binding() -> impl IntoView {
    let label: Signal<String> = signal("a".into());
    let busy = signal(false);
    let _ = busy;
    button(label)
}

fn two_bindings() -> impl IntoView {
    let label: Signal<String> = signal("a".into());
    let busy = signal(false);
    button(label).disabled(busy)
}

/// Allocations of one mount + unmount cycle, after the thread's pools and
/// arenas have grown once.
fn steady_mount<V: IntoView>(view: fn() -> V) -> u64 {
    let (mut cx, parent) = setup();
    for _ in 0..2 {
        cx.mount_view(parent, view)
            .unwrap()
            .unmount(&mut cx)
            .unwrap();
    }
    let (allocations, _, ()) = measure(|| {
        cx.mount_view(parent, view)
            .unwrap()
            .unmount(&mut cx)
            .unwrap();
    });
    allocations
}

#[test]
fn a_second_direct_binding_on_a_node_costs_one_subscription() {
    // Both views create the same two signals; the second binds the other.
    let one = steady_mount(one_binding);
    let two = steady_mount(two_bindings);
    eprintln!("mount+unmount: one binding {one} allocations, two bindings {two}");
    // The binding record lands in the vector the first binding allocated;
    // only the signal's subscriber list is new.
    assert_eq!(two - one, 1);
}

#[test]
fn a_rerun_that_reproduces_the_node_copies_nothing() {
    let (mut cx, parent) = setup();
    let sources = Cell::new(None);
    cx.mount_view(parent, || {
        let draft = signal(String::from("x"));
        let label = signal(String::from("保存"));
        sources.set(Some((draft, label)));
        button(label).disabled(move || draft.with(|d| d.is_empty()))
    })
    .unwrap();
    let (draft, label) = sources.get().unwrap();
    // Warm the queue once.
    draft.update(|d| d.push('y'));
    cx.flush_reactive().unwrap();
    let same = String::from("保存");
    let (allocations, _, ()) = measure(|| {
        draft.update(|d| d.push('y'));
        label.set(same);
        cx.flush_reactive().unwrap();
    });
    assert_eq!(allocations, 0, "no copy of the button, no projection");
}
