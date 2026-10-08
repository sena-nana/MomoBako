//! Built-in components install through the same [`UiExtension`] ABI as plugins.

use std::sync::Arc;

use nana_ui_core::{
    ButtonKind, CardKind, CommandPaletteItem, DrawerSide, Icon, LengthSpec, RegionId,
    SettingsModel, SettingsState, SettingsTab, SettingsTabId, SplitAxis, SplitPaneModel,
    SplitPaneMutation, StatusTone, SwitchControlPosition, TreeNode, ValidationIntent,
    WorkspaceModel,
};

#[cfg(feature = "rich-text")]
use crate::NativeMarkdown;
use crate::json_u64;
use crate::{
    ActionMenu, ActionMenuItem, AppShell, AppTitleBar, Avatar, Button, Card, Checkbox, Chip,
    ColorField, CommandPalette, ConfirmDialog, ContextMenu, ContextMenuItem, DesktopShell, Dialog,
    DiffView, Divider, Dock, DockAxis, DockNode, Drawer, Dropdown, DropdownOption, EmptyState,
    ExtensionRegistrar, FormField, FrameworkError, GpuTextureView, GpuView, HostedTextarea,
    IconButton, IconGlyph, InteractiveCard, LabeledValue, LevelMeter, List, ListItem,
    ListItemSlots, ModalSurface, NodeStyle, NumberInput, PaneChrome, PathField, Popover, Progress,
    QrCode, RangeField, ScrollView, SearchDropdown, SearchDropdownOption, SegmentedControl, Select,
    SettingsCard, SettingsCollapsibleCard, SettingsPage, SettingsRow, SidebarFooter, SidebarFrame,
    SidebarRow, SidebarRowState, SidebarRowTone, SidebarSection, Skeleton, Spinner, SplitPane,
    Stack, StatusBadge, Switch, Table, TableCell, TableRow, Tabs, Text, TextArea,
    TextDiagnosticSeverity, TextDiagnosticSpan, TextGitMark, TextGitMarkKind, TextInput,
    TextInputState, Thumbnail, ThumbnailState, Toast, ToastTone, Tooltip, TreeView, UiExtension,
    ValidationMessage, ValueEmphasis, Video, Workspace, WorkspaceRegionSlot, XYPad, XYPadValue,
    component_registry::{RegisterableComponent, SemanticSpec},
};
#[cfg(feature = "calendar")]
use crate::{CalendarHeatmap, CalendarHeatmapDatum, CalendarHeatmapOptions, CalendarLevelStrategy};
#[cfg(feature = "charts")]
use crate::{DonutChart, DonutSlice, TimeSeriesChart};
#[cfg(feature = "graph-canvas")]
use crate::{GraphCanvas, GraphModel};
#[cfg(feature = "image-viewer")]
use crate::{ImageViewer, ImageViewerContent};
#[cfg(feature = "controls")]
use crate::{ReorderItem, ReorderList};
#[cfg(feature = "graph-canvas")]
use nana_ui_core::{
    GraphEdge, GraphEndpoint, GraphNode, GraphPoint, GraphPort, GraphPortKind, GraphPortSide,
    GraphSelection, GraphSize, GraphViewport,
};

fn overlay_l2_css_size(
    target: &mut Arc<nana_ui_core::LayoutStyle>,
    css: &nana_ui_core::LayoutStyle,
) {
    Arc::make_mut(target).overlay_css_size_overrides(css);
}

pub struct NanaBuiltinComponents;

impl UiExtension for NanaBuiltinComponents {
    fn name(&self) -> &'static str {
        "nana.builtin"
    }

    fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
        install_builtins::<true>(registrar)
    }
}

/// The built-in components' identities only, for
/// [`crate::BuiltinComponents::Typed`].
pub(crate) struct NanaBuiltinIdentities;

impl UiExtension for NanaBuiltinIdentities {
    fn name(&self) -> &'static str {
        "nana.builtin"
    }

    fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
        install_builtins::<false>(registrar)
    }
}

/// One list for both modes. `BIND` is a constant, so the identity-only
/// instance never mentions a binder and the linker drops every component the
/// program does not create.
fn component<C: RegisterableComponent, const BIND: bool>(
    registrar: &mut ExtensionRegistrar,
) -> Result<(), FrameworkError> {
    if BIND {
        registrar.register_component::<C>()
    } else {
        registrar.register_identity::<C>(C::TYPE_ID, Some(std::any::TypeId::of::<C>()), C::TAGS)
    }
}

fn alias<C: RegisterableComponent, const BIND: bool>(
    registrar: &mut ExtensionRegistrar,
    type_id: &'static str,
    tags: &'static [&'static str],
) -> Result<(), FrameworkError> {
    if BIND {
        registrar.register_component_alias::<C>(type_id, tags)
    } else {
        registrar.register_identity::<C>(type_id, None, tags)
    }
}

fn install_builtins<const BIND: bool>(
    registrar: &mut ExtensionRegistrar,
) -> Result<(), FrameworkError> {
    component::<Stack, BIND>(registrar)?;
    alias::<Stack, BIND>(registrar, "nana.column", &["column"])?;
    alias::<Stack, BIND>(registrar, "nana.row", &["row"])?;
    alias::<Stack, BIND>(registrar, "nana.box", &["box"])?;
    alias::<Stack, BIND>(registrar, "nana.drop-target", &["drop-target"])?;
    component::<Text, BIND>(registrar)?;
    component::<Button, BIND>(registrar)?;
    component::<IconButton, BIND>(registrar)?;
    component::<IconGlyph, BIND>(registrar)?;
    component::<Checkbox, BIND>(registrar)?;
    component::<Divider, BIND>(registrar)?;
    component::<NumberInput, BIND>(registrar)?;
    component::<Switch, BIND>(registrar)?;
    component::<Card, BIND>(registrar)?;
    component::<ListItem, BIND>(registrar)?;
    component::<Thumbnail, BIND>(registrar)?;
    component::<Chip, BIND>(registrar)?;
    component::<crate::DatePicker, BIND>(registrar)?;
    component::<crate::Toolbar, BIND>(registrar)?;
    component::<crate::StatusBar, BIND>(registrar)?;
    component::<crate::MediaTransportBar, BIND>(registrar)?;
    component::<Avatar, BIND>(registrar)?;
    component::<TextInput, BIND>(registrar)?;
    component::<TextArea, BIND>(registrar)?;
    component::<crate::TerminalView, BIND>(registrar)?;
    component::<DiffView, BIND>(registrar)?;
    component::<crate::NativeContent, BIND>(registrar)?;
    component::<crate::BrowserView, BIND>(registrar)?;
    component::<HostedTextarea, BIND>(registrar)?;
    component::<RangeField, BIND>(registrar)?;
    component::<Progress, BIND>(registrar)?;
    component::<Spinner, BIND>(registrar)?;
    component::<StatusBadge, BIND>(registrar)?;
    component::<ValidationMessage, BIND>(registrar)?;
    component::<EmptyState, BIND>(registrar)?;
    component::<LabeledValue, BIND>(registrar)?;
    component::<Dialog, BIND>(registrar)?;
    component::<ConfirmDialog, BIND>(registrar)?;
    component::<Select, BIND>(registrar)?;
    component::<Tabs, BIND>(registrar)?;
    component::<SegmentedControl, BIND>(registrar)?;
    component::<Dropdown, BIND>(registrar)?;
    component::<SearchDropdown, BIND>(registrar)?;
    component::<Drawer, BIND>(registrar)?;
    component::<crate::Panel, BIND>(registrar)?;
    component::<Popover, BIND>(registrar)?;
    component::<ContextMenu, BIND>(registrar)?;
    component::<Toast, BIND>(registrar)?;
    component::<ActionMenu, BIND>(registrar)?;
    component::<ActionMenuItem, BIND>(registrar)?;
    component::<Tooltip, BIND>(registrar)?;
    component::<XYPad, BIND>(registrar)?;
    component::<ColorField, BIND>(registrar)?;
    component::<PathField, BIND>(registrar)?;
    component::<QrCode, BIND>(registrar)?;
    component::<FormField, BIND>(registrar)?;
    component::<InteractiveCard, BIND>(registrar)?;
    component::<Skeleton, BIND>(registrar)?;
    component::<LevelMeter, BIND>(registrar)?;
    component::<CommandPalette, BIND>(registrar)?;
    component::<TreeView, BIND>(registrar)?;
    #[cfg(feature = "calendar")]
    component::<CalendarHeatmap<()>, BIND>(registrar)?;
    #[cfg(feature = "image-viewer")]
    component::<ImageViewer, BIND>(registrar)?;
    #[cfg(feature = "rich-text")]
    component::<NativeMarkdown, BIND>(registrar)?;
    #[cfg(feature = "graph-canvas")]
    component::<GraphCanvas, BIND>(registrar)?;
    component::<Workspace, BIND>(registrar)?;
    component::<Dock, BIND>(registrar)?;
    component::<SplitPane, BIND>(registrar)?;
    component::<AppShell, BIND>(registrar)?;
    component::<SidebarFrame, BIND>(registrar)?;
    component::<SidebarRow, BIND>(registrar)?;
    component::<SettingsRow, BIND>(registrar)?;
    component::<SettingsCard, BIND>(registrar)?;
    component::<SettingsPage, BIND>(registrar)?;
    component::<SettingsCollapsibleCard, BIND>(registrar)?;
    component::<List, BIND>(registrar)?;
    component::<ScrollView, BIND>(registrar)?;
    component::<Table, BIND>(registrar)?;
    component::<TableRow, BIND>(registrar)?;
    component::<TableCell, BIND>(registrar)?;
    #[cfg(feature = "controls")]
    component::<ReorderList, BIND>(registrar)?;
    #[cfg(feature = "charts")]
    component::<TimeSeriesChart, BIND>(registrar)?;
    #[cfg(feature = "charts")]
    component::<DonutChart, BIND>(registrar)?;
    component::<DesktopShell, BIND>(registrar)?;
    component::<AppTitleBar, BIND>(registrar)?;
    component::<PaneChrome, BIND>(registrar)?;
    component::<SidebarSection, BIND>(registrar)?;
    component::<SidebarFooter, BIND>(registrar)?;
    component::<GpuTextureView, BIND>(registrar)?;
    component::<GpuView, BIND>(registrar)?;
    component::<Video, BIND>(registrar)?;
    Ok(())
}

impl RegisterableComponent for Stack {
    const TYPE_ID: &'static str = crate::component_descriptors::STACK.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::STACK.tags;
    const BIND_KIND: crate::ComponentBindKind = crate::ComponentBindKind::Layout;
    // A layout box carries its children; it does not address any of them by
    // name, and it has no icon. Saying so lets the host skip both scans of the
    // child list -- and a layout box is exactly the widget with the most
    // children to scan. `stack_ignores_child_derived_spec` holds this honest.
    const READS_CHILD_DERIVED_SPEC: bool = false;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut layout = spec.layout.as_ref().clone();
        // `nana.row` needs the seed: the engine's default flow axis is the
        // block axis, so a row that does not say so lays out as a column.
        //
        // `nana.column` must NOT be seeded, even though it reads as the
        // symmetric case. `used_flow_direction` is `unwrap_or(Column)`, so
        // `None` and `Some(Column)` are the same layout -- and writing the
        // second spelling over the first is not free. The Vue cascade rebuilds
        // a node's `LayoutStyle` from scratch whenever author CSS applies to
        // it, which leaves `direction` unset; this seed then wrote
        // `Some(Column)` back through `project_common`, and the two writers
        // alternated on every node, every pointer event. On a 2,000-row list
        // that alternation scheduled a layout pass per event that nothing had
        // asked for: removing it takes `FrameStage::Layout` on a hover from
        // 0.138 ms to zero and settle from 0.62 to 0.40 ms.
        //
        // `projecting_a_stack_does_not_rewrite_the_style_the_cascade_published`
        // holds this; `an_unset_flow_axis_is_a_column_but_not_a_row` holds the
        // half that is load-bearing.
        if layout.direction.is_none() && spec.type_id.as_str() == "nana.row" {
            layout.direction = Some(nana_ui_core::FlexDirection::Row);
        }
        Stack::from_layout(layout)
    }
}

impl RegisterableComponent for Text {
    const TYPE_ID: &'static str = crate::component_descriptors::TEXT.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TEXT.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        Text::new(spec.display_label()).style(crate::NodeStyle {
            layout: Arc::clone(spec.layout),
            ..crate::NodeStyle::default()
        })
    }
}

impl RegisterableComponent for Button {
    const TYPE_ID: &'static str = crate::component_descriptors::BUTTON.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::BUTTON.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Button::new(spec.display_label())
            .layout(Arc::clone(spec.layout))
            .kind(spec.button_kind)
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid);
        component.icon = spec.icon;
        if let Some(size) = attr_f32(spec, &["icon-size", "iconSize"]) {
            component = component.icon_size(size);
        }
        if let Some(gap) = attr_f32(spec, &["icon-gap", "iconGap"]) {
            component = component.icon_gap(gap);
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for IconButton {
    const TYPE_ID: &'static str = crate::component_descriptors::ICON_BUTTON.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::ICON_BUTTON.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let icon = spec.icon.unwrap_or(Icon::Add);
        let mut component = IconButton::new(icon, Arc::<str>::from(spec.display_label()))
            .kind(spec.button_kind)
            .size(spec.size)
            .selected(spec.active)
            .disabled(spec.disabled);
        if !spec.hint.is_empty() {
            component = component.tooltip(
                Arc::<str>::from(spec.hint),
                nana_ui_core::TooltipConfig::default(),
            );
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for IconGlyph {
    const TYPE_ID: &'static str = crate::component_descriptors::ICON_GLYPH.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::ICON_GLYPH.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let Some(icon) = spec.icon else {
            return IconGlyph::new(Icon::Search).size(0.0);
        };
        let size = match (spec.layout.width, spec.layout.height) {
            (Some(LengthSpec::Px(w)), Some(LengthSpec::Px(h))) if w > 0.0 && h > 0.0 => w.min(h),
            (Some(LengthSpec::Px(w)), _) if w > 0.0 => w,
            (_, Some(LengthSpec::Px(h))) if h > 0.0 => h,
            _ => spec.size.icon_size(),
        };
        let mut glyph = IconGlyph::new(icon).size(size);
        Arc::make_mut(&mut glyph.style.layout).overlay_css_size_overrides(spec.layout.as_ref());
        glyph
    }
}

impl RegisterableComponent for Checkbox {
    const TYPE_ID: &'static str = crate::component_descriptors::CHECKBOX.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CHECKBOX.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Checkbox::new(spec.display_label(), spec.toggled)
            .indeterminate(parse_flag(spec.attr("indeterminate")))
            .size(spec.size)
            .disabled(spec.disabled)
            .invalid(spec.invalid);
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for Divider {
    const TYPE_ID: &'static str = crate::component_descriptors::DIVIDER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DIVIDER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut divider = if spec
            .attr("orientation")
            .is_some_and(|value| value.eq_ignore_ascii_case("vertical"))
        {
            Divider::vertical()
        } else {
            Divider::horizontal()
        };
        if let Some(thickness) = spec.attr("thickness").and_then(|raw| raw.parse().ok()) {
            divider = divider.thickness(thickness);
        }
        if let Some(inset) = spec.attr("inset").and_then(|raw| raw.parse().ok()) {
            divider = divider.inset(inset);
        }
        divider
    }
}

impl RegisterableComponent for NumberInput {
    const TYPE_ID: &'static str = crate::component_descriptors::NUMBER_INPUT.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::NUMBER_INPUT.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    /// The draft being typed survives a rebind that carries the same number.
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut input = Self::from_semantic(spec);
        if let Some(previous) = previous.filter(|previous| previous.requested == input.requested) {
            input.state = previous.state.clone();
            input.value = previous.value;
        }
        input
    }
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut input = NumberInput::new(f64::from(spec.number))
            .range(f64::from(spec.min), f64::from(spec.max))
            .step(f64::from(spec.step))
            .size(spec.size)
            .disabled(spec.disabled)
            .read_only(spec.read_only)
            .invalid(spec.invalid)
            .placeholder(Arc::<str>::from(spec.placeholder));
        if let Some(precision) = spec.attr("precision").and_then(|raw| raw.parse().ok()) {
            input = input.precision(precision);
        }
        if !spec.label.is_empty() {
            input = input.label(Arc::<str>::from(spec.label));
        }
        overlay_l2_css_size(&mut input.style.layout, spec.layout.as_ref());
        input
    }
}

