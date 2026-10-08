//! Elements, fragments, and the builder that lowers them into one retained
//! commit.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::panic::Location;

use super::prop::{FieldWrite, IntoProp};
use super::reactive::{self, EffectKey, EffectTarget, ScopeKey, Signal, SignalKey};
use crate::{
    AppContext, ComponentView, Entity, FrameworkError, MutationQueue, StableNodeId, UiBuilder,
    View, ViewContext,
};

pub(crate) const UNBUILT: StableNodeId = match StableNodeId::new(u64::MAX) {
    Some(id) => id,
    None => panic!("u64::MAX is a valid stable id"),
};

pub(crate) struct DirectBinding<C> {
    pub(crate) source: SignalKey,
    pub(crate) apply: fn(&mut C, &dyn Any),
    pub(crate) differs: fn(&C, &dyn Any) -> bool,
    pub(crate) field: &'static str,
    pub(crate) site: &'static Location<'static>,
}

/// One closure-driven field, evaluated once per run: [`Self::evaluate`]
/// computes the value and keeps it, [`Self::write`] hands it to the staged
/// copy.
pub(crate) trait DynProp<C>: Send {
    /// Compute the value; true when it differs from `current`'s field.
    fn evaluate(&mut self, current: &C) -> bool;
    fn write(&mut self, target: &mut C);
    fn discard(&mut self);
}

pub(crate) struct DynBinding<C> {
    pub(crate) prop: Box<dyn DynProp<C>>,
    pub(crate) field: &'static str,
    pub(crate) site: &'static Location<'static>,
}

/// [`El::bind`]: an arbitrary edit, so it always counts as a change.
struct BindProp<F>(F);

impl<C, F: FnMut(&mut C) + Send> DynProp<C> for BindProp<F> {
    fn evaluate(&mut self, _: &C) -> bool {
        true
    }

    fn write(&mut self, target: &mut C) {
        (self.0)(target)
    }

    fn discard(&mut self) {}
}

/// Every dynamic field of one node. They share one effect: a change to any
/// input evaluates all of them against the retained view; only when one
/// differs is the view copied, written and projected, once.
pub struct NodeBindings<C> {
    pub(crate) direct: Vec<DirectBinding<C>>,
    pub(crate) dynamic: Vec<DynBinding<C>>,
}

impl<C> Default for NodeBindings<C> {
    fn default() -> Self {
        Self {
            direct: Vec::new(),
            dynamic: Vec::new(),
        }
    }
}

impl<C> NodeBindings<C> {
    fn is_empty(&self) -> bool {
        self.direct.is_empty() && self.dynamic.is_empty()
    }

    /// The first run, on the component before it is created.
    pub(crate) fn apply(&mut self, target: &mut C) {
        for binding in &self.direct {
            let cell = reactive::read_cell(binding.source, binding.site, true);
            (binding.apply)(target, &*cell);
        }
        for binding in &mut self.dynamic {
            binding.prop.evaluate(target);
            binding.prop.write(target);
        }
    }

    /// Evaluate every binding against the retained view, tracking what they
    /// read. True when any field would change.
    pub(crate) fn evaluate(&mut self, current: &C) -> bool {
        let mut changed = false;
        for binding in &self.direct {
            let cell = reactive::read_cell(binding.source, binding.site, true);
            changed |= (binding.differs)(current, &*cell);
        }
        for binding in &mut self.dynamic {
            changed |= binding.prop.evaluate(current);
        }
        changed
    }

    /// Write what [`Self::evaluate`] computed into a staged copy.
    pub(crate) fn write(&mut self, target: &mut C) {
        for binding in &self.direct {
            let cell = reactive::read_cell(binding.source, binding.site, false);
            (binding.apply)(target, &*cell);
        }
        for binding in &mut self.dynamic {
            binding.prop.write(target);
        }
    }

    fn discard(&mut self) {
        for binding in &mut self.dynamic {
            binding.prop.discard();
        }
    }
}

