use std::sync::Arc;

use nana_ui_core::{ControlSize, LengthSpec, SemanticColorRole, SemanticPalette};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentElevation, ComponentGeometry,
    ComponentTextRegion, ComputedStyle, InteractionState, InteractionStyle, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, SelectOptionData, SemanticPaint, StableNodeId,
    StandardVisual, TextContent, TextVerticalAlignment, UiWorld,
};

const HANDLE_WIDTH: f32 = nana_ui_core::type_scale::LINE;
const MENU_GAP: f32 = 0.0;
const MENU_PAD: f32 = crate::popover::MENU_SURFACE_PADDING;
use crate::popover::MENU_ITEM_GAP;
/// Width of the leading check lane in a drop-down menu row.
const MENU_CHECK_RESERVE: f32 = nana_ui_core::type_scale::LINE;

/// Option identity stays application-owned. Disabled options remain visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub value: Arc<str>,
    pub label: Arc<str>,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Committed selection from pointer, keyboard or accessibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectChanged {
    pub value: Arc<str>,
}

/// Single-value field. Disabled options stay visible in the opened menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    pub value: Option<Arc<str>>,
    pub options: Vec<SelectOption>,
    pub placeholder: Option<Arc<str>>,
    pub size: ControlSize,
    pub disabled: bool,
    pub loading: bool,
    pub invalid: bool,
    pub opened: bool,
    pub highlighted: Option<usize>,
    pub style: NodeStyle,
}

impl Select {
    /// Replaces the node style wholesale.
    ///
    /// Builders that derive layout from other props (such as `size`) overwrite
    /// only the fields they own, so call those after this one.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
    pub fn new(value: Option<impl Into<Arc<str>>>) -> Self {
        Self {
            value: value.map(Into::into),
            options: Vec::new(),
            placeholder: None,
            size: ControlSize::Medium,
            disabled: false,
            loading: false,
            invalid: false,
            opened: false,
            highlighted: None,
            style: field_style_for_size(ControlSize::Medium),
        }
    }