impl RegisterableComponent for Switch {
    const TYPE_ID: &'static str = crate::component_descriptors::SWITCH.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SWITCH.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Switch::new(spec.display_label(), spec.toggled)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid)
            .size(spec.size)
            .control_position(parse_control_position(spec.attr("control-position")));
        if !spec.hint.is_empty() {
            component = component.hint(Arc::<str>::from(spec.hint));
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for Card {
    const TYPE_ID: &'static str = crate::component_descriptors::CARD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CARD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut card = Card::new()
            .kind(parse_card_kind(spec.attr("card-kind")))
            .loading(spec.loading);
        if !spec.label.is_empty() {
            card = card.title(Arc::<str>::from(spec.label));
        }
        card.style.layout = Arc::clone(spec.layout);
        if spec.layout.border_radius.is_none() && card.style.radius.is_none() {
            card.style.radius = Some(nana_ui_core::RadiusTier::Md);
        }
        if spec.layout.padding.is_some()
            || spec.layout.padding_left.is_some()
            || spec.layout.padding_right.is_some()
            || spec.layout.padding_top.is_some()
            || spec.layout.padding_bottom.is_some()
        {
            card.style.surface_padding = None;
        }
        card
    }
}

impl RegisterableComponent for ListItem {
    const TYPE_ID: &'static str = crate::component_descriptors::LIST_ITEM.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::LIST_ITEM.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = ListItem::new(spec.display_label())
            .selected(spec.active)
            .disabled(spec.disabled)
            .size(spec.size)
            .gap(spec.layout.gap_or(8.0))
            .auto_height(flag_attr(spec, &["auto-height", "autoheight"]))
            .slots(ListItemSlots {
                leading: spec.slot("leading"),
                content: spec.slot("content"),
                trailing: spec.slot("trailing"),
            });
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for crate::Toolbar {
    const TYPE_ID: &'static str = crate::component_descriptors::TOOLBAR.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TOOLBAR.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let bar = crate::Toolbar::new();
        let label = spec.display_label();
        if label.is_empty() {
            bar
        } else {
            bar.label(label)
        }
    }
}

impl RegisterableComponent for crate::StatusBar {
    const TYPE_ID: &'static str = crate::component_descriptors::STATUS_BAR.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::STATUS_BAR.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let bar = crate::StatusBar::new();
        let label = spec.display_label();
        if label.is_empty() {
            bar
        } else {
            bar.label(label)
        }
    }
}

impl RegisterableComponent for crate::DatePicker {
    const TYPE_ID: &'static str = crate::component_descriptors::DATE_PICKER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DATE_PICKER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        // Without a date in the attributes the grid opens on the epoch; the
        // host sets the real cursor through the component.
        let cursor = nana_ui_core::CivilDate::new(1970, 1, 1).expect("epoch is a valid date");
        crate::DatePicker::new(cursor)
            .disabled(spec.disabled)
            .month_label(spec.display_label())
    }
}

impl RegisterableComponent for Chip {
    const TYPE_ID: &'static str = crate::component_descriptors::CHIP.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CHIP.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let selected = spec.active
            || spec.toggled
            || matches!(spec.button_kind, nana_ui_core::ButtonKind::Selected);
        Chip::new(spec.display_label())
            .selected(selected)
            .disabled(spec.disabled)
            .dismissible(flag_attr(spec, &["dismissible", "dismiss"]))
            .size(spec.size)
    }
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut chip = Self::from_semantic(spec);
        if let Some(previous) = previous {
            chip.close = previous.close;
            chip.close_label = Arc::clone(&previous.close_label);
        }
        chip
    }
    fn finish_semantic(
        context: &mut crate::AppContext,
        entity: crate::Entity<Self>,
    ) -> Result<(), crate::FrameworkError> {
        context.assemble_chip(entity).map(|_| ())
    }
}

impl RegisterableComponent for Avatar {
    const TYPE_ID: &'static str = crate::component_descriptors::AVATAR.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::AVATAR.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut avatar = Avatar::new(spec.value);
        if !spec.display_label().is_empty() {
            avatar = avatar.label(Arc::<str>::from(spec.display_label()));
        }
        if let Some(size) = spec
            .attr("size")
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| *value > 0.0)
        {
            avatar = avatar.size(size);
        }
        avatar
    }
}

impl RegisterableComponent for Thumbnail {
    const TYPE_ID: &'static str = crate::component_descriptors::THUMBNAIL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::THUMBNAIL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut thumbnail = Thumbnail::new(spec.value).size(spec.size);
        if spec.attr("fit") == Some("cover") {
            thumbnail = thumbnail.fit(nana_ui_core::ContentFit::Cover);
        }
        if !spec.display_label().is_empty() {
            thumbnail = thumbnail.label(Arc::<str>::from(spec.display_label()));
        }
        if let Some(aspect) = spec.attr("aspect").and_then(|value| value.parse().ok()) {
            thumbnail = thumbnail.aspect(aspect);
        }
        if spec.loading {
            thumbnail = thumbnail.state(ThumbnailState::Loading);
        } else if spec.invalid {
            thumbnail = thumbnail.state(ThumbnailState::Unavailable);
        }
        thumbnail
    }
}

/// An editor's state across a host rebind: its caret, selection and edits
/// stay unless the host's value differs, which replaces the text.
fn retained_edit_state(previous: &TextInputState, value: &str) -> TextInputState {
    let mut state = previous.clone();
    if state.value != value {
        state.replace_value(value);
    }
    state
}

impl RegisterableComponent for TextInput {
    const TYPE_ID: &'static str = crate::component_descriptors::TEXT_INPUT.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TEXT_INPUT.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut input = Self::from_semantic(spec);
        if let Some(previous) = previous {
            input.state = retained_edit_state(&previous.state, spec.value);
        }
        input
    }
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = if spec.placeholder.is_empty() {
            spec.hint
        } else {
            spec.placeholder
        };
        let mut component = TextInput::new("")
            .placeholder(Arc::<str>::from(placeholder))
            .layout(Arc::clone(spec.layout))
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .read_only(spec.read_only)
            .secure(spec.secure)
            .invalid(spec.invalid);
        component.state = TextInputState::new(spec.value);
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for TextArea {
    const TYPE_ID: &'static str = crate::component_descriptors::TEXT_AREA.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TEXT_AREA.tags;
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut area = Self::from_semantic(spec);
        if let Some(previous) = previous {
            area.state = retained_edit_state(&previous.state, spec.value);
            area.resized_height = previous.resized_height;
            area.resize_drag = previous.resize_drag;
            area.scroll_offset = previous.scroll_offset;
        }
        area
    }
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = textarea_placeholder(spec);
        let mut component = TextArea::new("")
            .placeholder(Arc::<str>::from(placeholder))
            .disabled(spec.disabled)
            .read_only(spec.read_only)
            .invalid(spec.invalid);
        if let Some(language) = highlight_language_from_spec(spec) {
            component = component.highlight(language);
        }
        if let Some(LengthSpec::Px(height)) = spec.layout.height {
            component = component.height(height);
        }
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        component.state = TextInputState::new(spec.value);
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        apply_textarea_gutter_from_spec(&mut component, spec);
        component
    }
}

impl RegisterableComponent for HostedTextarea {
    const TYPE_ID: &'static str = crate::component_descriptors::HOSTED_TEXTAREA.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::HOSTED_TEXTAREA.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let language = highlight_language_from_spec(spec).unwrap_or("");
        let mut component = HostedTextarea::new("", language)
            .placeholder(Arc::<str>::from(textarea_placeholder(spec)))
            .disabled(spec.disabled)
            .read_only(spec.read_only)
            .invalid(spec.invalid);
        if let Some(LengthSpec::Px(height)) = spec.layout.height {
            component = component.height(height);
        }
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        component.state = TextInputState::new(spec.value);
        apply_textarea_gutter_from_spec(&mut component, spec);
        component
    }
}

impl RegisterableComponent for RangeField {
    const TYPE_ID: &'static str = crate::component_descriptors::RANGE_FIELD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::RANGE_FIELD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let min = spec.min as f64;
        let max = if spec.max > spec.min {
            spec.max as f64
        } else {
            min + 1.0
        };
        let step = if spec.step > 0.0 {
            spec.step as f64
        } else {
            0.1
        };
        let value = (spec.number as f64).clamp(min, max);
        let mut component = RangeField::new(value, min, max, step)
            .disabled(spec.disabled)
            .invalid(spec.invalid)
            .size(spec.size);
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        if let Some(unit) = spec.attr("unit").filter(|value| !value.is_empty()) {
            component = component.unit(Arc::<str>::from(unit));
        }
        if spec
            .attr("show-value")
            .is_some_and(|value| matches!(value, "false" | "0" | "off"))
        {
            component = component.show_value(false);
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for Progress {
    const TYPE_ID: &'static str = crate::component_descriptors::PROGRESS.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::PROGRESS.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Progress::new(spec.number as f64, spec.max.max(1.0) as f64);
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component.cancellable(spec.attr("cancellable").is_some_and(truthy_attr))
    }
}

impl RegisterableComponent for Spinner {
    const TYPE_ID: &'static str = crate::component_descriptors::SPINNER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SPINNER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        Spinner::new(spec.display_label())
    }
}

impl RegisterableComponent for StatusBadge {
    const TYPE_ID: &'static str = crate::component_descriptors::STATUS_BADGE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::STATUS_BADGE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        StatusBadge::new(spec.display_label(), parse_status_tone(spec.attr("tone")))
            .compact(spec.attr("compact").is_some_and(truthy_attr))
    }
}

impl RegisterableComponent for ValidationMessage {
    const TYPE_ID: &'static str = crate::component_descriptors::VALIDATION_MESSAGE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::VALIDATION_MESSAGE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let message = if spec.hint.is_empty() {
            spec.display_label()
        } else {
            spec.hint
        };
        ValidationMessage::new(message, validation_intent_from_spec(spec))
    }
}

impl RegisterableComponent for EmptyState {
    const TYPE_ID: &'static str = crate::component_descriptors::EMPTY_STATE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::EMPTY_STATE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = EmptyState::new(spec.display_label());
        if !spec.hint.is_empty() {
            component = component.message(Arc::<str>::from(spec.hint));
        }
        if let Some(icon) = spec.icon {
            component = component.icon(icon);
        }
        component = component.compact(flag_attr(spec, &["compact"]));
        if let Some(action) = spec.slot("action") {
            component = component.action_child(action);
        }
        component
    }
}

impl RegisterableComponent for LabeledValue {
    const TYPE_ID: &'static str = crate::component_descriptors::LABELED_VALUE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::LABELED_VALUE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let emphasis = if flag_attr(spec, &["muted"]) {
            ValueEmphasis::Muted
        } else {
            ValueEmphasis::Strong
        };
        let mut component = LabeledValue::new(spec.label, spec.value)
            .emphasis(emphasis)
            .compact(flag_attr(spec, &["compact"]));
        if let Some(action) = spec.slot("action") {
            component = component.action_child(action);
        }
        component
    }
}

impl RegisterableComponent for Dialog {
    const TYPE_ID: &'static str = crate::component_descriptors::DIALOG.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DIALOG.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Dialog::new(spec.display_label());
        if !spec.hint.is_empty() {
            component = component.description(Arc::<str>::from(spec.hint));
        }
        if let Some(body) = spec.slot("body") {
            component.slots_mut().body = Some(body);
        }
        component
    }
}

impl RegisterableComponent for ConfirmDialog {
    const TYPE_ID: &'static str = crate::component_descriptors::CONFIRM_DIALOG.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CONFIRM_DIALOG.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let message = if spec.hint.is_empty() {
            spec.value
        } else {
            spec.hint
        };
        let mut component = ConfirmDialog::new(spec.display_label(), message);
        component.busy = spec.loading;
        component.danger = spec.invalid;
        component
    }
}

impl RegisterableComponent for Select {
    const TYPE_ID: &'static str = crate::component_descriptors::SELECT.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SELECT.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = if spec.placeholder.is_empty() {
            spec.hint
        } else {
            spec.placeholder
        };
        let mut component = Select::new((!spec.value.is_empty()).then_some(spec.value))
            .options(spec.options.iter().map(|option| {
                crate::SelectOption::new(option.value, option.label).disabled(option.disabled)
            }))
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid)
            .opened(spec.active || spec.toggled);
        if !placeholder.is_empty() {
            component = component.placeholder(Arc::<str>::from(placeholder));
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for Tabs {
    const TYPE_ID: &'static str = crate::component_descriptors::TABS.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TABS.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Tabs::new(if spec.value.is_empty() {
            spec.display_label()
        } else {
            spec.value
        })
        .size(spec.size)
        .fill(flag_attr(spec, &["fill"]));
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        component
    }
}

impl RegisterableComponent for SegmentedControl {
    const TYPE_ID: &'static str = crate::component_descriptors::SEGMENTED_CONTROL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SEGMENTED_CONTROL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = if spec
            .attr("chrome")
            .is_some_and(|value| value.eq_ignore_ascii_case("radio"))
            || spec
                .attr("role")
                .is_some_and(|value| value.eq_ignore_ascii_case("radiogroup"))
        {
            SegmentedControl::radio_group()
        } else {
            SegmentedControl::new()
        };
        component = component.size(spec.size).fill(flag_attr(spec, &["fill"]));
        if !spec.label.is_empty() {
            component = component.label(Arc::<str>::from(spec.label));
        }
        component
    }
}

impl RegisterableComponent for Dropdown {
    const TYPE_ID: &'static str = crate::component_descriptors::DROPDOWN.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DROPDOWN.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = if spec.placeholder.is_empty() {
            spec.hint
        } else {
            spec.placeholder
        };
        let mut component = if spec.attr("multiple").is_some() {
            let values = if spec.value.is_empty() {
                Vec::new()
            } else {
                spec.value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            };
            Dropdown::multiple(values)
        } else {
            Dropdown::single((!spec.value.is_empty()).then_some(spec.value))
        };
        component = component
            .options(spec.options.iter().map(|option| {
                DropdownOption::new(option.value, option.label).disabled(option.disabled)
            }))
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid)
            .opened(spec.active || spec.toggled);
        if !placeholder.is_empty() {
            component = component.placeholder(Arc::<str>::from(placeholder));
        }
        overlay_l2_css_size(&mut component.style.layout, spec.layout.as_ref());
        component
    }
}

impl RegisterableComponent for SearchDropdown {
    const TYPE_ID: &'static str = crate::component_descriptors::SEARCH_DROPDOWN.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SEARCH_DROPDOWN.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = if spec.placeholder.is_empty() {
            spec.hint
        } else {
            spec.placeholder
        };
        let mut component = SearchDropdown::new((!spec.value.is_empty()).then_some(spec.value))
            .options(
                spec.options
                    .iter()
                    .map(|option| SearchDropdownOption::new(option.value, option.label)),
            )
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid);
        if !placeholder.is_empty() {
            component = component.placeholder(Arc::<str>::from(placeholder));
        }
        let query = spec
            .attr("query")
            .or_else(|| spec.attr("data-query"))
            .unwrap_or("");
        if !query.is_empty() {
            component = component.query(query.to_string());
        }
        component.opened(spec.active || spec.toggled)
    }
}

impl RegisterableComponent for Drawer {
    const TYPE_ID: &'static str = crate::component_descriptors::DRAWER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DRAWER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component =
            Drawer::new(spec.display_label()).side(parse_drawer_side(spec.attr("side")));
        if !spec.hint.is_empty() {
            component = component.description(Arc::<str>::from(spec.hint));
        }
        if let Some(body) = spec.slot("body") {
            component.slots_mut().body = Some(body);
        }
        component
    }
}

impl RegisterableComponent for Popover {
    const TYPE_ID: &'static str = crate::component_descriptors::POPOVER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::POPOVER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut popover = Popover::new()
            .trigger(spec.display_label())
            .open(spec.active || spec.toggled);
        if let Some(icon) = spec.icon {
            popover = popover.trigger_icon(icon, spec.display_label());
        }
        popover
    }
}

impl RegisterableComponent for ContextMenu {
    const TYPE_ID: &'static str = crate::component_descriptors::CONTEXT_MENU.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CONTEXT_MENU.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let searchable = flag_attr(spec, &["searchable"]) || spec.options.len() >= 6;
        let query = spec
            .attr("query")
            .or_else(|| spec.attr("data-query"))
            .unwrap_or("");
        ContextMenu::new(
            attr_f32(spec, &["anchor-x", "data-anchor-x"]).unwrap_or(0.0),
            attr_f32(spec, &["anchor-y", "data-anchor-y"]).unwrap_or(0.0),
        )
        .items(spec.options.iter().map(|option| {
            ContextMenuItem::new(option.value, option.label).disabled(option.disabled)
        }))
        .query(query)
        .searchable(searchable)
        .open(spec.active || spec.toggled)
    }
}

impl RegisterableComponent for Toast {
    const TYPE_ID: &'static str = crate::component_descriptors::TOAST.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TOAST.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Toast::new(spec.display_label(), parse_toast_tone(spec.attr("tone")));
        if !spec.hint.is_empty() {
            component = component.description(Arc::<str>::from(spec.hint));
        }
        component.dismissible(spec.attr("dismissible").is_some_and(truthy_attr))
    }
}

impl RegisterableComponent for ActionMenu {
    const TYPE_ID: &'static str = crate::component_descriptors::ACTION_MENU.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::ACTION_MENU.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut menu = ActionMenu::new()
            .trigger(spec.display_label())
            .open(spec.active || spec.toggled);
        if let Some(icon) = spec.icon {
            menu = menu.trigger_icon(icon, spec.display_label());
        }
        menu
    }
}

impl RegisterableComponent for ActionMenuItem {
    const TYPE_ID: &'static str = crate::component_descriptors::ACTION_MENU_ITEM.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::ACTION_MENU_ITEM.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = ActionMenuItem::new(spec.display_label())
            .active(spec.active)
            .danger(action_menu_item_danger(spec))
            .disabled(spec.disabled)
            .size(spec.size);
        if !spec.hint.is_empty() {
            component = component.hint(Arc::<str>::from(spec.hint));
        }
        component
    }
}

