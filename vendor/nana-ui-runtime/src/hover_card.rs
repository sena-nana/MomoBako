//! Hover-triggered anchored card. The trigger stays in flow; the card content
//! hangs off it while the pointer rests on the trigger or on the card itself.
//!
//! Unlike [`crate::Popover`] the surface opens from pointer hover, not
//! activation, and its content stays interactive: moving the pointer onto the
//! card keeps it open, leaving both trigger and card closes it after a short
//! grace delay. The surface carries no padding of its own — the content child
//! owns its padding so the hover-safe area covers the whole card.

use std::sync::Arc;

use nana_ui_core::{
    ContentFit, Icon, LengthSpec, OverflowSpec, PopoverAlignment, PopoverPlacement,
    SemanticColorRole,
};

use crate::gpu_slots::{HOST_TEXTURE_RENDERER, pack_gpu_revision};
use crate::popover::{MENU_OVERLAY_Z_INDEX, trigger_button_style};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, CustomRenderNode, InteractionState,
    MenuSurfaceKind, MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual,
    TriggeredMenuOverlay, UiWorld,
};

pub(crate) const HOVER_CARD_WIDTH: f32 = crate::popover::POPOVER_WIDTH;
const HOVER_CARD_GAP: f32 = nana_ui_core::space::SM;
/// Grace period after the pointer leaves both trigger and card.
pub const DEFAULT_CLOSE_DELAY_MS: u64 = 120;

/// Hover-opened anchored card (`nana.hover-card`). The trigger renders in
/// flow; card content mounts as children and projects onto the anchored
/// surface while open. `open` is framework-driven: hover lifecycle owns it.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverCard {
    /// Text trigger label, or the accessible name when the trigger is a
    /// glyph or an avatar.
    pub trigger: Arc<str>,
    pub trigger_icon: Option<Icon>,
    /// Host-texture resource for an avatar trigger. The circular chrome comes
    /// from the style; an empty resource renders the neutral placeholder.
    pub trigger_image: Option<Arc<str>>,
    /// Host texture generation and in-place content version. Together they
    /// form the Scene revision, matching [`crate::Avatar`] / [`crate::Thumbnail`].
    pub generation: u64,
    pub version: u64,
    pub trigger_size: f32,
    pub open: bool,
    pub placement: PopoverPlacement,
    pub alignment: PopoverAlignment,
    pub gap: f32,
    pub width: f32,
    pub open_delay_ms: u64,
    pub close_delay_ms: u64,
    pub close_on_escape: bool,
    /// Keep an outside editor focused during pointer interaction; keyboard navigation stays free.
    pub preserve_editor_focus: bool,
}

impl HoverCard {
    pub fn new() -> Self {
        Self {
            trigger: Arc::from(""),
            trigger_icon: None,
            trigger_image: None,
            generation: 0,
            version: 0,
            trigger_size: crate::avatar::DEFAULT_SIZE,
            open: false,
            placement: PopoverPlacement::Right,
            alignment: PopoverAlignment::Center,
            gap: HOVER_CARD_GAP,
            width: HOVER_CARD_WIDTH,
            open_delay_ms: 300,
            close_delay_ms: DEFAULT_CLOSE_DELAY_MS,
            close_on_escape: true,
            preserve_editor_focus: false,
        }
    }

    /// Text trigger with its label.
    pub fn trigger(mut self, trigger: impl Into<Arc<str>>) -> Self {
        self.trigger = trigger.into();
        self
    }

    /// Glyph trigger; the label stays the accessible name only.
    pub fn trigger_icon(mut self, icon: Icon, label: impl Into<Arc<str>>) -> Self {
        self.trigger = label.into();
        self.trigger_icon = Some(icon);
        self
    }

    /// Avatar trigger backed by a host-texture resource; the label is the
    /// accessible name.
    pub fn trigger_image(
        mut self,
        resource: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        self.trigger = label.into();
        self.trigger_image = Some(resource.into());
        self
    }

    pub fn trigger_size(mut self, size: f32) -> Self {
        self.trigger_size = sanitize_size(size);
        self
    }

    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn alignment(mut self, alignment: PopoverAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(crate::popover::MENU_MIN_WIDTH);
        self
    }

    pub fn open_delay(mut self, ms: u64) -> Self {
        self.open_delay_ms = ms;
        self
    }

    pub fn close_delay(mut self, ms: u64) -> Self {
        self.close_delay_ms = ms;
        self
    }

    pub fn close_on_escape(mut self, enabled: bool) -> Self {
        self.close_on_escape = enabled;
        self
    }

