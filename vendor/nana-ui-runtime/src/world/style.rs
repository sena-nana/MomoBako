//! Style resolution and semantic palette inheritance.

use super::*;

impl UiWorld {
    fn resolve_style<const SHARE: bool>(
        &mut self,
        id: StableNodeId,
        resolved: &mut HashSet<StableNodeId>,
        work: &mut ThemeWorkCounters,
    ) -> Result<(), UiWorldError> {
        if !self.contains(id) {
            return Err(UiWorldError::MissingNode(id));
        }
        if !resolved.insert(id) {
            return Ok(());
        }
        let parent = self.record(id).hierarchy.parent;
        if let Some(parent) = parent {
            self.resolve_style::<SHARE>(parent, resolved, work)?;
        }
        let layout = self.motion_layout(id, &self.record(id).style.layout);
        // Only a handful of fields are read out of the parent, so share its Arc
        // instead of cloning the whole `ComputedStyle` (and its three heap
        // fields) once per node. A borrow would pin `&mut self` to the end.
        let inherited_style = parent
            .map(|parent| Arc::clone(&self.record(parent).resolved.0))
            .unwrap_or_else(crate::store::interned_default_style);
        let inherited = inherited_style.as_ref();
        let inherited_color = parent.and(inherited.color);
        let (foreground, color, background, border_color) =
            self.palette_paint_colors(id, inherited_color);
        let visibility = layout.paint.visibility.unwrap_or(inherited.visibility);
        // Overlay/menu presence is structural: descendants cannot make a closed
        // branch paintable again by resolving their own local visibility.
        // An inactive retained branch therefore omits its whole subtree just
        // like display:none. Keeping this only in `visible` would let
        // descendants inherit a live box and leave their paint primitives
        // behind on close.
        let box_visible = !layout.omits_box()
            && inherited.box_visible
            && self.overlay_branch_active(id)
            && self.menu_branch_open(id);
        let pointer_events =
            PointerEventsSpec::inherit_from(layout.pointer_events, inherited.pointer_events);
        let next = ComputedStyle {
            foreground,
            color,
            background,
            border_color,
            opacity: layout.opacity.unwrap_or(1.0) * inherited.opacity,
            visibility,
            box_visible,
            visible: box_visible && visibility != nana_ui_core::VisibilitySpec::Hidden,
            pointer_events,
            cursor: layout.cursor.unwrap_or(inherited.cursor),
            cursor_specified: layout.cursor.is_some() || inherited.cursor_specified,
            user_select: layout.user_select.unwrap_or(inherited.user_select),
            selection_background: layout
                .selection_background
                .or(inherited.selection_background),
            selection_color: layout.selection_color.or(inherited.selection_color),
            font_size: layout.font_size.unwrap_or(inherited.font_size),
            font_weight: layout.font_weight.or(inherited.font_weight),
            italic: layout.font_italic.unwrap_or(inherited.italic),
            font_family: layout
                .font_family
                .as_deref()
                .map(Arc::<str>::from)
                .or_else(|| inherited.font_family.clone()),
            line_height: layout.line_height.or(inherited.line_height),
            letter_spacing: layout.letter_spacing.unwrap_or(inherited.letter_spacing),
            font_features: layout
                .font_features
                .clone()
                .unwrap_or_else(|| inherited.font_features.clone()),
            font_variations: {
                let mut axes = layout
                    .font_variation_settings
                    .clone()
                    .unwrap_or_else(|| inherited.font_variations.clone());
                for (tag, value) in self.font_axis_overlay(id) {
                    match value {
                        Some(value) => {
                            nana_ui_core::FontVariationSetting::set_axis(&mut axes, tag, value)
                        }
                        None => axes.retain(|axis| axis.tag != tag),
                    }
                }
                axes
            },
            font_kerning: layout.font_kerning.unwrap_or(inherited.font_kerning),
            word_break: layout.word_break.unwrap_or(inherited.word_break),
            line_break: layout.line_break.unwrap_or(inherited.line_break),
            direction: layout.dir.unwrap_or(inherited.direction),
            writing_mode: layout.writing_mode.unwrap_or(inherited.writing_mode),
            text_orientation: layout
                .text_orientation
                .unwrap_or(inherited.text_orientation),
        };
        // Written before the early return: a node that declares its own
        // writing mode resolves to the same style when its parent's changes,
        // but its containing block is still the parent.
        // The computed values, not the used context: `upright` makes this
        // node's used direction `ltr`, but its children inherit `direction`.
        let record = self.record_mut(id);
        (record.inherited_writing, record.inherited_orientation) = if parent.is_some() {
            (
                nana_ui_core::WritingContext::new(inherited.writing_mode, inherited.direction),
                inherited.text_orientation,
            )
        } else {
            Default::default()
        };
        {
            let resolved = &self.record(id).resolved;
            if resolved.0.as_ref() == &next && resolved.1 == self.palette_epoch {
                work.record_skipped();
                return Ok(());
            }
        }
        work.record_resolved();
        // Identical inherited results can share immutable paint state. Local
        // authored style remains on the node; future changes publish a new Arc.
        let shared = if SHARE {
            parent
                .map(|parent| &self.record(parent).resolved.0)
                .filter(|inherited| inherited.as_ref() == &next)
                .map(Arc::clone)
        } else {
            None
        };
        let next = match shared {
            Some(shared) => shared,
            None => {
                // One `ComputedStyle` behind one `Arc`. A node that inherits
                // its parent's result verbatim borrows that allocation instead.
                work.record_allocation(1, size_of::<ComputedStyle>());
                Arc::new(next)
            }
        };
        let dirty =
            crate::text_node::classify_computed_style_change(&self.record(id).resolved.0, &next);
        // What this class costs the text pipeline is `TextDirty::work`'s
        // answer, not a second copy of that mapping here. A colour-only change
        // implies SCENE_PAINT, so a palette switch stays paint work.
        let text_work = dirty.work();
        if text_work.intersects(crate::text_node::TextWork::SHAPE)
            || text_work.intersects(crate::text_node::TextWork::LAYOUT)
        {
            work.record_text_invalidation(1);
        }
        self.nodes.invalidate_text(id, dirty);
        if !self.record(id).resolved.0.visible && next.visible {
            // Text passes skip hidden nodes without resolving them, so a node
            // whose box changed while hidden comes back stale. The scheduled
            // text pass of this same frame picks it up; marking it dirty here,
            // after the drain, would cost the frame another pass.
            self.text_shown.push(id);
        }
        self.record_mut(id).resolved = ResolvedStyle(next, self.palette_epoch);
        Ok(())
    }
}

