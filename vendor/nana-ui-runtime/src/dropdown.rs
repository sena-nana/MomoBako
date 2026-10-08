//! Single- and multiple-value field sharing the Select surface and menu.

use std::sync::Arc;

use nana_ui_core::{ControlSize, DropdownEvent, DropdownSelection};

use crate::select::{menu_option_label, select_option_at};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, MutationQueue,
    NodeKind, NodeStyle, SelectOptionData, SemanticPaint, StableNodeId, StandardVisual,
    TextContent, UiWorld,
};

/// Option identity stays application-owned. Disabled options remain visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropdownOption {
    pub value: Arc<str>,
    pub label: Arc<str>,
    pub hint: Option<Arc<str>>,
    pub disabled: bool,
}

impl DropdownOption {
    pub fn new(value: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            hint: None,
            disabled: false,
        }
    }

    pub fn hint(mut self, hint: impl Into<Arc<str>>) -> Self {
        let hint = hint.into();
        self.hint = (!hint.is_empty()).then_some(hint);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn menu_label(&self) -> Arc<str> {
        menu_option_label(&SelectOptionData {
            label: Arc::clone(&self.label),
            hint: self.hint.clone(),
            disabled: self.disabled,
            checked: false,
            icon: None,
        })
    }
}

/// Single- or multiple-value field. Disabled options stay visible.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropdown {
    pub selection: DropdownSelection<Arc<str>>,
    pub options: Vec<DropdownOption>,
    pub placeholder: Option<Arc<str>>,
    pub size: ControlSize,
    pub disabled: bool,
    pub loading: bool,
    pub invalid: bool,
    pub opened: bool,
    pub highlighted: Option<usize>,
    pub style: NodeStyle,
}

impl Dropdown {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn single(value: Option<impl Into<Arc<str>>>) -> Self {
        Self::new(DropdownSelection::Single(value.map(Into::into)))
    }

    pub fn multiple(values: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        Self::new(DropdownSelection::Multiple(
            values.into_iter().map(Into::into).collect(),
        ))
    }

    fn new(selection: DropdownSelection<Arc<str>>) -> Self {
        Self {
            selection,
            options: Vec::new(),
            placeholder: None,
            size: ControlSize::Medium,
            disabled: false,
            loading: false,
            invalid: false,
            opened: false,
            highlighted: None,
            style: crate::select::field_style_for_size(ControlSize::Medium),
        }
    }

