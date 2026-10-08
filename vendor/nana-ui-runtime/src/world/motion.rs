//! Retained surface presence: input closes immediately, paint survives the exit.
use super::*;
use nana_ui_core::{PaintTransform, motion as tokens};

use crate::MotionTargetId;

#[derive(Clone, Copy)]
pub(super) struct SurfaceMotion {
    pub open: bool,
    menu: bool,
    pub running: bool,
}

#[derive(Clone, Copy, Default)]
struct LayoutClassOverlay {
    width: Option<f32>,
    height: Option<f32>,
    padding: Option<f32>,
    margin: Option<f32>,
}

/// A layout length as a box can take it. An overshooting curve can carry a
/// width, height or padding below zero on its way; the box stops at zero
/// rather than laying out a negative size. A margin may be negative.
fn shown_length(property: crate::AnimatableProperty, px: f32) -> f32 {
    match property {
        crate::AnimatableProperty::Margin => px,
        _ => px.max(0.0),
    }
}

/// What a change to an inherited text input dirties, on the node and every
/// descendant inheriting it: a style write and a font-axis sample alike.
pub(super) const INHERITED_TEXT_DIRTY: u16 =
    DirtyMask::STYLE | DirtyMask::TEXT | DirtyMask::LAYOUT | DirtyMask::INPUT | DirtyMask::RENDER;

fn scale_transform(scale: f32) -> PaintTransform {
    PaintTransform {
        a: scale,
        b: 0.0,
        c: 0.0,
        d: scale,
        e: 0.0,
        f: 0.0,
    }
}

impl UiWorld {
    pub(super) fn set_surface_open(&mut self, id: StableNodeId, open: bool, menu: bool) {
        if !self.is_mounted(id) {
            return;
        }
        if self
            .surface_motion
            .get(&id)
            .is_some_and(|motion| motion.open == open)
        {
            return;
        }
        let from_opacity = MotionTargetId::new(id.get())
            .and_then(|target| {
                self.presentation.applied_value(
                    target,
                    crate::AnimatableProperty::Opacity,
                    self.animation_now,
                )
            })
            .and_then(|value| match value {
                crate::MotionValue::Scalar(value) => Some(value),
                _ => None,
            })
            .unwrap_or(f32::from(!open));
        let from_pop = MotionTargetId::new(id.get())
            .and_then(|target| {
                self.presentation.applied_value(
                    target,
                    crate::AnimatableProperty::Transform,
                    self.animation_now,
                )
            })
            .and_then(|value| match value {
                crate::MotionValue::Transform(transform) => Some(transform.a),
                _ => None,
            })
            .unwrap_or(if open { 0.9 } else { 1.0 });
        if open {
            self.closing_surfaces.remove(&id);
        } else {
            self.closing_surfaces.insert(id);
            self.clear_surface_pointer_interactions(id);
        }
        self.surface_motion.insert(
            id,
            SurfaceMotion {
                open,
                menu,
                running: true,
            },
        );
        let to_opacity = f32::from(open);
        self.start_component_track(
            id,
            crate::component_animation_kinds::SURFACE,
            if menu {
                tokens::MENU_OPACITY
            } else {
                tokens::OVERLAY_FADE
            },
            crate::Easing::EaseOutCubic,
            crate::AnimatableProperty::Opacity,
            crate::MotionValue::Scalar(from_opacity),
            crate::MotionValue::Scalar(to_opacity),
            crate::MotionInterrupt::Retarget,
            None,
        );
        if menu {
            let to_scale = if open { 1.0 } else { 0.9 };
            self.start_component_track(
                id,
                crate::component_animation_kinds::SURFACE_POP,
                tokens::MENU_POP,
                crate::Easing::MENU_POP,
                crate::AnimatableProperty::Transform,
                crate::MotionValue::Transform(scale_transform(from_pop)),
                crate::MotionValue::Transform(scale_transform(to_scale)),
                crate::MotionInterrupt::Retarget,
                None,
            );
        }
        self.mark_subtree(id, DirtyMask::STYLE | DirtyMask::RENDER | DirtyMask::INPUT);
    }

    pub(crate) fn project_menu_presence(
        &self,
        id: StableNodeId,
        requested: bool,
        mutations: &mut MutationQueue,
    ) -> bool {
        // OverlayHost owns presence while its child is activated or closing.
        if self
            .parent_id(id)
            .and_then(|parent| self.overlay_host(parent))
            .is_some_and(|host| host.active == Some(id))
            && self.surface_closed(id)
        {
            return requested || self.surface_closing(id);
        }
        let motion = self.surface_motion.get(&id);
        // `set_surface_open` ignores a parked node, so asking would repeat on
        // every projection; the remount projects the surface again. A node
        // still being created is not in the world yet and does ask.
        let parked = self.contains(id) && !self.is_mounted(id);
        if !parked && motion.map(|motion| motion.open).unwrap_or(false) != requested {
            mutations.set_surface_open(id, requested, true);
        }
        requested || motion.is_some_and(|motion| motion.open || motion.running)
    }

    pub(crate) fn surface_closed(&self, id: StableNodeId) -> bool {
        self.surface_motion
            .get(&id)
            .is_some_and(|motion| !motion.open)
    }

    pub(crate) fn surface_closing(&self, id: StableNodeId) -> bool {
        self.surface_motion
            .get(&id)
            .is_some_and(|motion| !motion.open && motion.running)
    }

    pub(crate) fn motion_blocks_input(&self, id: StableNodeId) -> bool {
        if self.closing_surfaces.is_empty() {
            return false;
        }
        let mut current = Some(id);
        while let Some(id) = current {
            if self.surface_closing(id) {
                return true;
            }
            current = self.parent_id(id);
        }
        false
    }

    pub(super) fn advance_surface_motion(&mut self, sample: &crate::AnimationSample) {
        let Some(motion) = self.surface_motion.get(&sample.target) else {
            return;
        };
        if !sample.finished {
            return;
        }
        let opacity_done =
            crate::component_animation_id(crate::component_animation_kinds::SURFACE, sample.target)
                .is_none_or(|id| !self.animation_is_active(id) || sample.id == id);
        let pop_done = !motion.menu
            || crate::component_animation_id(
                crate::component_animation_kinds::SURFACE_POP,
                sample.target,
            )
            .is_none_or(|id| !self.animation_is_active(id) || sample.id == id);
        if !(opacity_done && pop_done) {
            return;
        }
        let Some(motion) = self.surface_motion.get_mut(&sample.target) else {
            return;
        };
        motion.running = false;
        if !motion.open {
            self.closing_surfaces.remove(&sample.target);
        }
        // Closing drops the items out of the menu branch without writing
        // `hidden` on them. Their last boxes would keep the hanging surface
        // painted. Layout has to run again so those boxes go to zero.
        let mut dirty = DirtyMask::STYLE | DirtyMask::RENDER | DirtyMask::INPUT;
        if !motion.open {
            dirty |= DirtyMask::LAYOUT;
        }
        self.mark_subtree(sample.target, dirty);
        self.account_animation_dirty(dirty);
    }

    pub(super) fn motion_layout(
        &self,
        id: StableNodeId,
        source: &Arc<LayoutStyle>,
    ) -> Arc<LayoutStyle> {
        let Some(overlay) = self.layout_class_overlay(id) else {
            return Arc::clone(source);
        };
        let mut layout = (**source).clone();
        if let Some(width) = overlay.width {
            layout.width = Some(LengthSpec::Px(width));
        }
        if let Some(height) = overlay.height {
            layout.height = Some(LengthSpec::Px(height));
        }
        if let Some(padding) = overlay.padding {
            layout.padding = Some(LengthSpec::Px(padding));
        }
        if let Some(margin) = overlay.margin {
            layout.margin = Some(LengthSpec::Px(margin));
        }
        Arc::new(layout)
    }

