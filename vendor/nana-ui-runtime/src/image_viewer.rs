use std::sync::Arc;

use nana_ui_core::{
    ButtonKind, ControlSize, Icon, JustifySpec, LengthSpec, OverflowSpec, PaintTransform,
    PositionSpec, RadiusTier, SemanticColorRole, ThemeMetrics,
};

use crate::overlay_surfaces::modal_root_style;
use crate::view_components::{Activate, IconButton, Stack, Text, project_common};
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, ComponentView, CustomRenderNode, Entity,
    FrameworkError, HOST_TEXTURE_RENDERER, InteractionState, LayoutBox, MutationQueue, NodeKind,
    NodeStyle, StableNodeId, StandardVisual, TextContent, UiWorld,
};

/// Zoom scale step and clamp for the viewer surface.
pub const ZOOM_STEP: f32 = 1.12;
pub const ZOOM_MIN: f32 = 1.0;
pub const ZOOM_MAX: f32 = 6.0;

pub(crate) const SURFACE_PAD: f32 = nana_ui_core::space::PAGE * 2.0 + nana_ui_core::space::SM;
const SURFACE_PAD_TOP: f32 = SURFACE_PAD;
const SURFACE_PAD_RIGHT: f32 = SURFACE_PAD;
const SURFACE_PAD_BOTTOM: f32 = nana_ui_core::space::PAGE;
const SURFACE_PAD_LEFT: f32 = SURFACE_PAD;
const CLOSE_INSET: f32 = nana_ui_core::space::XXL;
const METADATA_GAP: f32 = nana_ui_core::space::LG;
const METADATA_HEIGHT: f32 = nana_ui_core::type_scale::LINE;
const COVERAGE: f32 = 0.75;
/// How far the navigation sits above the foot of the stage.
const NAVIGATION_INSET: f32 = nana_ui_core::space::XL;

/// Close, outside (scrim), and surface interaction are distinct.
/// Mounted viewers dismiss through the shared overlay lifecycle on Escape.
///
/// `Previous` and `Next` ask for the neighbouring image of the gallery the
/// viewer shows ([`ImageViewer::gallery`]); the application loads it and
/// writes the new position back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageViewerEvent {
    Close,
    Outside,
    Interaction,
    Previous,
    Next,
}

/// Where the image on screen sits in the gallery the application shows:
/// `index` (from zero) of `count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageViewerPosition {
    pub index: usize,
    pub count: usize,
}

impl ImageViewerPosition {
    pub const fn new(index: usize, count: usize) -> Self {
        Self { index, count }
    }

    /// Whether there is an image before this one.
    pub const fn has_previous(self) -> bool {
        self.index > 0 && self.index < self.count
    }

    /// Whether there is an image after this one.
    pub const fn has_next(self) -> bool {
        self.index + 1 < self.count
    }

    /// "3 / 9": the image's place, counted from one.
    pub fn counter(self) -> String {
        format!("{} / {}", self.index + 1, self.count)
    }

    /// A gallery of one image has nowhere to go.
    const fn navigates(self) -> bool {
        self.count > 1
    }
}

/// Application-owned visual content. NanaUI never stores pixels or codecs.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ImageViewerContent {
    #[default]
    None,
    Child(StableNodeId),
    HostTexture(Arc<str>),
    CustomRender(CustomRenderNode),
}

impl ImageViewerContent {
    pub fn child(id: StableNodeId) -> Self {
        Self::Child(id)
    }

    pub fn host_texture(slot: impl Into<Arc<str>>) -> Self {
        Self::HostTexture(slot.into())
    }

    pub fn custom_render(node: CustomRenderNode) -> Self {
        Self::CustomRender(node)
    }

    pub fn as_child(&self) -> Option<StableNodeId> {
        match self {
            Self::Child(id) => Some(*id),
            _ => None,
        }
    }

    /// HostTexture slots use [`HOST_TEXTURE_RENDERER`]. Empty identities are omitted.
    pub fn as_custom_render(&self) -> Option<CustomRenderNode> {
        match self {
            Self::HostTexture(slot) if !slot.trim().is_empty() => Some(CustomRenderNode::new(
                HOST_TEXTURE_RENDERER,
                Arc::clone(slot),
                0,
            )),
            Self::CustomRender(node)
                if !node.renderer.trim().is_empty() && !node.resource.trim().is_empty() =>
            {
                Some(node.clone())
            }
            _ => None,
        }
    }
}

impl From<StableNodeId> for ImageViewerContent {
    fn from(id: StableNodeId) -> Self {
        Self::Child(id)
    }
}

