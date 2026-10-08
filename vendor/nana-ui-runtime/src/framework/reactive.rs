//! The [`AppContext`] side of the declarative view layer: mounting, the
//! binding flush, and disposal with the nodes that own reactive state.

use super::*;
use crate::view::reactive::{self as rx, EffectKey, EffectTarget, ScopeKey};
use crate::view::{Mount, NodePatch, Refs, StructuralBinding, ViewBuilder, ViewParts, ViewState};
use crate::{AnimatableProperty, AnimationFillMode, AnimationId, Easing, MotionValue};

/// Rounds a flush runs before it gives up on effects that keep re-queueing
/// each other.
const MAX_ROUNDS: usize = 64;

struct NodeEntry {
    effect: EffectKey,
    patch: Box<dyn NodePatch>,
    source: Option<&'static crate::view::ViewSource>,
}

struct StructuralEntry {
    effect: EffectKey,
    binding: Box<dyn StructuralBinding>,
}

pub(crate) struct ReactiveHost {
    tag: u64,
    nodes: HashMap<StableNodeId, NodeEntry, crate::BuildIdHasher>,
    structural: HashMap<StableNodeId, StructuralEntry, crate::BuildIdHasher>,
    anchors: HashMap<StableNodeId, ScopeKey, crate::BuildIdHasher>,
    queue: Vec<(EffectKey, EffectTarget)>,
    patches: Vec<(StableNodeId, EffectKey)>,
    flushing: bool,
    /// Removed rows and branches playing their leave, despawned when it
    /// ends.
    leaving: HashMap<StableNodeId, Leaving, crate::BuildIdHasher>,
    /// Rows to slide from where they were once layout has placed them.
    flips: Vec<PendingFlip>,
    /// Properties of a node that animate when its bindings change them.
    implicit: HashMap<StableNodeId, Box<[crate::view::Implicit]>, crate::BuildIdHasher>,
}

struct Leaving {
    animation: AnimationId,
    moves: Option<(Duration, Easing)>,
}

struct PendingFlip {
    node: StableNodeId,
    first: nana_ui_core::FlipRect,
    duration: Duration,
    easing: Easing,
}

impl Default for ReactiveHost {
    fn default() -> Self {
        Self {
            tag: rx::next_context_tag(),
            nodes: HashMap::default(),
            structural: HashMap::default(),
            anchors: HashMap::default(),
            queue: Vec::new(),
            patches: Vec::new(),
            flushing: false,
            leaving: HashMap::default(),
            flips: Vec::new(),
            implicit: HashMap::default(),
        }
    }
}

impl Drop for ReactiveHost {
    fn drop(&mut self) {
        let effects = self
            .nodes
            .values()
            .map(|entry| entry.effect)
            .chain(self.structural.values().map(|entry| entry.effect))
            .collect();
        let scopes = self.anchors.values().copied().collect();
        rx::release_on_drop(effects, scopes);
    }
}

/// A view mounted with [`AppContext::mount_view`]. Dropping the handle keeps
/// the view; [`Self::unmount`] or despawning its roots removes it.
#[derive(Debug, Clone)]
pub struct MountedView {
    roots: Vec<StableNodeId>,
    scope: ScopeKey,
}

impl MountedView {
    pub fn roots(&self) -> &[StableNodeId] {
        &self.roots
    }

    /// The first root as an entity of type `C`, for a view with one root
    /// the caller knows the type of.
    pub fn root<C: View>(&self) -> Option<Entity<C>> {
        self.roots.first().copied().map(Entity::from_stable_id)
    }

    /// Dispose every signal and effect the view created and despawn it.
    pub fn unmount(self, cx: &mut AppContext) -> Result<(), FrameworkError> {
        rx::dispose_scope(self.scope);
        let mut mutations = MutationQueue::new();
        for root in self.roots {
            if cx.world.contains(root) {
                mutations.despawn_subtree(root);
            }
        }
        if mutations.is_empty() {
            return Ok(());
        }
        cx.commit_mutations(mutations).map(|_| ())
    }
}

/// The retained view of a bound node, for field-level comparison.
pub(crate) fn bound_view<C: ComponentView>(
    cx: &AppContext,
    id: StableNodeId,
) -> Result<&C, FrameworkError> {
    cx.views
        .get(&id)
        .ok_or(FrameworkError::MissingView(id))?
        .downcast_ref::<C>()
        .ok_or(FrameworkError::ViewType(id))
}