    fn layout_class_overlay(&self, id: StableNodeId) -> Option<LayoutClassOverlay> {
        let now = self.animation_now;
        let mut overlay = LayoutClassOverlay::default();
        let mut any = false;
        for animation in self.layout_length_tracks(id) {
            let Some(track) = animation.spec.to_motion_track() else {
                continue;
            };
            let sample = nana_ui_core::motion::evaluate_track(&track, now);
            let Some(crate::MotionValue::Scalar(px)) = sample
                .applied_value()
                .or(sample.finished.then_some(sample.value))
            else {
                continue;
            };
            if !px.is_finite() {
                continue;
            }
            let px = shown_length(animation.spec.property, px);
            match animation.spec.property {
                crate::AnimatableProperty::Width => overlay.width = Some(px),
                crate::AnimatableProperty::Height => overlay.height = Some(px),
                crate::AnimatableProperty::Padding => overlay.padding = Some(px),
                crate::AnimatableProperty::Margin => overlay.margin = Some(px),
                _ => continue,
            }
            any = true;
        }
        any.then_some(overlay)
    }

    /// The axes `id` declares, or inherits when it declares none, without its
    /// own in-flight axis tracks: what a font-axis track starts from, and
    /// what a finished one that fills forwards writes into.
    pub(crate) fn logical_font_variations(
        &self,
        id: StableNodeId,
    ) -> Vec<nana_ui_core::FontVariationSetting> {
        if !self.contains(id) {
            return Vec::new();
        }
        let record = self.record(id);
        if let Some(axes) = &record.style.layout.font_variation_settings {
            return axes.clone();
        }
        record
            .hierarchy
            .parent
            .and_then(|parent| self.computed_style(parent))
            .map(|style| style.font_variations.clone())
            .unwrap_or_default()
    }