impl UiWorld {
    /// Apply a node's design intent to its layout, against the installed
    /// metrics. Returns the authored `Arc` untouched when there is nothing to
    /// resolve, so a node without intent costs nothing.
    /// Resolve a node's design intent into the layout box the pipeline reads.
    ///
    /// Returns the resolved layout and whether producing it had to copy the
    /// authored one. A node whose intent already matches what it authored —
    /// and every node with no intent at all — keeps sharing the same `Arc`.
    pub(crate) fn resolve_layout_intent(
        style: &NodeStyle,
        metrics: nana_ui_core::ThemeMetrics,
    ) -> (Arc<nana_ui_core::LayoutStyle>, bool) {
        let radius = style.radius.map(|tier| tier.resolve(metrics));
        let corners = style.corner_radii.map(|tiers| {
            tiers.map(|tier| {
                nana_ui_core::LengthSpec::Px(tier.map_or(0.0, |tier| tier.resolve(metrics)))
            })
        });
        let control = style.control_height.map(|height| {
            (
                matches!(height, nana_ui_core::ControlHeight::Exact(_)),
                nana_ui_core::LengthSpec::Px(height.resolve(metrics)),
            )
        });
        let padding_x = style
            .control_padding_x
            .map(|padding| nana_ui_core::LengthSpec::Px(padding.resolve(metrics)));
        let padding_y = style
            .control_padding_y
            .map(|padding| nana_ui_core::LengthSpec::Px(padding.resolve(metrics)));
        let surface = style.surface_padding.map(|padding| {
            (
                nana_ui_core::LengthSpec::Px(padding.resolve_x(metrics)),
                padding.resolve_y(metrics).map(nana_ui_core::LengthSpec::Px),
            )
        });
        let square = style
            .square
            .map(|size| nana_ui_core::LengthSpec::Px(size.resolve(metrics)));
        // CSS `aspect-ratio` fills an automatic height from a definite width;
        // it does not shrink `width:auto` from a definite height. A control
        // that names Exact height and a ratio still has to resolve the inline
        // size here, or a spent `width` from the last project would stick.
        let aspect_width = style.control_height.and_then(|height| {
            if !matches!(height, nana_ui_core::ControlHeight::Exact(_))
                || style.layout.width.is_some()
            {
                return None;
            }
            let ratio = style.layout.aspect_ratio?;
            if !ratio.is_finite() || ratio <= 0.0 {
                return None;
            }
            Some(nana_ui_core::LengthSpec::Px(
                height.resolve(metrics) * ratio,
            ))
        });
        let radius_settled = radius.is_none_or(|value| style.layout.border_radius == Some(value));
        let corners_settled =
            corners.is_none_or(|value| style.layout.paint.border_radii == Some(value));
        let padding_x_settled = padding_x.is_none_or(|length| {
            style.layout.padding_left == Some(length) && style.layout.padding_right == Some(length)
        });
        let padding_y_settled = padding_y.is_none_or(|length| {
            style.layout.padding_top == Some(length) && style.layout.padding_bottom == Some(length)
        });
        let surface_settled = surface.is_none_or(|(x, y)| {
            let horizontal = style.layout.padding.is_some()
                || (style.layout.padding_left == Some(x) && style.layout.padding_right == Some(x));
            let vertical = y.is_none_or(|y| {
                style.layout.padding.is_some()
                    || (style.layout.padding_top == Some(y)
                        && style.layout.padding_bottom == Some(y))
            });
            horizontal && vertical
        });
        let square_settled = square.is_none_or(|length| {
            style.layout.min_width == Some(length) && style.layout.min_height == Some(length)
        });
        let control_settled = control.is_none_or(|(exact, length)| {
            if exact {
                style.layout.height == Some(length)
            } else {
                style.layout.min_height == Some(length)
            }
        });
        let aspect_width_settled =
            aspect_width.is_none_or(|length| style.layout.width == Some(length));
        if radius_settled
            && corners_settled
            && control_settled
            && padding_x_settled
            && padding_y_settled
            && surface_settled
            && square_settled
            && aspect_width_settled
        {
            return (Arc::clone(&style.layout), false);
        }
        let mut layout = Arc::clone(&style.layout);
        let target = Arc::make_mut(&mut layout);
        if let Some(value) = radius {
            target.border_radius = Some(value);
        }
        if let Some(value) = corners {
            target.paint.border_radii = Some(value);
        }
        if let Some((exact, length)) = control {
            if exact {
                target.height = Some(length);
            } else {
                target.min_height = Some(length);
            }
        }
        if let Some(length) = padding_x {
            target.padding_left = Some(length);
            target.padding_right = Some(length);
        }
        if let Some(length) = padding_y {
            target.padding_top = Some(length);
            target.padding_bottom = Some(length);
        }
        if let Some((x, y)) = surface
            && target.padding.is_none()
        {
            target.padding_left.get_or_insert(x);
            target.padding_right.get_or_insert(x);
            if let Some(y) = y {
                target.padding_top.get_or_insert(y);
                target.padding_bottom.get_or_insert(y);
            }
        }
        if let Some(length) = square {
            target.min_width = Some(length);
            target.min_height = Some(length);
        }
        if let Some(length) = aspect_width {
            target.width = Some(length);
        }
        (layout, true)
    }

