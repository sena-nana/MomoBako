//! A keyed list whose rows exist only while they are in view.
//!
//! [`each`](super::each) builds every row, which is right for tens or
//! hundreds of them and wrong for tens of thousands: every row is nodes,
//! bindings and a scope, about 2–3 KB live. [`each_virtual`] lays rows out at
//! a fixed height inside a `ScrollView` and builds only those the viewport
//! (plus overscan) covers, through the retained virtual list the Runtime
//! already has: a row scrolled away is despawned with its scope, a row that
//! holds focus or an IME composition is kept. With [`EachVirtual::measured`]
//! rows size to their content, the given height only an estimate.
//! [`EachVirtual::within`] lets the rows scroll with an ancestor
//! `ScrollView` (a page's) instead of one of their own, and
//! [`EachVirtual::grid`] flows items into as many columns as fit.

use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Range;
use std::panic::Location;
use std::sync::{Arc, Mutex, PoisonError};

use hashbrown::{HashMap, HashSet};
use nana_ui_core::VirtualListLayout;

use super::node::{AnyView, IntoView, NodeRef, StructuralBinding, UNBUILT, ViewBuilder, widget};
use super::reactive::{self, EffectKey, EffectTarget, Readable, ScopeKey, Signal};
use super::structural::{build_scoped, build_under, container};
use super::style::{ContainerStyle, container_styles};
use crate::{
    AppContext, Entity, FrameworkError, List, ScrollAnchor, ScrollAxes, ScrollChanged,
    ScrollLaidOut, ScrollOffset, ScrollView, ScrollViewportChanged, StableNodeId, Stack,
    VirtualAlignment, VirtualListItems,
};
use nana_ui_core::{AlignSpec, LengthSpec};

/// Where the rows scroll.
enum Container {
    /// A `ScrollView` of the list's own.
    Own(Box<ScrollView>),
    /// An ancestor's, known once it is built.
    Within(NodeRef),
}

/// [`EachVirtual::grid`]: columns at least `min_width` wide, `gap` apart
/// both ways.
#[derive(Clone, Copy)]
struct Grid {
    min_width: f32,
    gap: f32,
}

/// What the virtual list places: one item, or one grid row of them.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Unit<K> {
    Item(K),
    /// A grid row, by its column count and the keys in it.
    Row(u64),
}

/// `v-for` over many rows: see the module docs.
pub struct EachVirtual<T, K, S, KF, RF> {
    items: S,
    key_fn: KF,
    row_fn: RF,
    row_height: f32,
    measured: bool,
    overscan: f32,
    container: Container,
    grid: Option<Grid>,
    style: ContainerStyle,
    list_ref: Option<VirtualListRef<K>>,
    key: Option<Cow<'static, str>>,
    site: &'static Location<'static>,
    _types: PhantomData<fn(T) -> K>,
}

/// Keyed list over `items` whose rows, `row_height` tall, are built only
/// while they are in view. Duplicate keys after the first are skipped.
#[track_caller]
pub fn each_virtual<T, K, S, KF, RF, V>(
    items: S,
    key: KF,
    row_height: f32,
    row: RF,
) -> EachVirtual<T, K, S, KF, RF>
where
    T: Clone + Send + 'static,
    K: Eq + Hash + Clone + Send + 'static,
    S: Readable<Vec<T>>,
    KF: Fn(&T) -> K + Send + 'static,
    RF: Fn(T) -> V + Send + 'static,
    V: IntoView,
{
    EachVirtual {
        items,
        key_fn: key,
        row_fn: row,
        row_height: row_height.max(1.0),
        measured: false,
        overscan: row_height.max(1.0) * 4.0,
        container: Container::Own(Box::new(ScrollView::new(ScrollAxes::Vertical))),
        grid: None,
        style: ContainerStyle::default(),
        list_ref: None,
        key: None,
        site: Location::caller(),
        _types: PhantomData,
    }
}

