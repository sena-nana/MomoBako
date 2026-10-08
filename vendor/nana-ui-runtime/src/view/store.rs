//! Nested state tracked field by field (Solid `createStore`, Vue `reactive`).
//!
//! A [`Signal`](super::Signal) of a struct notifies everything that read any
//! part of it. A [`Store`] keeps the same value in one place but tracks every
//! path into it on its own: `#[derive(Store)]` gives each field an accessor,
//! and a list is addressed by key, so writing one row's `done` updates the
//! bindings that read that `done` and nothing else.
//!
//! ```ignore
//! #[derive(Clone, Store)]
//! struct Todo { id: u64, title: String, done: bool }
//! #[derive(Store)]
//! struct App { todos: Vec<Todo>, filter: String }
//!
//! let app = store(App { todos, filter: String::new() });
//! let todos = app.todos().keyed(|todo| todo.id);
//! todos.each(|todo| checkbox(todo.title()).checked(todo.done()));
//! todos.at(&7).done().set(true);   // one checkbox updates
//! app.todos().push(todo);           // the list adds a row; no row re-reads
//! ```
//!
//! Every path has two triggers, created only once something reads through
//! them, so an unread path costs nothing. Reading a value tracks its *deep*
//! trigger; iterating a list's keys tracks its *shallow* one. Writing a path
//! fires both on the path and every path below it that exists, and the
//! deep trigger of every path above it; a list's own `push`, `insert`,
//! `retain`, `swap` and `sort_by_key` fire only the list's triggers and the
//! deep ones above, because they leave every item's contents as they were.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{BuildHasher, BuildHasherDefault, Hash};
use std::marker::PhantomData;
use std::panic::Location;
use std::rc::Rc;

use hashbrown::HashMap;

use super::node::IntoView;
use super::prop::{FieldWrite, IntoProp, PropSource};
use super::reactive::{self, Readable, ScopeKey, Signal, SignalKey};
use super::structural::{Each, each};
use super::{EachVirtual, NodeBindings, each_virtual};

const ROOT: u64 = 0;

/// The path of `segment` under `parent`.
fn child_path(parent: u64, segment: u64) -> u64 {
    let mixed = (parent ^ segment.wrapping_mul(0x9e37_79b9_7f4a_7c15)).rotate_left(29);
    mixed.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ (mixed >> 31)
}

fn key_hash<K: Hash>(key: &K) -> u64 {
    BuildHasherDefault::<DefaultHasher>::default().hash_one(key)
}

/// Identity of a store, for the path handles into it.
#[doc(hidden)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StoreKey(SignalKey);

/// The triggers of the paths read so far.
#[doc(hidden)]
pub struct Paths {
    scope: Option<ScopeKey>,
    nodes: HashMap<u64, PathNode>,
    /// Triggers of rows no longer in their list, released once no borrow
    /// of the store is held.
    released: Vec<SignalKey>,
}

#[derive(Default)]
struct PathNode {
    children: Vec<u64>,
    /// A list item's key hash.
    key: Option<u64>,
    shallow: Option<SignalKey>,
    deep: Option<SignalKey>,
    /// A list's key hash → position, checked on each use.
    index: Option<HashMap<u64, usize>>,
}

impl Paths {
    fn link(&mut self, path: u64, parent: u64, key: Option<u64>) {
        if self.nodes.contains_key(&path) {
            return;
        }
        self.nodes.insert(
            path,
            PathNode {
                key,
                ..PathNode::default()
            },
        );
        // The root is linked under itself: it has no parent.
        if path != parent
            && let Some(parent) = self.nodes.get_mut(&parent)
        {
            parent.children.push(path);
        }
    }

    fn trigger(&mut self, path: u64, deep: bool, created: &'static Location<'static>) -> SignalKey {
        let scope = self.scope;
        let node = self
            .nodes
            .get_mut(&path)
            .expect("a read path is linked first");
        let slot = if deep {
            &mut node.deep
        } else {
            &mut node.shallow
        };
        *slot.get_or_insert_with(|| reactive::new_trigger(scope, created))
    }

