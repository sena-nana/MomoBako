//! Host pointer/time state. The Runtime workspace remains the only editable tree.
use nana_ui_runtime::{
    DockAxis, DockBoundsPersist, DockCommand, DockCommandOutcome, DockDropZone, DockWorkspace,
    DockWorkspaceEvent, dock_split_ratio_from_pointer,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const INSERT_DWELL: Duration = Duration::from_millis(80);
const DRAG_DISTANCE: f32 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct DockDropTarget {
    pub surface: Arc<str>,
    pub item: Arc<str>,
    pub zone: DockDropZone,
}
#[derive(Debug)]
struct Drag {
    item: Arc<str>,
    start: (f32, f32),
    moved: bool,
    candidate: Option<(DockDropTarget, Duration)>,
}
#[derive(Debug)]
struct Resize {
    surface: Arc<str>,
    path: Vec<usize>,
    axis: DockAxis,
    start: f32,
    ratio: f32,
    available: f32,
}
/// Converts host pointer events into Runtime commands. It owns no tree, item
/// registry, persistent layout or document. Targets come from Runtime hit data.
#[derive(Debug, Default)]
pub struct DockController {
    drag: Option<Drag>,
    resize: Option<Resize>,
    display_work_areas: BTreeMap<String, DockBoundsPersist>,
}
impl DockController {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_display_work_areas(&mut self, areas: BTreeMap<String, DockBoundsPersist>) {
        self.display_work_areas = areas;
    }
    pub fn begin_drag(
        &mut self,
        workspace: &DockWorkspace,
        item: impl Into<Arc<str>>,
        position: (f32, f32),
    ) -> bool {
        let item = item.into();
        if workspace.locked
            || !workspace.is_visible(&item)
            || workspace.primary.as_deref() == Some(&item)
            || !position.0.is_finite()
            || !position.1.is_finite()
        {
            return false;
        }
        self.cancel();
        self.drag = Some(Drag {
            item,
            start: position,
            moved: false,
            candidate: None,
        });
        true
    }
    pub fn pointer_moved(
        &mut self,
        position: (f32, f32),
        target: Option<DockDropTarget>,
        now: Duration,
    ) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if !position.0.is_finite() || !position.1.is_finite() {
            return;
        }
        drag.moved |= (position.0 - drag.start.0).hypot(position.1 - drag.start.1) >= DRAG_DISTANCE;
        let target = target.filter(|target| target.item != drag.item);
        if drag.candidate.as_ref().map(|(target, _)| target) != target.as_ref() {
            drag.candidate = target.map(|target| (target, now + INSERT_DWELL));
        }
    }
    pub fn drop_target(&self, now: Duration) -> Option<&DockDropTarget> {
        let drag = self.drag.as_ref()?;
        let (target, ready) = drag.candidate.as_ref()?;
        (drag.moved && now >= *ready).then_some(target)
    }
    pub fn next_wakeup(&self) -> Option<Duration> {
        self.drag
            .as_ref()?
            .candidate
            .as_ref()
            .map(|(_, ready)| *ready)
    }
    pub fn finish_drag(
        &mut self,
        workspace: &mut DockWorkspace,
        now: Duration,
        floating: Option<DockBoundsPersist>,
    ) -> DockCommandOutcome {
        let target = self.drop_target(now).cloned();
        let Some(drag) = self.drag.take() else {
            return DockCommandOutcome::default();
        };
        if !drag.moved {
            return DockCommandOutcome::default();
        }
        if let Some(target) = target {
            workspace.execute(DockCommand::Retarget {
                item: drag.item,
                surface: target.surface,
                target: target.item,
                zone: target.zone,
            })
        } else if drag.candidate.is_none() {
            floating
                .map(|bounds| {
                    workspace.execute(DockCommand::Float {
                        item: drag.item,
                        bounds,
                    })
                })
                .unwrap_or_default()
        } else {
            DockCommandOutcome::default()
        }
    }
    pub fn begin_resize(
        &mut self,
        workspace: &DockWorkspace,
        surface: impl Into<Arc<str>>,
        path: Vec<usize>,
        axis: DockAxis,
        pointer: f32,
        available: f32,
    ) -> bool {
        let surface = surface.into();
        let Some(ratio) = workspace
            .surface_root(&surface)
            .and_then(|root| root.split_ratio_at(&path))
        else {
            return false;
        };
        if workspace.locked || !pointer.is_finite() || !available.is_finite() || available <= 0.0 {
            return false;
        }
        self.cancel();
        self.resize = Some(Resize {
            surface,
            path,
            axis,
            start: pointer,
            ratio,
            available,
        });
        true
    }
    pub fn resize(
        &mut self,
        workspace: &mut DockWorkspace,
        position: (f32, f32),
    ) -> DockCommandOutcome {
        let Some(resize) = &self.resize else {
            return DockCommandOutcome::default();
        };
        let pointer = match resize.axis {
            DockAxis::Horizontal => position.0,
            DockAxis::Vertical => position.1,
        };
        if !pointer.is_finite() {
            return DockCommandOutcome::default();
        }
        workspace.execute(DockCommand::ResizeSplit {
            surface: resize.surface.clone(),
            path: resize.path.clone(),
            ratio: dock_split_ratio_from_pointer(
                resize.ratio,
                resize.start,
                pointer,
                resize.available,
            ),
            available: resize.available,
        })
    }
    /// Release pointer state on cancellation or any participating window closing.
    pub fn cancel(&mut self) {
        self.drag = None;
        self.resize = None;
    }
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// Clamp only host window metadata; emit the same Runtime window effects.
    pub fn clamp_floating_bounds(&self, workspace: &mut DockWorkspace) -> Vec<DockWorkspaceEvent> {
        let mut effects = Vec::new();
        for surface in &mut workspace.floating {
            let area = workspace
                .monitors
                .get(&surface.id)
                .and_then(|name| self.display_work_areas.get_key_value(name))
                .or_else(|| {
                    self.display_work_areas.iter().max_by(|(_, a), (_, b)| {
                        overlap(surface.x, surface.y, surface.width, surface.height, **a).total_cmp(
                            &overlap(surface.x, surface.y, surface.width, surface.height, **b),
                        )
                    })
                });
            let Some((name, area)) = area else {
                continue;
            };
            let original = DockBoundsPersist {
                x: surface.x,
                y: surface.y,
                width: surface.width,
                height: surface.height,
            };
            let bounds = original.clamped_to(*area);
            workspace.monitors.insert(surface.id.clone(), name.clone());
            if original != bounds {
                surface.x = bounds.x;
                surface.y = bounds.y;
                surface.width = bounds.width;
                surface.height = bounds.height;
                effects.push(DockWorkspaceEvent::MoveFloating {
                    id: surface.id.clone(),
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                });
            }
        }
        effects
    }
}
fn overlap(x: f32, y: f32, width: f32, height: f32, area: DockBoundsPersist) -> f32 {
    ((x + width).min(area.x + area.width) - x.max(area.x)).max(0.0)
        * ((y + height).min(area.y + area.height) - y.max(area.y)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_runtime::{DockNode, MAIN_SURFACE_ID};
    fn workspace() -> DockWorkspace {
        DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.5,
            DockNode::item("main", None),
            DockNode::item("tools", None),
        ))
        .primary("main")
    }
    #[test]
    fn dwell_restarts_when_target_changes_and_cancel_releases_state() {
        let mut workspace = workspace();
        let mut host = DockController::new();
        assert!(host.begin_drag(&workspace, "tools", (0.0, 0.0)));
        let target = DockDropTarget {
            surface: MAIN_SURFACE_ID.into(),
            item: "main".into(),
            zone: DockDropZone::Tab,
        };
        host.pointer_moved((20.0, 0.0), Some(target.clone()), Duration::ZERO);
        assert!(host.drop_target(Duration::from_millis(79)).is_none());
        assert_eq!(host.drop_target(Duration::from_millis(80)), Some(&target));
        host.pointer_moved((21.0, 0.0), None, Duration::from_millis(90));
        host.pointer_moved((22.0, 0.0), Some(target), Duration::from_millis(100));
        assert!(host.drop_target(Duration::from_millis(150)).is_none());
        assert!(
            host.finish_drag(&mut workspace, Duration::from_millis(180), None)
                .changed
        );
        assert!(!host.is_dragging());
        host.cancel();
        assert!(host.next_wakeup().is_none());
    }
    #[test]
    fn missing_monitor_is_reassigned_and_bounds_are_clamped() {
        let mut workspace = workspace();
        workspace.float_item("tools").unwrap();
        workspace.monitors.insert("1".into(), "removed".into());
        workspace.floating[0].x = 5000.0;
        let mut host = DockController::new();
        host.set_display_work_areas(BTreeMap::from([(
            "primary".into(),
            DockBoundsPersist {
                x: 0.0,
                y: 0.0,
                width: 1024.0,
                height: 768.0,
            },
        )]));
        assert_eq!(host.clamp_floating_bounds(&mut workspace).len(), 1);
        assert_eq!(workspace.monitors["1"], "primary");
        assert!(workspace.floating[0].x + workspace.floating[0].width <= 1024.0);
    }
}
