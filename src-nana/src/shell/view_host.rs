//! 常驻的壳层视图 `ShellView`。
//!
//! 骨架只挂一次：AppShell、标题栏（字段绑定，搜索框受控）、工作区（资源区宽度跟侧栏呈现宽度）
//! 和一个认出这棵骨架的隐藏标记。现有视图函数整块建成三块内容——侧栏、主区、浮层——挂好后放进
//! 骨架组合控件的槽位：有侧栏时侧栏和主区放进工作区的资源区和主区，工作区是 AppShell 的 body；
//! 主区独占时它自己是 body；浮层是 AppShell 的 overlay。槽位根节点拿到的区域样式、布局补丁和整棵
//! 重挂时一样；组合控件只在自己投影时打补丁，所以槽位根节点上不放绑定。
//!
//! 数据流：`ShellMessage → reduce → 服务副作用 → ShellView::sync`。同步先写热信号和标题栏字段
//! （新挂的内容按信号当前值建），再按块重挂内容。焦点在某块里、输入法还有预编辑时，这块延后
//! 重挂，`prepare` 每帧经 [`ShellView::retry_deferred`] 检查，组合结束后补挂。动效帧和播放推进
//! 只走 [`ShellView::sync_hot`]。这里只写信号，不刷新绑定，刷新由输入路由和帧开头完成。

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use nana_ui::runtime::view::{detached, entity_ref, node_ref, widget, with_refs, AnyView};
use nana_ui::runtime::{
    AppContext, AppShell, DocumentId, Entity, FrameworkError, LengthSpec, MountedView, RuntimeDocument, StableNodeId, Text,
    UiWorld, Workspace, WorkspaceRegionSlot,
};
use nana_ui::RegionId;

use super::hot::{self, HotSignals, ModelField, MotionFrame, PlaybackView, TitleBarView, TitleSignals};
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

/// 一块按需重挂的内容。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Sidebar,
    Primary,
    Overlay,
}

const PARTS: [Part; 3] = [Part::Sidebar, Part::Primary, Part::Overlay];

/// 同步计数，测试用来确认动效帧不重挂、组合输入会延后。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewStats {
    /// 重挂的内容块数。
    pub remounts: u64,
    /// 因输入法组合而延后的次数。
    pub deferred: u64,
}

/// 标记节点的文字前缀；后面是进程内唯一的编号。
const MARKER_PREFIX: &str = "momobako-shell:";
static NEXT_MARKER: AtomicU64 = AtomicU64::new(1);

/// 一扇窗口文档里的常驻壳层视图。
pub(crate) struct ShellView {
    marker: StableNodeId,
    marker_text: String,
    shell: Entity<AppShell>,
    workspace: Entity<Workspace>,
    hot: HotSignals,
    title: TitleSignals,
    query: ModelField,
    mode: BodyMode,
    /// 最近写进工作区资源区的侧栏呈现宽度。
    sidebar_width: f32,
    sidebar: Option<MountedView>,
    primary: Option<MountedView>,
    overlay: Option<MountedView>,
    /// 等输入法组合结束才重挂的块。
    deferred: Vec<Part>,
    /// 最近一次整体同步时 ViewModel 的归约次数。
    revision: u64,
    stats: ViewStats,
}

