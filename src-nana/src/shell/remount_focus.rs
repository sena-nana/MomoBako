//! 整棵壳层重挂时保住焦点和文本选区。
//!
//! 每次更新后 `mount_shell` 先卸掉旧树再挂新树，旧节点上的焦点随之消失：文本框打完一个字
//! 就失焦。这里在卸载前记下焦点节点的键路径、组件类型、在树里的位置和文本框的选区，新树
//! 挂上后找回键路径和类型都相同的节点，重新聚焦并恢复选区。节点被删、换了类型或键路径变了
//! 就不聚焦，只记日志。

use nana_ui::runtime::{ComponentTypeId, RuntimeDocument, StableNodeId, TextSelection, UiWorld};

/// 卸载前记下的焦点。
pub(super) struct KeptFocus {
    /// Nana 的键路径：从最外层带键的祖先到节点，用 `/` 连接。
    ///
    /// 组件槽位里的内容（标题栏中间、侧栏、浮层）是单独装配后放进去的，键路径从槽位内容
    /// 算起，不从壳层根算起，所以找回时按类型取候选再比键路径，不从根往下解析。
    path: String,
    component: ComponentTypeId,
    /// 从壳层根往下每层的子节点序号。键路径相同的候选不止一个时用它挑。
    position: Vec<usize>,
    /// 原节点是文本框时的主选区（锚点和焦点都是字节偏移）。
    selection: Option<TextSelection>,
}

/// 卸载前记下当前焦点。没有焦点、焦点不在这棵壳层里，或节点没有键路径时返回 `None`。
pub(super) fn capture(document: &RuntimeDocument, roots: &[StableNodeId]) -> Option<KeptFocus> {
    let context = document.context();
    let world = context.world();
    let focused = world.focused(document.document())?;
    let Some(position) = world_position(world, roots, focused) else {
        eprintln!("Nana 重挂前的焦点不在壳层树里，重挂后不再聚焦：{}", focused.get());
        return None;
    };
    let Some(path) = context.assembly_path(focused) else {
        eprintln!("Nana 重挂前的焦点节点没有键路径，重挂后不再聚焦：{}", focused.get());
        return None;
    };
    let Some(component) = world.component_type(focused).cloned() else {
        eprintln!("Nana 重挂前的焦点节点没有组件类型，重挂后不再聚焦：{path}");
        return None;
    };
    Some(KeptFocus {
        path,
        component,
        position,
        selection: world.text_input(focused).map(|view| view.selection),
    })
}

impl KeptFocus {
    /// 新树挂上后找回原焦点节点，重新聚焦；文本框再恢复选区。
    ///
    /// 算法：在新树里取同类型的节点，留下键路径相同的；只剩一个就是它，剩下多个再比树里的
    /// 位置。选区交给 Nana 的 `select_focused_text_range`，偏移超出新文本时由它钳到合法的字符边界。
    pub(super) fn restore(self, document: &mut RuntimeDocument, roots: &[StableNodeId]) {
        let document_id = document.document();
        let Some(target) = self.find(document, roots) else {
            return;
        };
        if let Err(error) = document.context_mut().focus_node(document_id, target) {
            eprintln!("Nana 重挂后恢复焦点失败：{} {error}", self.path);
            return;
        }
        if document.context().world().focused(document_id) != Some(target) {
            eprintln!("Nana 重挂后原焦点节点不可聚焦（可能已禁用）：{}", self.path);
            return;
        }
        let Some(selection) = self.selection else {
            return;
        };
        if let Err(error) = document.context_mut().select_focused_text_range(document_id, selection.anchor, selection.focus) {
            eprintln!("Nana 重挂后恢复文本选区失败：{} {error}", self.path);
        }
    }

    /// 新树里和原焦点同类型、同键路径的节点。找不到或分不清时记日志并返回 `None`。
    fn find(&self, document: &RuntimeDocument, roots: &[StableNodeId]) -> Option<StableNodeId> {
        let context = document.context();
        let world = context.world();
        let candidates = world
            .nodes_of_component(document.document(), self.component.as_str())
            .filter(|id| context.assembly_path(*id).as_deref() == Some(self.path.as_str()))
            .filter(|id| world_position(world, roots, *id).is_some())
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => {
                eprintln!("Nana 重挂后找不到原焦点节点，不再聚焦：{}", self.path);
                None
            }
            [only] => Some(*only),
            several => {
                let found = several
                    .iter()
                    .copied()
                    .find(|id| world_position(world, roots, *id).as_deref() == Some(self.position.as_slice()));
                if found.is_none() {
                    eprintln!("Nana 重挂后有 {} 个节点同键路径，分不清原焦点，不再聚焦：{}", several.len(), self.path);
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
