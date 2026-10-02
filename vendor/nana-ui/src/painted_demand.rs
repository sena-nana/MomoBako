//! 宿主纹理的绘制需求:每个 slot 当前被画到多少设备像素。
//!
//! 同一个 slot 可以同时被多个节点、多个窗口(多个绘制目标)采样,各自的
//! 尺寸不同。纹理只有一份,它该准备多大由**全部可见消费者里最大的那个**
//! 决定,而不是最后画的那个——后者随绘制顺序变,大视图会拿到小纹理。
//!
//! 每个绘制目标是一条通道:它每次**全新 prepare** 交一整份「这一帧画了
//! 哪些 slot、每个多大」,覆盖自己上一份;同一通道里同一 slot 画多次按宽、
//! 高各取最大。复用上一帧(blit / 缓存批次)的帧不交,内容没变,需求也
//! 没变。合并值是所有通道的逐边最大值;没有任何通道画到的 slot 没有需求。

use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::gpu_texture::HostTextureRegistry;

pub(crate) type Extents = HashMap<Arc<str>, [u32; 2]>;

#[derive(Debug, Default)]
pub(crate) struct PaintedDemand {
    channels: HashMap<u64, Extents>,
    merged: Extents,
    /// slot → 合并值最后一次变化时的 revision。slot 被 `remove` 时一并
    /// 删掉,所以大小受已登记 slot 数约束。
    changed_at: HashMap<Arc<str>, u64>,
    revision: u64,
}

impl PaintedDemand {
    pub(crate) fn extent(&self, slot: &str) -> Option<[u32; 2]> {
        self.merged.get(slot).copied()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn changes_since(&self, revision: u64, out: &mut Vec<Arc<str>>) {
        out.extend(
            self.changed_at
                .iter()
                .filter(|(_, changed)| **changed > revision)
                .map(|(slot, _)| Arc::clone(slot)),
        );
    }

    /// `pass` 与 `channel` 上一份记录相同:提交它不会改变任何东西。
    pub(crate) fn unchanged(&self, channel: u64, pass: &Extents) -> bool {
        self.channels
            .get(&channel)
            .map_or(pass.is_empty(), |previous| previous == pass)
    }

    /// 用 `pass` 替换 `channel` 的上一份记录;`pass` 换回旧记录(已清空),
    /// 供下一帧复用分配。只重新合并这条通道里增、删或尺寸变了的 slot,
    /// 返回合并值变了的 slot。
    pub(crate) fn commit(&mut self, channel: u64, pass: &mut Extents) -> Vec<Arc<str>> {
        let mut previous = self
            .channels
            .insert(channel, std::mem::take(pass))
            .unwrap_or_default();
        let current = &self.channels[&channel];
        let mut touched: Vec<Arc<str>> = previous
            .iter()
            .filter(|(slot, extent)| current.get(*slot) != Some(*extent))
            .map(|(slot, _)| Arc::clone(slot))
            .collect();
        touched.extend(
            current
                .keys()
                .filter(|slot| !previous.contains_key(*slot))
                .cloned(),
        );
        if current.is_empty() {
            self.channels.remove(&channel);
        }
        previous.clear();
        *pass = previous;
        self.remerge(touched)
    }

    /// 通道消失(绘制目标或画家被丢弃)时撤回它的全部需求。
    pub(crate) fn retire(&mut self, channel: u64) -> Vec<Arc<str>> {
        let Some(previous) = self.channels.remove(&channel) else {
            return Vec::new();
        };
        self.remerge(previous.into_keys().collect())
    }

    pub(crate) fn remove(&mut self, slot: &str) {
        for extents in self.channels.values_mut() {
            extents.remove(slot);
        }
        self.merged.remove(slot);
        self.changed_at.remove(slot);
    }

    pub(crate) fn clear(&mut self) {
        self.channels.clear();
        self.merged.clear();
        self.changed_at.clear();
    }

    fn remerge(&mut self, touched: Vec<Arc<str>>) -> Vec<Arc<str>> {
        let mut changed = Vec::new();
        for slot in touched {
            let merged = self
                .channels
                .values()
                .filter_map(|extents| extents.get(&slot))
                .copied()
                .reduce(|a, b| [a[0].max(b[0]), a[1].max(b[1])]);
            let differs = match (merged, self.merged.get_mut(&slot)) {
                (Some(extent), Some(current)) => std::mem::replace(current, extent) != extent,
                (Some(extent), None) => {
                    self.merged.insert(Arc::clone(&slot), extent);
                    true
                }
                (None, Some(_)) => {
                    self.merged.remove(&slot);
                    true
                }
                (None, None) => false,
            };
            if differs {
                changed.push(slot);
            }
        }
        if !changed.is_empty() {
            self.revision += 1;
            for slot in &changed {
                self.changed_at.insert(Arc::clone(slot), self.revision);
            }
        }
        changed
    }
}

/// 一个绘制目标的通道。随绘制目标的状态一起换入换出;丢弃时撤回它最后
/// 一次登记到的注册表里的需求。
#[derive(Debug)]
pub(crate) struct PaintedChannel {
    id: u64,
    pass: Extents,
    registry: Option<HostTextureRegistry>,
}

impl Default for PaintedChannel {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            pass: Extents::new(),
            registry: None,
        }
    }
}