    /// The triggers a write of `path` fires. `below`: the write may have
    /// changed paths under it too.
    fn fired(&self, path: u64, ancestors: &[u64], below: bool, out: &mut Vec<SignalKey>) {
        for ancestor in ancestors {
            if let Some(deep) = self.nodes.get(ancestor).and_then(|node| node.deep) {
                out.push(deep);
            }
        }
        let Some(node) = self.nodes.get(&path) else {
            return;
        };
        out.extend(node.shallow.into_iter().chain(node.deep));
        if !below {
            return;
        }
        let mut stack = node.children.clone();
        while let Some(child) = stack.pop() {
            if let Some(node) = self.nodes.get(&child) {
                out.extend(node.shallow.into_iter().chain(node.deep));
                stack.extend_from_slice(&node.children);
            }
        }
    }

    /// The position of the item keyed `key` in the list at `list`, rebuilding
    /// the list's index when it is stale; rows no longer present lose their
    /// paths then.
    fn position<T, K: Hash>(
        &mut self,
        list: u64,
        items: &[T],
        key: u64,
        key_of: fn(&T) -> K,
    ) -> Option<usize> {
        let node = self.nodes.get(&list)?;
        if let Some(at) = node
            .index
            .as_ref()
            .and_then(|index| index.get(&key).copied())
            && items
                .get(at)
                .is_some_and(|item| key_hash(&key_of(item)) == key)
        {
            return Some(at);
        }
        let mut index = HashMap::with_capacity(items.len());
        for (at, item) in items.iter().enumerate() {
            index.entry(key_hash(&key_of(item))).or_insert(at);
        }
        let found = index.get(&key).copied();
        let gone: Vec<u64> = node
            .children
            .iter()
            .copied()
            .filter(|child| {
                self.nodes
                    .get(child)
                    .and_then(|child| child.key)
                    .is_some_and(|key| !index.contains_key(&key))
            })
            .collect();
        let node = self.nodes.get_mut(&list).expect("checked above");
        node.index = Some(index);
        if !gone.is_empty() {
            node.children.retain(|child| !gone.contains(child));
            for child in gone {
                self.forget(child);
            }
        }
        found
    }

    fn forget(&mut self, path: u64) {
        let Some(node) = self.nodes.remove(&path) else {
            return;
        };
        self.released
            .extend(node.shallow.into_iter().chain(node.deep));
        for child in node.children {
            self.forget(child);
        }
    }
}

struct StoreCell<T> {
    value: RefCell<T>,
    paths: RefCell<Paths>,
    history: RefCell<Option<History<T>>>,
}

/// Snapshots of a store's value for undo and redo.
struct History<T> {
    past: VecDeque<(T, &'static Location<'static>)>,
    future: Vec<(T, &'static Location<'static>)>,
    limit: usize,
    clone: fn(&T) -> T,
    /// The flush epoch the newest step was taken in: writes until the next
    /// flush (one event handler, one task poll) join that step.
    epoch: Option<u64>,
    /// Bumped whenever the steps change, for `can_undo` / `can_redo`.
    version: Signal<u64>,
}

impl<T> History<T> {
    /// A copy of `value` when the next write starts a new step.
    fn snapshot(&self, value: &T) -> Option<T> {
        (self.epoch != Some(reactive::epoch())).then(|| (self.clone)(value))
    }

    fn push(&mut self, before: T, at: &'static Location<'static>) {
        self.epoch = Some(reactive::epoch());
        self.past.push_back((before, at));
        if self.past.len() > self.limit {
            self.past.pop_front();
        }
        self.future.clear();
        self.version.update(|version| *version += 1);
    }
}