    /// Replace `layout` with an equal one written recently, if any.
    pub(crate) fn share_layout(&mut self, layout: &mut Arc<nana_ui_core::LayoutStyle>) {
        self.layouts.intern(layout);
    }

    /// Write a node's authored style and keep its resolved layout in step.
    ///
    /// The two have to move together: projection diffs against the authored
    /// style, while layout and extraction read the resolved one. Every path
    /// that writes `record.style` goes through here so the pair cannot drift.
    pub(crate) fn write_node_style(&mut self, id: StableNodeId, mut style: NodeStyle) {
        let current = &self.record(id).style.layout;
        if !Arc::ptr_eq(current, &style.layout) && **current == *style.layout {
            style.layout = Arc::clone(current);
        } else {
            self.layouts.intern(&mut style.layout);
        }
        let (mut resolved, copied) = Self::resolve_layout_intent(&style, self.style_model.metrics);
        if copied {
            self.record_resolved_layout_copy();
            self.layouts.intern(&mut resolved);
        }
        let depends_on_viewport = style.layout.depends_on_viewport();
        let record = self.record_mut(id);
        record.style = style;
        record.resolved_layout = resolved;
        record.layout_depends_on_viewport = depends_on_viewport;
    }

    /// Re-resolve one node's layout after its authored layout was mutated in
    /// place. A node without design intent keeps sharing the same `Arc`.
    pub(crate) fn refresh_resolved_layout(&mut self, id: StableNodeId) {
        let (mut resolved, copied) =
            Self::resolve_layout_intent(&self.record(id).style, self.style_model.metrics);
        if copied {
            self.record_resolved_layout_copy();
            self.layouts.intern(&mut resolved);
        }
        self.record_mut(id).resolved_layout = resolved;
    }