/// Apply a node's bindings to a staged copy of its view and project it.
/// `None` when the bound fields came out equal.
pub(crate) fn stage_bound_node<C: ComponentView>(
    cx: &AppContext,
    id: StableNodeId,
    mutations: &mut MutationQueue,
    apply: impl FnOnce(&mut C),
) -> Result<Option<Box<dyn Any + Send>>, FrameworkError> {
    let current = cx
        .views
        .get(&id)
        .ok_or(FrameworkError::MissingView(id))?
        .downcast_ref::<C>()
        .ok_or(FrameworkError::ViewType(id))?;
    let mut staged = current.clone();
    apply(&mut staged);
    cx.inherit_segmented_option_surface(id, &mut staged);
    if staged == *current {
        if !C::ALWAYS_REPROJECT {
            return Ok(None);
        }
        let mut probe = MutationQueue::new();
        staged.project(id, &cx.world, &mut probe);
        if cx.world.is_noop_batch(&probe) {
            return Ok(None);
        }
        mutations.append(probe);
        return Ok(Some(Box::new(staged)));
    }
    staged.project(id, &cx.world, mutations);
    Ok(Some(Box::new(staged)))
}

impl AppContext {
    /// Build `view` under `parent` in one commit and keep its bindings live.
    ///
    /// `view` runs inside the mount's scope, so the signals it and the
    /// components it calls create are disposed with the mount.
    pub fn mount_view<M: Mount>(
        &mut self,
        parent: StableNodeId,
        view: impl FnOnce() -> M,
    ) -> Result<M::Output, FrameworkError> {
        let document = self
            .world
            .node(parent)
            .ok_or(FrameworkError::MissingView(parent))?
            .document;
        self.mount_view_in(document, Some(parent), view, true)
    }

    /// Like [`Self::mount_view`], with the view's roots as document roots.
    pub fn mount_view_root<M: Mount>(
        &mut self,
        document: DocumentId,
        view: impl FnOnce() -> M,
    ) -> Result<M::Output, FrameworkError> {
        self.mount_view_in(document, None, view, false)
    }

    /// Like [`Self::mount_view_root`], with the roots parked: in no tree
    /// until something places them (a composite's slot given by id, a
    /// later `reconcile_children`).
    pub fn mount_view_detached<M: Mount>(
        &mut self,
        document: DocumentId,
        view: impl FnOnce() -> M,
    ) -> Result<M::Output, FrameworkError> {
        self.mount_view_in(document, None, view, true)
    }