/// A path into a store: the store itself, a field of a path, or the item of
/// a list with a given key. Handles are `Copy` ids like signals; the
/// accessors `#[derive(Store)]` generates and [`StoreList`] build them.
pub trait StorePath: Copy + Send + 'static {
    /// The stored value this path starts from.
    type Root: 'static;
    /// The value it leads to.
    type Value: 'static;

    #[doc(hidden)]
    fn store(&self) -> StoreKey;
    #[doc(hidden)]
    fn path(&self) -> u64;
    /// Link this path and those above it into `paths`.
    #[doc(hidden)]
    fn link(&self, paths: &mut Paths);
    /// Every path above this one, the store's first.
    #[doc(hidden)]
    fn ancestors(&self, out: &mut Vec<u64>);
    #[doc(hidden)]
    fn locate<'a>(&self, root: &'a Self::Root, paths: &mut Paths) -> Option<&'a Self::Value>;
    #[doc(hidden)]
    fn locate_mut<'a>(
        &self,
        root: &'a mut Self::Root,
        paths: &mut Paths,
    ) -> Option<&'a mut Self::Value>;

    /// Read and track. Panics if the path leads to a list item that is gone.
    #[track_caller]
    fn with<R>(&self, f: impl FnOnce(&Self::Value) -> R) -> R {
        self.try_with(f).unwrap_or_else(|| {
            panic!(
                "store path read at {} leads to a removed item",
                Location::caller()
            )
        })
    }

    /// Read and track; `None` once a list item on the path is gone.
    #[track_caller]
    fn try_with<R>(&self, f: impl FnOnce(&Self::Value) -> R) -> Option<R> {
        read(self, Location::caller(), Some(true), f)
    }

    #[track_caller]
    fn get(&self) -> Self::Value
    where
        Self::Value: Clone,
    {
        self.with(Self::Value::clone)
    }

    #[track_caller]
    fn get_untracked(&self) -> Self::Value
    where
        Self::Value: Clone,
    {
        read(self, Location::caller(), None, Self::Value::clone).unwrap_or_else(|| {
            panic!(
                "store path read at {} leads to a removed item",
                Location::caller()
            )
        })
    }

    /// Replace the value and notify its readers, those of the paths below
    /// it and the whole-value readers above it.
    #[track_caller]
    fn set(&self, value: Self::Value) {
        write(self, Location::caller(), true, true, |slot| *slot = value);
    }

    /// Mutate in place and notify as [`Self::set`]. Does nothing once a list
    /// item on the path is gone. `f` must not read this store.
    #[track_caller]
    fn update(&self, f: impl FnOnce(&mut Self::Value)) {
        write(self, Location::caller(), true, true, f);
    }
}

fn cell<T: 'static>(store: StoreKey, at: &'static Location<'static>) -> Rc<dyn Any> {
    let cell = reactive::read_cell(store.0, at, false);
    debug_assert!(
        cell.is::<StoreCell<T>>(),
        "a store path reads its own store"
    );
    cell
}

/// Read through `path`; `track`: `Some(deep)` tracks that trigger.
fn read<P: StorePath, R>(
    path: &P,
    at: &'static Location<'static>,
    track: Option<bool>,
    f: impl FnOnce(&P::Value) -> R,
) -> Option<R> {
    let cell = cell::<P::Root>(path.store(), at);
    let cell = cell
        .downcast_ref::<StoreCell<P::Root>>()
        .expect("a store path reads its own store");
    if let Some(deep) = track
        && reactive::tracking()
    {
        let trigger = {
            let mut paths = cell.paths.borrow_mut();
            path.link(&mut paths);
            paths.trigger(path.path(), deep, at)
        };
        reactive::track_key(trigger);
    }
    let value = cell.value.borrow();
    let (found, released) = {
        let mut paths = cell.paths.borrow_mut();
        let found = path.locate(&value, &mut paths);
        (found, std::mem::take(&mut paths.released))
    };
    let result = found.map(f);
    drop(value);
    let scope = cell.paths.borrow().scope;
    reactive::release_signals(scope, &released);
    result
}