    /// Re-resolve every node's layout intent after a metrics install.
    fn reresolve_layout_intent(&mut self, ids: &[StableNodeId]) {
        let metrics = self.style_model.metrics;
        for &id in ids {
            let style = &self.record(id).style;
            if style.radius.is_none()
                && style.corner_radii.is_none()
                && style.control_height.is_none()
                && style.control_padding_x.is_none()
                && style.control_padding_y.is_none()
                && style.surface_padding.is_none()
                && style.square.is_none()
            {
                continue;
            }
            let (mut resolved, copied) =
                Self::resolve_layout_intent(&self.record(id).style, metrics);
            if copied {
                self.record_resolved_layout_copy();
                self.layouts.intern(&mut resolved);
            }
            self.record_mut(id).resolved_layout = resolved;
        }
    }

    /// Read one role out of the token authority, counted for Issue #101 §4.
    fn theme_color(&self, role: SemanticColorRole) -> [f32; 4] {
        self.record_theme_read();
        self.style_model.color(role).as_rgba_array()
    }

    /// Resolve a palette mix. One question asked of the theme, so one read —
    /// the roles it blends are the mix's own business.
    fn theme_mix(&self, mix: nana_ui_core::SemanticColorMix) -> [f32; 4] {
        self.record_theme_read();
        mix.resolve(self.style_model).as_rgba_array()
    }

    pub(super) fn palette_paint_colors(
        &self,
        id: StableNodeId,
        inherited_color: Option<[f32; 4]>,
    ) -> (
        SemanticColorRole,
        Option<[f32; 4]>,
        Option<[f32; 4]>,
        Option<[f32; 4]>,
    ) {
        let local = &self.record(id).style;
        let paint = self.semantic_paint(id, local);
        let layout = local.layout.as_ref();
        let parent = self.record(id).hierarchy.parent;
        let inherited_foreground = parent
            .map(|parent| self.record(parent).resolved.0.foreground)
            .unwrap_or(SemanticColorRole::Text);
        let foreground = paint.foreground.unwrap_or(inherited_foreground);
        let color = layout
            .color
            .or_else(|| paint.foreground_mix.map(|mix| self.theme_mix(mix)))
            .or_else(|| {
                paint
                    .foreground
                    .map(|role| self.theme_color(role))
                    .or(inherited_color)
                    .or_else(|| Some(self.theme_color(foreground)))
            });
        let background = layout
            .background
            .or_else(|| paint.background_mix.map(|mix| self.theme_mix(mix)))
            .or_else(|| paint.background.map(|role| self.theme_color(role)));
        let border_color = layout
            .resolved_border_color()
            .or_else(|| paint.border_mix.map(|mix| self.theme_mix(mix)))
            .or_else(|| paint.border.map(|role| self.theme_color(role)));
        if let Some(transition) = self.hover_transitions.get(&id) {
            let [color, background, border_color] = std::array::from_fn(|i| {
                interpolate_color(
                    transition.from[i],
                    [color, background, border_color][i],
                    transition.progress,
                )
            });
            return (foreground, color, background, border_color);
        }
        (foreground, color, background, border_color)
    }
}