    pub fn options(mut self, options: impl IntoIterator<Item = DropdownOption>) -> Self {
        self.options = options.into_iter().collect();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        let placeholder = placeholder.into();
        self.placeholder = (!placeholder.is_empty()).then_some(placeholder);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        crate::select::apply_field_size(&mut self.style, size);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn opened(mut self, opened: bool) -> Self {
        self.opened = opened;
        if opened {
            self.highlighted = self.selected_index().or_else(|| self.first_enabled());
        } else {
            self.highlighted = None;
        }
        self
    }

    pub fn inactive(&self) -> bool {
        self.disabled || self.loading
    }

    pub fn is_multiple(&self) -> bool {
        matches!(self.selection, DropdownSelection::Multiple(_))
    }

    pub fn selected_index(&self) -> Option<usize> {
        match &self.selection {
            DropdownSelection::Single(value) => {
                let value = value.as_ref()?;
                self.options
                    .iter()
                    .position(|option| &option.value == value)
            }
            DropdownSelection::Multiple(values) => values.first().and_then(|value| {
                self.options
                    .iter()
                    .position(|option| &option.value == value)
            }),
        }
    }

    pub fn display_label(&self) -> (Arc<str>, bool) {
        match &self.selection {
            DropdownSelection::Single(value) => {
                if let Some(index) = value.as_ref().and_then(|value| {
                    self.options
                        .iter()
                        .position(|option| &option.value == value)
                }) {
                    return (self.options[index].menu_label(), false);
                }
            }
            DropdownSelection::Multiple(values) => {
                if !values.is_empty() {
                    return (Arc::from(multiple_label(values, &self.options)), false);
                }
            }
        }
        (
            self.placeholder.clone().unwrap_or_else(|| Arc::from("")),
            true,
        )
    }

    pub fn toggle_open(&mut self) -> Option<DropdownEvent<Arc<str>>> {
        if self.inactive() {
            return None;
        }
        self.opened = !self.opened;
        if self.opened {
            self.highlighted = self.selected_index().or_else(|| self.first_enabled());
            Some(DropdownEvent::Opened)
        } else {
            self.highlighted = None;
            Some(DropdownEvent::Closed)
        }
    }

    pub fn close(&mut self) -> Option<DropdownEvent<Arc<str>>> {
        if !self.opened {
            return None;
        }
        self.opened = false;
        self.highlighted = None;
        Some(DropdownEvent::Closed)
    }

    pub fn highlight_delta(&mut self, delta: i32) -> bool {
        if self.inactive() || !self.opened || self.options.is_empty() {
            return false;
        }
        let enabled = self
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| !option.disabled)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            return false;
        }
        let current = self
            .highlighted
            .and_then(|index| enabled.iter().position(|candidate| *candidate == index));
        let next = match current {
            Some(index) => {
                let len = enabled.len() as i32;
                enabled[((index as i32 + delta).rem_euclid(len)) as usize]
            }
            None if delta >= 0 => enabled[0],
            None => *enabled.last().expect("enabled is non-empty"),
        };
        if self.highlighted == Some(next) {
            return false;
        }
        self.highlighted = Some(next);
        true
    }

    pub fn commit_highlighted(&mut self) -> Option<DropdownEvent<Arc<str>>> {
        let index = self.highlighted?;
        self.select_index(index)
    }

    pub fn select_index(&mut self, index: usize) -> Option<DropdownEvent<Arc<str>>> {
        let option = self.options.get(index)?;
        if option.disabled || self.inactive() {
            return None;
        }
        let value = Arc::clone(&option.value);
        match &mut self.selection {
            DropdownSelection::Single(current) => {
                *current = Some(Arc::clone(&value));
                let _ = self.close();
                Some(DropdownEvent::Select(value))
            }
            DropdownSelection::Multiple(values) => {
                if let Some(position) = values.iter().position(|item| item == &value) {
                    values.remove(position);
                } else {
                    values.push(Arc::clone(&value));
                }
                Some(DropdownEvent::Toggle(value))
            }
        }
    }

    fn first_enabled(&self) -> Option<usize> {
        self.options.iter().position(|option| !option.disabled)
    }

    fn option_data(&self) -> Vec<SelectOptionData> {
        self.options
            .iter()
            .map(|option| SelectOptionData {
                label: Arc::clone(&option.label),
                hint: option.hint.clone(),
                disabled: option.disabled,
                checked: match &self.selection {
                    DropdownSelection::Multiple(values) => {
                        values.iter().any(|value| value == &option.value)
                    }
                    DropdownSelection::Single(_) => false,
                },
                icon: None,
            })
            .collect()
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        let (_, placeholder) = self.display_label();
        style.foreground = Some(if self.inactive() || placeholder {
            nana_ui_core::SemanticColorRole::Faint
        } else {
            nana_ui_core::SemanticColorRole::Text
        });
        style.background = Some(if self.inactive() {
            nana_ui_core::SemanticColorRole::Subtle
        } else {
            nana_ui_core::SemanticColorRole::Background
        });
        style.border = Some(if self.invalid {
            nana_ui_core::SemanticColorRole::Danger
        } else if self.opened {
            nana_ui_core::SemanticColorRole::BorderSoft
        } else {
            nana_ui_core::SemanticColorRole::Border
        });
        style.interaction.hovered.border = Some(if self.invalid {
            nana_ui_core::SemanticColorRole::Danger
        } else {
            nana_ui_core::SemanticColorRole::BorderStrong
        });
        style.interaction.focused.border = Some(if self.invalid {
            nana_ui_core::SemanticColorRole::Danger
        } else {
            nana_ui_core::SemanticColorRole::FocusBorder
        });
        style.interaction.disabled = SemanticPaint {
            foreground: Some(nana_ui_core::SemanticColorRole::Faint),
            background: Some(nana_ui_core::SemanticColorRole::Subtle),
            border: Some(nana_ui_core::SemanticColorRole::Border),
            ..SemanticPaint::default()
        };
        let has_explicit_height = style.layout.height.is_some();
        {
            let layout = Arc::make_mut(&mut style.layout);
            if layout.width.is_none() {
                layout.width = Some(nana_ui_core::LengthSpec::Fill);
            }
            layout.border_width = Some(if self.invalid && self.opened {
                2.0
            } else {
                1.0
            });
        }
        if has_explicit_height {
            style.control_height = None;
        } else if style.control_height.is_none() {
            style.control_height = Some(nana_ui_core::ControlHeight::Exact(self.size));
        }
        style
    }
}