/// Write through `path` and fire its triggers; `below`: see [`Paths::fired`].
/// `record`: a store with history takes a snapshot first (undo and redo
/// write without one).
fn write<P: StorePath>(
    path: &P,
    at: &'static Location<'static>,
    below: bool,
    record: bool,
    f: impl FnOnce(&mut P::Value),
) {
    let cell = cell::<P::Root>(path.store(), at);
    let cell = cell
        .downcast_ref::<StoreCell<P::Root>>()
        .expect("a store path writes its own store");
    let snapshot = match record {
        true => cell
            .history
            .borrow()
            .as_ref()
            .and_then(|history| history.snapshot(&cell.value.borrow())),
        false => None,
    };
    let written = {
        let mut value = cell.value.borrow_mut();
        let mut paths = cell.paths.borrow_mut();
        match path.locate_mut(&mut value, &mut paths) {
            Some(slot) => {
                drop(paths);
                f(slot);
                true
            }
            None => false,
        }
    };
    let mut fired = Vec::new();
    let (released, scope) = {
        let mut paths = cell.paths.borrow_mut();
        if written {
            let mut ancestors = Vec::new();
            path.ancestors(&mut ancestors);
            paths.fired(path.path(), &ancestors, below, &mut fired);
        }
        (std::mem::take(&mut paths.released), paths.scope)
    };
    reactive::release_signals(scope, &released);
    reactive::notify_keys(&fired, at);
    if written
        && let Some(before) = snapshot
        && let Some(history) = cell.history.borrow_mut().as_mut()
    {
        history.push(before, at);
    }
}

/// Nested state whose fields are tracked one by one. See the module docs.
pub struct Store<T: 'static> {
    key: SignalKey,
    _type: PhantomData<fn() -> T>,
}

impl<T> Clone for Store<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Store<T> {}

/// Create a store owned by the current scope.
#[track_caller]
pub fn store<T: 'static>(value: T) -> Store<T> {
    let cell: Rc<dyn Any> = Rc::new(StoreCell {
        value: RefCell::new(value),
        paths: RefCell::new(Paths {
            scope: reactive::current_scope(),
            nodes: HashMap::new(),
            released: Vec::new(),
        }),
        history: RefCell::new(None),
    });
    Store {
        key: reactive::new_cell(cell, Location::caller()),
        _type: PhantomData,
    }
}

/// [`store`] that remembers up to `limit` earlier values for
/// [`Store::undo`] and [`Store::redo`] (time travel). Every write that
/// starts a step copies the whole value first: the writes one event handler
/// makes are one step.
#[track_caller]
pub fn store_with_history<T: Clone + 'static>(value: T, limit: usize) -> Store<T> {
    let store = store(value);
    let version = super::reactive::signal(0u64);
    let cell = cell::<T>(StoreKey(store.key), Location::caller());
    let cell = cell
        .downcast_ref::<StoreCell<T>>()
        .expect("a store reads its own cell");
    *cell.history.borrow_mut() = Some(History {
        past: VecDeque::new(),
        future: Vec::new(),
        limit: limit.max(1),
        clone: T::clone,
        epoch: None,
        version,
    });
    store
}