impl From<CustomRenderNode> for ImageViewerContent {
    fn from(node: CustomRenderNode) -> Self {
        Self::CustomRender(node)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ImageViewerOffset {
    pub x: f32,
    pub y: f32,
}

impl ImageViewerOffset {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageViewerDrag {
    pub pointer_id: u64,
    pub origin_x: f32,
    pub origin_y: f32,
    pub starting_offset: ImageViewerOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageViewerHit {
    Close,
    Stage,
    Surface,
    Scrim,
    Miss,
}

/// Overlay chrome plus the zoom/pan-transformed content box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageViewerGeometry {
    pub scrim: LayoutBox,
    pub surface: LayoutBox,
    pub stage: LayoutBox,
    pub close: LayoutBox,
    pub name: Option<LayoutBox>,
    pub metadata: Option<LayoutBox>,
    pub content: LayoutBox,
}

impl ImageViewerGeometry {
    pub fn hit(self, x: f32, y: f32) -> ImageViewerHit {
        if self.close.contains(x, y) {
            ImageViewerHit::Close
        } else if self.stage.contains(x, y) {
            ImageViewerHit::Stage
        } else if self.surface.contains(x, y) {
            ImageViewerHit::Surface
        } else if self.scrim.contains(x, y) {
            ImageViewerHit::Scrim
        } else {
            ImageViewerHit::Miss
        }
    }
}

/// The controls a viewer assembles as its own children
/// ([`AppContext::assemble_image_viewer`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ImageViewerControls {
    pub(crate) close: Option<StableNodeId>,
    /// The row that centres the navigation over the foot of the stage.
    pub(crate) navigation: Option<StableNodeId>,
    pub(crate) previous: Option<StableNodeId>,
    pub(crate) counter: Option<StableNodeId>,
    pub(crate) next: Option<StableNodeId>,
}

impl ImageViewerControls {
    /// The viewer's own children among them, in their order.
    fn ids(self) -> impl Iterator<Item = StableNodeId> {
        [self.navigation, self.close].into_iter().flatten()
    }
}

/// Full-window overlay viewer. Application owns decode and HostTexture/content.
///
/// Its controls are real controls — focusable, named, with hover and press
/// states — that the viewer assembles after its content, so a
/// [`ImageViewerContent::Child`] that covers the whole stage never covers
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageViewer {
    pub name: Option<Arc<str>>,
    pub metadata: Option<Arc<str>>,
    pub content: ImageViewerContent,
    pub intrinsic_size: Option<(u32, u32)>,
    pub zoom: f32,
    pub offset: ImageViewerOffset,
    pub dragging: Option<ImageViewerDrag>,
    /// The gallery this image belongs to. With more than one image the
    /// viewer shows previous / next controls and the image's place ("3 / 9"),
    /// and ← / → ask for the neighbouring image.
    pub gallery: Option<ImageViewerPosition>,
    /// Accessible name of the close control.
    pub close_label: Arc<str>,
    /// Accessible name of the previous-image control.
    pub previous_label: Arc<str>,
    /// Accessible name of the next-image control.
    pub next_label: Arc<str>,
    pub style: NodeStyle,
    pub(crate) controls: ImageViewerControls,
}

impl ImageViewer {
    pub fn new(content: impl Into<ImageViewerContent>) -> Self {
        Self {
            name: None,
            metadata: None,
            content: content.into(),
            intrinsic_size: None,
            zoom: ZOOM_MIN,
            offset: ImageViewerOffset::ZERO,
            dragging: None,
            gallery: None,
            close_label: Arc::from("关闭"),
            previous_label: Arc::from("上一张"),
            next_label: Arc::from("下一张"),
            style: overlay_style(),
            controls: ImageViewerControls::default(),
        }
    }

    pub fn intrinsic_size(mut self, width: u32, height: u32) -> Self {
        self.intrinsic_size = (width > 0 && height > 0).then_some((width, height));
        self
    }

    pub fn name(mut self, name: impl Into<Arc<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn metadata(mut self, metadata: impl Into<Arc<str>>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Show image `index` (from zero) of a gallery of `count`.
    pub fn gallery(mut self, index: usize, count: usize) -> Self {
        self.gallery = Some(ImageViewerPosition::new(index, count));
        self
    }

    pub fn close_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.close_label = label.into();
        self
    }

    pub fn previous_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.previous_label = label.into();
        self
    }

    pub fn next_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.next_label = label.into();
        self
    }

    /// The gallery, when it has somewhere to go.
    fn navigation(&self) -> Option<ImageViewerPosition> {
        self.gallery.filter(|gallery| gallery.navigates())
    }

    /// Height of the caption row under the stage, with the gap above it.
    fn caption_band(&self) -> f32 {
        if self.name.is_some() || self.metadata.is_some() {
            METADATA_GAP + METADATA_HEIGHT
        } else {
            0.0
        }
    }

    /// The event asking for the neighbouring image, if there is one.
    fn step(&self, forward: bool) -> Option<ImageViewerEvent> {
        let gallery = self.navigation()?;
        if forward {
            gallery.has_next().then_some(ImageViewerEvent::Next)
        } else {
            gallery.has_previous().then_some(ImageViewerEvent::Previous)
        }
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn close_size(metrics: ThemeMetrics) -> f32 {
        ControlSize::Small.height_in(metrics)
    }

    pub fn geometry(&self, bounds: LayoutBox, metrics: ThemeMetrics) -> ImageViewerGeometry {
        let surface = inset(
            bounds,
            SURFACE_PAD_LEFT,
            SURFACE_PAD_TOP,
            SURFACE_PAD_RIGHT,
            SURFACE_PAD_BOTTOM,
        );
        let has_name = self.name.is_some();
        let has_metadata = self.metadata.is_some();
        let band = self.caption_band();
        let stage = LayoutBox {
            x: surface.x,
            y: surface.y,
            width: surface.width,
            height: (surface.height - band).max(0.0),
        };
        let (name, metadata) = caption_boxes(surface, has_name, has_metadata);
        let zoom = clamp_zoom(self.zoom);
        let fitted = fitted_bounds(stage, self.intrinsic_size);
        let offset = clamp_offset(self.offset, zoom, stage, fitted);
        ImageViewerGeometry {
            scrim: bounds,
            surface,
            stage,
            close: close_box(surface, metrics),
            name,
            metadata,
            content: transform_about(fitted, stage, zoom, offset),
        }
    }

    /// CSS matrix around the stage center: scale(zoom) then translate(offset).
    pub fn content_transform(&self) -> PaintTransform {
        let zoom = clamp_zoom(self.zoom);
        let offset = if zoom <= 1.0 {
            ImageViewerOffset::ZERO
        } else {
            self.offset
        };
        PaintTransform {
            a: zoom,
            d: zoom,
            e: offset.x,
            f: offset.y,
            ..PaintTransform::default()
        }
    }

    /// Applies zoom/pan about `stage` center.
    pub fn transformed_bounds(&self, content: LayoutBox, stage: LayoutBox) -> LayoutBox {
        let zoom = clamp_zoom(self.zoom);
        transform_about(
            content,
            stage,
            zoom,
            clamp_offset(self.offset, zoom, stage, content),
        )
    }

    pub fn pointer_down(
        &mut self,
        geometry: &ImageViewerGeometry,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Option<ImageViewerEvent> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        match geometry.hit(x, y) {
            ImageViewerHit::Close => {
                self.dragging = None;
                Some(ImageViewerEvent::Close)
            }
            ImageViewerHit::Stage => {
                self.begin_pan(pointer_id, x, y);
                Some(ImageViewerEvent::Interaction)
            }
            ImageViewerHit::Surface => {
                self.dragging = None;
                Some(ImageViewerEvent::Interaction)
            }
            ImageViewerHit::Scrim => {
                self.dragging = None;
                Some(ImageViewerEvent::Outside)
            }
            ImageViewerHit::Miss => None,
        }
    }

    pub fn pointer_move(
        &mut self,
        geometry: &ImageViewerGeometry,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(drag) = self.dragging.filter(|drag| drag.pointer_id == pointer_id) else {
            return false;
        };
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        self.offset = clamp_offset(
            ImageViewerOffset::new(
                drag.starting_offset.x + (x - drag.origin_x),
                drag.starting_offset.y + (y - drag.origin_y),
            ),
            clamp_zoom(self.zoom),
            geometry.stage,
            fitted_bounds(geometry.stage, self.intrinsic_size),
        );
        true
    }

    pub fn pointer_up(&mut self, pointer_id: u64) -> bool {
        if self
            .dragging
            .is_some_and(|drag| drag.pointer_id == pointer_id)
        {
            self.dragging = None;
            true
        } else {
            false
        }
    }

    pub fn wheel(&mut self, geometry: &ImageViewerGeometry, x: f32, y: f32, delta_y: f32) -> bool {
        if !geometry.stage.contains(x, y) || !delta_y.is_finite() || delta_y == 0.0 {
            return false;
        }
        let previous = clamp_zoom(self.zoom);
        self.zoom = if delta_y > 0.0 {
            previous * ZOOM_STEP
        } else {
            previous / ZOOM_STEP
        }
        .clamp(ZOOM_MIN, ZOOM_MAX);
        let factor = self.zoom / previous - 1.0;
        let cx = geometry.stage.x + geometry.stage.width * 0.5;
        let cy = geometry.stage.y + geometry.stage.height * 0.5;
        self.offset = clamp_offset(
            ImageViewerOffset::new(
                self.offset.x + (x - cx) * factor + self.offset.x * factor,
                self.offset.y + (y - cy) * factor + self.offset.y * factor,
            ),
            self.zoom,
            geometry.stage,
            fitted_bounds(geometry.stage, self.intrinsic_size),
        );
        true
    }

    fn begin_pan(&mut self, pointer_id: u64, x: f32, y: f32) {
        self.zoom = clamp_zoom(self.zoom);
        if self.zoom > ZOOM_MIN {
            self.dragging = Some(ImageViewerDrag {
                pointer_id,
                origin_x: x,
                origin_y: y,
                starting_offset: self.offset,
            });
        } else {
            self.dragging = None;
            self.offset = ImageViewerOffset::ZERO;
        }
    }
}

impl Default for ImageViewer {
    fn default() -> Self {
        Self::new(ImageViewerContent::None)
    }
}

impl ComponentView for ImageViewer {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        assembler: Some(AppContext::assemble_image_viewer),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    /// Content placed after the controls would cover them; a change to the
    /// children assembles again, which moves the controls back to the end.
    fn wants_child_reproject() -> bool {
        true
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "image-viewer".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let visual = StandardVisual::ImageViewer {
            intrinsic_size: self.intrinsic_size,
            name: self.name.clone(),
            metadata: self.metadata.clone(),
            zoom: self.zoom,
            offset_x: self.offset.x,
            offset_y: self.offset.y,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        let label = self.name.clone();
        let text = label.as_deref().unwrap_or_default();
        if world.text(id) != Some(text) {
            mutations.set_text(
                id,
                TextContent {
                    value: text.to_owned().into(),
                },
            );
        }
        let custom = self.content.as_custom_render();
        if world.custom_render(id) != custom.as_ref() {
            mutations.set_custom_render(id, custom);
        }
        project_common(
            id,
            world,
            mutations,
            &self.style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Dialog,
                label,
                value: self.navigation().map(|gallery| gallery.counter().into()),
                description: self.metadata.clone(),
                modal: true,
                ..AccessibilityState::default()
            },
        );
    }
}

/// The close control, in the corner of the surface that
/// [`ImageViewerGeometry::close`] names.
fn close_control(label: Arc<str>) -> IconButton {
    let mut button = IconButton::new(Icon::Close, label)
        .kind(ButtonKind::Subtle)
        .size(ControlSize::Small);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.position = PositionSpec::Absolute;
    layout.offset_top = Some(LengthSpec::Px(SURFACE_PAD_TOP + CLOSE_INSET));
    layout.offset_right = Some(LengthSpec::Px(SURFACE_PAD_RIGHT + CLOSE_INSET));
    button
}

fn step_control(icon: Icon, label: Arc<str>) -> IconButton {
    IconButton::new(icon, label)
        .kind(ButtonKind::Text)
        .size(ControlSize::Small)
}

fn navigation_bottom(caption_band: f32) -> f32 {
    SURFACE_PAD_BOTTOM + caption_band + NAVIGATION_INSET
}

/// A row as wide as the stage, over its foot, that centres the navigation.
/// It takes no pointer itself: only the controls on it do.
fn navigation_row(caption_band: f32) -> Stack {
    Stack::row(0.0).with_layout(|layout| {
        layout.position = PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(SURFACE_PAD_LEFT));
        layout.offset_right = Some(LengthSpec::Px(SURFACE_PAD_RIGHT));
        layout.offset_bottom = Some(LengthSpec::Px(navigation_bottom(caption_band)));
        layout.width = None;
        layout.justify_content = JustifySpec::Center;
    })
}

/// The surface the previous / next controls and the counter sit on, so they
/// read over any image.
fn navigation_pill() -> Stack {
    let mut pill = Stack::row(nana_ui_core::space::XS).with_layout(|layout| {
        let pad = Some(LengthSpec::Px(nana_ui_core::space::XXS));
        layout.padding_left = pad;
        layout.padding_right = pad;
        layout.padding_top = pad;
        layout.padding_bottom = pad;
        layout.border_width = Some(nana_ui_core::HAIRLINE);
    });
    let style = pill.style_mut();
    style.background = Some(SemanticColorRole::Surface);
    style.border = Some(SemanticColorRole::BorderSoft);
    style.foreground = Some(SemanticColorRole::Text);
    style.radius = Some(RadiusTier::Md);
    pill
}

impl AppContext {
    /// Builds (or refreshes) the controls of an [`ImageViewer`]: the close
    /// button, placed where [`ImageViewerGeometry::close`] is, and — for a
    /// [`ImageViewer::gallery`] of more than one image — previous / next
    /// buttons around the image's place ("3 / 9") over the foot of the
    /// stage. They are kept after every other child, so the content never
    /// paints or hit-tests above them.
    ///
    /// Runs after each write to the viewer and when a view builds one; a
    /// viewer made with `create_component` calls it once itself. Idempotent,
    /// and writes nothing when the controls are already current. Returns
    /// whether it created them.
    pub fn assemble_image_viewer(
        &mut self,
        viewer: Entity<ImageViewer>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world()
            .node(viewer.stable_id())
            .ok_or(FrameworkError::MissingView(viewer.stable_id()))?
            .document;
        let snapshot = self.read(viewer, Clone::clone)?;
        let mut controls = snapshot.controls;
        let created = !controls
            .close
            .is_some_and(|close| self.world().contains(close));
        if created {
            let close = self.create_detached_component(
                document,
                close_control(Arc::clone(&snapshot.close_label)),
            )?;
            let navigation =
                self.create_detached_component(document, navigation_row(snapshot.caption_band()))?;
            let pill = self.create_detached_component(document, navigation_pill())?;
            let previous = self.create_detached_component(
                document,
                step_control(Icon::ArrowLeft, Arc::clone(&snapshot.previous_label)),
            )?;
            let counter = self.create_detached_component(
                document,
                Text::new("").font_size(nana_ui_core::type_scale::META),
            )?;
            let next = self.create_detached_component(
                document,
                step_control(Icon::ArrowRight, Arc::clone(&snapshot.next_label)),
            )?;
            self.append_child(pill, previous)?;
            self.append_child(pill, counter)?;
            self.append_child(pill, next)?;
            self.append_child(navigation, pill)?;
            self.observe(
                close,
                viewer,
                |viewer: &mut ImageViewer, _: &Activate, cx| {
                    viewer.dragging = None;
                    cx.emit(ImageViewerEvent::Close);
                },
            )?;
            for (button, forward) in [(previous, false), (next, true)] {
                self.observe(
                    button,
                    viewer,
                    move |viewer: &mut ImageViewer, _: &Activate, cx| {
                        if let Some(event) = viewer.step(forward) {
                            cx.emit(event);
                        }
                    },
                )?;
            }
            controls = ImageViewerControls {
                close: Some(close.stable_id()),
                navigation: Some(navigation.stable_id()),
                previous: Some(previous.stable_id()),
                counter: Some(counter.stable_id()),
                next: Some(next.stable_id()),
            };
            self.update_component(viewer, |viewer, _| viewer.controls = controls)?;
        }
        let button = |id: Option<StableNodeId>| id.map(Entity::<IconButton>::from_stable_id);
        if let Some(close) = button(controls.close) {
            let label = Arc::clone(&snapshot.close_label);
            self.update_component(close, |close, _| close.label = label)?;
        }
        let gallery = snapshot.navigation();
        if let Some(navigation) = controls.navigation.map(Entity::<Stack>::from_stable_id) {
            let hidden = gallery.is_none();
            let bottom = Some(LengthSpec::Px(navigation_bottom(snapshot.caption_band())));
            self.update_component(navigation, |row, _| {
                let layout = &row.style_ref().layout;
                if layout.hidden != hidden || layout.offset_bottom != bottom {
                    let layout = Arc::make_mut(&mut row.style_mut().layout);
                    layout.hidden = hidden;
                    layout.offset_bottom = bottom;
                }
            })?;
        }
        let steps = [
            (
                controls.previous,
                &snapshot.previous_label,
                gallery.is_some_and(ImageViewerPosition::has_previous),
            ),
            (
                controls.next,
                &snapshot.next_label,
                gallery.is_some_and(ImageViewerPosition::has_next),
            ),
        ];
        for (id, label, enabled) in steps {
            if let Some(step) = button(id) {
                let label = Arc::clone(label);
                self.update_component(step, |step, _| {
                    step.label = label;
                    step.disabled = !enabled;
                })?;
            }
        }
        if let Some(counter) = controls.counter.map(Entity::<Text>::from_stable_id) {
            let value = gallery
                .map(ImageViewerPosition::counter)
                .unwrap_or_default();
            self.update_component(counter, |counter, _| {
                if counter.value != value {
                    counter.value = value;
                }
            })?;
        }
        // Content placed after the controls would cover them: move the
        // controls back to the end, in their order.
        let children = self
            .world()
            .node(viewer.stable_id())
            .map(|node| node.children)
            .unwrap_or_default();
        let wanted = controls.ids().collect::<Vec<_>>();
        if !children.ends_with(&wanted) {
            for id in wanted {
                self.attach_child(viewer.stable_id(), id)?;
            }
        }
        Ok(created)
    }

