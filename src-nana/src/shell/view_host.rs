//! 常驻的壳层视图 `ShellView`：只做调度。
//!
//! 骨架只挂一次：AppShell、标题栏（字段绑定，搜索框受控）、工作区（资源区宽度跟侧栏呈现宽度）
//! 和一个认出这棵骨架的隐藏标记。内容分三块，各在自己的模块里：侧栏（`view_part_sidebar.rs`）、
//! 主区（`view_part_primary.rs`，按路由键切换分支）和浮层（`view_part_overlay.rs`，按浮层键切换）。
//! 三块实现同一个接口 [`ShellPart`]：挂载、同步、是否需要重挂、组合延后。这里按同一套流程调它们，
//! 再把各块的根放进骨架组合控件的槽位：有侧栏时侧栏和主区放进工作区的资源区和主区，工作区是
//! AppShell 的 body；主区独占时它自己是 body；浮层是 AppShell 的 overlay。组合控件只在自己投影时
//! 给槽位根节点打补丁，所以槽位根节点上不放绑定。
//!
//! 数据流：`ShellMessage → reduce → 服务副作用 → ShellView::sync`。同步先写热信号和标题栏字段，
//! 再让每一块写自己的信号、按需重挂。焦点在某块里、输入法还有预编辑时，这块的重挂延后，`prepare`
//! 每帧经 [`ShellView::retry_deferred`] 检查，组合结束后补挂。动效帧和播放推进只走
//! [`ShellView::sync_hot`]。这里只写信号，不刷新绑定，刷新由输入路由和帧开头完成。

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use nana_ui::runtime::view::{detached, entity_ref, node_ref, widget, with_refs};
use nana_ui::runtime::{
    AppContext, AppShell, Entity, FrameworkError, LengthSpec, RuntimeDocument, StableNodeId, Text, Workspace,
    WorkspaceRegionSlot,
};
use nana_ui::RegionId;

use super::hot::{self, HotSignals, ModelField, MotionFrame, PlaybackView, TitleBarView, TitleSignals};
use super::view_part::{body_mode, BodyMode, PartCx, PartId, ShellPart, Swap};
use super::view_part_overlay::OverlayPart;
use super::view_part_primary::PrimaryPart;
use super::view_part_sidebar::SidebarPart;
use super::ShellViewModel;

/// 同步计数，测试用来确认动效帧不重挂、组合输入会延后。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewStats {
    /// 重挂的内容块（或主区分支）数。
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
    sidebar: SidebarPart,
    primary: PrimaryPart,
    overlay: OverlayPart,
    /// 等输入法组合结束才重挂的块。
    deferred: Vec<PartId>,
    /// 最近一次整体同步时 ViewModel 的版本。
    revision: u64,
    stats: ViewStats,
}