impl<T, K, S, KF, RF> EachVirtual<T, K, S, KF, RF> {
    /// How far past the viewport rows are kept built, in pixels (default:
    /// four rows).
    pub fn overscan(mut self, overscan: f32) -> Self {
        self.overscan = overscan.max(0.0);
        self
    }

    /// Rows size to their content; the row height given is the estimate for
    /// rows not measured yet. Each layout pass that shows new rows is
    /// followed by one that places them at their measured heights, keeping
    /// the row at the top of the viewport where it is.
    pub fn measured(mut self) -> Self {
        self.measured = true;
        self
    }

    /// The list's height in pixels (the scroll container's). Not with
    /// [`Self::within`].
    pub fn height(self, height: f32) -> Self {
        self.own_scroll(|scroll| {
            scroll.with_layout(|layout| layout.height = Some(LengthSpec::Px(height)))
        })
    }

    /// The list's width in pixels. Not with [`Self::within`].
    pub fn width(self, width: f32) -> Self {
        self.own_scroll(|scroll| {
            scroll.with_layout(|layout| layout.width = Some(LengthSpec::Px(width)))
        })
    }

    /// Take the space left along the parent's main axis. Not with
    /// [`Self::within`].
    pub fn grow(self) -> Self {
        self.own_scroll(|scroll| {
            scroll.with_layout(|layout| {
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            })
        })
    }

    /// The scroll container rows live in, to size or style it. It must scroll
    /// vertically.
    pub fn scroll_view(mut self, scroll: ScrollView) -> Self {
        self.container = Container::Own(Box::new(scroll));
        self
    }

    /// Scroll with `scroll`, an ancestor `ScrollView`, instead of a scroll
    /// container of the list's own: a list under a page's header and above
    /// its footer. The window is the part of that viewport the list covers,
    /// and follows it as content above the list grows or shrinks.
    pub fn within(mut self, scroll: NodeRef) -> Self {
        self.container = Container::Within(scroll);
        self
    }

    /// Flow the items into rows of as many columns at least `min_width` wide
    /// as the list's width fits, `gap` apart across and down; the row height
    /// is one grid row's (without the gap). Columns share the width equally,
    /// and a short last row keeps the others' column width.
    pub fn grid(mut self, min_width: f32, gap: f32) -> Self {
        self.grid = Some(Grid {
            min_width: min_width.max(1.0),
            gap: gap.max(0.0),
        });
        self
    }

    fn own_scroll(mut self, f: impl FnOnce(ScrollView) -> ScrollView) -> Self {
        if let Container::Own(scroll) = self.container {
            self.container = Container::Own(Box::new(f(*scroll)));
        }
        self
    }

    pub fn key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Answer `list_ref` for this list: where each item is, which rows are
    /// built, and scrolling an item into view; see [`VirtualListRef`].
    pub fn list_ref(mut self, list_ref: VirtualListRef<K>) -> Self {
        self.list_ref = Some(list_ref);
        self
    }

    /// Styles go on the list's box in its parent: its own `ScrollView`, or
    /// the list itself with [`Self::within`].
    fn container_style_mut(&mut self) -> &mut ContainerStyle {
        &mut self.style
    }
}

container_styles!([T, K, S, KF, RF] EachVirtual<T, K, S, KF, RF>);

/// Where each item of an [`each_virtual`] list is, measured or estimated,
/// built or not: its top and its height within the list (a grid item's are
/// its grid row's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualItem {
    pub offset: f32,
    pub extent: f32,
}

/// A handle to an [`each_virtual`] list, given to it with
/// [`EachVirtual::list_ref`]: where each item is by its key, which rows are
/// built, and scrolling an item into view. Make it where the list is made
/// (like a [`NodeRef`]; it holds the list's geometry) and clone it into the
/// handlers that use it.
///
/// Reads answer as of the list's last sync (each scroll, resize and data
/// change syncs it); before the list is built they answer `None`.
pub struct VirtualListRef<K> {
    geometry: Arc<Mutex<Geometry<K>>>,
}

