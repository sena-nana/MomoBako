//! 壳层视图的测试支撑，只在测试里编译。
//!
//! 挂一棵和生产窗口相同的壳层，归约消息后按生产 `update` 末尾那一步同步视图，按生产 `prepare`
//! 跑准备帧；按无障碍名找节点，往聚焦的输入框注入输入法预编辑，读响应式计数和视图的同步计数，
//! 并把增量更新后的文档和同一 ViewModel 新挂的文档按无障碍树比对。

use nana_ui::runtime::view::{reactive_stats, ReactiveStats};
use nana_ui::runtime::{AccessibilityNode, AccessibilityRole, DocumentId, LayoutViewport, RuntimeDocument, StableNodeId};
use nana_ui::{ApplicationWindow, CompositionInput, HeadlessInput, NanaTextShaper};

use super::view_host::ViewStats;
use super::{ShellMessage, ShellView, ShellViewModel};

/// 一扇无头窗口里的生产壳层。`model` 是唯一状态源，视图只从它同步。
pub(crate) struct ShellHarness {
    pub model: ShellViewModel,
    pub window: ApplicationWindow,
    view: ShellView,
    input: HeadlessInput,
    shaper: NanaTextShaper,
    viewport: LayoutViewport,
}

impl ShellHarness {
    /// 按 1200×800 挂载并布局一次。
    pub fn mount(model: ShellViewModel) -> Self {
        Self::mount_at(model, 1200.0, 800.0)
    }

    /// 按给定视口挂载。ViewModel 先记下窗口宽，和宿主报告 `Ready` 时一样。
    pub fn mount_at(mut model: ShellViewModel, width: f32, height: f32) -> Self {
        model.set_viewport_width(width);
        model.surface_dirty = false;
        let mut window = ApplicationWindow::new();
        window.document = RuntimeDocument::new(DocumentId::new(1).expect("文档编号"));
        let view = ShellView::mount(&mut window.document, &model).expect("挂载生产壳层");
        let document_id = window.document.document();
        let input = HeadlessInput::bind(window.document.context_mut(), document_id);
        let mut harness = Self {
            model,
            window,
            view,
            input,
            shaper: NanaTextShaper::default(),
            viewport: LayoutViewport::new(width, height),
        };
        harness.flush();
        harness
    }

    pub fn document(&self) -> &RuntimeDocument {
        &self.window.document
    }

    /// 归约一条消息，再整体同步视图（生产 `update` 末尾那一步）。
    pub fn apply(&mut self, message: ShellMessage) {
        self.model.reduce(message);
        self.sync();
    }

    /// 把视图整体同步到当前 ViewModel，不刷新绑定、不布局。
    pub fn sync(&mut self) {
        self.view.sync(&mut self.window.document, &self.model).expect("同步壳层");
        self.model.surface_dirty = false;
    }

    /// 照生产 `prepare` 跑一帧准备（计时器和播放时钟、动效时钟、指针、同步），再刷新布局。
    pub fn frame(&mut self) {
        super::poll_timers(&mut self.model);
        crate::window_host::prepare_motion(&mut self.model, Some(&mut self.view), &mut self.window);
        self.flush();
    }

    /// 跑一帧：刷新绑定、排版、布局。
    pub fn flush(&mut self) {
        self.window.document.flush(self.viewport, &mut self.shaper).expect("布局");
    }

    /// 视图的同步计数。
    pub fn view_stats(&self) -> ViewStats {
        self.view.stats()
    }

    /// 当前文档的无障碍树。
    pub fn nodes(&self) -> Vec<AccessibilityNode> {
        self.document().context().world().project_accessibility(self.document().document())
    }

    /// 无障碍名正好是 `label` 的第一个节点。
    pub fn node(&self, label: &str) -> StableNodeId {
        self.find(label).unwrap_or_else(|| panic!("没有无障碍名为 {label} 的节点"))
    }

    /// 无障碍名正好是 `label` 的第一个节点，没有时为 `None`。
    pub fn find(&self, label: &str) -> Option<StableNodeId> {
        self.nodes().into_iter().find(|node| node.label.as_deref() == Some(label)).map(|node| node.id)
    }

    /// 无障碍名是 `label` 的输入框。
    pub fn input(&self, label: &str) -> StableNodeId {
        self.nodes()
            .into_iter()
            .find(|node| node.role == AccessibilityRole::TextInput && node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("没有名为 {label} 的输入框"))
            .id
    }

    /// 输入框当前的文字（不含预编辑）。
    pub fn value(&self, id: StableNodeId) -> String {
        self.document()
            .context()
            .world()
            .text_input(id)
            .map(|input| input.value.to_string())
            .unwrap_or_else(|| panic!("节点 {} 不是输入框", id.get()))
    }

    /// 文档焦点。
    pub fn focused(&self) -> Option<StableNodeId> {
        self.document().context().world().focused(self.document().document())
    }

    /// 把焦点放到 `id` 上。
    pub fn focus(&mut self, id: StableNodeId) {
        let document_id = self.window.document.document();
        self.window.document.context_mut().focus_node(document_id, id).expect("聚焦");
    }

