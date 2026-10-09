//! 壳层内容块的统一接口：侧栏、主区、浮层各实现一份，`ShellView` 只负责调度和放进骨架槽位。
//!
//! 一块内容分两部分：常驻的信号（[`ShellPart::Signals`]，在骨架的挂载闭包里建，跟骨架一起回收），
//! 和挂在槽位里的节点。同步时先 [`ShellPart::sync`] 写信号，再按 [`ShellPart::needs_remount`]
//! 决定要不要重挂；焦点在这块里、输入法还有预编辑时（[`ShellPart::composing`]）重挂延后到组合结束。
//! 重挂换下来的旧内容和要找回的焦点、滚动放进 [`Swap`]，等新根放进槽位以后再收尾。

use nana_ui::runtime::view::AnyView;
use nana_ui::runtime::{FrameworkError, MountedView, RuntimeDocument, StableNodeId};

use super::hot::{self, HotSignals};
use super::remount_state::KeptState;
use super::view_host::ViewStats;
use super::{ShellViewModel, StartupStatus};

/// 工作台怎么排：有侧栏时是资源区加主区，启动未就绪或侧栏收起时主区独占一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyMode {
    Workbench,
    Solo,
}

/// 侧栏呈现宽度大于半像素、启动已就绪时才放侧栏。折叠动画走完才换成主区独占。
pub(crate) fn body_mode(model: &ShellViewModel) -> BodyMode {
    let presented = model.motion.sidebar_presented_width();
    if presented > 0.5 && model.workspace.startup.status == StartupStatus::Ready {
        BodyMode::Workbench
    } else {
        BodyMode::Solo
    }
}

/// 哪一块，延后重挂的记账和日志用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartId {
    Sidebar,
    Primary,
    Overlay,
}

/// 同步一块内容时手边的东西：文档、热信号和视图计数。
pub(crate) struct PartCx<'a> {
    pub document: &'a mut RuntimeDocument,
    pub hot: HotSignals,
    pub stats: &'a mut ViewStats,
}

/// 壳层内容块。实现者只改自己的模块和区域文件，不必碰 `view_host.rs`。
pub(crate) trait ShellPart {
    const ID: PartId;
    /// 常驻的信号和 Store 句柄。整块重挂时不重建，绑定在新节点上接着读。
    type Signals: Copy;

    /// 在骨架的挂载闭包里建本块的信号，初值是这一刻的投影。
    fn signals(model: &ShellViewModel) -> Self::Signals;
    fn new(signals: Self::Signals) -> Self;
    /// 放进骨架槽位的根节点；这块现在没有内容时为 `None`。
    fn root(&self) -> Option<StableNodeId>;
    /// 按 `mode` 从头挂这一块：首次挂载，或者工作台排法变了。不等输入法组合。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, mode: BodyMode) -> Result<Swap, FrameworkError>;
    /// 只写本块的信号，不建节点。每次整体同步都调用，组合输入中也调用。
    fn sync(&mut self, model: &ShellViewModel);
    /// ViewModel 相对上次挂载有没有本块必须重挂才能跟上的变化。
    fn needs_remount(&self, model: &ShellViewModel) -> bool;
    /// 重挂本块需要重挂的部分（整块，或主区的当前路由分支）。
    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError>;
    /// 焦点在本块里，而且输入法正在组合（预编辑非空）。这时重挂会打断组合，要延后。
    fn composing(&self, document: &RuntimeDocument) -> bool;
}

/// 一次重挂留给调度方收尾的事：新根放进槽位以后卸掉换下来的旧内容，再在新内容里找回焦点、选区和滚动。
#[derive(Default)]
pub(crate) struct Swap {
    retired: Vec<MountedView>,
    restore: Vec<(KeptState, Vec<StableNodeId>)>,
}

impl Swap {
    /// 换下 `old`，在 `fresh` 下面找回 `kept`。
    pub(crate) fn replace(old: Option<MountedView>, kept: Option<KeptState>, fresh: Option<&MountedView>) -> Self {
        let mut swap = Self::default();
        swap.retired.extend(old);
        if let (Some(kept), Some(fresh)) = (kept, fresh) {
            swap.restore.push((kept, fresh.roots().to_vec()));
        }
        swap
    }

    /// 只在 `roots` 下面找回 `kept`，不换下什么。
    pub(crate) fn restore(kept: KeptState, roots: Vec<StableNodeId>) -> Self {
        Self { retired: Vec::new(), restore: vec![(kept, roots)] }
    }

    pub(crate) fn merge(&mut self, other: Swap) {
        self.retired.extend(other.retired);
        self.restore.extend(other.restore);
    }

    /// 卸掉旧内容，再按记下的身份找回状态。旧内容卸不掉只记日志，不影响新内容。
    pub(crate) fn finish(self, document: &mut RuntimeDocument) {
        for old in self.retired {
            if let Err(error) = old.unmount(document.context_mut()) {
                eprintln!("Nana 卸掉换下来的壳层内容失败：{error}");
            }
        }
        let document_id = document.document();
        for (kept, roots) in self.restore {
            kept.restore(document.context_mut(), document_id, &roots);
        }
    }
}

/// 把视图挂成脱离树的一块，建的时候把热信号交给旧视图函数。没有节点时卸掉空挂载返回 `None`。
pub(crate) fn mount_detached(
    document: &mut RuntimeDocument,
    hot: HotSignals,
    what: PartId,
    build: impl FnOnce() -> Option<AnyView>,
) -> Result<Option<MountedView>, FrameworkError> {
    let document_id = document.document();
    let context = document.context_mut();
    let mounted = context.mount_view_detached(document_id, move || hot::with_signals(hot, build))?;
    match mounted.roots().len() {
        0 => {
            mounted.unmount(context)?;
            Ok(None)
        }
        1 => Ok(Some(mounted)),
        count => {
            eprintln!("Nana 壳层内容 {what:?} 应该只有一个根，实际 {count} 个，只放第一个");
            Ok(Some(mounted))
        }
    }
}

/// 焦点在 `roots`（含自身）下面，而且输入法正在组合。
pub(crate) fn composing_under(document: &RuntimeDocument, roots: &[StableNodeId]) -> bool {
    let world = document.context().world();
    let Some(focused) = world.focused(document.document()) else {
        return false;
    };
    roots.iter().any(|root| world.is_descendant_or_self(focused, *root))
        && world.ime(focused).is_some_and(|ime| !ime.text.is_empty())
}

/// 挂载的第一个根。
pub(crate) fn first_root(view: Option<&MountedView>) -> Option<StableNodeId> {
    view.and_then(|view| view.roots().first().copied())
}