impl RegisterableComponent for Tooltip {
    const TYPE_ID: &'static str = crate::component_descriptors::TOOLTIP.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TOOLTIP.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        Tooltip::new(if spec.display_label().is_empty() {
            spec.hint
        } else {
            spec.display_label()
        })
    }
}

impl RegisterableComponent for XYPad {
    const TYPE_ID: &'static str = crate::component_descriptors::X_Y_PAD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::X_Y_PAD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let x = attr_f32(spec, &["x", "data-x"]).unwrap_or(spec.number);
        let y = attr_f32(spec, &["y", "data-y"]).unwrap_or(0.0);
        let x_min = attr_f32(spec, &["x-min", "xmin", "data-x-min"]).unwrap_or(spec.min);
        let x_max = attr_f32(spec, &["x-max", "xmax", "data-x-max"]).unwrap_or(spec.max);
        let y_min = attr_f32(spec, &["y-min", "ymin", "data-y-min"]).unwrap_or(spec.min);
        let y_max = attr_f32(spec, &["y-max", "ymax", "data-y-max"]).unwrap_or(spec.max);
        let mut component = XYPad::new(XYPadValue::new(x, y))
            .x_range(x_min, x_max)
            .y_range(y_min, y_max)
            .size(spec.size)
            .disabled(spec.disabled)
            .loading(spec.loading)
            .invalid(spec.invalid);
        if spec.step.is_finite() && spec.step > 0.0 {
            component = component.step(spec.step);
        }
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

impl RegisterableComponent for QrCode {
    const TYPE_ID: &'static str = crate::component_descriptors::QR_CODE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::QR_CODE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let size = match spec.layout.width {
            Some(LengthSpec::Px(px)) if px.is_finite() && px > 0.0 => px,
            _ => QrCode::DEFAULT_SIZE,
        };
        let label = if spec.display_label().is_empty() {
            "QR code"
        } else {
            spec.display_label()
        };
        if let Some((modules, width)) = qr_modules_from_spec(spec)
            && let Ok(component) = QrCode::from_modules(modules, width, size)
        {
            return component.label(Arc::<str>::from(label));
        }
        let payload = spec
            .attr("payload")
            .or_else(|| spec.attr("data-payload"))
            .filter(|value| !value.is_empty())
            .unwrap_or(spec.value);
        if !payload.is_empty()
            && let Ok(component) = QrCode::encode(payload, size)
        {
            return component.label(Arc::<str>::from(label));
        }
        qr_placeholder().label(Arc::<str>::from(label))
    }
}

impl RegisterableComponent for FormField {
    const TYPE_ID: &'static str = crate::component_descriptors::FORM_FIELD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::FORM_FIELD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = FormField::new(spec.display_label()).size(spec.size);
        if !spec.hint.is_empty() {
            if spec.invalid {
                component = component.error(Arc::<str>::from(spec.hint));
            } else {
                component = component.hint(Arc::<str>::from(spec.hint));
            }
        }
        if let Some(control) = spec.slot("control") {
            component = component.control_child(control);
        }
        component
    }
}

impl RegisterableComponent for ColorField {
    const TYPE_ID: &'static str = crate::component_descriptors::COLOR_FIELD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::COLOR_FIELD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let value = crate::parse_hex(spec.value)
            .or_else(|| crate::parse_hex(spec.attr("value").unwrap_or("")))
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        ColorField::new(value)
            .size(spec.size)
            .disabled(spec.disabled)
            .invalid(spec.invalid)
    }
}

impl RegisterableComponent for PathField {
    const TYPE_ID: &'static str = crate::component_descriptors::PATH_FIELD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::PATH_FIELD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = PathField::new(spec.value)
            .size(spec.size)
            .disabled(spec.disabled)
            .invalid(spec.invalid);
        if !spec.placeholder.is_empty() {
            component = component.placeholder(Arc::<str>::from(spec.placeholder));
        }
        component
    }
}

impl RegisterableComponent for InteractiveCard {
    const TYPE_ID: &'static str = crate::component_descriptors::INTERACTIVE_CARD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::INTERACTIVE_CARD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        InteractiveCard::new()
            .selected(spec.active)
            .disabled(spec.disabled)
    }
}

impl RegisterableComponent for Skeleton {
    const TYPE_ID: &'static str = crate::component_descriptors::SKELETON.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SKELETON.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut skeleton = Skeleton::new(
            spec.layout.width.unwrap_or(LengthSpec::Fill),
            match spec.layout.height {
                Some(LengthSpec::Px(h)) if h.is_finite() && h > 0.0 => h,
                _ => 16.0,
            },
        );
        let layout = Arc::make_mut(&mut skeleton.style.layout);
        layout.width = spec.layout.width.or(Some(LengthSpec::Fill));
        layout.height = spec.layout.height.or(Some(LengthSpec::Px(skeleton.height)));
        skeleton
    }
}

impl RegisterableComponent for LevelMeter {
    const TYPE_ID: &'static str = crate::component_descriptors::LEVEL_METER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::LEVEL_METER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = LevelMeter::new(spec.number).tone(parse_status_tone(spec.attr("tone")));
        if let Some(height) = spec.layout.height {
            Arc::make_mut(&mut component.style.layout).height = Some(height);
            if let LengthSpec::Px(px) = height
                && px.is_finite()
                && px > 0.0
            {
                component = component.height(px);
            }
        }
        component
    }
}

impl RegisterableComponent for CommandPalette {
    const TYPE_ID: &'static str = crate::component_descriptors::COMMAND_PALETTE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::COMMAND_PALETTE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let placeholder = if spec.placeholder.is_empty() {
            spec.hint
        } else {
            spec.placeholder
        };
        let query = if !spec.value.is_empty() {
            spec.value
        } else {
            spec.attr("query")
                .or_else(|| spec.attr("data-query"))
                .unwrap_or("")
        };
        let json_items = spec_json(spec, &["items", "options"])
            .map(|value| command_palette_items_from_json(&value))
            .filter(|items| !items.is_empty());
        let items = json_items.unwrap_or_else(|| {
            spec.options
                .iter()
                .map(|option| CommandPaletteItem::new(option.value, option.label))
                .collect()
        });
        let mut component = CommandPalette::new(spec.display_label(), items);
        if !placeholder.is_empty() {
            component = component.placeholder(Arc::<str>::from(placeholder));
        }
        if !query.is_empty() {
            component = component.query(query.to_string());
        }
        component
    }
}

impl RegisterableComponent for TreeView {
    const TYPE_ID: &'static str = crate::component_descriptors::TREE_VIEW.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TREE_VIEW.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        TreeView::new(tree_nodes_from_spec(spec)).size(spec.size)
    }
}

#[cfg(feature = "calendar")]
impl RegisterableComponent for CalendarHeatmap<()> {
    const TYPE_ID: &'static str = crate::component_descriptors::CALENDAR_HEATMAP.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::CALENDAR_HEATMAP.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = CalendarHeatmap::new(calendar_data_from_spec(spec))
            .options(calendar_options_from_spec(spec));
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

#[cfg(feature = "image-viewer")]
impl RegisterableComponent for ImageViewer {
    const TYPE_ID: &'static str = crate::component_descriptors::IMAGE_VIEWER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::IMAGE_VIEWER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slot = spec
            .attr("src")
            .or_else(|| spec.attr("data-src"))
            .filter(|value| !value.is_empty())
            .unwrap_or(spec.value);
        let content = if slot.is_empty() {
            ImageViewerContent::None
        } else {
            ImageViewerContent::host_texture(slot)
        };
        let mut component = ImageViewer::new(content);
        if !spec.display_label().is_empty() {
            component = component.name(Arc::<str>::from(spec.display_label()));
        }
        if !spec.hint.is_empty() {
            component = component.metadata(Arc::<str>::from(spec.hint));
        }
        component
    }
}

#[cfg(feature = "rich-text")]
impl RegisterableComponent for NativeMarkdown {
    const TYPE_ID: &'static str = crate::component_descriptors::NATIVE_MARKDOWN.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::NATIVE_MARKDOWN.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        NativeMarkdown::from_source(markdown_source_from_spec(spec)).style(layout_only_style(spec))
    }
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        if let Some(previous) =
            previous.filter(|previous| previous.source() == Some(markdown_source_from_spec(spec)))
        {
            previous.clone().style(layout_only_style(spec))
        } else {
            Self::from_semantic(spec)
        }
    }
    fn finish_semantic(
        context: &mut crate::AppContext,
        entity: crate::Entity<Self>,
    ) -> Result<(), FrameworkError> {
        context.assemble_markdown(entity).map(|_| ())
    }
    fn project_semantic(
        &self,
        spec: &SemanticSpec<'_>,
        id: crate::StableNodeId,
        world: &crate::UiWorld,
        mutations: &mut crate::MutationQueue,
    ) {
        crate::ComponentView::project(self, id, world, mutations);
        let request = self.blocks().iter().find_map(|block| {
            let (source, kind, names) = match block {
                crate::MarkdownBlock::Mermaid(source) => (
                    source,
                    "mermaid",
                    [
                        "mermaid-renderer",
                        "mermaidrenderer",
                        "data-mermaid-renderer",
                    ],
                ),
                crate::MarkdownBlock::DisplayMath(source) => (
                    source,
                    "math",
                    ["math-renderer", "mathrenderer", "data-math-renderer"],
                ),
                _ => return None,
            };
            names
                .into_iter()
                .find_map(|name| spec.attr(name))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| crate::HighlightRequest::new(name, format!("{kind}:{source}")))
        });
        if world.highlight_request(id) != request.as_ref() {
            mutations.set_highlight_request(id, request);
        }
    }
}

#[cfg(feature = "graph-canvas")]
impl RegisterableComponent for GraphCanvas {
    const TYPE_ID: &'static str = crate::component_descriptors::GRAPH_CANVAS.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::GRAPH_CANVAS.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component =
            GraphCanvas::new("main", graph_model_from_spec(spec)).disabled(spec.disabled);
        if let Some(viewport) = graph_viewport_from_spec(spec) {
            component = component.viewport(viewport);
        }
        if let Some(selection) = graph_selection_from_spec(spec) {
            component = component.selection(Some(selection));
        }
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        for (name, id) in spec.slots {
            let Some(node) = crate::graph_canvas::graph_node_slot_name(name, &component.model)
            else {
                continue;
            };
            component = component.node_content(node, crate::GraphNodeContent::region(*id));
        }
        let host_slots: Vec<_> = component
            .model
            .nodes()
            .iter()
            .filter_map(|node| {
                let slot = node.content_slot.as_deref()?.trim();
                (!slot.is_empty()).then(|| (node.id.clone(), slot.to_string()))
            })
            .collect();
        for (node, slot) in host_slots {
            if component
                .node_contents()
                .iter()
                .any(|(existing, _)| existing == &node)
            {
                continue;
            }
            component = component.node_content(node, crate::GraphNodeContent::host_texture(slot));
        }
        component
    }
}

impl RegisterableComponent for Workspace {
    const TYPE_ID: &'static str = crate::component_descriptors::WORKSPACE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::WORKSPACE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slots = spec
            .slots
            .iter()
            .filter_map(|(name, id)| {
                region_id_from_token(name).map(|region| WorkspaceRegionSlot::new(region, *id))
            })
            .collect::<Vec<_>>();
        Workspace::from_model(&WorkspaceModel::new(), slots)
    }
}

impl RegisterableComponent for Dock {
    const TYPE_ID: &'static str = crate::component_descriptors::DOCK.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DOCK.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        Dock::new(dock_root_from_spec(spec))
    }
}

impl RegisterableComponent for SplitPane {
    const TYPE_ID: &'static str = crate::component_descriptors::SPLIT_PANE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SPLIT_PANE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let default_size = attr_f32(spec, &["default-size", "defaultsize", "defaultSize"])
            .or_else(|| attr_f32(spec, &["size"]))
            .unwrap_or(crate::split_pane::DEFAULT_SIZE);
        let min = attr_f32(spec, &["min"]).unwrap_or(crate::split_pane::DEFAULT_MIN);
        let max = attr_f32(spec, &["max"]).unwrap_or(crate::split_pane::DEFAULT_MAX);
        let mut model =
            SplitPaneModel::new(parse_split_axis(spec.attr("axis")), default_size, min, max);
        if let Some(size) = attr_f32(spec, &["size"])
            && (size - model.size()).abs() > f32::EPSILON
        {
            model.update(SplitPaneMutation::SetSize(size));
        }
        SplitPane {
            first: spec.slot("first"),
            second: spec.slot("second"),
            handle: spec.slot("handle"),
            first_slot: None,
            second_slot: None,
            model,
            style: NodeStyle::default(),
        }
    }
}

impl RegisterableComponent for AppShell {
    const TYPE_ID: &'static str = crate::component_descriptors::APP_SHELL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::APP_SHELL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = AppShell::new();
        if let Some(title_bar) = spec.slot("title-bar").or_else(|| spec.slot("title_bar")) {
            component = component.title_bar(title_bar);
        }
        if let Some(body) = spec.slot("body") {
            component = component.body(body);
        }
        if let Some(overlay) = spec.slot("overlay") {
            component = component.overlay(overlay);
        }
        component
    }
}

impl RegisterableComponent for SidebarFrame {
    const TYPE_ID: &'static str = crate::component_descriptors::SIDEBAR_FRAME.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SIDEBAR_FRAME.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = SidebarFrame::new().gap(spec.layout.gap_or(14.0));
        if let Some(top) = spec.slot("top") {
            component = component.top(top);
        }
        if let Some(body) = spec.slot("body") {
            component = component.body(body);
        }
        if let Some(footer) = spec.slot("footer") {
            component = component.footer(footer);
        }
        component.style.layout = Arc::clone(spec.layout);
        component
    }
}

impl RegisterableComponent for SidebarRow {
    const TYPE_ID: &'static str = crate::component_descriptors::SIDEBAR_ROW.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SIDEBAR_ROW.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = SidebarRow::new(spec.display_label())
            .slots(ListItemSlots {
                leading: spec.slot("leading"),
                content: spec.slot("content"),
                trailing: spec.slot("trailing"),
            })
            .gap(spec.layout.gap_or(6.0))
            .size(spec.size)
            .state(parse_sidebar_row_state(spec))
            .tone(parse_sidebar_row_tone(spec))
            .depth(attr_u16(spec, &["depth", "data-depth", "indent"]).unwrap_or(0));
        if let Some(tools) = spec.slot("tools") {
            component = component.tools(tools);
        }
        if let Some(expanded) =
            parse_tristate_attr(spec, &["expanded", "data-expanded", "disclosure"])
        {
            component = component.disclosure(expanded);
        }
        component
    }
}

impl RegisterableComponent for SettingsRow {
    // Responsive layout reprojects after the row's actual width is available.
    const RETAIN_SEMANTIC_STATE: bool = true;
    const TYPE_ID: &'static str = crate::component_descriptors::SETTINGS_ROW.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SETTINGS_ROW.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = SettingsRow::new(spec.display_label())
            .stacked(flag_attr(spec, &["stacked"]))
            .divided(flag_attr(spec, &["divided"]))
            .loose(flag_attr(spec, &["loose"]));
        if let Some(width) = attr_f32(spec, &["stack-below", "stackBelow"]) {
            component = component.stack_below(width);
        }
        if !spec.hint.is_empty() {
            component = component.hint(Arc::<str>::from(spec.hint));
        }
        if flag_attr(spec, &["first-in-group", "firstInGroup", "first"]) {
            component = component.first_in_group();
        }
        if flag_attr(spec, &["last-in-group", "lastInGroup", "last"]) {
            component = component.last_in_group();
        }
        if let Some(control) = spec.slot("control") {
            component = component.control_child(control);
        }
        if let Some(copy) = spec.slot("copy") {
            component = component.copy_slot(copy);
        }
        if let Some(label) = spec.slot("label") {
            component = component.label_slot(label);
        }
        if let Some(hint) = spec.slot("hint") {
            component = component.hint_slot(hint);
        }
        component
    }
}

impl RegisterableComponent for SettingsCard {
    const TYPE_ID: &'static str = crate::component_descriptors::SETTINGS_CARD.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SETTINGS_CARD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let title = if spec.slot("title").is_some() {
            ""
        } else {
            spec.display_label()
        };
        SettingsCard::new(title).style(layout_only_style(spec))
    }
}

impl RegisterableComponent for SettingsPage {
    const TYPE_ID: &'static str = crate::component_descriptors::SETTINGS_PAGE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SETTINGS_PAGE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let model = settings_model_from_spec(spec).unwrap_or_else(|| fallback_settings_model(spec));
        let mut state = SettingsState::new(&model);
        if let Some(tab) = spec
            .attr("tab")
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!spec.value.trim().is_empty()).then_some(spec.value))
        {
            state.select(&model, &SettingsTabId::from(tab.trim()));
        }
        let mut component = SettingsPage::new(model, state);
        if let Some(raw) = spec
            .attr("content-padding")
            .or_else(|| spec.attr("contentPadding"))
        {
            let padding = serde_json::from_str::<f32>(raw)
                .map(nana_ui_core::PaddingSpec::uniform)
                .or_else(|_| serde_json::from_str::<nana_ui_core::PaddingSpec>(raw))
                .ok()
                .filter(|p| {
                    [p.top, p.right, p.bottom, p.left]
                        .iter()
                        .all(|v| v.is_finite())
                });
            if let Some(padding) = padding {
                component = component.content_padding(padding);
            }
        }
        if let Some(gap) = attr_f32(spec, &["content-gap", "contentGap"]).filter(|v| v.is_finite())
        {
            component = component.content_gap(gap);
        }
        if let Some(content) = spec.slot("content").or_else(|| spec.slot("body")) {
            component = component.content(content);
        }
        component
    }
}

