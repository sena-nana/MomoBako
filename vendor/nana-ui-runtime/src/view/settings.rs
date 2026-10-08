//! A settings row as a view: its label, hint and grouping are bindable, and
//! its control is a slot.
//!
//! ```ignore
//! settings_row("Mipmap")
//!     .hint("远处贴图用更小的层级")
//!     .divided(true)
//!     .control(switch("").checked(mipmaps).on_change(|e| set_mipmaps(e.checked)))
//! ```
//!
//! The row builds its copy (label over hint) and orders its children itself
//! (`TypeBehavior::slot_assembler`), as [`AppContext::mount_settings_leaf_row`]
//! does for a row built by hand; a view never calls
//! [`AppContext::assemble_settings_row`].
//!
//! [`AppContext::mount_settings_leaf_row`]: crate::AppContext::mount_settings_leaf_row
//! [`AppContext::assemble_settings_row`]: crate::AppContext::assemble_settings_row

use std::sync::Arc;

use super::controls::StyledComponent;
use super::node::{El, IntoView, widget};
use super::prop::{FieldWrite, IntoProp};
use crate::{NodeStyle, SettingsCollapsibleCard, SettingsPage, SettingsRow};

/// A row labelled `label`, laid out as the rows the framework assembles:
/// its control goes under its copy once the row is narrower than they fit.
#[track_caller]
pub fn settings_row(label: impl IntoProp<Arc<str>>) -> El<SettingsRow> {
    widget(SettingsRow::new("").stack_below(crate::settings::ASSEMBLED_ROW_STACK_BELOW))
        .label(label)
}

impl StyledComponent for SettingsRow {
    fn node_style(&self) -> &NodeStyle {
        &self.style
    }

    fn node_style_mut(&mut self) -> &mut NodeStyle {
        &mut self.style
    }
}

/// Bindable fields of [`SettingsRow`].
#[allow(non_camel_case_types)]
#[doc(hidden)]
pub mod settings_row {
    pub struct label;
    pub struct hint;
    pub struct divided;
    pub struct stacked;
    pub struct first_in_group;
    pub struct last_in_group;
}

macro_rules! row_fields {
    ($($field:ident: $ty:ty),* $(,)?) => {
        $(
            impl FieldWrite<SettingsRow, $ty> for settings_row::$field {
                const FIELD: &'static str = concat!("SettingsRow.", stringify!($field));

                fn write(target: &mut SettingsRow, value: $ty) {
                    target.$field = value;
                }

                fn differs(target: &SettingsRow, value: &$ty) -> bool {
                    target.$field != *value
                }
            }
        )*

        impl<K> El<SettingsRow, K> {
            $(
                #[track_caller]
                pub fn $field(self, value: impl IntoProp<$ty>) -> Self {
                    self.prop::<$ty, settings_row::$field>(value)
                }
            )*
        }
    };
}

row_fields! {
    label: Arc<str>,
    hint: Option<Arc<str>>,
    divided: bool,
    stacked: bool,
    first_in_group: bool,
    last_in_group: bool,
}

impl<K> El<SettingsCollapsibleCard, K> {
    /// What the card shows in its header, collapsed or not.
    pub fn summary(self, view: impl IntoView) -> Self {
        self.slot(view, SettingsCollapsibleCard::summary)
    }

    /// What it shows while expanded.
    pub fn details(self, view: impl IntoView) -> Self {
        self.slot(view, SettingsCollapsibleCard::details)
    }

    /// A control at the end of the header.
    pub fn accessory(self, view: impl IntoView) -> Self {
        self.slot(view, SettingsCollapsibleCard::accessory)
    }
}

impl<K> El<SettingsPage, K> {
    /// The page's content, under its header in its scroll area.
    pub fn content(self, view: impl IntoView) -> Self {
        self.slot(view, SettingsPage::content)
    }
}