impl<K> Clone for VirtualListRef<K> {
    fn clone(&self) -> Self {
        Self {
            geometry: Arc::clone(&self.geometry),
        }
    }
}

/// A [`VirtualListRef`] for a list whose items are keyed by `K`.
pub fn virtual_list_ref<K>() -> VirtualListRef<K> {
    VirtualListRef {
        geometry: Arc::new(Mutex::new(Geometry::default())),
    }
}

impl<K: Eq + Hash + Clone> VirtualListRef<K> {
    fn with<R>(&self, f: impl FnOnce(&mut Geometry<K>) -> R) -> R {
        f(&mut self.geometry.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Where the item keyed `key` is, whether it is built or not.
    pub fn item(&self, key: &K) -> Option<VirtualItem> {
        self.with(|geometry| {
            let at = geometry.unit_of(key)?;
            Some(VirtualItem {
                offset: geometry.layout.extent(0..at),
                extent: geometry.layout.extent(at..at + 1),
            })
        })
    }

    /// The node the item's view is built in while it is built: its key
    /// scope, so `cx.resolve_assembly_path(row, "title")` finds what the row
    /// keyed `title`. `None` while the item is scrolled away.
    pub fn row(&self, key: &K) -> Option<StableNodeId> {
        self.with(|geometry| geometry.rows.get(key).copied())
    }

    /// The reading position: the item at the top of the viewport, and how
    /// far into it the top is. [`Self::scroll_to_inset`] goes back to it.
    pub fn first_visible(&self) -> Option<(K, f32)> {
        self.with(|geometry| geometry.first_visible(geometry.offset))
    }

    /// Scroll so the item keyed `key` is in view, aligned as asked
    /// (`Nearest` moves as little as it can). Built rows around it follow.
    pub fn scroll_to(&self, key: &K, align: VirtualAlignment) {
        self.request(Request::Reveal(key.clone(), align));
    }

    /// Scroll so the top of the viewport is `inset` into the item keyed
    /// `key`: a reading position [`Self::first_visible`] gave.
    pub fn scroll_to_inset(&self, key: &K, inset: f32) {
        self.request(Request::At(key.clone(), inset.max(0.0)));
    }

    fn request(&self, request: Request<K>) {
        let wake = self.with(|geometry| {
            geometry.request = Some(request);
            geometry.wake
        });
        if let Some(wake) = wake {
            wake.update(|at| *at += 1);
        }
    }
}

/// Where a list is asked to scroll to, by item key.
enum Request<K> {
    Reveal(K, VirtualAlignment),
    /// The viewport's top this far into the item.
    At(K, f32),
    /// Keep the item at the viewport's top where it was, `inset` into it,
    /// across a data change: only a move is a request.
    Keep(K, f32),
}

/// What the list places and where, shared by the binding and its
/// [`VirtualListRef`].
struct Geometry<K> {
    layout: VirtualListLayout,
    /// What each placed unit is, in order, and the items it shows.
    units: Vec<Unit<K>>,
    members: Vec<Range<usize>>,
    index: HashMap<Unit<K>, usize>,
    /// Each item's key, in order, and its position.
    keys: Vec<K>,
    positions: HashMap<K, usize>,
    /// A grid's column count at the last sync; 1 for a list.
    columns: usize,
    grid: bool,
    /// The list's own offset at the viewport's top, and the viewport's
    /// height, at the last sync.
    offset: f32,
    extent: f32,
    /// The node each built item's view is in.
    rows: HashMap<K, StableNodeId>,
    request: Option<Request<K>>,
    /// Wakes the binding for a request.
    wake: Option<Signal<u64>>,
}

impl<K> Default for Geometry<K> {
    fn default() -> Self {
        Self {
            layout: VirtualListLayout::new([]),
            units: Vec::new(),
            members: Vec::new(),
            index: HashMap::new(),
            keys: Vec::new(),
            positions: HashMap::new(),
            columns: 1,
            grid: false,
            offset: 0.0,
            extent: 0.0,
            rows: HashMap::new(),
            request: None,
            wake: None,
        }
    }
}

impl<K: Eq + Hash + Clone> Geometry<K> {
    /// The unit an item is placed in.
    fn unit_of(&self, key: &K) -> Option<usize> {
        let position = *self.positions.get(key)?;
        if self.grid {
            let row = position / self.columns.max(1);
            (row < self.units.len()).then_some(row)
        } else {
            self.index.get(&Unit::Item(key.clone())).copied()
        }
    }

    /// The item at `offset` and how far into it `offset` is.
    fn first_visible(&self, offset: f32) -> Option<(K, f32)> {
        let anchor = self.layout.scroll_anchor(offset)?;
        let first = self.members.get(anchor.index)?.start;
        Some((self.keys.get(first)?.clone(), anchor.inset))
    }

    /// The list's offset a request scrolls to; `None` for one that does
    /// not move it (or whose item is gone).
    fn target(&self, request: &Request<K>) -> Option<f32> {
        let target = match request {
            Request::Reveal(key, align) => self.layout.offset_for_index(
                self.unit_of(key)?,
                self.offset,
                self.extent,
                *align,
            )?,
            // A reading position is a point in the list; the ScrollView
            // clamps it to how far its content scrolls, which for a list
            // within a page includes whatever follows the list.
            Request::At(key, inset) | Request::Keep(key, inset) => {
                let at = self.unit_of(key)?;
                (self.layout.extent(0..at) + inset.min(self.layout.extent(at..at + 1)))
                    .clamp(0.0, self.layout.total_extent())
            }
        };
        match request {
            Request::Keep(..) if (target - self.offset).abs() < 0.5 => None,
            _ => Some(target),
        }
    }
}

struct EachVirtualBinding<T, K, S, KF, RF> {
    items: S,
    key_fn: KF,
    row_fn: RF,
    scope: Option<ScopeKey>,
    /// Written by scroll and viewport events: the window moved.
    moved: Signal<u64>,
    /// The ancestor to scroll with; the ScrollView it names is `scroll`.
    within: Option<NodeRef>,
    scroll: Option<Entity<ScrollView>>,
    list: Entity<List>,
    grid: Option<Grid>,
    state: VirtualListItems<Unit<K>, Stack>,
    geometry: Arc<Mutex<Geometry<K>>>,
    row_height: f32,
    measured: bool,
    overscan: f32,
    /// Bumped when the units change, so the range gate re-materializes.
    version: u64,
    _types: PhantomData<fn(T)>,
}

impl<T, K, S, KF, RF, V> EachVirtualBinding<T, K, S, KF, RF>
where
    T: Clone + Send + 'static,
    K: Eq + Hash + Clone + Send + 'static,
    S: Readable<Vec<T>>,
    KF: Fn(&T) -> K + Send + 'static,
    RF: Fn(T) -> V + Send + 'static,
    V: IntoView,
{
    /// Every item with its key, first of each key only; reads what the
    /// binding depends on.
    fn read(&self) -> Vec<(K, T)> {
        self.moved.get();
        if let Some(within) = self.within {
            within.get();
        }
        self.items.with_value(|items| {
            let mut seen = HashSet::with_capacity(items.len());
            items
                .iter()
                .filter_map(|item| {
                    let key = (self.key_fn)(item);
                    seen.insert(key.clone()).then(|| (key, item.clone()))
                })
                .collect()
        })
    }

    /// The scroll container. An ancestor's is observed once its ref names
    /// it, and again when the ref names another one (the list moved to
    /// another scroll area): the old one is let go of. The list observes
    /// them, so they end with it.
    fn scroll(
        &mut self,
        cx: &mut AppContext,
    ) -> Result<Option<Entity<ScrollView>>, FrameworkError> {
        let Some(within) = self.within else {
            return Ok(self.scroll);
        };
        let named = within.get_untracked().filter(|id| cx.world().contains(*id));
        if named == self.scroll.map(|scroll| scroll.stable_id()) {
            return Ok(self.scroll);
        }
        if let Some(old) = self.scroll.take() {
            cx.unobserve(old.stable_id(), self.list.stable_id());
        }
        let Some(id) = named else {
            return Ok(None);
        };
        let scroll = Entity::<ScrollView>::from_stable_id(id);
        let moved = self.moved;
        cx.observe(scroll, self.list, move |_, _: &ScrollChanged, _| {
            moved.update(|at| *at += 1)
        })?;
        cx.observe(scroll, self.list, move |_, _: &ScrollViewportChanged, _| {
            moved.update(|at| *at += 1)
        })?;
        cx.observe(scroll, self.list, move |_, _: &ScrollLaidOut, _| {
            moved.update(|at| *at += 1)
        })?;
        self.scroll = Some(scroll);
        Ok(self.scroll)
    }

    /// Group the items into what the list places: each item, or grid rows
    /// of as many as fit across. A grid with no finite width still places
    /// one column, so the first sync mounts rows.
    fn units(
        &self,
        cx: &AppContext,
        _columns: usize,
        items: &[(K, T)],
    ) -> (usize, Option<Vec<(Unit<K>, Range<usize>)>>) {
        let Some(grid) = self.grid else {
            return (
                1,
                Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(at, (key, _))| (Unit::Item(key.clone()), at..at + 1))
                        .collect(),
                ),
            );
        };
        let measured = cx
            .world()
            .layout_box(self.list.stable_id())
            .map_or(0.0, |bounds| bounds.width);
        // 宽度还没排出来，或排成了非有限数时，仍按一列把行挂上。返回空列表会留下只有间距的空槽。
        let width = if measured.is_finite() && measured > 0.0 {
            measured
        } else {
            grid.min_width
        };
        let columns = grid_columns(width, grid);
        (
            columns,
            Some(
                (0..items.len())
                    .step_by(columns)
                    .map(|start| {
                        let end = (start + columns).min(items.len());
                        let mut hasher = DefaultHasher::new();
                        columns.hash(&mut hasher);
                        for (key, _) in &items[start..end] {
                            key.hash(&mut hasher);
                        }
                        (Unit::Row(hasher.finish()), start..end)
                    })
                    .collect(),
            ),
        )
    }

