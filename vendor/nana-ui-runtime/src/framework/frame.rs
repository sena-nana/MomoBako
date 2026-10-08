//! AppContext frame operations.

use super::*;

impl AppContext {
    /// Drain deterministic work scheduled since the previous frame.
    pub fn take_system_work(&mut self) -> crate::SystemWork {
        // Signal writes outside input (timers, program updates, other
        // contexts) land here, before the frame reads the world.
        crate::view::poll_tasks();
        if self.flush_reactive().is_err() {
            nana_diagnostics::metric!(nana_diagnostics::framework::runtime::FLUSH_FAILED);
        }
        self.world.take_system_work()
    }

    /// Consume the host cursor-sync hint after a successful document frame.
    #[doc(hidden)]
    pub fn take_window_cursor_dirty(&mut self) -> bool {
        self.world.take_window_cursor_dirty()
    }

    /// Algorithm-level counters from the last drained system batch.
    pub fn last_work_counters(&self) -> crate::WorkCounters {
        self.world.last_work_counters()
    }

    /// Record extract output onto the last drained work counters.
    pub fn record_extract(&mut self, extracted: &[crate::ExtractedNode]) {
        self.world.record_extract(extracted);
    }

    /// Open a product-frame profiler and work-counter accumulator.
    pub fn begin_frame_profile(&mut self) {
        self.frame_profiler = FrameProfiler::new();
        self.frame_profiler.mark_runtime_unsupported();
        self.profiling = true;
        self.world.begin_frame_counters();
    }

    pub fn finish_frame_profile(&mut self) {
        self.world.end_frame_counters();
        self.profiling = false;
        let profile = std::mem::take(&mut self.frame_profiler).finish();
        // Match last_work_counters: an idle flush (no stage ran) must not wipe
        // the last non-empty product profile.
        if profile.any_stage_ran() {
            record_frame_diagnostics(&profile);
            self.last_profile = profile;
        }
    }

    pub fn last_frame_profile(&self) -> &FrameProfile {
        &self.last_profile
    }

    pub(super) fn stage_clock(&self) -> Option<Instant> {
        self.profiling.then(Instant::now)
    }

    pub(super) fn record_stage(&mut self, stage: FrameStage, started: Option<Instant>) {
        if let Some(started) = started {
            self.frame_profiler.record(stage, started.elapsed());
        }
    }

    /// Record a stage duration while a product frame is open.
    pub fn time_stage_duration(&mut self, stage: FrameStage, duration: Duration) {
        if self.profiling {
            self.frame_profiler.record(stage, duration);
        }
    }

    /// Return a drained system batch to the scheduler after a canonical frame
    /// fails. Frame drivers should restore every consumed batch before retry.
    pub fn restore_system_work(&mut self, work: crate::SystemWork) {
        self.world.restore_system_work(work);
    }

    /// Resolve inherited style for the supplied dirty nodes.
    pub fn resolve_styles(&mut self, ids: &[StableNodeId]) -> Result<(), FrameworkError> {
        let started = self.stage_clock();
        let previous_focus = ids
            .iter()
            .filter_map(|id| self.world.document_of(*id))
            .filter_map(|document| self.world.focused(document).map(|node| (document, node)))
            .collect::<HashMap<_, _>>();
        let result = self.world.resolve_styles(ids).map_err(FrameworkError::from);
        if result.is_ok() {
            for (document, previous) in previous_focus {
                self.seal_blurred_editor_history(document, Some(previous));
            }
        }
        self.record_stage(FrameStage::Style, started);
        result
    }

    /// Derive registered text presentations for scheduled nodes.
    pub fn resolve_presentations(&mut self, ids: &[StableNodeId]) -> Result<(), FrameworkError> {
        self.world
            .resolve_presentations(ids)
            .map_err(FrameworkError::from)
    }