    fn mount_view_in<M: Mount>(
        &mut self,
        document: DocumentId,
        parent: Option<StableNodeId>,
        view: impl FnOnce() -> M,
        park: bool,
    ) -> Result<M::Output, FrameworkError> {
        let tag = self.reactive.tag;
        let scope = rx::create_scope(rx::current_scope());
        let mut refs = None;
        let build = |ui: &mut UiBuilder<'_>| {
            let mut st = ViewState::new(tag);
            let roots = rx::with_scope(scope, || {
                let (view, made) = view().split();
                refs = Some(made);
                ViewBuilder { ui, st: &mut st }.build_collect(view)
            });
            (roots, st.parts)
        };
        // Under a parent the roots are inserted unkeyed afterwards, so the
        // parent's own keyed children are not reassembled.
        let built = if park {
            self.build_detached(document, build)
        } else {
            self.build(document, build)
        };
        let (roots, parts) = match built {
            Ok(built) => built,
            Err(error) => {
                rx::dispose_scope(scope);
                return Err(error);
            }
        };
        if let Some(parent) = parent {
            let mut insert = MutationQueue::new();
            for root in &roots {
                insert.insert(parent, *root, None);
            }
            if let Err(error) = self.commit_mutations(insert) {
                rx::dispose_scope(scope);
                let mut cleanup = MutationQueue::new();
                for root in &roots {
                    cleanup.despawn_subtree(*root);
                }
                self.commit_mutations(cleanup)?;
                return Err(error);
            }
        }
        if let Err(error) = self.install_view_parts(parts) {
            // The tree stands; what failed is placing its slots.
            let mounted = MountedView { roots, scope };
            mounted.unmount(self)?;
            return Err(error);
        }
        for root in &roots {
            self.reactive.anchors.insert(*root, scope);
        }
        rx::run_mounted(self);
        let mounted = MountedView { roots, scope };
        // Every ref names an element that was built, or the caller asked
        // for a node that is not there.
        match refs.and_then(Refs::resolve) {
            Some(refs) => Ok(M::finish(mounted, refs)),
            None => {
                mounted.unmount(self)?;
                Err(FrameworkError::InvalidInput)
            }
        }
    }

    pub(crate) fn reactive_tag(&self) -> u64 {
        self.reactive.tag
    }

    /// Take over what a build left and assemble the slots of the composites
    /// it built, innermost first.
    pub(crate) fn install_view_parts(&mut self, parts: ViewParts) -> Result<(), FrameworkError> {
        for (id, effect, patch, source) in parts.nodes {
            if !self.world.contains(id) {
                rx::dispose_effect(effect);
                continue;
            }
            if let Some(old) = self.reactive.nodes.insert(
                id,
                NodeEntry {
                    effect,
                    patch,
                    source,
                },
            ) {
                rx::dispose_effect(old.effect);
            }
        }
        for (id, effect, binding) in parts.structural {
            if !self.world.contains(id) {
                rx::dispose_effect(effect);
                continue;
            }
            if let Some(old) = self
                .reactive
                .structural
                .insert(id, StructuralEntry { effect, binding })
            {
                rx::dispose_effect(old.effect);
            }
        }
        for (id, implicit) in parts.implicit {
            if self.world.contains(id) {
                self.reactive.implicit.insert(id, implicit);
            }
        }
        for (id, scope) in parts.anchors {
            if let Some(old) = self.reactive.anchors.insert(id, scope)
                && old != scope
            {
                rx::dispose_scope(old);
            }
        }
        for (id, type_id) in parts.assemble {
            self.run_built_assembler(id, type_id)?;
        }
        Ok(())
    }

    /// Whether a signal write left bindings of this context to apply.
    pub fn has_pending_reactive(&self) -> bool {
        rx::has_pending(self.reactive.tag)
    }

    /// Apply every binding whose signals changed: run watchers, rebuild
    /// keyed lists and conditional blocks, then stage each changed node once
    /// and commit them together. Input routing and
    /// [`Self::take_system_work`] call this; call it yourself after writing
    /// signals outside both.
    pub fn flush_reactive(&mut self) -> Result<(), FrameworkError> {
        rx::advance_epoch();
        if self.reactive.flushing || !rx::has_pending(self.reactive.tag) {
            return Ok(());
        }
        self.reactive.flushing = true;
        let started = Instant::now();
        let outcome = self.flush_reactive_rounds();
        self.reactive.flushing = false;
        nana_diagnostics::metric!(nana_diagnostics::framework::runtime::REACTIVE_FLUSHES);
        nana_diagnostics::metric!(
            nana_diagnostics::framework::runtime::REACTIVE_FLUSH_NS,
            started.elapsed()
        );
        outcome
    }

    fn flush_reactive_rounds(&mut self) -> Result<(), FrameworkError> {
        let tag = self.reactive.tag;
        let mut queue = std::mem::take(&mut self.reactive.queue);
        let mut patches = std::mem::take(&mut self.reactive.patches);
        let mut outcome = Ok(());
        let (mut patched, mut commits) = (0, 0);
        for _ in 0..MAX_ROUNDS {
            queue.clear();
            rx::take_pending(tag, &mut queue);
            if queue.is_empty() {
                break;
            }
            // An effect reached only through computeds that recomputed to
            // the same value has nothing to do.
            queue.retain(|&(effect, _)| rx::confirm(effect));
            #[cfg(feature = "reactive-trace")]
            let round = rx::with_trace(|trace| trace.begin_round());
            #[cfg(not(feature = "reactive-trace"))]
            let round = 0;
            for &(effect, target) in &queue {
                if target == EffectTarget::User {
                    rx::run_user_effect(effect);
                }
            }
            for &(effect, target) in &queue {
                if let EffectTarget::Structural(node) = target {
                    outcome = outcome.and(self.run_structural(node, effect));
                }
            }
            // Rows and branches are placed now.
            rx::run_mounted(self);
            patches.clear();
            patches.extend(queue.iter().filter_map(|&(effect, target)| match target {
                EffectTarget::Node(node) => Some((node, effect)),
                _ => None,
            }));
            if !patches.is_empty() {
                match self.patch_bound_nodes(&patches, round) {
                    Ok((nodes, commit)) => {
                        patched += nodes;
                        commits += commit;
                    }
                    Err(error) => outcome = outcome.and(Err(error)),
                }
            }
        }
        if rx::has_pending(tag) {
            nana_diagnostics::fault!(
                nana_diagnostics::framework::runtime::REACTIVE_DID_NOT_SETTLE,
                rounds = MAX_ROUNDS as u64
            );
            rx::drop_pending(tag);
        }
        rx::record_flush(patched, commits);
        self.reactive.queue = queue;
        self.reactive.patches = patches;
        outcome
    }

    fn run_structural(
        &mut self,
        node: StableNodeId,
        effect: EffectKey,
    ) -> Result<(), FrameworkError> {
        let Some(mut entry) = self.reactive.structural.remove(&node) else {
            return Ok(());
        };
        if entry.effect != effect || !self.world.contains(node) {
            self.keep_structural(node, entry);
            return Ok(());
        }
        let outcome = entry.binding.update(self, node, effect);
        self.keep_structural(node, entry);
        outcome
    }

    fn keep_structural(&mut self, node: StableNodeId, entry: StructuralEntry) {
        if self.world.contains(node) {
            self.reactive.structural.insert(node, entry);
        } else {
            rx::dispose_effect(entry.effect);
        }
    }

    /// Stage every queued node against the current world, commit the lot
    /// once, then install the staged views and follow their lifecycles.
    fn patch_bound_nodes(
        &mut self,
        patches: &[(StableNodeId, EffectKey)],
        #[allow(unused_variables)] round: u64,
    ) -> Result<(u64, u64), FrameworkError> {
        let mut mutations = MutationQueue::new();
        let mut staged: Vec<(StableNodeId, Box<dyn Any + Send>)> = Vec::new();
        let mut outcome = Ok(());
        for &(node, effect) in patches {
            let Some(mut entry) = self.reactive.nodes.remove(&node) else {
                continue;
            };
            if entry.effect != effect || !self.world.contains(node) {
                if self.world.contains(node) {
                    self.reactive.nodes.insert(node, entry);
                }
                continue;
            }
            let result = rx::run_tracked(effect, || entry.patch.stage(self, node, &mut mutations));
            self.reactive.nodes.insert(node, entry);
            match result {
                Ok(Some(view)) => {
                    #[cfg(feature = "reactive-trace")]
                    rx::with_trace(|trace| trace.patch(round, node, effect));
                    staged.push((node, view));
                }
                Ok(None) => {}
                Err(error) => outcome = outcome.and(Err(error)),
            }
        }
        let mut commits = 0;
        // What animated nodes show now, and hold logically, before the
        // commit moves their logical values.
        let now = self.component_lifecycle.now;
        let mut before = Vec::new();
        for (node, _) in &staged {
            let Some(implicit) = self.reactive.implicit.get(node) else {
                continue;
            };
            for implicit in implicit.iter() {
                let Some(logical) = self.world.logical_motion_value(*node, implicit.property)
                else {
                    continue;
                };
                let shown = self
                    .world
                    .presentation_motion_value(*node, implicit.property, now)
                    .unwrap_or(logical);
                before.push((*node, *implicit, logical, shown));
            }
        }
        if !mutations.is_empty() {
            self.commit_mutations(mutations)?;
            commits = 1;
            nana_diagnostics::metric!(nana_diagnostics::framework::runtime::REACTIVE_COMMITS);
        }
        self.play_implicit(before, now)?;
        let patched = staged.len() as u64;
        if patched > 0 {
            nana_diagnostics::metric!(
                nana_diagnostics::framework::runtime::REACTIVE_NODES_PATCHED,
                patched
            );
        }
        let mut changed = Vec::with_capacity(staged.len());
        for (node, view) in staged {
            changed.push((node, Any::type_id(&*view)));
            self.views.insert(node, view);
        }
        for (node, type_id) in changed {
            if !self.world.contains(node) {
                continue;
            }
            if !self.world.is_mounted(node) {
                outcome = outcome.and(self.suspend(node));
            }
            outcome = outcome.and(self.sync_component_lifecycle(node));
            outcome = outcome.and(self.run_component_assembler(node, type_id));
            outcome = outcome.and(self.run_slot_assembler(node, type_id));
        }
        outcome.map(|()| (patched, commits))
    }

    /// Release the reactive state owned by despawned nodes.
    pub(crate) fn forget_reactive(&mut self, removed: &HashSet<StableNodeId>) {
        let host = &mut self.reactive;
        if host.nodes.is_empty()
            && host.structural.is_empty()
            && host.anchors.is_empty()
            && host.leaving.is_empty()
            && host.implicit.is_empty()
        {
            return;
        }
        for id in removed {
            host.leaving.remove(id);
            host.implicit.remove(id);
            if let Some(entry) = host.nodes.remove(id) {
                rx::dispose_effect(entry.effect);
            }
            if let Some(entry) = host.structural.remove(id) {
                rx::dispose_effect(entry.effect);
            }
            if let Some(scope) = host.anchors.remove(id) {
                rx::dispose_scope(scope);
            }
        }
    }

    /// The bindings `node` carries: where its element was declared and each
    /// bound field with where its binding was declared. `None` for a node
    /// without bindings.
    pub fn view_bindings(&self, node: StableNodeId) -> Option<crate::view::NodeBindingInfo> {
        let entry = self.reactive.nodes.get(&node)?;
        let fields = entry.patch.fields();
        let source_fields = entry.source.map_or_else(Vec::new, |source| {
            fields
                .iter()
                .filter_map(|(field, _)| {
                    source
                        .fields
                        .iter()
                        .find(|(name, _)| {
                            *name == *field || field.rsplit('.').next() == Some(*name)
                        })
                        .map(|(_, at)| (*field, *at))
                })
                .collect()
        });
        Some(crate::view::NodeBindingInfo {
            element: rx::effect_site(entry.effect)?,
            fields,
            source: entry.source.map(|source| source.element),
            source_fields,
        })
    }

    pub(crate) fn view_is<C: 'static>(&self, node: StableNodeId) -> bool {
        self.views.get(&node).is_some_and(|view| view.is::<C>())
    }

    /// `node` as the view layer sees it: its control's fields and values
    /// and where its bindings were declared. `None` for a node that does not
    /// exist.
    pub fn inspect(&self, node: StableNodeId) -> Option<crate::view::Inspection> {
        if !self.world.contains(node) {
            return None;
        }
        let bindings = self.view_bindings(node);
        let bound = |name: &str| {
            bindings.as_ref().and_then(|info| {
                info.fields
                    .iter()
                    .find(|(field, _)| field.rsplit('.').next() == Some(name))
                    .map(|(_, at)| *at)
            })
        };
        let source_bound = |name: &str| {
            bindings.as_ref().and_then(|info| {
                info.source_fields
                    .iter()
                    .find(|(field, _)| field.rsplit('.').next() == Some(name))
                    .map(|(_, at)| *at)
            })
        };
        let (control, fields) = self
            .views
            .get(&node)
            .and_then(|view| crate::view::inspect_control(&**view))
            .map_or((None, Vec::new()), |(tag, fields)| (Some(tag), fields));
        Some(crate::view::Inspection {
            node,
            control,
            fields: fields
                .into_iter()
                .map(|(name, value)| crate::view::InspectedField {
                    name,
                    value,
                    bound_at: bound(name),
                    source_bound_at: source_bound(name),
                })
                .collect(),
            element: bindings.as_ref().map(|info| info.element),
            source_element: bindings.as_ref().and_then(|info| info.source),
        })
    }

    /// Set `field` of the built-in control at `node` from text, the way its
    /// binding would (devtools editing). Strings are taken as they are,
    /// numbers and `true` / `false` parsed; empty text clears an optional
    /// field. A bound field returns to its binding's value when that runs.
    pub fn set_field(&mut self, node: StableNodeId, field: &str, text: &str) -> Result<(), String> {
        crate::view::edit_control(self, node, field, text)
            .unwrap_or_else(|| Err(format!("node {} is not a built-in control", node.get())))
    }

    /// Why `node` was last patched: its element, its bound fields, and the
    /// signal writes that queued it. `None` when the trace holds no patch of
    /// it (it has no bindings, never changed, or the ring moved on).
    #[cfg(feature = "reactive-trace")]
    pub fn why_updated(&self, node: StableNodeId) -> Option<crate::view::WhyUpdated> {
        let causes = rx::with_trace(|trace| trace.why(node))?;
        let info = self.view_bindings(node)?;
        Some(crate::view::WhyUpdated {
            node,
            element: info.element,
            bindings: info.fields,
            source_element: info.source,
            source_bindings: info.source_fields,
            causes,
        })
    }
}