    fn sync(&mut self, cx: &mut AppContext, items: Vec<(K, T)>) -> Result<(), FrameworkError> {
        let Some(scroll) = self.scroll(cx)? else {
            return Ok(());
        };
        let viewport = cx.virtual_list_viewport(scroll, self.list, 0.0)?;
        let follow_end = cx.read(scroll, |view| view.follow_end)?;
        let shared = Arc::clone(&self.geometry);
        let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
        let geometry = &mut *guard;
        geometry.offset = viewport.offset[1];
        geometry.extent = viewport.extent[1];
        let mut request = geometry.request.take();
        let (columns, units) = self.units(cx, geometry.columns, &items);
        let units = units.unwrap_or_default();
        geometry.columns = columns;
        geometry.grid = self.grid.is_some();
        if units.len() != geometry.units.len()
            || units
                .iter()
                .zip(&geometry.units)
                .any(|((unit, _), kept)| unit != kept)
        {
            // Items inserted or removed above the one at the top of the
            // viewport keep it where it is (scroll anchoring), unless the
            // list follows its end.
            if request.is_none() && !follow_end && !geometry.units.is_empty() {
                request = geometry
                    .first_visible(viewport.offset[1])
                    .map(|(key, inset)| Request::Keep(key, inset));
            }
            // Units that stay keep the extent they were placed or measured at.
            let estimate = self.row_height + self.grid.map_or(0.0, |grid| grid.gap);
            let extents = units
                .iter()
                .map(|(unit, _)| {
                    geometry
                        .index
                        .get(unit)
                        .map_or(estimate, |&at| geometry.layout.extent(at..at + 1))
                })
                .collect::<Vec<_>>();
            geometry.layout = VirtualListLayout::new(extents);
            geometry.index = units
                .iter()
                .enumerate()
                .map(|(at, (unit, _))| (unit.clone(), at))
                .collect();
            (geometry.units, geometry.members) = units.into_iter().unzip();
            geometry.keys = items.iter().map(|(key, _)| key.clone()).collect();
            geometry.positions = geometry
                .keys
                .iter()
                .enumerate()
                .map(|(at, key)| (key.clone(), at))
                .collect();
            self.version += 1;
        }
        let target = request
            .as_ref()
            .and_then(|request| geometry.target(request));
        let (units, members, index) = (&geometry.units, &geometry.members, &geometry.index);
        let (row_fn, scope, grid) = (&self.row_fn, self.scope, self.grid);
        let key_at = |at: usize| units[at].clone();
        let index_of = |unit: &Unit<K>| index.get(unit).copied();
        let row_height = self.row_height;
        let slot = |_: usize, _: &Unit<K>| match grid {
            // The gap below a grid row is part of its extent. The row itself
            // keeps the estimate so a collapsed child cannot shrink the slot
            // down to the padding.
            Some(grid) => Stack::column(0.0).with_layout(|layout| {
                layout.width = Some(LengthSpec::Percent(100.0));
                layout.min_height = Some(LengthSpec::Px(row_height));
                layout.padding_bottom = Some(LengthSpec::Px(grid.gap));
            }),
            None => Stack::column(0.0),
        };
        let mount = |cx: &mut AppContext, slot: Entity<Stack>, at: usize, _: &Unit<K>| {
            let range = members[at].clone();
            let mounted = mount_row(cx, slot, scope, || match grid {
                None => row_fn(items[range.start].1.clone()).into_any(),
                Some(grid) => grid_row(grid, columns, &items[range], row_fn),
            });
            if let Err(error) = &mounted {
                eprintln!("Nana 虚拟行挂载失败：{error}");
            }
            mounted
        };
        let placed = if self.measured {
            let (_, offset) = cx.sync_virtual_list_measured_at(
                scroll,
                self.list,
                &mut self.state,
                &mut geometry.layout,
                self.overscan,
                target,
                self.version,
                &[],
                key_at,
                index_of,
                slot,
                mount,
            )?;
            // Rows mounted now are measured after the next layout pass.
            if self.state.pending_measure() {
                cx.notify_laid_out(scroll);
            }
            target.map(|_| offset)
        } else {
            cx.sync_virtual_list_retained_at(
                scroll,
                self.list,
                &mut self.state,
                &geometry.layout,
                self.overscan,
                target,
                self.version,
                &[],
                key_at,
                index_of,
                slot,
                mount,
            )?;
            target
        };
        if let Some(offset) = placed {
            self.scroll_list_to(cx, scroll, offset)?;
            geometry.offset = offset;
        }
        // Rows revealed at their estimate are measured after the next
        // layout, which moves an item aligned to the end or the center:
        // reveal it again then, until nothing it shows is estimated.
        if self.state.pending_measure()
            && let Some(request @ Request::Reveal(..)) = request
        {
            geometry.request = Some(request);
        }
        geometry.rows = self.built_rows(cx, geometry);
        if self.within.is_some() || self.grid.is_some() {
            // Content above the list, or the list's own width, can change
            // with no scroll or viewport change: look again after the next
            // layout pass (one only happens when something changed).
            cx.notify_laid_out(scroll);
        }
        Ok(())
    }