impl ComponentView for Dropdown {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::toggle_dropdown),
        activate_at: Some(crate::AppContext::activate_dropdown_at),
        close_options: Some(|context, entity| {
            context.update_component(entity, |dropdown, cx| {
                if let Some(event) = dropdown.close() {
                    cx.emit(event);
                    true
                } else {
                    false
                }
            })
        }),
        hooks: Some(|hooks| hooks.dropdown = Some(&DROPDOWN_HOOKS)),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn reconcile(&mut self, mut next: Self) {
        // Selection and options are the application's; the open menu and its
        // keyboard highlight belong to the interaction in flight, and survive a
        // refresh that did not touch those props.
        if next.inactive() {
            next.opened = false;
            next.highlighted = None;
        } else if self.selection == next.selection && self.options == next.options {
            next.opened = self.opened;
            next.highlighted = self.highlighted;
        }
        *self = next;
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "dropdown".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let (label, placeholder) = self.display_label();
        let visual = StandardVisual::Select {
            label: Arc::clone(&label),
            placeholder,
            size: self.size,
            opened: self.opened && !self.inactive(),
            invalid: self.invalid,
            loading: self.loading,
            options: self.option_data().into(),
            highlighted: self.highlighted,
            checkable: matches!(self.selection, DropdownSelection::Multiple(_)),
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        if world.text(id) != Some(label.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: label.to_string().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: !self.inactive(),
                focusable: !self.inactive(),
            },
            AccessibilityState {
                role: AccessibilityRole::ComboBox,
                label: Some(label),
                value: match &self.selection {
                    DropdownSelection::Single(value) => value.clone(),
                    DropdownSelection::Multiple(values) => values
                        .first()
                        .cloned()
                        .filter(|_| !values.is_empty())
                        .map(|_| Arc::from(multiple_label(values, &self.options))),
                },
                disabled: self.inactive(),
                busy: self.loading,
                invalid: self.invalid,
                selected: Some(self.opened && !self.inactive()),
                ..AccessibilityState::default()
            },
        );
    }
}

pub(crate) fn activate_dropdown_at(
    dropdown: &mut Dropdown,
    menu: Option<&crate::SelectMenuGeometry>,
    field: crate::LayoutBox,
    x: f32,
    y: f32,
) -> Option<DropdownEvent<Arc<str>>> {
    if dropdown.opened {
        if let Some(menu) = menu
            && let Some(index) = select_option_at(menu, x, y)
        {
            return dropdown.select_index(index);
        }
        if field.contains(x, y) {
            return dropdown.toggle_open();
        }
        return dropdown.close();
    }
    if field.contains(x, y) {
        dropdown.toggle_open()
    } else {
        None
    }
}

fn multiple_label(values: &[Arc<str>], options: &[DropdownOption]) -> String {
    let labels = values
        .iter()
        .filter_map(|value| {
            options
                .iter()
                .find(|option| &option.value == value)
                .map(|option| option.label.as_ref())
        })
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [] => String::new(),
        [first] => (*first).to_owned(),
        [first, second] => format!("{first}, {second}"),
        [first, second, rest @ ..] => format!("{first}, {second} +{}", rest.len()),
    }
}

/// The router reaches this list's keyboard only once one exists.
pub(crate) static DROPDOWN_HOOKS: crate::framework::ChoiceHooks = crate::framework::ChoiceHooks {
    adjust: crate::AppContext::adjust_focused_dropdown,
    commit: crate::AppContext::commit_focused_dropdown,
};