    pub fn options(mut self, options: impl IntoIterator<Item = SelectOption>) -> Self {
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
        apply_field_size(&mut self.style, size);
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

    pub fn selected_index(&self) -> Option<usize> {
        let value = self.value.as_ref()?;
        self.options
            .iter()
            .position(|option| &option.value == value)
    }

    pub fn display_label(&self) -> (Arc<str>, bool) {
        if let Some(index) = self.selected_index() {
            return (Arc::clone(&self.options[index].label), false);
        }
        (
            self.placeholder.clone().unwrap_or_else(|| Arc::from("")),
            true,
        )
    }

    pub fn toggle_open(&mut self) -> bool {
        if self.inactive() {
            return false;
        }
        self.opened = !self.opened;
        if self.opened {
            self.highlighted = self.selected_index().or_else(|| self.first_enabled());
        } else {
            self.highlighted = None;
        }
        true
    }

    pub fn close(&mut self) {
        self.opened = false;
        self.highlighted = None;
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

    pub fn commit_highlighted(&mut self) -> Option<SelectChanged> {
        let index = self.highlighted?;
        self.select_index(index)
    }

    pub fn select_index(&mut self, index: usize) -> Option<SelectChanged> {
        let option = self.options.get(index)?;
        if option.disabled || self.inactive() {
            return None;
        }
        let changed = self.value.as_deref() != Some(&*option.value);
        self.value = Some(Arc::clone(&option.value));
        self.close();
        changed.then(|| SelectChanged {
            value: Arc::clone(self.value.as_ref().expect("selected value")),
        })
    }

    fn first_enabled(&self) -> Option<usize> {
        self.options.iter().position(|option| !option.disabled)
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = self.style.clone();
        style.foreground = Some(if self.inactive() || self.display_label().1 {
            SemanticColorRole::Faint
        } else {
            SemanticColorRole::Text
        });
        style.background = Some(if self.inactive() {
            SemanticColorRole::Subtle
        } else {
            SemanticColorRole::Background
        });
        style.border = Some(if self.invalid {
            SemanticColorRole::Danger
        } else if self.opened {
            SemanticColorRole::BorderSoft
        } else {
            SemanticColorRole::Border
        });
        style.interaction.hovered.border = Some(if self.invalid {
            SemanticColorRole::Danger
        } else {
            SemanticColorRole::BorderStrong
        });
        style.interaction.focused.border = Some(if self.invalid {
            SemanticColorRole::Danger
        } else {
            SemanticColorRole::BorderStrong
        });
        style.interaction.disabled = SemanticPaint {
            foreground: Some(SemanticColorRole::Faint),
            background: Some(SemanticColorRole::Subtle),
            border: Some(SemanticColorRole::Border),
            ..SemanticPaint::default()
        };
        let has_explicit_height = style.layout.height.is_some();
        let needs_height = style.control_height.is_none() && !has_explicit_height;
        let needs_radius = style.layout.border_radius.is_none();
        {
            let layout = Arc::make_mut(&mut style.layout);
            if layout.width.is_none() {
                layout.width = Some(LengthSpec::Fill);
            }
            layout.border_width = Some(if self.invalid && self.opened {
                2.0
            } else {
                1.0
            });
            layout.white_space_nowrap = true;
        }
        if needs_radius {
            style.radius = Some(nana_ui_core::RadiusTier::Sm);
        }
        if has_explicit_height {
            // A CSS/host box is a spent number; don't let the size step
            // write over it.
            style.control_height = None;
        } else if needs_height {
            style.control_height = Some(nana_ui_core::ControlHeight::Exact(self.size));
        }
        style.control_padding_x = Some(self.size.into());
        style.text_vertical_alignment = TextVerticalAlignment::Center;
        style
    }
}

impl crate::ComponentView for Select {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::toggle_select),
        activate_at: Some(crate::AppContext::activate_select_at),
        close_options: Some(|context, entity| {
            context.update_component(entity, |select, _| {
                if !select.opened {
                    return false;
                }
                select.close();
                true
            })
        }),
        hooks: Some(|hooks| hooks.select = Some(&SELECT_HOOKS)),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn reconcile(&mut self, mut next: Self) {
        // Value/options are application state; the open menu and its keyboard
        // highlight belong to the current interaction while those props agree.
        if next.inactive() {
            next.close();
        } else if self.value == next.value && self.options == next.options {
            next.opened = self.opened;
            next.highlighted = self.highlighted;
        }
        *self = next;
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "select".into(),
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
            options: self
                .options
                .iter()
                .map(|option| SelectOptionData {
                    label: Arc::clone(&option.label),
                    hint: None,
                    disabled: option.disabled,
                    checked: false,
                    icon: None,
                })
                .collect(),
            highlighted: self.highlighted,
            checkable: false,
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
                // Named by its field (a settings row's label, see
                // `MutationQueue::set_labelled_by`), or by the option it shows
                // through its text when nothing names it; the option shown
                // is its value.
                label: None,
                value: (!placeholder).then_some(label),
                disabled: self.inactive(),
                busy: self.loading,
                invalid: self.invalid,
                selected: Some(self.opened && !self.inactive()),
                ..AccessibilityState::default()
            },
        );
    }
}

/// Geometry for a select / dropdown trigger and its open menu.
///
/// `padding` is the inset the node was **laid out** with, not the one it
/// authored. A control that names its inset (`NodeStyle::control_padding_x`)
/// carries no padding on its authored box — that is the point of naming a step
/// instead of spending it — so re-deriving the inset from `source.layout` here
/// reported 0 and left the trigger's label flush against its own border. The
/// caller has already resolved it through `UiWorld::used_layout_padding`.
pub(crate) fn select_geometry(
    bounds: LayoutBox,
    label: &Arc<str>,
    placeholder: bool,
    size: ControlSize,
    opened: bool,
    options: &[SelectOptionData],
    highlighted: Option<usize>,
    style: &ComputedStyle,
    source: &NodeStyle,
    palette: &SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
    viewport: Option<crate::LayoutViewport>,
    checkable: bool,
    padding: nana_ui_core::PaddingSpec,
) -> ComponentGeometry {
    let border = source.layout.resolved_border_width();
    let content = LayoutBox {
        x: bounds.x + border + padding.left,
        y: bounds.y + border + padding.top,
        width: (bounds.width - border * 2.0 - padding.left - padding.right).max(0.0),
        height: (bounds.height - border * 2.0 - padding.top - padding.bottom).max(0.0),
    };
    let handle_width = HANDLE_WIDTH.min(content.width);
    let label_width = (content.width - handle_width).max(0.0);
    let text_color = if placeholder || matches!(style.foreground, SemanticColorRole::Faint) {
        palette.faint.as_rgba_array()
    } else {
        style.color.unwrap_or_else(|| palette.text.as_rgba_array())
    };
    let handle_color = palette.muted.as_rgba_array();
    let menu = opened.then(|| {
        select_menu_geometry(
            bounds,
            size,
            options,
            highlighted,
            palette,
            metrics,
            viewport,
            checkable,
        )
    });
    ComponentGeometry::Select {
        label: ComponentTextRegion {
            bounds: LayoutBox {
                x: content.x,
                y: content.y,
                width: label_width,
                height: content.height,
            },
            content: Arc::clone(label).into(),
            color: Some(text_color),
            font_size: size.text_size(),
            font_weight: None,
        },
        handle: LayoutBox {
            x: content.x + label_width,
            y: content.y,
            width: handle_width,
            height: content.height,
        },
        handle_color,
        background: style.background,
        border: style.border_color,
        border_width: border,
        menu,
    }
}

