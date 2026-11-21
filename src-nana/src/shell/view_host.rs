//! 常驻的壳层视图 `ShellView`：只做调度。
//!
//! 骨架只挂一次：AppShell、标题栏（字段绑定，搜索框受控）、浮层层（AppShell 的 overlay 槽位）、
//! 工作区（资源区宽度跟侧栏宽度，侧栏收起展开时资源区按工作区自己的区域过渡收起展开）和一个认出这棵骨架的
//! 隐藏标记。内容分三块，各在自己的模块里：
//! 侧栏（`view_part_sidebar.rs`）、主区（`view_part_primary.rs`，按路由键切换分支）和浮层
//! （`view_part_overlay.rs`，按浮层身份换块）。三块实现同一个接口 [`ShellPart`]：挂载、同步、是否
//! 需要重挂、组合延后。这里按同一套流程调它们，再把侧栏和主区的根放进骨架组合控件的槽位：有侧栏时
//! 放进工作区的资源区和主区，工作区是 AppShell 的 body；主区独占时它自己是 body。浮层块自己把各块
//! 放进浮层层，AppShell 不用重新装配。组合控件只在自己投影时给槽位根节点打补丁，所以槽位根节点上
//! 不放绑定。
//!
//! 数据流：`ShellMessage → reduce → 服务副作用 → ShellView::sync`。同步先写热信号和标题栏字段，
//! 再让每一块写自己的信号、按需重挂，最后处理焦点：浮层开合时按统一的规则交还焦点
//! （`focus_return.rs`）；键盘打开的菜单由 NanaUI 把焦点放进去。
//! 焦点在某块里、输入法还有预编辑时，这块的重挂延后，`prepare`
//! 每帧经 [`ShellView::retry_deferred`] 检查，组合结束后补挂；`prepare` 每帧还经
//! [`ShellView::settle_overlays`] 卸掉放完退场的对话框。每帧画完以后（`presented`）经
//! [`ShellView::settle_focus`] 补还刷新时才落定的焦点（对话框关上、控件卸掉）。
//! 填充过渡、悬停和播放推进只走 [`ShellView::sync_hot`]。这里只写信号，不刷新绑定，刷新由输入路由和帧开头完成。
//!
//! 侧栏收起：资源区先在工作区里收到 0（区域过渡由 NanaUI 推进），走完以后主区才独占（[`ShellView::stale`]
//! 在这时报变化）；展开：主区先回到工作区，资源区再从 0 展开。过渡期间工作区每帧重投影，`prepare` 经
//! [`ShellView::settle_workspace`] 让 AppShell 补回给 body 打的布局补丁。

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use nana_ui::RegionId;
use nana_ui::runtime::view::{detached, entity_ref, node_ref, widget, with_refs};
use nana_ui::runtime::{
    AppContext, AppShell, Entity, FrameworkError, LanguageTag, LengthSpec, RuntimeDocument,
    StableNodeId, Text, Thumbnail, Workspace, WorkspaceRegionSlot, component_descriptors,
};

use super::ShellViewModel;
use super::focus_return::{Before, FocusReturn, Regions};
use super::hot::{HotFrame, HotSignals, ModelField, TitleBarView, TitleSignals};
use super::view_part::{BodyMode, PartCx, PartId, ShellPart, Swap, body_mode};
use super::view_part_overlay::{OverlayPart, OverlaySignals};
use super::view_part_primary::PrimaryPart;
use super::view_part_sidebar::SidebarPart;
use super::{StartupStatus, render};

/// 同步计数，测试用来确认填充过渡和播放推进不重挂、组合输入会延后。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViewStats {
    /// 重挂的内容块（或主区分支）数。
    pub remounts: u64,
    /// 因输入法组合而延后的次数。
    pub deferred: u64,
}