impl RegisterableComponent for SettingsCollapsibleCard {
    const TYPE_ID: &'static str = crate::component_descriptors::SETTINGS_COLLAPSIBLE_CARD.type_id;
    const TAGS: &'static [&'static str] =
        crate::component_descriptors::SETTINGS_COLLAPSIBLE_CARD.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = SettingsCollapsibleCard::new(spec.active || spec.toggled)
            .disabled(spec.disabled)
            .style(layout_only_style(spec));
        if let Some(summary) = spec.slot("summary").or_else(|| spec.slot("header")) {
            component = component.summary(summary);
        }
        if let Some(details) = spec.slot("details").or_else(|| spec.slot("body")) {
            component = component.details(details);
        }
        if let Some(accessory) = spec.slot("accessory") {
            component = component.accessory(accessory);
        }
        component
    }
}

impl RegisterableComponent for List {
    const TYPE_ID: &'static str = crate::component_descriptors::LIST.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::LIST.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = List::new().style(layout_only_style(spec));
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

impl RegisterableComponent for ScrollView {
    const TYPE_ID: &'static str = crate::component_descriptors::SCROLL_VIEW.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SCROLL_VIEW.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = ScrollView::new(parse_scroll_axes(spec))
            .scrollbars(parse_scrollbar_visibility(spec))
            .style(layout_only_style(spec));
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

impl RegisterableComponent for Table {
    const TYPE_ID: &'static str = crate::component_descriptors::TABLE.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TABLE.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = Table::new().style(layout_only_style(spec));
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

impl RegisterableComponent for TableRow {
    const TYPE_ID: &'static str = crate::component_descriptors::TABLE_ROW.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TABLE_ROW.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        TableRow::new()
            .selected(spec.active || spec.toggled)
            .style(layout_only_style(spec))
    }
}

/// `th` and `td` share one type, so the header flag comes from an attribute
/// rather than the tag.
impl RegisterableComponent for TableCell {
    const TYPE_ID: &'static str = crate::component_descriptors::TABLE_CELL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TABLE_CELL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = TableCell::new(spec.display_label())
            .column_header(flag_attr(
                spec,
                &["header", "column-header", "columnheader"],
            ))
            .selected(spec.active || spec.toggled);
        let style = layout_only_style(spec);
        if style.layout.as_ref() != &nana_ui_core::LayoutStyle::default() {
            component = component.style(style);
        }
        component
    }
}

#[cfg(feature = "controls")]
impl RegisterableComponent for ReorderList {
    const TYPE_ID: &'static str = crate::component_descriptors::REORDER_LIST.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::REORDER_LIST.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let items = spec
            .options
            .iter()
            .map(|option| ReorderItem::new(option.value, option.label).disabled(option.disabled))
            .collect::<Vec<_>>();
        let mut component = ReorderList::new(items)
            .size(spec.size)
            .tree_drop(flag_attr(spec, &["tree-drop", "treedrop"]))
            .style(layout_only_style(spec));
        if let Some(spacing) = attr_f32(spec, &["spacing", "gap"]) {
            component = component.spacing(spacing);
        }
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

#[cfg(feature = "charts")]
impl RegisterableComponent for TimeSeriesChart {
    const RETAIN_SEMANTIC_STATE: bool = true;
    const TYPE_ID: &'static str = crate::component_descriptors::TIME_SERIES_CHART.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::TIME_SERIES_CHART.tags;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut component = Self::from_semantic(spec);
        component.active = previous
            .and_then(|previous| previous.active)
            .filter(|&index| !component.layers.is_empty() && index < component.values.len());
        component
    }
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = TimeSeriesChart::new(time_series_values_from_spec(spec))
            .style(layout_only_style(spec))
            .axis_labels(chart_strings(spec, &["axis-labels", "axisLabels"]))
            .tooltip_details(chart_strings(spec, &["tooltip-details", "tooltipDetails"]));
        if let Some(layers) = spec_json(spec, &["layers"]) {
            component = component.stacked(json_array(&layers).into_iter().enumerate().map(
                |(i, layer)| {
                    crate::TimeSeriesLayer::new(
                        layer.get("label").map(json_text).unwrap_or_default(),
                        layer
                            .get("values")
                            .map(json_array)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|value| chart_value(value).unwrap_or(0.0)),
                        chart_color(layer.get("color"), i),
                    )
                },
            ));
        }
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        component
    }
}

#[cfg(feature = "charts")]
impl RegisterableComponent for DonutChart {
    const RETAIN_SEMANTIC_STATE: bool = true;
    const TYPE_ID: &'static str = crate::component_descriptors::DONUT_CHART.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DONUT_CHART.tags;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let mut component = Self::from_semantic(spec);
        component.active = previous
            .and_then(|previous| previous.active)
            .filter(|&index| {
                component
                    .slices
                    .get(index)
                    .is_some_and(|slice| slice.value.is_finite() && slice.value > 0.0)
            });
        component
    }
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let slices = if let Some(slices) = spec_json(spec, &["slices"]) {
            json_array(&slices)
                .into_iter()
                .enumerate()
                .map(|(i, slice)| DonutSlice {
                    value: slice.get("value").and_then(chart_value).unwrap_or(0.0),
                    color: chart_color(slice.get("color"), i),
                })
                .collect()
        } else {
            time_series_values_from_spec(spec)
                .into_iter()
                .enumerate()
                .map(|(i, value)| DonutSlice {
                    value,
                    color: chart_color(None, i),
                })
                .collect::<Vec<_>>()
        };
        let mut component = DonutChart::new(slices)
            .labels(chart_strings(spec, &["labels"]))
            .style(layout_only_style(spec));
        if !spec.display_label().is_empty() {
            component = component.label(Arc::<str>::from(spec.display_label()));
        }
        if let Some(cutout) = attr_f32(spec, &["cutout"]) {
            component = component.cutout(cutout);
        }
        component
    }
}

#[cfg(feature = "charts")]
fn chart_color(value: Option<&serde_json::Value>, index: usize) -> nana_ui_core::SemanticColorRole {
    use nana_ui_core::SemanticColorRole as R;
    const ROLES: [R; 5] = [R::Accent, R::Success, R::Warning, R::Text, R::Muted];
    value
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or(ROLES[index % ROLES.len()])
}
#[cfg(feature = "charts")]
fn chart_value(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}
#[cfg(feature = "charts")]
fn chart_strings(spec: &SemanticSpec<'_>, names: &[&str]) -> Vec<String> {
    spec_json(spec, names)
        .map(|value| json_array(&value).into_iter().map(json_text).collect())
        .unwrap_or_default()
}

impl RegisterableComponent for DesktopShell {
    const TYPE_ID: &'static str = crate::component_descriptors::DESKTOP_SHELL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::DESKTOP_SHELL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = DesktopShell::new();
        if let Some(title_bar) = spec.slot("title-bar").or_else(|| spec.slot("title_bar")) {
            component = component.title_bar(title_bar);
        } else if !spec.display_label().is_empty() {
            component = component.title(Arc::<str>::from(spec.display_label()));
        }
        if let Some(primary) = spec.slot("primary").or_else(|| spec.slot("main")) {
            component = component.primary(primary);
        }
        if let Some(navigation) = spec.slot("navigation").or_else(|| spec.slot("nav")) {
            component = component.navigation(navigation);
        }
        if let Some(footer) = spec
            .slot("navigation-footer")
            .or_else(|| spec.slot("navigation_footer"))
        {
            component = component.navigation_footer(footer);
        }
        if let Some(inspector) = spec.slot("inspector") {
            component = component.inspector(inspector);
        }
        if let Some(bottom) = spec.slot("bottom") {
            component = component.bottom(bottom);
        }
        if let Some(overlay) = spec.slot("overlay") {
            component = component.overlay(overlay);
        }
        component.style = layout_only_style(spec);
        component
    }
}

impl RegisterableComponent for AppTitleBar {
    const TYPE_ID: &'static str = crate::component_descriptors::APP_TITLE_BAR.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::APP_TITLE_BAR.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = AppTitleBar::new(spec.display_label())
            .transparent(flag_attr(spec, &["transparent"]))
            .drag_enabled(
                parse_tristate_attr(spec, &["drag-enabled", "dragenabled"]).unwrap_or(true),
            )
            .maximized(flag_attr(spec, &["maximized"]))
            .style(layout_only_style(spec));
        if let Some(show) = parse_tristate_attr(spec, &["window-controls", "windowcontrols"]) {
            component = component.show_window_controls(show);
        }
        if let Some(width) = attr_f32(spec, &["center-width", "centerwidth"]) {
            component = component.center_width(width);
        }
        if let Some(leading) = spec.slot("leading") {
            component = component.leading(leading);
        }
        if let Some(center) = spec.slot("center") {
            component = component.center(center);
        }
        if let Some(trailing) = spec.slot("trailing") {
            component = component.trailing(trailing);
        }
        if let Some(controls) = spec.slot("controls") {
            component = component.controls(controls);
        }
        component
    }
}

impl RegisterableComponent for PaneChrome {
    const TYPE_ID: &'static str = crate::component_descriptors::PANE_CHROME.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::PANE_CHROME.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = PaneChrome::new()
            .active(!spec.disabled)
            .style(layout_only_style(spec));
        if let Some(tabs) = spec.slot("tabs") {
            component = component.tabs(tabs);
        }
        if let Some(body) = spec.slot("body") {
            component = component.body(body);
        }
        if let Some(header) = spec.slot("header") {
            component = component.header(header);
        }
        component
    }
}

impl RegisterableComponent for SidebarSection {
    const TYPE_ID: &'static str = crate::component_descriptors::SIDEBAR_SECTION.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SIDEBAR_SECTION.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let mut component = SidebarSection::new(spec.display_label())
            .size(spec.size)
            .collapsible(flag_attr(spec, &["collapsible"]))
            .disabled(spec.disabled)
            .expanded(parse_tristate_attr(spec, &["expanded", "data-expanded"]).unwrap_or(true))
            .style(layout_only_style(spec));
        if let Some(count) = spec
            .attr("count")
            .and_then(|raw| raw.trim().parse::<usize>().ok())
        {
            component = component.count(count);
        }
        if !spec.hint.is_empty() {
            component = component.empty_text(Arc::<str>::from(spec.hint));
        }
        if let Some(tools) = spec.slot("tools") {
            component = component.tools(tools);
        }
        if let Some(header) = spec.slot("header") {
            component = component.header(header);
        }
        if let Some(body) = spec.slot("body") {
            component = component.body(body);
        }
        component
    }
}

impl RegisterableComponent for SidebarFooter {
    const TYPE_ID: &'static str = crate::component_descriptors::SIDEBAR_FOOTER.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::SIDEBAR_FOOTER.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        SidebarFooter::new().style(layout_only_style(spec))
    }
}

/// Carries only the spec's layout into a component style. Theme and text
/// presentation stay with the component default.
fn layout_only_style(spec: &SemanticSpec<'_>) -> crate::NodeStyle {
    crate::NodeStyle {
        layout: Arc::clone(spec.layout),
        ..crate::NodeStyle::default()
    }
}

fn parse_scroll_axes(spec: &SemanticSpec<'_>) -> crate::ScrollAxes {
    match spec
        .attr("axes")
        .or_else(|| spec.attr("axis"))
        .or_else(|| spec.attr("direction"))
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "horizontal" | "x" | "row" => crate::ScrollAxes::Horizontal,
        "both" | "xy" | "all" => crate::ScrollAxes::Both,
        _ => crate::ScrollAxes::Vertical,
    }
}

/// Treat a bare attribute (`indeterminate`) as true, like HTML boolean attrs.
fn parse_flag(raw: Option<&str>) -> bool {
    match raw.map(str::trim) {
        None => false,
        Some("") => true,
        Some(value) => !matches!(
            value.to_ascii_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
    }
}

fn parse_scrollbar_visibility(spec: &SemanticSpec<'_>) -> nana_ui_core::ScrollbarVisibility {
    match spec
        .attr("scrollbars")
        .or_else(|| spec.attr("scrollbar"))
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "always" | "visible" | "persistent" => nana_ui_core::ScrollbarVisibility::Always,
        "hidden" | "none" | "off" => nana_ui_core::ScrollbarVisibility::Hidden,
        _ => nana_ui_core::ScrollbarVisibility::AutoHide,
    }
}

#[cfg(feature = "charts")]
fn time_series_values_from_spec(spec: &SemanticSpec<'_>) -> Vec<f64> {
    if let Some(value) = spec_json(spec, &["values", "data", "series"]) {
        let values = json_array(&value)
            .into_iter()
            .map(|value| chart_value(value).unwrap_or(0.0))
            .collect::<Vec<_>>();
        if !values.is_empty() {
            return values;
        }
    }
    spec.value
        .split([',', ' ', '\n', '\t'])
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .collect()
}

fn textarea_placeholder<'a>(spec: &'a SemanticSpec<'_>) -> &'a str {
    if spec.placeholder.is_empty() {
        spec.hint
    } else {
        spec.placeholder
    }
}

fn highlight_language_from_spec<'a>(spec: &'a SemanticSpec<'_>) -> Option<&'a str> {
    spec.attr("language")
        .or_else(|| spec.attr("lang"))
        .or_else(|| spec.attr("syntax"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn apply_textarea_gutter_from_spec(area: &mut TextArea, spec: &SemanticSpec<'_>) {
    area.line_numbers = flag_attr(spec, &["line-numbers", "lineNumbers"]);
    area.relative_line_numbers = flag_attr(spec, &["relative-line-numbers", "relativeLineNumbers"]);
    area.minimap = flag_attr(spec, &["minimap"]);
    area.diagnostics = diagnostics_from_spec(spec);
    area.git_gutter = git_gutter_from_spec(spec);
}

fn json_objects(
    spec: &SemanticSpec<'_>,
    names: &[&str],
) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let Some(raw) = names.iter().find_map(|name| spec.attr(name)) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object)
        .cloned()
        .collect()
}

fn diagnostics_from_spec(spec: &SemanticSpec<'_>) -> Arc<[TextDiagnosticSpan]> {
    json_objects(spec, &["diagnostics"])
        .into_iter()
        .filter_map(|object| {
            let offset = json_u64(object.get("offset")?)? as usize;
            let length = object.get("length").and_then(json_u64).unwrap_or(0) as usize;
            let severity = match object
                .get("severity")
                .and_then(|value| value.as_str())
                .unwrap_or("error")
            {
                "warning" | "warn" => TextDiagnosticSeverity::Warning,
                "information" | "info" => TextDiagnosticSeverity::Information,
                "hint" => TextDiagnosticSeverity::Hint,
                _ => TextDiagnosticSeverity::Error,
            };
            let message = object
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_owned();
            Some(TextDiagnosticSpan::new(offset, length, severity).with_message(message))
        })
        .collect::<Vec<_>>()
        .into()
}

fn git_gutter_from_spec(spec: &SemanticSpec<'_>) -> Arc<[TextGitMark]> {
    json_objects(spec, &["git-gutter", "gitGutter"])
        .into_iter()
        .filter_map(|object| {
            let line = json_u64(object.get("line")?)? as u32;
            let kind = match object.get("kind").and_then(|value| value.as_str()) {
                Some("added" | "add") => TextGitMarkKind::Added,
                Some("deleted" | "removed") => TextGitMarkKind::Deleted,
                _ => TextGitMarkKind::Modified,
            };
            Some(TextGitMark::new(line, kind))
        })
        .collect::<Vec<_>>()
        .into()
}

fn truthy_attr(value: &str) -> bool {
    let value = value.trim();
    !(value.eq_ignore_ascii_case("false") || value == "0")
}

fn flag_attr(spec: &SemanticSpec<'_>, names: &[&str]) -> bool {
    names
        .iter()
        .find_map(|name| spec.attr(name))
        .is_some_and(truthy_attr)
}

pub(crate) fn parse_tristate_attr(spec: &SemanticSpec<'_>, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| spec.attr(name))
        .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
}

fn attr_u16(spec: &SemanticSpec<'_>, names: &[&str]) -> Option<u16> {
    names
        .iter()
        .find_map(|name| spec.attr(name))
        .and_then(|raw| raw.trim().parse().ok())
}

fn parse_drawer_side(raw: Option<&str>) -> DrawerSide {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "left" | "start" => DrawerSide::Left,
        "bottom" => DrawerSide::Bottom,
        _ => DrawerSide::Right,
    }
}

fn parse_sidebar_row_state(spec: &SemanticSpec<'_>) -> SidebarRowState {
    if spec.disabled {
        return SidebarRowState::Disabled;
    }
    match spec
        .attr("state")
        .or_else(|| spec.attr("data-state"))
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "disabled" => SidebarRowState::Disabled,
        "active" => SidebarRowState::Active,
        "ancestor" | "ancestor-active" | "ancestoractive" => SidebarRowState::AncestorActive,
        "idle" => SidebarRowState::Idle,
        _ if spec.active => SidebarRowState::Active,
        _ => SidebarRowState::Idle,
    }
}