impl UiWorld {
    pub(super) fn inherited_palette_color(
        &self,
        mut parent: Option<StableNodeId>,
    ) -> Option<[f32; 4]> {
        while let Some(id) = parent {
            let local = &self.record(id).style;
            if let Some(color) = local.layout.color {
                return Some(color);
            }
            let paint = self.semantic_paint(id, local);
            if let Some(mix) = paint.foreground_mix {
                return Some(self.theme_mix(mix));
            }
            if let Some(role) = paint.foreground {
                return Some(self.theme_color(role));
            }
            parent = self.record(id).hierarchy.parent;
        }
        None
    }
}

impl UiWorld {
    pub(super) fn semantic_paint(
        &self,
        id: StableNodeId,
        local: &NodeStyle,
    ) -> crate::SemanticPaint {
        let mut paint = crate::SemanticPaint {
            foreground: local.foreground,
            background: local.background,
            border: local.border,
            ..crate::SemanticPaint::default()
        }
        .overlay(local.interaction.base);
        let accessibility = &self.record(id).accessibility;
        let selected = accessibility.checked == Some(true)
            || accessibility.mixed
            || accessibility.selected == Some(true);
        if selected {
            paint = paint.overlay(local.interaction.selected);
        }
        if self
            .input
            .pointer_hover
            .values()
            .any(|target| *target == id)
        {
            paint = paint.overlay(
                if selected && !local.interaction.selected_hovered.is_empty() {
                    local.interaction.selected_hovered
                } else {
                    local.interaction.hovered
                },
            );
        }
        if self
            .input
            .pointer_press
            .values()
            .any(|target| *target == id)
        {
            paint = paint.overlay(
                if selected && !local.interaction.selected_pressed.is_empty() {
                    local.interaction.selected_pressed
                } else {
                    local.interaction.pressed
                },
            );
        }
        // `focus_visible`, not `focused`: a control that was clicked is focused
        // and does not say so.
        if self.focus_visible(self.record(id).document) == Some(id) {
            paint = paint.overlay(local.interaction.focused);
        }
        if accessibility.disabled && !accessibility.busy {
            paint = paint.overlay(local.interaction.disabled);
        }
        paint
    }

    /// The colour a component's quieter text resolves to for `id`, in its
    /// current interaction state.
    ///
    /// Component geometry used to read `palette.muted` straight off the
    /// palette for every detail line, hint, placeholder and unit. That skips
    /// the state the node is actually in: a focused row fills with
    /// `focus_surface` and its label follows to `focus_text`, while the pinned
    /// grey stays resolved against the surface the row no longer has. The
    /// role still defaults to `Muted`, so a component that says nothing paints
    /// exactly what it painted before.
    pub(super) fn secondary_text_color(&self, id: StableNodeId) -> [f32; 4] {
        let local = &self.record(id).style;
        let role = self
            .semantic_paint(id, local)
            .foreground_secondary
            .unwrap_or(SemanticColorRole::Muted);
        self.theme_color(role)
    }
}

impl UiWorld {
    /// Re-record a painted node. A painter whose outline is its hit shape
    /// takes its hit index entry with it: the entry holds the recording.
    /// A node gained or lost focus: its focus ring, IME and accessibility
    /// state, and a painter that paints focus.
    pub(super) fn mark_focus_changed(&mut self, id: StableNodeId) {
        self.mark(
            id,
            DirtyMask::FOCUS_IME | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
        );
        if self.node_painter(id).is_some() {
            self.mark_repaint(id);
        }
    }

    pub(super) fn mark_repaint(&mut self, id: StableNodeId) {
        let outline = self
            .node_painter(id)
            .is_some_and(|painter| painter.painter().hit_painted_outline());
        self.mark(
            id,
            if outline {
                DirtyMask::RENDER | DirtyMask::INPUT
            } else {
                DirtyMask::RENDER
            },
        );
    }