    /// ← / → on a focused viewer, or on a control inside one: ask for the
    /// neighbouring image of its gallery. Answers whether it asked.
    pub(crate) fn step_focused_image_viewer(
        &mut self,
        document: crate::DocumentId,
        forward: bool,
    ) -> Result<bool, FrameworkError> {
        let mut current = self.world().focused(document);
        while let Some(id) = current {
            if let Some(viewer) = self.view_entity::<ImageViewer>(id) {
                return self.update_component(viewer, |viewer, cx| match viewer.step(forward) {
                    Some(event) => {
                        cx.emit(event);
                        true
                    }
                    None => false,
                });
            }
            current = self.world().parent_id(id);
        }
        Ok(false)
    }

    pub fn image_viewer_pointer_down(
        &mut self,
        viewer: crate::Entity<ImageViewer>,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<Option<ImageViewerEvent>, crate::FrameworkError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(crate::FrameworkError::InvalidInput);
        }
        if !self.world().is_mounted(viewer.stable_id()) {
            return Ok(None);
        }
        let Some((x, y)) = self
            .world()
            .pointer_layout_position(viewer.stable_id(), x, y)
        else {
            return Ok(None);
        };
        let Some(bounds) = self.world().layout_box(viewer.stable_id()) else {
            return Ok(None);
        };
        let metrics = self.world().theme_metrics();
        let id = viewer.stable_id();
        self.update_component(viewer, |viewer, cx| {
            let event = viewer.pointer_down(&viewer.geometry(bounds, metrics), pointer_id, x, y);
            if viewer.dragging.is_some() {
                cx.mutations().capture_pointer(pointer_id, id);
            }
            if let Some(event) = event {
                cx.emit(event);
            }
            event
        })
    }