    /// Preserve an outside editor during pointer interaction and restore it when
    /// keyboard focus leaves this card on dismissal. Disabled by default.
    pub fn preserve_editor_focus(mut self, enabled: bool) -> Self {
        self.preserve_editor_focus = enabled;
        self
    }

    /// Records that the host refreshed the trigger slot's pixels in place.
    pub fn invalidate_content(&mut self) -> u64 {
        self.version = self.version.saturating_add(1);
        self.version
    }

    /// Records that the host replaced the underlying trigger view.
    pub fn replace_view(&mut self, generation: u64) -> u64 {
        self.generation = generation;
        self.generation
    }

    pub const fn revision(&self) -> u64 {
        pack_gpu_revision(self.generation, self.version)
    }

    /// Image trigger resource without the placeholder check, mirroring
    /// [`crate::Avatar::custom_render`].
    fn custom_render(&self) -> Option<CustomRenderNode> {
        let resource = self.trigger_image.as_deref()?.trim();
        if resource.is_empty() {
            return None;
        }
        Some(
            CustomRenderNode::new(HOST_TEXTURE_RENDERER, Arc::from(resource), self.revision())
                .with_fit(ContentFit::Cover),
        )
    }

    fn effective_style(&self) -> NodeStyle {
        let mut style = if self.trigger_image.is_some() {
            self.image_trigger_style()
        } else if self.trigger_icon.is_some() {
            self.icon_trigger_style()
        } else {
            trigger_button_style()
        };
        if self.open {
            Arc::make_mut(&mut style.layout).z_index = Some(MENU_OVERLAY_Z_INDEX);
        }
        style
    }

    /// Square Ghost chrome sized by [`Self::trigger_size`].
    fn icon_trigger_style(&self) -> NodeStyle {
        let size = sanitize_size(self.trigger_size);
        let mut style = NodeStyle {
            foreground: Some(SemanticColorRole::Muted),
            ..NodeStyle::default()
        };
        style.interaction.hovered.background = Some(SemanticColorRole::Hover);
        style.interaction.hovered.foreground = Some(SemanticColorRole::Text);
        style.interaction.pressed.background = Some(SemanticColorRole::Active);
        style.interaction.pressed.foreground = Some(SemanticColorRole::Text);
        let layout = Arc::make_mut(&mut style.layout);
        apply_trigger_size(layout, size);
        style.radius = Some(nana_ui_core::RadiusTier::Sm);
        style
    }

    /// Avatar chrome: circular clip, fixed box, neutral placeholder while the
    /// host texture is absent.
    fn image_trigger_style(&self) -> NodeStyle {
        let size = sanitize_size(self.trigger_size);
        let mut style = NodeStyle {
            background: Some(SemanticColorRole::Subtle),
            ..NodeStyle::default()
        };
        if self.custom_render().is_some() {
            style.background = None;
        }
        let layout = Arc::make_mut(&mut style.layout);
        apply_trigger_size(layout, size);
        layout.max_width = Some(LengthSpec::Px(size));
        layout.max_height = Some(LengthSpec::Px(size));
        layout.border_width = Some(0.0);
        layout.border_radius = Some(size * 0.5);
        layout.overflow_x = OverflowSpec::Hidden;
        layout.overflow_y = OverflowSpec::Hidden;
        style
    }
}

impl Default for HoverCard {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentView for HoverCard {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        activation: Some(crate::AppContext::activate_hover_card),
        activate_at: Some(crate::AppContext::activate_hover_card_at),
        ..crate::TypeBehavior::NONE
    };

