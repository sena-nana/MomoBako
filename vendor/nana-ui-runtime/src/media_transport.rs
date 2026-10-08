//! Media transport chrome with host-owned scene extras.
//!
//! Built-in controls are play, progress (seek or live meter), volume, settings
//! and fullscreen. Applications append scene-specific widgets into named slots.
//! An empty or fully hidden `secondary` slot collapses the bar to a single row.
//!
//! [`MediaTransportDensity`] picks the regular two-level chrome, a compact
//! single row, or a stacked variant with the progress on its own line; [`MediaTransportPlacement`] floats the bar over its stage or
//! lets it take part in the parent's layout (a shell mini player strip, the
//! bottom bar of a second window). Both are plain fields: the next
//! [`AppContext::sync_media_transport_bar`] applies a change to assembled
//! chrome.
//!
//! Seeking commits on release: dragging the progress range only previews the
//! time readout, and [`MediaTransportEvent::Seek`] fires once per committed
//! value (each keyboard step commits, as a native range does). A drag is
//! bracketed by [`MediaTransportEvent::SeekStarted`] and
//! [`MediaTransportEvent::SeekEnded`], so a host can hold its position
//! updates while the user drags. Volume follows the drag live.
//!
//! The settings menu's items are the application's
//! ([`MediaTransportBar::settings_content`], a view's `.settings(..)`).
//!
//! Idle hide is [`crate::OverlayVisibility`] held on this control and driven by
//! [`crate::AppContext::sync_overlay_visibility`]; an inline bar that the host
//! never syncs that way stays visible. It is kept apart from the bar's own
//! `hidden` (a view's `.visible(..)`), which stays the application's: the bar
//! shows while neither hides it ([`MediaTransportBar::shown`]), so hiding it
//! while playback is unavailable and the idle hide do not undo each other.

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, ControlSize, FlexDirection, FlexWrap, Icon, JustifySpec, LayoutStyle, LengthSpec,
    PointerEventsSpec, PopoverPlacement, PositionSpec, SemanticColorRole, space,
};

use crate::component_registry::{RegisterableComponent, SemanticSpec};
use crate::view_components::{
    Activate, IconButton, RangeChanged, RangeDragging, RangeField, RangeInput, Stack, Text,
    project_common,
};
use crate::{
    AccessibilityRole, AccessibilityState, ActionMenu, AppContext, ComponentView, Divider, Entity,
    FrameworkError, InteractionState, MutationQueue, NodeKind, NodeStyle, OverlayVisibility,
    Popover, Progress, StableNodeId, UiWorld,
};

const BAR_MAX_WIDTH: f32 = 820.0;
const BAR_MARGIN: f32 = space::XL;
const BAR_Z_INDEX: i32 = 30;
const BACKPLATE_OPACITY: f32 = 0.88;
const VOLUME_POPOVER_WIDTH: f32 = crate::popover::POPOVER_WIDTH;

/// Built-in transport action. Scene extras keep their own events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaTransportEvent {
    PlayPause,
    /// A committed seek target in seconds: pointer release or a keyboard /
    /// accessibility step, never an in-flight drag.
    Seek(f64),
    /// The user began dragging the progress; the readout previews the drag.
    SeekStarted,
    /// The drag ended: after its `Seek` when it moved the position, or
    /// without one when it was cancelled or let go where it began.
    SeekEnded,
    /// Live volume in `0..=100`, including drag previews.
    Volume(f64),
    Fullscreen,
    /// The bar's settings menu or volume popover opened. An open menu holds
    /// the bar visible: sync the overlay
    /// ([`AppContext::sync_overlay_visibility`]) so the hold starts now.
    MenuOpened,
    /// That menu closed, however it closed (its trigger, Escape, a press
    /// elsewhere, the menu hidden). Sync the overlay again: the idle timer
    /// restarts from here, where nothing else would wake the host.
    MenuClosed,
}

/// How much chrome the bar spends on its controls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaTransportDensity {
    /// Controls row with the time readout above the progress range; the
    /// `secondary` slot opens a second row.
    #[default]
    Regular,
    /// One tight row with the readout beside the range. Settings and
    /// fullscreen are hidden unless shown explicitly; every slot still works.
    Compact,
    /// Compact controls with the readout and range on a full-width line above
    /// them, so a narrow surface (a mini player) keeps a usable seek track
    /// instead of squeezing it between the buttons.
    Stacked,
}

/// Where the bar sits relative to its parent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaTransportPlacement {
    /// Absolute along the parent's bottom edge, capped at `max_width`; only
    /// the chrome takes hits, so the stage underneath stays interactive.
    #[default]
    Overlay,
    /// In the parent's flow with the chrome's own height, filling the width
    /// the parent gives it. `max_width` does not apply.
    Inline,
}

/// Icons the host supplies for chrome that is not in the shell catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaTransportIcons {
    pub play: Icon,
    pub pause: Icon,
    pub volume: Icon,
    pub volume_muted: Icon,
    pub fullscreen: Icon,
    pub fullscreen_exit: Icon,
    pub settings: Icon,
}

impl Default for MediaTransportIcons {
    fn default() -> Self {
        Self {
            play: Icon::MonitorPlay,
            pause: Icon::Minimize,
            volume: Icon::Eye,
            volume_muted: Icon::Eye,
            fullscreen: Icon::Maximize,
            fullscreen_exit: Icon::Restore,
            settings: Icon::Settings,
        }
    }
}

/// Application nodes the bar places in its groups: [`MediaTransportBar::leading_content`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MediaTransportContent {
    leading: Option<StableNodeId>,
    trailing: Option<StableNodeId>,
    secondary: Option<StableNodeId>,
    settings: Option<StableNodeId>,
}

/// Child nodes [`AppContext::assemble_media_transport_bar`] owns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaTransportSlots {
    pub chrome: Option<StableNodeId>,
    pub row: Option<StableNodeId>,
    pub center: Option<StableNodeId>,
    pub play: Option<StableNodeId>,
    pub leading: Option<StableNodeId>,
    pub time: Option<StableNodeId>,
    pub seek: Option<StableNodeId>,
    pub live_progress: Option<StableNodeId>,
    pub trailing: Option<StableNodeId>,
    pub volume: Option<StableNodeId>,
    pub volume_menu: Option<StableNodeId>,
    /// Holds [`Self::settings`] so the menu can be hidden as a whole.
    pub settings_group: Option<StableNodeId>,
    pub settings: Option<StableNodeId>,
    pub fullscreen: Option<StableNodeId>,
    pub secondary: Option<StableNodeId>,
    pub secondary_row: Option<StableNodeId>,
}

/// Media transport bar (`nana.media-transport-bar`).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaTransportBar {
    pub playing: bool,
    pub live: bool,
    pub fullscreen: bool,
    pub muted: bool,
    pub disabled: bool,
    /// Whether the on-demand media can be sought right now (it may still be
    /// loading or have no known duration). When false the range stays in
    /// place, disabled; live media shows its meter regardless.
    pub seekable: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub max_width: f32,
    pub density: MediaTransportDensity,
    pub placement: MediaTransportPlacement,
    /// `None` shows the play button in every density; `Some(false)` hides
    /// it (a live room that is not ready to play).
    pub show_play: Option<bool>,
    /// `None` follows the density: shown when regular, hidden when compact.
    pub show_settings: Option<bool>,
    /// `None` follows the density: shown when regular, hidden when compact.
    pub show_fullscreen: Option<bool>,
    pub icons: MediaTransportIcons,
    pub play_label: Arc<str>,
    pub pause_label: Arc<str>,
    pub volume_label: Arc<str>,
    pub settings_label: Arc<str>,
    pub fullscreen_label: Arc<str>,
    pub fullscreen_exit_label: Arc<str>,
    pub style: NodeStyle,
    pub(crate) slots: MediaTransportSlots,
    pub(crate) content: MediaTransportContent,
    pub(crate) visibility: OverlayVisibility,
    pub(crate) menu_was_open: bool,
    /// Chrome layout the assembled children carry, so a playback tick skips
    /// the layout pass.
    pub(crate) applied: Option<ChromeLayout>,
}

impl MediaTransportBar {
    pub fn new() -> Self {
        Self {
            playing: false,
            live: false,
            fullscreen: false,
            muted: false,
            disabled: false,
            seekable: true,
            position: 0.0,
            duration: 0.0,
            volume: 100.0,
            max_width: BAR_MAX_WIDTH,
            density: MediaTransportDensity::Regular,
            placement: MediaTransportPlacement::Overlay,
            show_play: None,
            show_settings: None,
            show_fullscreen: None,
            icons: MediaTransportIcons::default(),
            play_label: Arc::from("播放"),
            pause_label: Arc::from("暂停"),
            volume_label: Arc::from("音量"),
            settings_label: Arc::from("播放设置"),
            fullscreen_label: Arc::from("全屏"),
            fullscreen_exit_label: Arc::from("退出全屏"),
            style: bar_style(),
            slots: MediaTransportSlots::default(),
            content: MediaTransportContent::default(),
            visibility: OverlayVisibility::default(),
            menu_was_open: false,
            applied: None,
        }
    }

    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    /// An application node the bar keeps in its leading group, after play.
    /// Hosts that build the bar themselves append to [`Self::leading`]
    /// instead; views use `El::leading`.
    pub fn leading_content(mut self, node: StableNodeId) -> Self {
        self.content.leading = Some(node);
        self
    }

    /// An application node the bar keeps in its trailing group.
    pub fn trailing_content(mut self, node: StableNodeId) -> Self {
        self.content.trailing = Some(node);
        self
    }

    /// An application node the bar keeps in its second row.
    pub fn secondary_content(mut self, node: StableNodeId) -> Self {
        self.content.secondary = Some(node);
        self
    }