    pub fn image_viewer_pointer_move(
        &mut self,
        viewer: crate::Entity<ImageViewer>,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(crate::FrameworkError::InvalidInput);
        }
        let id = viewer.stable_id();
        let Some(document) = self.world().document_of(id) else {
            return Ok(false);
        };
        if !self.world().is_mounted(id) {
            return Ok(false);
        }
        if self.world().pointer_capture(document, pointer_id) != Some(id) {
            self.update_component(viewer, |viewer, _| {
                viewer.pointer_up(pointer_id);
            })?;
            return Ok(false);
        }
        let Some((x, y)) = self.world().pointer_layout_position(id, x, y) else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(viewer.stable_id()) else {
            return Ok(false);
        };
        let metrics = self.world().theme_metrics();
        self.update_component(viewer, |viewer, _| {
            viewer.pointer_move(&viewer.geometry(bounds, metrics), pointer_id, x, y)
        })
    }

    pub fn image_viewer_pointer_up(
        &mut self,
        viewer: crate::Entity<ImageViewer>,
        pointer_id: u64,
    ) -> Result<bool, crate::FrameworkError> {
        let id = viewer.stable_id();
        self.update_component(viewer, |viewer, cx| {
            let ended = viewer.pointer_up(pointer_id);
            if ended {
                cx.mutations().release_pointer(pointer_id, id);
            }
            ended
        })
    }

    pub fn image_viewer_wheel(
        &mut self,
        viewer: crate::Entity<ImageViewer>,
        x: f32,
        y: f32,
        delta_y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        if !x.is_finite() || !y.is_finite() || !delta_y.is_finite() {
            return Err(crate::FrameworkError::InvalidInput);
        }
        if !self.world().is_mounted(viewer.stable_id()) {
            return Ok(false);
        }
        let Some((x, y)) = self
            .world()
            .pointer_layout_position(viewer.stable_id(), x, y)
        else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(viewer.stable_id()) else {
            return Ok(false);
        };
        let metrics = self.world().theme_metrics();
        self.update_component(viewer, |viewer, _| {
            viewer.wheel(&viewer.geometry(bounds, metrics), x, y, delta_y)
        })
    }
}

