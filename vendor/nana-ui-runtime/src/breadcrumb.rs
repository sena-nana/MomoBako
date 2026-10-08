//! Title-bar breadcrumb: `parent › context` path segments for the shell's
//! centered title column (see `DesktopShell::title_center`).
//!
//! Visual language: earlier segments are muted, the last segment carries the
//! text color with a heavier weight, and separators are faint `›`. Segments
//! center within the fixed column, matching the default title text. Segments
//! ellipsize individually when the fixed center column runs out of room.
//! Interactive segments emit [`BreadcrumbEvent::Activate`] with their index
//! (forwarded from the segment to the [`Breadcrumb`] entity, so hosts bind
//! once on the bar); static segments stay pointer-transparent so the window
//! drag in the title bar keeps working across them. Reconcile items with
//! [`AppContext::set_breadcrumb_items`]: segment and separator nodes are
//! retained across calls, so a per-sync caller can update labels in place
//! without rebuilding nodes.

use std::sync::Arc;

use crate::view_components::{Text, project_common};
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, Entity, FrameworkError,
    InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId, TextContent, UiWorld,
};
use nana_ui_core::{AlignSpec, FlexDirection, JustifySpec, LengthSpec, SemanticColorRole};

/// 面包屑分隔符:父子段之间的弱化箭头。
const BREADCRUMB_SEPARATOR: &str = "›";

/// 段的视觉层级:路径前缀弱化,末段承接标题强调。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreadcrumbTone {
    /// 路径前缀(Muted,常规字重)。
    Parent,
    /// 当前层级(Text,600 字重)。
    Current,
}

/// 一个路径段:显示文本 + 层级 + 是否可激活。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbItem {
    pub label: Arc<str>,
    pub tone: BreadcrumbTone,
    pub interactive: bool,
}

impl BreadcrumbItem {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            tone: BreadcrumbTone::Current,
            interactive: false,
        }
    }

    pub fn tone(mut self, tone: BreadcrumbTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }
}

/// 段被激活时由面包屑实体上报(携带段下标)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreadcrumbEvent {
    pub index: usize,
}

/// 面包屑段:单段文本节点。交互段可激活,静态段对指针透明(标题栏拖拽
/// 直接穿透)。
#[derive(Debug, Clone, PartialEq)]
pub struct BreadcrumbSegment {
    pub label: Arc<str>,
    pub tone: BreadcrumbTone,
    pub interactive: bool,
    pub(crate) index: usize,
    pub style: NodeStyle,
}

impl BreadcrumbSegment {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    fn new(item: &BreadcrumbItem, index: usize) -> Self {
        Self {
            label: Arc::clone(&item.label),
            tone: item.tone,
            interactive: item.interactive,
            index,
            style: segment_style(),
        }
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(match self.tone {
            BreadcrumbTone::Parent => SemanticColorRole::Muted,
            BreadcrumbTone::Current => SemanticColorRole::Text,
        });
        if self.tone == BreadcrumbTone::Current {
            let layout = Arc::make_mut(&mut style.layout);
            layout.font_weight = Some(nana_ui_core::type_scale::SEMIBOLD);
        }
        style
    }
}

/// 段布局:不换行 + 省略号 + 可收缩,超宽时段各自截断而不是撑破标题列。
fn segment_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.white_space_nowrap = true;
    layout.text_overflow_ellipsis = true;
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.flex_shrink = Some(1.0);
    style
}

impl ComponentView for BreadcrumbSegment {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_breadcrumb_segment),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "breadcrumb-segment".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some(self.label.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: self.label.to_string().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: self.interactive,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Text,
                label: Some(Arc::clone(&self.label)),
                ..AccessibilityState::default()
            },
        );
    }
}

/// 标题栏面包屑:段 + 弱化分隔符的横排,挂在 `DesktopShell::title_center`。
#[derive(Debug, Clone, PartialEq)]
pub struct Breadcrumb {
    pub items: Vec<BreadcrumbItem>,
    pub style: NodeStyle,
    pub(crate) segments: Vec<StableNodeId>,
    pub(crate) separators: Vec<StableNodeId>,
}

