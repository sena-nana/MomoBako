//! Dock commands and constraints. Every mutation targets the caller's workspace.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockItemLimits {
    pub minimum: (f32, f32),
    pub maximum: (Option<f32>, Option<f32>),
    pub closeable: bool,
    pub floatable: bool,
}
impl Default for DockItemLimits {
    fn default() -> Self {
        Self {
            minimum: (96.0, 64.0),
            maximum: (None, None),
            closeable: true,
            floatable: true,
        }
    }
}

impl DockItemLimits {
    fn along_axis(self, axis: DockAxis) -> (f32, f32) {
        let (minimum, maximum) = match axis {
            DockAxis::Horizontal => (self.minimum.0, self.maximum.0),
            DockAxis::Vertical => (self.minimum.1, self.maximum.1),
        };
        (
            positive(minimum, 1.0),
            maximum
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(f32::MAX),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DockCommand {
    Hide(Arc<str>),
    Show(Arc<str>),
    SetLocked(bool),
    Reorder {
        surface: Arc<str>,
        item: Arc<str>,
        target: Arc<str>,
        before: bool,
    },
    Retarget {
        item: Arc<str>,
        surface: Arc<str>,
        target: Arc<str>,
        zone: DockDropZone,
    },
    ResizeSplit {
        surface: Arc<str>,
        path: Vec<usize>,
        ratio: f32,
        available: f32,
    },
    Float {
        item: Arc<str>,
        bounds: DockBoundsPersist,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DockCommandOutcome {
    pub changed: bool,
    pub effects: Vec<DockWorkspaceEvent>,
}

impl DockBoundsPersist {
    /// Keep a restored window visible, including after a display is removed.
    pub fn clamped_to(self, area: Self) -> Self {
        let aw = positive(area.width, 1.0);
        let ah = positive(area.height, 1.0);
        let ax = finite(area.x, 0.0);
        let ay = finite(area.y, 0.0);
        let width = positive(self.width, 160.0).clamp(160.0_f32.min(aw), aw);
        let height = positive(self.height, 120.0).clamp(120.0_f32.min(ah), ah);
        Self {
            x: finite(self.x, ax).clamp(ax, ax + aw - width),
            y: finite(self.y, ay).clamp(ay, ay + ah - height),
            width,
            height,
        }
    }
}
fn positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

impl DockWorkspace {
    pub fn surface_root(&self, surface: &str) -> Option<&DockNode> {
        if surface == MAIN_SURFACE_ID {
            Some(&self.main)
        } else {
            self.floating
                .iter()
                .find(|item| item.id.as_ref() == surface)
                .map(|item| &item.root)
        }
    }

    /// Atomic command boundary. Rejected operations leave both trees unchanged.
    pub fn execute(&mut self, command: DockCommand) -> DockCommandOutcome {
        let mut out = DockCommandOutcome::default();
        if let DockCommand::SetLocked(locked) = command {
            out.changed = self.locked != locked;
            self.locked = locked;
            return out;
        }
        if self.locked {
            return out;
        }
        out.changed = match command {
            DockCommand::Hide(item) => self.hide(item),
            DockCommand::Show(item) => self.show(item),
            DockCommand::Reorder {
                surface,
                item,
                target,
                before,
            } => self
                .surface_root_mut(&surface)
                .is_some_and(|root| root.reorder_tab(&item, &target, before)),
            DockCommand::Retarget {
                item,
                surface,
                target,
                zone,
            } => {
                let (changed, closed) = self.move_item(&item, &surface, &target, zone);
                out.effects
                    .extend(closed.into_iter().map(DockWorkspaceEvent::CloseFloating));
                changed
            }
            DockCommand::ResizeSplit {
                surface,
                path,
                ratio,
                available,
            } => {
                let ratio = self.constrained_split_ratio(&surface, &path, ratio, available);
                ratio.is_some_and(|ratio| self.set_split_ratio(&surface, &path, ratio))
            }
            DockCommand::Float { item, mut bounds } => {
                let limits = self.item_limits.get(&item).copied().unwrap_or_default();
                bounds.width = constrained_extent(bounds.width, limits.minimum.0, limits.maximum.0);
                bounds.height =
                    constrained_extent(bounds.height, limits.minimum.1, limits.maximum.1);
                if let Some(event) =
                    self.float_item_at(item, bounds.x, bounds.y, bounds.width, bounds.height)
                {
                    out.effects.push(event);
                    true
                } else {
                    false
                }
            }
            DockCommand::SetLocked(_) => unreachable!(),
        };
        out
    }

    fn move_item(
        &mut self,
        item: &str,
        surface: &str,
        target: &str,
        zone: DockDropZone,
    ) -> (bool, Option<Arc<str>>) {
        if item == target
            || self.primary.as_deref() == Some(item)
            || !self
                .surface_root(surface)
                .is_some_and(|root| root.contains(target))
        {
            return (false, None);
        }
        // Transaction-local candidate; never retained as a second workspace.
        let mut candidate = self.clone();
        let mut closed = None;
        let taken = if candidate.main.contains(item) {
            extract_item(&mut candidate.main, item)
        } else if let Some(index) = candidate
            .floating
            .iter()
            .position(|floating| floating.root.contains(item))
        {
            if candidate.floating[index].root.flatten().len() == 1 {
                let floating = candidate.floating.remove(index);
                closed = Some(floating.id.clone());
                candidate.monitors.remove(&floating.id);
                Some(floating.root)
            } else {
                extract_item(&mut candidate.floating[index].root, item)
            }
        } else {
            None
        };
        let Some(taken) = taken else {
            return (false, None);
        };
        let Some(root) = candidate.surface_root_mut(surface) else {
            return (false, None);
        };
        if !insert_dock_item(root, target, taken, zone) {
            return (false, None);
        }
        candidate.hidden.retain(|hidden| hidden.as_ref() != item);
        *self = candidate;
        (true, closed)
    }

    pub fn constrained_split_ratio(
        &self,
        surface: &str,
        path: &[usize],
        ratio: f32,
        available: f32,
    ) -> Option<f32> {
        if !available.is_finite() || available <= 0.0 || !ratio.is_finite() {
            return None;
        }
        let mut node = self.surface_root(surface)?;
        for child in path {
            let DockNode::Split { first, second, .. } = node else {
                return None;
            };
            node = match child {
                0 => first,
                1 => second,
                _ => return None,
            };
        }
        let DockNode::Split {
            axis,
            first,
            second,
            ..
        } = node
        else {
            return None;
        };
        let (a_min, a_max) = subtree_limits(first, *axis, &self.item_limits);
        let (b_min, b_max) = subtree_limits(second, *axis, &self.item_limits);
        let lower = (a_min / available)
            .max(1.0 - b_max / available)
            .max(MIN_SPLIT_RATIO);
        let upper = (1.0 - b_min / available)
            .min(a_max / available)
            .min(MAX_SPLIT_RATIO);
        if lower > upper {
            return None;
        }
        Some(ratio.clamp(lower, upper))
    }
}
fn constrained_extent(value: f32, minimum: f32, maximum: Option<f32>) -> f32 {
    let minimum = positive(minimum, 1.0);
    let maximum = maximum
        .filter(|v| v.is_finite() && *v >= minimum)
        .unwrap_or(f32::MAX);
    positive(value, minimum).clamp(minimum, maximum)
}
fn subtree_limits(
    node: &DockNode,
    axis: DockAxis,
    limits: &std::collections::HashMap<Arc<str>, DockItemLimits>,
) -> (f32, f32) {
    match node {
        DockNode::Item { id, .. } => limits.get(id).copied().unwrap_or_default().along_axis(axis),
        DockNode::Tabs { tabs, .. } => tabs
            .iter()
            .map(|id| limits.get(id).copied().unwrap_or_default().along_axis(axis))
            .fold((0.0_f32, f32::MAX), |(min, max), (a, b)| {
                (min.max(a), max.min(b))
            }),
        DockNode::Split {
            axis: split,
            first,
            second,
            ..
        } => {
            let (a, b) = subtree_limits(first, axis, limits);
            let (c, d) = subtree_limits(second, axis, limits);
            if *split == axis {
                (a + c, b + d)
            } else {
                (a.max(c), b.min(d))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> DockWorkspace {
        DockWorkspace::new(DockNode::split(
            DockAxis::Horizontal,
            0.5,
            DockNode::item("main", None),
            DockNode::tabs(
                [Arc::from("tools"), Arc::from("logs")],
                "tools",
                Vec::<(Arc<str>, Option<StableNodeId>)>::new(),
            ),
        ))
        .primary("main")
    }
    #[test]
    fn rejected_retarget_is_atomic_and_cross_window_drop_closes_empty_surface() {
        let mut workspace = workspace();
        workspace.float_item("tools").unwrap();
        let before = workspace.clone();
        assert!(
            !workspace
                .execute(DockCommand::Retarget {
                    item: "tools".into(),
                    surface: "missing".into(),
                    target: "main".into(),
                    zone: DockDropZone::Tab
                })
                .changed
        );
        assert_eq!(workspace, before);
        let outcome = workspace.execute(DockCommand::Retarget {
            item: "tools".into(),
            surface: MAIN_SURFACE_ID.into(),
            target: "logs".into(),
            zone: DockDropZone::Tab,
        });
        assert!(outcome.changed);
        assert_eq!(
            outcome.effects,
            [DockWorkspaceEvent::CloseFloating("1".into())]
        );
        assert!(workspace.main.contains("tools"));
        assert!(workspace.floating.is_empty());
    }
    #[test]
    fn resize_respects_both_children_and_lock_prevents_commands() {
        let mut workspace = workspace();
        workspace.item_limits.insert(
            "main".into(),
            DockItemLimits {
                minimum: (300.0, 64.0),
                ..DockItemLimits::default()
            },
        );
        assert!(
            workspace
                .execute(DockCommand::ResizeSplit {
                    surface: MAIN_SURFACE_ID.into(),
                    path: vec![],
                    ratio: 0.1,
                    available: 1000.0
                })
                .changed
        );
        assert_eq!(workspace.main.split_ratio_at(&[]), Some(0.3));
        workspace.execute(DockCommand::SetLocked(true));
        let before = workspace.clone();
        assert!(!workspace.execute(DockCommand::Hide("tools".into())).changed);
        assert!(workspace.float_item("tools").is_none());
        assert_eq!(workspace, before);
    }
}
