//! Futures on the UI thread.
//!
//! Signals live on the thread that created them, so asynchronous work that
//! writes them runs there too: [`spawn_local`] polls a future on the UI
//! thread whenever its waker fires, from any thread. Blocking work goes to
//! another thread with [`spawn_blocking`], which hands back a future of its
//! result. A task spawned while a view is built belongs to that view's
//! scope and is dropped with it.
//!
//! The host installs a wake hook ([`set_task_wake`]) so a waker fired on a
//! worker thread brings the UI thread round to [`poll_tasks`]; frames poll
//! too ([`crate::AppContext::take_system_work`]). Without a host, call
//! [`poll_tasks`] yourself.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use super::reactive;

type LocalFuture = Pin<Box<dyn Future<Output = ()>>>;
type HostWake = Arc<dyn Fn() + Send + Sync>;

/// Polls one batch of woken tasks may cascade into before the rest waits
/// for the next [`poll_tasks`].
const MAX_ROUNDS: usize = 16;

/// What wakers reach from any thread.
#[derive(Default)]
struct Shared {
    woken: Mutex<Vec<(u32, u32)>>,
    host: Mutex<Option<HostWake>>,
}

impl Shared {
    fn wake(&self, index: u32, generation: u32) {
        let first = {
            let mut woken = self.woken.lock().unwrap_or_else(PoisonError::into_inner);
            let first = woken.is_empty();
            woken.push((index, generation));
            first
        };
        if first {
            let host = self
                .host
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(host) = host {
                host();
            }
        }
    }
}

struct TaskWaker {
    index: u32,
    generation: u32,
    shared: Arc<Shared>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.shared.wake(self.index, self.generation);
    }
}

struct Slot {
    generation: u32,
    /// `None` while the task is being polled or once it is gone.
    future: Option<LocalFuture>,
    waker: Option<Waker>,
    live: bool,
}

#[derive(Default)]
struct Executor {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
    shared: Arc<Shared>,
}

thread_local! {
    static EXECUTOR: RefCell<Executor> = RefCell::new(Executor::default());
}

fn with_executor<R>(f: impl FnOnce(&mut Executor) -> R) -> R {
    EXECUTOR.with(|executor| f(&mut executor.borrow_mut()))
}

/// A task spawned with [`spawn_local`]. Dropping the handle keeps the task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    index: u32,
    generation: u32,
}

impl Task {
    /// Drop the future now. Nothing it would have done afterwards happens.
    pub fn abort(self) {
        with_executor(|executor| {
            let Some(slot) = executor.slots.get_mut(self.index as usize) else {
                return;
            };
            if slot.generation != self.generation || !slot.live {
                return;
            }
            slot.generation = slot.generation.wrapping_add(1);
            slot.live = false;
            // A task polling itself away drops its future when the poll
            // returns.
            drop(slot.future.take());
            slot.waker = None;
            executor.free.push(self.index);
            executor.live -= 1;
        });
    }

    /// Whether the task has neither finished nor been aborted.
    pub fn is_running(self) -> bool {
        with_executor(|executor| {
            executor
                .slots
                .get(self.index as usize)
                .is_some_and(|slot| slot.generation == self.generation && slot.live)
        })
    }
}

/// Run `future` on this thread. It is polled by [`poll_tasks`] whenever its
/// waker fires; spawned while a view is built, it is dropped with the
/// view's scope.
pub fn spawn_local(future: impl Future<Output = ()> + 'static) -> Task {
    let task = with_executor(|executor| {
        let future: LocalFuture = Box::pin(future);
        executor.live += 1;
        let (index, generation) = match executor.free.pop() {
            Some(index) => {
                let slot = &mut executor.slots[index as usize];
                slot.future = Some(future);
                slot.live = true;
                (index, slot.generation)
            }
            None => {
                executor.slots.push(Slot {
                    generation: 0,
                    future: Some(future),
                    waker: None,
                    live: true,
                });
                ((executor.slots.len() - 1) as u32, 0)
            }
        };
        Task { index, generation }
    });
    if reactive::current_scope().is_some() {
        reactive::on_cleanup(move || task.abort());
    }
    let shared = with_executor(|executor| Arc::clone(&executor.shared));
    shared.wake(task.index, task.generation);
    task
}

/// Poll every task woken since the last call, and those they wake in turn
/// (a bounded number of rounds). Returns how many polls ran. Signal writes
/// they make are applied by the next flush.
pub fn poll_tasks() -> usize {
    let shared = with_executor(|executor| Arc::clone(&executor.shared));
    let mut polled = 0;
    for _ in 0..MAX_ROUNDS {
        let batch =
            std::mem::take(&mut *shared.woken.lock().unwrap_or_else(PoisonError::into_inner));
        if batch.is_empty() {
            return polled;
        }
        for (index, generation) in batch {
            let taken = with_executor(|executor| {
                let shared = Arc::clone(&executor.shared);
                let slot = executor.slots.get_mut(index as usize)?;
                if slot.generation != generation {
                    return None;
                }
                let future = slot.future.take()?;
                let waker = slot
                    .waker
                    .get_or_insert_with(|| {
                        Waker::from(Arc::new(TaskWaker {
                            index,
                            generation,
                            shared,
                        }))
                    })
                    .clone();
                Some((future, waker))
            });
            let Some((mut future, waker)) = taken else {
                continue;
            };
            polled += 1;
            let ready = reactive::untrack(|| {
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_ready()
            });
            let finished = with_executor(|executor| {
                let slot = &mut executor.slots[index as usize];
                if slot.generation != generation {
                    // Aborted while it ran.
                    return Some(future);
                }
                if ready {
                    slot.generation = slot.generation.wrapping_add(1);
                    slot.live = false;
                    slot.waker = None;
                    executor.free.push(index);
                    executor.live -= 1;
                    Some(future)
                } else {
                    slot.future = Some(future);
                    None
                }
            });
            drop(finished);
        }
    }
    // Still waking each other: leave the rest for the next call.
    let pending = !shared
        .woken
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty();
    if pending {
        let host = shared
            .host
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(host) = host {
            host();
        }
    }
    polled
}

/// Whether a task of this thread was woken and waits for [`poll_tasks`].
pub fn has_woken_tasks() -> bool {
    let shared = with_executor(|executor| Arc::clone(&executor.shared));
    !shared
        .woken
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty()
}

/// Tasks of this thread that have not finished.
pub fn task_count() -> usize {
    with_executor(|executor| executor.live)
}

/// Called, from whichever thread a waker fires on, when this thread has
/// tasks to poll. The host passes its event-loop wake; it then polls on the
/// UI thread.
pub fn set_task_wake(wake: impl Fn() + Send + Sync + 'static) {
    let shared = with_executor(|executor| Arc::clone(&executor.shared));
    *shared.host.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(wake));
}

/// Run `work` on a new thread; the future resolves to its result.
pub fn spawn_blocking<R: Send + 'static>(
    work: impl FnOnce() -> R + Send + 'static,
) -> impl Future<Output = R> {
    let state = Arc::new(Mutex::new(Blocking::<R> {
        result: None,
        waker: None,
    }));
    let worker = Arc::clone(&state);
    std::thread::spawn(move || {
        let result = work();
        let waker = {
            let mut state = worker.lock().unwrap_or_else(PoisonError::into_inner);
            state.result = Some(result);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    });
    BlockingFuture { state }
}

struct Blocking<R> {
    result: Option<R>,
    waker: Option<Waker>,
}

struct BlockingFuture<R> {
    state: Arc<Mutex<Blocking<R>>>,
}

impl<R> Future for BlockingFuture<R> {
    type Output = R;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<R> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match state.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
