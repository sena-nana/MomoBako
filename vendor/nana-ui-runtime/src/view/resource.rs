//! Asynchronous values (Leptos `Resource`, Vue `<Suspense>` with an async
//! `setup`).
//!
//! ```ignore
//! let user = resource(move || id.get(), |id| spawn_blocking(move || load_user(id)));
//! suspense(
//!     || text("加载中…"),
//!     move || text(move || user.with(|u| u.map_or(String::new(), |u| u.name.clone()))),
//! )
//! ```
//!
//! A resource runs its fetch on the UI thread's executor ([`super::spawn_local`])
//! whenever its source changes, dropping the fetch it replaces. It keeps the
//! last value it resolved to while the next one loads.

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

use super::node::{IntoView, ViewBuilder, widget};
use super::reactive::{
    self, Readable, Signal, provide, signal, untrack, use_context, watch_effect,
};
use super::structural::{build_scoped, when};
use super::task::{Task, spawn_local};
use crate::Stack;

/// A value loaded asynchronously. `Copy`; see [`resource`].
pub struct Resource<T: 'static> {
    value: Signal<Option<T>>,
    loading: Signal<bool>,
    refetch: Signal<u64>,
}

impl<T> Clone for Resource<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Resource<T> {}

impl<T: 'static> Resource<T> {
    /// The latest value, `None` until the first fetch resolves. Tracked.
    #[track_caller]
    pub fn get(&self) -> Option<T>
    where
        T: Clone,
    {
        self.value.get()
    }

    #[track_caller]
    pub fn with<R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        self.value.with(|value| f(value.as_ref()))
    }

    /// Whether a fetch is running. Tracked.
    #[track_caller]
    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    /// Fetch again with the current source.
    pub fn refetch(&self) {
        self.refetch.update(|round| *round += 1);
    }

    /// Replace the value without fetching (an optimistic update); a running
    /// fetch still lands afterwards.
    pub fn set(&self, value: T) {
        self.value.set(Some(value));
    }
}

impl<T: 'static> Readable<Option<T>> for Resource<T> {
    #[track_caller]
    fn with_value<R>(&self, f: impl FnOnce(&Option<T>) -> R) -> R {
        self.value.with(f)
    }
}

/// The resources inside a [`suspense`] that have not resolved yet.
#[derive(Clone, Copy)]
struct SuspenseContext {
    pending: Signal<usize>,
}

/// Load `fetch(source())` now and again whenever `source` changes (it is
/// tracked like an effect). Inside a [`suspense`], the suspense shows its
/// fallback until the first fetch resolves.
#[track_caller]
pub fn resource<S, T, F, Fut>(source: impl Fn() -> S + 'static, fetch: F) -> Resource<T>
where
    S: 'static,
    T: 'static,
    F: Fn(S) -> Fut + 'static,
    Fut: Future<Output = T> + 'static,
{
    let resource = Resource {
        value: signal(None),
        loading: signal(false),
        refetch: signal(0u64),
    };
    // Held by the suspense until the first value lands, once.
    let suspense = use_context::<SuspenseContext>();
    let holding = Rc::new(Cell::new(false));
    if let Some(suspense) = suspense {
        suspense.pending.update(|pending| *pending += 1);
        holding.set(true);
        let holding = Rc::clone(&holding);
        reactive::on_cleanup(move || {
            if holding.replace(false) {
                suspense.pending.update(|pending| *pending -= 1);
            }
        });
    }
    let running: Rc<Cell<Option<Task>>> = Rc::new(Cell::new(None));
    let cancel = Rc::clone(&running);
    reactive::on_cleanup(move || {
        if let Some(task) = cancel.take() {
            task.abort();
        }
    });
    watch_effect(move || {
        resource.refetch.get();
        let input = source();
        untrack(|| {
            if let Some(task) = running.take() {
                task.abort();
            }
            if !resource.loading.get() {
                resource.loading.set(true);
            }
            let future = fetch(input);
            let holding = Rc::clone(&holding);
            running.set(Some(spawn_detached(async move {
                let value = future.await;
                resource.value.set(Some(value));
                resource.loading.set(false);
                if holding.replace(false)
                    && let Some(suspense) = suspense
                {
                    suspense.pending.update(|pending| *pending -= 1);
                }
            })));
        });
    });
    resource
}

/// A task outside any scope: the resource aborts it itself.
fn spawn_detached(future: impl Future<Output = ()> + 'static) -> Task {
    match reactive::current_scope() {
        Some(_) => reactive::without_scope(|| spawn_local(future)),
        None => spawn_local(future),
    }
}

/// `content` with `fallback` in its place until every [`resource`] created
/// while building it has resolved once (Vue `<Suspense>`). The content is
/// built at once, hidden; later refetches keep it shown.
pub fn suspense<F, V, C>(fallback: impl Fn() -> F + Send + 'static, content: C) -> Suspense<C>
where
    F: IntoView,
    V: IntoView,
    C: FnOnce() -> V + 'static,
{
    Suspense {
        fallback: Box::new(move || fallback().into_any()),
        content,
    }
}

/// See [`suspense`].
pub struct Suspense<C> {
    fallback: Box<dyn Fn() -> super::AnyView + Send>,
    content: C,
}

struct Scoped<C> {
    pending: Signal<usize>,
    content: C,
}

impl<V: IntoView, C: FnOnce() -> V + 'static> IntoView for Scoped<C> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let pending = self.pending;
        let content = self.content;
        build_scoped(vb, reactive::current_scope(), move || {
            provide(SuspenseContext { pending });
            content().into_any()
        });
    }
}

impl<V: IntoView, C: FnOnce() -> V + 'static> IntoView for Suspense<C> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let pending = signal(0usize);
        let fallback = self.fallback;
        let ready = move || pending.get() == 0;
        let view = super::column().children((
            widget(Stack::column(0.0)).visible(ready).children(Scoped {
                pending,
                content: self.content,
            }),
            when(move || pending.get() > 0, fallback),
        ));
        view.build(vb);
    }
}