impl Breadcrumb {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new() -> Self {
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.justify_content = JustifySpec::Center;
        layout.gap = Some(LengthSpec::Px(6.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        Self {
            items: Vec::new(),
            style,
            segments: Vec::new(),
            separators: Vec::new(),
        }
    }

    /// 已装配的段节点(与 items 同序;分隔符不在其中)。
    pub fn segment_nodes(&self) -> &[StableNodeId] {
        &self.segments
    }
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for Breadcrumb {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "breadcrumb".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState::default(),
        );
    }
}

impl AppContext {
    /// 把条目集合同步到面包屑:复用既有段/分隔符节点,缺失则创建,多余
    /// 则收起;条目与上次相同则短路。子序恒为 `段 分隔符 段 …`。
    pub fn set_breadcrumb_items(
        &mut self,
        breadcrumb: Entity<Breadcrumb>,
        items: Vec<BreadcrumbItem>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(breadcrumb.stable_id())
            .ok_or(FrameworkError::MissingView(breadcrumb.stable_id()))?
            .document;
        if self.read(breadcrumb, |breadcrumb| breadcrumb.items == items)? {
            return Ok(false);
        }
        let (mut segments, mut separators) = self.read(breadcrumb, |breadcrumb| {
            (breadcrumb.segments.clone(), breadcrumb.separators.clone())
        })?;
        let wanted_separators = items.len().saturating_sub(1);
        let mut removed = Vec::new();
        if segments.len() > items.len() {
            removed.extend(segments.split_off(items.len()));
        }
        if separators.len() > wanted_separators {
            removed.extend(separators.split_off(wanted_separators));
        }
        while segments.len() < items.len() {
            let index = segments.len();
            let segment = self.create_detached_component(
                document,
                BreadcrumbSegment::new(&items[index], index),
            )?;
            self.observe(segment, breadcrumb, |_, event: &BreadcrumbEvent, cx| {
                cx.emit(*event);
            })?;
            segments.push(segment.stable_id());
        }
        while separators.len() < wanted_separators {
            let mut separator = Text::new(BREADCRUMB_SEPARATOR);
            separator.style.foreground = Some(SemanticColorRole::Faint);
            let separator = self.create_detached_component(document, separator)?;
            separators.push(separator.stable_id());
        }

        for (index, item) in items.iter().enumerate() {
            let segment = Entity::<BreadcrumbSegment>::from_stable_id(segments[index]);
            self.update_component(segment, |segment, _| {
                segment.label = Arc::clone(&item.label);
                segment.tone = item.tone;
                segment.interactive = item.interactive;
                segment.index = index;
            })?;
        }
        self.update_component(breadcrumb, |breadcrumb, _| {
            breadcrumb.items = items;
            breadcrumb.segments = segments.clone();
            breadcrumb.separators = separators.clone();
        })?;

        let mut queue = MutationQueue::new();
        for id in removed {
            queue.park_subtree(id);
        }
        if !queue.is_empty() {
            self.commit_mutations(queue)?;
        }
        for index in 0..segments.len() {
            self.append_child(
                breadcrumb,
                Entity::<BreadcrumbSegment>::from_stable_id(segments[index]),
            )?;
            if index < separators.len() {
                self.append_child(
                    breadcrumb,
                    Entity::<Text>::from_stable_id(separators[index]),
                )?;
            }
        }
        Ok(true)
    }

    pub fn activate_breadcrumb_segment(
        &mut self,
        entity: Entity<BreadcrumbSegment>,
    ) -> Result<bool, FrameworkError> {
        let (interactive, index) =
            self.read(entity, |segment| (segment.interactive, segment.index))?;
        if !interactive {
            return Ok(false);
        }
        self.update_component(entity, |_, cx| cx.emit(BreadcrumbEvent { index }))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn items(labels: &[(&str, BreadcrumbTone, bool)]) -> Vec<BreadcrumbItem> {
        labels
            .iter()
            .map(|(label, tone, interactive)| {
                BreadcrumbItem::new(*label)
                    .tone(*tone)
                    .interactive(*interactive)
            })
            .collect()
    }

    #[test]
    fn set_items_reconciles_segments_and_separators() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let breadcrumb = context
            .create_component(document, Breadcrumb::new())
            .unwrap();
        assert!(
            context
                .set_breadcrumb_items(
                    breadcrumb,
                    items(&[
                        ("未命名 1", BreadcrumbTone::Parent, false),
                        ("fs_main", BreadcrumbTone::Current, true)
                    ]),
                )
                .unwrap(),
            "首次同步必须上报变更"
        );
        let children = context
            .world()
            .node(breadcrumb.stable_id())
            .map(|node| node.children.clone())
            .unwrap();
        assert_eq!(children.len(), 3, "段 分隔符 段:{children:?}");

        // 相同条目短路;段节点复用,标签原地更新。
        let snapshot = context.read(breadcrumb, Clone::clone).unwrap();
        assert!(
            !context
                .set_breadcrumb_items(
                    breadcrumb,
                    items(&[
                        ("未命名 1", BreadcrumbTone::Parent, false),
                        ("fs_main", BreadcrumbTone::Current, true)
                    ]),
                )
                .unwrap()
        );
        context
            .set_breadcrumb_items(
                breadcrumb,
                items(&[
                    ("效果图", BreadcrumbTone::Parent, false),
                    ("tint", BreadcrumbTone::Current, true),
                ]),
            )
            .unwrap();
        let reused = context.read(breadcrumb, Clone::clone).unwrap();
        assert_eq!(reused.segments, snapshot.segments, "段节点必须复用");
        assert_eq!(reused.separators, snapshot.separators);
        let label = context
            .read(
                Entity::<BreadcrumbSegment>::from_stable_id(reused.segments[1]),
                |segment| segment.label.to_string(),
            )
            .unwrap();
        assert_eq!(label, "tint");

        // 缩短条目:多余段与分隔符被收起,子列表跟着收缩。
        context
            .set_breadcrumb_items(
                breadcrumb,
                items(&[("只有一段", BreadcrumbTone::Current, false)]),
            )
            .unwrap();
        let children = context
            .world()
            .node(breadcrumb.stable_id())
            .map(|node| node.children.clone())
            .unwrap();
        assert_eq!(children.len(), 1, "单段无分隔符:{children:?}");
    }

