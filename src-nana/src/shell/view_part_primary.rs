//! 主区块：主区外框常驻，里面用 `dynamic` 按 [`RouteKey`] 切换路由分支。
//!
//! 外框是白底 `Lg` 圆角的一层（主区独占时外面再包一层壳层底色），放进工作区的主区或 AppShell 的
//! body。外框里只有一个结构块 `dynamic(RouteSlot)`：
//! - 常驻路由（启动、文件、缺失、空库、搜索、设置、日志、拓展、动作和播放集页）的分支只在进入路由时建一次，
//!   之后同步只写它的信号；
//! - 其余路由仍由旧视图函数整块建出。换到这种路由时，同步在本线程把新内容挂成脱离树的一块放进
//!   交接处，把路由和版本号写进键；下一次刷新 `dynamic` 换出新分支，分支的 `on_mount` 把这块内容
//!   挂进路由容器、卸掉上一块，再找回焦点、选区和滚动。留在同一个旧视图路由里、内容变了时不经过
//!   `dynamic`，同步当场换掉路由容器里的内容，和改动前一样不留时间差，紧接着的按键落在新节点上。
//!
//! 内容变了只重挂当前分支，外框、侧栏和浮层都不动。每个路由的入口函数在自己的 `route_*.rs` 里；
//! 把某个路由改成常驻时，在 [`resident`]、[`resident_view`] 和 [`legacy_view`] 里改它自己那一行，
//! 状态放进 [`RouteSignals`]。
//!
//! 常驻路由里还嵌着别的模块的旧视图（筛选栏、播放条、对话框）时，分支里给它们留占位节点，登记成
//! [`Island`]：进路由时随分支一起挂好，之后岛的版本变了才当场换掉占位节点里的内容，常驻的部分不动。

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
use super::inspect_search_view::{FilterBarSignals, FilterBarView, SearchPanelSignals, SearchPanelView};
use super::remount_state::{self, KeptState};
use super::route_admin::AdminSignals;
use super::route_empty::{EmptySignals, EmptyView};
use super::route_missing::MissingSignals;
use super::route_files::FilesRouteSignals;
use super::route_playlists::PlaylistRouteSignals;
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
}

impl RouteKey {
    /// 按 ViewModel 取当前路由。有仓库时按面板分：文件、回收站和智能文件夹是文件路由，停在播放集页时
    /// 其余面板都显示播放集路由。
    pub(crate) fn of(model: &ShellViewModel) -> Self {
        let settings_page = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
        let panel = model.workspace.panel;
        match model.workspace.main_region() {
            MainRegion::Startup | MainRegion::LoadError => Self::Startup,
            _ if settings_page => Self::Settings,
            MainRegion::MissingRepository => Self::Missing,
            MainRegion::EmptyRepository if panel == WorkspacePanel::Search => Self::EmptySearch,
            MainRegion::EmptyRepository => Self::Empty,
            MainRegion::HasRepository => match panel {
                WorkspacePanel::Files | WorkspacePanel::Trash | WorkspacePanel::SmartFolder => Self::Files,
                _ if model.page == ShellPage::Playlists => Self::Playlists,
                WorkspacePanel::Playlist => Self::Playlists,
                WorkspacePanel::Logs => Self::Logs,
                WorkspacePanel::Extensions => Self::Extensions,
                WorkspacePanel::Actions => Self::Actions,
                WorkspacePanel::Search => Self::Search,
            },
        }
    }
}

/// 常驻路由：分支只在进入时建一次，之后同步只写信号。
fn resident(route: RouteKey) -> bool {
    matches!(
        route,
        RouteKey::Startup
            | RouteKey::Files
            | RouteKey::Missing
            | RouteKey::Empty
            | RouteKey::Search
            | RouteKey::EmptySearch
            | RouteKey::Settings
            | RouteKey::Logs
            | RouteKey::Extensions
            | RouteKey::Actions
            | RouteKey::Playlists
    )
}

