//! Structured diff review. The application owns the buffer and feeds hunks.

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ButtonKind, ControlSize, FlexDirection, LengthSpec, OverflowSpec, PositionSpec,
    SemanticColorRole,
};

use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::view_components::{Activate, Button, Stack, Text, project_common};
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, Entity, FrameworkError,
    InteractionState, MutationQueue, NodeKind, NodeStyle, ScrollAxes, ScrollView, StableNodeId,
    UiWorld,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffLineKind {
    #[default]
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub old_number: Option<u32>,
    pub new_number: Option<u32>,
    pub kind: DiffLineKind,
    pub text: Arc<str>,
}

impl DiffLine {
    pub fn context(old_number: u32, new_number: u32, text: impl Into<Arc<str>>) -> Self {
        Self {
            old_number: Some(old_number),
            new_number: Some(new_number),
            kind: DiffLineKind::Context,
            text: text.into(),
        }
    }

    pub fn added(new_number: u32, text: impl Into<Arc<str>>) -> Self {
        Self {
            old_number: None,
            new_number: Some(new_number),
            kind: DiffLineKind::Added,
            text: text.into(),
        }
    }

    pub fn removed(old_number: u32, text: impl Into<Arc<str>>) -> Self {
        Self {
            old_number: Some(old_number),
            new_number: None,
            kind: DiffLineKind::Removed,
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: Arc<str>,
    pub lines: Arc<[DiffLine]>,
}

impl DiffHunk {
    pub fn new(header: impl Into<Arc<str>>, lines: impl Into<Arc<[DiffLine]>>) -> Self {
        Self {
            header: header.into(),
            lines: lines.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffLayout {
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffEvent {
    HunkAccepted { hunk: usize },
    HunkRejected { hunk: usize },
    LineAccepted { hunk: usize, line: usize },
    LineRejected { hunk: usize, line: usize },
    LayoutChanged { layout: DiffLayout },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffView {
    pub hunks: Arc<[DiffHunk]>,
    pub layout: DiffLayout,
    pub disabled: bool,
    pub style: NodeStyle,
    wired: Vec<StableNodeId>,
}

impl DiffView {
    pub fn new(hunks: impl Into<Arc<[DiffHunk]>>) -> Self {
        let mut style = NodeStyle::default().surface(SemanticColorRole::Surface);
        let layout = Arc::make_mut(&mut style.layout);
        layout.position = PositionSpec::Relative;
        layout.direction = Some(FlexDirection::Column);
        layout.gap = Some(LengthSpec::Px(8.0));
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        layout.font_family = Some("Cascadia Mono".to_owned());
        layout.font_size = Some(nana_ui_core::type_scale::META);
        Self {
            hunks: hunks.into(),
            layout: DiffLayout::Unified,
            disabled: false,
            style,
            wired: Vec::new(),
        }
    }

    pub fn layout(mut self, layout: DiffLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl ComponentView for DiffView {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        assembler: Some(crate::AppContext::assemble_diff_view),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element { tag: "diff".into() }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: !self.disabled,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::from("差异")),
                disabled: self.disabled,
                ..Default::default()
            },
        );
    }
}

impl RegisterableComponent for DiffView {
    const TYPE_ID: &'static str = crate::component_descriptors::DIFF.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DIFF.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let hunks = hunks_from_spec(spec);
        let mut view = Self::new(hunks);
        view.disabled = spec.disabled;
        view.layout = match spec
            .attr("layout")
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "split" | "side-by-side" => DiffLayout::Split,
            _ => DiffLayout::Unified,
        };
        view
    }
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut view = Self::from_semantic(spec);
        if let Some(previous) = previous {
            view.wired = previous.wired.clone();
        }
        view
    }
    fn finish_semantic(
        context: &mut AppContext,
        entity: Entity<Self>,
    ) -> Result<(), FrameworkError> {
        context.assemble_diff_view(entity).map(|_| ())
    }
}

impl AppContext {
    pub fn assemble_diff_view(&mut self, diff: Entity<DiffView>) -> Result<bool, FrameworkError> {
        let snapshot = self.read(diff, Clone::clone)?;
        self.mount(diff, |ui| {
            let unified = snapshot.layout == DiffLayout::Unified;
            let mut toolbar = NodeStyle::default();
            {
                let layout = Arc::make_mut(&mut toolbar.layout);
                layout.direction = Some(FlexDirection::Row);
                layout.align_items = AlignSpec::Center;
                layout.gap = Some(LengthSpec::Px(8.0));
            }
            ui.with_child(
                "toolbar",
                Stack::from_layout(toolbar.layout.as_ref().clone()),
                |ui| {
                    ui.child(
                        "layout-unified",
                        Button::new("统一")
                            .kind(if unified {
                                ButtonKind::Selected
                            } else {
                                ButtonKind::Subtle
                            })
                            .size(ControlSize::Small)
                            .disabled(snapshot.disabled),
                    )?;
                    ui.child(
                        "layout-split",
                        Button::new("分栏")
                            .kind(if unified {
                                ButtonKind::Subtle
                            } else {
                                ButtonKind::Selected
                            })
                            .size(ControlSize::Small)
                            .disabled(snapshot.disabled),
                    )?;
                    Ok(())
                },
            )?;

            let mut scroll_style = NodeStyle::default();
            {
                let layout = Arc::make_mut(&mut scroll_style.layout);
                layout.flex_grow = Some(1.0);
                layout.min_height = Some(LengthSpec::Px(0.0));
                layout.width = Some(LengthSpec::Fill);
            }
            ui.with_child(
                "body",
                ScrollView::new(ScrollAxes::Vertical).style(scroll_style),
                |ui| {
                    for (hunk_index, hunk) in snapshot.hunks.iter().enumerate() {
                        mount_hunk(ui, hunk_index, hunk, snapshot.layout, snapshot.disabled)?;
                    }
                    Ok(())
                },
            )?;
            Ok(())
        })?;
        self.wire_diff_actions(diff)
    }

    fn wire_diff_actions(&mut self, diff: Entity<DiffView>) -> Result<bool, FrameworkError> {
        let root = diff.stable_id();
        if let Some(unified) = find_labeled_button(self, root, "统一") {
            self.observe_diff_once(unified, diff, |view, _: &Activate, cx| {
                if view.disabled || view.layout == DiffLayout::Unified {
                    return;
                }
                view.layout = DiffLayout::Unified;
                cx.emit(DiffEvent::LayoutChanged {
                    layout: DiffLayout::Unified,
                });
            })?;
        }
        if let Some(split) = find_labeled_button(self, root, "分栏") {
            self.observe_diff_once(split, diff, |view, _: &Activate, cx| {
                if view.disabled || view.layout == DiffLayout::Split {
                    return;
                }
                view.layout = DiffLayout::Split;
                cx.emit(DiffEvent::LayoutChanged {
                    layout: DiffLayout::Split,
                });
            })?;
        }
        let hunks = self.read(diff, |view| view.hunks.len())?;
        for hunk in 0..hunks {
            if let Some(accept) = find_labeled_button(self, root, &format!("接受块{hunk}")) {
                self.observe_diff_once(accept, diff, move |view, _: &Activate, cx| {
                    if !view.disabled {
                        cx.emit(DiffEvent::HunkAccepted { hunk });
                    }
                })?;
            }
            if let Some(reject) = find_labeled_button(self, root, &format!("拒绝块{hunk}")) {
                self.observe_diff_once(reject, diff, move |view, _: &Activate, cx| {
                    if !view.disabled {
                        cx.emit(DiffEvent::HunkRejected { hunk });
                    }
                })?;
            }
            let lines = self.read(diff, |view| {
                view.hunks
                    .get(hunk)
                    .map(|hunk| hunk.lines.len())
                    .unwrap_or(0)
            })?;
            for line in 0..lines {
                if let Some(accept) =
                    find_labeled_button(self, root, &format!("接受行{hunk}.{line}"))
                {
                    self.observe_diff_once(accept, diff, move |view, _: &Activate, cx| {
                        if !view.disabled {
                            cx.emit(DiffEvent::LineAccepted { hunk, line });
                        }
                    })?;
                }
                if let Some(reject) =
                    find_labeled_button(self, root, &format!("拒绝行{hunk}.{line}"))
                {
                    self.observe_diff_once(reject, diff, move |view, _: &Activate, cx| {
                        if !view.disabled {
                            cx.emit(DiffEvent::LineRejected { hunk, line });
                        }
                    })?;
                }
            }
        }
        Ok(true)
    }

    fn observe_diff_once(
        &mut self,
        source: StableNodeId,
        diff: Entity<DiffView>,
        handler: impl FnMut(&mut DiffView, &Activate, &mut crate::ViewContext<'_, DiffView>)
        + Send
        + 'static,
    ) -> Result<(), FrameworkError> {
        let already = self.read(diff, |view| view.wired.contains(&source))?;
        if already {
            return Ok(());
        }
        self.observe(Entity::<Button>::from_stable_id(source), diff, handler)?;
        self.update_component(diff, |view, _| {
            view.wired.push(source);
        })?;
        Ok(())
    }
}

fn find_labeled_button(
    context: &AppContext,
    root: StableNodeId,
    label: &str,
) -> Option<StableNodeId> {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = context.world().node(id) else {
            continue;
        };
        stack.extend(node.children.iter().rev().copied());
        if context.world().text(id) == Some(label)
            || context
                .world()
                .accessibility(id)
                .is_some_and(|state| state.label.as_deref() == Some(label))
        {
            return Some(id);
        }
    }
    None
}

fn mount_hunk(
    ui: &mut crate::framework::AssemblyScope<'_>,
    hunk_index: usize,
    hunk: &DiffHunk,
    layout: DiffLayout,
    disabled: bool,
) -> Result<(), FrameworkError> {
    let mut hunk_style = NodeStyle::default().surface(SemanticColorRole::Subtle);
    {
        let style = Arc::make_mut(&mut hunk_style.layout);
        style.direction = Some(FlexDirection::Column);
        style.gap = Some(LengthSpec::Px(2.0));
        style.padding = Some(LengthSpec::Px(8.0));
        style.border_radius = Some(6.0);
    }
    ui.with_child(
        format!("hunk-{hunk_index}"),
        Stack::from_layout(hunk_style.layout.as_ref().clone()),
        |ui| {
            ui.child(
                "accept",
                Button::new(format!("接受块{hunk_index}"))
                    .kind(ButtonKind::Subtle)
                    .size(ControlSize::Small)
                    .disabled(disabled),
            )?;
            ui.child(
                "reject",
                Button::new(format!("拒绝块{hunk_index}"))
                    .kind(ButtonKind::Subtle)
                    .size(ControlSize::Small)
                    .disabled(disabled),
            )?;
            ui.child(
                "header",
                Text::new(hunk.header.as_ref()).style(muted_text()),
            )?;
            match layout {
                DiffLayout::Unified => {
                    for (line_index, line) in hunk.lines.iter().enumerate() {
                        mount_line(ui, hunk_index, line_index, line, disabled, true)?;
                    }
                }
                DiffLayout::Split => {
                    let mut row = NodeStyle::default();
                    {
                        let style = Arc::make_mut(&mut row.layout);
                        style.direction = Some(FlexDirection::Row);
                        style.gap = Some(LengthSpec::Px(8.0));
                    }
                    ui.with_child(
                        "split",
                        Stack::from_layout(row.layout.as_ref().clone()),
                        |ui| {
                            mount_split_column(ui, hunk_index, hunk, disabled, true)?;
                            mount_split_column(ui, hunk_index, hunk, disabled, false)?;
                            Ok(())
                        },
                    )?;
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}

fn mount_split_column(
    ui: &mut crate::framework::AssemblyScope<'_>,
    hunk_index: usize,
    hunk: &DiffHunk,
    disabled: bool,
    old_side: bool,
) -> Result<(), FrameworkError> {
    let mut column = NodeStyle::default();
    {
        let style = Arc::make_mut(&mut column.layout);
        style.direction = Some(FlexDirection::Column);
        style.flex_grow = Some(1.0);
        style.min_width = Some(LengthSpec::Px(0.0));
    }
    ui.with_child(
        if old_side { "old" } else { "new" },
        Stack::from_layout(column.layout.as_ref().clone()),
        |ui| {
            for (line_index, line) in hunk.lines.iter().enumerate() {
                let show = match line.kind {
                    DiffLineKind::Context => true,
                    DiffLineKind::Removed => old_side,
                    DiffLineKind::Added => !old_side,
                };
                if show {
                    let actions = match line.kind {
                        DiffLineKind::Context => false,
                        DiffLineKind::Removed => old_side,
                        DiffLineKind::Added => !old_side,
                    };
                    mount_line(ui, hunk_index, line_index, line, disabled, actions)?;
                } else if matches!(line.kind, DiffLineKind::Added | DiffLineKind::Removed) {
                    ui.child(
                        format!("pad-{line_index}"),
                        Text::new(" ").style(line_style(line.kind)),
                    )?;
                }
            }
            Ok(())
        },
    )?;
    Ok(())
}

fn mount_line(
    ui: &mut crate::framework::AssemblyScope<'_>,
    hunk_index: usize,
    line_index: usize,
    line: &DiffLine,
    disabled: bool,
    show_actions: bool,
) -> Result<(), FrameworkError> {
    let mut row = line_style(line.kind);
    {
        let style = Arc::make_mut(&mut row.layout);
        style.direction = Some(FlexDirection::Row);
        style.align_items = AlignSpec::Center;
        style.gap = Some(LengthSpec::Px(8.0));
        style.min_height = Some(LengthSpec::Px(20.0));
    }
    ui.with_child(
        format!("line-{line_index}"),
        Stack::from_layout(row.layout.as_ref().clone()),
        |ui| {
            let old = line.old_number.map(|n| n.to_string()).unwrap_or_default();
            let new = line.new_number.map(|n| n.to_string()).unwrap_or_default();
            ui.child("old-no", Text::new(old).style(muted_text()))?;
            ui.child("new-no", Text::new(new).style(muted_text()))?;
            let marker = match line.kind {
                DiffLineKind::Added => "+",
                DiffLineKind::Removed => "-",
                DiffLineKind::Context => " ",
            };
            ui.child("mark", Text::new(marker))?;
            ui.child("text", Text::new(line.text.as_ref()))?;
            if show_actions && !matches!(line.kind, DiffLineKind::Context) {
                ui.child(
                    "line-accept",
                    Button::new(format!("接受行{hunk_index}.{line_index}"))
                        .kind(ButtonKind::Text)
                        .size(ControlSize::Small)
                        .disabled(disabled),
                )?;
                ui.child(
                    "line-reject",
                    Button::new(format!("拒绝行{hunk_index}.{line_index}"))
                        .kind(ButtonKind::Text)
                        .size(ControlSize::Small)
                        .disabled(disabled),
                )?;
            }
            Ok(())
        },
    )?;
    Ok(())
}

fn line_style(kind: DiffLineKind) -> NodeStyle {
    match kind {
        DiffLineKind::Added => NodeStyle::default().surface(SemanticColorRole::Success),
        DiffLineKind::Removed => NodeStyle::default().surface(SemanticColorRole::Danger),
        DiffLineKind::Context => NodeStyle::default(),
    }
}

fn muted_text() -> NodeStyle {
    let mut style = NodeStyle::default();
    style.foreground = Some(SemanticColorRole::Muted);
    style
}

fn hunks_from_spec(spec: &SemanticSpec<'_>) -> Arc<[DiffHunk]> {
    let Some(raw) = spec.attr("hunks") else {
        return Arc::from([]);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Arc::from([]);
    };
    let Some(items) = value.as_array() else {
        return Arc::from([]);
    };
    items
        .iter()
        .filter_map(parse_hunk)
        .collect::<Vec<_>>()
        .into()
}

fn parse_hunk(value: &serde_json::Value) -> Option<DiffHunk> {
    let object = value.as_object()?;
    let header = object.get("header").and_then(|value| value.as_str())?;
    let lines = object
        .get("lines")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_line)
        .collect::<Vec<_>>();
    Some(DiffHunk::new(header, lines))
}

fn parse_line(value: &serde_json::Value) -> Option<DiffLine> {
    let object = value.as_object()?;
    let text = object.get("text").and_then(|value| value.as_str())?;
    let kind = match object
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("context")
    {
        "added" | "add" => DiffLineKind::Added,
        "removed" | "deleted" => DiffLineKind::Removed,
        _ => DiffLineKind::Context,
    };
    Some(DiffLine {
        old_number: object
            .get("old")
            .or_else(|| object.get("old_number"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        new_number: object
            .get("new")
            .or_else(|| object.get("new_number"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        kind,
        text: Arc::from(text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentId, LayoutViewport};
    use std::sync::{Arc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample_hunks() -> Arc<[DiffHunk]> {
        Arc::from([DiffHunk::new(
            "@@ -1,2 +1,2 @@",
            vec![
                DiffLine::context(1, 1, "fn main() {"),
                DiffLine::removed(2, "    let x = 1;"),
                DiffLine::added(2, "    let x = 2;"),
            ],
        )])
    }

    #[test]
    fn hunk_and_line_actions_emit_requests_without_mutating_hunks() {
        let mut context = AppContext::new();
        let diff = context
            .create_component(document(), DiffView::new(sample_hunks()))
            .unwrap();
        assert!(context.assemble_diff_view(diff).unwrap());
        context
            .layout_document(document(), LayoutViewport::new(480.0, 240.0))
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&events);
        context
            .on(diff, move |_, event: &DiffEvent, _| {
                log.lock().unwrap().push(event.clone());
            })
            .unwrap();

        let hunk_accept =
            find_labeled_button(&context, diff.stable_id(), "接受块0").expect("hunk accept");
        assert!(context.activate_node(hunk_accept).unwrap());
        let hunks_after = context.read(diff, |view| Arc::clone(&view.hunks)).unwrap();
        assert_eq!(hunks_after.len(), 1);
        let log = events.lock().unwrap();
        assert!(matches!(
            log.first(),
            Some(DiffEvent::HunkAccepted { hunk: 0 })
        ));
    }

    #[test]
    fn split_layout_shows_line_actions_on_the_added_column() {
        let mut context = AppContext::new();
        let diff = context
            .create_component(
                document(),
                DiffView::new(sample_hunks()).layout(DiffLayout::Split),
            )
            .unwrap();
        assert!(context.assemble_diff_view(diff).unwrap());
        assert!(
            find_labeled_button(&context, diff.stable_id(), "接受行0.2").is_some(),
            "added lines live on the new column and must keep accept/reject"
        );
        assert!(find_labeled_button(&context, diff.stable_id(), "接受行0.1").is_some());
        assert!(find_labeled_button(&context, diff.stable_id(), "接受行0.0").is_none());
    }
}