/// What [`AppContext::view_bindings`] reports for one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeBindingInfo {
    /// Where the element was declared.
    pub element: &'static Location<'static>,
    /// `Component.field` and where its binding was declared.
    pub fields: Vec<(&'static str, &'static Location<'static>)>,
    /// The `.vue` source location, when this node came from an SFC template.
    pub source: Option<SourceLocation>,
    /// The `.vue` source location for each bound field, when available.
    pub source_fields: Vec<(&'static str, SourceLocation)>,
}

/// A source position carried explicitly by the `.vue` compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
}

impl SourceLocation {
    pub const fn new(file: &'static str, line: u32, column: u32) -> Self {
        Self { file, line, column }
    }
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// Compiler-provided source metadata. Field names are the setter names;
/// positions use one-based lines and columns. No source file is read at runtime.
#[doc(hidden)]
#[derive(Debug)]
pub struct ViewSource {
    pub element: SourceLocation,
    pub fields: &'static [(&'static str, SourceLocation)],
}

/// Type-erased node bindings, owned by the [`AppContext`].
pub(crate) trait NodePatch: Send {
    /// Apply the bindings to a staged copy of the node's view and project it
    /// into `mutations`. `None` when nothing changed.
    fn stage(
        &mut self,
        cx: &AppContext,
        id: StableNodeId,
        mutations: &mut MutationQueue,
    ) -> Result<Option<Box<dyn Any + Send>>, FrameworkError>;

    fn fields(&self) -> Vec<(&'static str, &'static Location<'static>)>;
}

impl<C: ComponentView> NodePatch for NodeBindings<C> {
    fn stage(
        &mut self,
        cx: &AppContext,
        id: StableNodeId,
        mutations: &mut MutationQueue,
    ) -> Result<Option<Box<dyn Any + Send>>, FrameworkError> {
        // Field-level check first: a re-run that reproduces the node's
        // values copies and projects nothing.
        let current = crate::framework::bound_view::<C>(cx, id)?;
        if !self.evaluate(current) && !C::ALWAYS_REPROJECT {
            self.discard();
            return Ok(None);
        }
        crate::framework::stage_bound_node::<C>(cx, id, mutations, |staged| self.write(staged))
    }

    fn fields(&self) -> Vec<(&'static str, &'static Location<'static>)> {
        self.direct
            .iter()
            .map(|binding| (binding.field, binding.site))
            .chain(
                self.dynamic
                    .iter()
                    .map(|binding| (binding.field, binding.site)),
            )
            .collect()
    }
}

/// A keyed list or conditional block that rebuilds part of its container.
pub(crate) trait StructuralBinding: Send {
    fn update(
        &mut self,
        cx: &mut AppContext,
        container: StableNodeId,
        effect: EffectKey,
    ) -> Result<(), FrameworkError>;
}

/// A node as the view layer sees it, for devtools: the built-in control it
/// is and each bindable field with its value, where the element was
/// declared and where each bound field's binding was.
#[derive(Debug, Clone, PartialEq)]
pub struct Inspection {
    pub node: StableNodeId,
    /// The control table's tag (`"Button"`); `None` for other components.
    pub control: Option<&'static str>,
    pub fields: Vec<InspectedField>,
    /// Where the element was declared, for a node with bindings.
    pub element: Option<&'static Location<'static>>,
    /// Original source position, when supplied by a compiler.
    pub source_element: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InspectedField {
    pub name: &'static str,
    /// `Debug` of the value.
    pub value: String,
    /// Where the binding that drives it was declared; an edit is replaced
    /// the next time that binding runs.
    pub bound_at: Option<&'static Location<'static>>,
    pub source_bound_at: Option<SourceLocation>,
}

/// What a build leaves for the [`AppContext`] to own once it commits.
#[derive(Default)]
pub(crate) struct ViewParts {
    pub(crate) nodes: Vec<(
        StableNodeId,
        EffectKey,
        Box<dyn NodePatch>,
        Option<&'static ViewSource>,
    )>,
    pub(crate) structural: Vec<(StableNodeId, EffectKey, Box<dyn StructuralBinding>)>,
    /// Scopes disposed when the node leaves the world.
    pub(crate) anchors: Vec<(StableNodeId, ScopeKey)>,
    pub(crate) implicit: Vec<(StableNodeId, Box<[super::Implicit]>)>,
    /// Composites to assemble once built (their `slot_assembler`, or their
    /// `assembler`, which otherwise runs only after a write), each after
    /// its slots and its children.
    pub(crate) assemble: Vec<(StableNodeId, TypeId)>,
}

#[derive(Default)]
struct Level {
    next_auto: usize,
    roots: Vec<StableNodeId>,
    /// The key each root was declared with, beside `roots`; `None` for a
    /// positional one. A slot's roots take it under the node that adopts
    /// them.
    declared: Vec<Option<Cow<'static, str>>>,
    /// A slot's level: its roots are left for the composite to place.
    detached: bool,
}

pub(crate) struct ViewState {
    pub(crate) tag: u64,
    levels: Vec<Level>,
    pub(crate) parts: ViewParts,
    /// A [`keyed`] key waiting for the next unkeyed root at this level.
    pending_key: Option<Cow<'static, str>>,
    /// Where a detached build's roots are keyed: the container a structural
    /// view places them in afterwards.
    pub(crate) root_keys: Option<RootKeys>,
}

/// [`ViewState::root_keys`]: a structural view building roots for a
/// container that already exists keys them under it, as the roots it built
/// at mount were.
#[derive(Clone, Copy)]
pub(crate) struct RootKeys {
    pub(crate) container: StableNodeId,
    /// Positional roots too. A branch replaces the roots before it, so its
    /// positional keys are the ones they had; rows join others, whose
    /// positional keys are counted from another build, so only a declared
    /// key names a row.
    pub(crate) positional: bool,
}

impl ViewState {
    pub(crate) fn new(tag: u64) -> Self {
        Self {
            tag,
            levels: vec![Level::default()],
            parts: ViewParts::default(),
            pending_key: None,
            root_keys: None,
        }
    }
}

/// Lowers views into a [`UiBuilder`] batch. Views reach it only through
/// [`IntoView`].
pub struct ViewBuilder<'u, 'a, 's> {
    pub(crate) ui: &'u mut UiBuilder<'a>,
    pub(crate) st: &'s mut ViewState,
}

impl<'a> ViewBuilder<'_, 'a, '_> {
    fn level(&mut self) -> &mut Level {
        self.st.levels.last_mut().expect("view builder has a level")
    }

