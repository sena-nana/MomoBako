//! image-viewer geometry from committed node data.

use super::*;

pub(in crate::world) fn image_viewer_geometry(
    bounds: LayoutBox,
    intrinsic_size: Option<(u32, u32)>,
    name: Option<&Arc<str>>,
    metadata: Option<&Arc<str>>,
    zoom: f32,
    offset_x: f32,
    offset_y: f32,
    palette: &SemanticPalette,
    scrim: nana_ui_core::SemanticColor,
    metrics: nana_ui_core::ThemeMetrics,
) -> crate::ComponentGeometry {
    let mut viewer = crate::ImageViewer::new(crate::ImageViewerContent::None);
    viewer.intrinsic_size = intrinsic_size;
    if let Some(name) = name {
        viewer = viewer.name(Arc::clone(name));
    }
    if let Some(metadata) = metadata {
        viewer = viewer.metadata(Arc::clone(metadata));
    }
    viewer.zoom = zoom;
    viewer.offset = crate::ImageViewerOffset::new(offset_x, offset_y);
    let geometry = viewer.geometry(bounds, metrics);
    let mut stage = palette.background.as_rgba_array();
    stage[3] = 0.34;
    crate::ComponentGeometry::ImageViewer {
        scrim: geometry.scrim,
        surface: geometry.surface,
        stage: geometry.stage,
        name: name
            .zip(geometry.name)
            .map(|(text, region)| crate::ComponentTextRegion {
                bounds: region,
                content: Arc::clone(text).into(),
                color: Some(palette.text.as_rgba_array()),
                font_size: nana_ui_core::type_scale::META,
                font_weight: Some(nana_ui_core::type_scale::SEMIBOLD),
            }),
        metadata: metadata.zip(geometry.metadata).map(|(text, region)| {
            crate::ComponentTextRegion {
                bounds: region,
                content: Arc::clone(text).into(),
                color: Some(palette.muted.as_rgba_array()),
                font_size: nana_ui_core::type_scale::HINT,
                font_weight: None,
            }
        }),
        content: geometry.content,
        scrim_color: scrim.as_rgba_array(),
        surface_color: palette.surface.as_rgba_array(),
        stage_color: stage,
    }
}