fn select_menu_geometry(
    field: LayoutBox,
    size: ControlSize,
    options: &[SelectOptionData],
    highlighted: Option<usize>,
    palette: &SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
    viewport: Option<crate::LayoutViewport>,
    checkable: bool,
) -> crate::SelectMenuGeometry {
    let item_height = size.height_in(metrics);
    let count = options.len().max(1) as f32;
    let natural_height =
        MENU_PAD * 2.0 + count * item_height + (count - 1.0).max(0.0) * MENU_ITEM_GAP;
    let is_light = palette.background.as_rgba_array()[0] > 0.5;
    let (surface_y, height) = resolve_menu_vertical(field, natural_height, viewport);
    let surface = LayoutBox {
        x: field.x,
        y: surface_y,
        width: field.width,
        height,
    };
    // Reserved for the whole menu when the menu *can* check, not when one
    // option currently is: deriving it from `any(checked)` shifted every label
    // 16px sideways the moment the first option became checked.
    let check_reserve = if checkable { MENU_CHECK_RESERVE } else { 0.0 };
    let options = options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let y = surface.y + MENU_PAD + index as f32 * (item_height + MENU_ITEM_GAP);
            let selected = highlighted == Some(index);
            let bounds = LayoutBox {
                x: surface.x + MENU_PAD,
                y,
                width: (surface.width - MENU_PAD * 2.0).max(0.0),
                height: item_height,
            };
            crate::SelectOptionGeometry {
                bounds,
                label: ComponentTextRegion {
                    bounds: LayoutBox {
                        x: bounds.x + size.padding_x_in(metrics) + check_reserve,
                        y: bounds.y,
                        width: (bounds.width - size.padding_x_in(metrics) * 2.0 - check_reserve)
                            .max(0.0),
                        height: bounds.height,
                    },
                    content: menu_option_label(option).into(),
                    color: Some(if option.disabled {
                        palette.faint.as_rgba_array()
                    } else {
                        palette.text.as_rgba_array()
                    }),
                    font_size: size.text_size(),
                    font_weight: None,
                },
                selected,
                checked: option.checked,
                disabled: option.disabled,
                background: selected.then_some(palette.selected.as_rgba_array()),
                icon: None,
            }
        })
        .collect();
    crate::SelectMenuGeometry {
        surface,
        elevation: ComponentElevation {
            color: [0.0, 0.0, 0.0, if is_light { 0.24 } else { 0.48 }],
            offset_x: 0.0,
            offset_y: 8.0,
            blur_radius: 16.0,
            spread_radius: 0.0,
            inset: false,
        },
        background: palette.surface.as_rgba_array(),
        border: palette.border_soft.as_rgba_array(),
        options,
    }
}

pub(crate) fn menu_option_label(option: &SelectOptionData) -> Arc<str> {
    match option.hint.as_ref() {
        Some(hint) if !hint.is_empty() => Arc::from(format!("{}  ·  {hint}", option.label)),
        _ => Arc::clone(&option.label),
    }
}

pub(crate) fn select_option_at(menu: &crate::SelectMenuGeometry, x: f32, y: f32) -> Option<usize> {
    menu.options
        .iter()
        .position(|option| !option.disabled && option.bounds.contains(x, y))
}