    /// `open` is framework-owned (hover lifecycle); app-driven updates must
    /// not snap it shut while the pointer rests on the card.
    fn reconcile(&mut self, next: Self) {
        let open = self.open;
        *self = next;
        self.open = open;
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "hover-card".into(),
        }
    }

    /// The card content mounts and detaches under the anchored surface, so
    /// the projection must re-run with it to keep the origin math current.
    fn wants_child_reproject() -> bool {
        true
    }

    /// The framework flips `open` through the hover lifecycle; projection
    /// only mirrors it into the retained surface.
    fn wants_hover_tracking() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        // Empty image still uses avatar chrome; do not fall back to a text trigger.
        let trigger = (self.trigger_icon.is_none() && self.trigger_image.is_none())
            .then(|| Arc::clone(&self.trigger))
            .filter(|label| !label.is_empty());
        let visual = StandardVisual::MenuSurface {
            kind: MenuSurfaceKind::HoverCard,
            open: self.open,
            trigger: trigger.clone(),
            trigger_icon: self.trigger_icon,
            trigger_image: self.trigger_image.clone(),
            gap: self.gap,
            overlay: Some(TriggeredMenuOverlay {
                placement: self.placement,
                alignment: self.alignment,
                width: self.width.max(crate::popover::MENU_MIN_WIDTH),
                padding: 0.0,
                gap: self.gap,
                trigger_content: None,
            }),
            query: None,
            rows: Arc::from([]),
            highlighted: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        // Glyph and avatar triggers carry no text; their label lives in the
        // accessibility state only.
        let text = if self.trigger_icon.is_some() || self.trigger_image.is_some() {
            ""
        } else {
            self.trigger.as_ref()
        };
        if world.text(id) != Some(text) {
            mutations.set_text(
                id,
                crate::TextContent {
                    value: text.to_string().into(),
                },
            );
        }
        let custom = self.custom_render();
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        let label = (!self.trigger.is_empty()).then(|| Arc::clone(&self.trigger));
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            InteractionState {
                pointer_events: true,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Button,
                label,
                ..AccessibilityState::default()
            },
        );
    }
}

fn apply_trigger_size(layout: &mut nana_ui_core::LayoutStyle, size: f32) {
    let px = Some(LengthSpec::Px(size));
    layout.width = px;
    layout.height = px;
    layout.min_width = px;
    layout.min_height = px;
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
}

