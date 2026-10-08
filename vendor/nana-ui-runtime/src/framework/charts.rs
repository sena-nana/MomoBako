//! Retained chart hover and tooltip lifecycle on the normal document tree.
use super::*;
use crate::{DonutChart, TimeSeriesChart};

impl AppContext {
    pub(super) fn sync_chart_hover(
        &mut self,
        previous: Option<StableNodeId>,
        target: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        if previous != target
            && let Some(previous) = previous
        {
            self.clear_chart_hover(previous)?;
        }
        let Some(target) = target else {
            return Ok(());
        };
        let Some((x, y)) = self.pointer_location_on(target) else {
            return self.clear_chart_hover(target);
        };
        let Some((x, y)) = self.world.pointer_layout_position(target, x, y) else {
            return self.clear_chart_hover(target);
        };
        let Some(bounds) = self.world.layout_box(target) else {
            return Ok(());
        };
        let title = if let Ok((old, active, title)) =
            self.read(Entity::<DonutChart>::from_stable_id(target), |chart| {
                let active = chart.slice_at(bounds, x, y);
                (
                    chart.active,
                    active,
                    active.and_then(|index| chart.tooltip(index)),
                )
            }) {
            if old != active {
                self.update_component(Entity::<DonutChart>::from_stable_id(target), |chart, _| {
                    chart.active = active
                })?;
            }
            title
        } else if let Ok((old, active, title)) =
            self.read(Entity::<TimeSeriesChart>::from_stable_id(target), |chart| {
                let active = chart.datum_at(bounds, x, y);
                (
                    chart.active,
                    active,
                    active.and_then(|index| chart.tooltip(index)),
                )
            })
        {
            if old != active {
                self.update_component(
                    Entity::<TimeSeriesChart>::from_stable_id(target),
                    |chart, _| chart.active = active,
                )?;
            }
            title
        } else {
            return Ok(());
        };
        let Some(title) = title else {
            return self.clear_chart_hover(target);
        };
        let document = self
            .world
            .node(target)
            .ok_or(FrameworkError::MissingView(target))?
            .document;
        let tooltip = if let Some(id) = self
            .component_lifecycle
            .chart_tooltips
            .get(&target)
            .copied()
        {
            Entity::<Tooltip>::from_stable_id(id)
        } else {
            let mut tooltip = Tooltip::with_config(
                title.clone(),
                TooltipConfig {
                    placement: TooltipPlacement::FollowCursor,
                    delay_ms: 0,
                    max_width: 360.0,
                    ..Default::default()
                },
            );
            let layout = Arc::make_mut(&mut tooltip.style.layout);
            layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
            layout.white_space_nowrap = false;
            layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
            let tooltip = self.create_detached_component(document, tooltip)?;
            self.attach_child(target, tooltip.stable_id())?;
            self.component_lifecycle
                .chart_tooltips
                .insert(target, tooltip.stable_id());
            tooltip
        };
        self.update_component(tooltip, |tooltip, _| {
            tooltip.label = Arc::from(title.as_str());
            Arc::make_mut(&mut tooltip.style.layout).hidden = false;
        })?;
        self.position_tooltip(target, tooltip.stable_id())?;
        let state = crate::OverlayHostState {
            active: Some(tooltip.stable_id()),
            restore_focus: None,
        };
        if self.world.overlay_host(target) != Some(state) {
            let mut mutations = MutationQueue::new();
            mutations.set_overlay_host(target, state);
            self.commit_mutations(mutations)?;
        }
        Ok(())
    }

    pub(super) fn position_chart_tooltips(
        &mut self,
        document: DocumentId,
    ) -> Result<(), FrameworkError> {
        let targets = self
            .component_lifecycle
            .chart_tooltips
            .keys()
            .copied()
            .filter(|target| {
                self.world
                    .node(*target)
                    .is_some_and(|node| node.document == document)
            })
            .collect::<Vec<_>>();
        for target in targets {
            if self.world.is_mounted(target) {
                self.sync_chart_hover(Some(target), Some(target))?;
            }
        }
        Ok(())
    }

