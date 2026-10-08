//! Toolbar and status bar: the two horizontal strips a desktop shell has that
//! `AppTitleBar` does not cover.
//!
//! Both are containers — the application puts its own controls in them. What
//! they add over a plain [`crate::Stack`] row is the shell surface treatment
//! and, more importantly, the accessibility role: a screen reader announces a
//! toolbar's contents as a group of controls and a status bar as a live status
//! region, which a bare layout box cannot convey.

use std::sync::Arc;

use nana_ui_core::{AlignSpec, FlexDirection, JustifySpec, LengthSpec, SemanticColorRole, space};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, MutationQueue,
    NodeKind, NodeStyle, StableNodeId, UiWorld,
};

/// Strip of actions above the content (`nana.toolbar`).
#[derive(Debug, Clone, PartialEq)]
pub struct Toolbar {
    /// Accessible name, so a window with several toolbars is navigable.
    pub label: Option<Arc<str>>,
    /// Draws the shell surface and a bottom hairline. Off for a toolbar nested
    /// in a surface that already provides them.
    pub chrome: bool,
    pub style: NodeStyle,
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            label: None,
            chrome: true,
            style: bar_style(true, false),
        }
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn chrome(mut self, chrome: bool) -> Self {
        self.chrome = chrome;
        self.style = bar_style(chrome, false);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip of state below the content (`nana.status-bar`).
///
/// Announced as a live status region: assistive technology reports changes
/// here without the user moving focus to it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusBar {
    pub label: Option<Arc<str>>,
    pub chrome: bool,
    pub style: NodeStyle,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            label: None,
            chrome: true,
            style: bar_style(true, true),
        }
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn chrome(mut self, chrome: bool) -> Self {
        self.chrome = chrome;
        self.style = bar_style(chrome, true);
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared strip geometry. The hairline sits on the edge facing the content.
fn bar_style(chrome: bool, below_content: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.direction = Some(FlexDirection::Row);
    layout.align_items = AlignSpec::Center;
    layout.justify_content = JustifySpec::Start;
    layout.gap = Some(LengthSpec::Px(space::SM));
    layout.width = Some(LengthSpec::Fill);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.padding_left = Some(LengthSpec::Px(space::MD));
    layout.padding_right = Some(LengthSpec::Px(space::MD));
    layout.padding_top = Some(LengthSpec::Px(space::XS));
    layout.padding_bottom = Some(LengthSpec::Px(space::XS));
    if chrome {
        style.background = Some(SemanticColorRole::Surface);
        style.border = Some(SemanticColorRole::BorderSoft);
        layout.border_width = Some(nana_ui_core::HAIRLINE);
        // Only the edge that meets the content is drawn.
        if below_content {
            layout.border_top_width = Some(nana_ui_core::HAIRLINE);
            layout.border_right_width = Some(0.0);
            layout.border_bottom_width = Some(0.0);
            layout.border_left_width = Some(0.0);
        } else {
            layout.border_top_width = Some(0.0);
            layout.border_right_width = Some(0.0);
            layout.border_bottom_width = Some(nana_ui_core::HAIRLINE);
            layout.border_left_width = Some(0.0);
        }
    }
    style
}

fn project_bar(
    id: StableNodeId,
    world: &UiWorld,
    mutations: &mut MutationQueue,
    style: &NodeStyle,
    role: AccessibilityRole,
    label: Option<&Arc<str>>,
) {
    project_common(
        id,
        world,
        mutations,
        style,
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
        AccessibilityState {
            role,
            label: label.cloned(),
            ..AccessibilityState::default()
        },
    );
}

impl ComponentView for Toolbar {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "toolbar".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_bar(
            id,
            world,
            mutations,
            &self.style,
            AccessibilityRole::Toolbar,
            self.label.as_ref(),
        );
    }
}

impl ComponentView for StatusBar {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "status-bar".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_bar(
            id,
            world,
            mutations,
            &self.style,
            AccessibilityRole::Status,
            self.label.as_ref(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    #[test]
    fn the_bars_carry_the_roles_a_screen_reader_needs() {
        let mut cx = AppContext::new();
        let toolbar = cx
            .create_component(document(), Toolbar::new().label("Formatting"))
            .unwrap();
        let status = cx
            .create_component(document(), StatusBar::new().label("Document status"))
            .unwrap();

        let semantics = |id| {
            let state = cx.world().accessibility(id).cloned().unwrap();
            (state.role, state.label.map(|label| label.to_string()))
        };
        assert_eq!(
            semantics(toolbar.stable_id()),
            (AccessibilityRole::Toolbar, Some("Formatting".to_owned()))
        );
        assert_eq!(
            semantics(status.stable_id()),
            (
                AccessibilityRole::Status,
                Some("Document status".to_owned())
            )
        );
    }

    #[test]
    fn the_hairline_sits_on_the_edge_that_meets_the_content() {
        let toolbar = Toolbar::new();
        assert_eq!(
            toolbar.style.layout.border_bottom_width,
            Some(nana_ui_core::HAIRLINE)
        );
        assert_eq!(toolbar.style.layout.border_top_width, Some(0.0));

        let status = StatusBar::new();
        assert_eq!(
            status.style.layout.border_top_width,
            Some(nana_ui_core::HAIRLINE)
        );
        assert_eq!(status.style.layout.border_bottom_width, Some(0.0));

        // Nested in a surface that already draws them, a bar takes no chrome.
        let bare = Toolbar::new().chrome(false);
        assert_eq!(bare.style.background, None);
        assert_eq!(bare.style.layout.border_width, None);
    }
}