impl<T: 'static> Store<T> {
    fn history<R>(&self, f: impl FnOnce(&StoreCell<T>, Option<&mut History<T>>) -> R) -> R {
        let cell = cell::<T>(StoreKey(self.key), Location::caller());
        let cell = cell
            .downcast_ref::<StoreCell<T>>()
            .expect("a store reads its own cell");
        let mut history = cell.history.borrow_mut();
        f(cell, history.as_mut())
    }

    /// Go back one step. `false` when there is none (or no history).
    #[track_caller]
    pub fn undo(&self) -> bool {
        self.step(true, Location::caller())
    }

    /// Go forward one step undone before. `false` when there is none.
    #[track_caller]
    pub fn redo(&self) -> bool {
        self.step(false, Location::caller())
    }

    /// Undo (`steps < 0`) or redo as many steps as there are, up to `steps`.
    #[track_caller]
    pub fn travel(&self, steps: isize) -> usize {
        let at = Location::caller();
        (0..steps.unsigned_abs())
            .take_while(|_| self.step(steps < 0, at))
            .count()
    }

    fn step(&self, back: bool, at: &'static Location<'static>) -> bool {
        let target = self.history(|cell, history| {
            let history = history?;
            let (value, written_at) = match back {
                true => history.past.pop_back()?,
                false => history.future.pop()?,
            };
            let current = (history.clone)(&cell.value.borrow());
            match back {
                true => history.future.push((current, written_at)),
                false => history.past.push_back((current, written_at)),
            }
            // The next write starts a step of its own.
            history.epoch = None;
            history.version.update(|version| *version += 1);
            Some(value)
        });
        match target {
            Some(value) => {
                write(self, at, true, false, |slot| *slot = value);
                true
            }
            None => false,
        }
    }

    /// Whether [`Self::undo`] has a step. Tracked.
    pub fn can_undo(&self) -> bool {
        self.history(|_, history| {
            history.is_some_and(|history| {
                history.version.get();
                !history.past.is_empty()
            })
        })
    }

    /// Whether [`Self::redo`] has a step. Tracked.
    pub fn can_redo(&self) -> bool {
        self.history(|_, history| {
            history.is_some_and(|history| {
                history.version.get();
                !history.future.is_empty()
            })
        })
    }

    /// Where each step that can be undone was written, oldest first.
    /// Tracked.
    pub fn steps(&self) -> Vec<&'static Location<'static>> {
        self.history(|_, history| {
            history.map_or_else(Vec::new, |history| {
                history.version.get();
                history.past.iter().map(|(_, at)| *at).collect()
            })
        })
    }
}

impl<T: 'static> StorePath for Store<T> {
    type Root = T;
    type Value = T;

    fn store(&self) -> StoreKey {
        StoreKey(self.key)
    }

    fn path(&self) -> u64 {
        ROOT
    }

    fn link(&self, paths: &mut Paths) {
        paths.link(ROOT, ROOT, None);
    }

    fn ancestors(&self, _: &mut Vec<u64>) {}

    fn locate<'a>(&self, root: &'a T, _: &mut Paths) -> Option<&'a T> {
        Some(root)
    }

    fn locate_mut<'a>(&self, root: &'a mut T, _: &mut Paths) -> Option<&'a mut T> {
        Some(root)
    }
}

/// A field of the value at `P`, from a `#[derive(Store)]` accessor.
pub struct Subfield<P: StorePath, U: 'static> {
    parent: P,
    segment: u64,
    get: fn(&P::Value) -> &U,
    get_mut: fn(&mut P::Value) -> &mut U,
}

impl<P: StorePath, U> Clone for Subfield<P, U> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: StorePath, U> Copy for Subfield<P, U> {}

impl<P: StorePath, U: 'static> Subfield<P, U> {
    /// For `#[derive(Store)]`: field number `segment` of the value at
    /// `parent`, reached through `get` and `get_mut`.
    #[doc(hidden)]
    pub fn __new(
        parent: P,
        segment: u32,
        get: fn(&P::Value) -> &U,
        get_mut: fn(&mut P::Value) -> &mut U,
    ) -> Self {
        Self {
            parent,
            segment: u64::from(segment),
            get,
            get_mut,
        }
    }
}

impl<P: StorePath, U: 'static> StorePath for Subfield<P, U> {
    type Root = P::Root;
    type Value = U;

    fn store(&self) -> StoreKey {
        self.parent.store()
    }

    fn path(&self) -> u64 {
        child_path(self.parent.path(), self.segment)
    }

    fn link(&self, paths: &mut Paths) {
        let path = self.path();
        if !paths.nodes.contains_key(&path) {
            self.parent.link(paths);
            paths.link(path, self.parent.path(), None);
        }
    }

    fn ancestors(&self, out: &mut Vec<u64>) {
        self.parent.ancestors(out);
        out.push(self.parent.path());
    }

    fn locate<'a>(&self, root: &'a P::Root, paths: &mut Paths) -> Option<&'a U> {
        self.parent.locate(root, paths).map(self.get)
    }

    fn locate_mut<'a>(&self, root: &'a mut P::Root, paths: &mut Paths) -> Option<&'a mut U> {
        self.parent.locate_mut(root, paths).map(self.get_mut)
    }
}

