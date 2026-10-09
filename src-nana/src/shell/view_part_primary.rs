//! 主区块：主区外框常驻，里面用 `dynamic` 按 [`RouteKey`] 切换路由分支。
//!
//! 外框是白底 `Lg` 圆角的一层（主区独占时外面再包一层壳层底色），放进工作区的主区或 AppShell 的
//! body。外框里只有一个结构块 `dynamic(RouteSlot)`：
//! - 常驻路由（现在是启动页）的分支只在进入路由时建一次，之后同步只写它的信号；
//! - 其余路由仍由旧视图函数整块建出。换到这种路由时，同步在本线程把新内容挂成脱离树的一块放进
//!   交接处，把路由和版本号写进键；下一次刷新 `dynamic` 换出新分支，分支的 `on_mount` 把这块内容
//!   挂进路由容器、卸掉上一块，再找回焦点、选区和滚动。留在同一个旧视图路由里、内容变了时不经过
//!   `dynamic`，同步当场换掉路由容器里的内容，和改动前一样不留时间差，紧接着的按键落在新节点上。
//!
//! 内容变了只重挂当前分支，外框、侧栏和浮层都不动。每个路由的入口函数在自己的 `route_*.rs` 里；
//! 把某个路由改成常驻时，在 [`resident`]、[`resident_view`] 和 [`legacy_view`] 里改它自己那一行，
//! 状态放进 [`RouteSignals`]。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use nana_ui::runtime::view::{css, dynamic, node_ref, on_mount, signal, widget, AnyView, IntoView, NodeRef, Signal};
use nana_ui::runtime::{
    AppContext, FrameworkError, LengthSpec, MountedView, MutationQueue, RadiusTier, RuntimeDocument, SemanticColorRole,
    Stack, StableNodeId,
};

use super::hot::HotSignals;
use super::remount_state::{self, KeptState};
use super::route_empty::{EmptySignals, EmptyView};
use super::route_missing::MissingSignals;
use super::route_startup::{StartupSignals, StartupView};
use super::view_part::{composing_under, first_root, mount_detached, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::{MainRegion, ShellPage, ShellViewModel, WorkspacePanel};

/// 主区显示哪一种页面。和对应 Vue 路由的取舍一致：启动未就绪（含启动失败）只显示启动页；
/// 就绪后设置页走设置路由，其余按区域和工作区面板分到首页的各个路由。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RouteKey {
    /// 启动页和加载失败。
    Startup,
    /// 设置页。
    Settings,
    /// 文件面板，含回收站、智能文件夹和文件预览。
    Files,
    /// 有仓库时的搜索结果。
    Search,
    /// 还没有资源库时的搜索面板。
    EmptySearch,
    /// 播放集。
    Playlists,
    /// 系统日志。
    Logs,
    /// 拓展。
    Extensions,
    /// 仓库动作。
    Actions,
    /// 资源库丢失。
    Missing,
    /// 还没有资源库。
    Empty,
    /// 首页没有可显示的面板。
    Blank,
}

impl RouteKey {
    /// 按 ViewModel 取当前路由。
    pub(crate) fn of(model: &ShellViewModel) -> Self {
        let settings_page = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
        let panel = model.workspace.panel;
        match model.workspace.main_region() {
            MainRegion::Startup | MainRegion::LoadError => Self::Startup,
            _ if settings_page => Self::Settings,
            _ if model.files_surface_visible() => Self::Files,
            MainRegion::MissingRepository => Self::Missing,
            MainRegion::EmptyRepository if panel == WorkspacePanel::Search => Self::EmptySearch,
            MainRegion::EmptyRepository => Self::Empty,
            MainRegion::HasRepository if model.page == ShellPage::Playlists || panel == WorkspacePanel::Playlist => Self::Playlists,
            MainRegion::HasRepository => match panel {
                WorkspacePanel::Logs => Self::Logs,
                WorkspacePanel::Extensions => Self::Extensions,
                WorkspacePanel::Actions => Self::Actions,
                WorkspacePanel::Search => Self::Search,
                _ => Self::Blank,
            },
        }
    }
}

/// 常驻路由：分支只在进入时建一次，之后同步只写信号。
fn resident(route: RouteKey) -> bool {
    matches!(route, RouteKey::Startup | RouteKey::Missing | RouteKey::Empty)
}

/// 常驻路由的分支。非常驻路由返回 `None`。
fn resident_view(route: RouteKey, signals: RouteSignals, hot: HotSignals) -> Option<AnyView> {
    match route {
        RouteKey::Startup => Some(super::route_startup::view(signals.startup, hot)),
        RouteKey::Missing => Some(super::route_missing::view(signals.missing)),
        RouteKey::Empty => Some(super::route_empty::view(signals.empty)),
        _ => None,
    }
}