    /// Shape only scheduled text through the host's real text backend.
    pub fn shape_text(
        &mut self,
        ids: &[StableNodeId],
        shaper: &mut impl crate::TextShaper,
    ) -> Result<(), FrameworkError> {
        let started = self.stage_clock();
        let result = self
            .world
            .shape_text(ids, shaper)
            .map_err(FrameworkError::from);
        self.record_stage(FrameStage::TextShape, started);
        result?;
        self.reproject_text_backend_views()?;
        // Clamps are announced from the layout-aware passes: before layout
        // a text has no width to clamp against.
        Ok(())
    }

    pub fn shape_text_for_layout(
        &mut self,
        document: DocumentId,
        shaper: &mut impl crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        let started = self.stage_clock();
        let result = self
            .world
            .shape_text_for_layout(document, shaper)
            .map_err(FrameworkError::from);
        self.record_stage(FrameStage::TextShape, started);
        let reshaped = result?;
        let reprojected = self.reproject_text_backend_views()?;
        self.announce_text_clamps()?;
        Ok(reprojected || reshaped)
    }

    /// [`Self::shape_text_for_layout`] restricted to `ids` (the last layout
    /// scope): nodes outside it keep shapes matching their unchanged boxes.
    pub fn shape_text_for_layout_scoped(
        &mut self,
        ids: &[StableNodeId],
        shaper: &mut impl crate::TextShaper,
    ) -> Result<bool, FrameworkError> {
        let started = self.stage_clock();
        let result = self
            .world
            .shape_text_for_layout_scoped(ids, shaper)
            .map_err(FrameworkError::from);
        self.record_stage(FrameStage::TextShape, started);
        let reshaped = result?;
        let reprojected = self.reproject_text_backend_views()?;
        self.announce_text_clamps()?;
        Ok(reprojected || reshaped)
    }

    /// Compute and atomically publish canonical Runtime layout for one window.
    ///
    /// Full pass: recomputes every box and rebuilds the retained layout cache.
    /// Used when viewport semantics changed or a complete layout is required.
    pub fn layout_document(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
    ) -> Result<crate::CommitReport, FrameworkError> {
        self.layout_document_impl(document, viewport, &[], true)
    }

    /// [`Self::layout_document`] restricted to the ancestor closure of
    /// `dirty`. Clean subtrees reuse the retained cache, so the cost scales
    /// with the change, not the document. [`Self::take_last_layout_scope`]
    /// reports the recomputed set for scoped text re-shaping.
    pub fn layout_document_scoped(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
        dirty: &[StableNodeId],
    ) -> Result<crate::CommitReport, FrameworkError> {
        let mut dirty = dirty.to_vec();
        dirty.sort_unstable();
        dirty.dedup();
        self.layout_document_impl(document, viewport, &dirty, false)
    }

    /// Relayout after a viewport change. Document roots plus any live
    /// `position: fixed` / `vw` / `vh` boxes are dirty; unchanged subtrees keep
    /// the retained cache.
    pub fn layout_document_for_viewport(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
    ) -> Result<crate::CommitReport, FrameworkError> {
        let mut dirty = self.world.document_roots(document);
        dirty.extend(self.world.viewport_basis_ids_for(document));
        self.layout_document_impl(document, viewport, &dirty, false)
    }

    /// Nodes recomputed by the most recent layout pass; drains on read.
    pub fn take_last_layout_scope(&mut self) -> Vec<StableNodeId> {
        std::mem::take(&mut self.last_layout_scope)
    }

    /// Nodes carrying an undrained LAYOUT-dirty bit (e.g. set by a shaping
    /// pass after the work drain). Sorted for determinism.
    pub fn pending_layout_dirty(&mut self) -> Vec<StableNodeId> {
        self.world.pending_layout_dirty()
    }

    /// Full (`force_full`) layout passes, which discard the retained cache.
    pub fn layout_full_invocations(&self) -> usize {
        self.layout_full_invocations
    }

