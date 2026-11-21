//! 拖动要认得的节点：文件卡片、侧栏文件夹行和文件列表框。
//!
//! 目录卡片、侧栏文件夹行和列表框用 NanaUI 的 `set_drop_target` 登记成条目拖放目标
//! （[`ENTRY_DROP`]），侧栏文件夹行同时接系统文件拖放；指针底下落在哪个目标由 `drop_target_at` 回答。框架只认节点，
//! 节点是哪个路径记在这里的登记表：挂上时写、作用域销毁时删。登记表按线程存，
//! 同一线程里先后建过的文档可能复用节点编号，所以每条登记带着节点的组装路径，查的时候对一遍。

use std::cell::RefCell;
use std::collections::HashMap;

use nana_ui::runtime::view::{NodeRef, on_cleanup, on_mount};
use nana_ui::runtime::{AppContext, LayoutBox, RuntimeDocument, StableNodeId};
use nana_ui_core::{DropAccepts, DropEffect, DropKind};

/// 条目拖动的载荷名。
pub(crate) const ENTRY_DROP: &str = "momobako-entry";

/// 登记的节点是什么。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    /// 文件列表里的一张卡片。目录卡片同时是放下目标。
    Card { directory: bool },
    /// 侧栏目录树的一行。
    Sidebar,
    /// 文件列表框：放在空白处就是放进当前目录。
    Browser,
}

/// 一个登记的节点：类型和条目路径（列表框没有路径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryNode {
    pub kind: EntryKind,
    pub path: String,
}

impl EntryNode {
    /// 这个节点接受的放下。普通文件卡片不是目标；侧栏文件夹行还接系统文件拖放（导入到这里，效果是复制），
    /// 悬停样子由 NanaUI 画在这一行上。
    fn accepts(&self) -> Option<DropAccepts> {
        let entry = DropKind::custom(ENTRY_DROP);
        match self.kind {
            EntryKind::Card { directory: false } => None,
            EntryKind::Sidebar => {
                Some(DropAccepts::new([DropKind::Files, entry]).effect(DropEffect::Copy))
            }
            EntryKind::Card { directory: true } | EntryKind::Browser => {
                Some(DropAccepts::new([entry]).effect(DropEffect::Move))
            }
        }
    }
}

struct Registered {
    /// 登记时节点的组装路径。
    assembly: String,
    entry: EntryNode,
}

thread_local! {
    static ENTRIES: RefCell<HashMap<StableNodeId, Registered>> = RefCell::new(HashMap::new());
}

/// `target` 建好后登记成 `entry`。在建 `target` 的那段视图里调用，跟着它的作用域走；
/// 目录、侧栏文件夹和列表框同时登记成拖放目标（见 [`EntryNode::accepts`]）。
pub(crate) fn register(target: NodeRef, entry: EntryNode) {
    on_mount(move |cx| {
        let Some(id) = target.get_untracked() else {
            eprintln!("Nana 拖动节点没有建出来，不登记：{entry:?}");
            return;
        };
        if let Some(accepts) = entry.accepts() {
            if let Err(error) = cx.set_drop_target_node(id, accepts) {
                eprintln!("Nana 条目拖放目标没有挂上：{error}");
            }
        }
        let Some(assembly) = cx.assembly_path(id) else {
            eprintln!("Nana 拖动节点没有组装路径，不登记：{entry:?}");
            return;
        };
        ENTRIES.with(|entries| {
            entries.borrow_mut().insert(
                id,
                Registered {
                    assembly: assembly.clone(),
                    entry,
                },
            )
        });
        on_cleanup(move || {
            ENTRIES.with(|entries| {
                let mut entries = entries.borrow_mut();
                if entries
                    .get(&id)
                    .is_some_and(|item| item.assembly == assembly)
                {
                    entries.remove(&id);
                }
            });
        });
    });
}

/// `id` 登记成了什么。节点已经换成别的（组装路径对不上）时没有。
pub(crate) fn entry_of(context: &AppContext, id: StableNodeId) -> Option<EntryNode> {
    let (assembly, entry) = ENTRIES.with(|entries| {
        entries
            .borrow()
            .get(&id)
            .map(|item| (item.assembly.clone(), item.entry.clone()))
    })?;
    (context.assembly_path(id)? == assembly).then_some(entry)
}