/// Enter, leave and move animations of rows and branches
/// ([`crate::view::Transition`]).
impl AppContext {
    fn flip_rect(&self, id: StableNodeId) -> Option<nana_ui_core::FlipRect> {
        let bounds = self.world.layout_box(id)?;
        Some(nana_ui_core::FlipRect::new(
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
        ))
    }

    /// Play `enter` on each of `roots`, just placed: from its opacity and
    /// transform to the node's own.
    pub(crate) fn begin_enter(
        &mut self,
        roots: &[StableNodeId],
        enter: &crate::view::Presence,
    ) -> Result<(), FrameworkError> {
        if !enter.plays() {
            return Ok(());
        }
        let now = self.component_lifecycle.now;
        let mut mutations = MutationQueue::new();
        for &root in roots {
            for (property, from) in presence_values(enter) {
                let Some(to) = self.world.logical_motion_value(root, property) else {
                    continue;
                };
                mutations.start_animation(crate::motion_api::presence_spec(
                    root,
                    property,
                    from,
                    to,
                    now,
                    enter.duration,
                    enter.easing,
                    AnimationFillMode::Backwards,
                ));
            }
        }
        if mutations.is_empty() {
            return Ok(());
        }
        self.commit_mutations(mutations).map(|_| ())
    }

    /// Start `leave` on `root` instead of despawning it: it stays in place,
    /// out of hit testing and focus, until the leave ends. `false` when
    /// nothing plays, and the caller despawns it now.
    pub(crate) fn begin_leave(
        &mut self,
        root: StableNodeId,
        leave: &crate::view::Presence,
        moves: Option<(Duration, Easing)>,
    ) -> Result<bool, FrameworkError> {
        let Some(node) = self.world.node(root) else {
            return Ok(false);
        };
        if !leave.plays() || self.reactive.leaving.contains_key(&root) {
            return Ok(self.reactive.leaving.contains_key(&root));
        }
        let document = node.document;
        let now = self.component_lifecycle.now;
        let mut mutations = MutationQueue::new();
        let mut last = None;
        for (property, to) in presence_values(leave) {
            let Some(from) = self.world.logical_motion_value(root, property) else {
                continue;
            };
            let spec = crate::motion_api::presence_spec(
                root,
                property,
                from,
                to,
                now,
                leave.duration,
                leave.easing,
                AnimationFillMode::Forwards,
            );
            last = Some(spec.id);
            mutations.start_animation(spec);
        }
        let Some(animation) = last else {
            return Ok(false);
        };
        if let Some(mut style) = self.world.node_style(root).cloned() {
            Arc::make_mut(&mut style.layout).pointer_events =
                Some(nana_ui_core::PointerEventsSpec::None);
            mutations.set_style(root, style);
        }
        if self
            .world
            .focused(document)
            .is_some_and(|focused| self.world.is_descendant_or_self(focused, root))
        {
            mutations.request_focus(document, None);
        }
        self.commit_mutations(mutations)?;
        self.reactive
            .leaving
            .insert(root, Leaving { animation, moves });
        Ok(true)
    }

