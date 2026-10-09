//! 设置、插件、日志、拓展和动作页常驻视图共用的绑定工具。
//!
//! - [`sync_rows`]：把 Store 里的列表写成新投影，留下的项相对顺序没变时只删掉没有了的、插入新的、
//!   改内容变了的，留下的行一个都不重建；
//! - [`row_text`]、[`row_flag`]：列表行里的字段绑定，行已经删掉时读默认值；
//! - [`row_draft`]：列表行里受控输入框的草稿，行里的 ViewModel 值相对上次投影变了才写回；
//! - [`StyleField`]、[`ButtonIcon`]、[`ReadOnly`]：控件表里没有的可绑定字段。

use std::collections::HashSet;
use std::hash::Hash;

use nana_ui::runtime::view::{untrack, watch_effect, FieldWrite, StoreList, StorePath, StyledComponent};
use nana_ui::runtime::{Button, NodeStyle, TextInput};
use nana_ui_core::Icon;

use super::super::hot::ModelField;

/// [`sync_rows`] 对一次写入的打算。
#[derive(Debug, PartialEq, Eq)]
enum RowPlan {
    /// 和现在一样，不写。
    Same,
    /// 留下的项相对顺序没变：删掉不在新投影里的项，在这些最终下标插入新项，再改这些下标上内容变了的项。
    Patch { remove: bool, insert: Vec<usize>, change: Vec<usize> },
    /// 其余情况（重排、重复键）整体写。
    Whole,
}

/// 把 Store 里的列表 `list` 写成 `next`。
///
/// 整体写会让每一行的绑定都再比较一遍（行按键保留，节点不换）。留下的项相对顺序没变时按差异写：
/// 删掉没有了的项、插入新项、只改内容变了的项。新日志到达、最旧的一条掉出缓存时，只建新的那一行，
/// 已有行的绑定一个都不重跑。
pub(crate) fn sync_rows<P, T, K>(list: P, key_of: fn(&T) -> K, next: Vec<T>)
where
    P: StoreList<T>,
    T: Clone + PartialEq + 'static,
    K: Hash + Eq + 'static,
{
    match untrack(|| list.with(|current| plan_rows(current, &next, key_of))) {
        RowPlan::Same => {}
        RowPlan::Patch { remove, insert, change } => {
            if remove {
                let keep = next.iter().map(key_of).collect::<HashSet<_>>();
                list.retain(|item| keep.contains(&key_of(item)));
            }
            // 按最终位置从小到大插：插第 i 项时前面的项都已就位。
            for index in insert {
                list.insert(index, next[index].clone());
            }
            let keyed = list.keyed(key_of);
            for index in change {
                keyed.at(&key_of(&next[index])).set(next[index].clone());
            }
        }
        RowPlan::Whole => list.set(next),
    }
}

/// 比较现有列表和新投影，决定怎么写。任一边有重复键、或者留下的项换了相对顺序时整体写。
fn plan_rows<T: PartialEq, K: Hash + Eq>(current: &[T], next: &[T], key_of: fn(&T) -> K) -> RowPlan {
    let mut next_keys = HashSet::with_capacity(next.len());
    if !next.iter().all(|item| next_keys.insert(key_of(item))) {
        return RowPlan::Whole;
    }
    let mut current_keys = HashSet::with_capacity(current.len());
    if !current.iter().all(|item| current_keys.insert(key_of(item))) {
        return RowPlan::Whole;
    }
    let kept = current.iter().filter(|item| next_keys.contains(&key_of(item))).collect::<Vec<_>>();
    let mut remaining = kept.iter();
    let mut insert = Vec::new();
    let mut change = Vec::new();
    for (index, item) in next.iter().enumerate() {
        if !current_keys.contains(&key_of(item)) {
            insert.push(index);
            continue;
        }
        match remaining.next() {
            Some(&old) if key_of(old) == key_of(item) => {
                if old != item {
                    change.push(index);
                }
            }
            _ => return RowPlan::Whole,
        }
    }
    let remove = kept.len() != current.len();
    if !remove && insert.is_empty() && change.is_empty() {
        RowPlan::Same
    } else {
        RowPlan::Patch { remove, insert, change }
    }
}

/// 行里的一段文字。行已经删掉时读空串，不去读不存在的行。
pub(crate) fn row_text<P: StorePath>(item: P, pick: fn(&P::Value) -> &String) -> impl Fn() -> String + Send + 'static {
    move || item.try_with(|row| pick(row).clone()).unwrap_or_default()
}

/// 行里按字段算出的值。行已经删掉时读默认值。
pub(crate) fn row_flag<P: StorePath, R: Default + 'static>(item: P, pick: fn(&P::Value) -> R) -> impl Fn() -> R + Send + 'static {
    move || item.try_with(pick).unwrap_or_default()
}