impl PaintedChannel {
    /// 这条通道(绘制目标)的标识。`url(...)` 图片的需求用同一个标识登记。
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// 全新 prepare 开始:清空这一帧的记录。
    pub(crate) fn begin(&mut self) {
        self.pass.clear();
    }

    pub(crate) fn note(&mut self, slot: &Arc<str>, extent: [u32; 2]) {
        self.pass
            .entry(Arc::clone(slot))
            .and_modify(|current| {
                *current = [current[0].max(extent[0]), current[1].max(extent[1])];
            })
            .or_insert(extent);
    }

    /// 全新 prepare 结束:把这一帧的记录交给注册表。换了注册表时先从旧的
    /// 那边撤回。
    pub(crate) fn commit(&mut self, registry: Option<&HostTextureRegistry>) {
        let same = match (&self.registry, registry) {
            (Some(current), Some(next)) => current.same_registry(next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            if let Some(previous) = self.registry.take() {
                previous.retire_painted_channel(self.id);
            }
            self.registry = registry.cloned();
        }
        match &self.registry {
            Some(registry) => registry.commit_painted(self.id, &mut self.pass),
            None => self.pass.clear(),
        }
    }
}

impl Drop for PaintedChannel {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.take() {
            registry.retire_painted_channel(self.id);
        }
    }
}

pub(crate) type SharedPaintedDemand = Arc<RwLock<PaintedDemand>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str) -> Arc<str> {
        Arc::from(name)
    }

    fn pass(entries: &[(&str, [u32; 2])]) -> Extents {
        entries
            .iter()
            .map(|(name, extent)| (slot(name), *extent))
            .collect()
    }

    #[test]
    fn largest_consumer_wins_regardless_of_paint_order() {
        for order in [[[320, 180], [640, 360]], [[640, 360], [320, 180]]] {
            let mut channel = PaintedChannel::default();
            channel.begin();
            for extent in order {
                channel.note(&slot("cover"), extent);
            }
            let mut demand = PaintedDemand::default();
            demand.commit(channel.id, &mut channel.pass);
            assert_eq!(demand.extent("cover"), Some([640, 360]));
        }
    }

    #[test]
    fn a_removed_consumer_lets_the_demand_fall_on_the_next_pass() {
        let mut demand = PaintedDemand::default();
        demand.commit(1, &mut pass(&[("cover", [640, 360])]));
        let changed = demand.commit(1, &mut pass(&[("cover", [320, 180])]));
        assert_eq!(demand.extent("cover"), Some([320, 180]));
        assert_eq!(changed, vec![slot("cover")]);
        demand.commit(1, &mut Extents::new());
        assert_eq!(demand.extent("cover"), None, "no pass draws it any more");
    }

    #[test]
    fn channels_merge_by_maximum_and_retire_independently() {
        let mut demand = PaintedDemand::default();
        demand.commit(1, &mut pass(&[("cover", [320, 180])]));
        demand.commit(2, &mut pass(&[("cover", [200, 400])]));
        assert_eq!(demand.extent("cover"), Some([320, 400]));
        demand.retire(2);
        assert_eq!(demand.extent("cover"), Some([320, 180]));
    }

    #[test]
    fn unchanged_passes_do_not_advance_the_change_feed() {
        let mut demand = PaintedDemand::default();
        demand.commit(1, &mut pass(&[("a", [10, 10]), ("b", [20, 20])]));
        let seen = demand.revision();
        assert!(
            demand
                .commit(1, &mut pass(&[("a", [10, 10]), ("b", [20, 20])]))
                .is_empty()
        );
        assert_eq!(demand.revision(), seen);
        demand.commit(1, &mut pass(&[("a", [10, 10]), ("b", [40, 40])]));
        let mut changed = Vec::new();
        demand.changes_since(seen, &mut changed);
        assert_eq!(changed, vec![slot("b")]);
    }
}