/// Places the drop-down surface below its field, folding it back inside the
/// window near the bottom edge.
///
/// Preference order matches the menu families: open downwards; flip above the
/// field when the space below cannot hold the menu but the space above can;
/// otherwise stay on the roomier side and cap the height to it. Without a
/// viewport (the document has not been laid out yet) the menu keeps its natural
/// downward placement.
fn resolve_menu_vertical(
    field: LayoutBox,
    natural_height: f32,
    viewport: Option<crate::LayoutViewport>,
) -> (f32, f32) {
    let below_y = field.y + field.height + MENU_GAP;
    let Some(viewport) = viewport else {
        return (below_y, natural_height);
    };
    let space_below = (viewport.height - below_y).max(0.0);
    if natural_height <= space_below {
        return (below_y, natural_height);
    }
    let space_above = (field.y - MENU_GAP).max(0.0);
    if natural_height <= space_above {
        return (field.y - MENU_GAP - natural_height, natural_height);
    }
    if space_above > space_below {
        (0.0, space_above)
    } else {
        (below_y, space_below)
    }
}

/// Re-applies only the metrics `ControlSize` owns onto an existing field style.
///
/// `size()` builders use this instead of rebuilding the whole [`NodeStyle`], so
/// a style the caller supplied keeps its colors, borders and interaction paints.
pub(crate) fn apply_field_size(style: &mut NodeStyle, size: ControlSize) {
    style.control_height = Some(nana_ui_core::ControlHeight::Exact(size));
    style.control_padding_x = Some(size.into());
}

pub(crate) fn field_style_for_size(size: ControlSize) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Fill),
            border_width: Some(nana_ui_core::HAIRLINE),
            white_space_nowrap: true,
            ..nana_ui_core::LayoutStyle::default()
        }),
        radius: Some(nana_ui_core::RadiusTier::Sm),
        control_height: Some(nana_ui_core::ControlHeight::Exact(size)),
        control_padding_x: Some(size.into()),
        foreground: Some(SemanticColorRole::Text),
        background: Some(SemanticColorRole::Background),
        border: Some(SemanticColorRole::Border),
        interaction: InteractionStyle {
            hovered: SemanticPaint {
                border: Some(SemanticColorRole::BorderStrong),
                ..SemanticPaint::default()
            },
            focused: SemanticPaint::FOCUS_BORDER,
            disabled: SemanticPaint {
                foreground: Some(SemanticColorRole::Faint),
                background: Some(SemanticColorRole::Subtle),
                border: Some(SemanticColorRole::Border),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        },
        text_vertical_alignment: TextVerticalAlignment::Center,
        ..NodeStyle::default()
    }
}

