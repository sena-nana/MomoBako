use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, LengthSpec, PopoverAlignment, PositionSpec,
    SemanticColorRole,
};

use crate::panel::{origin, positive};
use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, PanelInsets, StableNodeId, TextContent, UiWorld,
};

const TITLE_SIZE: f32 = nana_ui_core::type_scale::META;
const TITLE_WEIGHT: u16 = nana_ui_core::type_scale::SEMIBOLD;
const DESCRIPTION_SIZE: f32 = nana_ui_core::type_scale::HINT;
pub(crate) const COPY_GAP: f32 = nana_ui_core::space::XXS;
pub(crate) const INDICATOR_SIZE: f32 = nana_ui_core::space::SM;
pub(crate) const INDICATOR_GAP: f32 = nana_ui_core::space::MD;
pub(crate) const PAD_Y: f32 = nana_ui_core::space::LG;
pub(crate) const PAD_X: f32 = nana_ui_core::space::XL;

fn sanitize_description(description: Option<&Arc<str>>) -> Option<Arc<str>> {
    description.filter(|value| !value.is_empty()).cloned()
}

fn inert() -> InteractionState {
    InteractionState {
        pointer_events: false,
        focusable: false,
    }
}

fn dismiss_target() -> InteractionState {
    InteractionState {
        pointer_events: true,
        focusable: true,
    }
}

/// Dismiss request from a dismissible toast. Toast does not own a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastDismissed;

pub use nana_ui_core::ToastTone;

/// Label-only outlined notification. Optional dismiss is a hit target, not a timer.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub title: Arc<str>,
    pub description: Option<Arc<str>>,
    pub tone: ToastTone,
    pub dismissible: bool,
    pub style: NodeStyle,
}

impl Toast {
    pub fn new(title: impl Into<Arc<str>>, tone: ToastTone) -> Self {
        Self {
            title: title.into(),
            description: None,
            tone,
            dismissible: false,
            style: NodeStyle::default(),
        }
    }

    pub fn description(mut self, description: impl Into<Arc<str>>) -> Self {
        let description = description.into();
        self.description = (!description.is_empty()).then_some(description);
        self
    }

    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// Pin the toast into the space `insets` leaves free in `viewport`, using
    /// the same chrome reservation contract as [`crate::Panel::viewport`].
    /// `align` distributes it across that free width and `max_width` caps it.
    /// The toast keeps its intrinsic height and rests on the reserved bottom
    /// edge, so `viewport` must be the positioned host's own box. This updates
    /// geometry only; authored padding and visual style survive.
    pub fn place_in(
        &mut self,
        viewport: LayoutBox,
        align: PopoverAlignment,
        max_width: f32,
        insets: PanelInsets,
    ) {
        let insets = insets.clamp_into(viewport);
        let free = positive(viewport.width) - insets.left - insets.right;
        let width = positive(max_width).min(free);
        let leading = match align {
            PopoverAlignment::Start => 0.0,
            PopoverAlignment::Center => (free - width) / 2.0,
            PopoverAlignment::End => free - width,
        };
        let layout = Arc::make_mut(&mut self.style.layout);
        layout.position = PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(origin(viewport.x) + insets.left + leading));
        layout.offset_right = None;
        layout.offset_top = None;
        layout.offset_bottom = Some(LengthSpec::Px(insets.bottom));
        layout.width = Some(LengthSpec::Px(width));
    }

    pub fn tone_role(&self) -> SemanticColorRole {
        crate::components::status_tone_role(self.tone.status())
    }

    pub fn resolved_description(&self) -> Option<Arc<str>> {
        sanitize_description(self.description.as_ref())
    }

    fn copy_height(&self) -> f32 {
        if self.resolved_description().is_some() {
            TITLE_SIZE + COPY_GAP + DESCRIPTION_SIZE
        } else {
            TITLE_SIZE.max(INDICATOR_SIZE)
        }
    }

    fn effective_style(&self, world: &UiWorld) -> NodeStyle {
        let metrics = world.theme_metrics();
        let dismiss = ControlSize::Small.height_in(metrics);
        let content_height = if self.dismissible {
            self.copy_height().max(dismiss)
        } else {
            self.copy_height()
        };
        let mut style = self.style.clone();
        style.foreground = Some(SemanticColorRole::Text);
        style.background = Some(SemanticColorRole::Surface);
        style.border = Some(SemanticColorRole::Border);
        style.text_vertical_alignment = crate::TextVerticalAlignment::Center;
        let layout = Arc::make_mut(&mut style.layout);
        // A placed toast owns its width; only an unplaced one fills its row.
        if layout.width.is_none() {
            layout.width = Some(LengthSpec::Fill);
        }
        layout.direction = Some(FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.gap = Some(LengthSpec::Px(INDICATOR_GAP));
        layout.padding_left = Some(LengthSpec::Px(PAD_X));
        layout.padding_right = Some(LengthSpec::Px(PAD_X));
        layout.padding_top = Some(LengthSpec::Px(PAD_Y));
        layout.padding_bottom = Some(LengthSpec::Px(PAD_Y));
        layout.min_height = Some(LengthSpec::Px(PAD_Y + content_height + PAD_Y));
        layout.border_width = Some(nana_ui_core::HAIRLINE);
        layout.border_radius = Some(metrics.radius_md);
        layout.font_size = Some(TITLE_SIZE);
        layout.font_weight = Some(TITLE_WEIGHT);
        style
    }
}