fn parse_sidebar_row_tone(spec: &SemanticSpec<'_>) -> SidebarRowTone {
    match spec
        .attr("tone")
        .or_else(|| spec.attr("data-tone"))
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "warning" | "warn" => SidebarRowTone::Warning,
        "error" | "danger" => SidebarRowTone::Error,
        _ => SidebarRowTone::Default,
    }
}

fn parse_control_position(raw: Option<&str>) -> SwitchControlPosition {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "start" | "left" => SwitchControlPosition::Start,
        _ => SwitchControlPosition::End,
    }
}

fn parse_card_kind(raw: Option<&str>) -> CardKind {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "outlined" | "outline" => CardKind::Outlined,
        "raised" | "elevated" => CardKind::Raised,
        "flat" => CardKind::Flat,
        "selected" => CardKind::Selected,
        _ => CardKind::Surface,
    }
}

fn parse_status_tone(raw: Option<&str>) -> StatusTone {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "info" => StatusTone::Info,
        "success" => StatusTone::Success,
        "warning" | "warn" => StatusTone::Warning,
        "danger" | "error" => StatusTone::Danger,
        _ => StatusTone::Neutral,
    }
}

fn parse_toast_tone(raw: Option<&str>) -> ToastTone {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "success" => ToastTone::Success,
        "warning" | "warn" => ToastTone::Warning,
        "danger" | "error" => ToastTone::Danger,
        _ => ToastTone::Info,
    }
}

fn parse_validation_intent(raw: Option<&str>) -> ValidationIntent {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "danger" | "error" => ValidationIntent::Danger,
        _ => ValidationIntent::Warning,
    }
}

fn validation_intent_from_spec(spec: &SemanticSpec<'_>) -> ValidationIntent {
    if spec.invalid {
        ValidationIntent::Danger
    } else {
        parse_validation_intent(spec.attr("intent").or_else(|| spec.attr("data-intent")))
    }
}

fn action_menu_item_danger(spec: &SemanticSpec<'_>) -> bool {
    spec.invalid
        || spec.button_kind == ButtonKind::Danger
        || spec.attr("danger").is_some_and(truthy_attr)
        || spec
            .attr("intent")
            .or_else(|| spec.attr("data-intent"))
            .or_else(|| spec.attr("data-variant"))
            .is_some_and(|value| value.eq_ignore_ascii_case("danger"))
}

fn attr_f32(spec: &SemanticSpec<'_>, names: &[&str]) -> Option<f32> {
    names
        .iter()
        .find_map(|name| spec.attr(name))
        .and_then(|raw| raw.trim().parse().ok())
}

fn qr_modules_from_spec(spec: &SemanticSpec<'_>) -> Option<(Arc<[bool]>, usize)> {
    let raw = spec.attr("modules").or_else(|| spec.attr("data-modules"))?;
    let width_hint = spec
        .attr("module-width")
        .or_else(|| spec.attr("modules-width"))
        .or_else(|| spec.attr("data-module-width"))
        .and_then(|raw| raw.trim().parse().ok());
    parse_qr_module_matrix(raw, width_hint)
}

fn parse_qr_module_matrix(raw: &str, width_hint: Option<usize>) -> Option<(Arc<[bool]>, usize)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cells: Vec<bool> = if trimmed.chars().all(|c| c == '0' || c == '1') {
        trimmed.chars().map(|c| c == '1').collect()
    } else {
        trimmed
            .split(['[', ']', ',', ';', ' ', '\n', '\t', '\r'])
            .filter(|part| !part.is_empty())
            .map(|part| match part {
                "1" | "true" | "TRUE" => Some(true),
                "0" | "false" | "FALSE" => Some(false),
                _ => None,
            })
            .collect::<Option<_>>()?
    };
    if cells.is_empty() {
        return None;
    }
    let width = width_hint.or_else(|| {
        let root = (cells.len() as f64).sqrt() as usize;
        (root > 0 && root.saturating_mul(root) == cells.len()).then_some(root)
    })?;
    if width == 0 || width.checked_mul(width) != Some(cells.len()) {
        return None;
    }
    Some((Arc::<[bool]>::from(cells), width))
}

fn qr_placeholder() -> QrCode {
    QrCode::from_modules(vec![true], 1, QrCode::DEFAULT_SIZE).unwrap_or_else(|_| QrCode {
        modules: Arc::from([true].as_slice()),
        width: 1,
        size: QrCode::DEFAULT_SIZE,
        label: Arc::from("QR code"),
    })
}

fn spec_json(spec: &SemanticSpec<'_>, names: &[&str]) -> Option<serde_json::Value> {
    names
        .iter()
        .find_map(|name| spec.attr(name))
        .and_then(parse_json_value)
}

fn parse_json_value(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn json_object_get<'a>(
    value: &'a serde_json::Value,
    names: &[&str],
) -> Option<&'a serde_json::Value> {
    let object = value.as_object()?;
    for name in names {
        if let Some(found) = object.get(*name) {
            return Some(found);
        }
    }
    for name in names {
        if let Some((_, found)) = object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            return Some(found);
        }
    }
    None
}

fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

fn json_object_text(value: &serde_json::Value, names: &[&str]) -> String {
    json_object_get(value, names)
        .map(json_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn json_f32(value: &serde_json::Value) -> Option<f32> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().map(|value| value as f32),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        serde_json::Value::Bool(true) => Some(1.0),
        serde_json::Value::Bool(false) => Some(0.0),
        _ => None,
    }
    .filter(|number| number.is_finite())
}

fn json_object_f32(value: &serde_json::Value, names: &[&str]) -> Option<f32> {
    json_object_get(value, names).and_then(json_f32)
}

fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        serde_json::Value::String(text) => {
            !text.is_empty() && !text.eq_ignore_ascii_case("false") && text != "0"
        }
        serde_json::Value::Null => false,
        _ => true,
    }
}

fn json_object_truthy(value: &serde_json::Value, names: &[&str]) -> bool {
    json_object_get(value, names).is_some_and(json_truthy)
}

fn json_array(value: &serde_json::Value) -> Vec<&serde_json::Value> {
    match value {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(map) => {
            let mut indexed = map
                .iter()
                .filter_map(|(key, item)| key.parse::<usize>().ok().map(|index| (index, item)))
                .collect::<Vec<_>>();
            if indexed.is_empty() {
                return Vec::new();
            }
            indexed.sort_by_key(|(index, _)| *index);
            indexed.into_iter().map(|(_, item)| item).collect()
        }
        _ => Vec::new(),
    }
}

fn command_palette_items_from_json(value: &serde_json::Value) -> Vec<CommandPaletteItem> {
    json_array(value)
        .into_iter()
        .filter_map(command_palette_item_from_json)
        .collect()
}

fn command_palette_item_from_json(value: &serde_json::Value) -> Option<CommandPaletteItem> {
    match value {
        serde_json::Value::Object(_) => {
            let action = json_object_text(value, &["value", "action", "id", "key"]);
            let label = json_object_text(value, &["label", "title", "text"]);
            if action.is_empty() && label.is_empty() {
                return None;
            }
            let identity = if action.is_empty() {
                label.clone()
            } else {
                action
            };
            let caption = if label.is_empty() {
                identity.clone()
            } else {
                label
            };
            let mut item = CommandPaletteItem::new(identity, caption);
            let category = json_object_text(value, &["category", "group"]);
            if !category.is_empty() {
                item = item.category(category);
            }
            let shortcut = json_object_text(value, &["shortcut", "keys"]);
            if !shortcut.is_empty() {
                item = item.shortcut(shortcut);
            }
            Some(item)
        }
        serde_json::Value::String(text) if !text.is_empty() => {
            Some(CommandPaletteItem::new(text.as_str(), text.clone()))
        }
        _ => None,
    }
}

fn tree_nodes_from_spec(spec: &SemanticSpec<'_>) -> Vec<TreeNode<Arc<str>>> {
    if let Some(value) = spec_json(spec, &["tree", "nodes", "options"]) {
        let nodes = json_array(&value)
            .into_iter()
            .filter_map(tree_node_from_json)
            .collect::<Vec<_>>();
        if !nodes.is_empty() {
            return nodes;
        }
    }
    spec.options
        .iter()
        .map(|option| {
            TreeNode::leaf(Arc::<str>::from(option.value), option.label)
                .disabled(option.disabled)
                .selected(option.value == spec.value && !spec.value.is_empty())
        })
        .collect()
}

fn tree_node_from_json(value: &serde_json::Value) -> Option<TreeNode<Arc<str>>> {
    match value {
        serde_json::Value::Object(_) => {
            let id = json_object_text(value, &["value", "id", "key"]);
            let label = json_object_text(value, &["label", "title", "text"]);
            if id.is_empty() && label.is_empty() {
                return None;
            }
            let children = json_object_get(value, &["children"])
                .map(json_array)
                .unwrap_or_default()
                .into_iter()
                .filter_map(tree_node_from_json)
                .collect::<Vec<_>>();
            let expanded = json_object_truthy(value, &["expanded"]);
            let selected = json_object_truthy(value, &["selected"]);
            let disabled = json_object_truthy(value, &["disabled"]);
            let identity = if id.is_empty() { label.clone() } else { id };
            let caption = if label.is_empty() {
                identity.clone()
            } else {
                label
            };
            let mut node = if children.is_empty() {
                TreeNode::leaf(Arc::<str>::from(identity.as_str()), caption)
            } else {
                TreeNode::branch(
                    Arc::<str>::from(identity.as_str()),
                    caption,
                    expanded,
                    children,
                )
            };
            node = node.selected(selected).disabled(disabled);
            if let Some(icon) = Icon::parse_name(&json_object_text(value, &["icon"])) {
                node = node.icon(icon);
            }
            Some(node)
        }
        serde_json::Value::String(text) if !text.is_empty() => Some(TreeNode::leaf(
            Arc::<str>::from(text.as_str()),
            text.clone(),
        )),
        _ => None,
    }
}

#[cfg(feature = "calendar")]
fn calendar_data_from_spec(spec: &SemanticSpec<'_>) -> Vec<CalendarHeatmapDatum<()>> {
    let candidates = [
        spec_json(spec, &["data"]),
        spec_json(spec, &["options"]),
        parse_json_value(spec.value),
    ];
    for value in candidates.into_iter().flatten() {
        if is_calendar_data_json(&value) {
            return json_array(&value)
                .into_iter()
                .filter_map(calendar_datum_from_json)
                .collect();
        }
    }
    Vec::new()
}

#[cfg(feature = "calendar")]
fn is_calendar_data_json(value: &serde_json::Value) -> bool {
    json_array(value)
        .into_iter()
        .any(|item| calendar_datum_from_json(item).is_some())
}

#[cfg(feature = "calendar")]
fn calendar_datum_from_json(value: &serde_json::Value) -> Option<CalendarHeatmapDatum<()>> {
    match value {
        serde_json::Value::Object(_) => {
            let date = json_object_text(value, &["date", "day", "key"]);
            if date.is_empty() {
                return None;
            }
            let number = json_object_get(value, &["value", "count"])
                .and_then(json_f32)
                .unwrap_or(0.0);
            Some(CalendarHeatmapDatum::new(date, number))
        }
        serde_json::Value::Array(items) if items.len() >= 2 => {
            let date = json_text(&items[0]);
            let number = json_f32(&items[1])?;
            if date.is_empty() {
                return None;
            }
            Some(CalendarHeatmapDatum::new(date, number))
        }
        _ => None,
    }
}

#[cfg(feature = "calendar")]
fn calendar_options_from_spec(spec: &SemanticSpec<'_>) -> CalendarHeatmapOptions<()> {
    let Some(value) = spec_json(spec, &["options"]) else {
        return CalendarHeatmapOptions::default();
    };
    if !value.is_object() || is_calendar_data_json(&value) {
        return CalendarHeatmapOptions::default();
    }
    calendar_options_from_json(&value)
}

#[cfg(feature = "calendar")]
fn calendar_options_from_json(value: &serde_json::Value) -> CalendarHeatmapOptions<()> {
    let mut options = CalendarHeatmapOptions::default();
    if let Some(size) = json_object_f32(value, &["cellSize", "cell_size", "cell-size"]) {
        options.cell_size = size;
    }
    if let Some(gap) = json_object_f32(value, &["cellGap", "cell_gap", "cell-gap"]) {
        options.cell_gap = gap;
    }
    if let Some(radius) = json_object_f32(value, &["cellRadius", "cell_radius", "cell-radius"]) {
        options.cell_radius = radius;
    }
    if let Some(width) = json_object_f32(value, &["labelWidth", "label_width", "label-width"]) {
        options.label_width = width;
    }
    if let Some(height) = json_object_f32(
        value,
        &[
            "monthLabelHeight",
            "month_label_height",
            "month-label-height",
        ],
    ) {
        options.month_label_height = height;
    }
    if let Some(week) =
        json_object_f32(value, &["weekStartsOn", "week_starts_on", "week-starts-on"])
    {
        options = options.week_starts_on(week as i32);
    }
    if let Some(labels) = json_object_get(
        value,
        &["weekdayLabels", "weekday_labels", "weekday-labels"],
    )
    .and_then(calendar_weekday_labels_from_json)
    {
        options = options.weekday_labels(labels);
    }
    if let Some(strategy) = json_object_get(
        value,
        &["levelStrategy", "level_strategy", "level-strategy"],
    )
    .and_then(calendar_level_strategy_from_json)
    {
        options = options.level_strategy(strategy);
    }
    if let Some(template) = json_object_get(
        value,
        &[
            "monthFormat",
            "monthFormatter",
            "month_formatter",
            "month-formatter",
        ],
    )
    .and_then(calendar_string_template_from_json)
    {
        options = options.month_formatter(move |year, month| {
            apply_calendar_template(
                &template,
                &[
                    ("{year}", year.to_string()),
                    ("{monthPad}", format!("{month:02}")),
                    ("{month}", month.to_string()),
                ],
            )
        });
    }
    if let Some(template) = json_object_get(
        value,
        &[
            "titleFormat",
            "titleFormatter",
            "title_formatter",
            "title-formatter",
        ],
    )
    .and_then(calendar_string_template_from_json)
    {
        options = options.title_formatter(move |datum| {
            apply_calendar_template(
                &template,
                &[
                    ("{date}", datum.date.clone()),
                    ("{value}", datum.value.to_string()),
                ],
            )
        });
    }
    options
}

#[cfg(feature = "calendar")]
fn calendar_weekday_labels_from_json(value: &serde_json::Value) -> Option<Vec<(u8, String)>> {
    let items = json_array(value);
    let mut labels = Vec::new();
    if items.is_empty() {
        let serde_json::Value::Object(map) = value else {
            return None;
        };
        for (key, item) in map {
            let Ok(day) = key.parse::<u8>() else {
                continue;
            };
            let label = json_text(item);
            if !label.is_empty() {
                labels.push((day % 7, label));
            }
        }
        return (!labels.is_empty()).then_some(labels);
    }
    for (index, item) in items.into_iter().enumerate() {
        match item {
            serde_json::Value::Array(pair) if pair.len() >= 2 => {
                let day = json_f32(&pair[0])
                    .map(|day| day as u8)
                    .unwrap_or(index as u8);
                let label = json_text(&pair[1]);
                if !label.is_empty() {
                    labels.push((day % 7, label));
                }
            }
            serde_json::Value::Object(_) => {
                let day = json_object_f32(item, &["day", "index", "weekday"])
                    .map(|day| day as u8)
                    .unwrap_or(index as u8);
                let label = json_object_text(item, &["label", "text", "name"]);
                if !label.is_empty() {
                    labels.push((day % 7, label));
                }
            }
            serde_json::Value::String(label) if !label.is_empty() => {
                labels.push((index as u8 % 7, label.clone()));
            }
            _ => {}
        }
    }
    (!labels.is_empty()).then_some(labels)
}

#[cfg(feature = "calendar")]
fn calendar_level_strategy_from_json(
    value: &serde_json::Value,
) -> Option<CalendarLevelStrategy<()>> {
    match value {
        serde_json::Value::Array(_) => {
            let thresholds = json_array(value)
                .into_iter()
                .filter_map(json_f32)
                .collect::<Vec<_>>();
            (!thresholds.is_empty()).then_some(CalendarLevelStrategy::Thresholds(thresholds))
        }
        serde_json::Value::Number(number) => {
            number
                .as_f64()
                .map(|levels| CalendarLevelStrategy::Relative {
                    levels: (levels as u8).max(1),
                })
        }
        serde_json::Value::Object(_) => {
            let kind = json_object_text(value, &["type", "kind", "strategy"]).to_ascii_lowercase();
            if kind == "custom" {
                return None;
            }
            if kind == "thresholds"
                || json_object_get(value, &["thresholds", "stops"]).is_some() && kind != "relative"
            {
                let thresholds = json_object_get(value, &["thresholds", "stops", "values"])
                    .map(json_array)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(json_f32)
                    .collect::<Vec<_>>();
                return (!thresholds.is_empty())
                    .then_some(CalendarLevelStrategy::Thresholds(thresholds));
            }
            if kind == "relative" || json_object_get(value, &["levels", "level"]).is_some() {
                let levels = json_object_f32(value, &["levels", "level"])
                    .map(|levels| (levels as u8).max(1))
                    .unwrap_or(5);
                return Some(CalendarLevelStrategy::Relative { levels });
            }
            None
        }
        _ => None,
    }
}