impl ShellView {
    /// 挂骨架，再把三块内容按 `model` 挂进槽位。骨架里的信号都建在这次挂载的作用域里。
    pub(crate) fn mount(document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<Self, FrameworkError> {
        let document_id = document.document();
        let marker_text = format!("{MARKER_PREFIX}{}", NEXT_MARKER.fetch_add(1, Ordering::Relaxed));
        let motion = MotionFrame::project(model);
        let playback = PlaybackView::project(model);
        let title_view = TitleBarView::project(model);
        let mut made = None;
        let (_skeleton, (shell, workspace, marker)) = document.context_mut().mount_view_root(document_id, || {
            let hot = HotSignals::new(motion, &playback);
            let title = TitleSignals::new(&title_view);
            let query = ModelField::new(&title_view.query);
            let shell = entity_ref::<AppShell>();
            let workspace = entity_ref::<Workspace>();
            let marker = node_ref();
            let view = (
                detached(widget(hidden_marker(&marker_text)).node_ref(marker)),
                detached(widget(super::render::workbench_workspace(motion.sidebar_width)).entity_ref(workspace)),
                widget(AppShell::new())
                    .title_bar(super::title_bar::title_bar(title, query.signal(), &title_view.query))
                    .entity_ref(shell),
            );
            made = Some((hot, title, query));
            with_refs(view, (shell, workspace, marker))
        })?;
        let (hot, title, query) = made.expect("骨架挂载闭包已经运行");
        super::title_bar::bind_window_controls(document)?;
        register(document, &[shell.stable_id()]);
        let mut view = Self {
            marker,
            marker_text,
            shell,
            workspace,
            hot,
            title,
            query,
            mode: body_mode(model),
            sidebar_width: motion.sidebar_width,
            sidebar: None,
            primary: None,
            overlay: None,
            deferred: Vec::new(),
            revision: model.revision,
            stats: ViewStats::default(),
        };
        view.remount(document, model, view.mode, &PARTS)?;
        Ok(view)
    }

    /// 这个视图是不是挂在 `document` 里。按标记节点的文字认，换了文档（哪怕节点编号相同）也认得出。
    pub(crate) fn owns(&self, document: &RuntimeDocument) -> bool {
        document.context().world().text(self.marker) == Some(self.marker_text.as_str())
    }

    /// 骨架的信号还在：所在文档没有被丢掉。
    pub(crate) fn alive(&self) -> bool {
        self.hot.alive()
    }

    /// 测试用：同步计数。
    #[cfg(test)]
    pub(crate) fn stats(&self) -> ViewStats {
        self.stats
    }

    /// 测试用：浮层和主区内容现在的根节点。
    #[cfg(test)]
    pub(crate) fn content_roots(&self) -> (Option<StableNodeId>, Option<StableNodeId>) {
        let first = |view: &Option<MountedView>| view.as_ref().and_then(|view| view.roots().first().copied());
        (first(&self.overlay), first(&self.primary))
    }

    /// ViewModel 有没有骨架之外要重挂的变化：归约过，或者工作台排法换了。
    pub(crate) fn stale(&self, model: &ShellViewModel) -> bool {
        model.revision != self.revision || body_mode(model) != self.mode
    }

    /// 整体同步：写热信号和标题栏，再重挂三块内容。正在组合输入的块延后，排法变了则不再等。
    pub(crate) fn sync(&mut self, document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
        if !self.alive() {
            eprintln!("Nana 壳层骨架已随文档回收，跳过同步");
            return Err(FrameworkError::MissingView(self.shell.stable_id()));
        }
        self.write_signals(document, model)?;
        let mode = body_mode(model);
        let mut parts = Vec::new();
        for part in PARTS {
            if mode == self.mode && self.composing(document, part) {
                if !self.deferred.contains(&part) {
                    self.deferred.push(part);
                }
                self.stats.deferred += 1;
                continue;
            }
            parts.push(part);
        }
        if mode != self.mode && !self.deferred.is_empty() {
            eprintln!("Nana 工作台排法变了，正在组合输入的内容也一起重挂：{:?}", self.deferred);
        }
        self.remount(document, model, mode, &parts)?;
        self.revision = model.revision;
        Ok(())
    }

    /// 只同步热信号和侧栏宽度：动效帧和播放推进走这里，不重挂内容。排法要换时由调用方整体同步。
    pub(crate) fn sync_hot(&mut self, document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
        if !self.alive() {
            eprintln!("Nana 壳层骨架已随文档回收，跳过热同步");
            return Err(FrameworkError::MissingView(self.shell.stable_id()));
        }
        self.write_signals(document, model)
    }

    /// 因输入法组合延后的块，组合已经结束的按当前 ViewModel 补挂。
    pub(crate) fn retry_deferred(&mut self, document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
        let ready = self.deferred.iter().copied().filter(|part| !self.composing(document, *part)).collect::<Vec<_>>();
        if ready.is_empty() || !self.alive() {
            return Ok(());
        }
        self.remount(document, model, self.mode, &ready)
    }

    /// 写热投影和标题栏投影；搜索框草稿只在 ViewModel 的值变了时写。侧栏呈现宽度变了时写进工作区。
    fn write_signals(&mut self, document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
        let motion = MotionFrame::project(model);
        let title = TitleBarView::project(model);
        self.hot.write(motion, &PlaybackView::project(model));
        self.title.write(&title);
        self.query.sync(&title.query);
        self.sync_sidebar_width(document.context_mut(), motion.sidebar_width)
    }

    /// 资源区宽度跟着侧栏呈现宽度，只在投影值变了时写，拖动中的区域尺寸不会被旧值拉回。
    /// 工作区重投影会冲掉 AppShell 给 body 打的布局补丁，工作区是 body 时写完让 AppShell 重新投影补回。
    fn sync_sidebar_width(&mut self, context: &mut AppContext, width: f32) -> Result<(), FrameworkError> {
        if self.sidebar_width == width {
            return Ok(());
        }
        self.sidebar_width = width;
        let changed = context.update_component(self.workspace, |workspace, _| hot::set_resources_width(workspace, width))?;
        if changed && self.mode == BodyMode::Workbench {
            context.reproject_component(self.shell)?;
        }
        Ok(())
    }

    /// 焦点在这块内容里，而且输入法正在组合（预编辑非空）。
    fn composing(&self, document: &RuntimeDocument, part: Part) -> bool {
        let world = document.context().world();
        let Some(focused) = world.focused(document.document()) else {
            return false;
        };
        self.roots(part).iter().any(|root| world.is_descendant_or_self(focused, *root))
            && world.ime(focused).is_some_and(|ime| !ime.text.is_empty())
    }

    fn roots(&self, part: Part) -> Vec<StableNodeId> {
        self.slot(part).as_ref().map(|view| view.roots().to_vec()).unwrap_or_default()
    }

    fn slot(&self, part: Part) -> &Option<MountedView> {
        match part {
            Part::Sidebar => &self.sidebar,
            Part::Primary => &self.primary,
            Part::Overlay => &self.overlay,
        }
    }

    fn slot_mut(&mut self, part: Part) -> &mut Option<MountedView> {
        match part {
            Part::Sidebar => &mut self.sidebar,
            Part::Primary => &mut self.primary,
            Part::Overlay => &mut self.overlay,
        }
    }

    /// 按 `mode` 重挂 `parts`：先记下旧内容里的焦点、选区和滚动，建新内容，放进槽位，卸掉旧的，
    /// 给新内容补登 Escape、拖放和字段名，最后在新内容里找回记下的状态。
    ///
    /// 建任何一块失败时卸掉已经建好的新块，旧内容原样留着。
    fn remount(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
        mode: BodyMode,
        parts: &[Part],
    ) -> Result<(), FrameworkError> {
        let kept = parts
            .iter()
            .map(|part| (*part, super::remount_state::capture(document, &self.roots(*part))))
            .collect::<Vec<_>>();
        let mut fresh: Vec<(Part, Option<MountedView>)> = Vec::new();
        for part in parts {
            match self.build(document, model, mode, *part) {
                Ok(view) => fresh.push((*part, view)),
                Err(error) => {
                    eprintln!("Nana 壳层内容挂载失败 {part:?}：{error}");
                    discard(document.context_mut(), fresh);
                    return Err(error);
                }
            }
        }
        let first_root = |view: &Option<MountedView>| view.as_ref().and_then(|view| view.roots().first().copied());
        let root_of = |part: Part| match fresh.iter().find(|(fresh_part, _)| *fresh_part == part) {
            Some((_, view)) => first_root(view),
            None => first_root(self.slot(part)),
        };
        let (sidebar, primary, overlay) = (root_of(Part::Sidebar), root_of(Part::Primary), root_of(Part::Overlay));
        if let Err(error) = self.place(document.context_mut(), mode, sidebar, primary, overlay) {
            eprintln!("Nana 壳层内容放进槽位失败，放回原来的内容：{error}");
            let restored = self.place(
                document.context_mut(),
                self.mode,
                first_root(&self.sidebar),
                first_root(&self.primary),
                first_root(&self.overlay),
            );
            if let Err(error) = restored {
                eprintln!("Nana 原来的壳层内容也放不回槽位：{error}");
            }
            discard(document.context_mut(), fresh);
            return Err(error);
        }
        self.mode = mode;
        let mut placed_roots = Vec::new();
        for (part, view) in fresh {
            if let Some(view) = &view {
                placed_roots.push((part, view.roots().to_vec()));
            }
            if let Some(old) = std::mem::replace(self.slot_mut(part), view)
                && let Err(error) = old.unmount(document.context_mut())
            {
                eprintln!("Nana 卸掉旧的壳层内容失败 {part:?}：{error}");
            }
            self.deferred.retain(|deferred| *deferred != part);
            self.stats.remounts += 1;
        }
        for (_, roots) in &placed_roots {
            register(document, roots);
        }
        for (part, state) in kept {
            if let Some((_, roots)) = placed_roots.iter().find(|(placed, _)| *placed == part) {
                state.restore(document, roots);
            }
        }
        Ok(())
    }

    /// 建一块内容，挂成脱离树的根，建的时候把热信号交给视图函数。没有这块（侧栏收起、没有浮层）时返回 `None`。
    fn build(
        &self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
        mode: BodyMode,
        part: Part,
    ) -> Result<Option<MountedView>, FrameworkError> {
        let document_id = document.document();
        let hot = self.hot;
        let view = move || -> Option<AnyView> {
            hot::with_signals(hot, || match part {
                Part::Sidebar => (mode == BodyMode::Workbench).then(|| super::render::sidebar_view(model)),
                Part::Primary => Some(super::render::primary_view(model, mode)),
                Part::Overlay => super::render::overlay_view(model),
            })
        };
        let context = document.context_mut();
        let mounted = context.mount_view_detached(document_id, view)?;
        match mounted.roots().len() {
            0 => {
                mounted.unmount(context)?;
                Ok(None)
            }
            1 => Ok(Some(mounted)),
            count => {
                eprintln!("Nana 壳层内容 {part:?} 应该只有一个根，实际 {count} 个，只放第一个");
                Ok(Some(mounted))
            }
        }
    }

    /// 把三块内容的根放进槽位。有侧栏时工作区是 body；主区独占时清空工作区的区域，主区自己是 body。
    fn place(
        &self,
        context: &mut AppContext,
        mode: BodyMode,
        sidebar: Option<StableNodeId>,
        primary: Option<StableNodeId>,
        overlay: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        match mode {
            BodyMode::Workbench => {
                let regions = place_regions(context, self.workspace, sidebar, primary)?;
                let shell = place_shell(context, self.shell, Some(self.workspace.stable_id()), overlay)?;
                // 换区域内容时工作区重投影，冲掉了 AppShell 给 body 打的布局补丁；AppShell 自己
                // 重新装配时会补上，没装配就让它重新投影一次。
                if regions && !shell {
                    context.reproject_component(self.shell)?;
                }
                Ok(())
            }
            BodyMode::Solo => {
                place_shell(context, self.shell, primary, overlay)?;
                place_regions(context, self.workspace, None, None).map(|_| ())
            }
        }
    }
}

/// 卸掉建好了却没放上去的新内容。
fn discard(context: &mut AppContext, fresh: Vec<(Part, Option<MountedView>)>) {
    for (part, view) in fresh {
        if let Some(view) = view
            && let Err(error) = view.unmount(context)
        {
            eprintln!("Nana 卸掉没放上去的壳层内容失败 {part:?}：{error}");
        }
    }
}

/// 工作区资源区和主区的内容。没变时不动，返回 `false`；变了以后重新装配，资源区要有拖动柄。
fn place_regions(
    context: &mut AppContext,
    workspace: Entity<Workspace>,
    sidebar: Option<StableNodeId>,
    primary: Option<StableNodeId>,
) -> Result<bool, FrameworkError> {
    let slots = [(RegionId::Resources, sidebar), (RegionId::Primary, primary)]
        .into_iter()
        .filter_map(|(region, content)| Some(WorkspaceRegionSlot::new(region, content?)))
        .collect::<Vec<_>>();
    if context.read(workspace, |current| current.slots == slots)? {
        return Ok(false);
    }
    context.update_component(workspace, |current, _| current.slots = slots)?;
    context.assemble_workspace(workspace)?;
    Ok(true)
}

/// AppShell 的 body 和 overlay。没变时不动，返回 `false`；变了以后重新装配（装配最后会重新投影），
/// 换下来的节点由装配停放。
fn place_shell(
    context: &mut AppContext,
    shell: Entity<AppShell>,
    body: Option<StableNodeId>,
    overlay: Option<StableNodeId>,
) -> Result<bool, FrameworkError> {
    if context.read(shell, |current| current.body == body && current.overlay == overlay)? {
        return Ok(false);
    }
    context.update_component(shell, |current, _| {
        current.body = body;
        current.overlay = overlay;
    })?;
    context.assemble_app_shell(shell)?;
    Ok(true)
}

/// 认出骨架的隐藏标记：脱离树停放，不参与布局、绘制、命中和无障碍。
fn hidden_marker(text: &str) -> Text {
    let mut marker = Text::new(text.to_owned());
    let layout = std::sync::Arc::make_mut(&mut marker.style.layout);
    layout.hidden = true;
    layout.width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(0.0));
    marker
}

