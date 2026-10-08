//! Editor file tab: one open-document item for host-built tab strips.
//!
//! One tab = leading document glyph + name + unsaved dot + close button, on
//! the [`crate::ListItem`] row chrome (selected plate, hover/press states).
//! The name ellipsizes past [`FILE_TAB_MAX_WIDTH`]; the dot tracks [`FileTab::dirty`];
//! the close button honors [`FileTab::close_disabled`]. Body clicks emit
//! [`FileTabEvent::Activate`]; the close button emits [`FileTabEvent::Close`]
//! (close stays a request — the host owns removal). Assemble with
//! [`AppContext::assemble_file_tab`] after creating or mutating the tab: it
//! idempotently builds the internal children and syncs them to the current
//! fields.

use std::sync::Arc;

use nana_ui_core::{ButtonKind, ControlSize, Icon, LengthSpec, SemanticColorRole};

use crate::sidebar::SidebarRowIcon;
use crate::view_components::{Activate, IconButton, ListItem, ListItemSlots, Stack, Text};
use crate::{
    AppContext, ComponentView, Entity, FrameworkError, MutationQueue, NodeKind, NodeStyle,
    StableNodeId, UiWorld,
};

/// 单个文件 tab 的最大宽度:名字超出即省略号截断。
pub const FILE_TAB_MAX_WIDTH: f32 = 180.0;
pub const FILE_TAB_DOT_SIZE: f32 = nana_ui_core::space::MD;

/// Host-facing events for one file tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTabEvent {
    /// The tab body was activated (click / keyboard activation).
    Activate,
    /// The close button was pressed. Closing stays host-owned.
    Close,
}

/// One open-document tab (glyph + name + unsaved dot + close).
#[derive(Debug, Clone, PartialEq)]
pub struct FileTab {
    pub label: Arc<str>,
    pub icon: Icon,
    pub dirty: bool,
    pub selected: bool,
    pub close_disabled: bool,
    pub close_label: Arc<str>,
    pub max_width: f32,
    pub style: NodeStyle,
    pub(crate) leading: Option<StableNodeId>,
    pub(crate) name_row: Option<StableNodeId>,
    pub(crate) name: Option<StableNodeId>,
    pub(crate) dot: Option<StableNodeId>,
    pub(crate) close: Option<StableNodeId>,
}

impl FileTab {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            icon: Icon::File,
            dirty: false,
            selected: false,
            close_disabled: false,
            close_label: Arc::from("关闭文件"),
            max_width: FILE_TAB_MAX_WIDTH,
            style: ListItem::new(String::new()).style,
            leading: None,
            name_row: None,
            name: None,
            dot: None,
            close: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn close_disabled(mut self, close_disabled: bool) -> Self {
        self.close_disabled = close_disabled;
        self
    }

    pub fn close_label(mut self, close_label: impl Into<Arc<str>>) -> Self {
        self.close_label = close_label.into();
        self
    }

    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        layout.max_width = Some(LengthSpec::Px(self.max_width));
        style
    }
}

impl ComponentView for FileTab {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_file_tab),
        assembler: Some(crate::AppContext::assemble_file_tab),
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
            tag: "file-tab".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut item = ListItem::new(self.label.as_ref())
            .selected(self.selected)
            .slots(ListItemSlots {
                leading: self.leading,
                content: self.name_row,
                trailing: self.close,
            });
        item.style = self.effective_style();
        item.project(id, world, mutations);
    }
}

impl AppContext {
    /// Idempotently build and sync the tab's internal children. Call after
    /// creating the tab and after mutating its fields.
    pub fn assemble_file_tab(&mut self, tab: Entity<FileTab>) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(tab.stable_id())
            .ok_or(FrameworkError::MissingView(tab.stable_id()))?
            .document;
        let snapshot = self.read(tab, Clone::clone)?;
        let created = snapshot.leading.is_none();
        let leading = match snapshot.leading.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<SidebarRowIcon>::from_stable_id(id),
            None => self.create_detached_component(document, SidebarRowIcon::new(snapshot.icon))?,
        };
        let name_row = match snapshot.name_row.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<Stack>::from_stable_id(id),
            None => self.create_detached_component(document, Stack::row(4.0))?,
        };
        let name = match snapshot.name.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<Text>::from_stable_id(id),
            None => self.create_detached_component(document, Text::new(snapshot.label.as_ref()))?,
        };
        let dot = match snapshot.dot.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<Text>::from_stable_id(id),
            None => self.create_detached_component(document, unsaved_dot())?,
        };
        let close = match snapshot.close.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<IconButton>::from_stable_id(id),
            None => self.create_detached_component(
                document,
                IconButton::new(Icon::Close, snapshot.close_label.as_ref())
                    .size(ControlSize::Small)
                    .kind(ButtonKind::Text),
            )?,
        };

        if created {
            self.observe(close, tab, |tab, _: &Activate, cx| {
                if !tab.close_disabled {
                    cx.emit(FileTabEvent::Close);
                }
            })?;
        }

        self.update_component(leading, |icon, _| icon.icon = snapshot.icon)?;
        self.update_component(name, |text, _| {
            if text.value != snapshot.label.as_ref() {
                text.value = snapshot.label.to_string();
            }
            text.style.foreground = Some(if snapshot.selected {
                SemanticColorRole::Text
            } else {
                SemanticColorRole::Faint
            });
            let layout = Arc::make_mut(&mut text.style.layout);
            // 名字过长省略号截断:不换行 + 收缩 + 挂在 tab 的最大宽度下。
            layout.white_space_nowrap = true;
            layout.text_overflow_ellipsis = true;
            layout.min_width = Some(LengthSpec::Px(0.0));
            layout.flex_shrink = Some(1.0);
        })?;
        self.update_component(dot, |text, _| {
            Arc::make_mut(&mut text.style.layout).hidden = !snapshot.dirty;
        })?;
        self.update_component(close, |button, _| {
            button.disabled = snapshot.close_disabled;
            button.label = Arc::clone(&snapshot.close_label);
        })?;
        self.update_component(tab, |tab, _| {
            tab.leading = Some(leading.stable_id());
            tab.name_row = Some(name_row.stable_id());
            tab.name = Some(name.stable_id());
            tab.dot = Some(dot.stable_id());
            tab.close = Some(close.stable_id());
        })?;
        self.append_children(name_row.stable_id(), &[name.stable_id(), dot.stable_id()])?;
        self.append_children(
            tab.stable_id(),
            &[leading.stable_id(), name_row.stable_id(), close.stable_id()],
        )?;
        Ok(created)
    }

    pub fn activate_file_tab(&mut self, entity: Entity<FileTab>) -> Result<bool, FrameworkError> {
        self.update_component(entity, |_tab, cx| cx.emit(FileTabEvent::Activate))?;
        Ok(true)
    }
}