/// 这份文档里挂着的、满足 `keep` 的登记节点。
fn mounted(
    document: &RuntimeDocument,
    keep: impl Fn(&EntryNode) -> bool,
) -> Vec<(StableNodeId, EntryNode)> {
    let context = document.context();
    let world = context.world();
    let document_id = document.document();
    let candidates = ENTRIES.with(|entries| {
        entries
            .borrow()
            .iter()
            .filter(|(_, item)| keep(&item.entry))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    });
    candidates
        .into_iter()
        .filter(|id| {
            world
                .node(*id)
                .is_some_and(|node| node.document == document_id)
        })
        .filter_map(|id| entry_of(context, id).map(|entry| (id, entry)))
        .collect()
}

/// 文件列表框：节点和它的布局盒。列表框不在滚动区里，布局盒就是窗口坐标。
pub(crate) fn browser(document: &RuntimeDocument) -> Option<(StableNodeId, LayoutBox)> {
    let world = document.context().world();
    mounted(document, |entry| entry.kind == EntryKind::Browser)
        .into_iter()
        .find_map(|(id, _)| {
            world
                .layout_box(id)
                .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
                .map(|bounds| (id, bounds))
        })
}

/// 挂着的文件卡片：节点、路径和布局盒。布局盒是滚动前的内容坐标。
pub(crate) fn cards(document: &RuntimeDocument) -> Vec<(StableNodeId, String, LayoutBox)> {
    let world = document.context().world();
    mounted(document, |entry| {
        matches!(entry.kind, EntryKind::Card { .. })
    })
    .into_iter()
    .filter_map(|(id, entry)| {
        let bounds = world
            .layout_box(id)
            .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)?;
        Some((id, entry.path, bounds))
    })
    .collect()
}

/// 指针底下的文件夹：目录卡片或侧栏文件夹行。拖着的那张目录卡片自己（`exclude`）不算；
/// 侧栏上的同一路径仍算目标。
///
/// 放下目标按布局盒匹配，滚动区里的盒子停在滚动前的位置，所以先找到指针命中的节点，
/// 把窗口坐标换到它的布局坐标里再问 `drop_target_at`。指针不在窗口里的节点上时没有。
pub(crate) fn folder_at(
    document: &RuntimeDocument,
    x: f32,
    y: f32,
    exclude: &str,
) -> Option<FolderHit> {
    let context = document.context();
    let world = context.world();
    let document_id = document.document();
    let hit = world.hit_test(document_id, x, y)?;
    let (lx, ly) = world.pointer_layout_position(hit, x, y)?;
    let (target, _) = context.drop_target_at(document_id, lx, ly, &DropKind::custom(ENTRY_DROP))?;
    // 换算用的是命中节点所在的滚动区；目标不在命中节点的祖先里时，盒子只是碰巧对上了别处的坐标。
    if !within(world, hit, target) {
        return None;
    }
    let entry = entry_of(context, target)?;
    match entry.kind {
        EntryKind::Card { directory: true } if entry.path != exclude => Some(FolderHit {
            path: entry.path,
            sidebar: false,
        }),
        EntryKind::Sidebar if !entry.path.is_empty() => Some(FolderHit {
            path: entry.path,
            sidebar: true,
        }),
        _ => None,
    }
}

/// `node` 是 `ancestor` 自己或在它的子树里。
fn within(world: &nana_ui::runtime::UiWorld, node: StableNodeId, ancestor: StableNodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = world.parent_id(id);
    }
    false
}

/// 指针底下的文件夹。`sidebar` 为真时是目录树的行，否则是文件列表里的目录卡片。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderHit {
    pub path: String,
    pub sidebar: bool,
}

/// 窗口点在文件列表框里。
pub(crate) fn over_browser(document: &RuntimeDocument, x: f32, y: f32) -> bool {
    browser(document).is_some_and(|(_, bounds)| super::contains(bounds, x, y))
}