    /// Layout passes, scoped and full.
    pub fn layout_invocations(&self) -> usize {
        self.layout_invocations
    }

    pub(super) fn layout_document_impl(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
        dirty: &[StableNodeId],
        force_full: bool,
    ) -> Result<crate::CommitReport, FrameworkError> {
        #[cfg(not(feature = "benchmark"))]
        {
            self.layout_document_observed(document, viewport, dirty, force_full, |_| {})
        }
        // Time the production path itself rather than a benchmark-only
        // reimplementation of it: the sub-stage clocks are the same
        // `completed` hook the full-layout benchmark already uses.
        #[cfg(feature = "benchmark")]
        {
            let mut substages = [Duration::ZERO; 4];
            let mut started = Instant::now();
            let result =
                self.layout_document_observed(document, viewport, dirty, force_full, |stage| {
                    substages[stage] = started.elapsed();
                    started = Instant::now();
                });
            for (total, elapsed) in self.layout_substage_totals.iter_mut().zip(substages) {
                *total += elapsed;
            }
            result
        }
    }

    /// Drain the per-sub-stage totals accumulated inside the Layout stage:
    /// `[position_open_tooltips, engine, writeback + commit, scroll metrics]`.
    #[cfg(feature = "benchmark")]
    pub fn take_layout_substage_totals(&mut self) -> [Duration; 4] {
        std::mem::take(&mut self.layout_substage_totals)
    }

    /// Diagnostic timings for the canonical full layout path. The production
    /// entry points use the same operations with no per-stage clock callbacks.
    #[cfg(feature = "benchmark")]
    pub fn benchmark_layout_document(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
    ) -> Result<[Duration; 4], FrameworkError> {
        let mut timings = [Duration::ZERO; 4];
        let mut started = Instant::now();
        self.layout_document_observed(document, viewport, &[], true, |stage| {
            timings[stage] = started.elapsed();
            started = Instant::now();
        })?;
        Ok(timings)
    }

    fn layout_document_observed(
        &mut self,
        document: DocumentId,
        viewport: crate::LayoutViewport,
        dirty: &[StableNodeId],
        force_full: bool,
        mut completed: impl FnMut(usize),
    ) -> Result<crate::CommitReport, FrameworkError> {
        self.layout_invocations += 1;
        if force_full {
            self.layout_full_invocations += 1;
        }
        let started = self.stage_clock();
        // Diagnostics time layout even outside a profiled frame.
        let diagnostics_clock =
            started.or_else(|| nana_diagnostics::metrics_enabled().then(Instant::now));
        self.world.set_document_viewport(document, viewport);
        let result = (|| {
            self.position_open_tooltips(document)?;
            completed(0);
            let layouts = crate::RuntimeLayoutEngine.layout_document_scoped(
                &self.world,
                document,
                viewport,
                dirty,
                &mut self.layout_cache,
                force_full,
            )?;
            completed(1);
            let mut mutations = MutationQueue::new();
            let mut scope = Vec::with_capacity(layouts.len());
            for (id, layout) in layouts {
                scope.push(id);
                let padding_changed = self
                    .layout_cache
                    .used_padding(document, id)
                    .is_some_and(|padding| self.world.write_layout_padding(id, padding));
                // A turned scroll origin re-measures even when no box moved.
                let far_start_changed = self
                    .layout_cache
                    .far_start(document, id)
                    .is_some_and(|far| self.world.write_layout_far_start(id, far));
                if padding_changed || far_start_changed || self.world.layout_box(id) != Some(layout)
                {
                    mutations.write_layout(id, layout);
                }
            }
            let mut settings_rows = Vec::new();
            let mut terminal_sizes = Vec::new();
            for id in &scope {
                let Some(view) = self.views.get(id) else {
                    continue;
                };
                if view.is::<crate::TerminalView>() {
                    terminal_sizes.push(*id);
                } else if view
                    .downcast_ref::<crate::SettingsRow>()
                    .is_some_and(|row| row.stack_below.is_some() && !row.stacked)
                {
                    settings_rows.push(*id);
                }
            }
            self.last_layout_scope = scope;
            let report = self.commit_mutations(mutations)?;
            for id in settings_rows {
                self.refresh_settings_row_layout(id)?;
            }
            for id in terminal_sizes {
                if let Some(bounds) = self.world.layout_box(id) {
                    self.resize_terminal_view(
                        Entity::from_stable_id(id),
                        bounds.width,
                        bounds.height,
                    )?;
                }
            }
            completed(2);
            self.apply_document_scroll_retention(document, force_full)?;
            self.announce_size_changes(document)?;
            self.play_pending_flips(document);
            completed(3);
            Ok(report)
        })();
        self.record_stage(FrameStage::Layout, started);
        if let Some(clock) = diagnostics_clock {
            use nana_diagnostics::{framework::layout, metric};
            metric!(layout::PASS_NS, clock.elapsed());
            metric!(layout::INVOCATIONS);
            if force_full {
                metric!(layout::FULL_INVOCATIONS);
            }
            metric!(layout::DIRTY_ROOTS, dirty.len());
            if result.is_ok() {
                metric!(layout::BOXES_LAID_OUT, self.last_layout_scope.len());
            }
        }
        result
    }