#[cfg(test)]
mod tests {
    #[test]
    fn refreshing_props_keeps_an_open_menu_and_its_highlight() {
        let options = [DropdownOption::new("a", "A"), DropdownOption::new("b", "B")];
        let mut dropdown = Dropdown::single(Some("a"))
            .options(options.clone())
            .opened(true);
        dropdown.highlighted = Some(1);

        // The shape an application refresh takes: rebuild from state.
        crate::ComponentView::reconcile(
            &mut dropdown,
            Dropdown::single(Some("a"))
                .options(options.clone())
                .invalid(true),
        );
        assert!(
            dropdown.opened,
            "an unrelated prop change keeps the menu open"
        );
        assert_eq!(dropdown.highlighted, Some(1));

        // A real selection change does replace the interaction.
        crate::ComponentView::reconcile(
            &mut dropdown,
            Dropdown::single(Some("b")).options(options),
        );
        assert!(!dropdown.opened);
        assert_eq!(dropdown.highlighted, None);
    }

    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample() -> Dropdown {
        Dropdown::multiple(["0", "100"])
            .placeholder("强度")
            .options([
                DropdownOption::new("0", "关闭"),
                DropdownOption::new("50", "平衡").disabled(true),
                DropdownOption::new("100", "最大").hint("峰值"),
            ])
    }

    #[test]
    fn multiple_dropdown_keeps_disabled_options_and_summarizes_labels() {
        let mut context = AppContext::new();
        let dropdown = context.create_component(document(), sample()).unwrap();
        let id = dropdown.stable_id();
        assert_eq!(context.world().text(id), Some("关闭, 最大"));
        let Some(StandardVisual::Select { options, .. }) = context.world().standard_visual(id)
        else {
            panic!("dropdown visual");
        };
        assert_eq!(options.len(), 3);
        assert!(options[1].disabled);
        assert!(options[0].checked);
        assert!(!options[1].checked);
        assert!(options[2].checked);
        assert_eq!(options[2].hint.as_deref(), Some("峰值"));
    }

    #[test]
    fn dropdown_projects_overlay_css_size_into_world_style() {
        let mut dropdown = sample();
        Arc::make_mut(&mut dropdown.style.layout).overlay_css_size_overrides(
            &nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(48.0)),
                width: Some(nana_ui_core::LengthSpec::Px(120.0)),
                ..nana_ui_core::LayoutStyle::default()
            },
        );
        let mut context = AppContext::new();
        let dropdown = context.create_component(document(), dropdown).unwrap();
        let layout = context
            .world()
            .node_style(dropdown.stable_id())
            .unwrap()
            .layout
            .as_ref();
        assert_eq!(layout.height, Some(nana_ui_core::LengthSpec::Px(48.0)));
        assert_eq!(layout.width, Some(nana_ui_core::LengthSpec::Px(120.0)));
        assert_ne!(
            layout.height,
            Some(nana_ui_core::LengthSpec::Px(
                nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
            ))
        );
    }

    #[test]
    fn toggle_does_not_close_a_multiple_menu_and_skips_disabled() {
        let mut dropdown = sample().opened(true);
        assert!(dropdown.opened);
        assert!(dropdown.select_index(1).is_none());
        let event = dropdown.select_index(0).expect("toggle 关闭");
        assert_eq!(event, DropdownEvent::Toggle(Arc::from("0")));
        assert!(dropdown.opened);
        match &dropdown.selection {
            DropdownSelection::Multiple(values) => {
                assert_eq!(values.as_slice(), [Arc::<str>::from("100")]);
            }
            DropdownSelection::Single(_) => panic!("multiple"),
        }
    }

    #[test]
    fn single_dropdown_selects_and_closes() {
        let mut dropdown = Dropdown::single(None::<&str>)
            .options([
                DropdownOption::new("code", "Code"),
                DropdownOption::new("preview", "Preview"),
            ])
            .opened(true);
        let event = dropdown.select_index(1).expect("select");
        assert_eq!(event, DropdownEvent::Select(Arc::from("preview")));
        assert!(!dropdown.opened);
    }
}