    /// Scroll so the viewport's top is `offset` into the list: now, as far
    /// as the ScrollView's extent allows, and exactly once the next layout
    /// gives it the list's new extent (a data change the list has not been
    /// laid out with yet), wherever the list then sits in the content.
    fn scroll_list_to(
        &self,
        cx: &mut AppContext,
        scroll: Entity<ScrollView>,
        offset: f32,
    ) -> Result<(), FrameworkError> {
        let start = match (
            cx.world().layout_box(self.list.stable_id()),
            cx.world().layout_box(scroll.stable_id()),
        ) {
            (Some(list), Some(scroll)) => list.y - scroll.y,
            _ => 0.0,
        };
        let current = cx
            .world()
            .scroll_offset(scroll.stable_id())
            .unwrap_or_default();
        cx.scroll_to(
            scroll,
            ScrollOffset {
                x: current.x,
                y: start + offset,
            },
        )?;
        cx.restore_scroll_anchor(
            scroll,
            ScrollAnchor {
                row: self.list.stable_id(),
                viewport_y: -offset,
            },
        )
    }

    /// The node each built item's view is in: its row's slot, or in a grid
    /// its cell of the row.
    fn built_rows(&self, cx: &AppContext, geometry: &Geometry<K>) -> HashMap<K, StableNodeId> {
        let mut rows = HashMap::new();
        for unit in self.state.mounted_keys() {
            let (Some(slot), Some(&at)) = (self.state.entity(unit), geometry.index.get(unit))
            else {
                continue;
            };
            let members = geometry.members[at].clone();
            if self.grid.is_none() {
                if let Some(key) = geometry.keys.get(members.start) {
                    rows.insert(key.clone(), slot.stable_id());
                }
                continue;
            }
            // A grid row's slot holds the row, which holds a cell per column.
            let cells = cx
                .world()
                .node(slot.stable_id())
                .and_then(|slot| slot.children.first().copied())
                .and_then(|row| cx.world().node(row))
                .map(|row| row.children.to_vec())
                .unwrap_or_default();
            for (position, cell) in members.zip(cells) {
                if let Some(key) = geometry.keys.get(position) {
                    rows.insert(key.clone(), cell);
                }
            }
        }
        rows
    }
}