    /// Position is identity for undeclared keys: static structure is built
    /// once, so the n-th child stays the n-th child.
    pub(crate) fn auto_key(&mut self) -> String {
        let level = self.level();
        let index = level.next_auto;
        level.next_auto += 1;
        format!("#v{index}")
    }

    /// Create `component` as a root of this level: keyed under the current
    /// parent, or, in a slot, detached for the composite that places it. A
    /// component's key ([`keyed`]) names its root over the root's own.
    pub(crate) fn place<C: ComponentView>(
        &mut self,
        key: Option<Cow<'static, str>>,
        component: C,
    ) -> Entity<C> {
        let declared = self.st.pending_key.take().or(key);
        let entity = if self.level().detached {
            self.ui.detached(component)
        } else {
            let key = match &declared {
                Some(key) => key.to_string(),
                None => self.auto_key(),
            };
            let root_key = self
                .st
                .root_keys
                .filter(|keys| self.st.levels.len() == 1 && (keys.positional || declared.is_some()))
                .map(|keys| (keys.container, key.clone()));
            let entity = self.ui.child(key, component);
            if let Some((container, key)) = root_key {
                self.ui.key_parked_root(container, key, entity);
            }
            entity
        };
        if entity.stable_id() != UNBUILT {
            let level = self.level();
            level.roots.push(entity.stable_id());
            level.declared.push(declared);
        }
        entity
    }