fn overlay_style() -> NodeStyle {
    let mut style = modal_root_style();
    let layout = Arc::make_mut(&mut style.layout);
    layout.overflow_x = OverflowSpec::Hidden;
    layout.overflow_y = OverflowSpec::Hidden;
    style.background = Some(SemanticColorRole::Background);
    style
}

fn inset(bounds: LayoutBox, left: f32, top: f32, right: f32, bottom: f32) -> LayoutBox {
    LayoutBox {
        x: bounds.x + left,
        y: bounds.y + top,
        width: (bounds.width - left - right).max(0.0),
        height: (bounds.height - top - bottom).max(0.0),
    }
}

fn close_box(surface: LayoutBox, metrics: ThemeMetrics) -> LayoutBox {
    let size = ImageViewer::close_size(metrics);
    LayoutBox {
        x: surface.x + surface.width - CLOSE_INSET - size,
        y: surface.y + CLOSE_INSET,
        width: size,
        height: size,
    }
}

fn caption_boxes(
    surface: LayoutBox,
    has_name: bool,
    has_metadata: bool,
) -> (Option<LayoutBox>, Option<LayoutBox>) {
    if !has_name && !has_metadata {
        return (None, None);
    }
    let y = surface.y + surface.height - METADATA_HEIGHT;
    let row = LayoutBox {
        x: surface.x,
        y,
        width: surface.width,
        height: METADATA_HEIGHT.max(0.0),
    };
    if has_name && has_metadata {
        let gap = METADATA_GAP;
        let half = ((surface.width - gap) / 2.0).max(0.0);
        (
            Some(LayoutBox {
                x: surface.x,
                y,
                width: half,
                height: row.height,
            }),
            Some(LayoutBox {
                x: surface.x + half + gap,
                y,
                width: half,
                height: row.height,
            }),
        )
    } else if has_name {
        (Some(row), None)
    } else {
        (None, Some(row))
    }
}

fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(ZOOM_MIN, ZOOM_MAX)
    } else {
        ZOOM_MIN
    }
}

fn fitted_bounds(stage: LayoutBox, size: Option<(u32, u32)>) -> LayoutBox {
    let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        return stage;
    };
    let scale = (stage.width / width as f32)
        .min(stage.height / height as f32)
        .clamp(0.0, 1.0);
    let (width, height) = (width as f32 * scale, height as f32 * scale);
    LayoutBox {
        x: stage.x + (stage.width - width) * 0.5,
        y: stage.y + (stage.height - height) * 0.5,
        width,
        height,
    }
}

fn clamp_offset(
    offset: ImageViewerOffset,
    zoom: f32,
    stage: LayoutBox,
    fitted: LayoutBox,
) -> ImageViewerOffset {
    if zoom <= 1.0 {
        return ImageViewerOffset::ZERO;
    }
    ImageViewerOffset::new(
        clamp_axis(offset.x, fitted.width * zoom, stage.width),
        clamp_axis(offset.y, fitted.height * zoom, stage.height),
    )
}

fn clamp_axis(value: f32, rendered: f32, viewport: f32) -> f32 {
    let required_coverage = viewport * COVERAGE;
    if rendered < required_coverage {
        return 0.0;
    }
    let max = ((viewport + rendered) / 2.0 - required_coverage).max(0.0);
    if value.is_finite() {
        value.clamp(-max, max)
    } else {
        0.0
    }
}