/// 整块重挂的路由内容。常驻路由返回 `None`。
fn legacy_view(route: RouteKey, model: &ShellViewModel) -> Option<AnyView> {
    let view = match route {
        RouteKey::Startup => return None,
        RouteKey::Settings => super::route_settings::view(model),
        RouteKey::Files => super::route_files::view(model),
        RouteKey::Search => super::route_search::view(model),
        RouteKey::EmptySearch => super::route_search::empty_library(model),
        RouteKey::Playlists => super::route_playlists::view(model),
        RouteKey::Logs | RouteKey::Extensions | RouteKey::Actions => super::route_admin::view(model),
        RouteKey::Missing | RouteKey::Empty => return None,
        RouteKey::Blank => super::route_home::blank(model),
    };
    Some(view)
}

/// 常驻路由的信号，在骨架作用域里建，进出路由都不重建。
#[derive(Clone, Copy)]
pub(crate) struct RouteSignals {
    startup: StartupSignals,
    missing: MissingSignals,
    empty: EmptySignals,
}

impl RouteSignals {
    fn new(model: &ShellViewModel) -> Self {
        Self { startup: StartupSignals::new(model), missing: MissingSignals::new(model), empty: EmptySignals::new(model) }
    }

    /// 只写当前路由的投影：别的常驻路由进来之前会先写一次再建分支。
    fn write(&self, route: RouteKey, model: &ShellViewModel) {
        if route == RouteKey::Startup {
            self.startup.write(StartupView::project(model));
        }
        if route == RouteKey::Missing {
            self.missing.write(model);
        }
        if route == RouteKey::Empty {
            self.empty.write(EmptyView::project(model));
        }
    }
}

/// `dynamic` 的键：路由加上旧视图分支的版本。常驻路由的版本固定是 0，换到旧视图路由时加一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteSlot {
    route: RouteKey,
    version: u64,
}

/// 主区块的常驻信号。
#[derive(Clone, Copy)]
pub(crate) struct PrimarySignals {
    slot: Signal<RouteSlot>,
    /// 主区外框节点，挂旧视图内容时据此找到路由容器。
    stage: NodeRef,
    routes: RouteSignals,
}

/// 路由容器在外框下的键。
const ROUTE_CONTAINER: &str = "primary-route";

/// 等着新分支挂进路由容器的旧视图内容。
struct Pending {
    version: u64,
    content: MountedView,
    /// 换下去的内容里记下的焦点和滚动，挂上后找回。
    kept: Option<KeptState>,
}

/// 旧视图内容的交接处：主区块同步时放进来，新分支的 `on_mount` 取走。
#[derive(Default)]
struct LegacySlot {
    /// 已经挂在路由容器里的内容。
    attached: Option<MountedView>,
    pending: Option<Pending>,
    /// `dynamic` 现在显示的路由，分支挂好时写。还没刷新的切换不算。
    shown: Option<RouteKey>,
}

thread_local! {
    /// 按主区块编号找交接处。分支的构建闭包要能跨线程移动，只带编号。
    static SLOTS: RefCell<HashMap<u64, Rc<RefCell<LegacySlot>>>> = RefCell::new(HashMap::new());
}

static NEXT_PART: AtomicU64 = AtomicU64::new(1);

/// 主区块。
pub(crate) struct PrimaryPart {
    id: u64,
    signals: PrimarySignals,
    slot: Rc<RefCell<LegacySlot>>,
    /// 外框的挂载；排法变了时整块重挂。
    root: Option<MountedView>,
    /// 最近写进键的路由和旧视图版本。
    route: RouteKey,
    version: u64,
    /// 当前分支内容建好时 ViewModel 的版本。
    revision: u64,
}

impl Drop for PrimaryPart {
    /// 线程结束时交接处可能先回收了，那时不必再摘。
    fn drop(&mut self) {
        let _ = SLOTS.try_with(|slots| slots.borrow_mut().remove(&self.id));
    }
}

impl ShellPart for PrimaryPart {
    const ID: PartId = PartId::Primary;
    type Signals = PrimarySignals;

    fn signals(model: &ShellViewModel) -> Self::Signals {
        PrimarySignals {
            slot: signal(RouteSlot { route: RouteKey::of(model), version: 0 }),
            stage: node_ref(),
            routes: RouteSignals::new(model),
        }
    }

    fn new(signals: Self::Signals) -> Self {
        let id = NEXT_PART.fetch_add(1, Ordering::Relaxed);
        let slot = Rc::new(RefCell::new(LegacySlot::default()));
        SLOTS.with(|slots| slots.borrow_mut().insert(id, slot.clone()));
        let route = signals.slot.get_untracked().route;
        Self { id, signals, slot, root: None, route, version: 0, revision: 0 }
    }