    /// After a layout pass, whose commit already re-measured the scroll
    /// containers: announce the `ScrollView` offsets it clamped, then apply
    /// follow-end / anchor retention.
    pub(super) fn apply_document_scroll_retention(
        &mut self,
        document: DocumentId,
        force_full: bool,
    ) -> Result<(), FrameworkError> {
        for id in self.world.take_scroll_reclamped() {
            if !self.is_scroll_view(id) {
                continue;
            }
            let offset = self.world.scroll_offset(id).unwrap_or_default();
            self.update(Entity::<ScrollView>::from_stable_id(id), |_, cx| {
                cx.emit(ScrollChanged { offset });
            })?;
        }
        let asked: Vec<_> = self
            .laid_out_notices
            .iter()
            .copied()
            .filter(|id| {
                self.world
                    .node(*id)
                    .is_some_and(|node| node.document == document)
            })
            .collect();
        for id in asked {
            self.laid_out_notices.remove(&id);
            if self.is_scroll_view(id) {
                self.update(Entity::<ScrollView>::from_stable_id(id), |_, cx| {
                    cx.emit(ScrollLaidOut);
                })?;
            }
        }
        for id in self.world.take_scroll_resized() {
            let Some(metrics) = self.world.scroll_metrics(id) else {
                continue;
            };
            if !self.is_scroll_view(id) {
                continue;
            }
            let (width, height) = (metrics.viewport_width, metrics.viewport_height);
            self.update(Entity::<ScrollView>::from_stable_id(id), |_, cx| {
                cx.emit(ScrollViewportChanged { width, height });
            })?;
        }
        let targets = if force_full {
            self.world
                .document_order(document)
                .into_iter()
                .filter(|id| self.is_scroll_view(*id))
                .collect()
        } else {
            self.scoped_scroll_retention_targets(document)
        };
        for id in targets {
            self.apply_scroll_retention(Entity::<ScrollView>::from_stable_id(id))?;
        }
        Ok(())
    }