/// 常驻路由的分支。非常驻路由返回 `None`。
fn resident_view(route: RouteKey, signals: RouteSignals, hot: HotSignals) -> Option<AnyView> {
    match route {
        RouteKey::Startup => Some(super::route_startup::view(signals.startup, hot)),
        RouteKey::Missing => Some(super::route_missing::view(signals.missing)),
        RouteKey::Empty => Some(super::route_empty::view(signals.empty)),
        RouteKey::Search => Some(super::route_search::view(signals.filter, signals.search)),
        RouteKey::EmptySearch => Some(super::route_search::empty_library(signals.search)),
        RouteKey::Settings => Some(super::route_settings::view(signals.admin)),
        RouteKey::Logs => Some(super::route_admin::logs(signals.filter, signals.admin)),
        RouteKey::Extensions => Some(super::route_admin::extensions(signals.filter, signals.admin)),
        RouteKey::Actions => Some(super::route_admin::actions(signals.filter, signals.admin)),
        RouteKey::Files => Some(super::route_files::view(signals.filter, signals.files, hot)),
        RouteKey::Playlists => Some(super::route_playlists::view(signals.filter, signals.playlists)),
    }
}

/// 整块重挂的路由内容。所有路由都已常驻，不再有旧视图路由；交接处随后拆掉。
fn legacy_view(_: RouteKey, _: &ShellViewModel) -> Option<AnyView> {
    None
}

/// 常驻路由的信号，在骨架作用域里建，进出路由都不重建。
#[derive(Clone, Copy)]
pub(crate) struct RouteSignals {
    startup: StartupSignals,
    missing: MissingSignals,
    empty: EmptySignals,
    /// 首页筛选栏：启动页和设置页以外每次同步都写，常驻首页路由用 `resident_filter_bar` 嵌进外框。
    filter: FilterBarSignals,
    /// 搜索面板，有仓库和没有资源库的两条搜索路由共用。
    search: SearchPanelSignals,
    /// 设置、日志、拓展和动作页。
    admin: AdminSignals,
    files: FilesRouteSignals,
    playlists: PlaylistRouteSignals,
}

impl RouteSignals {
    fn new(model: &ShellViewModel) -> Self {
        Self {
            startup: StartupSignals::new(model),
            missing: MissingSignals::new(model),
            empty: EmptySignals::new(model),
            filter: FilterBarSignals::new(),
            search: SearchPanelSignals::new(),
            admin: AdminSignals::new(),
            files: FilesRouteSignals::new(model),
            playlists: PlaylistRouteSignals::new(model),
        }
    }

    /// 只写当前路由的投影：别的常驻路由进来之前会先写一次再建分支。筛选栏属于首页外框，
    /// 首页各路由都写。
    fn write(&self, route: RouteKey, model: &ShellViewModel) {
        match route {
            RouteKey::Startup => self.startup.write(StartupView::project(model)),
            RouteKey::Missing => self.missing.write(model),
            RouteKey::Empty => self.empty.write(EmptyView::project(model)),
            RouteKey::Search | RouteKey::EmptySearch => self.search.write(SearchPanelView::project(model)),
            RouteKey::Settings => self.admin.write_settings(model),
            RouteKey::Logs => self.admin.write_logs(model),
            RouteKey::Extensions => self.admin.write_extensions(model),
            RouteKey::Actions => self.admin.write_actions(model),
            RouteKey::Files => self.files.write(model),
            RouteKey::Playlists => self.playlists.write(model),
        }
        // 筛选栏只在有仓库的首页上出现：启动、设置、缺失和空库页都不写。
        if !matches!(route, RouteKey::Startup | RouteKey::Settings | RouteKey::Missing | RouteKey::Empty) {
            self.filter.write(FilterBarView::project(model));
        }
    }
}

/// 常驻路由里嵌着的一块旧视图（岛）：分支里有个占位节点，里面的内容仍由别的模块的旧视图函数
/// 整块建出。内容在主区块同步时挂成脱离树的一块放进占位节点，`stamp` 变了才换。
#[derive(Clone, Copy)]
pub(crate) struct Island {
    /// 占位节点，分支建好以后才有值。
    pub slot: NodeRef,
    /// 岛里的内容；没有内容时为 `None`。
    pub build: fn(&ShellViewModel) -> Option<AnyView>,
    /// 内容的版本：和上次建内容时不同才重建。
    pub stamp: fn(&ShellViewModel) -> u64,
}

/// 常驻路由的岛，顺序就是分支里占位节点的先后。
fn islands(route: RouteKey, signals: &RouteSignals) -> Vec<Island> {
    match route {
        RouteKey::Files => super::route_files::islands(signals.files),
        RouteKey::Playlists => super::route_playlists::islands(signals.playlists),
        _ => Vec::new(),
    }
}

