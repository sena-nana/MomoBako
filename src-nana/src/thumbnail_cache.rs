//! 缩略图宿主纹理的槽位账本：哪些缩略图登记了纹理、什么时候卸。
//!
//! 纹理按缩略图文件登记一次（槽 `thumb:{文件}`），句柄留在这里，空闲帧不再登记。回收规则：
//! - 在用的不卸：文档里还有缩略图节点引用它（视口和前后预留的行、详情卡片、播放条、预览），卸了就会
//!   闪回类型图标；
//! - 不在用、也不在当前列表里的立刻卸：换了目录或仓库，列表重读以后没了这一条；
//! - 不在用但还在当前列表里的（滚出了视口）留作缓存，总数超过上限时按最近最少使用卸。
//!
//! 「最近使用」是最后一次在用的回收轮次，登记时也算用过一次。在用的数量本身超过上限时一张也不卸，上限
//! 只约束缓存。卸掉的文件交回调用方：由它从宿主纹理表里移除，并让这些行回到类型图标、再挂出来时重新解码。

use std::collections::{HashMap, HashSet};

/// 缩略图纹理缓存的上限（张）。缩略图最长边 480，一张最多约 0.9 MB。
pub(crate) const THUMBNAIL_CAPACITY: usize = 240;

struct Entry<T> {
    handle: T,
    /// 最后一次在用（或登记）时的回收轮次。
    last_used: u64,
}

/// 缩略图文件到纹理句柄的账本。`T` 在产品里是宿主纹理句柄，测试里是任意值。
pub(crate) struct ThumbnailCache<T> {
    entries: HashMap<String, Entry<T>>,
    round: u64,
}

impl<T> Default for ThumbnailCache<T> {
    fn default() -> Self {
        Self { entries: HashMap::new(), round: 0 }
    }
}

impl<T> ThumbnailCache<T> {
    /// 记下新登记的纹理；同一个文件重新解码时换掉旧句柄，返回旧的。
    pub(crate) fn insert(&mut self, path: String, handle: T) -> Option<T> {
        let entry = Entry { handle, last_used: self.round };
        self.entries.insert(path, entry).map(|old| old.handle)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// 回收一轮，返回卸掉的文件（先是不在列表里的，按文件名排；再是超过上限按最近最少使用卸的）。
    ///
    /// 算法：轮次加一，在用的记成这一轮用过；不在用又不在 `listed` 里的全部卸掉；剩下的超过 `capacity`
    /// 时，把不在用的按最后使用的轮次从早到晚（同一轮按文件名）卸，直到不超过上限或没有不在用的。
    pub(crate) fn sweep(&mut self, in_use: &HashSet<String>, listed: &HashSet<String>, capacity: usize) -> Vec<String> {
        self.round += 1;
        let round = self.round;
        for (path, entry) in &mut self.entries {
            if in_use.contains(path) {
                entry.last_used = round;
            }
        }
        let mut evicted: Vec<String> = self
            .entries
            .keys()
            .filter(|path| !in_use.contains(*path) && !listed.contains(*path))
            .cloned()
            .collect();
        evicted.sort();
        for path in &evicted {
            self.entries.remove(path);
        }
        if self.entries.len() > capacity {
            let mut idle: Vec<(u64, String)> = self
                .entries
                .iter()
                .filter(|(path, _)| !in_use.contains(*path))
                .map(|(path, entry)| (entry.last_used, path.clone()))
                .collect();
            idle.sort();
            let excess = self.entries.len() - capacity;
            for (_, path) in idle.into_iter().take(excess) {
                self.entries.remove(&path);
                evicted.push(path);
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::ThumbnailCache;

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    fn cache(paths: &[&str]) -> ThumbnailCache<u32> {
        let mut cache = ThumbnailCache::default();
        for (index, path) in paths.iter().enumerate() {
            cache.insert(path.to_string(), index as u32);
        }
        cache
    }

    /// 换了目录：不在用、不在列表里的立刻卸，还挂着的（旧卡片还没卸）留到不挂了再卸。
    #[test]
    fn unlisted_textures_go_once_nothing_shows_them() {
        let mut thumbs = cache(&["a", "b", "c"]);
        assert_eq!(thumbs.sweep(&set(&["c"]), &set(&["x"]), 240), ["a", "b"]);
        assert!(thumbs.contains("c"), "旧卡片还挂着，不卸");
        assert_eq!(thumbs.sweep(&set(&["x"]), &set(&["x"]), 240), ["c"]);
        assert_eq!(thumbs.len(), 0);
    }

    /// 超过上限时只卸不在用的，按最后一次在用从早到晚；在用的再多也不卸。
    #[test]
    fn over_capacity_evicts_the_least_recently_shown() {
        let mut thumbs = cache(&["a", "b", "c", "d"]);
        let listed = set(&["a", "b", "c", "d", "e"]);
        assert!(thumbs.sweep(&set(&["a", "b"]), &listed, 4).is_empty());
        assert!(thumbs.sweep(&set(&["c"]), &listed, 4).is_empty(), "不超过上限时滚出视口的留作缓存");
        thumbs.insert("e".into(), 4);
        assert_eq!(thumbs.sweep(&set(&["e"]), &listed, 4), ["d"], "d 从登记以后没再在用，最早");
        assert_eq!(thumbs.sweep(&set(&["e"]), &listed, 2), ["a", "b"], "a、b 比 c 早一轮");
        assert_eq!(thumbs.sweep(&set(&["c", "e"]), &listed, 1), Vec::<String>::new(), "在用的超过上限也不卸");
        assert_eq!(thumbs.len(), 2);
    }

    /// 同一个文件重新解码：换掉旧句柄，不多占一个位置。
    #[test]
    fn re_decoding_replaces_the_handle() {
        let mut thumbs = cache(&["a"]);
        assert_eq!(thumbs.insert("a".into(), 9), Some(0));
        assert_eq!(thumbs.len(), 1);
    }
}