/// 列表行里受控输入框的草稿：在行的作用域里建，随行一起回收。`read` 读这一行在 ViewModel 里的值
/// （行已经删掉时为 `None`），值相对上次投影变了才写进输入框的信号，刚打的字不会被较旧的值冲掉。
pub(crate) fn row_draft(read: impl Fn() -> Option<String> + 'static) -> ModelField {
    let draft = ModelField::new(&untrack(&read).unwrap_or_default());
    watch_effect(move || {
        if let Some(value) = read() {
            untrack(|| draft.sync(&value));
        }
    });
    draft
}

/// 整份节点样式：外观随状态整套换（分段按钮的选中态、状态胶囊的语气）。
/// 不要和 `.visible(..)` 绑在同一个节点上，整份写会把显隐冲掉。
pub(crate) struct StyleField;

impl<C: StyledComponent> FieldWrite<C, NodeStyle> for StyleField {
    const FIELD: &'static str = "style";

    fn write(target: &mut C, style: NodeStyle) {
        *target.node_style_mut() = style;
    }

    fn differs(target: &C, style: &NodeStyle) -> bool {
        target.node_style() != style
    }
}

/// `style::action` 按钮的禁用：Vue 原生按钮禁用时整体 45% 不透明，不换颜色，所以和禁用标志一起写透明度。
pub(crate) struct ActionDisabled;

impl FieldWrite<Button, bool> for ActionDisabled {
    const FIELD: &'static str = "Button.disabled+opacity";

    fn write(target: &mut Button, disabled: bool) {
        target.disabled = disabled;
        let opacity = disabled.then_some(super::style::DISABLED_OPACITY);
        if target.style.layout.opacity != opacity {
            std::sync::Arc::make_mut(&mut target.style.layout).opacity = opacity;
        }
    }

    fn differs(target: &Button, disabled: &bool) -> bool {
        target.disabled != *disabled || target.style.layout.opacity != disabled.then_some(super::style::DISABLED_OPACITY)
    }
}

/// 按钮的前置图标：暂停和恢复、执行中的转圈。
pub(crate) struct ButtonIcon;

impl FieldWrite<Button, Option<Icon>> for ButtonIcon {
    const FIELD: &'static str = "Button.icon";

    fn write(target: &mut Button, icon: Option<Icon>) {
        target.icon = icon;
    }

    fn differs(target: &Button, icon: &Option<Icon>) -> bool {
        target.icon != *icon
    }
}

/// 输入框只读：API Playground 的目标框只有 HTTP 端点能改。
pub(crate) struct ReadOnly;

impl FieldWrite<TextInput, bool> for ReadOnly {
    const FIELD: &'static str = "TextInput.read_only";

    fn write(target: &mut TextInput, read_only: bool) {
        target.read_only = read_only;
    }

    fn differs(target: &TextInput, read_only: &bool) -> bool {
        target.read_only != *read_only
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_rows, RowPlan};

    fn key(item: &(u32, &'static str)) -> u32 {
        item.0
    }

    fn patch(remove: bool, insert: &[usize], change: &[usize]) -> RowPlan {
        RowPlan::Patch { remove, insert: insert.to_vec(), change: change.to_vec() }
    }

    #[test]
    fn plans_follow_keys_and_contents() {
        let current = [(1, "a"), (2, "b")];
        assert_eq!(plan_rows(&current, &[(1, "a"), (2, "b")], key), RowPlan::Same);
        assert_eq!(plan_rows(&current, &[(1, "a"), (2, "c")], key), patch(false, &[], &[1]));
        assert_eq!(plan_rows(&current, &[(3, "c"), (1, "a"), (2, "b")], key), patch(false, &[0], &[]), "新项插在最前");
        assert_eq!(plan_rows(&current, &[(1, "a"), (3, "c"), (2, "b"), (4, "d")], key), patch(false, &[1, 3], &[]));
        assert_eq!(plan_rows(&current, &[(3, "c"), (1, "a")], key), patch(true, &[0], &[]), "新的进来、最旧的掉出去");
        assert_eq!(plan_rows(&current, &[(3, "c"), (1, "x"), (2, "b")], key), patch(false, &[0], &[1]), "留下的项变了");
        assert_eq!(plan_rows(&current, &[(1, "a")], key), patch(true, &[], &[]), "删了项");
        assert_eq!(plan_rows(&current, &[(2, "b"), (1, "a")], key), RowPlan::Whole, "换了顺序");
        assert_eq!(plan_rows(&current, &[(1, "a"), (1, "b"), (2, "b")], key), RowPlan::Whole, "重复键");
    }
}