fn grid_columns(width: f32, grid: Grid) -> usize {
    if !width.is_finite() || width <= 0.0 {
        return 1;
    }
    let columns = ((width + grid.gap) / (grid.min_width + grid.gap)).floor();
    if !columns.is_finite() {
        return 1;
    }
    (columns as usize).max(1)
}

/// One grid row: each item in an equal share of the width. A short last
/// row leaves the shares after it empty, so its columns line up.
fn grid_row<T: Clone, K, V: IntoView>(
    grid: Grid,
    columns: usize,
    items: &[(K, T)],
    row_fn: &impl Fn(T) -> V,
) -> AnyView {
    let share = || {
        Stack::column(0.0).with_layout(|layout| {
            layout.width = Some(LengthSpec::Px(0.0));
            layout.min_width = Some(LengthSpec::Px(0.0));
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(1.0);
        })
    };
    let mut cells = items
        .iter()
        .map(|(_, item)| {
            widget(share())
                .children(row_fn(item.clone()).into_any())
                .into_any()
        })
        .collect::<Vec<_>>();
    cells.extend((items.len()..columns).map(|_| widget(share()).into_any()));
    // 不用 fill_row：它在纵向父级里会按主轴 grow，行高会被压成 0，槽里只剩间距。
    widget(
        Stack::row(grid.gap)
            .align(AlignSpec::Start)
            .width(LengthSpec::Percent(100.0))
            .min_width(LengthSpec::Px(0.0)),
    )
    .children(cells)
    .into_any()
}