    /// Whether `root` is playing its leave.
    pub(crate) fn is_leaving(&self, root: StableNodeId) -> bool {
        self.reactive.leaving.contains_key(&root)
    }

    /// Slide `node` from `first` to wherever the next layout puts it.
    pub(crate) fn flip_after_layout(
        &mut self,
        node: StableNodeId,
        first: nana_ui_core::FlipRect,
        (duration, easing): (Duration, Easing),
    ) {
        self.reactive.flips.push(PendingFlip {
            node,
            first,
            duration,
            easing,
        });
    }

    /// Animate each property whose logical value the commit changed, from
    /// what was shown to the new value.
    fn play_implicit(
        &mut self,
        before: Vec<(
            StableNodeId,
            crate::view::Implicit,
            MotionValue,
            MotionValue,
        )>,
        now: Duration,
    ) -> Result<(), FrameworkError> {
        if before.is_empty() {
            return Ok(());
        }
        let mut mutations = MutationQueue::new();
        for (node, implicit, logical, shown) in before {
            let Some(next) = self.world.logical_motion_value(node, implicit.property) else {
                continue;
            };
            if next == logical || implicit.duration.is_zero() {
                continue;
            }
            mutations.start_animation(crate::motion_api::presence_spec(
                node,
                implicit.property,
                shown,
                next,
                now,
                implicit.duration,
                implicit.easing,
                AnimationFillMode::None,
            ));
        }
        if mutations.is_empty() {
            return Ok(());
        }
        self.commit_mutations(mutations).map(|_| ())
    }