    /// An application node the bar keeps in its settings menu: the items
    /// (theatre, a window of its own, stop) the menu shows when opened.
    pub fn settings_content(mut self, node: StableNodeId) -> Self {
        self.content.settings = Some(node);
        self
    }

    pub fn seekable(mut self, seekable: bool) -> Self {
        self.seekable = seekable;
        self
    }

    pub fn icons(mut self, icons: MediaTransportIcons) -> Self {
        self.icons = icons;
        self
    }

    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width.max(0.0);
        self
    }

    pub fn density(mut self, density: MediaTransportDensity) -> Self {
        self.density = density;
        self
    }

    pub fn placement(mut self, placement: MediaTransportPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Show or hide the play button; hidden, it takes no focus or press.
    pub fn show_play(mut self, show: bool) -> Self {
        self.show_play = Some(show);
        self
    }

    pub fn show_settings(mut self, show: bool) -> Self {
        self.show_settings = Some(show);
        self
    }

    pub fn show_fullscreen(mut self, show: bool) -> Self {
        self.show_fullscreen = Some(show);
        self
    }

    pub fn slots(&self) -> &MediaTransportSlots {
        &self.slots
    }

    /// Whether the bar shows: the application has not hidden it (its style's
    /// `hidden`, a view's `.visible(..)`) and the idle policy has not either.
    pub fn shown(&self) -> bool {
        !self.style.layout.hidden && self.visibility.visible()
    }

    pub fn leading(&self) -> Option<Entity<Stack>> {
        self.slots.leading.map(Entity::from_stable_id)
    }

    pub fn trailing(&self) -> Option<Entity<Stack>> {
        self.slots.trailing.map(Entity::from_stable_id)
    }

    pub fn secondary(&self) -> Option<Entity<Stack>> {
        self.slots.secondary.map(Entity::from_stable_id)
    }

    pub fn settings(&self) -> Option<Entity<ActionMenu>> {
        self.slots.settings.map(Entity::from_stable_id)
    }

    pub fn play(&self) -> Option<Entity<IconButton>> {
        self.slots.play.map(Entity::from_stable_id)
    }

    pub fn time(&self) -> Option<Entity<Text>> {
        self.slots.time.map(Entity::from_stable_id)
    }

    pub fn seek(&self) -> Option<Entity<RangeField>> {
        self.slots.seek.map(Entity::from_stable_id)
    }

    pub fn live_progress(&self) -> Option<Entity<Progress>> {
        self.slots.live_progress.map(Entity::from_stable_id)
    }

    pub fn volume(&self) -> Option<Entity<RangeField>> {
        self.slots.volume.map(Entity::from_stable_id)
    }

    pub fn volume_menu(&self) -> Option<Entity<Popover>> {
        self.slots.volume_menu.map(Entity::from_stable_id)
    }

    pub fn fullscreen_button(&self) -> Option<Entity<IconButton>> {
        self.slots.fullscreen.map(Entity::from_stable_id)
    }

    fn chrome_layout(&self) -> ChromeLayout {
        let regular = self.density == MediaTransportDensity::Regular;
        ChromeLayout {
            density: self.density,
            max_width: match self.placement {
                MediaTransportPlacement::Overlay => Some(self.max_width.to_bits()),
                MediaTransportPlacement::Inline => None,
            },
            play: self.show_play.unwrap_or(true),
            settings: self.show_settings.unwrap_or(regular),
            fullscreen: self.show_fullscreen.unwrap_or(regular),
        }
    }
}

impl Default for MediaTransportBar {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout of the assembled chrome that depends on bar configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChromeLayout {
    density: MediaTransportDensity,
    /// `f32` bits; `None` when the placement does not cap the width.
    max_width: Option<u32>,
    play: bool,
    settings: bool,
    fullscreen: bool,
}

impl ChromeLayout {
    fn chrome(self, layout: &mut LayoutStyle) {
        layout.max_width = self
            .max_width
            .map(|bits| LengthSpec::Px(f32::from_bits(bits)));
    }

    fn row(self, layout: &mut LayoutStyle) {
        let (gap, inline, top, bottom) = match self.density {
            MediaTransportDensity::Regular => (space::XL, space::XL, space::XL, space::XS),
            MediaTransportDensity::Compact | MediaTransportDensity::Stacked => {
                (space::MD, space::MD, space::SM, space::SM)
            }
        };
        // Stacked: the center takes the first line (see `center`); the button
        // groups wrap onto the second, pushed to its two ends.
        let stacked = self.density == MediaTransportDensity::Stacked;
        layout.flex_wrap = if stacked {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        };
        layout.justify_content = if stacked {
            JustifySpec::SpaceBetween
        } else {
            JustifySpec::Start
        };
        layout.row_gap = stacked.then_some(LengthSpec::Px(space::XS));
        layout.gap = Some(LengthSpec::Px(gap));
        layout.padding_left = Some(LengthSpec::Px(inline));
        layout.padding_right = Some(LengthSpec::Px(inline));
        layout.padding_top = Some(LengthSpec::Px(top));
        layout.padding_bottom = Some(LengthSpec::Px(bottom));
    }

    fn center(self, layout: &mut LayoutStyle) {
        let (direction, gap, align) = match self.density {
            MediaTransportDensity::Regular => {
                (FlexDirection::Column, space::XXS, AlignSpec::Stretch)
            }
            MediaTransportDensity::Compact | MediaTransportDensity::Stacked => {
                (FlexDirection::Row, space::MD, AlignSpec::Center)
            }
        };
        let stacked = self.density == MediaTransportDensity::Stacked;
        layout.order = if stacked { -1 } else { 0 };
        layout.flex_basis = stacked.then_some(LengthSpec::Percent(100.0));
        layout.direction = Some(direction);
        layout.gap = Some(LengthSpec::Px(gap));
        layout.align_items = align;
    }
}

/// Placement-independent defaults; [`MediaTransportBar::project`] adds the
/// placement.
fn bar_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.direction = Some(FlexDirection::Row);
    layout.width = Some(LengthSpec::Fill);
    style
}

impl ComponentView for MediaTransportBar {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        assembler: Some(crate::AppContext::assemble_media_transport_bar),
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
            tag: "media-transport-bar".into(),
        }
    }

    fn reconcile(&mut self, mut next: Self) {
        next.slots = self.slots.clone();
        next.visibility = self.visibility.clone();
        next.menu_was_open = self.menu_was_open;
        next.applied = self.applied;
        *self = next;
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        // Hidden by the application or by the idle policy: two owners, one
        // box, so neither write undoes the other.
        layout.hidden = !self.shown();
        layout.pointer_events = Some(PointerEventsSpec::None);
        layout.justify_content = JustifySpec::Center;
        layout.width = Some(LengthSpec::Fill);
        match self.placement {
            MediaTransportPlacement::Overlay => {
                layout.position = PositionSpec::Absolute;
                layout.offset_left.get_or_insert(LengthSpec::Px(0.0));
                layout.offset_right.get_or_insert(LengthSpec::Px(0.0));
                layout
                    .offset_bottom
                    .get_or_insert(LengthSpec::Px(BAR_MARGIN));
                layout.z_index.get_or_insert(BAR_Z_INDEX);
                layout
                    .padding_left
                    .get_or_insert(LengthSpec::Px(BAR_MARGIN));
                layout
                    .padding_right
                    .get_or_insert(LengthSpec::Px(BAR_MARGIN));
                layout.align_items = AlignSpec::End;
            }
            MediaTransportPlacement::Inline => {
                layout.align_items = AlignSpec::Center;
            }
        }
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
            AccessibilityState {
                role: AccessibilityRole::Toolbar,
                label: Some(Arc::from("播放控制")),
                ..AccessibilityState::default()
            },
        );
    }
}

impl RegisterableComponent for MediaTransportBar {
    const TYPE_ID: &'static str = crate::component_descriptors::MEDIA_TRANSPORT_BAR.type_id;
    const TAGS: &'static [&'static str] = crate::component_descriptors::MEDIA_TRANSPORT_BAR.tags;
    /// Without this the binding keeps no typed state and `finish_semantic` is
    /// never installed, so a `<media-transport-bar>` from markup would project
    /// an empty toolbar: no play button, no seek range, no time readout, no
    /// volume or settings popover, no fullscreen button.
    const RETAIN_SEMANTIC_STATE: bool = true;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self {
        let keyword = |name: &str, value: &str| {
            spec.attr(name)
                .is_some_and(|raw| raw.trim().eq_ignore_ascii_case(value))
        };
        let mut bar = MediaTransportBar::new()
            .live(
                spec.attr("live")
                    .is_some_and(|value| matches!(value, "true" | "1" | "live")),
            )
            .max_width(
                spec.attr("max-width")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(BAR_MAX_WIDTH),
            );
        if keyword("density", "compact") {
            bar.density = MediaTransportDensity::Compact;
        } else if keyword("density", "stacked") {
            bar.density = MediaTransportDensity::Stacked;
        }
        if keyword("placement", "inline") {
            bar.placement = MediaTransportPlacement::Inline;
        }
        bar.show_play = shown_attr(spec, "show-play");
        bar.show_settings = shown_attr(spec, "show-settings");
        bar.show_fullscreen = shown_attr(spec, "show-fullscreen");
        bar
    }
    /// Markup only carries configuration. Playback state, labels, idle
    /// visibility and the assembled chrome belong to the live bar, so a
    /// rebind keeps them instead of assembling a second chrome.
    fn reconcile_semantic(spec: &SemanticSpec<'_>, previous: Option<&Self>) -> Self {
        let next = Self::from_semantic(spec);
        let Some(previous) = previous else {
            return next;
        };
        Self {
            live: next.live,
            max_width: next.max_width,
            density: next.density,
            placement: next.placement,
            show_play: next.show_play,
            show_settings: next.show_settings,
            show_fullscreen: next.show_fullscreen,
            ..previous.clone()
        }
    }
    fn finish_semantic(
        context: &mut AppContext,
        entity: Entity<Self>,
    ) -> Result<(), FrameworkError> {
        context.assemble_media_transport_bar(entity).map(|_| ())
    }
}