    pub(crate) fn nest<P: View>(
        &mut self,
        parent: Entity<P>,
        f: impl FnOnce(&mut ViewBuilder<'_, 'a, '_>),
    ) {
        // A pending key names a root at this level, never a child.
        let pending = self.st.pending_key.take();
        self.st.levels.push(Level::default());
        let st = &mut *self.st;
        self.ui.nest(parent, |ui| f(&mut ViewBuilder { ui, st }));
        self.st.levels.pop();
        self.st.pending_key = pending;
    }

    /// Build `view` at the current level and return the roots it added.
    pub(crate) fn build_collect(&mut self, view: impl IntoView) -> Vec<StableNodeId> {
        let start = self.level().roots.len();
        view.build(self);
        self.level().roots[start..].to_vec()
    }

    /// Build `view` without placing its roots, for a slot: its roots, each
    /// with the key it was declared with.
    fn build_slot(
        &mut self,
        view: impl IntoView,
    ) -> Vec<(StableNodeId, Option<Cow<'static, str>>)> {
        let pending = self.st.pending_key.take();
        self.st.levels.push(Level {
            detached: true,
            ..Level::default()
        });
        view.build(self);
        let roots = self
            .st
            .levels
            .pop()
            .map(|level| level.roots.into_iter().zip(level.declared).collect());
        self.st.pending_key = pending;
        roots.unwrap_or_default()
    }
}

/// Something that lowers into retained nodes: an element, a tuple of views,
/// a keyed list, a conditional block.
pub trait IntoView: Sized + 'static {
    #[doc(hidden)]
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>);

    /// Erase the type, e.g. to return different views from one branch.
    fn into_any(self) -> AnyView {
        AnyView(Box::new(move |vb: &mut ViewBuilder<'_, '_, '_>| {
            self.build(vb)
        }))
    }
}

/// A type-erased view.
pub struct AnyView(Box<dyn for<'u, 'a, 's> FnOnce(&mut ViewBuilder<'u, 'a, 's>)>);

impl IntoView for AnyView {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        (self.0)(vb)
    }

    fn into_any(self) -> AnyView {
        self
    }
}

impl IntoView for () {
    fn build(self, _: &mut ViewBuilder<'_, '_, '_>) {}
}

impl<V: IntoView> IntoView for Option<V> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        if let Some(view) = self {
            view.build(vb);
        }
    }
}

impl<V: IntoView> IntoView for Vec<V> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        for view in self {
            view.build(vb);
        }
    }
}

macro_rules! tuple_views {
    ($($name:ident),+) => {
        impl<$($name: IntoView),+> IntoView for ($($name,)+) {
            #[allow(non_snake_case)]
            fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
                let ($($name,)+) = self;
                $($name.build(vb);)+
            }
        }
    };
}

tuple_views!(A);
tuple_views!(A, B);
tuple_views!(A, B, C);
tuple_views!(A, B, C, D);
tuple_views!(A, B, C, D, E);
tuple_views!(A, B, C, D, E, F);
tuple_views!(A, B, C, D, E, F, G);
tuple_views!(A, B, C, D, E, F, G, H);
tuple_views!(A, B, C, D, E, F, G, H, I);
tuple_views!(A, B, C, D, E, F, G, H, I, J);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K);
tuple_views!(A, B, C, D, E, F, G, H, I, J, K, L);

/// A view and the refs its mount hands back resolved: the closure given to
/// [`AppContext::mount_view_root`] (and the other mounts) may return
/// `with_refs(tree, (label, ok))`, and the mount then returns the
/// [`MountedView`](crate::MountedView) with `(Entity<Text>, Entity<Button>)`.
pub struct WithRefs<V, R> {
    view: V,
    refs: R,
}

/// See [`WithRefs`]. Make the refs inside the closure: they are signals, and
/// there they belong to the mount.
pub fn with_refs<V: IntoView, R: Refs>(view: V, refs: R) -> WithRefs<V, R> {
    WithRefs { view, refs }
}

/// Refs a mount resolves once the tree is built: an [`EntityRef<C>`] becomes
/// its `Entity<C>`, a [`NodeRef`] its node id, and tuples, arrays and `Vec`s
/// of them the same shape of results. `None` when an element was not built
/// (one under a `when` that did not hold).
pub trait Refs {
    type Resolved;
    fn resolve(self) -> Option<Self::Resolved>;
}

impl<C: View> Refs for EntityRef<C> {
    type Resolved = Entity<C>;
    fn resolve(self) -> Option<Entity<C>> {
        self.get()
    }
}

impl Refs for NodeRef {
    type Resolved = StableNodeId;
    fn resolve(self) -> Option<StableNodeId> {
        self.get_untracked()
    }
}

impl Refs for () {
    type Resolved = ();
    fn resolve(self) -> Option<()> {
        Some(())
    }
}