    pub(super) fn mark_interaction_style(&mut self, id: StableNodeId) {
        self.mark(id, DirtyMask::STATE);
        let record = self.record(id);
        // A painter reads the state it is in, so it re-records on it.
        let painted = record.style.painter.is_some()
            || (!self.painter_overrides.is_empty() && self.painter_overrides.contains_key(&id));
        let styled = !record.style.interaction.is_empty();
        if painted {
            self.mark_repaint(id);
        }
        if styled {
            let mut work = ThemeWorkCounters::default();
            work.record_paint_invalidation(1);
            self.record_theme_work(work);
            self.mark(id, DirtyMask::STYLE | DirtyMask::RENDER);
        }
    }
}

impl UiWorld {
    /// Resolve inherited visual state for dirty nodes. Parent state is always
    /// resolved before its descendants, independent of stable ID order.
    pub fn resolve_styles(&mut self, ids: &[StableNodeId]) -> Result<(), UiWorldError> {
        // Most frame batches already contain the full dirty frontier. Reserve
        // once so parent-chain deduplication does not repeatedly rehash large
        // initial documents.
        let mut resolved = HashSet::with_capacity(ids.len());
        let mut work = ThemeWorkCounters::default();
        for &id in ids {
            self.resolve_style::<true>(id, &mut resolved, &mut work)?;
        }
        self.record_theme_work(work);
        self.reconcile_focus(ids);
        self.drop_invalid_document_text_selections();
        Ok(())
    }
}

impl UiWorld {
    /// Diagnostic control path for paired sharing measurements; product resolution shares.
    #[cfg(feature = "benchmark")]
    pub fn benchmark_resolve_styles_unshared(
        &mut self,
        ids: &[StableNodeId],
    ) -> Result<(), UiWorldError> {
        let mut resolved = HashSet::new();
        let mut work = ThemeWorkCounters::default();
        for &id in ids {
            self.resolve_style::<false>(id, &mut resolved, &mut work)?;
        }
        self.record_theme_work(work);
        self.reconcile_focus(ids);
        Ok(())
    }