/// A boolean attribute: present without a value means shown.
fn shown_attr(spec: &SemanticSpec<'_>, name: &str) -> Option<bool> {
    let raw = spec.attr(name)?.trim();
    if raw.is_empty() {
        return Some(true);
    }
    crate::builtin_components::parse_tristate_attr(spec, &[name])
}

impl AppContext {
    /// Builds the chrome, slot stacks and built-in controls. Idempotent.
    pub fn assemble_media_transport_bar(
        &mut self,
        bar: Entity<MediaTransportBar>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(bar.stable_id())
            .ok_or(FrameworkError::MissingView(bar.stable_id()))?
            .document;
        // Hosts write the bar on every playback tick and each write lands
        // here, so only a first assembly pays for the snapshot.
        let created = self.read(bar, |bar| bar.slots.play.is_none())?;
        if created {
            let snapshot = self.read(bar, Clone::clone)?;
            let chrome = self.create_detached_component(
                document,
                Stack::column(space::MD)
                    .hittable()
                    .radius(nana_ui_core::RadiusTier::Md)
                    .with_layout(|layout| {
                        layout.position = PositionSpec::Relative;
                        layout.width = Some(LengthSpec::Fill);
                        layout.min_width = Some(LengthSpec::Px(0.0));
                        layout.flex_grow = Some(1.0);
                        layout.flex_shrink = Some(1.0);
                        layout.pointer_events = Some(PointerEventsSpec::Auto);
                    }),
            )?;
            self.append_child(bar, chrome)?;
            let backplate = self.create_detached_component(
                document,
                Stack::column(0.0)
                    .surface(SemanticColorRole::Surface)
                    .radius(nana_ui_core::RadiusTier::Md)
                    .with_layout(|layout| {
                        layout.position = PositionSpec::Absolute;
                        layout.offset_left = Some(LengthSpec::Px(0.0));
                        layout.offset_right = Some(LengthSpec::Px(0.0));
                        layout.offset_top = Some(LengthSpec::Px(0.0));
                        layout.offset_bottom = Some(LengthSpec::Px(0.0));
                        layout.opacity = Some(BACKPLATE_OPACITY);
                        layout.pointer_events = Some(PointerEventsSpec::None);
                        layout.z_index = Some(0);
                    }),
            )?;
            self.append_child(chrome, backplate)?;
            let row = self.create_detached_component(
                document,
                Stack::row(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Fill);
                    layout.min_width = Some(LengthSpec::Px(0.0));
                    layout.align_items = AlignSpec::Center;
                    layout.z_index = Some(1);
                }),
            )?;
            self.append_child(chrome, row)?;
            let left = self.create_detached_component(
                document,
                Stack::row(space::XS).with_layout(|layout| {
                    layout.align_items = AlignSpec::Center;
                    layout.flex_shrink = Some(0.0);
                }),
            )?;
            self.append_child(row, left)?;
            let play = self.create_detached_component(
                document,
                chrome_icon(snapshot.icons.play, snapshot.play_label.as_ref()),
            )?;
            self.append_child(left, play)?;
            let leading = self.create_detached_component(
                document,
                Stack::row(space::XS).with_layout(|layout| {
                    layout.align_items = AlignSpec::Center;
                    layout.flex_shrink = Some(0.0);
                }),
            )?;
            self.append_child(left, leading)?;
            let center = self.create_detached_component(
                document,
                Stack::column(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Fill);
                    layout.min_width = Some(LengthSpec::Px(0.0));
                    layout.flex_grow = Some(1.0);
                    layout.flex_shrink = Some(1.0);
                }),
            )?;
            self.append_child(row, center)?;
            let mut time = Text::new("")
                .font_size(12.0)
                .color(SemanticColorRole::Muted);
            Arc::make_mut(&mut time.style.layout).flex_shrink = Some(0.0);
            let time = self.create_detached_component(document, time)?;
            self.append_child(center, time)?;
            // The readout beside it already says what the track is; the name
            // stays for assistive technology only.
            let mut seek = RangeField::new(0.0, 0.0, 1.0, 1.0)
                .show_value(false)
                .show_label(false)
                .size(ControlSize::Small)
                .label("进度");
            {
                let layout = Arc::make_mut(&mut seek.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
            }
            let seek = self.create_detached_component(document, seek)?;
            self.append_child(center, seek)?;
            let mut live_progress = Progress::new(1.0, 1.0);
            {
                let layout = Arc::make_mut(&mut live_progress.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
            }
            let live_progress = self.create_detached_component(document, live_progress)?;
            self.append_child(center, live_progress)?;
            let right = self.create_detached_component(
                document,
                Stack::row(space::XS).with_layout(|layout| {
                    layout.align_items = AlignSpec::Center;
                    layout.flex_shrink = Some(0.0);
                }),
            )?;
            self.append_child(row, right)?;
            let trailing = self.create_detached_component(
                document,
                Stack::row(space::XS).with_layout(|layout| {
                    layout.align_items = AlignSpec::Center;
                    layout.flex_shrink = Some(0.0);
                }),
            )?;
            self.append_child(right, trailing)?;
            let volume_menu = self.create_detached_component(
                document,
                Popover::new()
                    .trigger_icon(snapshot.icons.volume, snapshot.volume_label.as_ref())
                    .placement(PopoverPlacement::Top)
                    .width(VOLUME_POPOVER_WIDTH),
            )?;
            self.append_child(right, volume_menu)?;
            let volume = self.create_detached_component(
                document,
                RangeField::new(100.0, 0.0, 100.0, 1.0)
                    .label("音量")
                    .show_value(false)
                    .size(ControlSize::Small),
            )?;
            self.append_child(volume_menu, volume)?;
            let settings_group = self.create_detached_component(
                document,
                Stack::row(0.0).with_layout(|layout| {
                    layout.align_items = AlignSpec::Center;
                    layout.flex_shrink = Some(0.0);
                }),
            )?;
            self.append_child(right, settings_group)?;
            let settings = self.create_detached_component(
                document,
                ActionMenu::new()
                    .trigger_icon(snapshot.icons.settings, snapshot.settings_label.as_ref())
                    .placement(PopoverPlacement::Top),
            )?;
            self.append_child(settings_group, settings)?;
            let fullscreen = self.create_detached_component(
                document,
                chrome_icon(
                    snapshot.icons.fullscreen,
                    snapshot.fullscreen_label.as_ref(),
                ),
            )?;
            self.append_child(right, fullscreen)?;
            let secondary_row = self.create_detached_component(
                document,
                Stack::column(space::XS).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Fill);
                    layout.min_width = Some(LengthSpec::Px(0.0));
                    layout.padding_left = Some(LengthSpec::Px(space::XS));
                    layout.padding_right = Some(LengthSpec::Px(space::XS));
                    layout.padding_bottom = Some(LengthSpec::Px(space::XS));
                    layout.z_index = Some(1);
                    layout.hidden = true;
                }),
            )?;
            self.append_child(chrome, secondary_row)?;
            let divider = self.create_detached_component(document, Divider::horizontal())?;
            self.append_child(secondary_row, divider)?;
            let secondary = self.create_detached_component(
                document,
                Stack::row(space::XL).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Fill);
                    layout.min_width = Some(LengthSpec::Px(0.0));
                    layout.align_items = AlignSpec::Center;
                    layout.padding = Some(LengthSpec::Px(space::XS));
                }),
            )?;
            self.append_child(secondary_row, secondary)?;

            self.observe(play, bar, |_, _: &Activate, cx| {
                cx.emit(MediaTransportEvent::PlayPause);
            })?;
            // Seek and volume write their target into the bar before the host
            // hears of it and reassemble, so the ranges hold the new value
            // instead of snapping back to the stale host value until the host
            // catches up. The host's next write stays authoritative.
            self.observe(seek, bar, |bar, event: &RangeChanged, cx| {
                bar.position = event.value;
                cx.reassemble();
                cx.emit(MediaTransportEvent::Seek(event.value));
            })?;
            // The readout follows a drag even while a paused host sends no
            // ticks.
            self.observe(seek, bar, |_, _: &RangeInput, cx| cx.reassemble())?;
            self.observe(seek, bar, |_, event: &RangeDragging, cx| {
                cx.emit(if event.dragging {
                    MediaTransportEvent::SeekStarted
                } else {
                    MediaTransportEvent::SeekEnded
                });
            })?;
            self.observe(volume, bar, |bar, event: &RangeInput, cx| {
                bar.volume = event.value;
                cx.reassemble();
                cx.emit(MediaTransportEvent::Volume(event.value));
            })?;
            self.observe(fullscreen, bar, |_, _: &Activate, cx| {
                cx.emit(MediaTransportEvent::Fullscreen);
            })?;
            self.observe(volume_menu, bar, report_menu)?;
            self.observe(settings, bar, report_menu)?;

            self.update_component(bar, |bar, _| {
                bar.slots = MediaTransportSlots {
                    chrome: Some(chrome.stable_id()),
                    row: Some(row.stable_id()),
                    center: Some(center.stable_id()),
                    play: Some(play.stable_id()),
                    leading: Some(leading.stable_id()),
                    time: Some(time.stable_id()),
                    seek: Some(seek.stable_id()),
                    live_progress: Some(live_progress.stable_id()),
                    trailing: Some(trailing.stable_id()),
                    volume: Some(volume.stable_id()),
                    volume_menu: Some(volume_menu.stable_id()),
                    settings_group: Some(settings_group.stable_id()),
                    settings: Some(settings.stable_id()),
                    fullscreen: Some(fullscreen.stable_id()),
                    secondary: Some(secondary.stable_id()),
                    secondary_row: Some(secondary_row.stable_id()),
                };
                bar.applied = None;
            })?;
        }
        self.place_media_transport_content(bar)?;
        self.sync_media_transport_bar(bar)?;
        Ok(created)
    }

    /// Keep the bar's application content in its groups; a move only on the
    /// first assembly, so a playback tick finds each already in place.
    fn place_media_transport_content(
        &mut self,
        bar: Entity<MediaTransportBar>,
    ) -> Result<(), FrameworkError> {
        let placements = self.read(bar, |bar| {
            [
                (bar.slots.leading, bar.content.leading),
                (bar.slots.trailing, bar.content.trailing),
                (bar.slots.secondary, bar.content.secondary),
                (bar.slots.settings, bar.content.settings),
            ]
        })?;
        let mut mutations = MutationQueue::new();
        for (group, node) in placements {
            if let (Some(group), Some(node)) = (group, node)
                && self
                    .world()
                    .node(node)
                    .is_some_and(|placed| placed.parent != Some(group))
            {
                mutations.insert(group, node, None);
            }
        }
        if !mutations.is_empty() {
            self.commit_mutations(mutations)?;
        }
        Ok(())
    }

    /// Refreshes built-in chrome and collapses an empty second row, writing
    /// only values that changed. `update_component` on the bar already runs
    /// it, so a playback tick needs no explicit call.
    pub fn sync_media_transport_bar(
        &mut self,
        bar: Entity<MediaTransportBar>,
    ) -> Result<(), FrameworkError> {
        let snapshot = self.read(bar, Clone::clone)?;
        let slots = &snapshot.slots;
        let chrome = snapshot.chrome_layout();
        if snapshot.applied != Some(chrome) {
            self.apply_chrome_layout(slots, chrome)?;
            self.update_component(bar, |bar, _| bar.applied = Some(chrome))?;
        }
        if let Some(play) = slots.play {
            let (icon, label) = if snapshot.playing {
                (snapshot.icons.pause, &snapshot.pause_label)
            } else {
                (snapshot.icons.play, &snapshot.play_label)
            };
            sync_icon_button(self, play, icon, label, Some(snapshot.disabled))?;
        }
        let (position, duration) = media_time(&snapshot);
        let seek = slots.seek.map(Entity::<RangeField>::from_stable_id);
        // A drag previews its value until release commits it through Seek;
        // the readout follows the preview instead of the host position.
        let scrub = seek.and_then(|seek| {
            self.read(seek, |range| range.dragging.map(|_| range.value))
                .ok()
                .flatten()
        });
        if let Some(time) = slots.time {
            let time = Entity::<Text>::from_stable_id(time);
            let shown = scrub.map_or(position, |value| value.min(duration));
            // Compared without formatting: the shown second changes about
            // once per second of the ticks that reach this line.
            let current = self.read(time, |text| {
                reads_time(&text.value, shown, duration)
                    && text.style.layout.hidden == snapshot.live
            })?;
            if !current {
                self.update_component(time, |text, _| {
                    text.value = time_readout(shown, duration);
                    Arc::make_mut(&mut text.style.layout).hidden = snapshot.live;
                })?;
            }
        }
        if let Some(seek) = seek {
            let disabled = snapshot.disabled || snapshot.live || !snapshot.seekable;
            let maximum = duration.max(1.0);
            let stale = self.read(seek, |range| {
                range.style.layout.hidden != snapshot.live
                    || range.disabled != disabled
                    || range.minimum != 0.0
                    || range.maximum != maximum
                    || (scrub.is_none() && range.value != position)
            })?;
            if stale {
                self.update_component(seek, |range, _| {
                    Arc::make_mut(&mut range.style.layout).hidden = snapshot.live;
                    range.disabled = disabled;
                    range.minimum = 0.0;
                    range.maximum = maximum;
                    if scrub.is_none() {
                        range.value = position;
                    }
                })?;
            }
        }
        if let Some(live_progress) = slots.live_progress {
            let live_progress = Entity::<Progress>::from_stable_id(live_progress);
            if self.read(live_progress, |progress| {
                progress.style.layout.hidden == snapshot.live
            })? {
                self.update_component(live_progress, |progress, _| {
                    Arc::make_mut(&mut progress.style.layout).hidden = !snapshot.live;
                })?;
            }
        }
        if let Some(volume) = slots.volume {
            let volume = Entity::<RangeField>::from_stable_id(volume);
            let value = if snapshot.volume.is_finite() {
                snapshot.volume.clamp(0.0, 100.0)
            } else {
                0.0
            };
            let stale = self.read(volume, |range| {
                range.disabled != snapshot.disabled
                    || (range.dragging.is_none() && range.value != value)
            })?;
            if stale {
                self.update_component(volume, |range, _| {
                    range.disabled = snapshot.disabled;
                    if range.dragging.is_none() {
                        range.value = value;
                    }
                })?;
            }
        }
        if let Some(volume_menu) = slots.volume_menu {
            let volume_menu = Entity::<Popover>::from_stable_id(volume_menu);
            let muted = snapshot.muted || snapshot.volume <= 0.0;
            let icon = if muted {
                snapshot.icons.volume_muted
            } else {
                snapshot.icons.volume
            };
            if self.read(volume_menu, |menu| {
                menu.trigger_icon != Some(icon) || menu.trigger != snapshot.volume_label
            })? {
                self.update_component(volume_menu, |menu, _| {
                    menu.trigger_icon = Some(icon);
                    menu.trigger = Arc::clone(&snapshot.volume_label);
                })?;
            }
        }
        if let Some(fullscreen) = slots.fullscreen {
            let (icon, label) = if snapshot.fullscreen {
                (
                    snapshot.icons.fullscreen_exit,
                    &snapshot.fullscreen_exit_label,
                )
            } else {
                (snapshot.icons.fullscreen, &snapshot.fullscreen_label)
            };
            sync_icon_button(self, fullscreen, icon, label, None)?;
        }
        if let (Some(secondary), Some(secondary_row)) = (slots.secondary, slots.secondary_row) {
            let hidden = !self.slot_has_visible_child(secondary);
            if self
                .world()
                .node_style(secondary_row)
                .is_some_and(|style| style.layout.hidden != hidden)
            {
                self.update_component(Entity::<Stack>::from_stable_id(secondary_row), |row, _| {
                    *row = row.clone().with_layout(|layout| layout.hidden = hidden);
                })?;
            }
        }
        Ok(())
    }

    /// Writes the configuration-dependent layout onto assembled chrome.
    fn apply_chrome_layout(
        &mut self,
        slots: &MediaTransportSlots,
        chrome: ChromeLayout,
    ) -> Result<(), FrameworkError> {
        let stacks: [(Option<StableNodeId>, fn(ChromeLayout, &mut LayoutStyle)); 3] = [
            (slots.chrome, ChromeLayout::chrome),
            (slots.row, ChromeLayout::row),
            (slots.center, ChromeLayout::center),
        ];
        for (id, apply) in stacks {
            if let Some(id) = id {
                self.update_component(Entity::<Stack>::from_stable_id(id), |stack, _| {
                    *stack = stack.clone().with_layout(|layout| apply(chrome, layout));
                })?;
            }
        }
        if let Some(group) = slots.settings_group {
            self.update_component(Entity::<Stack>::from_stable_id(group), |stack, _| {
                *stack = stack
                    .clone()
                    .with_layout(|layout| layout.hidden = !chrome.settings);
            })?;
        }
        for (button, shown) in [
            (slots.play, chrome.play),
            (slots.fullscreen, chrome.fullscreen),
        ] {
            if let Some(button) = button {
                self.update_component(
                    Entity::<IconButton>::from_stable_id(button),
                    |button, _| {
                        Arc::make_mut(&mut button.style.layout).hidden = !shown;
                    },
                )?;
            }
        }
        if !chrome.settings
            && let Some(settings) = slots.settings
        {
            // A hidden menu must not stay open where nobody can reach it. It
            // closes as a press elsewhere would, so the bar reports it.
            let settings = Entity::<ActionMenu>::from_stable_id(settings);
            if self.read(settings, |menu| menu.popover.open)? {
                self.toggle_action_menu(settings)?;
            }
        }
        // Nor may focus stay on a hidden control: it would keep the overlay
        // locked visible and let the keyboard activate what is not shown.
        let hidden = [
            (!chrome.play).then_some(slots.play).flatten(),
            (!chrome.settings).then_some(slots.settings_group).flatten(),
            (!chrome.fullscreen).then_some(slots.fullscreen).flatten(),
        ];
        if let Some(document) = slots
            .chrome
            .and_then(|id| self.world().node(id))
            .map(|node| node.document)
            && let Some(focused) = self.world().focused(document)
            && hidden
                .into_iter()
                .flatten()
                .any(|root| self.world().is_descendant_or_self(focused, root))
        {
            self.clear_focus(document)?;
        }
        Ok(())
    }

    fn slot_has_visible_child(&self, slot: StableNodeId) -> bool {
        self.world()
            .node(slot)
            .map(|node| {
                node.children.iter().any(|child| {
                    self.world()
                        .node_style(*child)
                        .is_some_and(|style| !style.layout.hidden)
                })
            })
            .unwrap_or(false)
    }
}