impl<R: Refs, const N: usize> Refs for [R; N] {
    type Resolved = [R::Resolved; N];
    fn resolve(self) -> Option<Self::Resolved> {
        let resolved: Vec<_> = self.into_iter().map(Refs::resolve).collect::<Option<_>>()?;
        resolved.try_into().ok()
    }
}

impl<R: Refs> Refs for Vec<R> {
    type Resolved = Vec<R::Resolved>;
    fn resolve(self) -> Option<Self::Resolved> {
        self.into_iter().map(Refs::resolve).collect()
    }
}

macro_rules! tuple_refs {
    ($($name:ident),+) => {
        impl<$($name: Refs),+> Refs for ($($name,)+) {
            type Resolved = ($($name::Resolved,)+);
            #[allow(non_snake_case)]
            fn resolve(self) -> Option<Self::Resolved> {
                let ($($name,)+) = self;
                Some(($($name.resolve()?,)+))
            }
        }
    };
}

tuple_refs!(A);
tuple_refs!(A, B);
tuple_refs!(A, B, C);
tuple_refs!(A, B, C, D);
tuple_refs!(A, B, C, D, E);
tuple_refs!(A, B, C, D, E, F);
tuple_refs!(A, B, C, D, E, F, G);
tuple_refs!(A, B, C, D, E, F, G, H);

/// What a mount's closure returns: a view (the mount returns a
/// [`MountedView`](crate::MountedView)), or [`with_refs`] (it returns the
/// mounted view and the resolved refs).
pub trait Mount {
    #[doc(hidden)]
    type View: IntoView;
    #[doc(hidden)]
    type Refs: Refs;
    #[doc(hidden)]
    type Output;
    #[doc(hidden)]
    fn split(self) -> (Self::View, Self::Refs);
    #[doc(hidden)]
    fn finish(mounted: crate::MountedView, refs: <Self::Refs as Refs>::Resolved) -> Self::Output;
}

impl<V: IntoView> Mount for V {
    type View = V;
    type Refs = ();
    type Output = crate::MountedView;
    fn split(self) -> (V, ()) {
        (self, ())
    }
    fn finish(mounted: crate::MountedView, _: ()) -> crate::MountedView {
        mounted
    }
}

impl<V: IntoView, R: Refs> Mount for WithRefs<V, R> {
    type View = V;
    type Refs = R;
    type Output = (crate::MountedView, R::Resolved);
    fn split(self) -> (V, R) {
        (self.view, self.refs)
    }
    fn finish(mounted: crate::MountedView, refs: R::Resolved) -> Self::Output {
        (mounted, refs)
    }
}

/// The children [`El::with`] collects.
pub struct Children(Vec<AnyView>);

impl Children {
    /// Add `view` after the children added so far.
    pub fn add(&mut self, view: impl IntoView) -> &mut Self {
        self.0.push(view.into_any());
        self
    }
}

/// A view built where it is written but placed nowhere: its roots stay
/// parked, for something else to place later (a composite that takes the
/// node by id, `reconcile_children`, a later `append_child`). Get their ids
/// with [`EntityRef`]s. Nodes nothing places stay parked until the document
/// goes, and unmounting the view does not remove them.
pub fn detached<V: IntoView>(view: V) -> Detached<V> {
    Detached(view)
}

/// See [`detached`].
pub struct Detached<V>(V);

impl<V: IntoView> IntoView for Detached<V> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        vb.build_slot(self.0);
    }
}

/// A view whose first root takes `key`: a key on a component, as Vue puts
/// it on the component's root element.
pub struct Keyed<V> {
    key: Cow<'static, str>,
    view: V,
}

/// Name the first root `view` builds among its siblings.
pub fn keyed<V: IntoView>(key: impl Into<Cow<'static, str>>, view: V) -> Keyed<V> {
    Keyed {
        key: key.into(),
        view,
    }
}

impl<V: IntoView> IntoView for Keyed<V> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        vb.st.pending_key = Some(self.key);
        self.view.build(vb);
        vb.st.pending_key = None;
    }
}