impl ShellView {
    /// 挂骨架，再把三块内容按 `model` 挂进槽位。骨架和各块的常驻信号都建在这次挂载的作用域里。
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
            let parts = (SidebarPart::signals(model), PrimaryPart::signals(model), OverlayPart::signals(model));
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
            made = Some((hot, title, query, parts));
            with_refs(view, (shell, workspace, marker))
        })?;
        let (hot, title, query, (sidebar, primary, overlay)) = made.expect("骨架挂载闭包已经运行");
        super::title_bar::bind_window_controls(document)?;
        let mode = body_mode(model);
        let mut view = Self {
            marker,
            marker_text,
            shell,
            workspace,
            hot,
            title,
            query,
            mode,
            sidebar_width: motion.sidebar_width,
            sidebar: SidebarPart::new(sidebar),
            primary: PrimaryPart::new(primary),
            overlay: OverlayPart::new(overlay),
            deferred: Vec::new(),
            revision: model.revision,
            stats: ViewStats::default(),
        };
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx { document: &mut *document, hot, stats: &mut view.stats };
            absorb(&mut swap, &mut error, PartId::Sidebar, view.sidebar.mount(&mut cx, model, mode));
            absorb(&mut swap, &mut error, PartId::Primary, view.primary.mount(&mut cx, model, mode));
            absorb(&mut swap, &mut error, PartId::Overlay, view.overlay.mount(&mut cx, model, mode));
        }
        view.place(document.context_mut(), mode)?;
        swap.finish(document);
        error.map_or(Ok(view), Err)
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
        (self.overlay.root(), self.primary.root())
    }

    /// 测试用：侧栏内容现在的根节点。
    #[cfg(test)]
    pub(crate) fn sidebar_root(&self) -> Option<StableNodeId> {
        self.sidebar.root()
    }

    /// ViewModel 有没有骨架之外要同步的变化：版本变了，或者工作台排法换了。
    pub(crate) fn stale(&self, model: &ShellViewModel) -> bool {
        model.revision != self.revision || body_mode(model) != self.mode
    }

    /// 整体同步：写热信号和标题栏；排法变了时侧栏和主区挪到新槽位（第一次排成工作台时才建侧栏）；
    /// 再让每一块写信号、按需重挂。正在组合输入的块延后重挂。
    pub(crate) fn sync(&mut self, document: &mut RuntimeDocument, model: &ShellViewModel) -> Result<(), FrameworkError> {
        if !self.alive() {
            eprintln!("Nana 壳层骨架已随文档回收，跳过同步");
            return Err(FrameworkError::MissingView(self.shell.stable_id()));
        }
        self.write_signals(document, model)?;
        let mode = body_mode(model);
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx { document: &mut *document, hot: self.hot, stats: &mut self.stats };
            if mode != self.mode {
                absorb(&mut swap, &mut error, PartId::Sidebar, self.sidebar.mount(&mut cx, model, mode));
                absorb(&mut swap, &mut error, PartId::Primary, self.primary.mount(&mut cx, model, mode));
            }
            absorb(&mut swap, &mut error, PartId::Sidebar, step(&mut self.sidebar, &mut cx, model, &mut self.deferred));
            absorb(&mut swap, &mut error, PartId::Primary, step(&mut self.primary, &mut cx, model, &mut self.deferred));
            absorb(&mut swap, &mut error, PartId::Overlay, step(&mut self.overlay, &mut cx, model, &mut self.deferred));
        }
        self.settle(document, mode, swap, error)?;
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
        if self.deferred.is_empty() || !self.alive() {
            return Ok(());
        }
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx { document: &mut *document, hot: self.hot, stats: &mut self.stats };
            for id in self.deferred.clone() {
                let result = match id {
                    PartId::Sidebar if !self.sidebar.composing(cx.document) => step(&mut self.sidebar, &mut cx, model, &mut self.deferred),
                    PartId::Primary if !self.primary.composing(cx.document) => step(&mut self.primary, &mut cx, model, &mut self.deferred),
                    PartId::Overlay if !self.overlay.composing(cx.document) => step(&mut self.overlay, &mut cx, model, &mut self.deferred),
                    _ => continue,
                };
                absorb(&mut swap, &mut error, id, result);
            }
        }
        self.settle(document, self.mode, swap, error)
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
    ///
    /// 工作区每次 `update_component` 都会重投影（哪怕夹到上下限以后区域尺寸没变），冲掉 AppShell 给 body
    /// 打的布局补丁，所以工作区是 body 时写完总让 AppShell 重新投影补回。
    fn sync_sidebar_width(&mut self, context: &mut AppContext, width: f32) -> Result<(), FrameworkError> {
        if self.sidebar_width == width {
            return Ok(());
        }
        self.sidebar_width = width;
        context.update_component(self.workspace, |workspace, _| hot::set_resources_width(workspace, width))?;
        if self.mode == BodyMode::Workbench {
            context.reproject_component(self.shell)?;
        }
        Ok(())
    }

    /// 各块的根放进槽位，再卸掉换下来的旧内容、找回状态。放不进槽位时记日志，下一次同步再放。
    fn settle(
        &mut self,
        document: &mut RuntimeDocument,
        mode: BodyMode,
        swap: Swap,
        error: Option<FrameworkError>,
    ) -> Result<(), FrameworkError> {
        let placed = self.place(document.context_mut(), mode);
        match &placed {
            Ok(()) => self.mode = mode,
            Err(placing) => eprintln!("Nana 壳层内容放进槽位失败：{placing}"),
        }
        swap.finish(document);
        match error {
            Some(error) => Err(error),
            None => placed,
        }
    }

    /// 把三块内容的根放进槽位。有侧栏时工作区是 body；主区独占时主区外框挪进壳层底色，底色是 body，
    /// 侧栏留在停放的工作区里。
    fn place(&self, context: &mut AppContext, mode: BodyMode) -> Result<(), FrameworkError> {
        let (sidebar, primary, overlay) = (self.sidebar.root(), self.primary.root(), self.overlay.root());
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
                // 工作区先放下主区外框（侧栏留在工作区里，跟着工作区一起停放），外框再进壳层底色，
                // 最后底色当 body。外框挪位置时不重建，焦点和滚动由 `Swap` 收尾时找回。
                place_regions(context, self.workspace, sidebar, None)?;
                self.primary.hold_solo(context)?;
                place_shell(context, self.shell, primary, overlay).map(|_| ())
            }
        }
    }
}

/// 一块的常规同步：写信号；有变化要重挂时，组合中就记下延后，否则重挂。
fn step<P: ShellPart>(
    part: &mut P,
    cx: &mut PartCx<'_>,
    model: &ShellViewModel,
    deferred: &mut Vec<PartId>,
) -> Result<Swap, FrameworkError> {
    part.sync(model);
    if !part.needs_remount(model) {
        deferred.retain(|id| *id != P::ID);
        return Ok(Swap::default());
    }
    if part.composing(cx.document) {
        if !deferred.contains(&P::ID) {
            deferred.push(P::ID);
        }
        cx.stats.deferred += 1;
        return Ok(Swap::default());
    }
    deferred.retain(|id| *id != P::ID);
    part.remount(cx, model)
}

/// 收下一块的结果：成功的并进收尾，失败的记日志，留下第一个错误。失败的块保留旧内容。
fn absorb(swap: &mut Swap, error: &mut Option<FrameworkError>, part: PartId, result: Result<Swap, FrameworkError>) {
    match result {
        Ok(done) => swap.merge(done),
        Err(failed) => {
            eprintln!("Nana 壳层内容 {part:?} 挂载失败：{failed}");
            error.get_or_insert(failed);
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
