//! 把投影出的整份列表按键写回 Store 列表：侧栏的三份树、播放集页的条目、搜索结果、日志和插件面板共用。
//!
//! 整体 `set` 会让每一行的绑定都再比较一遍。这里按键做差异，只发结构操作和变了的那几行：
//! 删掉新列表里没有的行，按最终位置插入新行，内容变了的行整行改写（只重跑读这一行的绑定），
//! 删、插之后顺序还和新列表不同时再按新位置排一次。删、插、排都只通知列表本身，留下的行一个绑定都不重跑。
//!
//! 键是行的身份，同一列表里不该重复：新列表里重复的键只留第一行（和带键的 `each` 只建第一行一致），
//! 现有列表里重复的键在删的那一步只留第一行。

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use nana_ui::runtime::view::{untrack, StoreList, StorePath};

/// 一次写入要做的结构操作，按新列表（已经去掉重复键）的下标记。
#[derive(Debug, PartialEq, Eq)]
struct RowPlan {
    /// 有要删的行：现有列表里新列表没有的键，或者现有列表里重复的键。
    remove: bool,
    /// 新行在新列表里的下标，从小到大：插第 i 行时前面的行都已就位。
    insert: Vec<usize>,
    /// 内容变了的已有行在新列表里的下标。
    change: Vec<usize>,
    /// 删、插之后留下的行相对顺序和新列表不同，要按新位置排一次。
    reorder: bool,
}

/// 把 Store 里的列表 `list` 写成 `rows`。和现在一样时什么都不写。
///
/// 只在同步时调用（不在副作用里），读现有列表不建立依赖。
pub(crate) fn sync_rows<P, T, K>(list: P, key_of: fn(&T) -> K, rows: Vec<T>)
where
    P: StoreList<T>,
    T: Clone + PartialEq + 'static,
    K: Hash + Eq + 'static,
{
    let next = unique_rows(rows, key_of);
    let Some(plan) = untrack(|| list.with(|current| plan_rows(current, &next, key_of))) else {
        return;
    };
    let position = next.iter().enumerate().map(|(index, row)| (key_of(row), index)).collect::<HashMap<_, _>>();
    if plan.remove {
        let mut kept = HashSet::with_capacity(next.len());
        list.retain(|row| {
            let key = key_of(row);
            position.contains_key(&key) && kept.insert(key)
        });
    }
    for index in plan.insert {
        list.insert(index, next[index].clone());
    }
    let keyed = list.keyed(key_of);
    for index in plan.change {
        keyed.at(&key_of(&next[index])).set(next[index].clone());
    }
    if plan.reorder {
        list.sort_by_key(|row| position.get(&key_of(row)).copied().unwrap_or(usize::MAX));
    }
}

/// 去掉键重复的行，只留第一行。
fn unique_rows<T, K: Hash + Eq>(rows: Vec<T>, key_of: fn(&T) -> K) -> Vec<T> {
    let mut seen = HashSet::with_capacity(rows.len());
    rows.into_iter().filter(|row| seen.insert(key_of(row))).collect()
}

/// 比较现有列表和新列表（没有重复键），算出要做的操作；一样时为 `None`。
///
/// 算法：现有列表里每个键只认第一行。新列表里现有列表没有的键是插入，有的键内容不同是改写；
/// 现有列表里新列表没有的键、以及重复的键要删。删掉以后留下的行保持原来的相对顺序，新行按最终下标
/// 从小到大插入，所以只要留下的行相对顺序和新列表里一样，插完就是新列表的顺序；否则再排一次。
fn plan_rows<T: PartialEq, K: Hash + Eq>(current: &[T], next: &[T], key_of: fn(&T) -> K) -> Option<RowPlan> {
    if current == next {
        return None;
    }
    let mut first = HashMap::with_capacity(current.len());
    let mut duplicated = false;
    for row in current {
        match first.entry(key_of(row)) {
            Entry::Occupied(_) => duplicated = true,
            Entry::Vacant(slot) => {
                slot.insert(row);
            }
        }
    }
    let wanted = next.iter().map(key_of).collect::<HashSet<_>>();
    let remove = duplicated || first.keys().any(|key| !wanted.contains(key));
    let mut insert = Vec::new();
    let mut change = Vec::new();
    for (index, row) in next.iter().enumerate() {
        match first.get(&key_of(row)) {
            None => insert.push(index),
            Some(previous) if *previous != row => change.push(index),
            Some(_) => {}
        }
    }
    let kept = current.iter().filter_map(|row| {
        let key = key_of(row);
        let first_of_key = first.get(&key).is_some_and(|first| std::ptr::eq(*first, row));
        (first_of_key && wanted.contains(&key)).then_some(key)
    });
    let reorder = kept.ne(next.iter().map(key_of).filter(|key| first.contains_key(key)));
    Some(RowPlan { remove, insert, change, reorder })
}

#[cfg(test)]
#[path = "row_sync_tests.rs"]
mod tests;