/// The bar's own menus opening and closing, as the bar's events.
fn report_menu(
    _: &mut MediaTransportBar,
    event: &crate::PopoverToggled,
    cx: &mut crate::ViewContext<'_, MediaTransportBar>,
) {
    cx.emit(if event.open {
        MediaTransportEvent::MenuOpened
    } else {
        MediaTransportEvent::MenuClosed
    });
}

/// Writes an icon button's glyph, label (and tooltip), and optionally its
/// disabled flag, only when one of them changed.
fn sync_icon_button(
    cx: &mut AppContext,
    id: StableNodeId,
    icon: Icon,
    label: &Arc<str>,
    disabled: Option<bool>,
) -> Result<(), FrameworkError> {
    let button = Entity::<IconButton>::from_stable_id(id);
    let stale = cx.read(button, |button| {
        button.icon != icon
            || button.label != *label
            || disabled.is_some_and(|disabled| button.disabled != disabled)
    })?;
    if !stale {
        return Ok(());
    }
    cx.update_component(button, |button, _| {
        button.icon = icon;
        if button.label != *label {
            button.label = Arc::clone(label);
            button.tooltip = Some(crate::IconButtonTooltip {
                label: Arc::clone(label),
                config: Default::default(),
            });
        }
        if let Some(disabled) = disabled {
            button.disabled = disabled;
        }
    })
}