    /// `id`'s font-axis values at the animation clock, from its overlays:
    /// the in-flight tracks and the holds finished ones that fill forwards
    /// left, one winner per axis by [`nana_ui_core::motion::PresentationStore`]'s
    /// rule.
    ///
    /// Style resolution lays these over the node's computed axes, so a sample
    /// lands in `ComputedStyle::font_variations` — the text style shaping
    /// reads — and descendants inherit it, while the authored list stays what
    /// the author wrote. The text dirty graph sees an axis change there as
    /// `SHAPE_STYLE`, like any other. A node without axis overlays pays one
    /// map lookup.
    ///
    /// `None` is an axis the animated list leaves out (a keyframe that does
    /// not name it samples as NaN): it is dropped from the computed axes, so
    /// the face's default applies.
    pub(super) fn font_axis_overlay(&self, id: StableNodeId) -> Vec<([u8; 4], Option<f32>)> {
        let Some(target) = MotionTargetId::new(id.get()) else {
            return Vec::new();
        };
        let now = self.animation_now;
        self.presentation
            .properties_of(target)
            .filter_map(|property| {
                let crate::AnimatableProperty::FontAxis(tag) = property else {
                    return None;
                };
                match self.presentation.applied_value(target, property, now)? {
                    value if value.is_absent() => Some((tag, None)),
                    crate::MotionValue::Scalar(value) if value.is_finite() => {
                        Some((tag, Some(value)))
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// Records that what `target`'s axes resolve to changed: the style
    /// resolves again, then the text work the axis change classifies as.
    ///
    /// Axes inherit as one list, so a descendant that declares its own does
    /// not see this node's at all; the walk stops there instead of restyling
    /// and reshaping a subtree the change cannot reach.
    pub(super) fn mark_font_axes_changed(&mut self, target: StableNodeId) {
        if !self.contains(target) {
            return;
        }
        for id in self.axis_inheritors(target) {
            let _ = self.mark(id, INHERITED_TEXT_DIRTY);
        }
        self.account_animation_dirty(INHERITED_TEXT_DIRTY);
    }

    /// `id` and every descendant that takes its axes from it: the walk stops
    /// at a node declaring its own list.
    fn axis_inheritors(&self, id: StableNodeId) -> Vec<StableNodeId> {
        let mut found = Vec::new();
        let mut stack = vec![id];
        while let Some(node) = stack.pop() {
            found.push(node);
            stack.extend(
                self.record(node)
                    .hierarchy
                    .children
                    .iter()
                    .copied()
                    .filter(|child| {
                        self.record(*child)
                            .style
                            .layout
                            .font_variation_settings
                            .is_none()
                    }),
            );
        }
        found
    }

    /// Whether animating axis `tag` of `id` changes nothing: every run of
    /// every text taking its axes from `id` was shaped by a face without the
    /// axis. `false` until something was shaped, and for `wght` / `wdth`,
    /// which steer which face is picked even where no face has the axis.
    pub fn font_axis_is_ineffective(&self, id: StableNodeId, tag: [u8; 4]) -> bool {
        if tag == nana_ui_core::FontVariationSetting::WGHT
            || tag == nana_ui_core::FontVariationSetting::WDTH
            || !self.contains(id)
        {
            return false;
        }
        let mut shaped = false;
        for node in self.axis_inheritors(id) {
            if let Some((_, layout)) = self.text_layout(node) {
                for run in &layout.runs {
                    if !run.ignored_axes.contains(&tag) {
                        return false;
                    }
                    shaped = true;
                }
            }
        }
        shaped
    }

    /// Developer diagnostic for a track that changes nothing it animates.
    pub(crate) fn ineffective_motion_reason(
        &self,
        id: StableNodeId,
        property: crate::AnimatableProperty,
    ) -> Option<String> {
        let crate::AnimatableProperty::FontAxis(tag) = property else {
            return None;
        };
        self.font_axis_is_ineffective(id, tag).then(|| {
            format!(
                "no face the text is shaped with has axis `{}`; it is ignored, not mapped onto another axis",
                String::from_utf8_lossy(&tag)
            )
        })
    }

    /// A font-axis sample. The value itself is read back through
    /// [`Self::font_axis_overlay`] when the style resolves, so this only
    /// schedules that work — and not even that when the node already shows
    /// the value (a flat keyframe stretch, a hold being re-sampled).
    pub(super) fn apply_font_axis_sample(
        &mut self,
        sample: &crate::AnimationSample,
        tag: [u8; 4],
    ) -> bool {
        if !self.contains(sample.target) {
            return false;
        }
        if !sample.finished {
            let Some(crate::MotionValue::Scalar(value)) = sample.applied_value() else {
                // Before a delay with no backwards fill: nothing is shown yet.
                return false;
            };
            let shown = self.computed_style(sample.target).and_then(|style| {
                nana_ui_core::FontVariationSetting::axis_value(&style.font_variations, tag)
            });
            let showing = (!crate::MotionValue::Scalar(value).is_absent()).then_some(value);
            if shown == showing {
                return false;
            }
        }
        self.mark_font_axes_changed(sample.target);
        true
    }

    pub(super) fn apply_layout_class_sample(&mut self, sample: &crate::AnimationSample) -> bool {
        if sample.property.animation_class() != crate::AnimationClass::Layout {
            return false;
        }
        if let crate::AnimatableProperty::FontAxis(tag) = sample.property {
            return self.apply_font_axis_sample(sample, tag);
        }
        let Some(crate::MotionValue::Scalar(px)) = sample
            .applied_value()
            .or(sample.finished.then_some(sample.value))
        else {
            return false;
        };
        if !px.is_finite() || !self.contains(sample.target) {
            return false;
        }
        let length = LengthSpec::Px(shown_length(sample.property, px));
        let layout = Arc::make_mut(&mut self.record_mut(sample.target).style.layout);
        let changed = match sample.property {
            crate::AnimatableProperty::Width => {
                let changed = layout.width != Some(length);
                layout.width = Some(length);
                changed
            }
            crate::AnimatableProperty::Height => {
                let changed = layout.height != Some(length);
                layout.height = Some(length);
                changed
            }
            crate::AnimatableProperty::Padding => {
                let changed = layout.padding != Some(length);
                layout.padding = Some(length);
                changed
            }
            crate::AnimatableProperty::Margin => {
                let changed = layout.margin != Some(length);
                layout.margin = Some(length);
                changed
            }
            _ => return false,
        };
        if !changed {
            return false;
        }
        // The authored layout just moved in place; the resolved copy layout
        // reads has to follow. A node with no design intent keeps sharing the
        // same `Arc`, so an animation that touches no tier costs nothing.
        self.refresh_resolved_layout(sample.target);
        // A pixel length can only take a viewport dependency away.
        let record = self.record_mut(sample.target);
        if record.layout_depends_on_viewport {
            record.layout_depends_on_viewport = record.style.layout.depends_on_viewport();
        }
        if sample.property != crate::AnimatableProperty::Margin {
            // Written straight onto the authored style, not through
            // `SetStyle`: the content box, and whether its height is definite,
            // are the text's constraints. A margin only moves the box.
            self.nodes
                .invalidate_text(sample.target, crate::text_node::TextDirty::CONSTRAINT);
        }
        self.mark_subtree(
            sample.target,
            DirtyMask::LAYOUT | DirtyMask::INPUT | DirtyMask::ACCESSIBILITY | DirtyMask::RENDER,
        );
        self.account_animation_dirty(DirtyMask::LAYOUT | DirtyMask::RENDER);
        true
    }

    pub(super) fn advance_loading_phase(&mut self, sample: &crate::AnimationSample) {
        let Some(mut visual) = self.standard_visual(sample.target) else {
            return;
        };
        match &mut visual {
            StandardVisual::Button { loading_phase, .. }
            | StandardVisual::Switch { loading_phase, .. }
            | StandardVisual::Card { loading_phase, .. } => {
                *loading_phase = sample.progress;
            }
            _ => return,
        }
        self.nodes.set_visual(sample.target, Some(visual));
        self.mark(sample.target, DirtyMask::RENDER);
        self.account_animation_dirty(DirtyMask::RENDER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionMenu, AnchoredActionMenu, AppContext, Button, Dialog, Entity, OverlayHost,
        OverlayKey, Switch,
    };

    /// A parked surface cannot hold open motion, so asking for it while
    /// parked is skipped; mounting it must project it again.
    #[test]
    fn a_menu_surface_opened_while_parked_opens_once_mounted() {
        let mut cx = AppContext::new();
        let document = crate::DocumentId::new(1).unwrap();
        let root = cx
            .create_component(document, crate::Stack::column(0.0))
            .unwrap();
        let popover = cx
            .create_detached_component(document, crate::Popover::new())
            .unwrap();
        cx.update_component(popover, |popover, _| popover.open = true)
            .unwrap();
        let generation = cx.world().generation();
        cx.update_component(popover, |popover, _| popover.open = true)
            .unwrap();
        assert_eq!(
            cx.world().generation(),
            generation,
            "a parked surface does not ask for open motion on every projection"
        );

        cx.append_child(root, popover).unwrap();

        assert!(
            cx.world()
                .surface_motion
                .get(&popover.stable_id())
                .is_some_and(|motion| motion.open)
        );
    }

    fn tick(cx: &mut AppContext, ms: u64) {
        cx.advance_animations(Duration::from_millis(ms));
        let work = cx.take_system_work();
        cx.resolve_styles(&work.style).unwrap();
    }

    /// What shows: the overlay while one applies, else the logical opacity.
    fn alpha(cx: &AppContext, id: StableNodeId) -> f32 {
        match cx.world().presentation_applied_value(
            id,
            crate::AnimatableProperty::Opacity,
            cx.world().animation_now(),
        ) {
            Some(crate::MotionValue::Scalar(value)) => value,
            _ => logical_opacity(cx, id).unwrap_or(1.0),
        }
    }

    fn logical_opacity(cx: &AppContext, id: StableNodeId) -> Option<f32> {
        cx.world()
            .node_style(id)
            .and_then(|style| style.layout.opacity)
    }

    #[test]
    fn menu_exit_keeps_paint_and_reopening_reverses_without_an_old_completion() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let menu = cx
            .create_component(doc, ActionMenu::new().open(true))
            .unwrap();
        tick(&mut cx, 0);
        assert_eq!(alpha(&cx, menu.stable_id()), 0.0);
        assert_eq!(logical_opacity(&cx, menu.stable_id()), None);
        tick(&mut cx, 80);
        let opening = alpha(&cx, menu.stable_id());
        assert!((opening - 0.875).abs() < 1e-5);
        assert_eq!(logical_opacity(&cx, menu.stable_id()), None);
        let pop = match cx.world().presentation_applied_value(
            menu.stable_id(),
            crate::AnimatableProperty::Transform,
            cx.world().animation_now(),
        ) {
            Some(crate::MotionValue::Transform(transform)) => transform.a,
            _ => panic!("menu pop must live on the transform overlay"),
        };
        assert!(pop > 0.9 && pop < 1.0);
        cx.update_component(menu, |menu, _| menu.popover.open = false)
            .unwrap();
        assert_eq!(alpha(&cx, menu.stable_id()), opening);
        assert!(matches!(
            cx.world().standard_visual(menu.stable_id()),
            Some(StandardVisual::MenuSurface { open: true, .. })
        ));
        tick(&mut cx, 120);
        let closing = alpha(&cx, menu.stable_id());
        assert!(closing < opening);
        cx.update_component(menu, |menu, _| menu.popover.open = true)
            .unwrap();
        assert_eq!(alpha(&cx, menu.stable_id()), closing);
        tick(&mut cx, 260);
        assert!(matches!(
            cx.world().standard_visual(menu.stable_id()),
            Some(StandardVisual::MenuSurface { open: true, .. })
        ));
        tick(&mut cx, 300);
        assert_eq!(alpha(&cx, menu.stable_id()), 1.0);
        assert_eq!(cx.next_animation_deadline(), None);
        cx.update_component(menu, |menu, _| menu.popover.open = false)
            .unwrap();
        tick(&mut cx, 480);
        assert_eq!(cx.world().standard_visual(menu.stable_id()), None);
        assert_eq!(cx.next_animation_deadline(), None);
    }

    #[test]
    fn anchored_menu_unmounts_its_surface_only_after_the_exit() {
        let mut cx = AppContext::new();
        let menu = cx
            .create_component(
                DocumentId::new(1).unwrap(),
                AnchoredActionMenu::new(20.0, 20.0),
            )
            .unwrap();
        tick(&mut cx, 180);
        cx.update_component(menu, |menu, _| menu.open = false)
            .unwrap();
        assert!(
            !cx.world()
                .node_style(menu.stable_id())
                .unwrap()
                .layout
                .hidden
        );
        tick(&mut cx, 359);
        assert!(
            !cx.world()
                .node_style(menu.stable_id())
                .unwrap()
                .layout
                .hidden
        );
        tick(&mut cx, 360);
        assert!(
            cx.world()
                .node_style(menu.stable_id())
                .unwrap()
                .layout
                .hidden
        );
    }

    #[test]
    fn dialog_escape_restores_focus_and_excludes_hits_before_delayed_unload() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let base = cx.create_component(doc, Button::new("Open")).unwrap();
        let host = cx.create_component(doc, OverlayHost::new()).unwrap();
        let dialog = cx.create_component(doc, Dialog::new("Settings")).unwrap();
        cx.append_child(host, dialog).unwrap();
        cx.focus_node(doc, base.stable_id()).unwrap();
        cx.activate_overlay(host, dialog).unwrap();
        let mut layout = MutationQueue::new();
        layout.write_layout(
            dialog.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        cx.commit_mutations(layout).unwrap();
        tick(&mut cx, 140);
        cx.rebuild_hit_test(doc);
        assert!(
            cx.world()
                .hit_test_candidates(doc, 10.0, 10.0)
                .contains(&dialog.stable_id())
        );
        assert!(cx.route_overlay_key(doc, OverlayKey::Escape).unwrap());
        assert_eq!(cx.world().focused(doc), Some(base.stable_id()));
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            Some(dialog.stable_id())
        );
        assert!(
            !cx.world()
                .hit_test_candidates(doc, 10.0, 10.0)
                .contains(&dialog.stable_id())
        );
        assert!(cx.active_runtime_overlay(doc).is_none());
        tick(&mut cx, 200);
        assert_eq!(cx.world().focused(doc), Some(base.stable_id()));
        let closing = alpha(&cx, dialog.stable_id());
        assert!(closing > 0.0 && closing < 1.0);
        cx.activate_overlay(host, dialog).unwrap();
        assert_eq!(alpha(&cx, dialog.stable_id()), closing);
        assert_eq!(cx.world().focused(doc), Some(dialog.stable_id()));
        tick(&mut cx, 340);
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            Some(dialog.stable_id())
        );
        cx.dismiss_overlay(host).unwrap();
        cx.focus_node(doc, base.stable_id()).unwrap();
        tick(&mut cx, 480);
        assert_eq!(
            cx.world().overlay_host(host.stable_id()).unwrap().active,
            None
        );
        assert_eq!(cx.world().focused(doc), Some(base.stable_id()));
        assert_eq!(cx.next_animation_deadline(), None);
    }

    fn thumb(cx: &AppContext, switch: Entity<Switch>) -> f32 {
        match cx.world().standard_visual(switch.stable_id()).unwrap() {
            StandardVisual::Switch { thumb_progress, .. } => thumb_progress,
            _ => unreachable!(),
        }
    }

    #[test]
    fn switch_reverses_from_current_position_and_releases_its_deadline() {
        let mut cx = AppContext::new();
        let switch = cx
            .create_component(DocumentId::new(1).unwrap(), Switch::new("Enabled", false))
            .unwrap();
        assert_eq!(cx.next_animation_deadline(), None);
        cx.update_component(switch, |switch, _| switch.checked = true)
            .unwrap();
        assert_eq!(thumb(&cx, switch), 0.0);
        tick(&mut cx, 70);
        assert!((thumb(&cx, switch) - 0.875).abs() < 1e-5);
        cx.update_component(switch, |switch, _| switch.checked = false)
            .unwrap();
        assert!((thumb(&cx, switch) - 0.875).abs() < 1e-5);
        tick(&mut cx, 140);
        assert!(thumb(&cx, switch) > 0.0 && thumb(&cx, switch) < 0.875);
        tick(&mut cx, 210);
        assert_eq!(thumb(&cx, switch), 0.0);
        assert_eq!(cx.next_animation_deadline(), None);
    }

    #[test]
    fn hover_interpolates_both_directions_without_layout_or_idle_frames() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Hover")).unwrap();
        tick(&mut cx, 0);
        let id = button.stable_id();
        let idle = cx.world().computed_style(id).unwrap().background;
        cx.set_pointer_hover_at(doc, 1, Some(id), Duration::ZERO)
            .unwrap();
        tick(&mut cx, 0);
        assert_eq!(cx.world().computed_style(id).unwrap().background, idle);
        assert!(
            cx.world().animation_is_active(
                crate::component_animation_id(crate::component_animation_kinds::HOVER, id)
                    .expect("hover animation id")
            ),
            "hover color is a Motion IR track, not a parallel timer"
        );
        tick(&mut cx, 60);
        let middle = cx.world().computed_style(id).unwrap().background;
        assert_ne!(middle, idle);
        cx.set_pointer_hover_at(doc, 1, None, Duration::from_millis(60))
            .unwrap();
        let work = cx.take_system_work();
        assert!(work.layout.is_empty());
        cx.resolve_styles(&work.style).unwrap();
        assert_eq!(cx.world().computed_style(id).unwrap().background, middle);
        tick(&mut cx, 180);
        assert_eq!(cx.world().computed_style(id).unwrap().background, idle);
        assert_eq!(cx.next_animation_deadline(), None);
        assert!(
            !cx.advance_animations(Duration::from_millis(1000))
                .has_updates()
        );
    }

    #[test]
    fn hover_foreground_repaints_inheriting_children_through_the_final_frame() {
        let mut world = UiWorld::new();
        let doc = DocumentId::new(1).unwrap();
        let root = StableNodeId::new(1).unwrap();
        let parent = StableNodeId::new(2).unwrap();
        let child = StableNodeId::new(3).unwrap();
        let mut create = MutationQueue::new();
        create.create(root, doc, NodeKind::Document);
        create.create(parent, doc, NodeKind::Element { tag: "row".into() });
        create.create(child, doc, NodeKind::Text);
        create.insert(root, parent, None);
        create.insert(parent, child, None);
        let mut style = NodeStyle {
            foreground: Some(SemanticColorRole::Text),
            ..NodeStyle::default()
        };
        style.interaction.hovered.foreground = Some(SemanticColorRole::Accent);
        create.set_style(parent, style);
        create.set_interaction(
            parent,
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
        );
        world.commit(create).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        let idle = world.computed_style(child).unwrap().color;
        world.set_pointer_hover(doc, 1, Some(parent)).unwrap();
        for ms in [0, 60, 120] {
            world.advance_animations(Duration::from_millis(ms));
            let work = world.take_system_work();
            assert!(work.style.contains(&child));
            assert!(work.render_extraction.contains(&child));
            assert!(work.layout.is_empty());
            world.resolve_styles(&work.style).unwrap();
            assert_eq!(
                world.computed_style(child).unwrap().color,
                world.computed_style(parent).unwrap().color
            );
            if ms == 0 {
                assert_eq!(world.computed_style(child).unwrap().color, idle);
            } else {
                assert_ne!(world.computed_style(child).unwrap().color, idle);
            }
        }
        assert_eq!(world.next_animation_deadline(), None);
    }

    #[test]
    fn l3_transition_spring_and_timeline_compile_to_motion_ir() {
        use crate::{
            AnimationPlayback, MotionCurve, MotionGraph, MotionTargetId, MotionTiming, MotionTrack,
            MotionTrackId, Spring, Timeline,
        };

        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Motion")).unwrap();
        let id = button.stable_id();
        cx.update_component(button, |_, ctx| {
            ctx.transition()
                .opacity(0.0)
                .duration(Duration::from_millis(100))
                .ease(crate::Easing::Linear);
        })
        .unwrap();
        tick(&mut cx, 0);
        assert_eq!(logical_opacity(&cx, id), None);
        assert!((alpha(&cx, id) - 1.0).abs() < 1e-5);
        tick(&mut cx, 50);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4);
        tick(&mut cx, 100);
        assert!((alpha(&cx, id) - 0.0).abs() < 1e-5);

        cx.update_component(button, |_, ctx| {
            ctx.node().motion(Spring::to(1.0)).opacity();
        })
        .unwrap();
        tick(&mut cx, 100);
        let spring_start = alpha(&cx, id);
        tick(&mut cx, 250);
        let spring_later = alpha(&cx, id);
        assert!(spring_later > spring_start);

        let track = MotionTrack::transition(
            MotionTrackId::new(1).unwrap(),
            MotionTargetId::new(1).unwrap(),
            crate::AnimatableProperty::Opacity,
            crate::MotionValue::Scalar(1.0),
            crate::MotionValue::Scalar(0.25),
            MotionTiming::new(
                Duration::ZERO,
                Duration::from_millis(80),
                Duration::from_millis(16),
            ),
            MotionCurve::Easing(crate::Easing::Linear),
            AnimationPlayback::default(),
        );
        cx.update_component(button, |_, ctx| {
            ctx.node()
                .timeline(Timeline::parallel([MotionGraph::track(track)]));
        })
        .unwrap();
        tick(&mut cx, 250);
        tick(&mut cx, 290);
        let sequenced = alpha(&cx, id);
        assert!(sequenced < 1.0 && sequenced > 0.25);
    }

    #[test]
    fn zero_duration_l3_transition_snaps_without_style_work_and_retargets() {
        fn fade(cx: &mut AppContext, id: StableNodeId, now_ms: u64, to: f32, duration_ms: u64) {
            let mut queue = MutationQueue::new();
            queue
                .node(id, Duration::from_millis(now_ms))
                .transition()
                .opacity(to)
                .duration(Duration::from_millis(duration_ms))
                .ease(crate::Easing::Linear)
                .start();
            cx.commit_mutations(queue).unwrap();
        }

        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Chrome")).unwrap();
        let id = button.stable_id();
        tick(&mut cx, 0);

        fade(&mut cx, id, 0, 0.0, 100);
        tick(&mut cx, 50);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4);

        fade(&mut cx, id, 50, 0.25, 0);
        let work = cx.take_system_work();
        cx.resolve_styles(&work.style).unwrap();
        cx.advance_animations(Duration::from_millis(50));
        let work = cx.take_system_work();
        assert!(work.layout.is_empty() && work.style.is_empty());
        assert_eq!(alpha(&cx, id), 0.25);
        assert_eq!(logical_opacity(&cx, id), None);
        assert_eq!(cx.next_animation_deadline(), None);
        tick(&mut cx, 200);
        assert_eq!(alpha(&cx, id), 0.25, "the interrupted run must not resume");

        fade(&mut cx, id, 200, 1.0, 100);
        tick(&mut cx, 250);
        assert!((alpha(&cx, id) - 0.625).abs() < 1e-4);
    }