impl<K> El<SettingsRow, K> {
    /// The row's control: built before the row, placed after its copy.
    pub fn control(self, view: impl IntoView) -> Self {
        self.slot(view, SettingsRow::control_child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{signal, switch};
    use crate::{AppContext, DocumentId, Entity, StableNodeId, Stack, Switch};

    fn children(cx: &AppContext, id: StableNodeId) -> Vec<StableNodeId> {
        cx.world()
            .node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default()
    }

    /// What the world shows at `id`: a row projects its copy straight into
    /// the world.
    fn text(cx: &AppContext, id: StableNodeId) -> String {
        cx.world().text(id).unwrap_or_default().to_owned()
    }

    /// A row from a view has the shape of one built by hand: its copy (label
    /// over hint), then its control, and the row points at all three.
    #[test]
    fn a_view_row_is_assembled_like_a_leaf_row() {
        let document = DocumentId::new(1).unwrap();
        let mut cx = AppContext::new();
        let root = cx.create_component(document, Stack::column(0.0)).unwrap();
        let label = signal(Arc::<str>::from("提高 GPU 优先级"));
        let hint = signal(Some(Arc::<str>::from("优先把渲染交给高性能 GPU")));
        cx.mount_view(root.stable_id(), move || {
            settings_row(label)
                .hint(hint)
                .divided(true)
                .control(switch("").checked(true))
        })
        .unwrap();
        cx.flush_reactive().unwrap();

        let row = children(&cx, root.stable_id())[0];
        let entity = Entity::<SettingsRow>::from_stable_id(row);
        let (control, label_slot, hint_slot, copy) = cx
            .read(entity, |row| {
                (row.control, row.label_slot, row.hint_slot, row.copy_slot)
            })
            .unwrap();
        let (control, label_slot, hint_slot, copy) = (
            control.unwrap(),
            label_slot.unwrap(),
            hint_slot.unwrap(),
            copy.unwrap(),
        );
        assert_eq!(children(&cx, row), [copy, control]);
        assert_eq!(children(&cx, copy), [label_slot, hint_slot]);
        assert!(
            cx.read(Entity::<Switch>::from_stable_id(control), |s| s.checked)
                .unwrap()
        );
        assert_eq!(text(&cx, label_slot), "提高 GPU 优先级");
        assert_eq!(text(&cx, hint_slot), "优先把渲染交给高性能 GPU");

        // The copy follows the row's bindings; the nodes stay.
        label.set(Arc::from("HDR"));
        hint.set(None);
        cx.flush_reactive().unwrap();
        assert_eq!(children(&cx, row), [copy, control]);
        assert_eq!(text(&cx, label_slot), "HDR");
        assert!(
            cx.world().node_style(hint_slot).unwrap().layout.hidden,
            "a row without a hint keeps no empty line"
        );
    }

    /// The row's label names the control it holds for assistive technology,
    /// and follows the row's label; a control with a label of its own keeps
    /// it, and a select is named by its row, not by the option it shows.
    #[test]
    fn a_row_label_names_the_control_it_holds() {
        use crate::view::{IntoView, column, select};
        let document = DocumentId::new(1).unwrap();
        let mut cx = AppContext::new();
        let label = signal(Arc::<str>::from("深色模式"));
        let view = cx
            .mount_view_root(document, move || {
                column()
                    .children((
                        settings_row(label).control(switch("")),
                        settings_row("通知").control(switch("静音")),
                        settings_row("字号").control(
                            select()
                                .options(vec![crate::SelectOption::new("m", "中")])
                                .value(Some(Arc::from("m"))),
                        ),
                    ))
                    .into_any()
            })
            .unwrap();
        cx.layout_document(document, crate::LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let rows = children(&cx, view.roots()[0]);
        let control = |cx: &AppContext, row: StableNodeId| {
            cx.read(Entity::<SettingsRow>::from_stable_id(row), |row| {
                row.control.unwrap()
            })
            .unwrap()
        };
        let name = |cx: &AppContext, id: StableNodeId| {
            cx.world()
                .project_accessibility(document)
                .into_iter()
                .find(|node| node.id == id)
                .and_then(|node| node.label.map(|label| label.to_string()))
        };
        let (dark, mute, size) = (
            control(&cx, rows[0]),
            control(&cx, rows[1]),
            control(&cx, rows[2]),
        );
        assert_eq!(name(&cx, dark).as_deref(), Some("深色模式"));
        assert_eq!(name(&cx, mute).as_deref(), Some("静音"));
        assert_eq!(name(&cx, size).as_deref(), Some("字号"));

        cx.take_system_work();
        label.set(Arc::from("HDR"));
        cx.flush_reactive().unwrap();
        let work = cx.take_system_work();
        let delta = cx.world().project_accessibility_delta(&work);
        let renamed = delta.updated.iter().find(|node| node.id == dark);
        assert_eq!(
            renamed.and_then(|node| node.label.as_deref()),
            Some("HDR"),
            "the control is projected again when the label it is named by changes"
        );
    }

    #[test]
    fn a_view_row_matches_a_leaf_row_node_for_node() {
        let document = DocumentId::new(1).unwrap();
        let mut built = AppContext::new();
        let root = built
            .create_component(document, Stack::column(0.0))
            .unwrap();
        let control = built
            .create_detached_component(document, Switch::new("", false))
            .unwrap();
        let row = built
            .mount_settings_leaf_row(document, "Mipmap", Some("远处贴图"), control.stable_id())
            .unwrap();
        built.append_child(root, row).unwrap();

        let mut viewed = AppContext::new();
        let view_root = viewed
            .create_component(document, Stack::column(0.0))
            .unwrap();
        viewed
            .mount_view(view_root.stable_id(), || {
                settings_row("Mipmap")
                    .hint("远处贴图")
                    .first_in_group(true)
                    .last_in_group(true)
                    .control(switch(""))
            })
            .unwrap();
        viewed.flush_reactive().unwrap();

        fn shape(cx: &AppContext, id: StableNodeId) -> Vec<(usize, String, String)> {
            let mut out = Vec::new();
            let mut stack = vec![(id, 0)];
            while let Some((id, depth)) = stack.pop() {
                let node = cx.world().node(id).unwrap();
                out.push((
                    depth,
                    format!("{:?}", node.kind),
                    cx.world().text(id).unwrap_or_default().to_owned(),
                ));
                for child in node.children.iter().rev() {
                    stack.push((*child, depth + 1));
                }
            }
            out
        }
        let built_row = children(&built, root.stable_id())[0];
        let viewed_row = children(&viewed, view_root.stable_id())[0];
        assert_eq!(shape(&built, built_row), shape(&viewed, viewed_row));
        let fields = |cx: &AppContext, row: StableNodeId| {
            cx.read(Entity::<SettingsRow>::from_stable_id(row), |row| {
                (
                    row.label.clone(),
                    row.hint.clone(),
                    row.stack_below,
                    row.first_in_group,
                    row.last_in_group,
                    row.divided,
                )
            })
            .unwrap()
        };
        assert_eq!(fields(&built, built_row), fields(&viewed, viewed_row));
    }
}