    /// 在聚焦的输入框上开始（或继续）组合输入，预编辑是 `preedit`，光标在末尾。
    pub fn compose(&mut self, preedit: &str) {
        let composing = self.focused().and_then(|id| self.preedit(id)).is_some();
        if !composing {
            self.route(CompositionInput::Start);
        }
        let end = preedit.len();
        self.route(CompositionInput::Update { text: preedit.into(), selection: Some((end, end)) });
    }

    /// 提交组合输入并结束。
    pub fn commit(&mut self, text: &str) {
        self.route(CompositionInput::Commit(text.into()));
        self.route(CompositionInput::End);
    }

    /// 往聚焦的输入框里直接提交文字，像键盘打字一样。发出的消息留在队列里，由调用方决定何时归约。
    pub fn type_text(&mut self, text: &str) {
        self.input.text(self.window.document.context_mut(), text).expect("输入文字");
    }

    /// 不提交，直接结束组合输入（输入法取消）。
    pub fn cancel_composition(&mut self) {
        self.route(CompositionInput::End);
    }

    /// 浮层和主区内容的根节点。
    pub fn content_roots(&self) -> (Option<StableNodeId>, Option<StableNodeId>) {
        self.view.content_roots()
    }

    fn route(&mut self, composition: CompositionInput) {
        self.input.composition(self.window.document.context_mut(), composition).expect("输入法事件");
    }

    /// 节点上非空的预编辑文字。
    pub fn preedit(&self, id: StableNodeId) -> Option<String> {
        self.document()
            .context()
            .world()
            .ime(id)
            .map(|ime| ime.text.to_string())
            .filter(|text| !text.is_empty())
    }

    /// 视图发给程序的消息，按发出顺序。
    pub fn take_messages(&mut self) -> Vec<ShellMessage> {
        self.window
            .document
            .context_mut()
            .take_program_messages()
            .into_iter()
            .map(|message| *message.downcast::<ShellMessage>().expect("壳层消息"))
            .collect()
    }

    /// 本线程响应式运行时的计数。
    pub fn stats() -> ReactiveStats {
        reactive_stats()
    }

    /// 增量同步后的文档和同一 ViewModel 新挂的文档逐个比较无障碍节点：角色、名称、值和布局盒。
    pub fn assert_same_as_fresh_mount(&mut self) {
        self.flush();
        let ours = semantic_lines(&self.nodes());
        let mut fresh = RuntimeDocument::new(DocumentId::new(1).expect("文档编号"));
        let _view = ShellView::mount(&mut fresh, &self.model).expect("新挂对照文档");
        fresh.flush(self.viewport, &mut self.shaper).expect("对照文档布局");
        let theirs = semantic_lines(&fresh.context().world().project_accessibility(fresh.document()));
        if ours == theirs {
            return;
        }
        let first = ours.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
        let window = |lines: &[String]| lines[first.saturating_sub(3)..(first + 4).min(lines.len())].join("\n");
        panic!(
            "增量同步后的无障碍树和新挂的不一样（{} 对 {} 个节点，第 {first} 个起不同）\n增量：\n{}\n新挂：\n{}",
            ours.len(),
            theirs.len(),
            window(&ours),
            window(&theirs)
        );
    }
}

/// 一个节点的可比较部分。盒子取到 0.1 像素，节点编号不比。
fn semantic_lines(nodes: &[AccessibilityNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|node| {
            let bounds = node.bounds;
            format!(
                "{:?} {:?} {:?} {:.1},{:.1} {:.1}x{:.1}",
                node.role,
                node.label.as_deref().unwrap_or(""),
                node.value.as_ref().map(|value| value.as_str()).unwrap_or(""),
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ShellHarness;
    use crate::shell::{InspectMessage, ShellMessage, ShellPage, ShellViewModel};

    /// 支撑自身：输入框能接住预编辑，提交后发出搜索消息；消息归约同步之后，文档和同一 ViewModel
    /// 新挂的文档一致；反复同步不留下信号、副作用和作用域。
    #[test]
    fn harness_applies_messages_and_injects_a_preedit() {
        let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::FileList));
        assert!(harness.document().context().world().contains(harness.node("MomoBako")));
        let search = harness.input("全局搜索");
        harness.focus(search);
        for (preedit, text) in [("fengmian", "封面"), ("zhong", "中")] {
            harness.compose(preedit);
            assert_eq!(harness.preedit(search).as_deref(), Some(preedit));
            harness.commit(text);
            assert_eq!(harness.preedit(search), None);
            let messages = harness.take_messages();
            assert!(
                messages.iter().any(|message| matches!(message, ShellMessage::Inspect(InspectMessage::SetQuery(_)))),
                "提交组合输入后没有发出搜索消息"
            );
            for message in messages {
                harness.apply(message);
            }
            harness.flush();
        }
        assert_eq!(harness.value(search), "封面中");
        assert_eq!(harness.model.inspect.query, "封面中");
        let settled = ShellHarness::stats();
        for _ in 0..3 {
            harness.apply(ShellMessage::Refresh);
            harness.flush();
        }
        let after = ShellHarness::stats();
        assert_eq!(
            (after.signals, after.effects, after.scopes),
            (settled.signals, settled.effects, settled.scopes),
            "反复同步留下了响应式状态"
        );
        harness.assert_same_as_fresh_mount();
    }
}