/// Build one row's view into its placement slot, in a scope the row's roots
/// own: despawning the slot disposes it. The slot is the row's key scope:
/// what the row keys is found from it, and rows keying the same names do
/// not meet.
fn mount_row<V: IntoView>(
    cx: &mut AppContext,
    slot: Entity<Stack>,
    scope: Option<ScopeKey>,
    row: impl FnOnce() -> V,
) -> Result<(), FrameworkError> {
    build_under(cx, slot, |vb, created| {
        let built = build_scoped(vb, scope, || row().into_any());
        created.push(built.scope);
    })
}

impl<T, K, S, KF, RF, V> IntoView for EachVirtual<T, K, S, KF, RF>
where
    T: Clone + Send + 'static,
    K: Eq + Hash + Clone + Send + 'static,
    S: Readable<Vec<T>>,
    KF: Fn(&T) -> K + Send + 'static,
    RF: Fn(T) -> V + Send + 'static,
    V: IntoView,
{
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let moved = super::signal(0u64);
        let bump = move || moved.update(|at| *at += 1);
        // The list takes the width it is given, not its rows': rows wrap
        // their text to it and grid columns divide it.
        let mut list = List::new();
        std::sync::Arc::make_mut(&mut list.style.layout).width = Some(LengthSpec::Percent(100.0));
        let (scroll, list, within) = match self.container {
            Container::Own(scroll) => {
                let Some(scroll) = container(*scroll, self.key, self.style, self.site).place(vb)
                else {
                    return;
                };
                let list = vb.ui.nest(scroll, |ui| ui.child("list", list));
                vb.ui.on(scroll, move |_, _: &ScrollChanged, _| bump());
                vb.ui
                    .on(scroll, move |_, _: &ScrollViewportChanged, _| bump());
                if self.measured || self.grid.is_some() {
                    vb.ui.on(scroll, move |_, _: &ScrollLaidOut, _| bump());
                }
                (Some(scroll), list, None)
            }
            Container::Within(within) => {
                let Some(list) = container(list, self.key, self.style, self.site).place(vb) else {
                    return;
                };
                (None, list, Some(within))
            }
        };
        let id = list.stable_id();
        if id == UNBUILT {
            return;
        }
        let geometry = match self.list_ref {
            Some(list_ref) => list_ref.geometry,
            None => Arc::new(Mutex::new(Geometry::default())),
        };
        {
            // A handle given to a list built again answers for the new one.
            let mut shared = geometry.lock().unwrap_or_else(PoisonError::into_inner);
            *shared = Geometry::default();
            shared.wake = Some(moved);
        }
        let effect =
            reactive::create_effect(vb.st.tag, EffectTarget::Structural(id), None, self.site);
        let binding = EachVirtualBinding {
            items: self.items,
            key_fn: self.key_fn,
            row_fn: self.row_fn,
            scope: reactive::current_scope(),
            moved,
            within,
            scroll,
            list,
            grid: self.grid,
            state: if self.measured {
                VirtualListItems::measured()
            } else {
                VirtualListItems::default()
            },
            geometry,
            row_height: self.row_height,
            measured: self.measured,
            overscan: self.overscan,
            version: 0,
            _types: PhantomData,
        };
        // Subscribe now; the first window follows the first layout, whose
        // viewport event moves it. An ancestor's events are only heard once
        // a sync has found it, so ask for that first sync.
        let within = binding.within.is_some();
        let grid = binding.grid.is_some();
        reactive::run_tracked(effect, || binding.read());
        vb.st.parts.structural.push((id, effect, Box::new(binding)));
        // 网格和借用滚动都要在第一次布局前挂上视口内的行。只等 ScrollLaidOut 时，离屏第一帧是空槽。
        if within || grid {
            bump();
        }
    }
}

impl<T, K, S, KF, RF, V> StructuralBinding for EachVirtualBinding<T, K, S, KF, RF>
where
    T: Clone + Send + 'static,
    K: Eq + Hash + Clone + Send + 'static,
    S: Readable<Vec<T>>,
    KF: Fn(&T) -> K + Send + 'static,
    RF: Fn(T) -> V + Send + 'static,
    V: IntoView,
{
    fn update(
        &mut self,
        cx: &mut AppContext,
        _container: StableNodeId,
        effect: EffectKey,
    ) -> Result<(), FrameworkError> {
        let items = reactive::run_tracked(effect, || self.read());
        self.sync(cx, items)
    }
}