impl ComponentView for Toast {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "toast".into(),
        }
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = crate::StandardVisual::Toast {
            title: Arc::clone(&self.title),
            description: self.resolved_description(),
            tone: self.tone,
            dismissible: self.dismissible,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        if world.text(id) != Some(self.title.as_ref()) {
            mutations.set_text(
                id,
                TextContent {
                    value: self.title.to_string().into(),
                },
            );
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(world),
            if self.dismissible {
                dismiss_target()
            } else {
                inert()
            },
            AccessibilityState {
                role: AccessibilityRole::Status,
                label: Some(Arc::clone(&self.title)),
                description: self.resolved_description(),
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;
    use std::sync::{Arc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn toast_constructs_and_maps_tone() {
        let empty = Toast::new("Saved", ToastTone::Success).description("");
        assert!(empty.description.is_none());
        assert_eq!(empty.resolved_description(), None);
        assert_eq!(ToastTone::Info.status(), nana_ui_core::StatusTone::Info);
        assert_eq!(
            Toast::new("A", ToastTone::Info).tone_role(),
            SemanticColorRole::Accent
        );
        assert_eq!(
            Toast::new("A", ToastTone::Success).tone_role(),
            SemanticColorRole::Success
        );
        assert_eq!(
            Toast::new("A", ToastTone::Warning).tone_role(),
            SemanticColorRole::Warning
        );
        assert_eq!(
            Toast::new("A", ToastTone::Danger).tone_role(),
            SemanticColorRole::Danger
        );

        let mut context = AppContext::new();
        let toast = context
            .create_component(
                document(),
                Toast::new("Saved", ToastTone::Success).description("Project exported"),
            )
            .unwrap();
        let id = toast.stable_id();

        assert!(matches!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == "toast"
        ));
        assert!(matches!(
            context.world().standard_visual(id),
            Some(crate::StandardVisual::Toast {
                tone: ToastTone::Success,
                dismissible: false,
                ..
            })
        ));
        assert_eq!(context.world().text(id), Some("Saved"));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.background, Some(SemanticColorRole::Surface));
        assert_eq!(style.border, Some(SemanticColorRole::Border));
        assert_eq!(style.foreground, Some(SemanticColorRole::Text));
        assert_eq!(style.layout.border_width, Some(nana_ui_core::HAIRLINE));
        assert_eq!(
            style.layout.border_radius,
            Some(context.world().theme_metrics().radius_md)
        );
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(style.layout.padding_left, Some(LengthSpec::Px(PAD_X)));
        assert_eq!(style.layout.padding_right, Some(LengthSpec::Px(PAD_X)));
        assert_eq!(style.layout.padding_top, Some(LengthSpec::Px(PAD_Y)));
        assert_eq!(style.layout.padding_bottom, Some(LengthSpec::Px(PAD_Y)));
        assert_eq!(style.layout.font_size, Some(TITLE_SIZE));
        assert_eq!(style.layout.font_weight, Some(TITLE_WEIGHT));
        assert_eq!(
            style.layout.min_height,
            Some(LengthSpec::Px(
                PAD_Y + TITLE_SIZE + COPY_GAP + DESCRIPTION_SIZE + PAD_Y
            ))
        );
        assert_eq!(context.world().interaction(id), Some(inert()));
    }

    #[test]
    fn toast_dismissible_vs_inert() {
        let mut context = AppContext::new();
        let toast = context
            .create_component(document(), Toast::new("Copied", ToastTone::Info))
            .unwrap();
        let id = toast.stable_id();
        let dismiss = ControlSize::Small.height_in(context.world().theme_metrics());

        assert!(!context.read(toast, |toast| toast.dismissible).unwrap());
        assert_eq!(context.world().interaction(id), Some(inert()));
        assert_eq!(
            context.world().node_style(id).unwrap().layout.min_height,
            Some(LengthSpec::Px(
                PAD_Y + TITLE_SIZE.max(INDICATOR_SIZE) + PAD_Y
            ))
        );

        context
            .update_component(toast, |toast, _| {
                toast.dismissible = true;
            })
            .unwrap();
        assert_eq!(context.world().interaction(id), Some(dismiss_target()));
        assert_eq!(
            context.world().node_style(id).unwrap().layout.min_height,
            Some(LengthSpec::Px(PAD_Y + dismiss + PAD_Y))
        );

        let dismissed = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&dismissed);
        context
            .on(toast, move |_toast, _event: &ToastDismissed, _cx| {
                *flag.lock().unwrap() = true;
            })
            .unwrap();
        context
            .update_component(toast, |_toast, cx| {
                cx.emit(ToastDismissed);
            })
            .unwrap();
        assert!(*dismissed.lock().unwrap());

        context
            .update_component(toast, |toast, _| {
                toast.dismissible = false;
            })
            .unwrap();
        assert_eq!(context.world().interaction(id), Some(inert()));
    }

    #[test]
    fn toast_accessibility_label() {
        let mut context = AppContext::new();
        let toast = context
            .create_component(
                document(),
                Toast::new("Sync failed", ToastTone::Danger).description("Network offline"),
            )
            .unwrap();
        let id = toast.stable_id();
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::Status);
        assert_eq!(accessibility.label.as_deref(), Some("Sync failed"));
        assert_eq!(
            accessibility.description.as_deref(),
            Some("Network offline")
        );

        context
            .update_component(toast, |toast, _| {
                toast.description = Some(Arc::from(""));
            })
            .unwrap();
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.label.as_deref(), Some("Sync failed"));
        assert_eq!(accessibility.description, None);
    }

    #[test]
    fn idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let toast = context
            .create_component(
                document(),
                Toast::new("Ready", ToastTone::Info)
                    .description("Idle")
                    .dismissible(true),
            )
            .unwrap();
        let _ = context.take_system_work();
        context.reproject_component(toast).unwrap();
        assert!(context.take_system_work().is_empty());
    }

    #[test]
    fn place_in_centres_within_reserved_chrome_and_survives_a_tiny_window() {
        let mut toast = Toast::new("Saved", ToastTone::Info);
        let insets = crate::PanelInsets {
            top: 48.0,
            right: 416.0,
            bottom: 100.0,
            left: 16.0,
        };
        let viewport = crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        };
        toast.place_in(viewport, PopoverAlignment::Center, 360.0, insets);
        let layout = &toast.style.layout;
        assert_eq!(layout.position, PositionSpec::Absolute);
        assert_eq!(layout.width, Some(LengthSpec::Px(360.0)));
        assert_eq!(layout.offset_left, Some(LengthSpec::Px(120.0)));
        assert_eq!(layout.offset_bottom, Some(LengthSpec::Px(100.0)));
        assert_eq!(layout.offset_right, None);

        toast.place_in(viewport, PopoverAlignment::End, 360.0, insets);
        assert_eq!(
            toast.style.layout.offset_left,
            Some(LengthSpec::Px(1000.0 - 416.0 - 360.0))
        );

        let tiny = crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        toast.place_in(tiny, PopoverAlignment::Center, 360.0, insets);
        let layout = &toast.style.layout;
        assert_eq!(layout.width, Some(LengthSpec::Px(0.0)));
        assert_eq!(layout.offset_left, Some(LengthSpec::Px(16.0)));
        assert_eq!(layout.offset_bottom, Some(LengthSpec::Px(2.0)));
    }

    #[test]
    fn a_placed_toast_keeps_its_width_through_projection() {
        let mut context = AppContext::new();
        let mut toast = Toast::new("Saved", ToastTone::Info);
        toast.place_in(
            crate::LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
            PopoverAlignment::Center,
            360.0,
            crate::PanelInsets::default(),
        );
        let entity = context.create_component(document(), toast).unwrap();
        let style = context.world().node_style(entity.stable_id()).unwrap();
        assert_eq!(style.layout.width, Some(LengthSpec::Px(360.0)));
    }
}