    /// An L3 transition holds its target over the logical style once it ends,
    /// but only until the property is written again with another value: the
    /// later write shows as written, not under a stale hold. Writing the same
    /// value back, as a component re-projecting its style does, keeps it.
    #[test]
    fn a_style_write_after_an_l3_transition_shows_as_written() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Fade")).unwrap();
        let id = button.stable_id();
        tick(&mut cx, 0);
        let mut queue = MutationQueue::new();
        queue
            .node(id, Duration::ZERO)
            .transition()
            .opacity(0.0)
            .duration(Duration::from_millis(100))
            .ease(crate::Easing::Linear)
            .start();
        cx.commit_mutations(queue).unwrap();
        tick(&mut cx, 100);
        assert_eq!(alpha(&cx, id), 0.0);
        cx.reproject_component(button).unwrap();
        tick(&mut cx, 110);
        assert_eq!(alpha(&cx, id), 0.0, "a re-projection is not a new write");

        let mut style = cx.world().node_style(id).unwrap().clone();
        Arc::make_mut(&mut style.layout).opacity = Some(1.0);
        let mut write = MutationQueue::new();
        write.set_style(id, style);
        cx.commit_mutations(write).unwrap();
        tick(&mut cx, 120);
        assert_eq!(alpha(&cx, id), 1.0);
    }

    /// An L3 transition whose start value comes from the logical style still
    /// waits out its own delay.
    #[test]
    fn an_l3_transition_keeps_its_delay() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Later")).unwrap();
        let id = button.stable_id();
        tick(&mut cx, 0);
        let mut queue = MutationQueue::new();
        queue
            .node(id, Duration::ZERO)
            .transition()
            .opacity(0.0)
            .duration(Duration::from_millis(100))
            .delay(Duration::from_millis(50))
            .ease(crate::Easing::Linear)
            .start();
        cx.commit_mutations(queue).unwrap();
        tick(&mut cx, 25);
        assert_eq!(alpha(&cx, id), 1.0, "still in the delay");
        tick(&mut cx, 100);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4);
        tick(&mut cx, 150);
        assert_eq!(alpha(&cx, id), 0.0);
    }

    fn fade_after(cx: &mut AppContext, id: StableNodeId, now_ms: u64, to: f32, timing: [u64; 2]) {
        let [duration_ms, delay_ms] = timing;
        let mut queue = MutationQueue::new();
        queue
            .node(id, Duration::from_millis(now_ms))
            .transition()
            .opacity(to)
            .duration(Duration::from_millis(duration_ms))
            .delay(Duration::from_millis(delay_ms))
            .ease(crate::Easing::Linear)
            .start();
        cx.commit_mutations(queue).unwrap();
    }

    /// A transition that starts from a finished run's hold keeps its delay:
    /// nothing is moving, so the held value shows until the run starts from
    /// it. A stagger of reveals from a hidden hold must not all play at once.
    #[test]
    fn an_l3_transition_from_a_settled_hold_keeps_its_delay() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Stagger")).unwrap();
        let id = button.stable_id();
        tick(&mut cx, 0);

        fade_after(&mut cx, id, 0, 0.0, [0, 0]);
        tick(&mut cx, 10);
        assert_eq!(alpha(&cx, id), 0.0);
        assert_eq!(cx.next_animation_deadline(), None, "the hide has ended");

        fade_after(&mut cx, id, 100, 1.0, [100, 70]);
        tick(&mut cx, 130);
        assert_eq!(alpha(&cx, id), 0.0, "still in the delay, on the hold");
        tick(&mut cx, 169);
        assert_eq!(alpha(&cx, id), 0.0, "still in the delay, on the hold");
        tick(&mut cx, 220);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4, "{}", alpha(&cx, id));
        tick(&mut cx, 270);
        assert_eq!(alpha(&cx, id), 1.0);

        // Hidden and revealed in one frame: the zero-length hide has ended
        // though no frame has reaped it yet, so it is a hold too.
        fade_after(&mut cx, id, 300, 0.0, [0, 0]);
        fade_after(&mut cx, id, 300, 1.0, [100, 70]);
        tick(&mut cx, 330);
        assert_eq!(alpha(&cx, id), 0.0, "still in the delay, on the hold");
        tick(&mut cx, 420);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4, "{}", alpha(&cx, id));
        tick(&mut cx, 470);
        assert_eq!(alpha(&cx, id), 1.0);
    }

    /// A transition that retargets one in flight carries on from where it is
    /// at once: its delay would freeze a moving value.
    #[test]
    fn an_l3_transition_retargeting_one_in_flight_drops_its_delay() {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Turn")).unwrap();
        let id = button.stable_id();
        tick(&mut cx, 0);

        fade_after(&mut cx, id, 0, 0.0, [100, 0]);
        tick(&mut cx, 50);
        assert!((alpha(&cx, id) - 0.5).abs() < 1e-4);

        fade_after(&mut cx, id, 50, 1.0, [100, 70]);
        tick(&mut cx, 100);
        assert!((alpha(&cx, id) - 0.75).abs() < 1e-4, "{}", alpha(&cx, id));
        tick(&mut cx, 150);
        assert_eq!(alpha(&cx, id), 1.0);
    }

    /// A sequence that touches one property twice compiles to two tracks. Both
    /// have to install: keying the animation on `(node, property)` alone lets
    /// the second overwrite the first in the same batch, so the opening stage
    /// never plays at all.
    #[test]
    fn a_sequence_on_one_property_plays_every_stage() {
        fn stage(track: u64, start_ms: u64, duration_ms: u64, from: f32, to: f32) -> MotionTrack {
            MotionTrack::transition(
                MotionTrackId::new(track).unwrap(),
                MotionTargetId::new(1).unwrap(),
                crate::AnimatableProperty::Opacity,
                crate::MotionValue::Scalar(from),
                crate::MotionValue::Scalar(to),
                MotionTiming::new(
                    Duration::from_millis(start_ms),
                    Duration::from_millis(duration_ms),
                    Duration::from_millis(16),
                ),
                MotionCurve::Easing(crate::Easing::Linear),
                AnimationPlayback::default(),
            )
        }

        use crate::{
            AnimationPlayback, MotionCurve, MotionGraph, MotionTargetId, MotionTiming, MotionTrack,
            MotionTrackId, Timeline,
        };

        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let button = cx.create_component(doc, Button::new("Motion")).unwrap();
        let id = button.stable_id();
        cx.update_component(button, |_, ctx| {
            ctx.node().timeline(Timeline::sequence([
                MotionGraph::track(stage(1, 0, 100, 0.0, 1.0)),
                MotionGraph::track(stage(2, 0, 100, 1.0, 0.0)),
            ]));
        })
        .unwrap();

        tick(&mut cx, 50);
        let opening = alpha(&cx, id);
        assert!(
            opening > 0.0 && opening < 1.0,
            "the first stage never ran: {opening}"
        );
        tick(&mut cx, 150);
        let closing = alpha(&cx, id);
        assert!(
            closing > 0.0 && closing < 1.0,
            "the second stage never ran: {closing}"
        );
        tick(&mut cx, 190);
        let nearly_closed = alpha(&cx, id);
        assert!(
            nearly_closed < closing,
            "the second stage is the closing one: {closing} -> {nearly_closed}"
        );
        assert!(
            nearly_closed < 0.2,
            "and it is nearly done: {nearly_closed}"
        );
    }

    use crate::MotionTo;

    const BEVL: [u8; 4] = *b"BEVL";
    const WDTH: [u8; 4] = *b"wdth";

    /// A paragraph that declares `BEVL 0, wdth 100`, with a text child that
    /// inherits them.
    fn axis_world() -> (UiWorld, StableNodeId, StableNodeId) {
        let mut world = UiWorld::new();
        let doc = DocumentId::new(1).unwrap();
        let root = StableNodeId::new(1).unwrap();
        let paragraph = StableNodeId::new(2).unwrap();
        let text = StableNodeId::new(3).unwrap();
        let mut create = MutationQueue::new();
        create.create(root, doc, NodeKind::Document);
        create.create(paragraph, doc, NodeKind::Element { tag: "p".into() });
        create.create(text, doc, NodeKind::Text);
        create.insert(root, paragraph, None);
        create.insert(paragraph, text, None);
        create.set_text(
            text,
            TextContent {
                value: "Axis".into(),
            },
        );
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).font_variation_settings = Some(vec![
            nana_ui_core::FontVariationSetting::new(BEVL, 0.0),
            nana_ui_core::FontVariationSetting::new(WDTH, 100.0),
        ]);
        create.set_style(paragraph, style);
        world.commit(create).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        (world, paragraph, text)
    }

    fn computed_axis(world: &UiWorld, id: StableNodeId, tag: [u8; 4]) -> Option<f32> {
        nana_ui_core::FontVariationSetting::axis_value(
            &world.computed_style(id).unwrap().font_variations,
            tag,
        )
    }

    fn authored_axis(world: &UiWorld, id: StableNodeId, tag: [u8; 4]) -> Option<f32> {
        world
            .node_style(id)
            .and_then(|style| style.layout.font_variation_settings.as_deref())
            .and_then(|axes| nana_ui_core::FontVariationSetting::axis_value(axes, tag))
    }

    fn axis_transition(world: &mut UiWorld, id: StableNodeId, now_ms: u64, tag: [u8; 4], to: f32) {
        let mut queue = MutationQueue::new();
        queue
            .node(id, Duration::from_millis(now_ms))
            .transition()
            .font_axis(tag, to)
            .duration(Duration::from_millis(100))
            .ease(crate::Easing::Linear)
            .start();
        world.commit(queue).unwrap();
    }

    /// Issue #85: a font-axis track is a Motion track whose samples land in
    /// the computed text style — the node's and every inheriting
    /// descendant's — and cost what the #88 dirty graph says an axis change
    /// costs: shaping and layout, never a compositor overlay.
    #[test]
    fn a_font_axis_sample_reaches_the_text_style_and_reshapes_its_text() {
        let (mut world, paragraph, text) = axis_world();
        axis_transition(&mut world, paragraph, 0, BEVL, 100.0);
        let mut shape = world.text_revisions(text).unwrap().shape;
        for (ms, expected) in [(0, 0.0), (50, 50.0), (100, 100.0)] {
            world.advance_animations(Duration::from_millis(ms));
            let work = world.take_system_work();
            for id in [paragraph, text] {
                assert!(work.style.contains(&id), "{ms}ms: style");
                assert!(work.layout.contains(&id), "{ms}ms: layout");
            }
            assert!(work.text.contains(&text), "{ms}ms: text");
            world.resolve_styles(&work.style).unwrap();
            for id in [paragraph, text] {
                let got = computed_axis(&world, id, BEVL).unwrap();
                assert!((got - expected).abs() < 1e-3, "{ms}ms: {got}");
                assert_eq!(computed_axis(&world, id, WDTH), Some(100.0));
            }
            let revisions = world.text_revisions(text).unwrap();
            if ms > 0 {
                assert_ne!(revisions.shape, shape, "{ms}ms: the text reshapes");
            }
            shape = revisions.shape;
            assert_eq!(
                authored_axis(&world, paragraph, BEVL),
                Some(0.0),
                "the authored axes stay the author's"
            );
            if ms < 100 {
                let inspected = world
                    .inspect_motion()
                    .into_iter()
                    .find(|entry| entry.property == crate::AnimatableProperty::FontAxis(BEVL))
                    .expect("the axis track is inspectable");
                assert_eq!(inspected.class, crate::AnimationClass::Layout);
                assert_eq!(inspected.evaluator, crate::MotionEvaluatorBackend::Cpu);
                assert_eq!(
                    inspected.gpu_handle.filter(|handle| !handle.is_null()),
                    None,
                    "a glyph variation is not a compositor descriptor"
                );
                assert_eq!(inspected.base, Some(crate::MotionValue::Scalar(0.0)));
                assert!(
                    matches!(inspected.presentation, Some(crate::MotionValue::Scalar(v)) if (v - expected).abs() < 1e-3)
                );
            }
        }
        assert_eq!(world.next_animation_deadline(), None);
        tick_world(&mut world, 1000);
        assert_eq!(computed_axis(&world, text, BEVL), Some(100.0));
        assert_eq!(computed_axis(&world, text, WDTH), Some(100.0));
        assert_eq!(
            authored_axis(&world, paragraph, BEVL),
            Some(0.0),
            "held, not written"
        );
        assert_eq!(
            world
                .node_style(text)
                .unwrap()
                .layout
                .font_variation_settings,
            None,
            "the child still inherits"
        );
    }

    /// A hold yields to the next write of its axis, and the next run starts
    /// from what that write shows.
    #[test]
    fn after_a_run_the_style_owns_the_axis_again() {
        let (mut world, paragraph, text) = axis_world();
        axis_transition(&mut world, paragraph, 0, BEVL, 100.0);
        tick_world(&mut world, 100);
        assert_eq!(computed_axis(&world, text, BEVL), Some(100.0));

        let mut write = MutationQueue::new();
        let mut style = world.node_style(paragraph).unwrap().clone();
        Arc::make_mut(&mut style.layout).font_variation_settings = Some(vec![
            nana_ui_core::FontVariationSetting::new(BEVL, 30.0),
            nana_ui_core::FontVariationSetting::new(WDTH, 100.0),
        ]);
        write.set_style(paragraph, style);
        world.commit(write).unwrap();
        tick_world(&mut world, 150);
        assert_eq!(computed_axis(&world, text, BEVL), Some(30.0));

        axis_transition(&mut world, paragraph, 200, BEVL, 0.0);
        tick_world(&mut world, 200);
        assert_eq!(
            computed_axis(&world, text, BEVL),
            Some(30.0),
            "no jump at the start"
        );
        tick_world(&mut world, 250);
        assert!((computed_axis(&world, text, BEVL).unwrap() - 15.0).abs() < 1e-3);
    }

    #[test]
    fn font_axes_are_separate_tracks_and_retarget_from_their_current_value() {
        let (mut world, paragraph, _) = axis_world();
        let mut queue = MutationQueue::new();
        queue
            .node(paragraph, Duration::ZERO)
            .transition()
            .font_axis(BEVL, 100.0)
            .font_axis(WDTH, 200.0)
            .duration(Duration::from_millis(100))
            .ease(crate::Easing::Linear)
            .start();
        world.commit(queue).unwrap();
        tick_world(&mut world, 50);
        assert!((computed_axis(&world, paragraph, BEVL).unwrap() - 50.0).abs() < 1e-3);
        assert!((computed_axis(&world, paragraph, WDTH).unwrap() - 150.0).abs() < 1e-3);

        // Retargeting `BEVL` leaves the `wdth` track alone and starts from
        // where `BEVL` is, not from its authored value.
        axis_transition(&mut world, paragraph, 50, BEVL, 0.0);
        tick_world(&mut world, 50);
        assert!((computed_axis(&world, paragraph, BEVL).unwrap() - 50.0).abs() < 1e-3);
        tick_world(&mut world, 100);
        assert!((computed_axis(&world, paragraph, BEVL).unwrap() - 25.0).abs() < 1e-3);
        assert!((computed_axis(&world, paragraph, WDTH).unwrap() - 200.0).abs() < 1e-3);
    }

    /// No style says what an axis the list does not name starts at — only the
    /// face knows its default — so there is nothing to interpolate from, and
    /// the axis takes its value at once instead of an invented start.
    #[test]
    fn an_undeclared_axis_takes_its_value_without_an_invented_start() {
        let (mut world, paragraph, text) = axis_world();
        axis_transition(&mut world, paragraph, 0, *b"opsz", 24.0);
        tick_world(&mut world, 0);
        assert_eq!(computed_axis(&world, text, *b"opsz"), Some(24.0));
        assert_eq!(computed_axis(&world, text, *b"wght"), None);
        tick_world(&mut world, 100);
        assert_eq!(computed_axis(&world, text, *b"opsz"), Some(24.0));
        assert_eq!(authored_axis(&world, paragraph, *b"opsz"), None);
    }

    /// Stopping a track removes its overlay; the style has to resolve again
    /// or the text keeps the last sample forever.
    #[test]
    fn a_cancelled_run_falls_back_to_the_authored_axes() {
        let (mut world, paragraph, text) = axis_world();
        axis_transition(&mut world, paragraph, 0, BEVL, 100.0);
        tick_world(&mut world, 50);
        assert!((computed_axis(&world, text, BEVL).unwrap() - 50.0).abs() < 1e-3);
        let id = *world
            .animations
            .keys()
            .next()
            .expect("the axis track is active");
        let mut stop = MutationQueue::new();
        stop.stop_animation(id);
        world.commit(stop).unwrap();
        let work = world.take_system_work();
        assert!(work.text.contains(&text));
        world.resolve_styles(&work.style).unwrap();
        assert_eq!(computed_axis(&world, text, BEVL), Some(0.0));
        assert_eq!(authored_axis(&world, paragraph, BEVL), Some(0.0));
    }

    fn axis_track(id: u64, from: f32, to: MotionTo, start_ms: u64) -> crate::MotionTrack {
        crate::MotionTrack {
            id: crate::MotionTrackId::new(id).unwrap(),
            target: crate::MotionTargetId::new(2).unwrap(),
            property: crate::AnimatableProperty::FontAxis(BEVL),
            from: crate::MotionValue::Scalar(from),
            to,
            timing: crate::MotionTiming::new(
                Duration::from_millis(start_ms),
                Duration::from_millis(100),
                Duration::from_millis(16),
            ),
            curve: crate::MotionCurve::Easing(crate::Easing::Linear),
            playback: crate::AnimationPlayback::running(
                crate::AnimationIteration::ONCE,
                crate::AnimationDirection::Normal,
                crate::AnimationFillMode::Forwards,
            ),
            velocity: crate::MotionValue::Scalar(0.0),
        }
    }

    /// A sequence may move one axis twice. The second stage starts later, so
    /// it is over the hold the first stage left, whatever their ids hash to.
    #[test]
    fn a_later_stage_on_an_axis_wins_over_an_earlier_hold() {
        use crate::{MotionGraph, Timeline};
        for (first, second) in [(1, 2), (2, 1), (9, 3)] {
            let (mut world, paragraph, text) = axis_world();
            let mut queue = MutationQueue::new();
            queue
                .node(paragraph, Duration::ZERO)
                .timeline(Timeline::sequence([
                    MotionGraph::track(axis_track(
                        first,
                        0.0,
                        MotionTo::Value(crate::MotionValue::Scalar(100.0)),
                        0,
                    )),
                    MotionGraph::track(axis_track(
                        second,
                        100.0,
                        MotionTo::Value(crate::MotionValue::Scalar(20.0)),
                        0,
                    )),
                ]))
                .start();
            world.commit(queue).unwrap();
            tick_world(&mut world, 100);
            assert_eq!(computed_axis(&world, text, BEVL), Some(100.0));
            tick_world(&mut world, 150);
            assert!(
                (computed_axis(&world, text, BEVL).unwrap() - 60.0).abs() < 1e-3,
                "ids {first}/{second}: the running stage shows"
            );
            tick_world(&mut world, 300);
            assert_eq!(
                computed_axis(&world, text, BEVL),
                Some(20.0),
                "ids {first}/{second}: the last stage's end stays"
            );
        }
    }

    #[test]
    fn stopping_a_finished_run_gives_its_axis_back() {
        use crate::{MotionGraph, Timeline};
        let (mut world, paragraph, text) = axis_world();
        let mut queue = MutationQueue::new();
        queue
            .node(paragraph, Duration::ZERO)
            .timeline(Timeline::parallel([MotionGraph::track(axis_track(
                3,
                0.0,
                MotionTo::Value(crate::MotionValue::Scalar(100.0)),
                0,
            ))]))
            .start();
        world.commit(queue).unwrap();
        tick_world(&mut world, 100);
        assert_eq!(computed_axis(&world, text, BEVL), Some(100.0));
        assert_eq!(
            authored_axis(&world, paragraph, BEVL),
            Some(0.0),
            "an effect, not a write"
        );
        let held = world
            .presentation_store()
            .overlays()
            .map(|overlay| AnimationId::new(overlay.track.id.get()).unwrap())
            .next()
            .expect("a hold");
        assert!(world.animation_is_held(held));
        let mut stop = MutationQueue::new();
        stop.stop_animation(held);
        world.commit(stop).unwrap();
        let work = world.take_system_work();
        assert!(work.text.contains(&text), "releasing a hold reshapes");
        world.resolve_styles(&work.style).unwrap();
        assert_eq!(computed_axis(&world, text, BEVL), Some(0.0));
        assert!(!world.animation_is_held(held));
    }

    /// Finishing early holds the value the run would have ended on. Two
    /// alternating runs end where they began, not on the last keyframe.
    #[test]
    fn finishing_an_alternating_run_holds_where_it_ends() {
        use crate::{Keyframe, MotionGraph, Timeline};
        let (mut world, paragraph, text) = axis_world();
        let stop = |offset: f32, value: f32| Keyframe {
            offset,
            value: crate::MotionValue::Scalar(value),
            easing: None,
        };
        let mut track = axis_track(
            5,
            0.0,
            MotionTo::Keyframes(vec![stop(0.0, 0.0), stop(1.0, 80.0)]),
            0,
        );
        track.playback.iteration_count = crate::AnimationIteration::Count(2);
        track.playback.direction = crate::AnimationDirection::Alternate;
        let mut queue = MutationQueue::new();
        queue
            .node(paragraph, Duration::ZERO)
            .timeline(Timeline::parallel([MotionGraph::track(track)]))
            .start();
        world.commit(queue).unwrap();
        tick_world(&mut world, 50);
        let id = *world.animations.keys().next().expect("running");
        let mut finish = MutationQueue::new();
        finish.finish_animation(id);
        world.commit(finish).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        assert_eq!(computed_axis(&world, text, BEVL), Some(0.0));
    }

    /// As in the CSS cascade, a transition on an axis wins over an animation
    /// on it, whichever started later.
    #[test]
    fn a_css_transition_on_an_axis_wins_over_a_css_animation() {
        let (mut world, paragraph, text) = axis_world();
        let spec = |id: u64, start: u64, to: f32, layer: crate::MotionLayer| {
            crate::AnimationSpec::new(
                AnimationId::new(id).unwrap(),
                paragraph,
                Duration::from_millis(start),
                Duration::from_millis(100),
                Duration::from_millis(16),
                crate::Easing::Linear,
            )
            .with_property(crate::AnimatableProperty::FontAxis(BEVL))
            .with_range(
                crate::MotionValue::Scalar(0.0),
                MotionTo::Value(crate::MotionValue::Scalar(to)),
            )
            .with_layer(layer)
        };
        let mut queue = MutationQueue::new();
        queue.start_animation(spec(1, 0, 100.0, crate::MotionLayer::CssTransition));
        queue.start_animation(spec(2, 20, 50.0, crate::MotionLayer::CssAnimation));
        world.commit(queue).unwrap();
        tick_world(&mut world, 50);
        assert!((computed_axis(&world, text, BEVL).unwrap() - 50.0).abs() < 1e-3);
        tick_world(&mut world, 110);
        // The transition is over; the animation still runs and now shows.
        assert!((computed_axis(&world, text, BEVL).unwrap() - 45.0).abs() < 1e-3);
    }

    /// A later run on an axis holds over an earlier run's hold: its end is
    /// what stays, not the older value resurfacing.
    #[test]
    fn a_later_run_is_not_undone_by_an_earlier_hold() {
        use crate::{MotionGraph, Timeline};
        let (mut world, paragraph, text) = axis_world();
        let mut queue = MutationQueue::new();
        queue
            .node(paragraph, Duration::ZERO)
            .timeline(Timeline::parallel([MotionGraph::track(axis_track(
                4,
                0.0,
                MotionTo::Value(crate::MotionValue::Scalar(20.0)),
                0,
            ))]))
            .start();
        world.commit(queue).unwrap();
        tick_world(&mut world, 100);
        assert_eq!(computed_axis(&world, text, BEVL), Some(20.0));
        axis_transition(&mut world, paragraph, 150, BEVL, 80.0);
        tick_world(&mut world, 200);
        assert!((computed_axis(&world, text, BEVL).unwrap() - 50.0).abs() < 1e-3);
        tick_world(&mut world, 300);
        assert_eq!(computed_axis(&world, text, BEVL), Some(80.0));
    }

    /// A target the style would refuse is refused as the style refuses it,
    /// not clamped into something the author did not write.
    #[test]
    fn an_out_of_range_target_is_refused_not_clamped() {
        let (mut world, paragraph, _) = axis_world();
        let mut queue = MutationQueue::new();
        queue
            .node(paragraph, Duration::ZERO)
            .transition()
            .opacity(3.0)
            .duration(Duration::from_millis(100))
            .start();
        assert!(matches!(
            world.commit(queue),
            Err(UiWorldError::InvalidAnimation(_))
        ));
    }

    /// An overshooting curve may carry a width below zero on its way; the box
    /// stops at zero instead of laying out a negative size.
    #[test]
    fn an_overshooting_width_stops_at_zero() {
        let (mut world, paragraph, _) = axis_world();
        let spec = crate::AnimationSpec::new(
            AnimationId::new(77).unwrap(),
            paragraph,
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            // Out and past the end, then back: a "back-out" curve.
            crate::Easing::CubicBezier([0.3, 1.8, 0.6, 1.0]),
        )
        .with_property(crate::AnimatableProperty::Width)
        .with_range(
            crate::MotionValue::Scalar(10.0),
            MotionTo::Value(crate::MotionValue::Scalar(0.0)),
        );
        let mut queue = MutationQueue::new();
        queue.start_animation(spec);
        world.commit(queue).unwrap();
        let mut lowest = f32::MAX;
        for ms in (0..=100).step_by(10) {
            tick_world(&mut world, ms);
            if let Some(LengthSpec::Px(px)) = world.node_style(paragraph).unwrap().layout.width {
                lowest = lowest.min(px);
            }
        }
        assert_eq!(lowest, 0.0, "reached zero and went no further");
    }

    fn tick_world(world: &mut UiWorld, ms: u64) {
        world.advance_animations(Duration::from_millis(ms));
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
    }
}
