//! 壳层内容重挂时保住焦点、文本选区和滚动位置。
//!
//! 骨架常驻，侧栏、主区和浮层三块内容仍在同步时整块重挂，旧节点上的运行时状态随之消失：文本框
//! 打完一个字就失焦，滚动容器回到顶部。这里在卸掉一块之前，只在这块的根下面记下这些状态，新内容
//! 挂上后按同一套规则在新根下面找回对应节点再写回。
//!
//! 找回规则：节点的组件类型和 Nana 键路径（`assembly_path`）都相同才算同一个节点；键路径相同的
//! 候选不止一个时，再比它在壳层树里的位置（从根往下每层的子节点序号）。节点被删、换了类型或
//! 键路径变了就不恢复，只记日志。想在内容换了以后从头开始的滚动容器，用内容身份做键即可。

use nana_ui::runtime::{
    component_descriptors, ComponentTypeId, Entity, RuntimeDocument, ScrollOffset, ScrollView, StableNodeId,
    TextSelection, UiWorld,
};

/// 卸载前记下的全部状态。
pub(super) struct KeptState {
    focus: Option<KeptFocus>,
    scrolls: Vec<KeptScroll>,
}

/// 一个节点的身份：组件类型、键路径和树位置。
struct NodeIdentity {
    component: ComponentTypeId,
    /// Nana 的键路径：从最外层带键的祖先到节点，用 `/` 连接。
    ///
    /// 组件槽位里的内容（标题栏中间、侧栏、浮层）是单独装配后放进去的，键路径从槽位内容算起，
    /// 不从壳层根算起，所以找回时按类型取候选再比键路径，不从根往下解析。
    path: String,
    /// 从壳层根往下每层的子节点序号，第一项是根的序号。
    position: Vec<usize>,
}

/// 焦点节点，文本框再带上主选区（锚点和焦点都是字节偏移）。
struct KeptFocus {
    identity: NodeIdentity,
    selection: Option<TextSelection>,
}

/// 一个滚动容器的偏移。偏移为零的不记。
struct KeptScroll {
    identity: NodeIdentity,
    offset: ScrollOffset,
}

/// 卸载前记下焦点、选区和各滚动容器的偏移。只看这棵壳层里的节点。
pub(super) fn capture(document: &RuntimeDocument, roots: &[StableNodeId]) -> KeptState {
    KeptState { focus: capture_focus(document, roots), scrolls: capture_scrolls(document, roots) }
}

/// 焦点在这块内容里才记。焦点在别的内容或常驻骨架上时不归这次重挂管。
fn capture_focus(document: &RuntimeDocument, roots: &[StableNodeId]) -> Option<KeptFocus> {
    let world = document.context().world();
    let focused = world.focused(document.document())?;
    let position = world_position(world, roots, focused)?;
    let identity = identify(document, focused, position, "焦点")?;
    Some(KeptFocus { identity, selection: world.text_input(focused).map(|view| view.selection) })
}

/// 壳层树里偏移不为零的滚动容器。
fn capture_scrolls(document: &RuntimeDocument, roots: &[StableNodeId]) -> Vec<KeptScroll> {
    let world = document.context().world();
    world
        .nodes_of_component(document.document(), component_descriptors::SCROLL_VIEW.type_id)
        .filter_map(|id| {
            let offset = world.scroll_offset(id)?;
            if offset.x == 0.0 && offset.y == 0.0 {
                return None;
            }
            let position = world_position(world, roots, id)?;
            let identity = identify(document, id, position, "滚动容器")?;
            Some(KeptScroll { identity, offset })
        })
        .collect()
}

/// 节点的身份。没有键路径或没有组件类型时记日志并返回 `None`。
fn identify(document: &RuntimeDocument, id: StableNodeId, position: Vec<usize>, what: &str) -> Option<NodeIdentity> {
    let context = document.context();
    let world = context.world();
    let Some(path) = context.assembly_path(id) else {
        eprintln!("Nana 重挂前的{what}没有键路径，重挂后不再恢复：{}", id.get());
        return None;
    };
    let Some(component) = world.component_type(id).cloned() else {
        eprintln!("Nana 重挂前的{what}没有组件类型，重挂后不再恢复：{path}");
        return None;
    };
    Some(NodeIdentity { component, path, position })
}