    fn clear_chart_hover(&mut self, target: StableNodeId) -> Result<(), FrameworkError> {
        if self
            .read(Entity::<DonutChart>::from_stable_id(target), |chart| {
                chart.active.is_some()
            })
            .unwrap_or(false)
        {
            self.update_component(Entity::<DonutChart>::from_stable_id(target), |chart, _| {
                chart.active = None
            })?;
        } else if self
            .read(Entity::<TimeSeriesChart>::from_stable_id(target), |chart| {
                chart.active.is_some()
            })
            .unwrap_or(false)
        {
            self.update_component(
                Entity::<TimeSeriesChart>::from_stable_id(target),
                |chart, _| chart.active = None,
            )?;
        }
        if let Some(id) = self
            .component_lifecycle
            .chart_tooltips
            .get(&target)
            .copied()
        {
            self.update_component(Entity::<Tooltip>::from_stable_id(id), |tooltip, _| {
                Arc::make_mut(&mut tooltip.style.layout).hidden = true
            })?;
            if self
                .world
                .overlay_host(target)
                .is_some_and(|state| state.active.is_some())
            {
                let mut mutations = MutationQueue::new();
                mutations.set_overlay_host(target, crate::OverlayHostState::default());
                self.commit_mutations(mutations)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComponentGeometry, DonutSlice, LayoutViewport, Stack, StandardVisual, TimeSeriesLayer,
    };
    use nana_ui_core::SemanticColorRole;

    #[test]
    fn normal_pointer_hover_selects_donut_sector_and_releases_tooltip_on_leave() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let chart = context
            .create_component(
                document,
                DonutChart::new([
                    DonutSlice {
                        value: 1.0,
                        color: SemanticColorRole::Accent,
                    },
                    DonutSlice {
                        value: 3.0,
                        color: SemanticColorRole::Success,
                    },
                ])
                .labels(["First", "Second"]),
            )
            .unwrap();
        context
            .layout_document(document, LayoutViewport::new(400.0, 300.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let id = chart.stable_id();
        context.set_pointer_location(document, 1, Some((55.0, 12.0)));
        let target = context.pointer_target(document, 55.0, 12.0);
        assert_eq!(target, Some(id));
        context.set_pointer_hover(document, 1, target).unwrap();
        assert_eq!(context.read(chart, |chart| chart.active).unwrap(), Some(0));
        let tooltip =
            Entity::<Tooltip>::from_stable_id(context.component_lifecycle.chart_tooltips[&id]);
        assert_eq!(
            context
                .read(tooltip, |tooltip| tooltip.label.to_string())
                .unwrap(),
            "First: 1 (25%)"
        );
        assert!(
            !context
                .read(tooltip, |tooltip| tooltip.style.layout.hidden)
                .unwrap()
        );
        context.set_pointer_hover(document, 1, None).unwrap();
        assert_eq!(context.read(chart, |chart| chart.active).unwrap(), None);
        assert!(
            context
                .read(tooltip, |tooltip| tooltip.style.layout.hidden)
                .unwrap()
        );
        context.remove_view(chart).unwrap();
        assert!(context.component_lifecycle.chart_tooltips.is_empty());
        assert!(!context.world().contains(tooltip.stable_id()));
    }

    #[test]
    fn stacked_geometry_and_real_hover_share_same_date_index() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let chart = context
            .create_component(
                document,
                TimeSeriesChart::new([4.0, 8.0])
                    .label("Total")
                    .axis_labels(["09-01", "09-02"])
                    .stacked([
                        TimeSeriesLayer::new("Input", [1.0, 2.0], SemanticColorRole::Accent),
                        TimeSeriesLayer::new("Output", [3.0, 6.0], SemanticColorRole::Success),
                    ]),
            )
            .unwrap();
        context
            .layout_document(document, LayoutViewport::new(400.0, 248.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let id = chart.stable_id();
        let bounds = context.world().layout_box(id).unwrap();
        let plot = TimeSeriesChart::stacked_plot(bounds);
        let x = plot.x + plot.width * 0.75;
        let y = plot.y + plot.height / 2.0;
        context.set_pointer_location(document, 1, Some((x, y)));
        context.set_pointer_hover(document, 1, Some(id)).unwrap();
        assert_eq!(context.read(chart, |chart| chart.active).unwrap(), Some(1));
        let nodes = context.world().extract_nodes(&[id]);
        let Some(ComponentGeometry::StackedTimeSeriesChart {
            bars, line, marker, ..
        }) = nodes[0].component_geometry.as_deref()
        else {
            panic!("stacked chart geometry");
        };
        assert_eq!(bars.len(), 4);
        assert!((bars[0].0.height * 3.0 - bars[1].0.height).abs() < 0.01);
        assert!((bars[0].0.y - bars[1].0.y - bars[1].0.height).abs() < 0.01);
        assert!((line[1][1] - plot.y).abs() < 0.01);
        assert!(marker.is_some());
        let tooltip =
            Entity::<Tooltip>::from_stable_id(context.component_lifecycle.chart_tooltips[&id]);
        let value = context
            .read(tooltip, |tooltip| tooltip.label.to_string())
            .unwrap();
        assert_eq!(value, "09-02\nInput: 2\nOutput: 6\nTotal: 8");
    }
    #[test]
    fn donut_hover_uses_scrolled_hit_coordinates() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let mut style = crate::NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(crate::LengthSpec::Px(66.0));
        layout.height = Some(crate::LengthSpec::Px(36.0));
        let scroll = context
            .create_component(document, ScrollView::new(ScrollAxes::Vertical).style(style))
            .unwrap();
        let chart = context
            .create_detached_component(
                document,
                DonutChart::new([
                    DonutSlice {
                        value: 1.0,
                        color: SemanticColorRole::Accent,
                    },
                    DonutSlice {
                        value: 3.0,
                        color: SemanticColorRole::Success,
                    },
                ])
                .labels(["First", "Second"]),
            )
            .unwrap();
        context.append_child(scroll, chart).unwrap();
        context
            .layout_document(document, LayoutViewport::new(400.0, 300.0))
            .unwrap();
        assert!(
            context
                .scroll_to(scroll, ScrollOffset { x: 0.0, y: 30.0 })
                .unwrap()
        );
        context.rebuild_hit_test(document);
        let point = (55.0, 25.0);
        context.set_pointer_location(document, 1, Some(point));
        let target = context.pointer_target(document, point.0, point.1);
        assert_eq!(target, Some(chart.stable_id()));
        context.set_pointer_hover(document, 1, target).unwrap();
        assert_eq!(context.read(chart, |chart| chart.active).unwrap(), Some(1));
    }

    #[test]
    fn chart_reparent_preserves_live_hover_and_park_dismisses_without_reviving() {
        let mut context = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let root = context.create_component(doc, Stack::column(0.0)).unwrap();
        let branch = context
            .create_detached_component(doc, Stack::column(0.0))
            .unwrap();
        let chart = context
            .create_detached_component(
                doc,
                DonutChart::new([DonutSlice {
                    value: 1.0,
                    color: SemanticColorRole::Accent,
                }]),
            )
            .unwrap();
        context.append_child(root, branch).unwrap();
        context.append_child(branch, chart).unwrap();
        context
            .layout_document(doc, LayoutViewport::new(400.0, 300.0))
            .unwrap();
        context.rebuild_hit_test(doc);
        context.set_pointer_location(doc, 1, Some((55.0, 33.0)));
        context
            .set_pointer_hover(doc, 1, Some(chart.stable_id()))
            .unwrap();
        let tip = context.component_lifecycle.chart_tooltips[&chart.stable_id()];
        context
            .reconcile_children(root.stable_id(), &[chart.stable_id()])
            .unwrap();
        assert_eq!(context.read(chart, |c| c.active).unwrap(), Some(0));
        assert_eq!(
            context
                .world()
                .overlay_host(chart.stable_id())
                .unwrap()
                .active,
            Some(tip)
        );
        context.reconcile_children(root.stable_id(), &[]).unwrap();
        assert_eq!(context.read(chart, |c| c.active).unwrap(), None);
        assert!(matches!(
            context.world().standard_visual(chart.stable_id()),
            Some(StandardVisual::DonutChart { active: None, .. })
        ));
        assert_eq!(
            context
                .world()
                .overlay_host(chart.stable_id())
                .unwrap()
                .active,
            None
        );
        context
            .reconcile_children(root.stable_id(), &[chart.stable_id()])
            .unwrap();
        context
            .layout_document(doc, LayoutViewport::new(400.0, 300.0))
            .unwrap();
        assert_eq!(
            context
                .world()
                .overlay_host(chart.stable_id())
                .unwrap()
                .active,
            None
        );
    }
}