#[cfg(feature = "calendar")]
fn calendar_string_template_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(template) if !template.is_empty() => Some(template.clone()),
        _ => None,
    }
}

#[cfg(feature = "calendar")]
fn apply_calendar_template(template: &str, replacements: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (needle, value) in replacements {
        out = out.replace(needle, value);
    }
    out
}

#[cfg(feature = "rich-text")]
fn markdown_source_from_spec<'a>(spec: &'a SemanticSpec<'_>) -> &'a str {
    if !spec.value.trim().is_empty() {
        return spec.value;
    }
    spec.attr("source")
        .or_else(|| spec.attr("markdown"))
        .or_else(|| spec.attr("value"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("")
}

#[cfg(feature = "graph-canvas")]
const DEFAULT_GRAPH_NODE_WIDTH: f32 = 160.0;
#[cfg(feature = "graph-canvas")]
const DEFAULT_GRAPH_NODE_HEIGHT: f32 = 80.0;

#[cfg(feature = "graph-canvas")]
fn graph_model_from_spec(spec: &SemanticSpec<'_>) -> GraphModel {
    let model = spec_json(spec, &["model"]);
    if let Some(model) = model.as_ref()
        && let Ok(parsed) = serde_json::from_value::<GraphModel>(model.clone())
        && !parsed.nodes().is_empty()
    {
        return parsed;
    }
    let nodes_value = model
        .as_ref()
        .and_then(|value| json_object_get(value, &["nodes"]).cloned())
        .or_else(|| spec_json(spec, &["nodes"]));
    let edges_value = model
        .as_ref()
        .and_then(|value| json_object_get(value, &["edges"]).cloned())
        .or_else(|| spec_json(spec, &["edges"]));
    let nodes = nodes_value
        .as_ref()
        .map(|value| {
            json_array(value)
                .into_iter()
                .filter_map(graph_node_from_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let edges = edges_value
        .as_ref()
        .map(|value| {
            json_array(value)
                .into_iter()
                .enumerate()
                .filter_map(|(index, item)| graph_edge_from_json(index, item))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    GraphModel::new(nodes.clone(), edges)
        .or_else(|_| GraphModel::new(nodes, Vec::new()))
        .unwrap_or_else(|_| GraphModel::empty())
}

#[cfg(feature = "graph-canvas")]
fn graph_viewport_from_spec(spec: &SemanticSpec<'_>) -> Option<GraphViewport> {
    let value = spec_json(spec, &["viewport"]).or_else(|| {
        spec_json(spec, &["model"])
            .and_then(|model| json_object_get(&model, &["viewport"]).cloned())
    })?;
    if let Ok(viewport) = serde_json::from_value::<GraphViewport>(value.clone()) {
        return Some(GraphViewport::new(viewport.offset, viewport.zoom));
    }
    graph_viewport_from_json(&value)
}

#[cfg(feature = "graph-canvas")]
fn graph_viewport_from_json(value: &serde_json::Value) -> Option<GraphViewport> {
    if !value.is_object() {
        return None;
    }
    let zoom = json_object_f32(value, &["zoom"]).unwrap_or(1.0);
    let offset = json_object_get(value, &["offset"])
        .and_then(graph_point_from_json)
        .unwrap_or_else(|| {
            GraphPoint::new(
                json_object_f32(value, &["offsetX", "offset_x", "offset-x", "x"]).unwrap_or(0.0),
                json_object_f32(value, &["offsetY", "offset_y", "offset-y", "y"]).unwrap_or(0.0),
            )
        });
    Some(GraphViewport::new(offset, zoom))
}

#[cfg(feature = "graph-canvas")]
fn graph_point_from_json(value: &serde_json::Value) -> Option<GraphPoint> {
    match value {
        serde_json::Value::Object(_) => {
            let x = json_object_f32(value, &["x", "offsetX", "offset_x", "offset-x"])?;
            let y = json_object_f32(value, &["y", "offsetY", "offset_y", "offset-y"])?;
            Some(GraphPoint::new(x, y))
        }
        serde_json::Value::Array(items) if items.len() >= 2 => {
            Some(GraphPoint::new(json_f32(&items[0])?, json_f32(&items[1])?))
        }
        _ => None,
    }
}

#[cfg(feature = "graph-canvas")]
fn graph_selection_from_spec(spec: &SemanticSpec<'_>) -> Option<GraphSelection> {
    let value = spec_json(spec, &["selection"]).or_else(|| {
        spec_json(spec, &["model"])
            .and_then(|model| json_object_get(&model, &["selection"]).cloned())
    })?;
    graph_selection_from_json(&value)
        .or_else(|| serde_json::from_value::<GraphSelection>(value).ok())
}

#[cfg(feature = "graph-canvas")]
fn graph_selection_from_json(value: &serde_json::Value) -> Option<GraphSelection> {
    if !value.is_object() {
        return None;
    }
    let kind = json_object_text(value, &["kind", "type"]).to_ascii_lowercase();
    let node = json_object_text(value, &["node", "nodeId", "node-id", "node_id"]);
    let port = json_object_text(value, &["port", "portId", "port-id", "port_id"]);
    let edge = json_object_text(value, &["edge", "edgeId", "edge-id", "edge_id"]);
    let id = json_object_text(value, &["id", "key"]);
    if kind == "port" || (!port.is_empty() && (!node.is_empty() || !id.is_empty())) {
        let node = if node.is_empty() { id } else { node };
        if node.is_empty() || port.is_empty() {
            return None;
        }
        return Some(GraphSelection::Port {
            node: node.into(),
            port: port.into(),
        });
    }
    if kind == "edge" || !edge.is_empty() {
        let edge = if edge.is_empty() { id } else { edge };
        if edge.is_empty() {
            return None;
        }
        return Some(GraphSelection::Edge(edge.into()));
    }
    if kind == "node" || !node.is_empty() || !id.is_empty() {
        let node = if node.is_empty() { id } else { node };
        if node.is_empty() {
            return None;
        }
        return Some(GraphSelection::Node(node.into()));
    }
    None
}

#[cfg(feature = "graph-canvas")]
fn graph_node_from_json(value: &serde_json::Value) -> Option<GraphNode> {
    if let Ok(node) = serde_json::from_value::<GraphNode>(value.clone()) {
        return Some(node);
    }
    if !value.is_object() {
        return None;
    }
    let label = json_object_text(value, &["title", "label", "name"]);
    let id = json_object_text(value, &["id", "key"]);
    if id.is_empty() && label.is_empty() {
        return None;
    }
    let identity = if id.is_empty() { label.clone() } else { id };
    let caption = if label.is_empty() {
        identity.clone()
    } else {
        label
    };
    let position = json_object_get(value, &["position"])
        .and_then(graph_point_from_json)
        .unwrap_or_else(|| {
            GraphPoint::new(
                json_object_f32(value, &["x"]).unwrap_or(0.0),
                json_object_f32(value, &["y"]).unwrap_or(0.0),
            )
        });
    let size = json_object_get(value, &["size"])
        .and_then(|size| {
            Some(GraphSize::new(
                json_object_f32(size, &["width"])?,
                json_object_f32(size, &["height"])?,
            ))
        })
        .unwrap_or_else(|| {
            GraphSize::new(
                json_object_f32(value, &["width"])
                    .filter(|width| *width > 0.0)
                    .unwrap_or(DEFAULT_GRAPH_NODE_WIDTH),
                json_object_f32(value, &["height"])
                    .filter(|height| *height > 0.0)
                    .unwrap_or(DEFAULT_GRAPH_NODE_HEIGHT),
            )
        });
    if !position.is_finite() || !size.is_valid() {
        return None;
    }
    let mut node = GraphNode::new(identity, caption, position, size);
    if let Some(ports) = json_object_get(value, &["ports"]) {
        for port in json_array(ports)
            .into_iter()
            .filter_map(graph_port_from_json)
        {
            node = node.with_port(port);
        }
    }
    let slot = json_object_text(
        value,
        &["contentSlot", "content_slot", "hostTexture", "host_texture"],
    );
    if !slot.is_empty() {
        node = node.with_content_slot(slot);
    }
    Some(node)
}

#[cfg(feature = "graph-canvas")]
fn graph_port_from_json(value: &serde_json::Value) -> Option<GraphPort> {
    if !value.is_object() {
        return None;
    }
    let id = json_object_text(value, &["id", "key"]);
    if id.is_empty() {
        return None;
    }
    let label = json_object_text(value, &["label", "title", "name"]);
    let caption = if label.is_empty() { id.clone() } else { label };
    let kind = parse_graph_port_kind(&json_object_text(value, &["kind", "type"]));
    let side = parse_graph_port_side(&json_object_text(value, &["side"]), kind);
    Some(GraphPort::new(id, caption, kind, side))
}

#[cfg(feature = "graph-canvas")]
fn graph_edge_from_json(index: usize, value: &serde_json::Value) -> Option<GraphEdge> {
    if let Ok(edge) = serde_json::from_value::<GraphEdge>(value.clone()) {
        return Some(edge);
    }
    if !value.is_object() {
        return None;
    }
    let source = graph_endpoint_from_json(value, "source", "from", "source-port", "source_port")?;
    let target = graph_endpoint_from_json(value, "target", "to", "target-port", "target_port")?;
    let mut id = json_object_text(value, &["id", "key"]);
    if id.is_empty() {
        id = format!(
            "e{index}-{}:{}-{}:{}",
            source.node, source.port, target.node, target.port
        );
    }
    let mut edge = GraphEdge::new(id, source, target);
    let label = json_object_text(value, &["label", "title"]);
    if !label.is_empty() {
        edge = edge.with_label(label);
    }
    Some(edge)
}

#[cfg(feature = "graph-canvas")]
fn graph_endpoint_from_json(
    value: &serde_json::Value,
    primary: &str,
    alias: &str,
    port_key: &str,
    port_alias: &str,
) -> Option<GraphEndpoint> {
    let endpoint = json_object_get(value, &[primary, alias])?;
    match endpoint {
        serde_json::Value::Object(_) => {
            let node = json_object_text(endpoint, &["node", "id"]);
            let port = json_object_text(endpoint, &["port", "id"]);
            if node.is_empty() || port.is_empty() {
                return None;
            }
            Some(GraphEndpoint::new(node, port))
        }
        other => {
            let node = json_text(other);
            let port = json_object_text(value, &[port_key, port_alias, "port"]);
            if node.is_empty() || port.is_empty() {
                return None;
            }
            Some(GraphEndpoint::new(node, port))
        }
    }
}

#[cfg(feature = "graph-canvas")]
fn parse_graph_port_kind(raw: &str) -> GraphPortKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "input" | "in" => GraphPortKind::Input,
        "bidirectional" | "both" | "inout" => GraphPortKind::Bidirectional,
        _ => GraphPortKind::Output,
    }
}

#[cfg(feature = "graph-canvas")]
fn parse_graph_port_side(raw: &str, kind: GraphPortKind) -> GraphPortSide {
    match raw.trim().to_ascii_lowercase().as_str() {
        "top" => GraphPortSide::Top,
        "right" => GraphPortSide::Right,
        "bottom" => GraphPortSide::Bottom,
        "left" => GraphPortSide::Left,
        _ => match kind {
            GraphPortKind::Input => GraphPortSide::Left,
            GraphPortKind::Output | GraphPortKind::Bidirectional => GraphPortSide::Right,
        },
    }
}

fn parse_split_axis(raw: Option<&str>) -> SplitAxis {
    match raw
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_ascii_lowercase()
        .as_str()
    {
        "vertical" | "column" | "y" => SplitAxis::Vertical,
        _ => SplitAxis::Horizontal,
    }
}

fn region_id_from_token(token: &str) -> Option<RegionId> {
    let token = token.trim().trim_start_matches("region-");
    if token.is_empty() {
        return None;
    }
    Some(match token {
        "global-navigation" | "globalnavigation" | "global" => RegionId::GlobalNavigation,
        "section-navigation" | "sectionnavigation" => RegionId::SectionNavigation,
        "resources" | "sidebar" | "files" => RegionId::Resources,
        "primary-toolbar" | "primarytoolbar" | "toolbar" => RegionId::PrimaryToolbar,
        "primary" | "main" => RegionId::Primary,
        "inspector" => RegionId::Inspector,
        "diagnostics" | "console" => RegionId::Diagnostics,
        other => RegionId::custom(other),
    })
}

fn dock_root_from_spec(spec: &SemanticSpec<'_>) -> DockNode {
    let contents = spec
        .slots
        .iter()
        .map(|(name, id)| (*name, Some(*id)))
        .collect::<Vec<_>>();
    if let Some(root) = spec_json(spec, &["root", "layout"])
        .and_then(|value| dock_node_from_json(&value, &contents))
    {
        return root;
    }
    dock_root_from_slots(&contents)
}

fn dock_root_from_slots(contents: &[(&str, Option<crate::StableNodeId>)]) -> DockNode {
    match contents {
        [] => DockNode::item("dock", None),
        [(id, content)] => DockNode::item(*id, *content),
        _ => {
            let tabs = contents
                .iter()
                .map(|(id, _)| Arc::<str>::from(*id))
                .collect::<Vec<_>>();
            let active = Arc::clone(&tabs[0]);
            let items = contents
                .iter()
                .map(|(id, content)| (Arc::<str>::from(*id), *content))
                .collect::<Vec<_>>();
            DockNode::tabs(tabs, active, items)
        }
    }
}

fn dock_node_from_json(
    value: &serde_json::Value,
    contents: &[(&str, Option<crate::StableNodeId>)],
) -> Option<DockNode> {
    let content_for = |id: &str| {
        contents
            .iter()
            .find_map(|(name, content)| (*name == id).then_some(*content).flatten())
    };
    match value {
        serde_json::Value::String(id) if !id.is_empty() => {
            Some(DockNode::item(id.as_str(), content_for(id)))
        }
        serde_json::Value::Array(items) => {
            let nodes = items
                .iter()
                .filter_map(|item| dock_node_from_json(item, contents))
                .collect::<Vec<_>>();
            dock_nodes_join(nodes)
        }
        serde_json::Value::Object(_) => {
            let kind = json_object_text(value, &["type", "kind", "node"]).to_ascii_lowercase();
            if kind == "split"
                || json_object_get(value, &["first"]).is_some()
                || json_object_get(value, &["second"]).is_some()
            {
                let first = json_object_get(value, &["first"])
                    .and_then(|item| dock_node_from_json(item, contents))?;
                let second = json_object_get(value, &["second"])
                    .and_then(|item| dock_node_from_json(item, contents))?;
                let axis = parse_dock_axis(&json_object_text(value, &["axis"]));
                let ratio = json_object_f32(value, &["ratio", "size"]).unwrap_or(0.5);
                return Some(DockNode::split(axis, ratio, first, second));
            }
            if kind == "tabs" || json_object_get(value, &["tabs"]).is_some() {
                let tab_values = json_object_get(value, &["tabs"])
                    .map(json_array)
                    .unwrap_or_default();
                let mut tabs = Vec::new();
                let mut tab_contents = Vec::new();
                for tab in tab_values {
                    let id = match tab {
                        serde_json::Value::String(id) if !id.is_empty() => id.clone(),
                        serde_json::Value::Object(_) => {
                            let id = json_object_text(tab, &["id", "key", "value"]);
                            if id.is_empty() {
                                continue;
                            }
                            id
                        }
                        _ => continue,
                    };
                    if tabs
                        .iter()
                        .any(|existing: &Arc<str>| existing.as_ref() == id)
                    {
                        continue;
                    }
                    let content = content_for(&id);
                    let id = Arc::<str>::from(id);
                    tab_contents.push((Arc::clone(&id), content));
                    tabs.push(id);
                }
                if tabs.is_empty() {
                    return None;
                }
                let active = json_object_text(value, &["active", "value"]);
                let active = if active.is_empty() {
                    Arc::clone(&tabs[0])
                } else {
                    Arc::<str>::from(active)
                };
                return Some(DockNode::tabs(tabs, active, tab_contents));
            }
            let id = json_object_text(value, &["id", "key", "value", "dock-id", "data-dock-id"]);
            if id.is_empty() {
                return None;
            }
            Some(DockNode::item(id.as_str(), content_for(&id)))
        }
        _ => None,
    }
}

fn dock_nodes_join(mut nodes: Vec<DockNode>) -> Option<DockNode> {
    match nodes.len() {
        0 => None,
        1 => nodes.pop(),
        _ => {
            let ids = nodes.iter().flat_map(DockNode::flatten).collect::<Vec<_>>();
            if ids.is_empty() {
                return None;
            }
            let mut contents = Vec::new();
            for node in &nodes {
                collect_dock_contents(node, &mut contents);
            }
            let active = Arc::clone(&ids[0]);
            Some(DockNode::tabs(ids, active, contents))
        }
    }
}

fn collect_dock_contents(
    node: &DockNode,
    output: &mut Vec<(Arc<str>, Option<crate::StableNodeId>)>,
) {
    match node {
        DockNode::Item { id, content } => output.push((Arc::clone(id), *content)),
        DockNode::Tabs { contents, .. } => output.extend(contents.iter().cloned()),
        DockNode::Split { first, second, .. } => {
            collect_dock_contents(first, output);
            collect_dock_contents(second, output);
        }
    }
}

fn parse_dock_axis(raw: &str) -> DockAxis {
    match raw.trim().to_ascii_lowercase().as_str() {
        "vertical" | "column" | "y" => DockAxis::Vertical,
        _ => DockAxis::Horizontal,
    }
}

fn settings_model_from_spec(spec: &SemanticSpec<'_>) -> Option<SettingsModel> {
    let settings = spec_json(spec, &["settings", "model"])?;
    if !settings.is_object() {
        return None;
    }
    let tabs_value = json_object_get(&settings, &["tabs", "items"])?;
    let mut tabs = settings_tabs_from_json(tabs_value);
    if tabs.is_empty() {
        return None;
    }
    let full_page = settings_full_page_keys_from_json(&settings);
    if !full_page.is_empty() {
        tabs = tabs
            .into_iter()
            .map(|tab| {
                let flagged =
                    tab.full_page_value() || full_page.iter().any(|key| key == tab.id().as_str());
                let mut next = SettingsTab::new(tab.id().clone(), tab.label());
                if let Some(icon) = tab.icon_value() {
                    next = next.icon(icon);
                }
                next.full_page(flagged)
            })
            .collect();
    }
    let default_tab = json_object_text(&settings, &["defaultTab", "default_tab", "default-tab"]);
    let default_tab =
        if default_tab.is_empty() || tabs.iter().all(|tab| tab.id().as_str() != default_tab) {
            tabs[0].id().as_str().to_string()
        } else {
            default_tab
        };
    let mut model = SettingsModel::new(default_tab, tabs).ok()?;
    if let Some(aliases) =
        json_object_get(&settings, &["aliases", "alias"]).and_then(|value| value.as_object())
    {
        for (alias, target) in aliases {
            let target = json_text(target);
            if target.is_empty() {
                continue;
            }
            if let Ok(next) = model.clone().with_alias(alias.as_str(), target) {
                model = next;
            }
        }
    }
    let hide_header = json_object_truthy(&settings, &["hideHeader", "hide_header", "hide-header"])
        || flag_attr(spec, &["hide-header", "hideheader", "hideHeader"]);
    Some(model.hide_header(hide_header))
}

fn settings_tabs_from_json(value: &serde_json::Value) -> Vec<SettingsTab> {
    let items = json_array(value);
    if !items.is_empty() {
        return items
            .into_iter()
            .filter_map(settings_tab_from_json)
            .collect();
    }
    if let Some(tab) = settings_tab_from_json(value) {
        return vec![tab];
    }
    let serde_json::Value::Object(map) = value else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(key, item)| match item {
            serde_json::Value::String(label) if !label.is_empty() => {
                Some(SettingsTab::new(key.as_str(), label.as_str()))
            }
            serde_json::Value::Object(_) => {
                let tab = settings_tab_from_json(item)?;
                if tab.id().as_str().is_empty() {
                    Some(SettingsTab::new(key.as_str(), tab.label()))
                } else {
                    Some(tab)
                }
            }
            _ => None,
        })
        .collect()
}

fn settings_tab_from_json(value: &serde_json::Value) -> Option<SettingsTab> {
    match value {
        serde_json::Value::String(id) if !id.is_empty() => {
            Some(SettingsTab::new(id.as_str(), id.as_str()))
        }
        serde_json::Value::Object(_) => {
            let id = json_object_text(value, &["key", "id", "value"]);
            if id.is_empty() {
                return None;
            }
            let label = json_object_text(value, &["label", "title", "name"]);
            let label = if label.is_empty() { id.clone() } else { label };
            let mut tab = SettingsTab::new(id, label);
            if let Some(icon) =
                Icon::parse_name(&json_object_text(value, &["icon", "iconName", "icon-name"]))
            {
                tab = tab.icon(icon);
            }
            Some(tab.full_page(json_object_truthy(
                value,
                &["fullPage", "full_page", "full-page"],
            )))
        }
        _ => None,
    }
}

fn settings_full_page_keys_from_json(value: &serde_json::Value) -> Vec<String> {
    json_object_get(value, &["fullPageTabs", "full_page_tabs", "full-page-tabs"])
        .map(json_array)
        .unwrap_or_default()
        .into_iter()
        .map(json_text)
        .filter(|key| !key.is_empty())
        .collect()
}

fn fallback_settings_model(spec: &SemanticSpec<'_>) -> SettingsModel {
    let label = if spec.display_label().is_empty() {
        "Settings"
    } else {
        spec.display_label()
    };
    SettingsModel::new("settings", [SettingsTab::new("settings", label)])
        .expect("fallback settings model has one tab")
}

impl RegisterableComponent for crate::Panel {
    const TYPE_ID: &'static str = crate::component_descriptors::PANEL.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::PANEL.tags;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        Self::new(spec.display_label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::AppContext;
    use crate::{ComponentTypeId, DocumentId, RegisterableComponent, SemanticSpec, StableNodeId};
    use nana_ui_core::LayoutStyle;

    fn spec_with<'a>(
        type_id: &'a ComponentTypeId,
        layout: &'a Arc<LayoutStyle>,
        attrs: &'a [(&'a str, &'a str)],
        slots: &'a [(&'a str, StableNodeId)],
        options: &'a [crate::SemanticOption<'a>],
        value: &'a str,
        label: &'a str,
    ) -> SemanticSpec<'a> {
        SemanticSpec {
            label,
            value,
            options,
            attrs,
            slots,
            ..SemanticSpec::from_parts(type_id, layout)
        }
    }

    #[cfg(feature = "charts")]
    #[test]
    fn semantic_charts_preserve_datum_alignment_and_theme_roles() {
        let ty = ComponentTypeId::new("nana.time-series-chart").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [
            ("values", "[16777217,null,4]"),
            ("axis-labels", r#"["A","B","C"]"#),
            (
                "layers",
                r#"[{"label":"Input","values":[2,null,4],"color":"Warning"}]"#,
            ),
            ("tooltip-details", r#"["Cost 2","Cost 0","Cost 4"]"#),
        ];
        let spec = spec_with(&ty, &layout, &attrs, &[], &[], "", "");
        let chart = TimeSeriesChart::from_semantic(&spec);
        assert_eq!(chart.values, [16777217.0, 0.0, 4.0]);
        assert_eq!(chart.layers[0].values, [2.0, 0.0, 4.0]);
        assert_eq!(
            chart.layers[0].color,
            nana_ui_core::SemanticColorRole::Warning
        );
        assert!(chart.tooltip(1).unwrap().contains("Cost 0"));
        let mut chart = chart;
        chart.active = Some(1);
        assert_eq!(
            TimeSeriesChart::reconcile_semantic(&spec, Some(&chart)).active,
            Some(1)
        );
        chart.active = Some(9);
        assert_eq!(
            TimeSeriesChart::reconcile_semantic(&spec, Some(&chart)).active,
            None
        );
        let ty = ComponentTypeId::new("nana.donut-chart").unwrap();
        let attrs = [
            (
                "slices",
                r#"[{"value":null},{"value":4,"color":"Success"}]"#,
            ),
            ("labels", r#"["Missing","Valid"]"#),
        ];
        let spec = spec_with(&ty, &layout, &attrs, &[], &[], "", "");
        let chart = DonutChart::from_semantic(&spec);
        assert_eq!(chart.slices[0].value, 0.0);
        assert_eq!(
            chart.slices[1].color,
            nana_ui_core::SemanticColorRole::Success
        );
        assert_eq!(
            chart.slice_at(
                crate::LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0
                },
                95.0,
                50.0
            ),
            Some(1)
        );
        let mut chart = chart;
        chart.active = Some(1);
        assert_eq!(
            DonutChart::reconcile_semantic(&spec, Some(&chart)).active,
            Some(1)
        );
        chart.active = Some(0);
        assert_eq!(
            DonutChart::reconcile_semantic(&spec, Some(&chart)).active,
            None
        );
    }

    #[test]
    fn title_bar_semantic_transparency_and_drag_policy() {
        let type_id = ComponentTypeId::new(AppTitleBar::TYPE_ID).unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let defaults =
            AppTitleBar::from_semantic(&spec_with(&type_id, &layout, &[], &[], &[], "", ""));
        assert!(!defaults.transparent);
        assert!(defaults.drag_enabled);
        let configured = AppTitleBar::from_semantic(&spec_with(
            &type_id,
            &layout,
            &[("transparent", "true"), ("drag-enabled", "false")],
            &[],
            &[],
            "",
            "",
        ));
        assert!(configured.transparent);
        assert!(!configured.drag_enabled);
    }

    #[test]
    fn card_from_semantic_keeps_uniform_padding_and_em_height() {
        let type_id = ComponentTypeId::new("nana.card").unwrap();
        let layout = Arc::new(LayoutStyle {
            padding: Some(LengthSpec::Px(8.0)),
            height: Some(LengthSpec::Em(2.0)),
            ..LayoutStyle::default()
        });
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "");
        let card = Card::from_semantic(&spec);
        assert_eq!(card.style.layout.padding, Some(LengthSpec::Px(8.0)));
        assert_eq!(card.style.layout.padding_left, None);
        assert_eq!(card.style.layout.padding_right, None);
        assert_eq!(card.style.layout.padding_top, None);
        assert_eq!(card.style.layout.padding_bottom, None);
        assert_eq!(card.style.layout.height, Some(LengthSpec::Em(2.0)));
        assert_eq!(card.style.radius, Some(nana_ui_core::RadiusTier::Md));
        assert!(card.style.layout.border_radius.is_none());
    }

    #[test]
    fn button_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.button").unwrap();
        let layout = Arc::new(LayoutStyle {
            height: Some(LengthSpec::Px(48.0)),
            width: Some(LengthSpec::Px(120.0)),
            border_radius: Some(2.0),
            ..LayoutStyle::default()
        });
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Go");
        let button = Button::from_semantic(&spec);
        assert_eq!(button.style.layout.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(button.style.layout.width, Some(LengthSpec::Px(120.0)));
        assert!(button.style.layout.min_height.is_none());
        assert_eq!(button.style.layout.border_radius, Some(2.0));
        assert_eq!(
            button.style.layout.font_size,
            Some(nana_ui_core::ControlSize::Medium.text_size())
        );
    }

    fn css_size_override_layout() -> LayoutStyle {
        LayoutStyle {
            height: Some(LengthSpec::Px(48.0)),
            width: Some(LengthSpec::Px(120.0)),
            border_radius: Some(2.0),
            ..LayoutStyle::default()
        }
    }

    fn assert_l2_css_size_overlaid(layout: &LayoutStyle) {
        assert_eq!(layout.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(layout.width, Some(LengthSpec::Px(120.0)));
        assert!(layout.min_height.is_none());
        assert_eq!(layout.border_radius, Some(2.0));
    }

    #[test]
    fn switch_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.switch").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "On");
        let switch = Switch::from_semantic(&spec);
        assert_l2_css_size_overlaid(&switch.style.layout);
        assert_eq!(switch.size, nana_ui_core::ControlSize::Medium);
    }

    #[test]
    fn switch_from_semantic_without_css_keeps_control_size() {
        let type_id = ComponentTypeId::new("nana.switch").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "On");
        let switch = Switch::from_semantic(&spec);
        assert_eq!(
            switch.style.control_height,
            Some(nana_ui_core::ControlHeight::Min(
                nana_ui_core::ControlSize::Medium
            ))
        );
        assert!(switch.style.layout.height.is_none());
        assert!(switch.style.layout.min_height.is_none());
        assert!(switch.style.layout.width.is_none());
    }

    #[test]
    fn checkbox_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.checkbox").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Agree");
        let checkbox = Checkbox::from_semantic(&spec);
        assert_l2_css_size_overlaid(&checkbox.style.layout);
        assert_eq!(checkbox.size, nana_ui_core::ControlSize::Medium);
    }

    #[test]
    fn checkbox_from_semantic_without_css_keeps_control_size() {
        let type_id = ComponentTypeId::new("nana.checkbox").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Agree");
        let checkbox = Checkbox::from_semantic(&spec);
        assert_eq!(
            checkbox.style.control_height,
            Some(nana_ui_core::ControlHeight::Min(
                nana_ui_core::ControlSize::Medium
            ))
        );
        assert!(checkbox.style.layout.min_height.is_none());
        assert!(checkbox.style.layout.height.is_none());
        assert!(checkbox.style.layout.width.is_none());
    }

    #[test]
    fn select_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.select").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Pick");
        let select = Select::from_semantic(&spec);
        assert_l2_css_size_overlaid(&select.style.layout);
        assert_eq!(select.size, nana_ui_core::ControlSize::Medium);
    }

    #[test]
    fn select_from_semantic_without_css_keeps_control_size() {
        let type_id = ComponentTypeId::new("nana.select").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Pick");
        let select = Select::from_semantic(&spec);
        assert_eq!(
            select.style.control_height,
            Some(nana_ui_core::ControlHeight::Exact(
                nana_ui_core::ControlSize::Medium
            ))
        );
        assert!(select.style.layout.height.is_none());
        assert_eq!(select.style.layout.width, Some(LengthSpec::Fill));
    }

    #[test]
    fn dropdown_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.dropdown").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Pick");
        let dropdown = Dropdown::from_semantic(&spec);
        assert_l2_css_size_overlaid(&dropdown.style.layout);
        assert_eq!(dropdown.size, nana_ui_core::ControlSize::Medium);
    }

    #[test]
    fn range_field_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.range-field").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Volume");
        let field = RangeField::from_semantic(&spec);
        assert_l2_css_size_overlaid(&field.style.layout);
        assert_eq!(field.size, nana_ui_core::ControlSize::Medium);
        assert!(field.show_value);
    }

    #[test]
    fn range_field_from_semantic_can_hide_the_value_readout() {
        let type_id = ComponentTypeId::new("nana.range-field").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [("show-value", "false")];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "Seek");
        let field = RangeField::from_semantic(&spec);
        assert!(!field.show_value);
    }

    #[test]
    fn textarea_from_semantic_css_size_overrides_control_tokens() {
        let type_id = ComponentTypeId::new("nana.textarea").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Notes");
        let area = TextArea::from_semantic(&spec);
        assert_l2_css_size_overlaid(&area.style.layout);
    }

    fn project_layout(component: impl crate::ComponentView) -> Arc<LayoutStyle> {
        let mut context = AppContext::new();
        let entity = context
            .create_component(DocumentId::new(1).unwrap(), component)
            .unwrap();
        Arc::clone(
            &context
                .world()
                .node_style(entity.stable_id())
                .unwrap()
                .layout,
        )
    }

    #[test]
    fn select_from_semantic_projects_overlay_css_size_into_world() {
        let type_id = ComponentTypeId::new("nana.select").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Pick");
        let projected = project_layout(Select::from_semantic(&spec));
        assert_eq!(projected.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(projected.width, Some(LengthSpec::Px(120.0)));
        assert_ne!(
            projected.height,
            Some(LengthSpec::Px(
                nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
            ))
        );
    }

    #[test]
    fn dropdown_from_semantic_projects_overlay_css_size_into_world() {
        let type_id = ComponentTypeId::new("nana.dropdown").unwrap();
        let layout = Arc::new(css_size_override_layout());
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Pick");
        let projected = project_layout(Dropdown::from_semantic(&spec));
        assert_eq!(projected.height, Some(LengthSpec::Px(48.0)));
        assert_eq!(projected.width, Some(LengthSpec::Px(120.0)));
        assert_ne!(
            projected.height,
            Some(LengthSpec::Px(
                nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
            ))
        );
    }

    #[test]
    fn switch_from_semantic_projects_overlay_css_size_without_control_min_height() {
        let type_id = ComponentTypeId::new("nana.switch").unwrap();
        let layout = Arc::new(LayoutStyle {
            height: Some(LengthSpec::Px(20.0)),
            width: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        });
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "On");
        let projected = project_layout(Switch::from_semantic(&spec));
        assert_eq!(projected.height, Some(LengthSpec::Px(20.0)));
        assert_eq!(projected.width, Some(LengthSpec::Px(80.0)));
        assert!(projected.min_height.is_none());
    }

    #[test]
    fn range_field_from_semantic_projects_overlay_css_size_without_control_min_height() {
        let type_id = ComponentTypeId::new("nana.range-field").unwrap();
        let layout = Arc::new(LayoutStyle {
            height: Some(LengthSpec::Px(20.0)),
            width: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        });
        let spec = spec_with(&type_id, &layout, &[], &[], &[], "", "Volume");
        let projected = project_layout(RangeField::from_semantic(&spec));
        assert_eq!(projected.height, Some(LengthSpec::Px(20.0)));
        assert_eq!(projected.width, Some(LengthSpec::Px(80.0)));
        assert!(projected.min_height.is_none());
    }

    #[test]
    fn command_palette_json_items_keep_category() {
        let type_id = ComponentTypeId::new("nana.command-palette").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [(
            "items",
            r#"[{"value":"open","label":"Open file","category":"Workspace","shortcut":"Ctrl+P"}]"#,
        )];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "Go to");
        let palette = CommandPalette::from_semantic(&spec);
        assert_eq!(palette.items.len(), 1);
        assert_eq!(palette.items[0].label, "Open file");
        assert_eq!(palette.items[0].category.as_deref(), Some("Workspace"));
        assert_eq!(palette.items[0].shortcut.as_deref(), Some("Ctrl+P"));
    }

    #[test]
    fn tree_view_json_keeps_nested_children() {
        let type_id = ComponentTypeId::new("nana.tree-view").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [(
            "tree",
            r#"[{"id":"src","label":"src","expanded":true,"children":[{"id":"lib","label":"lib.rs"}]}]"#,
        )];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "");
        let tree = TreeView::from_semantic(&spec);
        assert_eq!(tree.nodes.len(), 1);
        assert!(tree.nodes[0].branch && tree.nodes[0].expanded);
        assert_eq!(tree.nodes[0].children.len(), 1);
        assert_eq!(tree.nodes[0].children[0].id.as_ref(), "lib");
    }

    #[test]
    #[cfg(feature = "calendar")]
    fn calendar_heatmap_json_data_is_not_empty() {
        let type_id = ComponentTypeId::new("nana.calendar-heatmap").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [(
            "data",
            r#"[{"date":"2026-06-01","value":2},["2026-06-03",8]]"#,
        )];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "Activity");
        let calendar = CalendarHeatmap::<()>::from_semantic(&spec);
        assert_eq!(calendar.data.len(), 2);
        assert_eq!(calendar.data[0].date, "2026-06-01");
        assert_eq!(calendar.label.as_deref(), Some("Activity"));
    }

    #[test]
    #[cfg(feature = "calendar")]
    fn calendar_title_format_json_changes_cell_title() {
        let type_id = ComponentTypeId::new("nana.calendar-heatmap").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [
            ("data", r#"[{"date":"2026-06-01","value":4}]"#),
            ("options", r#"{"titleFormat":"{date}={value}"}"#),
        ];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "");
        let calendar = CalendarHeatmap::<()>::from_semantic(&spec);
        let model = calendar.model();
        assert!(
            model
                .cells
                .iter()
                .any(|cell| cell.date == "2026-06-01" && cell.title == "2026-06-01=4"),
            "titleFormat {{date}}={{value}} must change the model cell title"
        );
    }

    #[test]
    #[cfg(feature = "graph-canvas")]
    fn graph_canvas_json_nodes_are_not_empty() {
        let type_id = ComponentTypeId::new("nana.graph-canvas").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [(
            "model",
            r#"{"nodes":[{"id":"source","title":"Source","x":20,"y":24,"contentSlot":"formula.source"}]}"#,
        )];
        let content = StableNodeId::new(9).unwrap();
        let slots = [("source", content)];
        let spec = spec_with(&type_id, &layout, &attrs, &slots, &[], "", "Graph");
        let canvas = GraphCanvas::from_semantic(&spec);
        assert_eq!(canvas.model.nodes().len(), 1);
        assert_eq!(canvas.model.nodes()[0].id.as_str(), "source");
        assert_eq!(
            canvas.model.nodes()[0].content_slot.as_deref(),
            Some("formula.source")
        );
        assert_eq!(canvas.label.as_deref(), Some("Graph"));
        assert_eq!(
            canvas.node_contents(),
            [("source".into(), crate::GraphNodeContent::region(content))]
        );
    }

    #[test]
    #[cfg(feature = "rich-text")]
    fn native_markdown_reads_source_attr_when_value_empty() {
        let type_id = ComponentTypeId::new("nana.native-markdown").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [("source", "# Native")];
        let spec = spec_with(&type_id, &layout, &attrs, &[], &[], "", "");
        let markdown = NativeMarkdown::from_semantic(&spec);
        assert!(
            markdown.plain_text().contains("Native"),
            "source attr must parse when value is empty"
        );
    }

    #[test]
    fn app_shell_binds_title_bar_and_body_slots() {
        let type_id = ComponentTypeId::new("nana.app-shell").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let title = StableNodeId::new(2).unwrap();
        let body = StableNodeId::new(3).unwrap();
        let slots = [("title-bar", title), ("body", body)];
        let spec = spec_with(&type_id, &layout, &[], &slots, &[], "", "");
        let shell = AppShell::from_semantic(&spec);
        assert_eq!(shell.title_bar, Some(title));
        assert_eq!(shell.body, Some(body));
    }

    #[test]
    fn split_pane_reads_axis_and_child_slots() {
        let type_id = ComponentTypeId::new("nana.split-pane").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let first = StableNodeId::new(4).unwrap();
        let second = StableNodeId::new(5).unwrap();
        let slots = [("first", first), ("second", second)];
        let attrs = [("axis", "vertical"), ("size", "320")];
        let spec = spec_with(&type_id, &layout, &attrs, &slots, &[], "", "");
        let pane = SplitPane::from_semantic(&spec);
        assert_eq!(pane.first, Some(first));
        assert_eq!(pane.second, Some(second));
        assert_eq!(pane.model.axis(), SplitAxis::Vertical);
        assert_eq!(pane.model.size(), 320.0);
    }

    #[test]
    fn workspace_slots_become_region_tokens() {
        let type_id = ComponentTypeId::new("nana.workspace").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let primary = StableNodeId::new(6).unwrap();
        let slots = [("primary", primary)];
        let spec = spec_with(&type_id, &layout, &[], &slots, &[], "", "");
        let workspace = Workspace::from_semantic(&spec);
        assert_eq!(workspace.slots.len(), 1);
        assert_eq!(workspace.slots[0].id, RegionId::Primary);
        assert_eq!(workspace.slots[0].content, Some(primary));
    }

    #[test]
    fn dock_slots_are_not_dummy_item() {
        let type_id = ComponentTypeId::new("nana.dock").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let nav = StableNodeId::new(7).unwrap();
        let files = StableNodeId::new(8).unwrap();
        let slots = [("nav", nav), ("files", files)];
        let spec = spec_with(&type_id, &layout, &[], &slots, &[], "", "");
        let dock = Dock::from_semantic(&spec);
        let ids = dock
            .flatten()
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["nav", "files"]);
    }

    #[test]
    fn settings_page_parses_model_json_and_content_slot() {
        let type_id = ComponentTypeId::new("nana.settings-page").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let content = StableNodeId::new(9).unwrap();
        let slots = [("content", content)];
        let attrs = [(
            "settings",
            r#"{"tabs":[{"id":"appearance","label":"外观"}],"defaultTab":"appearance"}"#,
        )];
        let spec = spec_with(&type_id, &layout, &attrs, &slots, &[], "", "");
        let page = SettingsPage::from_semantic(&spec);
        assert_eq!(page.content, Some(content));
        assert_eq!(page.model.tabs().len(), 1);
        assert_eq!(page.model.tabs()[0].id().as_str(), "appearance");
    }

    #[test]
    fn icon_glyph_from_semantic_uses_catalog_icon_and_css_size() {
        let type_id = ComponentTypeId::new("nana.icon").unwrap();
        let layout = Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(24.0)),
            height: Some(LengthSpec::Px(24.0)),
            ..LayoutStyle::default()
        });
        let spec = SemanticSpec {
            icon: Some(Icon::Search),
            ..spec_with(&type_id, &layout, &[], &[], &[], "search", "")
        };
        let glyph = IconGlyph::from_semantic(&spec);
        assert_eq!(glyph.icon, Icon::Search);
        assert_eq!(glyph.size, 24.0);
    }

    #[test]
    fn stack_tags_resolve_to_stack_and_aliases() {
        let context = AppContext::new();
        assert_eq!(
            context
                .resolve_component_tag("stack")
                .map(ComponentTypeId::as_str),
            Some("nana.stack")
        );
        assert_eq!(
            context
                .resolve_component_tag("column")
                .map(ComponentTypeId::as_str),
            Some("nana.column")
        );
        assert!(
            context.resolve_component_tag("div").is_none(),
            "HTML div stays a layout box; the column alias owns `column`"
        );
        assert_eq!(
            context
                .resolve_component_tag("row")
                .map(ComponentTypeId::as_str),
            Some("nana.row")
        );
        assert_eq!(
            context
                .resolve_component_tag("box")
                .map(ComponentTypeId::as_str),
            Some("nana.box")
        );
        let type_id = ComponentTypeId::new("nana.row").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let stack = Stack::from_semantic(&SemanticSpec::from_parts(&type_id, &layout));
        assert_eq!(
            stack.node_style().layout.direction,
            Some(nana_ui_core::FlexDirection::Row)
        );
    }

    #[test]
    fn icon_and_icon_button_tags_do_not_collide() {
        let context = AppContext::new();
        assert_eq!(
            context
                .resolve_component_tag("icon")
                .map(ComponentTypeId::as_str),
            Some("nana.icon")
        );
        assert_eq!(
            context
                .resolve_component_tag("i")
                .map(ComponentTypeId::as_str),
            Some("nana.icon")
        );
        assert_eq!(
            context
                .resolve_component_tag("nana-icon")
                .map(ComponentTypeId::as_str),
            Some("nana.icon")
        );
        assert_eq!(
            context
                .resolve_component_tag("icon-button")
                .map(ComponentTypeId::as_str),
            Some("nana.icon-button")
        );
        assert!(context.resolve_component_tag("svg").is_none());
    }

    #[test]
    fn video_tag_resolves_to_single_video_component() {
        let context = AppContext::new();
        assert_eq!(
            context
                .resolve_component_tag("video")
                .map(ComponentTypeId::as_str),
            Some("nana.video"),
            "HTML <video> is the same-name Runtime control tag"
        );
        assert_eq!(
            context
                .resolve_component_tag("nana-video")
                .map(ComponentTypeId::as_str),
            Some("nana.video")
        );
    }

    #[test]
    fn hosted_textarea_from_semantic_applies_gutter_attrs() {
        let ty = ComponentTypeId::new("nana.hosted-textarea").unwrap();
        let layout = Arc::new(LayoutStyle::default());
        let attrs = [
            ("language", "rs"),
            ("line-numbers", ""),
            ("relative-line-numbers", "true"),
            ("minimap", "1"),
            (
                "diagnostics",
                r#"[{"offset":0,"length":2,"severity":"warning","message":"unused"}]"#,
            ),
            ("git-gutter", r#"[{"line":1,"kind":"added"}]"#),
        ];
        let spec = spec_with(&ty, &layout, &attrs, &[], &[], "fn", "Editor");
        let area = HostedTextarea::from_semantic(&spec);
        assert!(area.line_numbers);
        assert!(area.relative_line_numbers);
        assert!(area.minimap);
        assert_eq!(area.diagnostics.len(), 1);
        assert_eq!(
            area.diagnostics[0].severity,
            TextDiagnosticSeverity::Warning
        );
        assert_eq!(
            area.git_gutter,
            Arc::from([TextGitMark::new(1, TextGitMarkKind::Added)])
        );
    }
}