/// The router reaches this list's keyboard only once one exists.
pub(crate) static SELECT_HOOKS: crate::framework::ChoiceHooks = crate::framework::ChoiceHooks {
    adjust: crate::AppContext::adjust_focused_select,
    commit: crate::AppContext::commit_focused_select,
};

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_multi_select_dropdown_reserves_the_check_lane() {
        use crate::{Dropdown, DropdownOption, StandardVisual};

        let checkable_of = |context: &AppContext, id| match context.world().standard_visual(id) {
            Some(StandardVisual::Select { checkable, .. }) => checkable,
            other => panic!("expected a Select visual, got {other:?}"),
        };
        let options = [DropdownOption::new("a", "A"), DropdownOption::new("b", "B")];

        let mut context = AppContext::new();
        // A plain Select never draws checks, so it must not indent its labels.
        let select = context.create_component(document(), sample()).unwrap();
        assert!(!checkable_of(&context, select.stable_id()));

        // Single-select dropdown: still no check column.
        let single = context
            .create_component(
                document(),
                Dropdown::single(Some("a")).options(options.clone()),
            )
            .unwrap();
        assert!(!checkable_of(&context, single.stable_id()));

        // Multi-select reserves the lane even while nothing is checked yet, so
        // labels do not jump sideways on the first check.
        let empty_multi = context
            .create_component(
                document(),
                Dropdown::multiple(Vec::<String>::new()).options(options.clone()),
            )
            .unwrap();
        assert!(checkable_of(&context, empty_multi.stable_id()));

        let checked_multi = context
            .create_component(
                document(),
                Dropdown::multiple(vec!["a".to_string()]).options(options),
            )
            .unwrap();
        assert!(checkable_of(&context, checked_multi.stable_id()));
    }

    fn menu_field(y: f32) -> LayoutBox {
        LayoutBox {
            x: 10.0,
            y,
            width: 120.0,
            height: 32.0,
        }
    }

    #[test]
    fn menu_opens_downwards_when_the_viewport_has_room() {
        let viewport = crate::LayoutViewport::new(600.0, 600.0);
        let field = menu_field(100.0);
        let (y, height) = resolve_menu_vertical(field, 90.0, Some(viewport));
        assert_eq!(y, field.y + field.height + MENU_GAP);
        assert_eq!(height, 90.0);
    }

    #[test]
    fn menu_flips_above_the_field_when_it_would_fall_off_the_bottom() {
        // Field near the bottom edge: 600 - (500 + 32 + gap) leaves < 90px below,
        // while 500px sit above it.
        let viewport = crate::LayoutViewport::new(600.0, 600.0);
        let field = menu_field(500.0);
        let (y, height) = resolve_menu_vertical(field, 90.0, Some(viewport));
        assert_eq!(height, 90.0, "a flipped menu keeps its natural height");
        assert_eq!(y, field.y - MENU_GAP - 90.0);
        assert!(y >= 0.0, "flipped menu stays inside the viewport");
    }

    #[test]
    fn a_menu_taller_than_both_sides_is_capped_to_the_roomier_one() {
        let viewport = crate::LayoutViewport::new(600.0, 300.0);
        let field = menu_field(220.0);
        let (y, height) = resolve_menu_vertical(field, 5_000.0, Some(viewport));
        // 220px above vs 300-(220+32+gap) below: it opens upwards, capped.
        assert_eq!(y, 0.0);
        assert!(height <= 220.0, "capped to the space above, got {height}");
        assert!(height > 0.0);
    }

    #[test]
    fn menu_without_a_laid_out_viewport_keeps_its_downward_placement() {
        let field = menu_field(500.0);
        let (y, height) = resolve_menu_vertical(field, 90.0, None);
        assert_eq!(y, field.y + field.height + MENU_GAP);
        assert_eq!(height, 90.0);
    }

    #[test]
    fn size_keeps_caller_supplied_style_and_only_rewrites_size_metrics() {
        let mut custom = field_style_for_size(ControlSize::Medium);
        custom.background = Some(SemanticColorRole::Selected);
        Arc::make_mut(&mut custom.layout).border_radius = Some(17.0);

        let select = Select::new(Some("a"))
            .style(custom)
            .size(ControlSize::Large);

        // Caller-owned paint and geometry survive `size()`.
        assert_eq!(select.style.background, Some(SemanticColorRole::Selected));
        assert_eq!(select.style.layout.border_radius, Some(17.0));
        // Metrics that `ControlSize` owns are refreshed as named steps.
        assert_eq!(
            select.style.control_height,
            Some(nana_ui_core::ControlHeight::Exact(ControlSize::Large))
        );
        assert_eq!(
            select.style.control_padding_x,
            Some(ControlSize::Large.into())
        );
    }

    #[test]
    fn background_props_refresh_preserves_open_select_and_keyboard_highlight() {
        let options = [SelectOption::new("a", "A"), SelectOption::new("b", "B")];
        let mut select = Select::new(Some("a")).options(options.clone()).opened(true);
        select.highlight_delta(1);
        crate::ComponentView::reconcile(
            &mut select,
            Select::new(Some("a")).options(options).invalid(true),
        );
        assert!(select.opened);
        assert_eq!(select.highlighted, Some(1));
        assert!(select.invalid);
    }

    #[test]
    fn application_selection_or_options_change_replaces_menu_state() {
        let options = [SelectOption::new("a", "A"), SelectOption::new("b", "B")];
        let mut select = Select::new(Some("a")).options(options.clone()).opened(true);
        crate::ComponentView::reconcile(
            &mut select,
            Select::new(Some("b")).options(options.clone()),
        );
        assert!(!select.opened);
        assert_eq!(select.highlighted, None);
        assert_eq!(select.value.as_deref(), Some("b"));
        select.toggle_open();
        crate::ComponentView::reconcile(
            &mut select,
            Select::new(Some("b")).options([options[1].clone()]),
        );
        assert!(!select.opened);
        assert_eq!(select.highlighted, None);
    }

    #[test]
    fn disabling_or_loading_select_closes_its_open_menu() {
        for loading in [false, true] {
            let mut select = Select::new(Some("a"))
                .options([SelectOption::new("a", "A")])
                .opened(true);
            let next = select.clone().disabled(!loading).loading(loading);
            crate::ComponentView::reconcile(&mut select, next);
            assert!(!select.opened);
            assert_eq!(select.highlighted, None);
        }
    }

    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample() -> Select {
        Select::new(Some("code"))
            .placeholder("Choose view")
            .options([
                SelectOption::new("code", "Code"),
                SelectOption::new("split", "Split").disabled(true),
                SelectOption::new("preview", "Preview"),
            ])
    }

    #[test]
    fn select_projects_closed_field_and_keeps_disabled_options() {
        let mut context = AppContext::new();
        let select = context.create_component(document(), sample()).unwrap();
        let id = select.stable_id();
        assert!(matches!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == "select"
        ));
        assert!(matches!(
            context.world().standard_visual(id),
            Some(StandardVisual::Select {
                placeholder: false,
                opened: false,
                ..
            })
        ));
        assert_eq!(context.world().text(id), Some("Code"));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Background));
        assert_eq!(style.border, Some(SemanticColorRole::Border));
        // The field names the radius step; the number is produced at extract
        // against the installed metrics (Issue #101 F1).
        assert_eq!(style.radius, Some(nana_ui_core::RadiusTier::Sm));
        assert_eq!(
            context.world().extract_nodes(&[id])[0]
                .source_style
                .layout
                .border_radius,
            Some(nana_ui_core::UI_METRICS.radius_sm)
        );
        let visual = match context.world().standard_visual(id) {
            Some(StandardVisual::Select { options, .. }) => options,
            _ => panic!("select visual"),
        };
        assert_eq!(visual.len(), 3);
        assert!(visual[1].disabled);
    }

    #[test]
    fn select_toggle_and_commit_skip_disabled_options() {
        let mut select = sample();
        assert!(select.toggle_open());
        assert!(select.opened);
        assert_eq!(select.highlighted, Some(0));
        assert!(select.highlight_delta(1));
        assert_eq!(select.highlighted, Some(2));
        let changed = select.commit_highlighted().expect("commit preview");
        assert_eq!(changed.value.as_ref(), "preview");
        assert!(!select.opened);
        assert_eq!(select.value.as_deref(), Some("preview"));
        assert!(select.select_index(1).is_none());
    }

    #[test]
    fn inactive_select_does_not_open() {
        let mut select = sample().disabled(true);
        assert!(!select.toggle_open());
        let mut loading = sample().loading(true);
        assert!(!loading.toggle_open());
    }

    #[test]
    fn opened_invalid_select_uses_a_thicker_danger_border() {
        let mut context = AppContext::new();
        let select = context
            .create_component(document(), sample().invalid(true).opened(true))
            .unwrap();
        let style = context.world().node_style(select.stable_id()).unwrap();
        assert_eq!(style.border, Some(SemanticColorRole::Danger));
        assert_eq!(style.layout.border_width, Some(2.0));
        assert!(matches!(
            context.world().standard_visual(select.stable_id()),
            Some(StandardVisual::Select {
                opened: true,
                invalid: true,
                ..
            })
        ));
    }

    #[test]
    fn select_projects_overlay_css_size_into_world_style() {
        let mut select = sample();
        Arc::make_mut(&mut select.style.layout).overlay_css_size_overrides(
            &nana_ui_core::LayoutStyle {
                height: Some(LengthSpec::Px(48.0)),
                width: Some(LengthSpec::Px(120.0)),
                ..nana_ui_core::LayoutStyle::default()
            },
        );
        let mut context = AppContext::new();
        let select = context.create_component(document(), select).unwrap();
        let layout = context
            .world()
            .node_style(select.stable_id())
            .unwrap()
            .layout
            .as_ref();
        assert_eq!(layout.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(layout.width, Some(LengthSpec::Px(120.0)));
        assert_ne!(
            layout.height,
            Some(LengthSpec::Px(
                ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
            ))
        );
    }

    #[test]
    fn select_projects_without_css_keeps_control_size() {
        let mut context = AppContext::new();
        let select = context.create_component(document(), sample()).unwrap();
        let style = context.world().node_style(select.stable_id()).unwrap();
        assert_eq!(
            style.control_height,
            Some(nana_ui_core::ControlHeight::Exact(ControlSize::Medium))
        );
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(
            context.world().extract_nodes(&[select.stable_id()])[0]
                .source_style
                .layout
                .height,
            Some(LengthSpec::Px(
                ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
            ))
        );
    }

    #[test]
    fn placeholder_is_used_when_no_value_matches() {
        let select = Select::new(None::<Arc<str>>)
            .placeholder("Choose view")
            .options([SelectOption::new("code", "Code")]);
        let (label, placeholder) = select.display_label();
        assert_eq!(label.as_ref(), "Choose view");
        assert!(placeholder);
    }
}
