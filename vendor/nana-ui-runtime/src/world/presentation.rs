//! Logical vs presentation query. Overlay storage is Motion IR; UiWorld style
//! remains the logical authority.

use super::*;
use nana_ui_core::{
    DisplaySpec, LengthSpec, PaintTransform,
    motion::{
        AnimatableProperty, AnimationClass, MotionEvaluatorBackend, MotionInspectorEntry,
        MotionTargetId, MotionValue, MotionWorkCounters, PresentationPair, cpu_fallback_reason,
    },
};

impl UiWorld {
    pub fn presentation_store(&self) -> &nana_ui_core::motion::PresentationStore {
        &self.presentation
    }

    pub fn animation_now(&self) -> Duration {
        self.animation_now
    }

    /// Host compositor present clock. Does not sample CPU/Layout tracks or
    /// mark dirty; GPU evaluate and layer hysteresis read this timestamp.
    pub fn sync_presentation_clock(&mut self, now: Duration) {
        self.animation_now = now;
    }

    pub fn motion_descriptor_store(&self) -> &nana_ui_core::motion::MotionDescriptorStore {
        &self.motion_descriptors
    }

    pub fn motion_handle(
        &self,
        id: crate::AnimationId,
    ) -> Option<nana_ui_core::motion::MotionHandle> {
        self.motion_descriptors.handle_for(id.track_id())
    }

    /// Logical / base value from retained style. Not the in-flight overlay.
    pub fn logical_motion_value(
        &self,
        id: StableNodeId,
        property: AnimatableProperty,
    ) -> Option<MotionValue> {
        let style = self.node_style(id)?;
        let layout = style.layout.as_ref();
        match property {
            AnimatableProperty::Opacity => Some(MotionValue::Scalar(layout.opacity.unwrap_or(1.0))),
            AnimatableProperty::Transform => {
                Some(MotionValue::Transform(layout.transform.unwrap_or_default()))
            }
            AnimatableProperty::Background => layout.background.map(MotionValue::Color),
            AnimatableProperty::Width => length_px(layout.width),
            AnimatableProperty::Height => length_px(layout.height),
            AnimatableProperty::Padding => length_px(layout.padding),
            AnimatableProperty::Margin => length_px(layout.margin),
            AnimatableProperty::Display => Some(MotionValue::Discrete(display_tag(
                layout.display.unwrap_or(DisplaySpec::Flex),
            ))),
            AnimatableProperty::FontAxis(tag) => nana_ui_core::FontVariationSetting::axis_value(
                &self.logical_font_variations(id),
                tag,
            )
            .map(MotionValue::Scalar),
            AnimatableProperty::Color
            | AnimatableProperty::Clip
            | AnimatableProperty::Blur
            | AnimatableProperty::Filter
            | AnimatableProperty::Shadow
            | AnimatableProperty::ShaderParameter
            | AnimatableProperty::FontSize
            | AnimatableProperty::Progress => None,
        }
    }

    /// On-demand presentation at `now`. Hit-test / pointer mapping / focus /
    /// a11y sample compositor properties here; they do not write UiWorld.
    pub fn presentation_motion_value(
        &self,
        id: StableNodeId,
        property: AnimatableProperty,
        now: Duration,
    ) -> Option<MotionValue> {
        Some(self.presentation_pair(id, property, now)?.presentation)
    }

    /// Query-time compositor samples since this world was created. Idle
    /// [`Self::advance_animations`] frames do not increment this.
    pub fn presentation_values_cpu_sampled(&self) -> usize {
        self.presentation_query_samples.get()
    }

    /// Track classification plus last-frame animation attribution.
    /// GPU upload counters stay zero until a renderer observes them.
    /// `presentation_values_cpu_sampled` is the **cumulative** query count so
    /// existing hit-test tests can diff it; last-frame delta is
    /// [`Self::last_motion_frame_counters`].
    pub fn motion_work_counters(&self) -> MotionWorkCounters {
        let mut counters = MotionWorkCounters::from_tracks(
            self.presentation.overlays().map(|overlay| &overlay.track),
        );
        counters.presentation_values_cpu_sampled = self.presentation_query_samples.get();
        counters.uiworld_mutations_from_animation =
            self.last_motion_frame.uiworld_mutations_from_animation;
        counters.layout_nodes_from_animation = self.last_motion_frame.layout_nodes_from_animation;
        counters.style_processed_from_animation =
            self.last_motion_frame.style_processed_from_animation;
        counters.render_nodes_reextracted_from_animation = self
            .last_motion_frame
            .render_nodes_reextracted_from_animation;
        counters
    }