fn transform_about(
    bounds: LayoutBox,
    stage: LayoutBox,
    zoom: f32,
    offset: ImageViewerOffset,
) -> LayoutBox {
    let cx = stage.x + stage.width * 0.5;
    let cy = stage.y + stage.height * 0.5;
    LayoutBox {
        x: zoom * (bounds.x - cx) + cx + offset.x,
        y: zoom * (bounds.y - cy) + cy + offset.y,
        width: (bounds.width * zoom).max(0.0),
        height: (bounds.height * zoom).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext, DocumentId, OverlayHost};
    use std::sync::{Arc, Mutex};

    fn bounds() -> LayoutBox {
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 300.0,
        }
    }

    fn stage_point(geometry: &ImageViewerGeometry, nx: f32, ny: f32) -> (f32, f32) {
        (
            geometry.stage.x + geometry.stage.width * nx,
            geometry.stage.y + geometry.stage.height * ny,
        )
    }

    #[test]
    fn wheel_zoom_around_a_point_changes_zoom_and_offset() {
        let mut viewer = ImageViewer::new(ImageViewerContent::None);
        let geometry = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        let (x, y) = stage_point(&geometry, 0.75, 0.5);
        assert!(viewer.wheel(&geometry, x, y, 1.0));
        assert!((viewer.zoom - ZOOM_STEP).abs() < 1e-6);
        let factor = ZOOM_STEP / ZOOM_MIN - 1.0;
        let cx = geometry.stage.x + geometry.stage.width * 0.5;
        assert!((viewer.offset.x - (x - cx) * factor).abs() < 1e-5);
        assert!(viewer.offset.y.abs() < 1e-5);
        assert!(
            viewer
                .geometry(bounds(), nana_ui_core::UI_METRICS)
                .content
                .width
                > geometry.stage.width
        );
    }

    #[test]
    fn pan_updates_offset() {
        let mut viewer = ImageViewer::new(ImageViewerContent::None);
        viewer.zoom = 2.0;
        let geometry = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        let (x, y) = stage_point(&geometry, 0.5, 0.5);
        assert_eq!(
            viewer.pointer_down(&geometry, 1, x, y),
            Some(ImageViewerEvent::Interaction)
        );
        assert!(viewer.pointer_move(&geometry, 1, x + 20.0, y - 16.0));
        assert!((viewer.offset.x - 20.0).abs() < 1e-5);
        assert!((viewer.offset.y + 16.0).abs() < 1e-5);
        assert!(viewer.pointer_up(1));
        assert!(viewer.dragging.is_none());
    }

    #[test]
    fn close_and_outside_are_distinct_events() {
        let mut viewer = ImageViewer::new(ImageViewerContent::None)
            .name("preview")
            .metadata("1600 × 900");
        let geometry = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        let close = (
            geometry.close.x + geometry.close.width * 0.5,
            geometry.close.y + geometry.close.height * 0.5,
        );
        let (stage_x, stage_y) = stage_point(&geometry, 0.4, 0.4);
        assert_eq!(
            viewer.pointer_down(&geometry, 1, close.0, close.1),
            Some(ImageViewerEvent::Close)
        );
        assert_eq!(
            viewer.pointer_down(&geometry, 2, 8.0, 8.0),
            Some(ImageViewerEvent::Outside)
        );
        assert_eq!(
            viewer.pointer_down(&geometry, 3, stage_x, stage_y),
            Some(ImageViewerEvent::Interaction)
        );
        assert_ne!(ImageViewerEvent::Close, ImageViewerEvent::Outside);
    }

    #[test]
    fn zoom_clamps_to_min_max() {
        let mut viewer = ImageViewer::new(ImageViewerContent::None);
        let geometry = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        let (x, y) = stage_point(&geometry, 0.2, 0.3);
        for _ in 0..64 {
            viewer.wheel(&geometry, x, y, 1.0);
        }
        assert_eq!(viewer.zoom, ZOOM_MAX);
        for _ in 0..64 {
            viewer.wheel(&geometry, x, y, -1.0);
        }
        assert_eq!(viewer.zoom, ZOOM_MIN);
        assert_eq!(viewer.offset, ImageViewerOffset::ZERO);
        viewer.zoom = 99.0;
        viewer.wheel(&geometry, x, y, 1.0);
        assert_eq!(viewer.zoom, ZOOM_MAX);
    }

    #[test]
    fn pan_clamp_keeps_required_content_coverage_visible() {
        let stage = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 80.0,
        };
        assert_eq!(
            clamp_offset(ImageViewerOffset::new(500.0, -500.0), 2.0, stage, stage),
            ImageViewerOffset::new(75.0, -60.0)
        );
        assert_eq!(
            clamp_offset(ImageViewerOffset::new(20.0, 20.0), 1.0, stage, stage),
            ImageViewerOffset::ZERO
        );
    }

    #[test]
    fn zoom_pan_transform_is_applied_to_content_bounds() {
        let mut viewer = ImageViewer::new(ImageViewerContent::None);
        viewer.zoom = 2.0;
        viewer.offset = ImageViewerOffset::new(10.0, -4.0);
        let stage = viewer.geometry(bounds(), nana_ui_core::UI_METRICS).stage;
        let content = viewer.transformed_bounds(stage, stage);
        assert!((content.width - stage.width * 2.0).abs() < 1e-5);
        assert!((content.height - stage.height * 2.0).abs() < 1e-5);
        assert!((content.x - (stage.x - stage.width * 0.5 + 10.0)).abs() < 1e-5);
        assert!((content.y - (stage.y - stage.height * 0.5 - 4.0)).abs() < 1e-5);
        let matrix = viewer.content_transform();
        assert_eq!(matrix.a, 2.0);
        assert_eq!(matrix.d, 2.0);
        assert_eq!(matrix.e, 10.0);
        assert_eq!(matrix.f, -4.0);
    }

    #[test]
    fn host_texture_projects_custom_render_identity() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let viewer = context
            .create_component(
                document,
                ImageViewer::new(ImageViewerContent::host_texture("preview-slot"))
                    .name("NanaUI 渲染预览"),
            )
            .unwrap();
        let custom = context
            .world()
            .custom_render(viewer.stable_id())
            .cloned()
            .unwrap();
        assert_eq!(custom.renderer.as_ref(), HOST_TEXTURE_RENDERER);
        assert_eq!(custom.resource.as_ref(), "preview-slot");
        let accessibility = context.world().accessibility(viewer.stable_id()).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::Dialog);
        assert!(accessibility.modal);
        assert_eq!(accessibility.label.as_deref(), Some("NanaUI 渲染预览"));
        assert!(matches!(
            context.world().node(viewer.stable_id()).unwrap().kind,
            NodeKind::Element { tag } if tag == "image-viewer"
        ));
        assert!(matches!(
            context.world().standard_visual(viewer.stable_id()),
            Some(StandardVisual::ImageViewer {
                intrinsic_size: None,
                ref name,
                ref metadata,
                zoom,
                offset_x,
                offset_y,
            }) if name.as_deref() == Some("NanaUI 渲染预览")
                && metadata.is_none()
                && zoom == ZOOM_MIN
                && offset_x == 0.0
                && offset_y == 0.0
        ));

        let child = context
            .create_detached_component(document, crate::Button::new("decoded"))
            .unwrap();
        let slotted = context
            .create_component(
                document,
                ImageViewer::new(ImageViewerContent::child(child.stable_id())),
            )
            .unwrap();
        assert!(context.world().custom_render(slotted.stable_id()).is_none());
        assert!(matches!(
            context.world().standard_visual(slotted.stable_id()),
            Some(StandardVisual::ImageViewer { .. })
        ));
    }

    fn close_control(context: &AppContext, viewer: crate::Entity<ImageViewer>) -> StableNodeId {
        context
            .read(viewer, |viewer| viewer.controls.close)
            .unwrap()
            .expect("the viewer assembles its close control")
    }

    #[test]
    fn an_installed_compact_height_reaches_the_close_control() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let viewer = context
            .create_component(document, ImageViewer::new(ImageViewerContent::None))
            .unwrap();
        context.assemble_image_viewer(viewer).unwrap();
        let close = close_control(&context, viewer);
        let viewport = crate::LayoutViewport::new(400.0, 300.0);
        let close_box = |context: &mut AppContext| {
            context.layout_document(document, viewport).unwrap();
            let bounds = context.world().layout_box(viewer.stable_id()).unwrap();
            let expected = context
                .read(viewer, |view| {
                    view.geometry(bounds, context.world().theme_metrics()).close
                })
                .unwrap();
            (context.world().layout_box(close).unwrap(), expected)
        };
        let default = nana_ui_core::UI_METRICS.compact_control_height;
        let (actual, expected) = close_box(&mut context);
        assert_eq!(actual, expected);
        assert_eq!((actual.width, actual.height), (default, default));
        let mut metrics = nana_ui_core::UI_METRICS;
        metrics.compact_control_height = 36.0;
        assert!(
            context
                .set_style_tokens(
                    nana_ui_core::ThemeMode::Dark,
                    metrics,
                    nana_ui_core::SemanticPalette::dark(),
                    nana_ui_core::SemanticPalette::dark().surface,
                )
                .unwrap()
        );
        let (actual, expected) = close_box(&mut context);
        assert_eq!(actual, expected);
        assert_eq!((actual.width, actual.height), (36.0, 36.0));
    }

    /// A child that covers the whole viewer is under its close control: the
    /// control is hit, named, and closes the viewer.
    #[test]
    fn child_content_never_covers_the_close_control() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        // A hittable child as large as the viewer: an image the application
        // lets the user press.
        let mut picture = crate::Button::new("picture");
        {
            let layout = Arc::make_mut(&mut picture.style.layout);
            layout.position = PositionSpec::Absolute;
            layout.offset_left = Some(LengthSpec::Px(0.0));
            layout.offset_top = Some(LengthSpec::Px(0.0));
            layout.width = Some(LengthSpec::Percent(100.0));
            layout.height = Some(LengthSpec::Percent(100.0));
        }
        let cover = context
            .create_detached_component(document, picture)
            .unwrap();
        let viewer = context
            .create_component(
                document,
                ImageViewer::new(ImageViewerContent::child(cover.stable_id())),
            )
            .unwrap();
        context.assemble_image_viewer(viewer).unwrap();
        // Content that arrives after the viewer assembled its controls.
        context.append_child(viewer, cover).unwrap();
        let close = close_control(&context, viewer);
        let navigation = context
            .read(viewer, |viewer| viewer.controls.navigation)
            .unwrap()
            .unwrap();
        assert_eq!(
            context.world().node(viewer.stable_id()).unwrap().children,
            [cover.stable_id(), navigation, close]
        );
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let control = context.world().layout_box(close).unwrap();
        let (x, y) = (
            control.x + control.width / 2.0,
            control.y + control.height / 2.0,
        );
        assert_eq!(context.world().hit_test(document, x, y), Some(close));
        assert_eq!(
            context
                .world()
                .accessibility(close)
                .unwrap()
                .label
                .as_deref(),
            Some("关闭")
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(viewer, move |_viewer, event: &ImageViewerEvent, _cx| {
                observed.lock().unwrap().push(*event);
            })
            .unwrap();
        assert!(context.activate_node(close).unwrap());
        assert_eq!(*events.lock().unwrap(), [ImageViewerEvent::Close]);
    }

    /// A gallery of more than one image shows previous / next controls and
    /// the image's place; each control asks for its neighbour and is off at
    /// the end it points past.
    #[test]
    fn a_gallery_shows_named_previous_and_next_controls_and_the_place() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let viewer = context
            .create_component(document, ImageViewer::new(ImageViewerContent::None))
            .unwrap();
        context.assemble_image_viewer(viewer).unwrap();
        let controls = context.read(viewer, |viewer| viewer.controls).unwrap();
        let (navigation, previous, counter, next) = (
            controls.navigation.unwrap(),
            controls.previous.unwrap(),
            controls.counter.unwrap(),
            controls.next.unwrap(),
        );
        let hidden = |context: &AppContext| {
            context
                .world()
                .node_style(navigation)
                .unwrap()
                .layout
                .hidden
        };
        // No gallery, then a gallery of one: nowhere to go.
        assert!(hidden(&context));
        context
            .update_component(viewer, |viewer, _| {
                viewer.gallery = Some(ImageViewerPosition::new(0, 1))
            })
            .unwrap();
        assert!(hidden(&context));

        context
            .update_component(viewer, |viewer, _| {
                viewer.gallery = Some(ImageViewerPosition::new(2, 9))
            })
            .unwrap();
        assert!(!hidden(&context));
        assert_eq!(context.world().text(counter), Some("3 / 9"));
        let accessibility = |id| context.world().accessibility(id).unwrap().clone();
        assert_eq!(accessibility(previous).label.as_deref(), Some("上一张"));
        assert_eq!(accessibility(next).label.as_deref(), Some("下一张"));
        assert_eq!(
            accessibility(viewer.stable_id()).value.as_deref(),
            Some("3 / 9")
        );
        // The controls stay after the content, over it.
        assert_eq!(
            context.world().node(viewer.stable_id()).unwrap().children,
            [navigation, controls.close.unwrap()]
        );

        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(viewer, move |_viewer, event: &ImageViewerEvent, _cx| {
                observed.lock().unwrap().push(*event);
            })
            .unwrap();
        assert!(context.activate_node(previous).unwrap());
        assert!(context.activate_node(next).unwrap());
        assert_eq!(
            *events.lock().unwrap(),
            [ImageViewerEvent::Previous, ImageViewerEvent::Next]
        );

        let disabled =
            |context: &AppContext, id| context.world().accessibility(id).unwrap().disabled;
        context
            .update_component(viewer, |viewer, _| {
                viewer.gallery = Some(ImageViewerPosition::new(0, 9))
            })
            .unwrap();
        assert!(disabled(&context, previous) && !disabled(&context, next));
        context
            .update_component(viewer, |viewer, _| {
                viewer.gallery = Some(ImageViewerPosition::new(8, 9))
            })
            .unwrap();
        assert!(!disabled(&context, previous) && disabled(&context, next));
        assert_eq!(context.world().text(counter), Some("9 / 9"));

        // Over the foot of the stage, above the caption row.
        context
            .update_component(viewer, |viewer, _| viewer.name = Some("图".into()))
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        let bounds = context.world().layout_box(viewer.stable_id()).unwrap();
        let stage = context
            .read(viewer, |view| {
                view.geometry(bounds, context.world().theme_metrics()).stage
            })
            .unwrap();
        let row = context.world().layout_box(navigation).unwrap();
        assert!(row.y + row.height <= stage.y + stage.height);
        assert!(row.y >= stage.y);
    }

    /// The backdrop is the theme's media scrim, dark in the light theme
    /// as in the dark one.
    #[test]
    fn the_backdrop_is_a_dark_scrim_in_both_themes() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let viewer = context
            .create_component(document, ImageViewer::new(ImageViewerContent::None))
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        for (mode, palette) in [
            (
                nana_ui_core::ThemeMode::Light,
                nana_ui_core::SemanticPalette::light(),
            ),
            (
                nana_ui_core::ThemeMode::Dark,
                nana_ui_core::SemanticPalette::dark(),
            ),
        ] {
            context
                .set_style_tokens(mode, nana_ui_core::UI_METRICS, palette, palette.surface)
                .unwrap();
            let Some(crate::ComponentGeometry::ImageViewer { scrim_color, .. }) =
                context.world().component_geometry(viewer.stable_id())
            else {
                panic!("image viewer geometry");
            };
            assert_eq!(
                scrim_color,
                nana_ui_core::EffectTokens::for_mode(mode)
                    .media_scrim
                    .as_rgba_array()
            );
            let [r, g, b, a] = scrim_color;
            assert!(
                r.max(g).max(b) < 0.2 && a > 0.8,
                "{mode:?}: {scrim_color:?}"
            );
        }
    }

    #[test]
    fn overlay_host_can_activate_the_viewer() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let viewer = context
            .create_component(
                document,
                ImageViewer::new(ImageViewerContent::None).name("preview"),
            )
            .unwrap();
        context.append_child(host, viewer).unwrap();
        assert!(context.activate_overlay(host, viewer).unwrap());
        assert_eq!(
            context
                .world()
                .overlay_host(host.stable_id())
                .unwrap()
                .active,
            Some(viewer.stable_id())
        );
    }

    #[test]
    fn app_context_emits_close_and_outside() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let viewer = context
            .create_component(document, ImageViewer::new(ImageViewerContent::None))
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        context.rebuild_hit_test(document);
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(viewer, move |_viewer, event: &ImageViewerEvent, _cx| {
                observed.lock().unwrap().push(*event);
            })
            .unwrap();
        let geometry = context
            .read(viewer, |view| {
                view.geometry(
                    context.world().layout_box(viewer.stable_id()).unwrap(),
                    context.world().theme_metrics(),
                )
            })
            .unwrap();
        let close = context
            .world()
            .layout_pointer_position(
                viewer.stable_id(),
                geometry.close.x + 2.0,
                geometry.close.y + 2.0,
            )
            .unwrap();
        assert_eq!(
            context
                .image_viewer_pointer_down(viewer, 1, close.0, close.1)
                .unwrap(),
            Some(ImageViewerEvent::Close)
        );
        assert_eq!(
            context
                .image_viewer_pointer_down(viewer, 2, 4.0, 4.0)
                .unwrap(),
            Some(ImageViewerEvent::Outside)
        );
        assert_eq!(
            *events.lock().unwrap(),
            [ImageViewerEvent::Close, ImageViewerEvent::Outside]
        );
    }
    #[test]
    fn intrinsic_images_keep_small_size_and_contain_large_aspect_ratios() {
        let small = ImageViewer::default()
            .intrinsic_size(72, 40)
            .geometry(bounds(), nana_ui_core::UI_METRICS);
        assert_eq!((small.content.width, small.content.height), (72.0, 40.0));
        assert!((small.content.x + 36.0 - small.stage.x - small.stage.width * 0.5).abs() < 0.001);
        assert!((small.content.y + 20.0 - small.stage.y - small.stage.height * 0.5).abs() < 0.001);
        for (width, height) in [(4000, 1000), (1000, 4000)] {
            let geometry = ImageViewer::default()
                .intrinsic_size(width, height)
                .geometry(bounds(), nana_ui_core::UI_METRICS);
            assert!(geometry.content.width <= geometry.stage.width + 0.001);
            assert!(geometry.content.height <= geometry.stage.height + 0.001);
            assert!(
                (geometry.content.width / geometry.content.height - width as f32 / height as f32)
                    .abs()
                    < 0.001
            );
            assert!(
                (geometry.content.width - geometry.stage.width).abs() < 0.001
                    || (geometry.content.height - geometry.stage.height).abs() < 0.001
            );
        }
    }

    #[test]
    fn intrinsic_image_zoom_and_pan_use_fitted_dimensions() {
        let mut viewer = ImageViewer::default().intrinsic_size(72, 40);
        viewer.zoom = 2.0;
        viewer.offset = ImageViewerOffset::new(1000.0, -1000.0);
        let geometry = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        assert_eq!(
            (geometry.content.width, geometry.content.height),
            (144.0, 80.0)
        );
        assert!(
            (geometry.content.x + 72.0 - geometry.stage.x - geometry.stage.width * 0.5).abs()
                < 0.001
        );
        let (x, y) = stage_point(&geometry, 0.5, 0.5);
        viewer.pointer_down(&geometry, 4, x, y);
        viewer.pointer_move(&geometry, 4, x + 1000.0, y - 1000.0);
        assert_eq!(viewer.offset, ImageViewerOffset::ZERO);
        viewer.intrinsic_size = Some((4000, 1000));
        let large = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        viewer.pointer_down(&large, 5, x, y);
        viewer.pointer_move(&large, 5, x + 10000.0, y + 10000.0);
        let moved = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        assert!(viewer.offset.x > 0.0);
        assert!(moved.content.x <= moved.stage.x + moved.stage.width * (1.0 - COVERAGE) + 0.001);
        viewer.zoom = 1.0;
        let reset = viewer.geometry(bounds(), nana_ui_core::UI_METRICS);
        assert!(
            (reset.content.x + reset.content.width * 0.5 - reset.stage.x - reset.stage.width * 0.5)
                .abs()
                < 0.001
        );
    }
}