/// 未保存圆点:与名字同行的小号 Muted 圆点。
fn unsaved_dot() -> Text {
    let mut text = Text::new("●");
    text.style.foreground = Some(SemanticColorRole::Muted);
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.font_size = Some(FILE_TAB_DOT_SIZE);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn assembled_tab(context: &mut AppContext) -> (Entity<FileTab>, FileTab) {
        let document = DocumentId::new(1).unwrap();
        let tab = context
            .create_component(document, FileTab::new("未命名 1"))
            .unwrap();
        context.assemble_file_tab(tab).unwrap();
        let snapshot = context.read(tab, Clone::clone).unwrap();
        (tab, snapshot)
    }

    #[test]
    fn assemble_creates_children_once_and_syncs_them() {
        let mut context = AppContext::new();
        let (tab, snapshot) = assembled_tab(&mut context);
        assert!(
            snapshot
                .leading
                .is_some_and(|id| context.world().contains(id))
        );
        assert!(
            snapshot
                .name_row
                .is_some_and(|id| context.world().contains(id))
        );
        assert!(
            snapshot
                .close
                .is_some_and(|id| context.world().contains(id))
        );
        // name + dot live inside the name row, close trails on the tab.
        let name_row = context
            .world()
            .node(snapshot.name_row.unwrap())
            .map(|node| node.children.clone())
            .unwrap();
        assert_eq!(name_row.len(), 2, "name + dot: {name_row:?}");
        let tab_children = context
            .world()
            .node(tab.stable_id())
            .map(|node| node.children.clone())
            .unwrap();
        assert_eq!(
            tab_children,
            vec![
                snapshot.leading.unwrap(),
                snapshot.name_row.unwrap(),
                snapshot.close.unwrap()
            ],
            "icon / name row / close"
        );

        // Re-assembly reuses the same children (idempotent).
        assert!(!context.assemble_file_tab(tab).unwrap());
        let rebuilt = context.read(tab, Clone::clone).unwrap();
        assert_eq!(rebuilt.leading, snapshot.leading);
        assert_eq!(rebuilt.name_row, snapshot.name_row);
        assert_eq!(rebuilt.close, snapshot.close);
    }

    #[test]
    fn dirty_and_selected_flow_into_children() {
        let mut context = AppContext::new();
        let (tab, snapshot) = assembled_tab(&mut context);
        let name = Entity::<Text>::from_stable_id(snapshot.name.unwrap());
        let dot = Entity::<Text>::from_stable_id(snapshot.dot.unwrap());
        let close = Entity::<IconButton>::from_stable_id(snapshot.close.unwrap());

        let clean_dot_hidden = context.read(dot, |text| text.style.layout.hidden).unwrap();
        assert!(clean_dot_hidden, "干净文档不显示圆点");

        context
            .update_component(tab, |tab, _| {
                tab.dirty = true;
                tab.selected = true;
                tab.close_disabled = true;
                tab.label = Arc::from("效果.toyung");
            })
            .unwrap();
        context.assemble_file_tab(tab).unwrap();

        assert!(!context.read(dot, |text| text.style.layout.hidden).unwrap());
        let (value, foreground) = context
            .read(name, |text| (text.value.clone(), text.style.foreground))
            .unwrap();
        assert_eq!(value, "效果.toyung");
        assert_eq!(foreground, Some(SemanticColorRole::Text));
        assert!(context.read(close, |button| button.disabled).unwrap());
    }

    #[test]
    fn body_emits_activate_and_close_button_emits_close() {
        let mut context = AppContext::new();
        let (tab, snapshot) = assembled_tab(&mut context);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        context
            .on(tab, move |_, event: &FileTabEvent, _| {
                sink.lock().unwrap().push(*event);
            })
            .unwrap();

        context.activate_file_tab(tab).unwrap();
        let close = Entity::<IconButton>::from_stable_id(snapshot.close.unwrap());
        context.activate_icon_button(close).unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![FileTabEvent::Activate, FileTabEvent::Close],
            "tab body and close button emit distinct events"
        );

        // Disabled close is inert on both the button and the observed path.
        events.lock().unwrap().clear();
        context
            .update_component(tab, |tab, _| tab.close_disabled = true)
            .unwrap();
        context.assemble_file_tab(tab).unwrap();
        context.activate_icon_button(close).unwrap();
        assert!(
            events.lock().unwrap().is_empty(),
            "disabled close emits nothing"
        );
    }
}