    /// Last `advance_animations` work. CPU samples are the delta since that
    /// call started (no hit-test/query → 0).
    pub fn last_motion_frame_counters(&self) -> MotionWorkCounters {
        let mut counters = self.motion_work_counters();
        counters.presentation_values_cpu_sampled = self.last_motion_frame_cpu_samples();
        counters
    }

    pub fn last_motion_frame_cpu_samples(&self) -> usize {
        self.presentation_query_samples
            .get()
            .saturating_sub(self.presentation_samples_at_advance.get())
    }

    /// Issue #87 §14 inspector rows. Scene layer fields stay `None` until
    /// [`nana_ui_scene::UiScene::annotate_motion_inspector`] fills them.
    pub fn inspect_motion(&self) -> Vec<MotionInspectorEntry> {
        let samples = self.last_motion_frame_cpu_samples();
        let layout = self.last_motion_frame.layout_nodes_from_animation;
        let style = self.last_motion_frame.style_processed_from_animation;
        let extract = self
            .last_motion_frame
            .render_nodes_reextracted_from_animation;
        let now = self.animation_now;
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for overlay in self.presentation.overlays() {
            let track = &overlay.track;
            seen.insert(track.id);
            let node = track.target.get();
            let handle = self.motion_descriptors.handle_for(track.id);
            let has_gpu = handle.is_some_and(|handle| !handle.is_null());
            let evaluator =
                if track.property.animation_class() == AnimationClass::Compositor && has_gpu {
                    MotionEvaluatorBackend::Gpu
                } else {
                    MotionEvaluatorBackend::Cpu
                };
            let id = crate::AnimationId::new(track.id.get());
            let next_deadline = id.and_then(|id| {
                self.animations
                    .get(&id)
                    .map(|animation| animation.next_deadline)
            });
            let pair = StableNodeId::new(node)
                .and_then(|id| self.presentation_pair(id, track.property, now));
            entries.push(MotionInspectorEntry {
                node,
                property: track.property,
                class: track.property.animation_class(),
                evaluator,
                layer: None,
                layer_promotion_reason: None,
                cpu_fallback_reason: cpu_fallback_reason(track.property, has_gpu),
                ineffective_reason: StableNodeId::new(node)
                    .and_then(|id| self.ineffective_motion_reason(id, track.property)),
                base: pair.as_ref().map(|pair| pair.logical),
                presentation: pair.map(|pair| pair.presentation),
                next_deadline,
                gpu_handle: handle,
                runtime_samples_this_frame: if track.property.animation_class()
                    == AnimationClass::Compositor
                {
                    samples
                } else {
                    1
                },
                layout_nodes_from_animation: layout,
                style_processed_from_animation: style,
                render_nodes_reextracted_from_animation: extract,
            });
        }
        for (id, animation) in &self.animations {
            if seen.contains(&id.track_id()) {
                continue;
            }
            let spec = &animation.spec;
            let has_gpu = false;
            entries.push(MotionInspectorEntry {
                node: spec.target.get(),
                property: spec.property,
                class: spec.property.animation_class(),
                evaluator: MotionEvaluatorBackend::Cpu,
                layer: None,
                layer_promotion_reason: None,
                cpu_fallback_reason: cpu_fallback_reason(spec.property, has_gpu),
                ineffective_reason: self.ineffective_motion_reason(spec.target, spec.property),
                base: self.logical_motion_value(spec.target, spec.property),
                presentation: None,
                next_deadline: Some(animation.next_deadline),
                gpu_handle: None,
                runtime_samples_this_frame: 1,
                layout_nodes_from_animation: layout,
                style_processed_from_animation: style,
                render_nodes_reextracted_from_animation: extract,
            });
        }
        entries.sort_by_key(|entry| (entry.node, entry.property));
        entries
    }