/// The item of the list at `P` whose key is the one this handle was made
/// with, wherever it moves. See [`KeyedList::at`].
pub struct Item<P: StorePath, K: 'static, T: 'static> {
    list: P,
    key: u64,
    key_of: fn(&T) -> K,
}

impl<P: StorePath, K, T> Clone for Item<P, K, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: StorePath, K, T> Copy for Item<P, K, T> {}

impl<P: StorePath, K, T> Item<P, K, T> {
    /// The hash of this item's key: its identity in a keyed list.
    pub fn id(&self) -> u64 {
        self.key
    }
}

impl<P, K, T> StorePath for Item<P, K, T>
where
    P: StorePath<Value = Vec<T>>,
    K: Hash + 'static,
    T: 'static,
{
    type Root = P::Root;
    type Value = T;

    fn store(&self) -> StoreKey {
        self.list.store()
    }

    fn path(&self) -> u64 {
        child_path(self.list.path(), self.key)
    }

    fn link(&self, paths: &mut Paths) {
        let path = self.path();
        if !paths.nodes.contains_key(&path) {
            self.list.link(paths);
            paths.link(path, self.list.path(), Some(self.key));
        }
    }

    fn ancestors(&self, out: &mut Vec<u64>) {
        self.list.ancestors(out);
        out.push(self.list.path());
    }

    fn locate<'a>(&self, root: &'a P::Root, paths: &mut Paths) -> Option<&'a T> {
        let items = self.list.locate(root, paths)?;
        self.list.link(paths);
        let at = paths.position(self.list.path(), items, self.key, self.key_of)?;
        items.get(at)
    }

    fn locate_mut<'a>(&self, root: &'a mut P::Root, paths: &mut Paths) -> Option<&'a mut T> {
        let items = self.list.locate_mut(root, paths)?;
        self.list.link(paths);
        let at = paths.position(self.list.path(), items, self.key, self.key_of)?;
        items.get_mut(at)
    }
}

/// A list path with the function that keys its items. See
/// [`StoreList::keyed`].
pub struct KeyedList<P: StorePath, K: 'static, T: 'static> {
    list: P,
    key_of: fn(&T) -> K,
}

impl<P: StorePath, K, T> Clone for KeyedList<P, K, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: StorePath, K, T> Copy for KeyedList<P, K, T> {}

impl<P, K, T> KeyedList<P, K, T>
where
    P: StorePath<Value = Vec<T>>,
    K: Hash + 'static,
    T: 'static,
{
    /// The item keyed `key`, for as long as the list holds it.
    pub fn at(&self, key: &K) -> Item<P, K, T> {
        Item {
            list: self.list,
            key: key_hash(key),
            key_of: self.key_of,
        }
    }

    /// Every item's handle, in order, tracking only the list's structure:
    /// writes inside an item do not re-run the reader.
    #[track_caller]
    pub fn items(&self) -> Vec<Item<P, K, T>> {
        let at = Location::caller();
        let (list, key_of) = (self.list, self.key_of);
        read(&list, at, Some(false), |items| {
            let mut seen = hashbrown::HashSet::with_capacity(items.len());
            items
                .iter()
                .map(|item| key_hash(&key_of(item)))
                .filter(|key| seen.insert(*key))
                .map(|key| Item { list, key, key_of })
                .collect()
        })
        .unwrap_or_default()
    }

    /// A keyed list view: a row per item, built once and kept while the
    /// item is in the list; writes inside an item reach only the row's
    /// bindings that read them.
    #[track_caller]
    #[allow(clippy::type_complexity)]
    pub fn each<RF, V>(
        self,
        row: RF,
    ) -> Each<Item<P, K, T>, u64, Self, fn(&Item<P, K, T>) -> u64, RF>
    where
        K: Send,
        RF: Fn(Item<P, K, T>) -> V + Send + 'static,
        V: IntoView,
    {
        each(self, Item::id as fn(&Item<P, K, T>) -> u64, row)
    }

    /// [`Self::each`] over rows `row_height` tall, built only while in view.
    #[track_caller]
    #[allow(clippy::type_complexity)]
    pub fn each_virtual<RF, V>(
        self,
        row_height: f32,
        row: RF,
    ) -> EachVirtual<Item<P, K, T>, u64, Self, fn(&Item<P, K, T>) -> u64, RF>
    where
        K: Send,
        RF: Fn(Item<P, K, T>) -> V + Send + 'static,
        V: IntoView,
    {
        each_virtual(self, Item::id as fn(&Item<P, K, T>) -> u64, row_height, row)
    }
}