    /// Where `node` is now, for [`Self::flip_after_layout`].
    pub(crate) fn flip_first(&self, node: StableNodeId) -> Option<nana_ui_core::FlipRect> {
        self.flip_rect(node)
    }

    /// Despawn the rows whose leave ended (or was cut short), sliding their
    /// siblings into the space they leave when the transition moves rows.
    pub(crate) fn finish_leaves(&mut self, events: &[crate::AnimationEvent]) {
        if self.reactive.leaving.is_empty() {
            return;
        }
        let done: Vec<StableNodeId> = events
            .iter()
            .filter(|event| {
                self.reactive
                    .leaving
                    .get(&event.target)
                    .is_some_and(|leaving| leaving.animation == event.id)
            })
            .map(|event| event.target)
            .collect();
        if done.is_empty() {
            return;
        }
        let mut mutations = MutationQueue::new();
        for root in done {
            let Some(leaving) = self.reactive.leaving.remove(&root) else {
                continue;
            };
            if !self.world.contains(root) {
                continue;
            }
            if let Some(moves) = leaving.moves
                && let Some(parent) = self.world.node(root).and_then(|node| node.parent)
            {
                let siblings = self
                    .world
                    .node(parent)
                    .map(|node| node.children.to_vec())
                    .unwrap_or_default();
                for sibling in siblings {
                    if sibling != root
                        && !self.reactive.leaving.contains_key(&sibling)
                        && let Some(first) = self.flip_rect(sibling)
                    {
                        self.flip_after_layout(sibling, first, moves);
                    }
                }
            }
            mutations.despawn_subtree(root);
        }
        if !mutations.is_empty() && self.commit_mutations(mutations).is_err() {
            nana_diagnostics::metric!(nana_diagnostics::framework::runtime::FLUSH_FAILED);
        }
    }