    pub(super) fn begin_motion_frame(&mut self) {
        self.presentation_samples_at_advance
            .set(self.presentation_query_samples.get());
        self.last_motion_frame = MotionWorkCounters::default();
    }

    pub(super) fn account_animation_dirty(&mut self, bits: u16) {
        self.last_motion_frame.uiworld_mutations_from_animation = self
            .last_motion_frame
            .uiworld_mutations_from_animation
            .saturating_add(1);
        if bits & crate::schedule::DirtyMask::LAYOUT != 0 {
            self.last_motion_frame.layout_nodes_from_animation = self
                .last_motion_frame
                .layout_nodes_from_animation
                .saturating_add(1);
        }
        if bits & crate::schedule::DirtyMask::STYLE != 0 {
            self.last_motion_frame.style_processed_from_animation = self
                .last_motion_frame
                .style_processed_from_animation
                .saturating_add(1);
        }
        if bits & crate::schedule::DirtyMask::RENDER != 0 {
            self.last_motion_frame
                .render_nodes_reextracted_from_animation = self
                .last_motion_frame
                .render_nodes_reextracted_from_animation
                .saturating_add(1);
        }
    }

    pub(super) fn has_compositor_transform_overlay_for(&self, document: DocumentId) -> bool {
        let revision = (self.presentation.revision(), self.generation);
        if let Some((presentation_revision, world_revision, documents)) = self
            .presentation_transform_documents_cache
            .borrow()
            .as_ref()
            && (*presentation_revision, *world_revision) == revision
        {
            return documents.contains(&document);
        }
        let documents: std::collections::HashSet<DocumentId> = self
            .presentation
            .overlays()
            .filter(|overlay| overlay.track.property == AnimatableProperty::Transform)
            .filter_map(|overlay| {
                StableNodeId::new(overlay.track.target.get()).and_then(|id| self.document_of(id))
            })
            .collect();
        let contains = documents.contains(&document);
        *self.presentation_transform_documents_cache.borrow_mut() =
            Some((revision.0, revision.1, documents));
        contains
    }

    pub(super) fn sampled_compositor_transform(&self, id: StableNodeId) -> Option<PaintTransform> {
        match self.sample_compositor_presentation(id, AnimatableProperty::Transform)? {
            MotionValue::Transform(transform) => Some(transform),
            _ => None,
        }
    }

    pub(super) fn sampled_compositor_opacity(&self, id: StableNodeId) -> Option<f32> {
        match self.sample_compositor_presentation(id, AnimatableProperty::Opacity)? {
            MotionValue::Scalar(value) => Some(value),
            _ => None,
        }
    }

    fn sample_compositor_presentation(
        &self,
        id: StableNodeId,
        property: AnimatableProperty,
    ) -> Option<MotionValue> {
        if property.animation_class() != AnimationClass::Compositor {
            return None;
        }
        let target = MotionTargetId::new(id.get())?;
        if !self.presentation.has_property(target, property) {
            return None;
        }
        self.presentation_query_samples
            .set(self.presentation_query_samples.get().saturating_add(1));
        self.presentation
            .applied_value(target, property, self.animation_now)
    }

    /// Local scene transform for hit-test / a11y / focus. Compositor
    /// `transform` overlays replace the logical matrix; Layout-class overlays
    /// are ignored so width/height animation cannot fake a scale.
    pub(super) fn input_local_scene_transform(
        &self,
        id: StableNodeId,
        style: &nana_ui_core::LayoutStyle,
        bounds: LayoutBox,
        blocks_3d: bool,
    ) -> ([f32; 6], [f32; 2]) {
        // The refusal comes first: an overlay is not a way around a closed 3D
        // context, or the pointer keeps a projection paint gave up and a click
        // lands where the node is not drawn.
        if blocks_3d && style.transform_3d.is_some() {
            return (IDENTITY_AFFINE, [0.0, 0.0]);
        }
        if let Some(transform) = self.sampled_compositor_transform(id) {
            let [ox, oy] = style.resolved_transform_origin(bounds.width, bounds.height);
            return (
                transform.around_origin(bounds.x, bounds.y, ox, oy),
                [0.0, 0.0],
            );
        }
        style
            .world_scene_transform(bounds.x, bounds.y, bounds.width, bounds.height)
            .unwrap_or((IDENTITY_AFFINE, [0.0, 0.0]))
    }