impl<P, K, T> Readable<Vec<Item<P, K, T>>> for KeyedList<P, K, T>
where
    P: StorePath<Value = Vec<T>>,
    K: Hash + Send + 'static,
    T: 'static,
{
    #[track_caller]
    fn with_value<R>(&self, f: impl FnOnce(&Vec<Item<P, K, T>>) -> R) -> R {
        f(&self.items())
    }
}

/// Structural operations on a list path. They leave every item's contents
/// as they were, so only the list's own readers and the whole-value readers
/// above it run again.
pub trait StoreList<T: 'static>: StorePath<Value = Vec<T>> {
    /// Address items by `key` (a plain function or a closure that captures
    /// nothing): `todos.keyed(|todo| todo.id)`. Keys hash to 64 bits; two
    /// keys of one list must not collide.
    fn keyed<K: Hash + 'static>(self, key: fn(&T) -> K) -> KeyedList<Self, K, T> {
        KeyedList {
            list: self,
            key_of: key,
        }
    }

    /// The number of items, tracking only the list's structure.
    #[track_caller]
    fn len(&self) -> usize {
        read(self, Location::caller(), Some(false), Vec::len).unwrap_or(0)
    }

    #[track_caller]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[track_caller]
    fn push(&self, item: T) {
        write(self, Location::caller(), false, true, |items| {
            items.push(item)
        });
    }

    #[track_caller]
    fn insert(&self, at: usize, item: T) {
        write(self, Location::caller(), false, true, |items| {
            items.insert(at.min(items.len()), item);
        });
    }

    #[track_caller]
    fn retain(&self, keep: impl FnMut(&T) -> bool) {
        write(self, Location::caller(), false, true, |items| {
            items.retain(keep)
        });
    }

    #[track_caller]
    fn swap(&self, a: usize, b: usize) {
        write(self, Location::caller(), false, true, |items| {
            if a < items.len() && b < items.len() {
                items.swap(a, b);
            }
        });
    }

    #[track_caller]
    fn sort_by_key<K: Ord>(&self, key: impl FnMut(&T) -> K) {
        write(self, Location::caller(), false, true, |items| {
            items.sort_by_key(key)
        });
    }
}

impl<T: 'static, P: StorePath<Value = Vec<T>>> StoreList<T> for P {}

macro_rules! readable_paths {
    ($([$($generics:tt)*] $path:ty => $value:ty;)*) => {$(
        impl<$($generics)*> Readable<$value> for $path {
            #[track_caller]
            fn with_value<R>(&self, f: impl FnOnce(&$value) -> R) -> R {
                self.with(f)
            }
        }

        impl<$($generics)*> IntoProp<$value> for $path
        where
            $value: Clone + Send,
        {
            fn bind_field<C: 'static, W: FieldWrite<C, $value> + 'static>(
                self,
                target: &mut C,
                bindings: &mut NodeBindings<C>,
                site: &'static Location<'static>,
            ) {
                (move || self.get()).bind_field::<C, W>(target, bindings, site);
            }

            fn into_source(self) -> PropSource<$value> {
                PropSource::Dyn(Box::new(move || self.get()))
            }
        }
    )*};
}

readable_paths! {
    [T: 'static] Store<T> => T;
    [P: StorePath, U: 'static] Subfield<P, U> => U;
    [P: StorePath<Value = Vec<T>>, K: Hash + 'static, T: 'static] Item<P, K, T> => T;
}
