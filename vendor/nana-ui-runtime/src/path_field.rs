//! Path text field with a browse affordance.
//!
//! The control never opens a native dialog — a dialog needs the parent window
//! handle, which controls do not reach. It emits [`BrowseRequested`]; the
//! application answers by sending `WindowCommand::OpenFileDialog`, and writes
//! the chosen path back when the result arrives through
//! `nana_window::take_file_dialog_results`.

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, Icon, JustifySpec, LengthSpec, SemanticColorRole,
};

use crate::view_components::{IconButton, TextChanged, TextInput, project_common};
use crate::{
    AccessibilityRole, AccessibilityState, Activate, AppContext, ComponentView, Entity,
    FrameworkError, InteractionState, MutationQueue, NodeKind, NodeStyle, StableNodeId,
    TextContent, UiWorld,
};

/// Host should open a file or folder picker and assign the result.
/// Default accessible name. Applications localize it with
/// [`PathField::label`].
const DEFAULT_LABEL: &str = "路径";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowseRequested;

/// Path string plus a trailing browse button.
#[derive(Debug, Clone, PartialEq)]
pub struct PathField {
    pub value: Arc<str>,
    pub placeholder: Arc<str>,
    pub disabled: bool,
    pub invalid: bool,
    pub size: ControlSize,
    /// Accessible name announced for the field.
    pub label: Arc<str>,
    pub input: Option<StableNodeId>,
    pub browse: Option<StableNodeId>,
    pub style: NodeStyle,
}

impl PathField {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    /// Overrides the accessible name announced for the field.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }

    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            placeholder: Arc::from(""),
            disabled: false,
            invalid: false,
            size: ControlSize::Medium,
            label: Arc::from(DEFAULT_LABEL),
            input: None,
            browse: None,
            style: field_style(ControlSize::Medium),
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self.style = field_style(size);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        if self.invalid {
            style.border = Some(SemanticColorRole::Danger);
        }
        {
            let layout = std::sync::Arc::make_mut(&mut style.layout);
            layout.direction = Some(FlexDirection::Row);
            layout.align_items = AlignSpec::Center;
            layout.justify_content = JustifySpec::Start;
            layout.gap = Some(LengthSpec::Px(4.0));
            layout.width = Some(LengthSpec::Fill);
        }
        style.control_height = Some(nana_ui_core::ControlHeight::Min(self.size));
        style
    }
}

impl ComponentView for PathField {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        assembler: Some(crate::AppContext::assemble_path_field),
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
            tag: "path-field".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        if world.text(id) != Some("") {
            mutations.set_text(
                id,
                TextContent {
                    value: String::new().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: Some(Arc::clone(&self.label)),
                value: Some(Arc::clone(&self.value)),
                disabled: self.disabled,
                invalid: self.invalid,
                ..AccessibilityState::default()
            },
        );
    }
}

impl AppContext {
    /// Create or refresh the text field and browse button for `field`.
    pub fn assemble_path_field(
        &mut self,
        field: Entity<PathField>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(field.stable_id())
            .ok_or(FrameworkError::MissingView(field.stable_id()))?
            .document;
        let snapshot = self.read(field, Clone::clone)?;
        let created = snapshot.input.is_none();
        let input = match snapshot.input.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<TextInput>::from_stable_id(id),
            None => {
                let mut input = TextInput::new(snapshot.value.to_string()).size(snapshot.size);
                input.placeholder = Arc::clone(&snapshot.placeholder);
                self.create_detached_component(document, input)?
            }
        };
        let browse = match snapshot.browse.filter(|id| self.world().contains(*id)) {
            Some(id) => Entity::<IconButton>::from_stable_id(id),
            None => self.create_detached_component(
                document,
                IconButton::new(Icon::Folder, "浏览").size(snapshot.size),
            )?,
        };

        if created {
            self.observe(input, field, |field, event: &TextChanged, cx| {
                field.value = Arc::from(event.value.as_str());
                cx.emit(event.clone());
            })?;
            self.observe(browse, field, |field, _: &Activate, cx| {
                if !field.disabled {
                    cx.emit(BrowseRequested);
                }
            })?;
        }

        self.update_component(input, |input, _| {
            if input.state.value != field_value(&snapshot) {
                input.state.replace_value(snapshot.value.to_string());
            }
            input.placeholder = Arc::clone(&snapshot.placeholder);
            input.disabled = snapshot.disabled;
            input.invalid = snapshot.invalid;
            let layout = std::sync::Arc::make_mut(&mut input.style.layout);
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(1.0);
            layout.min_width = Some(LengthSpec::Px(0.0));
        })?;
        self.update_component(browse, |button, _| {
            button.disabled = snapshot.disabled;
        })?;
        self.update_component(field, |field, _| {
            field.input = Some(input.stable_id());
            field.browse = Some(browse.stable_id());
        })?;
        self.append_children(field.stable_id(), &[input.stable_id(), browse.stable_id()])?;
        Ok(created)
    }

    pub fn set_path_field_value(
        &mut self,
        field: Entity<PathField>,
        value: impl Into<Arc<str>>,
    ) -> Result<bool, FrameworkError> {
        let value = value.into();
        let changed = self.update_component(field, |field, _| {
            if field.value == value {
                return false;
            }
            field.value = Arc::clone(&value);
            true
        })?;
        if changed {
            self.assemble_path_field(field)?;
        }
        Ok(changed)
    }
}

fn field_value(field: &PathField) -> &str {
    field.value.as_ref()
}

fn field_style(size: ControlSize) -> NodeStyle {
    let mut style = NodeStyle::default();
    {
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Fill);
    }
    style.control_height = Some(nana_ui_core::ControlHeight::Min(size));
    style.radius = Some(nana_ui_core::RadiusTier::Sm);
    style
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_accessible_name_is_localizable_and_defaults_to_the_bundled_string() {
        use crate::{AppContext, DocumentId};
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();

        let default_field = context
            .create_component(document, PathField::new("/tmp"))
            .unwrap();
        assert_eq!(
            context
                .world()
                .accessibility(default_field.stable_id())
                .and_then(|state| state.label.clone())
                .as_deref(),
            Some(DEFAULT_LABEL)
        );

        let localized = context
            .create_component(document, PathField::new("/tmp").label("Path"))
            .unwrap();
        assert_eq!(
            context
                .world()
                .accessibility(localized.stable_id())
                .and_then(|state| state.label.clone())
                .as_deref(),
            Some("Path")
        );
    }

    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    #[test]
    fn assemble_creates_input_and_browse() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let field = context
            .create_component(document, PathField::new("C:\\models\\nana.model3.json"))
            .unwrap();
        assert!(context.assemble_path_field(field).unwrap());
        let snapshot = context.read(field, Clone::clone).unwrap();
        assert!(snapshot.input.is_some());
        assert!(snapshot.browse.is_some());
        assert!(!context.assemble_path_field(field).unwrap());
    }
}