/// 岛里现在的内容。
struct IslandView {
    island: Island,
    /// 内容建好时的版本。
    stamp: u64,
    content: Option<MountedView>,
    /// 已经放进占位节点；还没放的等分支挂好时放。
    placed: bool,
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
    /// 常驻路由的岛。进路由时先挂好内容，分支挂好时放进占位节点。
    islands: Vec<IslandView>,
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
        let kept_islands = if route == self.route { self.capture_islands(cx.document) } else { Vec::new() };
        self.signals.routes.write(route, model);
        self.stage_branch(cx, model, route, None)?;
        let document_id = cx.document.document();
        let (signals, hot, id) = (self.signals, cx.hot, self.id);
        let root = cx.document.context_mut().mount_view_detached(document_id, move || frame(mode, signals, hot, id))?;
        let mut swap = Swap::replace(self.root.replace(root), None, None);
        if let (Some(kept), Some(roots)) = (kept, self.attached_roots()) {
            swap.merge(Swap::restore(kept, roots));
        }
        for (index, kept) in kept_islands {
            if let Some(roots) = self.island_roots(index) {
                swap.merge(Swap::restore(kept, roots));
            }
        }
        Ok(swap)
    }

    fn sync(&mut self, model: &ShellViewModel) {
        self.signals.routes.write(RouteKey::of(model), model);
    }

    /// 换了路由要重挂；旧视图路由看 ViewModel 版本，常驻路由只看岛的版本。
    fn needs_remount(&self, model: &ShellViewModel) -> bool {
        let route = RouteKey::of(model);
        if route != self.route {
            return true;
        }
        if resident(route) { self.islands_stale(model) } else { model.revision != self.revision }
    }

    /// 换路由时写新的键，旧视图路由先把新内容挂好等着，真正换分支在下一次刷新；
    /// 留在同一个旧视图路由里时当场换掉路由容器里的内容。常驻分支还在时只换岛。
    fn remount(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let route = RouteKey::of(model);
        if resident(route) && self.slot.borrow().shown == Some(route) {
            // 同一路由里岛的版本变了，或者换走又在刷新前换了回来：撤掉等着挂上的旧视图内容、把键写回来。
            self.revoke_pending(cx);
            if self.route != route {
                self.signals.slot.set(RouteSlot { route, version: 0 });
                self.route = route;
            }
            self.revision = model.revision;
            return self.refresh_islands(cx, model);
        }
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
        self.revoke_pending(cx);
        self.stage_islands(cx, model, route)?;
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

    /// 卸掉还没来得及挂上就被顶替的旧视图内容。
    fn revoke_pending(&mut self, cx: &mut PartCx<'_>) {
        let stale = self.slot.borrow_mut().pending.take();
        if let Some(stale) = stale
            && let Err(error) = stale.content.unmount(cx.document.context_mut())
        {
            eprintln!("Nana 卸掉没来得及挂上的主区内容失败：{error}");
        }
    }

    /// 换到 `route` 时准备它的岛：卸掉上一批，按当前 ViewModel 挂好新内容，等分支挂好时放进占位节点。
    fn stage_islands(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, route: RouteKey) -> Result<(), FrameworkError> {
        let old = std::mem::take(&mut self.slot.borrow_mut().islands);
        for view in old {
            if let Some(content) = view.content {
                discard(cx.document.context_mut(), content);
            }
        }
        for island in islands(route, &self.signals.routes) {
            let content = mount_detached(cx.document, cx.hot, Self::ID, || (island.build)(model))?;
            let stamp = (island.stamp)(model);
            self.slot.borrow_mut().islands.push(IslandView { island, stamp, content, placed: false });
        }
        Ok(())
    }

    /// 有岛的版本和 ViewModel 对不上。
    fn islands_stale(&self, model: &ShellViewModel) -> bool {
        let slot = self.slot.borrow();
        let wanted = islands(self.route, &self.signals.routes).len();
        slot.islands.len() != wanted || slot.islands.iter().any(|view| view.stamp != (view.island.stamp)(model))
    }

    /// 分支还在时换岛：版本变了（或者还没放进去）的岛按当前 ViewModel 重建，当场放进占位节点；
    /// 旧内容和要找回的焦点、滚动交给调度方收尾。离开又回到路由时岛已经卸掉，这里按登记重建。
    fn refresh_islands(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel) -> Result<Swap, FrameworkError> {
        let wanted = islands(self.route, &self.signals.routes);
        if self.slot.borrow().islands.len() != wanted.len() {
            let old = std::mem::take(&mut self.slot.borrow_mut().islands);
            for content in old.into_iter().filter_map(|view| view.content) {
                discard(cx.document.context_mut(), content);
            }
            let fresh = wanted.into_iter().map(|island| IslandView { island, stamp: 0, content: None, placed: false });
            self.slot.borrow_mut().islands.extend(fresh);
        }
        let mut swap = Swap::default();
        let count = self.slot.borrow().islands.len();
        for index in 0..count {
            let (island, stale) = {
                let slot = self.slot.borrow();
                let view = &slot.islands[index];
                (view.island, !view.placed || view.stamp != (view.island.stamp)(model))
            };
            if stale {
                swap.merge(self.refresh_island(cx, model, index, island)?);
            }
        }
        Ok(swap)
    }

    /// 重建第 `index` 个岛，放进占位节点。占位节点不在时只记日志，下次同步再放。
    fn refresh_island(&mut self, cx: &mut PartCx<'_>, model: &ShellViewModel, index: usize, island: Island) -> Result<Swap, FrameworkError> {
        let Some(target) = island.slot.get_untracked().filter(|id| cx.document.context().world().contains(*id)) else {
            eprintln!("Nana 主区岛的占位节点不在，岛 {index} 下次同步再放");
            return Ok(Swap::default());
        };
        let document_id = cx.document.document();
        let kept = self.island_roots(index).map(|roots| remount_state::capture(cx.document.context(), document_id, &roots));
        let fresh = mount_detached(cx.document, cx.hot, Self::ID, || (island.build)(model))?;
        if let Some(fresh) = &fresh
            && !place(cx.document.context_mut(), target, fresh)
        {
            discard(cx.document.context_mut(), fresh.clone());
            return Ok(Swap::default());
        }
        let mut slot = self.slot.borrow_mut();
        let view = &mut slot.islands[index];
        view.stamp = (island.stamp)(model);
        view.placed = true;
        let old = std::mem::replace(&mut view.content, fresh);
        if old.is_some() || view.content.is_some() {
            cx.stats.remounts += 1;
        }
        Ok(Swap::replace(old, kept, view.content.as_ref()))
    }

    /// 第 `index` 个岛现在内容的根。
    fn island_roots(&self, index: usize) -> Option<Vec<StableNodeId>> {
        self.slot.borrow().islands.get(index).and_then(|view| view.content.as_ref()).map(|content| content.roots().to_vec())
    }

    /// 记下各岛里的焦点、选区和滚动，按岛的序号。
    fn capture_islands(&self, document: &RuntimeDocument) -> Vec<(usize, KeptState)> {
        let count = self.slot.borrow().islands.len();
        (0..count)
            .filter_map(|index| {
                let roots = self.island_roots(index)?;
                Some((index, remount_state::capture(document.context(), document.document(), &roots)))
            })
            .collect()
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
/// 卸掉上一块旧视图内容（换分支时它已被挪出容器）。常驻分支卸掉上一块，再把岛放进占位节点。
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
    if resident(shown.route) {
        place_islands(cx, &mut slot.islands);
    }
}

/// 常驻分支挂好后把还没放的岛放进各自的占位节点。占位节点不在时记日志，下次同步再放。
fn place_islands(cx: &mut AppContext, islands: &mut [IslandView]) {
    for (index, view) in islands.iter_mut().enumerate().filter(|(_, view)| !view.placed) {
        let Some(target) = view.island.slot.get_untracked().filter(|id| cx.world().contains(*id)) else {
            eprintln!("Nana 常驻分支里没有岛 {index} 的占位节点");
            continue;
        };
        view.placed = match &view.content {
            Some(content) => place(cx, target, content),
            None => true,
        };
    }
}

/// 把一块内容的根放进 `target`，排在已有子节点后面。放不进去时记日志，返回 `false`。
fn place(cx: &mut AppContext, target: StableNodeId, content: &MountedView) -> bool {
    let mut mutations = MutationQueue::new();
    for root in content.roots() {
        mutations.insert(target, *root, None);
    }
    match cx.commit_mutations(mutations) {
        Ok(_) => true,
        Err(error) => {
            eprintln!("Nana 岛的内容放不进占位节点：{error}");
            false
        }
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