    pub(super) fn apply_compiled_theme(&mut self, next: Arc<nana_ui_core::CompiledTheme>) {
        // Values, not identity. `CompiledTheme::is_same_revision` is the cheap
        // question and it trusts the author's generation bump; an install has
        // to be right even for a theme that forgot to bump.
        if *self.theme == *next {
            return;
        }
        let hover_ids = self.hover_transitions.keys().copied().collect::<Vec<_>>();
        for id in hover_ids {
            self.cancel_hover_transition(id);
        }
        let previous_metrics = self.style_model.metrics;
        self.style_model = next.style_model();
        self.theme = next;
        self.palette_epoch = self.palette_epoch.wrapping_add(1).max(1);
        // Painters re-record against the new palette and radius tiers.
        let painted = self
            .paint_recordings
            .get_mut()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for id in painted {
            self.mark_repaint(id);
        }
        let mut bits = DirtyMask::RENDER;
        let metrics_changed = self.style_model.metrics != previous_metrics;
        if metrics_changed {
            bits |= DirtyMask::LAYOUT;
        }
        let mut ids = Vec::new();
        for roots in self.live_document_roots.values() {
            for &root in roots {
                ids.extend(self.subtree_ids(root));
            }
        }
        // Installing a theme today invalidates every live node (Issue #101 §4
        // baseline; Issue #100 §7 is where that becomes dependency-scoped).
        // Recording the real width is what makes the later narrowing visible.
        let mut work = ThemeWorkCounters::default();
        work.record_paint_invalidation(ids.len());
        if metrics_changed {
            work.record_layout_invalidation(ids.len());
        }
        if !ids.is_empty() {
            work.record_allocation(1, ids.len().saturating_mul(size_of::<StableNodeId>()));
        }
        self.record_theme_work(work);
        if metrics_changed {
            // Design intent resolves against the metrics, so a metrics install
            // is the one event that has to re-run it. Doing it here, once, is
            // what keeps it off every frame's read path.
            self.reresolve_layout_intent(&ids);
        }
        for id in ids {
            self.mark(id, bits);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct HoverTransition {
    pub from: [Option<[f32; 4]>; 3],
    /// Motion IR sample progress. Style interpolation reads this; it does not
    /// sample `animation_now` on its own clock.
    pub progress: f32,
    pub inherits_color: bool,
}

fn interpolate_color(
    from: Option<[f32; 4]>,
    to: Option<[f32; 4]>,
    progress: f32,
) -> Option<[f32; 4]> {
    if progress >= 1.0 {
        return to;
    }
    if progress <= 0.0 {
        return from;
    }
    let (mut a, mut b) = match (from, to) {
        (None, None) => return None,
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => (a, a),
        (None, Some(b)) => (b, b),
    };
    if from.is_none() {
        a[3] = 0.0;
    }
    if to.is_none() {
        b[3] = 0.0;
    }
    Some(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * progress))
}

impl UiWorld {
    pub(super) fn hover_paint(&self, id: StableNodeId) -> [Option<[f32; 4]>; 3] {
        let inherited = self
            .record(id)
            .hierarchy
            .parent
            .and_then(|parent| self.record(parent).resolved.0.color);
        let (_, color, background, border) = self.palette_paint_colors(id, inherited);
        [color, background, border]
    }

    pub(super) fn transition_hover(&mut self, id: StableNodeId, from: [Option<[f32; 4]>; 3]) {
        if self.hover_transitions.contains_key(&id) {
            self.mark_hover_paint(id);
        }
        self.cancel_hover_transition(id);
        let to = self.hover_paint(id);
        if from != to {
            self.hover_transitions.insert(
                id,
                HoverTransition {
                    from,
                    progress: 0.0,
                    inherits_color: from[0] != to[0],
                },
            );
            self.start_component_track(
                id,
                crate::component_animation_kinds::HOVER,
                self.theme.duration(nana_ui_core::MotionRole::HoverColor),
                self.theme
                    .motion()
                    .easing(nana_ui_core::EasingRole::Standard),
                crate::AnimatableProperty::Progress,
                crate::MotionValue::Scalar(0.0),
                crate::MotionValue::Scalar(1.0),
                crate::MotionInterrupt::Retarget,
                None,
            );
            self.mark_hover_paint(id);
        }
    }
}

impl UiWorld {
    pub(super) fn mark_hover_paint(&mut self, target: StableNodeId) {
        let bits = DirtyMask::STYLE | DirtyMask::RENDER;
        if self
            .hover_transitions
            .get(&target)
            .is_some_and(|transition| transition.inherits_color)
        {
            self.mark_subtree(target, bits);
        } else {
            self.mark(target, bits);
        }
    }

    pub(super) fn cancel_hover_transition(&mut self, target: StableNodeId) {
        self.hover_transitions.remove(&target);
        if let Some(id) =
            crate::component_animation_id(crate::component_animation_kinds::HOVER, target)
            && let Some(animation) = self.animations.remove(&id)
        {
            self.animation_deadlines
                .remove(&(animation.next_deadline, id));
        }
    }
}

#[cfg(test)]
#[path = "style_sharing_tests.rs"]
mod sharing_tests;

/// How many recent layouts a world keeps to share. Siblings built alike
/// (list rows, toolbars) repeat within a few writes.
const RECENT_LAYOUTS: usize = 8;

/// Layouts written recently, so nodes and resolutions whose layouts are
/// equal share one allocation instead of one each. Comparing is cheap: the
/// large groups of a layout are shared and compare by pointer first.
#[derive(Default)]
pub(crate) struct LayoutInterner {
    recent: [Option<Arc<nana_ui_core::LayoutStyle>>; RECENT_LAYOUTS],
    next: usize,
}

impl LayoutInterner {
    /// Replace `layout` with an equal recent one, or remember it.
    pub(crate) fn intern(&mut self, layout: &mut Arc<nana_ui_core::LayoutStyle>) {
        let recent = self.recent.iter().flatten();
        if recent.clone().any(|kept| Arc::ptr_eq(kept, layout)) {
            return;
        }
        if let Some(kept) = recent.clone().find(|kept| ***kept == **layout) {
            *layout = Arc::clone(kept);
            return;
        }
        self.recent[self.next] = Some(Arc::clone(layout));
        self.next = (self.next + 1) % RECENT_LAYOUTS;
    }
}