fn chrome_icon(icon: Icon, label: &str) -> IconButton {
    IconButton::new(icon, label)
        .size(ControlSize::Small)
        .with_tooltip(label)
}

/// `(position, duration)` shared by the readout and the seek range. Non-finite
/// or negative seconds become zero; a position past the end clamps to it.
fn media_time(bar: &MediaTransportBar) -> (f64, f64) {
    let seconds = |value: f64| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    let duration = seconds(bar.duration);
    (seconds(bar.position).min(duration), duration)
}

fn time_readout(position: f64, duration: f64) -> String {
    format!("{} / {}", Clock(position), Clock(duration))
}

/// Whether `text` already reads [`time_readout`], checked without allocating.
fn reads_time(text: &str, position: f64, duration: f64) -> bool {
    struct Prefix<'a>(Option<&'a str>);
    impl std::fmt::Write for Prefix<'_> {
        fn write_str(&mut self, piece: &str) -> std::fmt::Result {
            self.0 = self.0.and_then(|rest| rest.strip_prefix(piece));
            Ok(())
        }
    }
    let mut rest = Prefix(Some(text));
    let _ = std::fmt::write(
        &mut rest,
        format_args!("{} / {}", Clock(position), Clock(duration)),
    );
    rest.0 == Some("")
}

/// Whole seconds as `m:ss`, or `h:mm:ss` from one hour on. Non-finite or
/// negative input reads as `0:00`.
pub fn media_clock(seconds: f64) -> String {
    Clock(seconds).to_string()
}

struct Clock(f64);

