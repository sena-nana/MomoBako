//! Opt-in benchmark-thread allocation accounting. Native driver allocations,
//! worker threads, GPU storage and allocations outside the measured scope are
//! deliberately excluded. This allocator is installed only by the benchmark.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(super) struct Counts {
    pub calls: u64,
    /// Requested bytes, including the new size of successful reallocations.
    pub requested_bytes: u64,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { calls: 0, requested_bytes: 0 }) };
}

pub(super) struct CountingAllocator;

fn record(pointer: *mut u8, bytes: usize) {
    if !pointer.is_null() && ACTIVE.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|counts| {
            let mut value = counts.get();
            value.calls = value.calls.saturating_add(1);
            value.requested_bytes = value.requested_bytes.saturating_add(bytes as u64);
            counts.set(value);
        });
    }
}

// SAFETY: every operation forwards the caller's pointer/layout unchanged to
// System. Accounting uses allocation-free thread-local cells and never panics.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(pointer, layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        record(pointer, size);
        pointer
    }
}

pub(super) fn measure<T>(enabled: bool, operation: impl FnOnce() -> T) -> (T, Counts) {
    if !enabled {
        return (operation(), Counts::default());
    }
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            ACTIVE.set(false);
        }
    }
    assert!(!ACTIVE.get(), "allocation scopes cannot nest");
    COUNTS.set(Counts::default());
    ACTIVE.set(true);
    let scope = Scope;
    let result = operation();
    drop(scope);
    (result, COUNTS.get())
}

#[derive(Default, serde::Serialize)]
pub(super) struct Report {
    pub samples: usize,
    pub total: Counts,
    pub maximum_per_frame: Counts,
}

impl Report {
    pub fn observe(&mut self, runtime: Counts, paint: Counts) {
        let calls = runtime.calls + paint.calls;
        let bytes = runtime.requested_bytes + paint.requested_bytes;
        self.samples += 1;
        self.total.calls += calls;
        self.total.requested_bytes += bytes;
        self.maximum_per_frame.calls = self.maximum_per_frame.calls.max(calls);
        self.maximum_per_frame.requested_bytes = self.maximum_per_frame.requested_bytes.max(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_scope_counts_successful_requests_and_resets() {
        let allocate = || {
            let layout = Layout::from_size_align(32, 8).unwrap();
            // SAFETY: both allocations use matching valid layouts; the old
            // allocation is released by successful realloc, the new one here.
            unsafe {
                let pointer = CountingAllocator.alloc_zeroed(layout);
                assert!(!pointer.is_null());
                let pointer = CountingAllocator.realloc(pointer, layout, 64);
                assert!(!pointer.is_null());
                CountingAllocator.dealloc(pointer, Layout::from_size_align(64, 8).unwrap());
            }
        };
        let (_, counts) = measure(true, allocate);
        assert_eq!(counts.calls, 2);
        assert_eq!(counts.requested_bytes, 96);
        assert_eq!(measure(false, allocate).1.calls, 0);
        assert_eq!(measure(true, || {}).1.calls, 0);
    }

    #[test]
    fn allocation_scope_is_disabled_after_unwind() {
        let _ = std::panic::catch_unwind(|| measure(true, || panic!("scope unwinding")));
        assert!(!ACTIVE.get());
        assert_eq!(measure(true, || {}).1.calls, 0);
    }
}