    #[test]
    fn interactive_segment_activates_and_static_does_not() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let breadcrumb = context
            .create_component(document, Breadcrumb::new())
            .unwrap();
        context
            .set_breadcrumb_items(
                breadcrumb,
                items(&[
                    ("未命名 1", BreadcrumbTone::Parent, false),
                    ("fs_main", BreadcrumbTone::Current, true),
                ]),
            )
            .unwrap();
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        context
            .on(breadcrumb, move |_, event: &BreadcrumbEvent, _| {
                sink.lock().unwrap().push(event.index);
            })
            .unwrap();

        let segments = context.read(breadcrumb, |b| b.segments.clone()).unwrap();
        // 静态段:激活被拒绝且不上报。
        let static_segment = Entity::<BreadcrumbSegment>::from_stable_id(segments[0]);
        assert!(!context.activate_breadcrumb_segment(static_segment).unwrap());
        // 交互段:事件携带段下标转发到面包屑实体。
        let symbol = Entity::<BreadcrumbSegment>::from_stable_id(segments[1]);
        assert!(context.activate_breadcrumb_segment(symbol).unwrap());
        assert_eq!(*events.lock().unwrap(), vec![1]);
    }

    /// 面包屑段组在标题栏内水平居中,与默认标题文本同语义(中列自身固定
    /// 宽且严格居中,左右列均分剩余空间)。同时回归覆盖:装配前的首次
    /// project 不得把中列样式打到宿主挂载的面包屑根节点上。
    #[test]
    fn breadcrumb_centers_within_the_title_bar() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let breadcrumb = context
            .create_component(document, Breadcrumb::new())
            .unwrap();
        context
            .set_breadcrumb_items(
                breadcrumb,
                items(&[
                    ("效果图", BreadcrumbTone::Parent, false),
                    ("fs_main", BreadcrumbTone::Current, true),
                ]),
            )
            .unwrap();
        let bar = context
            .create_component(
                document,
                crate::AppTitleBar::new("Nana")
                    .center(breadcrumb.stable_id())
                    .center_width(440.0),
            )
            .unwrap();
        context.assemble_app_title_bar(bar).unwrap();

        let text_nodes = context
            .read(breadcrumb, |b| {
                b.segments
                    .iter()
                    .copied()
                    .chain(b.separators.iter().copied())
                    .collect::<Vec<_>>()
            })
            .unwrap();
        context
            .shape_text(&text_nodes, &mut crate::MeasureTextShaper)
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(800.0, 400.0))
            .unwrap();

        let world = context.world();
        let bar_box = world.layout_box(bar.stable_id()).unwrap();
        let segments = context.read(breadcrumb, |b| b.segments.clone()).unwrap();
        let first = world.layout_box(segments[0]).unwrap();
        let last = world.layout_box(*segments.last().unwrap()).unwrap();
        let left = first.x - bar_box.x;
        let right = (bar_box.x + bar_box.width) - (last.x + last.width);
        assert!(
            (left - right).abs() < 1.0,
            "段组必须在标题栏内水平居中:left={left}, right={right}"
        );
    }
}