    fn root(&self) -> Option<StableNodeId> {
        first_root(self.root.as_ref())
    }

    /// 挂外框和当前路由的分支。分支在外框挂载时同步建好，旧视图内容随即挂进路由容器；
    /// 换下来的旧外框和要找回的状态交给调度方在新外框放进槽位以后处理。
    fn mount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, mode: BodyMode) -> Result<Swap, FrameworkError> {
        let kept = self.capture_attached(cx.document);
        let route = RouteKey::of(model);
        self.signals.routes.write(route, model);
        self.stage_branch(cx, model, route, None)?;
        let document_id = cx.document.document();
        let (signals, hot, id) = (self.signals, cx.hot, self.id);
        let root = cx.document.context_mut().mount_view_detached(document_id, move || frame(mode, signals, hot, id))?;
        let mut swap = Swap::replace(self.root.replace(root), None, None);
        if let (Some(kept), Some(roots)) = (kept, self.attached_roots()) {
            swap.merge(Swap::restore(kept, roots));
        }
        Ok(swap)
    }

    fn sync(&mut self, model: &ShellViewModel) {
        self.signals.routes.write(RouteKey::of(model), model);
    }

    fn needs_remount(&self, model: &ShellViewModel) -> bool {
        let route = RouteKey::of(model);
        route != self.route || (!resident(route) && model.revision != self.revision)
    }

    /// 换路由时写新的键，旧视图路由先把新内容挂好等着，真正换分支在下一次刷新；
    /// 留在同一个旧视图路由里时当场换掉路由容器里的内容。
    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let route = RouteKey::of(model);
        let in_place = {
            let slot = self.slot.borrow();
            !resident(route) && slot.shown == Some(route) && slot.pending.is_none()
        };
        if in_place && let Some(container) = self.container(cx.document) {
            return self.replace_content(cx, model, route, container);
        }
        let kept = self.capture_attached(cx.document);
        self.stage_branch(cx, model, route, kept)?;
        Ok(Swap::default())
    }

    fn composing(&self, document: &RuntimeDocument) -> bool {
        self.root.as_ref().is_some_and(|root| composing_under(document, root.roots()))
    }
}

impl PrimaryPart {
    /// 当前挂在路由容器里的旧视图内容的根。
    fn attached_roots(&self) -> Option<Vec<StableNodeId>> {
        self.slot.borrow().attached.as_ref().map(|view| view.roots().to_vec())
    }

    /// 记下路由容器里旧视图内容的焦点、选区和滚动。
    fn capture_attached(&self, document: &RuntimeDocument) -> Option<KeptState> {
        let roots = self.attached_roots()?;
        Some(remount_state::capture(document.context(), document.document(), &roots))
    }

    /// 外框下的路由容器。
    fn container(&self, document: &RuntimeDocument) -> Option<StableNodeId> {
        let stage = self.signals.stage.get_untracked()?;
        let container = document.context().assembled_child(stage, ROUTE_CONTAINER);
        if container.is_none() {
            eprintln!("Nana 主区外框下找不到路由容器，改走分支切换");
        }
        container
    }

    /// 同一个旧视图路由里内容变了：当场挂好新内容放进路由容器，旧内容和要找回的状态交给调度方收尾。
    fn replace_content(
        &mut self,
        cx: &mut PartCx<'_>,
        model: &ShellViewModel,
        route: RouteKey,
        container: StableNodeId,
    ) -> Result<Swap, FrameworkError> {
        let kept = self.capture_attached(cx.document);
        let Some(content) = mount_detached(cx.document, cx.hot, Self::ID, || legacy_view(route, model))? else {
            eprintln!("Nana 主区路由 {route:?} 没有建出内容");
            return Ok(Swap::default());
        };
        let mut mutations = MutationQueue::new();
        for root in content.roots() {
            mutations.insert(container, *root, None);
        }
        if let Err(error) = cx.document.context_mut().commit_mutations(mutations) {
            eprintln!("Nana 新内容放不进主区路由容器：{error}");
            if let Err(error) = content.unmount(cx.document.context_mut()) {
                eprintln!("Nana 卸掉放不进去的主区内容失败：{error}");
            }
            return Err(error);
        }
        let old = self.slot.borrow_mut().attached.replace(content);
        self.revision = model.revision;
        cx.stats.remounts += 1;
        let slot = self.slot.borrow();
        Ok(Swap::replace(old, kept, slot.attached.as_ref()))
    }