    /// After a layout pass of `document`: start the slides of rows it moved.
    pub(crate) fn play_pending_flips(&mut self, document: DocumentId) {
        if self.reactive.flips.is_empty() {
            return;
        }
        let now = self.component_lifecycle.now;
        let mut mutations = MutationQueue::new();
        let flips = std::mem::take(&mut self.reactive.flips);
        let mut kept = Vec::new();
        for flip in flips {
            let Some(node) = self.world.node(flip.node) else {
                continue;
            };
            if node.document != document {
                kept.push(flip);
                continue;
            }
            let Some(last) = self.flip_rect(flip.node) else {
                continue;
            };
            let moved = (flip.first.x - last.x).abs() > 0.5 || (flip.first.y - last.y).abs() > 0.5;
            if moved {
                mutations.start_layout_flip(
                    flip.node,
                    flip.first,
                    last,
                    now,
                    flip.duration,
                    flip.easing,
                );
            }
        }
        self.reactive.flips = kept;
        if !mutations.is_empty() && self.commit_mutations(mutations).is_err() {
            nana_diagnostics::metric!(nana_diagnostics::framework::runtime::FLUSH_FAILED);
        }
    }
}

/// The properties a presence animates and its value for each.
fn presence_values(
    presence: &crate::view::Presence,
) -> impl Iterator<Item = (AnimatableProperty, MotionValue)> {
    presence
        .opacity
        .map(|opacity| (AnimatableProperty::Opacity, MotionValue::Scalar(opacity)))
        .into_iter()
        .chain(presence.transform.map(|transform| {
            (
                AnimatableProperty::Transform,
                MotionValue::Transform(transform),
            )
        }))
}

#[cfg(test)]
impl AppContext {
    pub(crate) fn event_handler_count(&self) -> usize {
        self.event_handlers.values().map(Vec::len).sum()
    }
}