/// 新挂进来的节点补登命令式的处理：Escape、系统文件拖放和对话框下拉框的字段名。只扫这些根下面。
fn register(document: &mut RuntimeDocument, roots: &[StableNodeId]) {
    crate::window_host::bind_escape(document, roots);
    crate::window_host::bind_file_drop(document, roots);
    super::sidebar_view::bind_field_labels(document, roots);
}

/// 文档里组件类型是 `component`、落在 `roots`（含自身）下面的节点。按组件索引取候选再往上查祖先，
/// 不遍历整棵子树。
pub(crate) fn components_under(
    world: &UiWorld,
    document: DocumentId,
    component: &str,
    roots: &[StableNodeId],
) -> Vec<StableNodeId> {
    world
        .nodes_of_component(document, component)
        .filter(|id| roots.iter().any(|root| world.is_descendant_or_self(*id, *root)))
        .collect()
}

thread_local! {
    /// `mount_shell` 挂过的视图。测试会在同一线程上建好几份文档，按标记认是哪一份。
    static REGISTERED: RefCell<Vec<ShellView>> = const { RefCell::new(Vec::new()) };
}

/// 在给定 Runtime 文档里挂载或同步 MomoBako 壳层。
///
/// 文档里已经有这里挂过的壳层就整体同步，否则新挂一棵。生产窗口自己持有 `ShellView`；
/// 这个入口给验收文档和测试用，文档丢掉后对应的视图在下次调用时清掉。
pub fn mount_shell(document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
    REGISTERED.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(ShellView::alive);
        if let Some(view) = views.iter_mut().find(|view| view.owns(document)) {
            return view.sync(document, model);
        }
        let view = ShellView::mount(document, model)?;
        views.push(view);
        Ok(())
    })
}

/// 测试用：照生产 `prepare` 跑一帧准备，视图用 `mount_shell` 挂在这份文档里的那个。
#[cfg(test)]
pub(crate) fn prepare_registered(model: &mut ShellViewModel, window: &mut nana_ui::ApplicationWindow) {
    REGISTERED.with(|views| {
        let mut views = views.borrow_mut();
        let view = views.iter_mut().find(|view| view.alive() && view.owns(&window.document));
        crate::window_host::prepare_motion(model, view, window);
    })
}

#[cfg(test)]
#[path = "view_host_tests.rs"]
mod tests;