type EventInstall<C> = Box<dyn FnOnce(&mut UiBuilder<'_>, Entity<C>)>;

/// A view handed to a component by the id of its root ([`El::slot`]).
struct Slot<C> {
    view: AnyView,
    write: Box<dyn FnOnce(C, StableNodeId) -> C>,
    child: bool,
}

/// The node an element was built as ([`El::node_ref`]); `None` until then.
pub type NodeRef = Signal<Option<StableNodeId>>;

/// A [`NodeRef`] owned by the current scope.
#[track_caller]
pub fn node_ref() -> NodeRef {
    super::signal(None)
}

/// The entity an element of type `C` was built as ([`El::entity_ref`]): a
/// typed [`NodeRef`], for code that updates the node by hand afterwards.
pub struct EntityRef<C> {
    node: NodeRef,
    _type: std::marker::PhantomData<fn() -> C>,
}

impl<C> Clone for EntityRef<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for EntityRef<C> {}

/// An [`EntityRef`] owned by the current scope.
#[track_caller]
pub fn entity_ref<C: ComponentView>() -> EntityRef<C> {
    EntityRef {
        node: node_ref(),
        _type: std::marker::PhantomData,
    }
}

impl<C: View> EntityRef<C> {
    /// The entity, once the element is built; `None` before. Not tracked:
    /// reading it binds nothing.
    pub fn get(&self) -> Option<Entity<C>> {
        self.node.get_untracked().map(Entity::from_stable_id)
    }

    /// The same reference untyped, for what takes a [`NodeRef`] (a
    /// teleport target).
    pub fn node_ref(&self) -> NodeRef {
        self.node
    }
}

/// One component node with its constant fields, bindings, event handlers
/// and children.
pub struct El<C: ComponentView, K = ()> {
    component: C,
    key: Option<Cow<'static, str>>,
    /// Every [`NodeRef`] (an [`EntityRef`]'s among them) to record the node
    /// in once it is built.
    node_refs: Vec<NodeRef>,
    bindings: NodeBindings<C>,
    events: Vec<EventInstall<C>>,
    /// Properties that animate to their new value when a binding changes
    /// them (CSS `transition`).
    implicit: Vec<super::Implicit>,
    slots: Vec<Slot<C>>,
    classes: Option<super::style::Classes<C>>,
    /// Boxed, so an element is the same size whatever it holds: a static
    /// tree is a value, and holding its children inline made each
    /// element as large as its subtree (a settings page, some 70 KB),
    /// copied by every builder method and every frame that builds it.
    children: Box<K>,
    site: &'static Location<'static>,
    source: Option<&'static ViewSource>,
}

/// Any component as an element. Bind fields with [`El::bind`] or
/// [`El::prop`], listen with [`El::on`].
#[track_caller]
pub fn widget<C: ComponentView>(component: C) -> El<C> {
    El {
        component,
        key: None,
        node_refs: Vec::new(),
        bindings: NodeBindings::default(),
        events: Vec::new(),
        implicit: Vec::new(),
        slots: Vec::new(),
        classes: None,
        children: Box::new(()),
        site: Location::caller(),
        source: None,
    }
}

impl<C: ComponentView, K> El<C, K> {
    /// Name this node among its siblings, for [`AppContext::resolve_assembly_path`].
    /// Undeclared keys are positional.
    pub fn key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Record this node's id in `node_ref` once it is built (Vue's template
    /// `ref`), for [`on_mount`](super::on_mount) and event handlers. An
    /// element can fill several: each `node_ref` / [`Self::entity_ref`] it is
    /// given gets the node.
    pub fn node_ref(mut self, node_ref: NodeRef) -> Self {
        self.node_refs.push(node_ref);
        self
    }

    /// Where the element was declared, for traces: the structural view's
    /// site for the container it builds.
    pub(crate) fn declared_at(mut self, site: &'static Location<'static>) -> Self {
        self.site = site;
        self
    }

    /// Attach the originating `.vue` template position to this node and its
    /// bindings. This is emitted only by the SFC compiler.
    #[doc(hidden)]
    pub fn source_site(mut self, source: &'static ViewSource) -> Self {
        self.source = Some(source);
        self
    }

    pub(crate) fn map_component(mut self, f: impl FnOnce(C) -> C) -> Self {
        self.component = f(self.component);
        self
    }

    pub(crate) fn classes_mut(&mut self) -> &mut Option<super::style::Classes<C>> {
        &mut self.classes
    }

    pub(crate) fn parts_mut(&mut self) -> (&mut C, &mut NodeBindings<C>) {
        (&mut self.component, &mut self.bindings)
    }

    /// Drive one field through a [`FieldWrite`].
    #[track_caller]
    pub fn prop<T: 'static, W: FieldWrite<C, T> + 'static>(
        mut self,
        value: impl IntoProp<T>,
    ) -> Self {
        value.bind_field::<C, W>(&mut self.component, &mut self.bindings, Location::caller());
        self
    }

    /// Re-run `f` on the component whenever a signal it reads changes.
    #[track_caller]
    pub fn bind(mut self, f: impl FnMut(&mut C) + Send + 'static) -> Self {
        self.bindings.dynamic.push(DynBinding {
            prop: Box::new(BindProp(f)),
            field: "bind",
            site: Location::caller(),
        });
        self
    }

    /// Run `f` for every `E` this node emits.
    pub fn on<E: Send + 'static>(mut self, mut f: impl FnMut(&E) + Send + 'static) -> Self {
        self.events.push(Box::new(move |ui, entity| {
            ui.on(entity, move |_: &mut C, event: &E, _| f(event));
        }));
        self
    }

    /// Like [`Self::on`], with the component and its context: to change
    /// the component in place, emit an event, or send the application's
    /// program a message (`cx.dispatch_program(..)`).
    pub fn on_cx<E: Send + 'static>(
        mut self,
        f: impl FnMut(&mut C, &E, &mut ViewContext<'_, C>) + Send + 'static,
    ) -> Self {
        self.events.push(Box::new(move |ui, entity| {
            ui.on(entity, f);
        }));
        self
    }

    /// Record the entity this element is built as in `entity_ref`, beside
    /// any [`Self::node_ref`] it also fills.
    pub fn entity_ref(self, entity_ref: EntityRef<C>) -> Self {
        self.node_ref(entity_ref.node)
    }

    /// The children, added in a block of ordinary Rust:
    ///
    /// ```ignore
    /// column().gap(8).with(|c| {
    ///     c.add(text("标题"));
    ///     for tag in ["a", "b"] {
    ///         c.add(text(tag));
    ///     }
    /// })
    /// ```
    ///
    /// The block runs once, while the view is built: a `for` or `if` in it
    /// shapes the tree then and does not follow data. For structure that
    /// follows data, add an [`each`](super::each) or a
    /// [`when`](super::when).
    pub fn with(self, add: impl FnOnce(&mut Children)) -> El<C, Vec<AnyView>> {
        let mut children = Children(Vec::new());
        add(&mut children);
        self.children(children.0)
    }

    /// Replace the children.
    pub fn children<K2: IntoView>(self, children: K2) -> El<C, K2> {
        El {
            component: self.component,
            key: self.key,
            node_refs: self.node_refs,
            bindings: self.bindings,
            events: self.events,
            implicit: self.implicit,
            slots: self.slots,
            classes: self.classes,
            children: Box::new(children),
            site: self.site,
            source: self.source,
        }
    }

    /// Hand `view` to the component by the id of its one root, for a slot
    /// the component places itself (a shell region): `view` is built
    /// first, `write` gives the component its id, and the component's
    /// `slot_assembler` places it once the tree commits. See
    /// [`Self::child_slot`] for a slot that is a child of this node.
    pub fn slot<V: IntoView>(
        mut self,
        view: V,
        write: impl FnOnce(C, StableNodeId) -> C + 'static,
    ) -> Self {
        self.slots.push(Slot {
            view: view.into_any(),
            write: Box::new(write),
            child: false,
        });
        self
    }

    /// Like [`Self::slot`], for a slot that is this node's own child (a
    /// section's header, a list item's leading icon): its root is inserted
    /// here, in slot order, before [`Self::children`].
    pub fn child_slot<V: IntoView>(
        mut self,
        view: V,
        write: impl FnOnce(C, StableNodeId) -> C + 'static,
    ) -> Self {
        self.slots.push(Slot {
            view: view.into_any(),
            write: Box::new(write),
            child: true,
        });
        self
    }

    /// When a binding changes one of these properties, play from the value
    /// shown to the new one instead of jumping (CSS `transition`). Runs on
    /// the compositor track; the logical style takes the new value at once.
    pub fn animate(mut self, implicit: impl IntoIterator<Item = super::Implicit>) -> Self {
        self.implicit.extend(implicit);
        self
    }
}