#[cfg(test)]
mod stack_direction_tests {
    use super::*;
    use crate::layout_engine::{LayoutViewport, RuntimeLayoutEngine, StyleLayoutNode};
    use crate::{
        AppContext, ComponentTypeId, ComponentView, DocumentId, MutationQueue, NodeKind,
        RegisterableComponent, SemanticSpec, StableNodeId, UiWorld,
    };
    use nana_ui_core::{FlexDirection, LayoutStyle, LengthSpec};

    /// Whether projecting a `Stack` over the style the CSS cascade just
    /// published asks for a further style write.
    fn projection_rewrites_style(type_id: &str, cascaded: &Arc<LayoutStyle>) -> bool {
        let document = DocumentId::new(1).unwrap();
        let id = StableNodeId::new(1).unwrap();
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(id, document, NodeKind::Element { tag: "div".into() });
        // What `publish_layouts` writes: the cascade's own result.
        queue.set_style(
            id,
            crate::NodeStyle {
                layout: Arc::clone(cascaded),
                ..crate::NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();

        let type_id = ComponentTypeId::new(type_id).unwrap();
        let stack = Stack::from_semantic(&SemanticSpec::from_parts(&type_id, cascaded));
        let mut queue = MutationQueue::new();
        stack.project(id, &world, &mut queue);
        !queue.is_empty()
    }

    /// Projection must not ask to rewrite the style the cascade just wrote.
    ///
    /// The Vue cascade rebuilds a node's `LayoutStyle` from scratch whenever
    /// author CSS applies to it, which leaves `direction` unset. If projection
    /// seeds a value there, the next cascade drops it again and the two writers
    /// alternate on that node forever -- a style write per node per frame, and
    /// with it a layout pass nothing asked for. On a 2,000-row list that cost
    /// `FrameStage::Layout` 0.138 ms and settle 0.22 ms per pointer event.
    ///
    /// `nana.row` is the exception, and it is not a spelling difference: the
    /// engine's default flow axis is the block axis, so a row that does not say
    /// so lays out as a column. Its write is owed. That it still fights the
    /// cascade is the remaining half, recorded in `archive/docs-notes/runtime-dirty-frame.md`.
    #[test]
    fn projecting_a_stack_does_not_rewrite_the_style_the_cascade_published() {
        // Sized by author CSS, no `direction` -- what the cascade produces.
        let cascaded = Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(320.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        });
        for type_id in ["nana.column", "nana.box", "nana.stack"] {
            assert!(
                !projection_rewrites_style(type_id, &cascaded),
                "{type_id}: projection rewrote the cascade's style, which puts \
                 this node in a write-per-frame loop"
            );
        }
        assert!(
            projection_rewrites_style("nana.row", &cascaded),
            "nana.row must still seed its flow axis -- if this stops being \
             true the assertion above is testing nothing"
        );
    }

    /// `Stack::READS_CHILD_DERIVED_SPEC = false` is a promise a host acts on by
    /// skipping two scans of the child list entirely. If `from_semantic` ever
    /// starts reading `spec.slots` or `spec.icon`, that host silently drops
    /// those children instead of binding them -- so the promise has to be
    /// checked, not just declared.
    #[test]
    fn stack_ignores_child_derived_spec_so_declaring_it_is_honest() {
        const { assert!(!<Stack as RegisterableComponent>::READS_CHILD_DERIVED_SPEC) };
        let layout = Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(64.0)),
            ..LayoutStyle::default()
        });
        let slot_child = StableNodeId::new(7).unwrap();
        for type_id in ["nana.stack", "nana.column", "nana.row", "nana.box"] {
            let type_id = ComponentTypeId::new(type_id).unwrap();
            let bare = Stack::from_semantic(&SemanticSpec::from_parts(&type_id, &layout));
            let slots = [("content", slot_child), ("leading", slot_child)];
            let with_slots = Stack::from_semantic(&SemanticSpec {
                slots: &slots,
                ..SemanticSpec::from_parts(&type_id, &layout)
            });
            assert_eq!(
                bare, with_slots,
                "{type_id:?}: Stack read its slots, but it is registered as \
                 ignoring them, so a host that skips collecting them would \
                 silently drop those children"
            );
            let with_icon = Stack::from_semantic(&SemanticSpec {
                icon: Some(nana_ui_core::Icon::Add),
                ..SemanticSpec::from_parts(&type_id, &layout)
            });
            assert_eq!(
                bare, with_icon,
                "{type_id:?}: Stack read its icon, but it is registered as \
                 ignoring it, so a host that skips probing for one would \
                 silently drop it"
            );
        }
    }

    /// The flag has to reach the registry under every id the component answers
    /// to. `Stack` is registered once and aliased three times; an alias that
    /// lost the flag would just quietly keep paying both scans.
    #[test]
    fn the_child_spec_flag_reaches_every_alias_and_defaults_to_yes() {
        let context = AppContext::new();
        for slotless in [
            "nana.stack",
            "nana.column",
            "nana.row",
            "nana.box",
            "nana.drop-target",
        ] {
            let id = ComponentTypeId::new(slotless).unwrap();
            assert!(
                !context.component_reads_child_derived_spec(&id),
                "{slotless}: the layout alias did not inherit \
                 READS_CHILD_DERIVED_SPEC"
            );
        }
        // A component that does address children by name, and an id the
        // registry has never heard of, both have to answer "yes".
        for reads in [
            "nana.list-item",
            "nana.sidebar-frame",
            "nana.not-a-component",
        ] {
            let id = ComponentTypeId::new(reads).unwrap();
            assert!(
                context.component_reads_child_derived_spec(&id),
                "{reads}: skipping the child scans here would drop children"
            );
        }
    }

    /// The half of the seed that is load-bearing, and the half that is not.
    ///
    /// Without this, "stop seeding" reads as a free simplification in both
    /// directions -- and laying a row out as a column is a very visible bug.
    #[test]
    fn an_unset_flow_axis_is_a_column_but_not_a_row() {
        let child = |name: &str| StyleLayoutNode {
            id: name.into(),
            style: LayoutStyle {
                width: Some(LengthSpec::Px(40.0)),
                height: Some(LengthSpec::Px(10.0)),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        };
        let laid_out = |direction: Option<FlexDirection>| {
            let root = StyleLayoutNode {
                id: "root".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Px(300.0)),
                    height: Some(LengthSpec::Px(300.0)),
                    direction,
                    ..LayoutStyle::default()
                },
                children: vec![child("a"), child("b")],
                text: None,
            };
            RuntimeLayoutEngine
                .layout_style_tree(
                    &root,
                    LayoutViewport::new(320.0, 480.0),
                    &mut crate::MeasureTextShaper,
                )
                .into_iter()
                .find(|(name, _)| name == "b")
                .expect("second child")
                .1
        };
        assert_eq!(
            laid_out(None),
            laid_out(Some(FlexDirection::Column)),
            "dropping the nana.column seed has to be inert"
        );
        assert_ne!(
            laid_out(None),
            laid_out(Some(FlexDirection::Row)),
            "the nana.row seed has to stay"
        );
    }
}
