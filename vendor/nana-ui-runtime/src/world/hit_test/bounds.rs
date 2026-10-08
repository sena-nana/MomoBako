//! Ordered sibling bounds. Unknown geometry is retained conservatively;
//! unused tree slots are empty, not unknown. Queries preserve reverse order.
use super::{LayoutBox, union_bounds};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) enum Bounds {
    #[default]
    Empty,
    /// Occupied by a retained entry without a hittable contribution. Its slot
    /// must survive so a later activation can refit without rebuilding siblings.
    Inactive,
    Known(LayoutBox),
    Unknown,
}

impl Bounds {
    pub(super) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Empty, value) | (value, Self::Empty) => value,
            (Self::Inactive, value) | (value, Self::Inactive) => value,
            (Self::Known(a), Self::Known(b)) => Self::Known(union_bounds(a, b)),
            _ => Self::Unknown,
        }
    }
}

impl From<Option<LayoutBox>> for Bounds {
    fn from(value: Option<LayoutBox>) -> Self {
        value.map_or(Self::Unknown, Self::Known)
    }
}

impl Bounds {
    pub(super) fn contains(self, x: f32, y: f32) -> bool {
        match self {
            Self::Empty | Self::Inactive => false,
            Self::Known(bounds) => bounds.contains(x, y),
            Self::Unknown => true,
        }
    }

    pub(super) fn translated(self, offset: [f32; 2]) -> Self {
        match self {
            Self::Known(mut bounds) => {
                bounds.x += offset[0];
                bounds.y += offset[1];
                if bounds.x.is_finite() && bounds.y.is_finite() {
                    Self::Known(bounds)
                } else {
                    Self::Unknown
                }
            }
            other => other,
        }
    }
}

#[derive(Default)]
pub(super) struct BoundsTree {
    nodes: Vec<Bounds>,
    leaf: usize,
    len: usize,
}

impl BoundsTree {
    pub(super) fn new<T: Into<Bounds>>(bounds: impl ExactSizeIterator<Item = T>) -> Self {
        let len = bounds.len();
        if len == 0 {
            return Self::default();
        }
        let leaf = len.next_power_of_two();
        let mut nodes = vec![Bounds::Empty; leaf * 2];
        for (index, bounds) in bounds.enumerate() {
            nodes[leaf + index] = bounds.into();
        }
        for index in (1..leaf).rev() {
            nodes[index] = nodes[index * 2].merge(nodes[index * 2 + 1]);
        }
        Self { nodes, leaf, len }
    }

    pub(super) fn bounds(&self) -> Bounds {
        self.nodes.get(1).copied().unwrap_or_default()
    }

    pub(super) fn set(&mut self, index: usize, bounds: impl Into<Bounds>) {
        self.set_value(index, bounds.into());
    }

    pub(super) fn clear(&mut self, index: usize) {
        self.set_value(index, Bounds::Empty);
    }

    pub(super) fn is_empty(&self) -> bool {
        matches!(self.nodes.get(1), None | Some(Bounds::Empty))
    }

    fn set_value(&mut self, index: usize, bounds: Bounds) {
        assert!(index < self.len);
        let mut at = self.leaf + index;
        self.nodes[at] = bounds;
        while at > 1 {
            at /= 2;
            self.nodes[at] = self.nodes[at * 2].merge(self.nodes[at * 2 + 1]);
        }
    }

    pub(super) fn visit(&self, x: f32, y: f32, emit: &mut impl FnMut(usize) -> bool) -> bool {
        self.visit_counted(1, x, y, emit, &mut 0)
    }

    fn visit_counted(
        &self,
        at: usize,
        x: f32,
        y: f32,
        emit: &mut impl FnMut(usize) -> bool,
        visited: &mut usize,
    ) -> bool {
        let Some(bounds) = self.nodes.get(at) else {
            return false;
        };
        *visited += 1;
        if !bounds.contains(x, y) {
            return false;
        }
        if at >= self.leaf {
            return emit(at - self.leaf);
        }
        self.visit_counted(at * 2 + 1, x, y, emit, visited)
            || self.visit_counted(at * 2, x, y, emit, visited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_ranges_prune_without_becoming_vacant_or_hiding_unknown_geometry() {
        let mut tree = BoundsTree::new(std::iter::repeat_n(Bounds::Inactive, 100_000));
        let mut visited = 0;
        tree.visit_counted(
            1,
            5.0,
            5.0,
            &mut |_| panic!("inactive leaf queried"),
            &mut visited,
        );
        assert_eq!(visited, 1);
        assert!(
            !tree.is_empty(),
            "retained sibling slots must survive pruning"
        );
        tree.set(50_000, Bounds::Unknown);
        let mut hits = Vec::new();
        visited = 0;
        tree.visit_counted(
            1,
            5.0,
            5.0,
            &mut |slot| {
                hits.push(slot);
                false
            },
            &mut visited,
        );
        assert_eq!(hits, [50_000]);
        assert!(
            visited < 40,
            "reactivating one entry inspected {visited} ranges"
        );
        tree.clear(50_000);
        visited = 0;
        tree.visit_counted(
            1,
            5.0,
            5.0,
            &mut |_| panic!("cleared leaf queried"),
            &mut visited,
        );
        assert_eq!(visited, 1);
        assert!(!tree.is_empty());
    }

    #[test]
    fn hundred_thousand_siblings_query_local_bounds_and_keep_reverse_order() {
        let rect = |index: usize| LayoutBox {
            x: 0.0,
            y: index as f32 * 20.0,
            width: 100.0,
            height: 20.0,
        };
        let mut tree = BoundsTree::new((0..100_000).map(|i| Some(rect(i))));
        let mut visited = 0;
        let mut hits = Vec::new();
        tree.visit_counted(
            1,
            5.0,
            500_005.0,
            &mut |i| {
                hits.push(i);
                false
            },
            &mut visited,
        );
        assert_eq!(hits, [25_000]);
        assert!(visited < 100, "point query inspected {visited} ranges");
        tree.set(99_999, Some(rect(25_000)));
        hits.clear();
        visited = 0;
        tree.visit_counted(
            1,
            5.0,
            500_005.0,
            &mut |i| {
                hits.push(i);
                false
            },
            &mut visited,
        );
        assert_eq!(hits, [99_999, 25_000]);
        assert!(visited < 100);
        tree.set(99_999, None);
        hits.clear();
        tree.visit(5.0, -100.0, &mut |i| {
            hits.push(i);
            false
        });
        assert_eq!(hits, [99_999], "unknown geometry must remain a candidate");
        tree.set(99_999, Some(rect(99_999)));
        assert!(
            matches!(tree.bounds(), Bounds::Known(_)),
            "local update must repair unknown ancestors"
        );
    }
}