    pub(super) fn scoped_scroll_retention_targets(
        &self,
        document: DocumentId,
    ) -> Vec<StableNodeId> {
        let mut visited = HashSet::new();
        let mut targets = HashSet::new();
        for &id in &self.last_layout_scope {
            if self.world.document_of(id) != Some(document) {
                continue;
            }
            let mut cursor = Some(id);
            while let Some(id) = cursor {
                if !visited.insert(id) {
                    break;
                }
                if self.is_scroll_view(id) && self.world.presence_live(id) {
                    targets.insert(id);
                }
                cursor = self.world.parent_id(id);
            }
        }
        if targets.len() <= 1 {
            return targets.into_iter().collect();
        }
        // Preserve document order without descending into unrelated subtrees.
        let mut branches = HashSet::new();
        let mut branch_parents = HashSet::new();
        for &id in &targets {
            let mut cursor = Some(id);
            while let Some(id) = cursor {
                if !branches.insert(id) {
                    break;
                }
                cursor = self.world.parent_id(id);
                if let Some(parent) = cursor {
                    branch_parents.insert(parent);
                }
            }
        }
        let mut ordered = Vec::with_capacity(targets.len());
        let mut stack = self.world.document_roots(document);
        stack.reverse();
        while let Some(id) = stack.pop() {
            if !branches.contains(&id) {
                continue;
            }
            if targets.contains(&id) {
                ordered.push(id);
            }
            if ordered.len() == targets.len() {
                break;
            }
            if branch_parents.contains(&id)
                && let Some(node) = self.world.node(id)
            {
                stack.extend(
                    node.children
                        .into_iter()
                        .rev()
                        .filter(|id| branches.contains(id)),
                );
            }
        }
        ordered
    }

    /// Re-queue LAYOUT after a host drained a frame without measuring.
    pub fn defer_layout(&mut self, ids: &[StableNodeId]) {
        for &id in ids {
            self.world.mark_layout(id);
        }
    }

    /// Rebuild the compact hit index for one document after layout or input
    /// work. The retained hierarchy remains private to this context.
    pub fn rebuild_hit_test(&mut self, document: DocumentId) {
        let started = self.stage_clock();
        self.world.rebuild_hit_test(document);
        self.record_stage(FrameStage::HitTest, started);
    }

    /// Patch only the subtrees covering `dirty`, falling back to a full document
    /// rebuild when the change is structural. See
    /// [`UiWorld::rebuild_hit_test_scoped`].
    pub fn rebuild_hit_test_for(&mut self, document: DocumentId, dirty: &[StableNodeId]) {
        let started = self.stage_clock();
        if !self.world.rebuild_hit_test_scoped(document, dirty) {
            self.world.rebuild_hit_test(document);
        }
        self.record_stage(FrameStage::HitTest, started);
    }

    /// Drain recorded scroll deltas for the in-place hit-index patch.
    pub fn take_scroll_hit_updates(&mut self) -> Vec<(StableNodeId, [f32; 2])> {
        self.world.take_scroll_hit_updates()
    }

    /// See [`UiWorld::hit_test_work_is_scroll_only`].
    pub fn hit_test_work_is_scroll_only(
        &self,
        input: &[StableNodeId],
        updates: &[(StableNodeId, [f32; 2])],
    ) -> bool {
        self.world.hit_test_work_is_scroll_only(input, updates)
    }

    /// Pre-compose a scroll translation onto the scroller subtree's hit
    /// entries instead of rebuilding the document index.
    pub fn update_hit_test_scroll(
        &mut self,
        document: DocumentId,
        scroller: StableNodeId,
        delta: [f32; 2],
    ) {
        let started = self.stage_clock();
        self.world.update_hit_test_scroll(document, scroller, delta);
        self.record_stage(FrameStage::HitTest, started);
    }

    pub fn next_animation_deadline(&self) -> Option<Duration> {
        self.world
            .next_animation_deadline()
            .into_iter()
            .chain(
                self.component_lifecycle
                    .tooltips
                    .iter()
                    .filter(|(target, _)| self.world.is_mounted(**target))
                    .filter_map(|(_, tooltip)| tooltip.show_at),
            )
            .chain(
                self.component_lifecycle
                    .hover_cards
                    .iter()
                    .filter(|(target, _)| self.world.is_mounted(**target))
                    .filter_map(|(_, lifecycle)| lifecycle.show_at.or(lifecycle.close_at)),
            )
            .min()
    }