impl std::fmt::Display for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total = if self.0.is_finite() {
            self.0.max(0.0) as u64
        } else {
            0
        };
        let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
        if hours > 0 {
            write!(f, "{hours}:{minutes:02}:{seconds:02}")
        } else {
            write!(f, "{minutes}:{seconds:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use nana_ui_core::UI_METRICS;
    use std::sync::{Arc as StdArc, Mutex};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    /// Markup binds through the registry, which only installs `finish` for a
    /// type that retains its semantic state. Without that the bar projects as
    /// an empty toolbar: no play button, no seek range, no time readout.
    #[test]
    fn a_bar_bound_from_markup_assembles_its_controls() {
        // Tag-based binding needs `BuiltinComponents::Full`.
        if crate::framework::typed_builtins_under_test() {
            return;
        }
        let mut cx = AppContext::new();
        let document = document();
        let id = StableNodeId::new(42).unwrap();
        let mut queue = crate::MutationQueue::new();
        queue.create(
            id,
            document,
            crate::NodeKind::Element {
                tag: "media-transport-bar".into(),
            },
        );
        cx.commit_mutations(queue).unwrap();

        let type_id = cx
            .resolve_component_tag("media-transport-bar")
            .unwrap()
            .clone();
        let layout = Arc::new(nana_ui_core::LayoutStyle::default());
        let spec = crate::SemanticSpec::from_parts(&type_id, &layout);
        let mut mutations = crate::MutationQueue::new();
        let binding = cx
            .prepare_semantic_binding(id, &spec, &mut mutations)
            .unwrap();
        cx.commit_mutations(mutations).unwrap();
        cx.finish_semantic_binding(binding).unwrap();

        let bar = Entity::<MediaTransportBar>::from_stable_id(id);
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        assert!(
            slots.play.is_some(),
            "a bar from markup has no play button: {slots:?}"
        );
        assert!(slots.seek.is_some(), "and no seek range");
        assert!(slots.time.is_some(), "and no time readout");
    }

    #[test]
    fn empty_secondary_stays_a_single_row_until_a_child_is_appended() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        assert!(cx.assemble_media_transport_bar(bar).unwrap());
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        let row = slots.secondary_row.unwrap();
        assert!(
            cx.world().node_style(row).unwrap().layout.hidden,
            "no extras means one row"
        );

        let extra = cx
            .create_detached_component(document(), Stack::row(0.0))
            .unwrap();
        cx.append_child(
            Entity::<Stack>::from_stable_id(slots.secondary.unwrap()),
            extra,
        )
        .unwrap();
        cx.sync_media_transport_bar(bar).unwrap();
        assert!(
            !cx.world().node_style(row).unwrap().layout.hidden,
            "a visible child opens the second row"
        );

        cx.update_component(extra, |stack, _| {
            *stack = stack.clone().with_layout(|layout| layout.hidden = true);
        })
        .unwrap();
        cx.sync_media_transport_bar(bar).unwrap();
        assert!(
            cx.world().node_style(row).unwrap().layout.hidden,
            "hiding every extra collapses the bar"
        );
    }

    #[test]
    fn live_hides_seek_and_shows_a_meter() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new().live(true))
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        assert!(
            cx.world()
                .node_style(slots.seek.unwrap())
                .unwrap()
                .layout
                .hidden
        );
        assert!(
            cx.world()
                .node_style(slots.time.unwrap())
                .unwrap()
                .layout
                .hidden
        );
        assert!(
            !cx.world()
                .node_style(slots.live_progress.unwrap())
                .unwrap()
                .layout
                .hidden
        );
    }

    #[test]
    fn transport_events_are_the_same_for_both_densities() {
        for density in [
            MediaTransportDensity::Regular,
            MediaTransportDensity::Compact,
        ] {
            let mut cx = AppContext::new();
            let bar = cx
                .create_component(document(), MediaTransportBar::new().density(density))
                .unwrap();
            cx.assemble_media_transport_bar(bar).unwrap();
            cx.update_component(bar, |bar, _| bar.duration = 120.0)
                .unwrap();
            let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
            let seen = StdArc::new(Mutex::new(Vec::new()));
            let out = StdArc::clone(&seen);
            cx.on(bar, move |_, event: &MediaTransportEvent, _| {
                out.lock().unwrap().push(*event);
            })
            .unwrap();
            cx.activate_icon_button(Entity::from_stable_id(slots.play.unwrap()))
                .unwrap();
            cx.set_range_value(Entity::from_stable_id(slots.seek.unwrap()), 12.0)
                .unwrap();
            let volume = Entity::<RangeField>::from_stable_id(slots.volume.unwrap());
            cx.adjust_range(volume, crate::RangeAdjustment::PageDecrement)
                .unwrap();
            let level = cx.read(volume, |range| range.value).unwrap();
            assert_eq!(
                *seen.lock().unwrap(),
                vec![
                    MediaTransportEvent::PlayPause,
                    MediaTransportEvent::Seek(12.0),
                    MediaTransportEvent::Volume(level),
                ],
                "{density:?}"
            );
            assert_eq!(
                cx.read(volume, |range| range.value).unwrap(),
                level,
                "{density:?}: the step holds until the host syncs its volume"
            );
        }
    }

    fn sync_time(
        cx: &mut AppContext,
        bar: Entity<MediaTransportBar>,
        position: f64,
        duration: f64,
    ) {
        cx.update_component(bar, |bar, _| {
            bar.position = position;
            bar.duration = duration;
        })
        .unwrap();
        cx.sync_media_transport_bar(bar).unwrap();
    }

    fn readout(cx: &AppContext, slots: &MediaTransportSlots) -> String {
        cx.world().text(slots.time.unwrap()).unwrap().to_owned()
    }

    fn seek_value(cx: &AppContext, slots: &MediaTransportSlots) -> f64 {
        cx.read(
            Entity::<RangeField>::from_stable_id(slots.seek.unwrap()),
            |range| range.value,
        )
        .unwrap()
    }

    #[test]
    fn time_readout_follows_position_and_duration() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();

        sync_time(&mut cx, bar, 12.0, 120.0);
        assert_eq!(readout(&cx, &slots), "0:12 / 2:00");
        sync_time(&mut cx, bar, 12.7, 120.0);
        assert_eq!(readout(&cx, &slots), "0:12 / 2:00");
        sync_time(&mut cx, bar, 13.0, 120.0);
        assert_eq!(readout(&cx, &slots), "0:13 / 2:00");

        sync_time(&mut cx, bar, 500.0, 120.0);
        assert_eq!(readout(&cx, &slots), "2:00 / 2:00");
        assert_eq!(seek_value(&cx, &slots), 120.0);

        let generation = cx.world().generation();
        cx.sync_media_transport_bar(bar).unwrap();
        assert_eq!(cx.world().generation(), generation);
    }

    #[test]
    fn leaving_live_shows_the_current_readout() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new().live(true))
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        let time = slots.time.unwrap();

        sync_time(&mut cx, bar, 30.0, 90.0);
        assert!(cx.world().node_style(time).unwrap().layout.hidden);

        cx.update_component(bar, |bar, _| bar.live = false).unwrap();
        cx.sync_media_transport_bar(bar).unwrap();
        assert!(!cx.world().node_style(time).unwrap().layout.hidden);
        assert_eq!(readout(&cx, &slots), "0:30 / 1:30");
    }

    #[test]
    fn readout_handles_hours_and_invalid_seconds() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();

        sync_time(&mut cx, bar, 3600.0, 3725.0);
        assert_eq!(readout(&cx, &slots), "1:00:00 / 1:02:05");

        sync_time(&mut cx, bar, f64::NAN, f64::INFINITY);
        assert_eq!(readout(&cx, &slots), "0:00 / 0:00");
        assert_eq!(seek_value(&cx, &slots), 0.0);

        sync_time(&mut cx, bar, -5.0, 60.0);
        assert_eq!(readout(&cx, &slots), "0:00 / 1:00");
        assert_eq!(seek_value(&cx, &slots), 0.0);
    }

    #[test]
    fn scrubbing_previews_the_dragged_position() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        let seek = Entity::<RangeField>::from_stable_id(slots.seek.unwrap());
        sync_time(&mut cx, bar, 12.0, 120.0);

        cx.update_component(seek, |range, _| {
            range.dragging = Some(crate::RangeDragState {
                pointer_id: 1,
                initial_value: 12.0,
            });
            range.value = 45.0;
        })
        .unwrap();
        sync_time(&mut cx, bar, 13.0, 120.0);
        assert_eq!(readout(&cx, &slots), "0:45 / 2:00");
        assert_eq!(seek_value(&cx, &slots), 45.0);

        cx.update_component(seek, |range, _| range.dragging = None)
            .unwrap();
        sync_time(&mut cx, bar, 45.0, 120.0);
        assert_eq!(readout(&cx, &slots), "0:45 / 2:00");
    }

    const STAGE_WIDTH: f32 = 1200.0;
    const STAGE_HEIGHT: f32 = 600.0;
    const SIBLING_HEIGHT: f32 = 100.0;

    struct Stage {
        cx: AppContext,
        sibling: Entity<Stack>,
        bar: Entity<MediaTransportBar>,
    }

    impl Stage {
        fn new(document: DocumentId, bar: MediaTransportBar) -> Self {
            let mut cx = AppContext::new();
            let root = cx
                .create_component(
                    document,
                    Stack::column(0.0).with_layout(|layout| {
                        layout.width = Some(LengthSpec::Px(STAGE_WIDTH));
                        layout.height = Some(LengthSpec::Px(STAGE_HEIGHT));
                    }),
                )
                .unwrap();
            let sibling = cx
                .create_detached_component(
                    document,
                    Stack::column(0.0).with_layout(|layout| {
                        layout.width = Some(LengthSpec::Fill);
                        layout.height = Some(LengthSpec::Px(SIBLING_HEIGHT));
                        layout.flex_shrink = Some(0.0);
                    }),
                )
                .unwrap();
            cx.append_child(root, sibling).unwrap();
            let bar = cx.create_detached_component(document, bar).unwrap();
            cx.append_child(root, bar).unwrap();
            cx.assemble_media_transport_bar(bar).unwrap();
            let mut stage = Self { cx, sibling, bar };
            stage.layout(document);
            stage
        }

        fn layout(&mut self, document: DocumentId) {
            let texts: Vec<_> = self.slots().time.into_iter().collect();
            self.cx
                .shape_text(&texts, &mut crate::MeasureTextShaper)
                .unwrap();
            self.cx
                .layout_document(
                    document,
                    crate::LayoutViewport::new(STAGE_WIDTH, STAGE_HEIGHT),
                )
                .unwrap();
        }

        fn slots(&self) -> MediaTransportSlots {
            self.cx.read(self.bar, |bar| bar.slots().clone()).unwrap()
        }

        fn frame(&self, id: StableNodeId) -> crate::LayoutBox {
            self.cx.world().layout_box(id).unwrap()
        }

        fn hidden(&self, id: StableNodeId) -> bool {
            self.cx.world().node_style(id).unwrap().layout.hidden
        }
    }

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 0.5,
            "{what}: {actual} != {expected}"
        );
    }

    #[test]
    fn overlay_floats_on_the_parent_bottom_edge_for_both_densities() {
        let mut heights = Vec::new();
        for density in [
            MediaTransportDensity::Regular,
            MediaTransportDensity::Compact,
        ] {
            let stage = Stage::new(document(), MediaTransportBar::new().density(density));
            let slots = stage.slots();
            let bar = stage.frame(stage.bar.stable_id());
            let chrome = stage.frame(slots.chrome.unwrap());
            assert_close(
                bar.y + bar.height,
                STAGE_HEIGHT - BAR_MARGIN,
                "overlay bottom edge",
            );
            assert_close(
                stage.frame(stage.sibling.stable_id()).y,
                0.0,
                "an overlay does not move siblings",
            );
            assert_close(chrome.width, BAR_MAX_WIDTH, "overlay keeps max_width");
            assert_close(
                chrome.x,
                (STAGE_WIDTH - BAR_MAX_WIDTH) / 2.0,
                "overlay centers the chrome",
            );
            heights.push(chrome.height);
        }
        let compact = heights[1];
        assert!(
            compact < heights[0],
            "compact is shorter than regular: {heights:?}"
        );
        assert_close(
            compact + BAR_MARGIN,
            UI_METRICS.small_control_height() + space::SM * 2.0 + BAR_MARGIN,
            "a compact overlay is one control row plus its margin",
        );
    }

    #[test]
    fn inline_takes_its_height_in_the_parent_flow_for_both_densities() {
        for density in [
            MediaTransportDensity::Regular,
            MediaTransportDensity::Compact,
        ] {
            let stage = Stage::new(
                document(),
                MediaTransportBar::new()
                    .density(density)
                    .placement(MediaTransportPlacement::Inline),
            );
            let slots = stage.slots();
            let bar = stage.frame(stage.bar.stable_id());
            let chrome = stage.frame(slots.chrome.unwrap());
            assert_close(bar.y, SIBLING_HEIGHT, "inline follows its sibling");
            assert!(chrome.height > 0.0);
            assert_close(bar.height, chrome.height, "inline is as tall as its chrome");
            assert_close(chrome.width, STAGE_WIDTH, "inline fills the parent width");
            let layout = &stage
                .cx
                .world()
                .node_style(stage.bar.stable_id())
                .unwrap()
                .layout;
            assert_ne!(layout.position, PositionSpec::Absolute);
            assert_eq!(layout.offset_bottom, None);
        }
    }

    #[test]
    fn compact_is_one_row_with_settings_and_fullscreen_hidden_by_default() {
        let stage = Stage::new(
            document(),
            MediaTransportBar::new().density(MediaTransportDensity::Compact),
        );
        let slots = stage.slots();
        let time = stage.frame(slots.time.unwrap());
        let seek = stage.frame(slots.seek.unwrap());
        assert!(
            time.x + time.width <= seek.x,
            "readout sits beside the range"
        );
        assert_close(
            time.y + time.height / 2.0,
            seek.y + seek.height / 2.0,
            "readout and range share one row",
        );
        assert!(stage.hidden(slots.settings_group.unwrap()));
        assert!(stage.hidden(slots.fullscreen.unwrap()));
        assert!(stage.hidden(slots.secondary_row.unwrap()));

        let regular = Stage::new(document(), MediaTransportBar::new());
        let slots = regular.slots();
        let time = regular.frame(slots.time.unwrap());
        assert!(
            time.y + time.height <= regular.frame(slots.seek.unwrap()).y,
            "regular keeps the readout above the range"
        );
        assert!(!regular.hidden(slots.settings_group.unwrap()));
        assert!(!regular.hidden(slots.fullscreen.unwrap()));
    }

    #[test]
    fn stacked_puts_the_progress_on_its_own_line_above_the_controls() {
        let stage = Stage::new(
            document(),
            MediaTransportBar::new().density(MediaTransportDensity::Stacked),
        );
        let slots = stage.slots();
        let seek = stage.frame(slots.seek.unwrap());
        let time = stage.frame(slots.time.unwrap());
        let play = stage.frame(slots.play.unwrap());
        let volume = stage.frame(slots.volume_menu.unwrap());
        let row = stage.frame(slots.row.unwrap());
        assert!(
            seek.y + seek.height <= play.y,
            "the range sits above the buttons: seek={seek:?} play={play:?}"
        );
        assert!(time.x + time.width <= seek.x, "readout beside the range");
        assert!(
            seek.width > row.width * 0.6,
            "the range gets most of the line: seek={seek:?} row={row:?}"
        );
        assert!(
            volume.x > row.x + row.width / 2.0,
            "the trailing group stays at the end of the button line"
        );
        assert!(stage.hidden(slots.settings_group.unwrap()));
        assert!(stage.hidden(slots.fullscreen.unwrap()));
    }

    #[test]
    fn a_stacked_inline_bar_is_as_tall_as_its_two_lines() {
        // 进度行靠 `order` 排到按钮前面;量高时也得按这个顺序折行,否则按文档
        // 顺序会折成三行,条下面空出一整行。
        let stage = Stage::new(
            document(),
            MediaTransportBar::new()
                .density(MediaTransportDensity::Stacked)
                .placement(MediaTransportPlacement::Inline),
        );
        let slots = stage.slots();
        let bar = stage.frame(stage.bar.stable_id());
        let play = stage.frame(slots.play.unwrap());
        assert_close(
            bar.y + bar.height,
            play.y + play.height + space::SM,
            "the bar ends one padding below the button line",
        );
    }

    #[test]
    fn an_unseekable_program_keeps_its_range_in_place_but_disabled() {
        let mut stage = Stage::new(document(), MediaTransportBar::new());
        let slots = stage.slots();
        let seek = Entity::<RangeField>::from_stable_id(slots.seek.unwrap());
        stage
            .cx
            .update_component(stage.bar, |bar, _| {
                bar.seekable = false;
                bar.duration = 120.0;
            })
            .unwrap();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        let (disabled, hidden) = stage
            .cx
            .read(seek, |range| (range.disabled, range.style.layout.hidden))
            .unwrap();
        assert!(disabled && !hidden);
        stage
            .cx
            .update_component(stage.bar, |bar, _| bar.seekable = true)
            .unwrap();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        assert!(!stage.cx.read(seek, |range| range.disabled).unwrap());
    }

    #[test]
    fn the_seek_range_is_named_for_assistive_technology_but_draws_no_label() {
        let stage = Stage::new(document(), MediaTransportBar::new());
        let seek = stage.slots().seek.unwrap();
        let world = stage.cx.world();
        assert!(matches!(
            world.standard_visual(seek),
            Some(crate::StandardVisual::Range { label: None, .. })
        ));
        let accessible = world
            .project_accessibility(stage.cx.world().node(seek).unwrap().document)
            .into_iter()
            .find(|node| node.id == seek)
            .and_then(|node| node.label);
        assert_eq!(accessible.as_deref(), Some("进度"));
    }

    #[test]
    fn compact_slots_and_explicit_controls_stay_available() {
        let mut stage = Stage::new(
            document(),
            MediaTransportBar::new()
                .density(MediaTransportDensity::Compact)
                .show_fullscreen(true),
        );
        let slots = stage.slots();
        assert!(!stage.hidden(slots.fullscreen.unwrap()));
        assert!(stage.hidden(slots.settings_group.unwrap()));

        let next = stage
            .cx
            .create_detached_component(document(), IconButton::new(Icon::MonitorPlay, "下一个"))
            .unwrap();
        let trailing = stage
            .cx
            .read(stage.bar, |bar| bar.trailing())
            .unwrap()
            .unwrap();
        stage.cx.append_child(trailing, next).unwrap();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        stage.layout(document());
        let next = stage.frame(next.stable_id());
        let row = stage.frame(slots.row.unwrap());
        assert!(next.width > 0.0 && next.y >= row.y && next.y + next.height <= row.y + row.height);
    }

    #[test]
    fn density_and_placement_changes_apply_on_sync() {
        let mut stage = Stage::new(document(), MediaTransportBar::new());
        let slots = stage.slots();
        let settings = cx_settings(&stage);
        assert!(stage.cx.toggle_action_menu(settings).unwrap());
        assert!(
            stage
                .cx
                .focus_node(document(), slots.fullscreen.unwrap())
                .unwrap()
        );
        let regular = stage.frame(slots.chrome.unwrap()).height;

        stage
            .cx
            .update_component(stage.bar, |bar, _| {
                bar.density = MediaTransportDensity::Compact;
                bar.placement = MediaTransportPlacement::Inline;
            })
            .unwrap();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        stage.layout(document());
        assert!(stage.frame(slots.chrome.unwrap()).height < regular);
        assert_close(
            stage.frame(stage.bar.stable_id()).y,
            SIBLING_HEIGHT,
            "now inline",
        );
        assert!(stage.hidden(slots.settings_group.unwrap()));
        assert!(
            !stage.cx.read(settings, |menu| menu.popover.open).unwrap(),
            "hiding the settings menu closes it"
        );
        assert_eq!(
            stage.cx.world().focused(document()),
            None,
            "hiding the focused fullscreen button releases focus"
        );

        stage
            .cx
            .update_component(stage.bar, |bar, _| {
                bar.density = MediaTransportDensity::Regular;
                bar.placement = MediaTransportPlacement::Overlay;
            })
            .unwrap();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        stage.layout(document());
        assert_close(
            stage.frame(slots.chrome.unwrap()).height,
            regular,
            "back to regular",
        );
        assert!(!stage.hidden(slots.settings_group.unwrap()));

        let generation = stage.cx.world().generation();
        stage.cx.sync_media_transport_bar(stage.bar).unwrap();
        assert_eq!(
            stage.cx.world().generation(),
            generation,
            "an unchanged sync writes nothing"
        );
    }

    fn cx_settings(stage: &Stage) -> Entity<ActionMenu> {
        stage
            .cx
            .read(stage.bar, |bar| bar.settings())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn a_seek_drag_previews_the_readout_and_seeks_once_on_release() {
        let mut stage = Stage::new(
            document(),
            MediaTransportBar::new().density(MediaTransportDensity::Compact),
        );
        let (bar, slots) = (stage.bar, stage.slots());
        sync_time(&mut stage.cx, bar, 10.0, 100.0);
        stage.layout(document());
        let seen = StdArc::new(Mutex::new(Vec::new()));
        let out = StdArc::clone(&seen);
        stage
            .cx
            .on(bar, move |_, event: &MediaTransportEvent, _| {
                out.lock().unwrap().push(*event);
            })
            .unwrap();
        let seek = slots.seek.unwrap();
        let Some(crate::ComponentGeometry::Range { track, .. }) =
            stage.cx.world().component_geometry(seek)
        else {
            panic!("seek range geometry");
        };
        let at = |fraction: f32| track.x + track.width * fraction;

        stage
            .cx
            .begin_range_drag(document(), 1, seek, at(0.3))
            .unwrap();
        stage.cx.update_range_drag(document(), 1, at(0.6)).unwrap();
        assert_eq!(
            readout(&stage.cx, &slots),
            "1:00 / 1:40",
            "a paused host sends no tick, yet the drag previews the readout"
        );
        sync_time(&mut stage.cx, bar, 11.0, 100.0);
        assert_eq!(readout(&stage.cx, &slots), "1:00 / 1:40");
        assert_eq!(seek_value(&stage.cx, &slots), 60.0);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![MediaTransportEvent::SeekStarted],
            "the drag began; no Seek while it is in flight"
        );

        stage.cx.end_range_drag(document(), 1, false).unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                MediaTransportEvent::SeekStarted,
                MediaTransportEvent::Seek(60.0),
                MediaTransportEvent::SeekEnded,
            ]
        );
        assert_eq!(
            seek_value(&stage.cx, &slots),
            60.0,
            "the committed target holds until the host reports a position"
        );

        seen.lock().unwrap().clear();
        stage
            .cx
            .begin_range_drag(document(), 2, seek, at(0.9))
            .unwrap();
        stage.cx.end_range_drag(document(), 2, true).unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                MediaTransportEvent::SeekStarted,
                MediaTransportEvent::SeekEnded
            ],
            "a cancelled drag never seeks, and still ends"
        );
    }

    #[test]
    fn two_documents_in_one_context_each_drive_their_own_chrome() {
        let mut cx = AppContext::new();
        let documents = [document(), DocumentId::new(2).unwrap()];
        let bars = documents.map(|document| {
            let bar = cx
                .create_component(
                    document,
                    MediaTransportBar::new().density(MediaTransportDensity::Compact),
                )
                .unwrap();
            assert!(cx.assemble_media_transport_bar(bar).unwrap());
            bar
        });
        let seen = StdArc::new(Mutex::new(Vec::new()));
        for (index, bar) in bars.into_iter().enumerate() {
            let out = StdArc::clone(&seen);
            cx.on(bar, move |_, event: &MediaTransportEvent, _| {
                out.lock().unwrap().push((index, *event));
            })
            .unwrap();
        }
        for (bar, document) in bars.into_iter().zip(documents) {
            let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();
            for id in [
                slots.play,
                slots.time,
                slots.seek,
                slots.volume_menu,
                slots.settings,
                slots.fullscreen,
                slots.leading,
                slots.trailing,
                slots.secondary,
            ] {
                let id = id.expect("every built-in control is assembled");
                assert_eq!(cx.world().node(id).unwrap().document, document);
            }
        }
        sync_time(&mut cx, bars[1], 30.0, 60.0);
        let second = cx.read(bars[1], |bar| bar.slots().clone()).unwrap();
        let first = cx.read(bars[0], |bar| bar.slots().clone()).unwrap();
        assert_eq!(readout(&cx, &second), "0:30 / 1:00");
        assert_eq!(readout(&cx, &first), "0:00 / 0:00");
        cx.activate_icon_button(Entity::from_stable_id(second.play.unwrap()))
            .unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(1, MediaTransportEvent::PlayPause)]
        );
    }

    #[test]
    fn a_compact_inline_bar_shares_a_row_with_host_content() {
        let mut cx = AppContext::new();
        let strip = cx
            .create_component(
                document(),
                Stack::row(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(STAGE_WIDTH));
                    layout.height = Some(LengthSpec::Px(112.0));
                    layout.align_items = AlignSpec::Center;
                }),
            )
            .unwrap();
        let cover = cx
            .create_detached_component(
                document(),
                Stack::row(0.0).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(120.0));
                    layout.height = Some(LengthSpec::Px(68.0));
                    layout.flex_shrink = Some(0.0);
                }),
            )
            .unwrap();
        cx.append_child(strip, cover).unwrap();
        let mut bar = MediaTransportBar::new()
            .density(MediaTransportDensity::Compact)
            .placement(MediaTransportPlacement::Inline);
        Arc::make_mut(&mut bar.style.layout).flex_grow = Some(1.0);
        let bar = cx.create_detached_component(document(), bar).unwrap();
        cx.append_child(strip, bar).unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        cx.layout_document(
            document(),
            crate::LayoutViewport::new(STAGE_WIDTH, STAGE_HEIGHT),
        )
        .unwrap();
        let bar = cx.world().layout_box(bar.stable_id()).unwrap();
        assert_close(bar.x, 120.0, "the bar starts after the host cover");
        assert_close(
            bar.x + bar.width,
            STAGE_WIDTH,
            "and takes the rest of the row",
        );
        assert!(bar.y > 0.0 && bar.y + bar.height < 112.0, "{bar:?}");
    }

    #[test]
    fn rebinding_markup_keeps_one_chrome_and_the_host_state() {
        // Tag-based binding needs `BuiltinComponents::Full`.
        if crate::framework::typed_builtins_under_test() {
            return;
        }
        let mut cx = AppContext::new();
        let id = StableNodeId::new(42).unwrap();
        let mut queue = crate::MutationQueue::new();
        queue.create(
            id,
            document(),
            crate::NodeKind::Element {
                tag: "media-transport-bar".into(),
            },
        );
        cx.commit_mutations(queue).unwrap();
        let type_id = cx
            .resolve_component_tag("media-transport-bar")
            .unwrap()
            .clone();
        let layout = Arc::new(nana_ui_core::LayoutStyle::default());
        let bind = |cx: &mut AppContext, attrs: &[(&str, &str)]| {
            let spec = crate::SemanticSpec {
                attrs,
                ..crate::SemanticSpec::from_parts(&type_id, &layout)
            };
            let mut mutations = crate::MutationQueue::new();
            let binding = cx
                .prepare_semantic_binding(id, &spec, &mut mutations)
                .unwrap();
            cx.commit_mutations(mutations).unwrap();
            cx.finish_semantic_binding(binding).unwrap();
        };
        bind(&mut cx, &[]);
        let bar = Entity::<MediaTransportBar>::from_stable_id(id);
        sync_time(&mut cx, bar, 30.0, 60.0);
        let slots = cx.read(bar, |bar| bar.slots().clone()).unwrap();

        bind(&mut cx, &[("density", "compact")]);
        assert_eq!(cx.world().node(id).unwrap().children.len(), 1, "one chrome");
        let rebound = cx.read(bar, Clone::clone).unwrap();
        assert_eq!(rebound.slots, slots);
        assert_eq!(rebound.density, MediaTransportDensity::Compact);
        assert_eq!(rebound.position, 30.0);
        assert_eq!(readout(&cx, &slots), "0:30 / 1:00");
        assert!(
            cx.world()
                .node_style(slots.fullscreen.unwrap())
                .unwrap()
                .layout
                .hidden,
            "the new density applies to the kept chrome"
        );
    }

    #[test]
    fn readout_comparison_matches_the_formatted_text() {
        assert!(reads_time("1:05 / 1:00:00", 65.2, 3600.0));
        assert!(!reads_time("1:05 / 1:00:0", 65.2, 3600.0));
        assert!(!reads_time("1:05 / 1:00:000", 65.2, 3600.0));
        assert!(!reads_time("", 0.0, 0.0));
        assert!(reads_time(&time_readout(f64::NAN, 59.9), f64::NAN, 59.9));
    }

    #[test]
    fn markup_attributes_pick_density_and_placement() {
        let type_id = crate::ComponentTypeId::new("nana.media-transport-bar").unwrap();
        let layout = Arc::new(nana_ui_core::LayoutStyle::default());
        let attrs = [
            ("density", "compact"),
            ("placement", "Inline"),
            ("show-fullscreen", ""),
            ("show-play", "false"),
        ];
        let spec = crate::SemanticSpec {
            attrs: &attrs,
            ..crate::SemanticSpec::from_parts(&type_id, &layout)
        };
        let bar = MediaTransportBar::from_semantic(&spec);
        assert_eq!(bar.density, MediaTransportDensity::Compact);
        assert_eq!(bar.placement, MediaTransportPlacement::Inline);
        assert_eq!(bar.show_fullscreen, Some(true));
        assert_eq!(bar.show_settings, None);
        assert_eq!(bar.show_play, Some(false));
    }

    #[test]
    fn media_clock_formats_minutes_and_hours() {
        assert_eq!(media_clock(0.0), "0:00");
        assert_eq!(media_clock(65.9), "1:05");
        assert_eq!(media_clock(3725.0), "1:02:05");
        assert_eq!(media_clock(-3.0), "0:00");
        assert_eq!(media_clock(f64::NAN), "0:00");
    }

    /// The application hides the bar while playback is unavailable
    /// (`.visible(..)`) and the idle policy hides it after a while: neither
    /// undoes the other. A binding re-run (a playback tick) keeps an idle
    /// bar hidden, reveal does not show a bar the application hid, and the
    /// bar shows again once both allow it.
    #[test]
    fn availability_and_idle_hide_the_bar_together() {
        use crate::view::{signal, widget};
        use std::time::{Duration, Instant};
        let mut cx = AppContext::new();
        let parent = cx.create_component(document(), Stack::column(0.0)).unwrap();
        let state = std::cell::Cell::new(None);
        let refs = std::cell::Cell::new(None);
        cx.mount_view(parent.stable_id(), || {
            let (available, position) = (signal(true), signal(0.0f64));
            let bar = crate::view::entity_ref::<MediaTransportBar>();
            state.set(Some((available, position)));
            refs.set(Some(bar));
            widget(MediaTransportBar::new())
                .entity_ref(bar)
                .visible(available)
                .bind(move |bar| bar.position = position.get())
        })
        .unwrap();
        let (available, position) = state.get().unwrap();
        let bar = refs.get().unwrap().get().unwrap();
        let hidden = |cx: &AppContext| {
            cx.world()
                .node_style(bar.stable_id())
                .unwrap()
                .layout
                .hidden
        };
        let now = Instant::now();
        cx.sync_overlay_visibility(bar, now, true).unwrap();
        cx.sync_overlay_visibility(bar, now + crate::OVERLAY_IDLE, true)
            .unwrap();
        assert!(hidden(&cx), "idle");

        position.set(12.0);
        cx.flush_reactive().unwrap();
        assert_eq!(cx.read(bar, |bar| bar.position).unwrap(), 12.0);
        assert!(hidden(&cx), "a playback tick keeps the idle bar hidden");

        available.set(false);
        cx.flush_reactive().unwrap();
        cx.reveal_overlay(bar, now + Duration::from_secs(5))
            .unwrap();
        assert!(
            hidden(&cx),
            "reveal does not show a bar the application hid"
        );
        assert!(!cx.read(bar, MediaTransportBar::shown).unwrap());

        available.set(true);
        cx.flush_reactive().unwrap();
        assert!(!hidden(&cx), "available again, and revealed");
        assert!(cx.read(bar, MediaTransportBar::shown).unwrap());
    }

    /// The bar reports its own menus opening and closing, however they
    /// close, so a host re-syncs the idle hide without listening on nodes
    /// the bar built; after the close the sync restarts the idle timer.
    #[test]
    fn the_bar_reports_its_menus_opening_and_closing() {
        use std::time::Instant;
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        cx.assemble_media_transport_bar(bar).unwrap();
        let events = StdArc::new(Mutex::new(Vec::new()));
        let seen = StdArc::clone(&events);
        cx.on(bar, move |_, event: &MediaTransportEvent, _| {
            seen.lock().unwrap().push(*event);
        })
        .unwrap();
        let take = || std::mem::take(&mut *events.lock().unwrap());
        let (settings, volume) = cx
            .read(bar, |bar| {
                (bar.settings().unwrap(), bar.volume_menu().unwrap())
            })
            .unwrap();

        cx.toggle_action_menu(settings).unwrap();
        assert_eq!(take(), [MediaTransportEvent::MenuOpened]);
        let now = Instant::now();
        assert_eq!(cx.sync_overlay_visibility(bar, now, true).unwrap(), None);
        assert!(cx.dismiss_popovers_on_escape().unwrap());
        assert_eq!(take(), [MediaTransportEvent::MenuClosed]);
        assert_eq!(
            cx.sync_overlay_visibility(bar, now, true).unwrap(),
            Some(now + crate::OVERLAY_IDLE),
            "the close restarts the idle timer"
        );

        cx.toggle_popover(volume).unwrap();
        assert_eq!(take(), [MediaTransportEvent::MenuOpened]);
        assert!(cx.dismiss_popovers_outside(None).unwrap());
        assert_eq!(take(), [MediaTransportEvent::MenuClosed]);

        cx.toggle_action_menu(settings).unwrap();
        take();
        cx.update_component(bar, |bar, _| bar.show_settings = Some(false))
            .unwrap();
        assert_eq!(
            take(),
            [MediaTransportEvent::MenuClosed],
            "hiding the open settings menu closes it"
        );
    }

    /// A live room that is not ready hides the play button, as a view binds
    /// it; the focus does not stay on a hidden button, and the button comes
    /// back when the room is ready.
    #[test]
    fn show_play_hides_the_play_button() {
        use crate::view::{signal, widget};
        let mut cx = AppContext::new();
        let parent = cx.create_component(document(), Stack::column(0.0)).unwrap();
        let state = std::cell::Cell::new(None);
        let refs = std::cell::Cell::new(None);
        cx.mount_view(parent.stable_id(), || {
            let ready = signal(true);
            let bar = crate::view::entity_ref::<MediaTransportBar>();
            state.set(Some(ready));
            refs.set(Some(bar));
            widget(MediaTransportBar::new().live(true))
                .entity_ref(bar)
                .bind(move |bar| bar.show_play = Some(ready.get()))
        })
        .unwrap();
        let ready = state.get().unwrap();
        let bar = refs.get().unwrap().get().unwrap();
        let play = cx.read(bar, |bar| bar.play().unwrap()).unwrap();
        let hidden = |cx: &AppContext| {
            cx.world()
                .node_style(play.stable_id())
                .unwrap()
                .layout
                .hidden
        };
        assert!(!hidden(&cx));
        assert!(cx.focus_node(document(), play.stable_id()).unwrap());

        ready.set(false);
        cx.flush_reactive().unwrap();
        assert!(hidden(&cx), "not ready: no play button");
        assert_eq!(
            cx.world().focused(document()),
            None,
            "the focus leaves the hidden button"
        );

        ready.set(true);
        cx.flush_reactive().unwrap();
        assert!(!hidden(&cx));
    }
    #[test]
    fn assemble_is_idempotent() {
        let mut cx = AppContext::new();
        let bar = cx
            .create_component(document(), MediaTransportBar::new())
            .unwrap();
        assert!(cx.assemble_media_transport_bar(bar).unwrap());
        let first = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        assert!(!cx.assemble_media_transport_bar(bar).unwrap());
        let second = cx.read(bar, |bar| bar.slots().clone()).unwrap();
        assert_eq!(first, second);
    }
}