impl<C: ComponentView, K> El<C, K> {
    /// Build this element's own node: its slots, classes, bindings and
    /// events. Returns the entity, the slot roots to adopt as its children
    /// (with the keys they were declared with), and the children to build
    /// under it; `None` when the build failed.
    #[allow(clippy::type_complexity)]
    fn place_node(
        self,
        vb: &mut ViewBuilder<'_, '_, '_>,
    ) -> Option<(Entity<C>, Vec<(StableNodeId, Option<Cow<'static, str>>)>, K)> {
        let El {
            mut component,
            key,
            node_refs,
            mut bindings,
            events,
            mut implicit,
            slots,
            classes,
            children,
            site,
            source,
        } = self;
        if let Some(classes) = classes {
            implicit.extend_from_slice(classes.apply(&mut component, &mut bindings));
        }
        let mut adopt = Vec::new();
        for slot in slots {
            match &mut vb.build_slot(slot.view)[..] {
                [(root, key)] => {
                    component = (slot.write)(component, *root);
                    if slot.child {
                        adopt.push((*root, key.take()));
                    }
                }
                // A failed build already recorded its error.
                [] if vb.ui.failed() => {}
                _ => {
                    vb.ui.fail::<C>(FrameworkError::InvalidInput);
                }
            }
        }
        // The first run is the initial value: evaluate under the node's own
        // effect so what it reads becomes its dependencies.
        let effect = (!bindings.is_empty()).then(|| {
            let effect =
                reactive::create_effect(vb.st.tag, EffectTarget::Node(UNBUILT), None, site);
            reactive::run_tracked(effect, || bindings.apply(&mut component));
            effect
        });
        let entity = vb.place(key, component);
        let id = entity.stable_id();
        if id == UNBUILT {
            if let Some(effect) = effect {
                reactive::dispose_effect(effect);
            }
            return None;
        }
        for node_ref in node_refs {
            node_ref.set(Some(id));
        }
        if let Some(effect) = effect {
            reactive::set_effect_target(effect, EffectTarget::Node(id));
            vb.st
                .parts
                .nodes
                .push((id, effect, Box::new(bindings), source));
        }
        if !implicit.is_empty() {
            vb.st.parts.implicit.push((id, implicit.into_boxed_slice()));
        }
        for install in events {
            install(vb.ui, entity);
        }
        Some((entity, adopt, *children))
    }
}

impl<C: ComponentView> El<C> {
    /// Build this element as the container of a structural view, which
    /// builds the children under it itself: the node with its classes,
    /// bindings and events, and no slots. `None` when the build failed.
    pub(crate) fn place(self, vb: &mut ViewBuilder<'_, '_, '_>) -> Option<Entity<C>> {
        debug_assert!(
            self.slots.is_empty()
                && C::BEHAVIOR.slot_assembler.is_none()
                && C::BEHAVIOR.assembler.is_none(),
            "a structural container takes no slots and assembles nothing"
        );
        self.place_node(vb).map(|(entity, _, ())| entity)
    }
}

impl<C: ComponentView, K: IntoView> IntoView for El<C, K> {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let Some((entity, adopt, children)) = self.place_node(vb) else {
            return;
        };
        let id = entity.stable_id();
        vb.nest(entity, |vb| {
            for (root, key) in adopt {
                vb.ui.adopt_as(root, TypeId::of::<AnyView>(), key);
            }
            children.build(vb)
        });
        // After its children, whose entries went in above, and after its
        // slots, built before it: a composite assembles once everything it
        // takes is assembled, so its own patches to that content stand.
        if C::BEHAVIOR.slot_assembler.is_some() || C::BEHAVIOR.assembler.is_some() {
            vb.st.parts.assemble.push((id, TypeId::of::<C>()));
        }
    }
}