fn sanitize_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        crate::avatar::DEFAULT_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, LayoutViewport};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn tick(context: &mut AppContext, at_ms: u64) {
        context.advance_animations(std::time::Duration::from_millis(at_ms));
    }

    fn hover_at(
        context: &mut AppContext,
        document: DocumentId,
        target: Option<StableNodeId>,
        at_ms: u64,
    ) {
        context
            .set_pointer_hover_at(document, 1, target, std::time::Duration::from_millis(at_ms))
            .unwrap();
    }

    fn card_with_button() -> (
        AppContext,
        crate::Entity<HoverCard>,
        crate::Entity<crate::Button>,
    ) {
        let mut context = AppContext::new();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger("账户")
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let button = context
            .create_component(document(), crate::Button::new("退出登录"))
            .unwrap();
        context.append_child(card, button).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        (context, card, button)
    }

    fn relayout(context: &mut AppContext) {
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
    }

    fn hover_point(context: &mut AppContext, x: f32, y: f32, at_ms: u64) -> Option<StableNodeId> {
        context.rebuild_hit_test(document());
        let target = context.pointer_target(document(), x, y);
        hover_at(context, document(), target, at_ms);
        target
    }

    fn desktop_shell_account_hover_card() -> (
        AppContext,
        crate::Entity<HoverCard>,
        crate::Entity<crate::Button>,
        crate::Entity<crate::Stack>,
    ) {
        let mut context = AppContext::new();
        let card = context
            .create_detached_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Bottom)
                    .alignment(PopoverAlignment::End)
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let button = context
            .create_detached_component(document(), crate::Button::new("退出登录"))
            .unwrap();
        context.append_child(card, button).unwrap();
        let trailing = context
            .create_detached_component(
                document(),
                crate::Stack::row(4.0).with_layout(|layout| {
                    layout.height = Some(LengthSpec::Fill);
                    layout.align_items = nana_ui_core::AlignSpec::Center;
                }),
            )
            .unwrap();
        context.append_child(trailing, card).unwrap();
        let body = context
            .create_detached_component(document(), crate::Stack::fill_column(0.0).hittable())
            .unwrap();
        let shell = context
            .create_component(
                document(),
                crate::DesktopShell::new()
                    .title("LiliaBilibili")
                    .title_trailing(trailing.stable_id())
                    .title_window_controls(false)
                    .primary(body.stable_id()),
            )
            .unwrap();
        context.assemble_desktop_shell(shell).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        (context, card, button, body)
    }

    fn shell_with_titlebar_hover_card() -> (
        AppContext,
        crate::Entity<HoverCard>,
        crate::Entity<crate::Button>,
        crate::Entity<crate::Stack>,
    ) {
        let mut context = AppContext::new();
        let shell = context
            .create_component(document(), crate::Stack::fill_column(0.0))
            .unwrap();
        let titlebar = context
            .create_component(
                document(),
                crate::Stack::bar(0.0).with_layout(|layout| {
                    layout.height = Some(LengthSpec::Px(36.0));
                    layout.justify_content = nana_ui_core::JustifySpec::End;
                    layout.overflow_x = OverflowSpec::Hidden;
                }),
            )
            .unwrap();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Bottom)
                    .alignment(PopoverAlignment::End)
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let button = context
            .create_component(document(), crate::Button::new("退出登录"))
            .unwrap();
        context.append_child(card, button).unwrap();
        context.append_child(titlebar, card).unwrap();
        let body = context
            .create_component(document(), crate::Stack::fill_column(0.0).hittable())
            .unwrap();
        context.append_child(shell, titlebar).unwrap();
        context.append_child(shell, body).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        (context, card, button, body)
    }

    fn avatar_card_with_button() -> (
        AppContext,
        crate::Entity<HoverCard>,
        crate::Entity<crate::Button>,
    ) {
        let mut context = AppContext::new();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_image("", "账户")
                    .trigger_size(18.0)
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let button = context
            .create_component(document(), crate::Button::new("进入空间"))
            .unwrap();
        context.append_child(card, button).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        (context, card, button)
    }

    /// Overlay children receive `position: fixed` only from `effective_layout_style`.
    #[test]
    fn pointer_on_titlebar_card_content_beats_the_fill_body() {
        let (mut context, card, button, body) = shell_with_titlebar_hover_card();
        let card_id = card.stable_id();
        let button_id = button.stable_id();
        let body_id = body.stable_id();
        hover_at(&mut context, document(), Some(card_id), 0);
        tick(&mut context, 400);
        relayout(&mut context);
        context.rebuild_hit_test(document());
        let button_box = context.world().layout_box(button_id).unwrap();
        let trigger_box = context.world().layout_box(card_id).unwrap();
        let body_box = context.world().layout_box(body_id).unwrap();
        assert!(
            button_box.y >= trigger_box.y + trigger_box.height,
            "card hangs below the titlebar trigger: trigger={trigger_box:?} button={button_box:?}"
        );
        assert!(
            button_box.y + 1.0 >= body_box.y,
            "card content overlaps the fill body: body={body_box:?} button={button_box:?}"
        );
        let hx = button_box.x + button_box.width / 2.0;
        let hy = button_box.y + button_box.height / 2.0;
        let hit = context.pointer_target(document(), hx, hy);
        assert_eq!(
            hit,
            Some(button_id),
            "titlebar hover card must beat the fill body: trigger={trigger_box:?} button={button_box:?} body={body_box:?} hit=({hx},{hy}) -> {hit:?}"
        );
        hover_at(&mut context, document(), Some(button_id), 450);
        tick(&mut context, 800);
        assert!(
            context.read(card, |card| card.open).unwrap(),
            "pointer on the card content must not close it"
        );
    }

    fn assert_gap_path_keeps_open(
        context: &mut AppContext,
        card: crate::Entity<HoverCard>,
        content: crate::Entity<crate::Button>,
        placement: PopoverPlacement,
    ) {
        let card_id = card.stable_id();
        let content_id = content.stable_id();
        hover_at(context, document(), Some(card_id), 0);
        tick(context, 400);
        relayout(context);
        context.rebuild_hit_test(document());
        let trigger = context.world().layout_box(card_id).unwrap();
        let content_box = context.world().layout_box(content_id).unwrap();
        let (along_x, hangs, edge, mid) = match placement {
            PopoverPlacement::Bottom => (
                false,
                content_box.y >= trigger.y + trigger.height,
                trigger.y + trigger.height,
                (trigger.y + trigger.height + content_box.y) / 2.0,
            ),
            PopoverPlacement::Top => (
                false,
                content_box.y + content_box.height <= trigger.y,
                trigger.y,
                (content_box.y + content_box.height + trigger.y) / 2.0,
            ),
            PopoverPlacement::Right => (
                true,
                content_box.x >= trigger.x + trigger.width,
                trigger.x + trigger.width,
                (trigger.x + trigger.width + content_box.x) / 2.0,
            ),
            PopoverPlacement::Left => (
                true,
                content_box.x + content_box.width <= trigger.x,
                trigger.x,
                (content_box.x + content_box.width + trigger.x) / 2.0,
            ),
        };
        assert!(
            hangs,
            "card hangs on {placement:?}: trigger={trigger:?} content={content_box:?}"
        );
        let step = match placement {
            PopoverPlacement::Bottom | PopoverPlacement::Right => 1.0,
            PopoverPlacement::Top | PopoverPlacement::Left => -1.0,
        };
        let cx = trigger.x + trigger.width / 2.0;
        let cy = trigger.y + trigger.height / 2.0;
        let (edge_x, edge_y, mid_x, mid_y) = if along_x {
            (edge + step, cy, mid, cy)
        } else {
            (cx, edge + step, cx, mid)
        };
        let path = [
            (cx, cy, 450u64),
            (edge_x, edge_y, 580),
            (mid_x, mid_y, 710),
            (
                content_box.x + content_box.width / 2.0,
                content_box.y + content_box.height / 2.0,
                840,
            ),
        ];
        for (x, y, at_ms) in path {
            let hit = hover_point(context, x, y, at_ms);
            tick(context, at_ms + 130);
            assert!(
                context.read(card, |card| card.open).unwrap(),
                "open must survive ({x},{y}) hit={hit:?} trigger={trigger:?} content={content_box:?}"
            );
        }
    }

    /// DesktopShell trailing HoverCard: the gap is outside the subtree.
    #[test]
    fn pointer_path_from_titlebar_trigger_across_the_gap_keeps_the_card_open() {
        let (mut context, card, button, body) = desktop_shell_account_hover_card();
        assert_gap_path_keeps_open(&mut context, card, button, PopoverPlacement::Bottom);
        let button_id = button.stable_id();
        let content = context.world().layout_box(button_id).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        assert_eq!(
            context.pointer_target(
                document(),
                content.x + content.width / 2.0,
                content.y + content.height / 2.0
            ),
            Some(button_id),
            "titlebar hover card must beat the fill body: content={content:?} body={body_box:?}"
        );
    }

    #[test]
    fn pointer_path_across_a_top_gap_keeps_the_card_open() {
        let mut context = AppContext::new();
        let root = context
            .create_component(document(), crate::Stack::column(0.0))
            .unwrap();
        let spacer = context
            .create_component(
                document(),
                crate::Stack::column(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(28.0));
                    layout.height = Some(LengthSpec::Px(200.0));
                }),
            )
            .unwrap();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Top)
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let button = context
            .create_component(document(), crate::Button::new("进入空间"))
            .unwrap();
        context.append_child(card, button).unwrap();
        context.append_child(root, spacer).unwrap();
        context.append_child(root, card).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        assert_gap_path_keeps_open(&mut context, card, button, PopoverPlacement::Top);
    }

    #[test]
    fn pointer_path_across_a_right_gap_keeps_the_card_open() {
        let (mut context, card, button) = card_with_button();
        assert_gap_path_keeps_open(&mut context, card, button, PopoverPlacement::Right);
    }

    fn card_with_stack_body(
        hittable: bool,
    ) -> (
        AppContext,
        crate::Entity<HoverCard>,
        crate::Entity<crate::Stack>,
        crate::Entity<crate::Button>,
    ) {
        let mut context = AppContext::new();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Bottom)
                    .open_delay(0)
                    .close_delay(120),
            )
            .unwrap();
        let mut body = crate::Stack::column(8.0).with_layout(|layout| {
            layout.padding_left = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(12.0));
            layout.padding_top = Some(LengthSpec::Px(12.0));
            layout.padding_bottom = Some(LengthSpec::Px(12.0));
        });
        if hittable {
            body = body.hittable();
        }
        let body = context.create_component(document(), body).unwrap();
        let button = context
            .create_component(document(), crate::Button::new("进入空间"))
            .unwrap();
        context.append_child(body, button).unwrap();
        context.append_child(card, body).unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        (context, card, body, button)
    }

    /// A non-hittable content Stack must not become the gap pointer target.
    #[test]
    fn gap_over_a_non_interactive_card_body_is_a_valid_pointer_target() {
        let (mut context, card, body, _) = card_with_stack_body(false);
        hover_at(&mut context, document(), Some(card.stable_id()), 0);
        tick(&mut context, 400);
        relayout(&mut context);
        context.rebuild_hit_test(document());
        let trigger = context.world().layout_box(card.stable_id()).unwrap();
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        let hit = context.pointer_target(
            document(),
            trigger.x + trigger.width / 2.0,
            (trigger.y + trigger.height + body_box.y) / 2.0,
        );
        assert_ne!(hit, Some(body.stable_id()));
        context
            .set_pointer_hover_at(document(), 1, hit, std::time::Duration::from_millis(450))
            .unwrap();
    }

    #[test]
    fn pointer_on_hittable_card_body_padding_keeps_the_card_open() {
        let (mut context, card, body, button) = card_with_stack_body(true);
        assert_gap_path_keeps_open(&mut context, card, button, PopoverPlacement::Bottom);
        let body_box = context.world().layout_box(body.stable_id()).unwrap();
        let hit = hover_point(&mut context, body_box.x + 6.0, body_box.y + 6.0, 900);
        assert_eq!(hit, Some(body.stable_id()));
        tick(&mut context, 1100);
        assert!(context.read(card, |card| card.open).unwrap());
    }

    /// Avatar triggers clip their circular chrome; the open card must still
    /// receive pointer hits and keep the surface open.
    #[test]
    fn pointer_on_avatar_card_content_outside_the_trigger_keeps_it_open() {
        let (mut context, card, button) = avatar_card_with_button();
        let card_id = card.stable_id();
        let button_id = button.stable_id();
        hover_at(&mut context, document(), Some(card_id), 0);
        tick(&mut context, 400);
        relayout(&mut context);
        context.rebuild_hit_test(document());
        let button_box = context.world().layout_box(button_id).unwrap();
        let trigger_box = context.world().layout_box(card_id).unwrap();
        assert!(
            button_box.x >= trigger_box.x + trigger_box.width
                || button_box.y >= trigger_box.y + trigger_box.height,
            "card content must sit outside the 18px trigger: trigger={trigger_box:?} button={button_box:?}"
        );
        let hx = button_box.x + button_box.width / 2.0;
        let hy = button_box.y + button_box.height / 2.0;
        let hit = context.pointer_target(document(), hx, hy);
        assert_eq!(
            hit,
            Some(button_id),
            "trigger={trigger_box:?} button={button_box:?} hit=({hx},{hy}) -> {hit:?}"
        );
        hover_at(&mut context, document(), Some(button_id), 450);
        tick(&mut context, 800);
        assert!(context.read(card, |card| card.open).unwrap());
    }

    fn open_card(context: &mut AppContext, card: crate::Entity<HoverCard>) {
        hover_at(context, document(), Some(card.stable_id()), 0);
        tick(context, 400);
        relayout(context);
        context.rebuild_hit_test(document());
    }

    /// A trigger with no room below it for a Bottom card opens the card
    /// above it instead of clamping it back over the trigger.
    #[test]
    fn a_card_without_room_below_opens_above_its_trigger() {
        let mut context = AppContext::new();
        let root = context
            .create_component(document(), crate::Stack::column(0.0))
            .unwrap();
        let spacer = context
            .create_component(
                document(),
                crate::Stack::column(0.0).with_layout(|layout| {
                    layout.height = Some(LengthSpec::Px(500.0));
                }),
            )
            .unwrap();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Bottom)
                    .alignment(PopoverAlignment::Start)
                    .open_delay(0),
            )
            .unwrap();
        let body = context
            .create_component(
                document(),
                crate::Stack::column(0.0).with_layout(|layout| {
                    layout.height = Some(LengthSpec::Px(200.0));
                }),
            )
            .unwrap();
        context.append_child(card, body).unwrap();
        context.append_child(root, spacer).unwrap();
        context.append_child(root, card).unwrap();
        relayout(&mut context);
        open_card(&mut context, card);
        let trigger = context.world().layout_box(card.stable_id()).unwrap();
        let content = context.world().layout_box(body.stable_id()).unwrap();
        assert!(
            content.y + content.height <= trigger.y && content.y >= 0.0,
            "the card opens above the trigger, inside the viewport: trigger={trigger:?} content={content:?}"
        );
        assert_eq!(content.x, trigger.x, "start alignment keeps the left edges");
    }

    /// The card is viewport-fixed, so it hangs off the trigger where the
    /// page's scroll shows it, and the pointer reaches it there.
    #[test]
    fn a_scrolled_trigger_anchors_the_card_where_it_shows() {
        let mut context = AppContext::new();
        let page = context
            .create_component(
                document(),
                crate::ScrollView::new(crate::ScrollAxes::Vertical).style(
                    crate::Stack::column(0.0)
                        .width(LengthSpec::Fill)
                        .height(LengthSpec::Px(400.0))
                        .node_style(),
                ),
            )
            .unwrap();
        let column = context
            .create_component(
                document(),
                crate::Stack::column(0.0).width(LengthSpec::Fill),
            )
            .unwrap();
        let spacer = context
            .create_component(
                document(),
                crate::Stack::column(0.0).height(LengthSpec::Px(700.0)),
            )
            .unwrap();
        let tail = context
            .create_component(
                document(),
                crate::Stack::column(0.0).height(LengthSpec::Px(700.0)),
            )
            .unwrap();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0)
                    .placement(PopoverPlacement::Bottom)
                    .alignment(PopoverAlignment::Start)
                    .open_delay(0),
            )
            .unwrap();
        let button = context
            .create_component(document(), crate::Button::new("进入空间"))
            .unwrap();
        context.append_child(card, button).unwrap();
        context.append_child(page, column).unwrap();
        context.append_child(column, spacer).unwrap();
        context.append_child(column, card).unwrap();
        context.append_child(column, tail).unwrap();
        relayout(&mut context);
        context
            .scroll_to(page, crate::ScrollOffset { x: 0.0, y: 600.0 })
            .unwrap();
        relayout(&mut context);
        open_card(&mut context, card);
        let shown = context
            .world()
            .presentation_input_bounds(card.stable_id())
            .unwrap();
        assert!((shown.y - 100.0).abs() < 0.5, "trigger shows at {shown:?}");
        let content = context.world().layout_box(button.stable_id()).unwrap();
        assert!(
            content.y >= shown.y + shown.height && content.y < shown.y + shown.height + 16.0,
            "the card hangs just below the shown trigger: shown={shown:?} content={content:?}"
        );
        assert_eq!(
            context.pointer_target(
                document(),
                content.x + content.width / 2.0,
                content.y + content.height / 2.0
            ),
            Some(button.stable_id())
        );
    }

    #[test]
    fn activating_the_trigger_emits_activate() {
        let (mut context, card, _) = card_with_button();
        let received = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&received);
        context
            .on(card, move |_, _: &crate::Activate, _| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .unwrap();
        assert!(context.activate_node(card.stable_id()).unwrap());
        assert!(received.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Hovering the trigger opens the anchored surface after the configured
    /// delay, and the card content projects as a viewport-fixed overlay.
    #[test]
    fn hovering_the_trigger_opens_the_card_after_the_delay() {
        let (mut context, card, button) = card_with_button();
        context
            .update_component(card, |card, _| card.open_delay_ms = 100)
            .unwrap();
        let card_id = card.stable_id();
        hover_at(&mut context, document(), Some(card_id), 0);
        tick(&mut context, 40);
        assert!(!context.read(card, |card| card.open).unwrap());
        tick(&mut context, 140);
        assert!(context.read(card, |card| card.open).unwrap());
        relayout(&mut context);
        // The open card keeps its trigger in flow; the button floats free.
        let trigger_box = context.world().layout_box(card_id).unwrap();
        let button_box = context.world().layout_box(button.stable_id()).unwrap();
        let style = context.world().layout_style(button.stable_id()).unwrap();
        assert_eq!(style.position, nana_ui_core::PositionSpec::Fixed);
        assert!(
            button_box.x >= trigger_box.x + trigger_box.width,
            "card content hangs beside the trigger: trigger={trigger_box:?} button={button_box:?}"
        );
    }

    /// Moving the pointer onto the open card keeps it open past the close
    /// grace; only leaving both trigger and card closes it.
    #[test]
    fn moving_onto_the_card_keeps_it_open() {
        let (mut context, card, button) = card_with_button();
        let card_id = card.stable_id();
        let button_id = button.stable_id();
        hover_at(&mut context, document(), Some(card_id), 0);
        tick(&mut context, 400);
        assert!(context.read(card, |card| card.open).unwrap());
        hover_at(&mut context, document(), Some(button_id), 450);
        tick(&mut context, 800);
        assert!(
            context.read(card, |card| card.open).unwrap(),
            "pointer on the card content must not close it"
        );
        hover_at(&mut context, document(), None, 850);
        tick(&mut context, 1000);
        assert!(!context.read(card, |card| card.open).unwrap());
    }

    /// A scheduled open is cancelled when the pointer leaves before the
    /// delay elapses.
    #[test]
    fn leaving_before_the_delay_cancels_the_open() {
        let (mut context, card, _) = card_with_button();
        context
            .update_component(card, |card, _| card.open_delay_ms = 100)
            .unwrap();
        hover_at(&mut context, document(), Some(card.stable_id()), 0);
        hover_at(&mut context, document(), None, 40);
        tick(&mut context, 400);
        assert!(!context.read(card, |card| card.open).unwrap());
        assert_eq!(context.next_animation_deadline(), None);
    }

    /// App-driven data updates re-enter through `reconcile` (the mount
    /// upsert path) and must keep the framework-owned `open` state, so a
    /// session refresh cannot snap the card shut under a resting pointer.
    #[test]
    fn reconcile_preserves_the_framework_open_state() {
        let (mut context, card, _) = card_with_button();
        hover_at(&mut context, document(), Some(card.stable_id()), 0);
        tick(&mut context, 400);
        assert!(context.read(card, |card| card.open).unwrap());
        context
            .update_component(card, |card, _| {
                card.reconcile(HoverCard::new().trigger("账户"));
            })
            .unwrap();
        assert!(
            context.read(card, |card| card.open).unwrap(),
            "reconcile must preserve the framework-driven open state"
        );
    }

    /// Escape closes an open hover card that allows it.
    #[test]
    fn escape_closes_the_open_card() {
        let (mut context, card, _) = card_with_button();
        hover_at(&mut context, document(), Some(card.stable_id()), 0);
        tick(&mut context, 400);
        assert!(context.read(card, |card| card.open).unwrap());
        assert!(context.dismiss_popovers_on_escape().unwrap());
        assert!(!context.read(card, |card| card.open).unwrap());
    }

    /// The avatar trigger renders through the host-texture slot and keeps a
    /// neutral placeholder chrome while the texture is absent.
    #[test]
    fn avatar_trigger_carries_host_texture_and_placeholder() {
        let mut context = AppContext::new();
        let card = context
            .create_component(
                document(),
                HoverCard::new().trigger_image("user.avatar", "账户"),
            )
            .unwrap();
        let id = card.stable_id();
        let custom = context.world().custom_render(id).unwrap();
        assert_eq!(custom.renderer.as_ref(), HOST_TEXTURE_RENDERER);
        assert!(context.world().text(id).is_none_or(|text| text.is_empty()));
        let style = context.world().node_style(id).unwrap();
        assert!(style.background.is_none(), "loaded avatars paint no chrome");
        assert_eq!(
            context
                .read(card, |card| card.trigger.as_ref().to_owned())
                .unwrap(),
            "账户"
        );

        let placeholder = context
            .create_component(document(), HoverCard::new().trigger_image("", "账户"))
            .unwrap();
        assert!(
            context
                .world()
                .custom_render(placeholder.stable_id())
                .is_none()
        );
        let placeholder_style = context.world().node_style(placeholder.stable_id()).unwrap();
        assert_eq!(
            placeholder_style.background,
            Some(SemanticColorRole::Subtle)
        );
    }

    #[test]
    fn icon_trigger_is_a_ghost_square_honoring_trigger_size() {
        let mut context = AppContext::new();
        let card = context
            .create_component(
                document(),
                HoverCard::new()
                    .trigger_icon(Icon::Add, "账号")
                    .trigger_size(28.0),
            )
            .unwrap();
        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let style = context.world().node_style(card.stable_id()).unwrap();
        assert_eq!(style.layout.align_self, None);
        assert_eq!(style.layout.width, Some(LengthSpec::Px(28.0)));
        assert_eq!(style.layout.height, Some(LengthSpec::Px(28.0)));
        assert!(style.background.is_none());
        assert!(style.border.is_none());
        let bounds = context.world().layout_box(card.stable_id()).unwrap();
        assert!((bounds.width - 28.0).abs() < f32::EPSILON);
        assert!((bounds.height - 28.0).abs() < f32::EPSILON);
    }

    #[test]
    fn opening_does_not_scale_or_fade_the_trigger() {
        let (mut context, card, _) = card_with_button();
        hover_at(&mut context, document(), Some(card.stable_id()), 0);
        tick(&mut context, 400);
        assert!(context.read(card, |card| card.open).unwrap());
        let extracted = context.world().extract_nodes(&[card.stable_id()]);
        let layout = extracted[0].source_style.layout.as_ref();
        assert!(layout.transform.is_none());
        assert!(layout.opacity.is_none() || layout.opacity == Some(1.0));
        assert_eq!(layout.z_index, Some(MENU_OVERLAY_Z_INDEX));
    }

    #[test]
    fn late_host_texture_moves_the_trigger_revision() {
        let mut card = HoverCard::new().trigger_image("user.avatar", "账户");
        let first = card.custom_render().expect("ready slot").revision;

        card.replace_view(1);
        let replaced = card.custom_render().expect("ready slot").revision;
        assert_ne!(first, replaced, "a replaced view is a new revision");

        card.invalidate_content();
        let refreshed = card.custom_render().expect("ready slot").revision;
        assert_ne!(replaced, refreshed, "in-place pixels are a new revision");
        assert_eq!(refreshed, pack_gpu_revision(1, 1));
    }
}