    /// Whether a split handle hover probe may run now for this document;
    /// records the probe when true. Split-pane pointermove handling gates the
    /// probe to one per frame interval because a probe outside every handle
    /// slop walks the whole document.
    pub(crate) fn begin_split_hover_probe(&mut self, document: DocumentId, now: Duration) -> bool {
        self.component_lifecycle
            .begin_split_hover_probe(document, now)
    }

    pub fn advance_animations(&mut self, now: Duration) -> AnimationFrame {
        self.component_lifecycle.now = now;
        let mut frame = self.world.advance_animations(now);
        let finished_surfaces = frame
            .samples
            .iter()
            .filter(|sample| {
                sample.finished
                    && (crate::component_animation_id(
                        crate::component_animation_kinds::SURFACE,
                        sample.target,
                    ) == Some(sample.id)
                        || crate::component_animation_id(
                            crate::component_animation_kinds::SURFACE_POP,
                            sample.target,
                        ) == Some(sample.id))
                    && !self.world.surface_closing(sample.target)
            })
            .map(|sample| sample.target)
            .collect::<Vec<_>>();
        for target in finished_surfaces {
            let _ = self.finish_surface_exit(target);
        }
        self.finish_leaves(&frame.events);
        let tooltip_targets = self
            .component_lifecycle
            .tooltips
            .iter()
            .filter_map(|(&target, tooltip)| {
                if !self.world.is_mounted(target) {
                    return None;
                }
                tooltip.show_at.filter(|deadline| *deadline <= now)?;
                Some(target)
            })
            .collect::<Vec<_>>();
        for target in tooltip_targets {
            if self.open_tooltip(target).unwrap_or(false) {
                frame.component_updates.push(target);
            }
        }
        self.sweep_hover_cards();
        let hover_targets = self
            .component_lifecycle
            .hover_cards
            .iter()
            .filter(|(target, _)| self.world.is_mounted(**target))
            .filter_map(|(&target, lifecycle)| {
                if lifecycle.show_at.is_some_and(|deadline| deadline <= now) {
                    return Some((target, true));
                }
                if lifecycle.close_at.is_some_and(|deadline| deadline <= now) {
                    return Some((target, false));
                }
                None
            })
            .collect::<Vec<_>>();
        for (target, open) in hover_targets {
            let changed = if open {
                self.open_hover_card(target).unwrap_or(false)
            } else {
                self.close_hover_card(target).unwrap_or(false)
            };
            if changed {
                frame.component_updates.push(target);
            }
        }
        let loading_targets = frame
            .samples
            .iter()
            .filter(|sample| {
                crate::component_animation_id(
                    crate::component_animation_kinds::LOADING,
                    sample.target,
                ) == Some(sample.id)
            })
            .map(|sample| (sample.target, sample.progress))
            .collect::<Vec<_>>();
        for (target, phase) in loading_targets {
            let Some(kind) = self.component_lifecycle.loading.get(&target).copied() else {
                continue;
            };
            let changed = match kind {
                LoadingComponent::Button => self
                    .update_component(Entity::<Button>::from_stable_id(target), |button, _| {
                        button.loading_phase = phase;
                    })
                    .is_ok(),
                LoadingComponent::Switch => self
                    .update_component(Entity::<Switch>::from_stable_id(target), |switch, _| {
                        switch.loading_phase = phase;
                    })
                    .is_ok(),
                LoadingComponent::Card => self
                    .update_component(Entity::<crate::Card>::from_stable_id(target), |card, _| {
                        card.loading_phase = phase;
                    })
                    .is_ok(),
            };
            if changed && !frame.component_updates.contains(&target) {
                frame.component_updates.push(target);
            }
        }
        let section_targets = frame
            .samples
            .iter()
            .filter(|sample| {
                crate::component_animation_id(
                    crate::component_animation_kinds::SIDEBAR,
                    sample.target,
                ) == Some(sample.id)
            })
            .map(|sample| (sample.target, sample.progress, sample.value))
            .collect::<Vec<_>>();
        for (target, progress, value) in section_targets {
            let expansion = match value {
                crate::MotionValue::Scalar(value) => value,
                _ => progress,
            };
            if self
                .update_component(
                    Entity::<SidebarSection>::from_stable_id(target),
                    |section, _| {
                        section.animation_progress = expansion.clamp(0.0, 1.0);
                    },
                )
                .is_ok()
                // `advance_animations` already pushed this target for the
                // SIDEBAR sample, and the host reprojects once per entry:
                // without the guard its sibling loops all carry, a toggling
                // section is uploaded twice every frame of its transition.
                && !frame.component_updates.contains(&target)
            {
                frame.component_updates.push(target);
            }
        }
        let spinner_targets = frame
            .samples
            .iter()
            .filter(|sample| {
                crate::component_animation_id(
                    crate::component_animation_kinds::SPINNER,
                    sample.target,
                ) == Some(sample.id)
            })
            .map(|sample| (sample.target, sample.progress))
            .collect::<Vec<_>>();
        for (target, phase) in spinner_targets {
            if self
                .update_component(
                    Entity::<crate::Spinner>::from_stable_id(target),
                    |spinner, _| {
                        spinner.phase = phase;
                    },
                )
                .is_ok()
                && !frame.component_updates.contains(&target)
            {
                frame.component_updates.push(target);
            }
        }
        let workspace_targets = frame
            .samples
            .iter()
            .filter(|sample| {
                crate::component_animation_id(
                    crate::component_animation_kinds::WORKSPACE,
                    sample.target,
                ) == Some(sample.id)
            })
            .map(|sample| sample.target)
            .collect::<Vec<_>>();
        for id in workspace_targets {
            let advanced = self
                .update_component(Entity::<Workspace>::from_stable_id(id), |workspace, _| {
                    workspace.apply(WorkspaceMutation::AdvanceAnimations, now)
                })
                .unwrap_or(false);
            if advanced && !frame.component_updates.contains(&id) {
                frame.component_updates.push(id);
            }
        }
        frame.next_deadline = self.next_animation_deadline();
        frame
    }
}