impl KeptState {
    /// 新树挂上后写回。先写滚动偏移，虚拟列表收到滚动变化后按新视口挂行；再恢复焦点和选区。
    pub(super) fn restore(self, document: &mut RuntimeDocument, roots: &[StableNodeId]) {
        for scroll in self.scrolls {
            scroll.restore(document, roots);
        }
        if let Some(focus) = self.focus {
            focus.restore(document, roots);
        }
    }
}

impl KeptScroll {
    /// 用 Nana 的 `scroll_to` 写回偏移：它会发滚动变化，虚拟列表据此重新算窗口。
    /// 第一次布局前没有滚动尺寸，偏移原样写入；布局后内容变短时由运行时钳到范围内。
    fn restore(self, document: &mut RuntimeDocument, roots: &[StableNodeId]) {
        let Some(target) = self.identity.find(document, roots, "滚动容器") else {
            return;
        };
        if let Err(error) = document.context_mut().scroll_to(Entity::<ScrollView>::from_stable_id(target), self.offset) {
            eprintln!("Nana 重挂后恢复滚动位置失败：{} {error}", self.identity.path);
        }
    }
}

impl KeptFocus {
    /// 原地重新聚焦（不滚动，刚写回的滚动偏移不被冲掉）；文本框再用 `select_focused_text_range`
    /// 恢复选区，偏移超出新文本时由它钳到合法的字符边界。
    fn restore(self, document: &mut RuntimeDocument, roots: &[StableNodeId]) {
        let document_id = document.document();
        let Some(target) = self.identity.find(document, roots, "焦点") else {
            return;
        };
        if let Err(error) = document.context_mut().focus_node_in_place(document_id, target) {
            eprintln!("Nana 重挂后恢复焦点失败：{} {error}", self.identity.path);
            return;
        }
        if document.context().world().focused(document_id) != Some(target) {
            eprintln!("Nana 重挂后原焦点节点不可聚焦（可能已禁用）：{}", self.identity.path);
            return;
        }
        let Some(selection) = self.selection else {
            return;
        };
        if let Err(error) = document.context_mut().select_focused_text_range(document_id, selection.anchor, selection.focus) {
            eprintln!("Nana 重挂后恢复文本选区失败：{} {error}", self.identity.path);
        }
    }
}

impl NodeIdentity {
    /// 新树里同类型、同键路径的节点。只剩一个就是它，剩下多个再比树位置；找不到或分不清时记日志。
    fn find(&self, document: &RuntimeDocument, roots: &[StableNodeId], what: &str) -> Option<StableNodeId> {
        let context = document.context();
        let world = context.world();
        let candidates = world
            .nodes_of_component(document.document(), self.component.as_str())
            .filter(|id| context.assembly_path(*id).as_deref() == Some(self.path.as_str()))
            .filter(|id| world_position(world, roots, *id).is_some())
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => {
                eprintln!("Nana 重挂后找不到原{what}，不再恢复：{}", self.path);
                None
            }
            [only] => Some(*only),
            several => {
                let found = several
                    .iter()
                    .copied()
                    .find(|id| world_position(world, roots, *id).as_deref() == Some(self.position.as_slice()));
                if found.is_none() {
                    eprintln!("Nana 重挂后有 {} 个{what}同键路径，分不清原节点，不再恢复：{}", several.len(), self.path);
                }
                found
            }
        }
    }
}

/// 节点在壳层树里的位置：从根往下每层的子节点序号，第一项是根的序号。
/// 不在这些根下时返回 `None`。
fn world_position(world: &UiWorld, roots: &[StableNodeId], node: StableNodeId) -> Option<Vec<usize>> {
    let mut steps = Vec::new();
    let mut cursor = node;
    loop {
        if let Some(index) = roots.iter().position(|root| *root == cursor) {
            steps.push(index);
            steps.reverse();
            return Some(steps);
        }
        let parent = world.parent_id(cursor)?;
        let index = world.node(parent)?.children.iter().position(|child| *child == cursor)?;
        steps.push(index);
        cursor = parent;
    }
}

#[cfg(test)]
#[path = "remount_state_tests.rs"]
mod tests;