/// 应用的语言：简体中文。
const APP_LANGUAGE: &str = "zh-CN";

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
    /// 最近写进工作区资源区的侧栏宽度。
    sidebar_width: f32,
    /// 最近交给工作区的资源区收起状态。
    collapsed: bool,
    /// 上一帧资源区还在收起或展开。
    resources_moving: bool,
    sidebar: SidebarPart,
    primary: PrimaryPart,
    overlay: OverlayPart,
    /// 等输入法组合结束才重挂的块。
    deferred: Vec<PartId>,
    /// 浮层关掉以后把焦点还回去。
    focus_return: FocusReturn,
    /// 最近一次整体同步时 ViewModel 的版本。
    revision: u64,
    /// 上次收集挂着的缩略图时文档的变更代次。
    thumbnails_scanned: Option<u64>,
    /// 歌词当前行上次滚到中间时的视口。同一行不再拉回去。
    lyric_follow: Option<super::inspect_view::LyricFollow>,
    stats: ViewStats,
}

impl ShellView {
    /// 挂骨架，再把三块内容按 `model` 挂进槽位。骨架和各块的常驻信号都建在这次挂载的作用域里。
    ///
    /// 挂之前先给这份文档声明应用语言（[`APP_LANGUAGE`]）：中文标点挤压、避头尾按中文排，拉丁文字和数字
    /// 也按简体中文挑字形，和 Chrome 用同一份字体画 `lang="zh-CN"` 一致。真实窗口、测试 harness 和截图场景
    /// 都从这里挂，排出来的字一样。
    pub(crate) fn mount(
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> Result<Self, FrameworkError> {
        let document_id = document.document();
        document
            .context_mut()
            .set_default_language(LanguageTag::new(APP_LANGUAGE));
        let marker_text = format!(
            "{MARKER_PREFIX}{}",
            NEXT_MARKER.fetch_add(1, Ordering::Relaxed)
        );
        let hot_frame = HotFrame::project(model);
        let (sidebar_width, collapsed) = (
            model.workspace.sidebar_width,
            model.workspace.sidebar_collapsed,
        );
        let title_view = TitleBarView::project(model);
        let mut made = None;
        let (_skeleton, (shell, workspace, marker)) =
            document.context_mut().mount_view_root(document_id, || {
                let hot = HotSignals::new(hot_frame);
                let title = TitleSignals::new(&title_view);
                let query = ModelField::new(&title_view.query);
                let sidebar = SidebarPart::signals(model);
                // 仓库弹层的页由浮层块挂进侧栏仓库头的弹层里，两边共用那一格。
                let overlay = OverlaySignals {
                    popover_slot: sidebar.popover_slot,
                    ..OverlayPart::signals(model)
                };
                let parts = (sidebar, PrimaryPart::signals(model), overlay);
                let shell = entity_ref::<AppShell>();
                let workspace = entity_ref::<Workspace>();
                let marker = node_ref();
                let view = (
                    detached(widget(hidden_marker(&marker_text)).node_ref(marker)),
                    detached(
                        widget(render::workbench_workspace(sidebar_width, collapsed))
                            .entity_ref(workspace),
                    ),
                    widget(AppShell::new())
                        .title_bar(super::title_bar::title_bar(
                            title,
                            query.signal(),
                            &title_view.query,
                        ))
                        .overlay(super::view_part_overlay::layer(parts.2.layer))
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
            sidebar_width,
            collapsed,
            resources_moving: false,
            sidebar: SidebarPart::new(sidebar),
            primary: PrimaryPart::new(primary),
            overlay: OverlayPart::new(overlay),
            deferred: Vec::new(),
            focus_return: FocusReturn::default(),
            revision: model.revision,
            thumbnails_scanned: None,
            lyric_follow: None,
            stats: ViewStats::default(),
        };
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx {
                document: &mut *document,
                hot,
                stats: &mut view.stats,
            };
            absorb(
                &mut swap,
                &mut error,
                PartId::Sidebar,
                view.sidebar.mount(&mut cx, model, mode),
            );
            absorb(
                &mut swap,
                &mut error,
                PartId::Primary,
                view.primary.mount(&mut cx, model, mode),
            );
            absorb(
                &mut swap,
                &mut error,
                PartId::Overlay,
                view.overlay.mount(&mut cx, model, mode),
            );
        }
        view.place(document.context_mut(), mode)?;
        swap.finish(document);
        view.focus_return = FocusReturn::new(view.overlay.identity());
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

    /// ViewModel 有没有骨架之外要同步的变化：版本变了，或者工作台排法换了（包括侧栏收起的过渡刚走完）。
    pub(crate) fn stale(&self, document: &RuntimeDocument, model: &ShellViewModel) -> bool {
        model.revision != self.revision || self.layout_mode(document.context(), model) != self.mode
    }

    /// 这一刻的排法。侧栏收起时资源区先在工作区里收到 0：过渡没走完以前留在工作台排法，走完才换成主区独占。
    fn layout_mode(&self, context: &AppContext, model: &ShellViewModel) -> BodyMode {
        let wanted = body_mode(model);
        let collapsing = wanted == BodyMode::Solo
            && self.mode == BodyMode::Workbench
            && model.workspace.sidebar_collapsed
            && model.workspace.startup.status == StartupStatus::Ready
            && self.resources_moving_now(context);
        if collapsing {
            BodyMode::Workbench
        } else {
            wanted
        }
    }

    /// 工作区的资源区正在收起或展开。
    fn resources_moving_now(&self, context: &AppContext) -> bool {
        context
            .read(self.workspace, render::resources_moving)
            .unwrap_or(false)
    }

    /// 资源区还在收起或展开：准备帧要一直要帧，走完以后才换排法。
    pub(crate) fn resources_moving(&self) -> bool {
        self.resources_moving
    }

    /// 每帧收尾：资源区收起展开期间工作区每帧重投影，冲掉 AppShell 给 body 打的布局补丁，让 AppShell
    /// 重新投影补回；过渡走完的那一帧再补一次。
    pub(crate) fn settle_workspace(&mut self, document: &mut RuntimeDocument) {
        if !self.alive() {
            return;
        }
        let context = document.context_mut();
        let moving = self.mode == BodyMode::Workbench && self.resources_moving_now(context);
        if (moving || self.resources_moving)
            && self.mode == BodyMode::Workbench
            && let Err(error) = context.reproject_component(self.shell)
        {
            eprintln!("Nana 侧栏收起展开时补 AppShell 布局失败：{error}");
        }
        self.resources_moving = moving;
    }

    /// 整体同步：写热信号和标题栏；排法变了时侧栏和主区挪到新槽位（第一次排成工作台时才建侧栏）；
    /// 再让每一块写信号、按需重挂。正在组合输入的块延后重挂。最后按浮层的开合交还焦点，再照键盘
    /// 打开的菜单放焦点。
    pub(crate) fn sync(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> Result<(), FrameworkError> {
        if !self.alive() {
            eprintln!("Nana 壳层骨架已随文档回收，跳过同步");
            return Err(FrameworkError::MissingView(self.shell.stable_id()));
        }
        let document_id = document.document();
        let before = Before::capture(document.context(), document_id, self.regions());
        self.write_signals(document, model)?;
        // 收起先在工作区里播，再按过渡有没有走完定排法；展开要等工作区放回 AppShell（下面 `settle` 之后）再播。
        self.follow_collapse(document.context_mut(), model)?;
        let mode = self.layout_mode(document.context(), model);
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx {
                document: &mut *document,
                hot: self.hot,
                stats: &mut self.stats,
            };
            if mode != self.mode {
                absorb(
                    &mut swap,
                    &mut error,
                    PartId::Sidebar,
                    self.sidebar.mount(&mut cx, model, mode),
                );
                absorb(
                    &mut swap,
                    &mut error,
                    PartId::Primary,
                    self.primary.mount(&mut cx, model, mode),
                );
            }
            absorb(
                &mut swap,
                &mut error,
                PartId::Sidebar,
                step(&mut self.sidebar, &mut cx, model, &mut self.deferred),
            );
            absorb(
                &mut swap,
                &mut error,
                PartId::Primary,
                step(&mut self.primary, &mut cx, model, &mut self.deferred),
            );
            absorb(
                &mut swap,
                &mut error,
                PartId::Overlay,
                step(&mut self.overlay, &mut cx, model, &mut self.deferred),
            );
        }
        self.settle(document, mode, swap, error)?;
        self.follow_collapse(document.context_mut(), model)?;
        self.overlay.sweep(document.context_mut());
        self.return_focus(document, model, before);
        self.revision = model.revision;
        Ok(())
    }

    /// 资源区跟上 ViewModel 的侧栏收起状态。工作区在 AppShell 里时交给工作区自己的区域过渡（减少动态效果时
    /// 直接到位）；工作区停放在树外时直接到位，只有马上要放回工作台的展开留到放回以后再播。
    fn follow_collapse(
        &mut self,
        context: &mut AppContext,
        model: &ShellViewModel,
    ) -> Result<(), FrameworkError> {
        let collapsed = model.workspace.sidebar_collapsed;
        if collapsed == self.collapsed {
            return Ok(());
        }
        let placed = self.mode == BodyMode::Workbench;
        if !collapsed && !placed && body_mode(model) == BodyMode::Workbench {
            return Ok(());
        }
        let animate = placed && !context.reduced_motion();
        let now = context.animation_now();
        context.update_component(self.workspace, |workspace, _| {
            render::set_resources_collapsed(workspace, collapsed, now, animate)
        })?;
        self.collapsed = collapsed;
        self.resources_moving |= animate;
        if placed {
            context.reproject_component(self.shell)?;
        }
        Ok(())
    }

    /// 每帧收尾：卸掉放完退场的对话框，现在显示的对话框照 ViewModel 该开着。只碰浮层块。
    pub(crate) fn settle_overlays(&mut self, document: &mut RuntimeDocument) {
        if self.alive() {
            self.overlay.sweep(document.context_mut());
        }
    }

    /// 一帧刷新以后：对话框在刷新时才真正关上，控件也可能在刷新时才卸掉。这时照焦点归还的规则补还
    /// 焦点（`focus_return.rs`），焦点动了返回真，调用方再画一帧。
    pub(crate) fn settle_focus(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> bool {
        if !self.alive() {
            return false;
        }
        let regions = self.regions();
        let document_id = document.document();
        let before = document.context().world().focused(document_id);
        self.focus_return
            .settle(document.context_mut(), document_id, regions);
        let lyric = if super::files_view::previewing(model) && !model.inspect.lyrics.is_empty() {
            super::inspect_view::follow_active_lyric(document, &mut self.lyric_follow)
        } else {
            self.lyric_follow = None;
            false
        };
        document.context().world().focused(document_id) != before || lyric
    }

    /// 页面的几块：侧栏和主区的根、浮层层和它现在显示的那一块。
    fn regions(&self) -> Regions {
        Regions {
            sidebar: self.sidebar.root(),
            primary: self.primary.root(),
            layer: self.overlay.layer_id(),
            overlay: self.overlay.root(),
        }
    }

    /// 按浮层现在显示的那一层交还焦点（`focus_return.rs`）。`before` 是这次同步开始前焦点在哪。
    fn return_focus(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
        before: Before,
    ) {
        let regions = self.regions();
        let document_id = document.document();
        self.focus_return.sync(
            document.context_mut(),
            document_id,
            model,
            self.overlay.identity(),
            before,
            regions,
        );
    }

    /// 还有对话框在放退场：准备帧要一直要帧，放完以后才卸得掉。
    pub(crate) fn overlay_leaving(&self) -> bool {
        self.overlay.leaving()
    }

    /// 文档变了（变更代次和上次收集时不同）以后，收集还有缩略图节点引用的缩略图文件；没变时返回 `None`，
    /// 闲着的帧不重复收集。
    ///
    /// 缩略图节点的资源是 `thumb:{文件}`：文件卡片有缩略图时一直引用它，还没有纹理时只是藏着。视口和前后
    /// 预留的行、详情卡片、播放条和预览里的缩略图都算；滚走的行连节点一起卸掉，就不算了。宿主按这份决定
    /// 解码哪些缩略图、哪些纹理不能卸。
    pub(crate) fn mounted_thumbnails(
        &mut self,
        document: &RuntimeDocument,
    ) -> Option<HashSet<String>> {
        let context = document.context();
        let generation = context.world().generation();
        if self.thumbnails_scanned == Some(generation) {
            return None;
        }
        self.thumbnails_scanned = Some(generation);
        let mut mounted = HashSet::new();
        for node in context.world().nodes_of_component(
            document.document(),
            component_descriptors::THUMBNAIL.type_id,
        ) {
            match context.read(Entity::<Thumbnail>::from_stable_id(node), |thumb| {
                thumb.resource.clone()
            }) {
                Ok(resource) => {
                    if let Some(path) = super::thumbs::slot_file(resource.as_str()) {
                        mounted.insert(path.to_string());
                    }
                }
                Err(error) => eprintln!("Nana 读不到缩略图节点 {}：{error}", node.get()),
            }
        }
        Some(mounted)
    }

    /// 只同步热信号和侧栏宽度：填充过渡、悬停和播放推进走这里，不重挂内容。排法要换时由调用方整体同步。
    pub(crate) fn sync_hot(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> Result<(), FrameworkError> {
        if !self.alive() {
            eprintln!("Nana 壳层骨架已随文档回收，跳过热同步");
            return Err(FrameworkError::MissingView(self.shell.stable_id()));
        }
        self.write_signals(document, model)
    }

    /// 因输入法组合延后的块，组合已经结束的按当前 ViewModel 补挂。
    pub(crate) fn retry_deferred(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> Result<(), FrameworkError> {
        if self.deferred.is_empty() || !self.alive() {
            return Ok(());
        }
        let before = Before::capture(document.context(), document.document(), self.regions());
        let mut swap = Swap::default();
        let mut error = None;
        {
            let mut cx = PartCx {
                document: &mut *document,
                hot: self.hot,
                stats: &mut self.stats,
            };
            for id in self.deferred.clone() {
                let result = match id {
                    PartId::Sidebar if !self.sidebar.composing(cx.document) => {
                        step(&mut self.sidebar, &mut cx, model, &mut self.deferred)
                    }
                    PartId::Primary if !self.primary.composing(cx.document) => {
                        step(&mut self.primary, &mut cx, model, &mut self.deferred)
                    }
                    PartId::Overlay if !self.overlay.composing(cx.document) => {
                        step(&mut self.overlay, &mut cx, model, &mut self.deferred)
                    }
                    _ => continue,
                };
                absorb(&mut swap, &mut error, id, result);
            }
        }
        let settled = self.settle(document, self.mode, swap, error);
        self.return_focus(document, model, before);
        settled
    }

    /// 写热投影和标题栏投影；搜索框草稿只在 ViewModel 的值变了时写。侧栏宽度变了时写进工作区。
    fn write_signals(
        &mut self,
        document: &mut RuntimeDocument,
        model: &ShellViewModel,
    ) -> Result<(), FrameworkError> {
        let title = TitleBarView::project(model);
        self.hot.write(HotFrame::project(model));
        self.title.write(&title);
        self.query.sync(&title.query);
        self.sync_sidebar_width(document.context_mut(), model.workspace.sidebar_width)
    }

    /// 资源区宽度跟着侧栏宽度，只在 ViewModel 的值变了时写，拖动中的区域尺寸不会被旧值拉回。
    ///
    /// 工作区每次 `update_component` 都会重投影（哪怕夹到上下限以后区域尺寸没变），冲掉 AppShell 给 body
    /// 打的布局补丁，所以工作区是 body 时写完总让 AppShell 重新投影补回。
    fn sync_sidebar_width(
        &mut self,
        context: &mut AppContext,
        width: f32,
    ) -> Result<(), FrameworkError> {
        if self.sidebar_width == width {
            return Ok(());
        }
        self.sidebar_width = width;
        context.update_component(self.workspace, |workspace, _| {
            render::set_resources_width(workspace, width)
        })?;
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

    /// 把侧栏和主区的根放进槽位。有侧栏时工作区是 body；主区独占时主区外框挪进壳层底色，底色是
    /// body，侧栏留在停放的工作区里。浮层块自己把各块放进常驻的浮层层，不经这里。
    fn place(&self, context: &mut AppContext, mode: BodyMode) -> Result<(), FrameworkError> {
        let (sidebar, primary) = (self.sidebar.root(), self.primary.root());
        match mode {
            BodyMode::Workbench => {
                let regions = place_regions(context, self.workspace, sidebar, primary)?;
                let shell = place_shell(context, self.shell, Some(self.workspace.stable_id()))?;
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
                place_shell(context, self.shell, primary).map(|_| ())
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
fn absorb(
    swap: &mut Swap,
    error: &mut Option<FrameworkError>,
    part: PartId,
    result: Result<Swap, FrameworkError>,
) {
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
    // 主区外框是借给工作区的：工作区只管区域布局和底色，无障碍和输入仍是外框自己的。
    // 侧栏折叠时外框要挪出工作区（`PrimaryPart::hold_solo`），借用的语义才不会把区域的无障碍留在它身上。
    let slots = sidebar
        .map(|sidebar| WorkspaceRegionSlot::new(RegionId::Resources, sidebar))
        .into_iter()
        .chain(primary.map(|primary| WorkspaceRegionSlot::borrowed(RegionId::Primary, primary)))
        .collect::<Vec<_>>();
    if context.read(workspace, |current| current.slots == slots)? {
        return Ok(false);
    }
    context.update_component(workspace, |current, _| current.slots = slots)?;
    context.assemble_workspace(workspace)?;
    Ok(true)
}

/// AppShell 的 body。没变时不动，返回 `false`；变了以后重新装配（装配最后会重新投影），换下来的
/// 节点由装配停放。overlay 槽位是骨架挂载时交给它的浮层层，一直不换。
fn place_shell(
    context: &mut AppContext,
    shell: Entity<AppShell>,
    body: Option<StableNodeId>,
) -> Result<bool, FrameworkError> {
    if context.read(shell, |current| current.body == body)? {
        return Ok(false);
    }
    context.update_component(shell, |current, _| current.body = body)?;
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
pub fn mount_shell(
    document: &mut RuntimeDocument,
    model: &ShellViewModel,
) -> Result<(), FrameworkError> {
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

/// 测试用：照生产 `prepare` 跑一帧准备，一帧按 16ms 算，视图用 `mount_shell` 挂在这份文档里的那个。
#[cfg(test)]
pub(crate) fn prepare_registered(
    model: &mut ShellViewModel,
    window: &mut nana_ui::ApplicationWindow,
) {
    REGISTERED.with(|views| {
        let mut views = views.borrow_mut();
        let view = views
            .iter_mut()
            .find(|view| view.alive() && view.owns(&window.document));
        crate::window_host::prepare_motion(
            model,
            view,
            window,
            crate::frame_clock::FrameStep::millis(16),
        );
    })
}

#[cfg(test)]
mod tests;