/// Aggregate a finished frame profile into the process diagnostics (Issue
/// #227). Reuses the profiler's measurements; nothing is timed twice.
fn record_frame_diagnostics(profile: &FrameProfile) {
    use nana_diagnostics::MetricValue;
    use nana_diagnostics::framework::runtime as rt;
    if !nana_diagnostics::metrics_enabled() {
        return;
    }
    rt::FLUSHES.record(1);
    rt::FRAME_CPU_NS.record(profile.cpu_total.to_metric());
    for timing in &profile.stages {
        if timing.status != crate::StageStatus::Ran {
            continue;
        }
        let metric = match timing.stage {
            FrameStage::Input => &rt::STAGE_INPUT_NS,
            FrameStage::Reconcile => &rt::STAGE_RECONCILE_NS,
            FrameStage::Style => &rt::STAGE_STYLE_NS,
            FrameStage::TextShape => &rt::STAGE_TEXT_SHAPE_NS,
            FrameStage::Layout => &rt::STAGE_LAYOUT_NS,
            FrameStage::HitTest => &rt::STAGE_HIT_TEST_NS,
            FrameStage::Accessibility => &rt::STAGE_ACCESSIBILITY_NS,
            FrameStage::Animation => &rt::STAGE_ANIMATION_NS,
            FrameStage::Extract => &rt::STAGE_EXTRACT_NS,
            // GPU-host stages are recorded by the host that times them.
            FrameStage::Batch | FrameStage::GpuUpload | FrameStage::Encode | FrameStage::Submit => {
                continue;
            }
        };
        metric.record(timing.duration.to_metric());
    }
}