    /// Viewport AABB of the logical layout box after compositor presentation
    /// transform. Layout-class overlays do not change the box.
    pub fn presentation_input_bounds(&self, id: StableNodeId) -> Option<LayoutBox> {
        self.project_input_bounds(id, self.layout_box(id)?)
    }

    /// Focus geometry follows compositor presentation, not the logical rest
    /// transform.
    pub fn focused_geometry(&self, document: DocumentId) -> Option<LayoutBox> {
        self.presentation_input_bounds(self.focused(document)?)
    }

    /// Where `bounds`, in `id`'s layout coordinates, sits in the viewport:
    /// through the scroll offsets and transforms above `id`, up to its nearest
    /// viewport-fixed ancestor. Layout anchors fixed surfaces on this.
    pub(crate) fn project_input_bounds(
        &self,
        id: StableNodeId,
        bounds: LayoutBox,
    ) -> Option<LayoutBox> {
        let ([a, by, c, d, e, f], [g, h]) = self.layout_projection_transform(id)?;
        project_transformed_box(bounds, [a, by, c, d, e, f], [g, h])
    }

    pub fn presentation_pair(
        &self,
        id: StableNodeId,
        property: AnimatableProperty,
        now: Duration,
    ) -> Option<PresentationPair> {
        let logical = self.logical_motion_value(id, property)?;
        let target = MotionTargetId::new(id.get())?;
        Some(self.presentation.pair(target, property, logical, now))
    }

    pub fn presentation_applied_value(
        &self,
        id: StableNodeId,
        property: AnimatableProperty,
        now: Duration,
    ) -> Option<MotionValue> {
        let target = MotionTargetId::new(id.get())?;
        self.presentation.applied_value(target, property, now)
    }

    /// Enter/exit invariant: unfinished overlays survive park/logical unmount.
    pub fn presentation_retains_unmounted(&self, id: StableNodeId, now: Duration) -> bool {
        MotionTargetId::new(id.get())
            .is_some_and(|target| self.presentation.retains_unmounted(target, now))
    }

    pub fn motion_descriptors(&self) -> &nana_ui_core::motion::MotionDescriptorStore {
        &self.motion_descriptors
    }

    /// Advanced compositor layer request. Extract copies this onto
    /// [`crate::ExtractedCompositor::request_layer`].
    pub fn request_compositor_layer(&mut self, id: StableNodeId) {
        self.compositor_layer_requests.insert(id);
        self.mark(id, crate::schedule::DirtyMask::RENDER);
    }

    pub fn clear_compositor_layer_request(&mut self, id: StableNodeId) {
        self.compositor_layer_requests.remove(&id);
        self.mark(id, crate::schedule::DirtyMask::RENDER);
    }

    pub(super) fn extracted_compositor(&self, id: StableNodeId) -> crate::ExtractedCompositor {
        let mut bindings = Vec::new();
        if let Some(target) = MotionTargetId::new(id.get()) {
            for property in [
                AnimatableProperty::Transform,
                AnimatableProperty::Opacity,
                AnimatableProperty::Clip,
                AnimatableProperty::ShaderParameter,
            ] {
                if let Some(track) =
                    self.presentation
                        .winning_track_id(target, property, self.animation_now)
                    && !bindings.contains(&track)
                {
                    bindings.push(track);
                }
            }
        }
        crate::ExtractedCompositor {
            bindings,
            request_layer: self.compositor_layer_requests.contains(&id),
        }
    }
}

fn length_px(spec: Option<LengthSpec>) -> Option<MotionValue> {
    match spec {
        Some(LengthSpec::Px(px)) if px.is_finite() => Some(MotionValue::Scalar(px)),
        _ => None,
    }
}

fn display_tag(display: DisplaySpec) -> u32 {
    display as u32
}