    /// 准备 `route` 的分支并写进键。旧视图路由把内容挂成脱离树的一块放进交接处，还没挂上就被
    /// 新内容顶替的那块直接卸掉。
    fn stage_branch(
        &mut self,
        cx: &mut PartCx<'_>,
        model: &ShellViewModel,
        route: RouteKey,
        kept: Option<KeptState>,
    ) -> Result<(), FrameworkError> {
        let content = if resident(route) {
            None
        } else {
            mount_detached(cx.document, cx.hot, Self::ID, || legacy_view(route, model))?
        };
        let stale = self.slot.borrow_mut().pending.take();
        if let Some(stale) = stale
            && let Err(error) = stale.content.unmount(cx.document.context_mut())
        {
            eprintln!("Nana 卸掉没来得及挂上的主区内容失败：{error}");
        }
        let version = match content {
            Some(content) => {
                self.version += 1;
                self.slot.borrow_mut().pending = Some(Pending { version: self.version, content, kept });
                self.version
            }
            None => 0,
        };
        self.signals.slot.set(RouteSlot { route, version });
        self.route = route;
        self.revision = model.revision;
        cx.stats.remounts += 1;
        Ok(())
    }
}

/// 主区外框。有侧栏时它自己放进工作区的主区；独占时外面再包一层壳层底色，圆角外露出 `--bg-elev`。
/// 外框里的路由容器占满外框，样式和 `Stack::fill_column(0)` 相同。
fn frame(mode: BodyMode, signals: PrimarySignals, hot: HotSignals, id: u64) -> AnyView {
    let routes = dynamic(signals.slot, move |slot: &RouteSlot| branch(*slot, signals, hot, id))
        .key(ROUTE_CONTAINER)
        .css(css! { height: 100%; min-height: 0; flex-grow: 1; flex-shrink: 1; });
    let stage = widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Lg),
    )
    .node_ref(signals.stage)
    .children((routes,))
    .key("workspace-body")
    .into_any();
    match mode {
        BodyMode::Workbench => stage,
        BodyMode::Solo => widget(Stack::fill_column(0.0).surface(SemanticColorRole::Surface)).children((stage,)).into_any(),
    }
}

/// 一个路由分支。常驻路由直接建视图；旧视图路由的分支没有节点，挂上后由 [`settle`] 把交接处的内容放进来。
fn branch(slot: RouteSlot, signals: PrimarySignals, hot: HotSignals, id: u64) -> AnyView {
    let stage = signals.stage;
    on_mount(move |cx| settle(cx, id, slot, stage));
    resident_view(slot.route, signals.routes, hot).unwrap_or_else(|| ().into_any())
}

/// 新分支放好以后：记下现在显示的路由；版本对上的待挂内容挂进路由容器，找回状态；
/// 卸掉上一块旧视图内容（换分支时它已被挪出容器）。常驻分支只卸掉上一块。
fn settle(cx: &mut AppContext, id: u64, shown: RouteSlot, stage: NodeRef) {
    let Some(slot) = SLOTS.with(|slots| slots.borrow().get(&id).cloned()) else {
        eprintln!("Nana 主区路由分支找不到所属的主区块：{id}");
        return;
    };
    let mut slot = slot.borrow_mut();
    slot.shown = Some(shown.route);
    let pending = slot.pending.take_if(|pending| pending.version == shown.version);
    if pending.is_none() && shown.version != 0 {
        eprintln!("Nana 主区路由分支 {:?} 第 {} 版没有等着挂上的内容", shown.route, shown.version);
    }
    let attached = pending.and_then(|pending| attach(cx, pending, stage));
    if let Some(old) = std::mem::replace(&mut slot.attached, attached)
        && let Err(error) = old.unmount(cx)
    {
        eprintln!("Nana 卸掉换下来的主区内容失败：{error}");
    }
}

/// 把待挂内容放进路由容器，找回记下的状态。放不进去时卸掉它，返回 `None`。
fn attach(cx: &mut AppContext, pending: Pending, stage: NodeRef) -> Option<MountedView> {
    let container = stage.get_untracked().and_then(|stage| cx.assembled_child(stage, ROUTE_CONTAINER));
    let Some(container) = container else {
        eprintln!("Nana 主区路由容器不在，旧视图内容挂不上");
        discard(cx, pending.content);
        return None;
    };
    let roots = pending.content.roots().to_vec();
    let mut mutations = MutationQueue::new();
    for root in &roots {
        mutations.insert(container, *root, None);
    }
    if let Err(error) = cx.commit_mutations(mutations) {
        eprintln!("Nana 旧视图内容放不进主区路由容器：{error}");
        discard(cx, pending.content);
        return None;
    }
    if let Some(kept) = pending.kept
        && let Some(document) = cx.world().node(container).map(|node| node.document)
    {
        kept.restore(cx, document, &roots);
    }
    Some(pending.content)
}

fn discard(cx: &mut AppContext, content: MountedView) {
    if let Err(error) = content.unmount(cx) {
        eprintln!("Nana 卸掉挂不上的主区内容失败：{error}");
    }
}

#[cfg(test)]
#[path = "view_part_primary_tests.rs"]
mod tests;
