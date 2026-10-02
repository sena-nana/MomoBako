use super::*;
use nana_ui_runtime::{KeyCaptureLayer, KeymapLayer, LayoutViewport, Stack};
use nana_ui_scene::RuntimeDocument;

#[test]
fn key_layer_badges_keep_text_above_their_backplates_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    for (theme_name, theme, clear) in [
        ("light", nana_ui_core::ThemeMode::Light, [1.0; 4]),
        (
            "dark",
            nana_ui_core::ThemeMode::Dark,
            [0.08, 0.08, 0.08, 1.0],
        ),
    ] {
        let id = DocumentId::new(921).unwrap();
        let mut document = RuntimeDocument::new(id);
        let context = document.context_mut();
        context.set_theme(theme).unwrap();
        let root = context
            .create_component(id, Stack::fill_column(12.0).padding(8.0))
            .unwrap();
        let capture = context
            .create_detached_component(id, KeyCaptureLayer::new())
            .unwrap();
        let keymap = context
            .create_detached_component(id, KeymapLayer::default())
            .unwrap();
        context.append_child(root, capture).unwrap();
        context.append_child(root, keymap).unwrap();
        for scale in [1, 2] {
            let width = 180 * scale;
            let height = 120 * scale;
            let (texture, view) = test_copy_target(&device, format, width, height);
            for (step, recording) in [false, true, false].into_iter().enumerate() {
                document
                    .context_mut()
                    .update_component(capture, |layer, _| layer.set_recording(recording))
                    .unwrap();
                document
                    .flush(
                        LayoutViewport::new(180.0, 120.0),
                        &mut crate::NanaTextShaper::default(),
                    )
                    .unwrap();
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                painter
                    .paint_encoder(
                        document.scene(),
                        &mut encoder,
                        &view,
                        ScenePaintViewport {
                            logical_size: [180.0, 120.0],
                            physical_size: [width, height],
                            scale_factor: scale as f32,
                            scene_origin: [0.0; 2],
                            target_origin: [0.0; 2],
                            clear_color: clear,
                            clear: true,
                        },
                        None,
                        None,
                    )
                    .unwrap();
                let pixels = readback_rgba(&device, &queue, encoder, &texture, width, height);
                if let Some(directory) = std::env::var_os("NANA_KEY_BADGE_SNAPSHOTS") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    image::save_buffer(
                        directory.join(format!("key-badges-{theme_name}-{scale}x-{step}.png")),
                        &pixels,
                        width,
                        height,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
                for node in [capture.stable_id(), keymap.stable_id()] {
                    let primitives = document
                        .scene()
                        .primitives()
                        .filter(|primitive| primitive.node == node)
                        .collect::<Vec<_>>();
                    let (text_index, text) = primitives
                        .iter()
                        .enumerate()
                        .find(|(_, primitive)| {
                            matches!(
                                &primitive.kind,
                                ScenePrimitiveKind::Text { content, .. } if !content.is_empty()
                            )
                        })
                        .expect("key layer must project a nonempty badge");
                    let bounds = text.bounds;
                    let reference = pixel(
                        &pixels,
                        width,
                        ((bounds.x + 4.0) * scale as f32) as u32,
                        ((bounds.y + 4.0) * scale as f32) as u32,
                    );
                    let mut ink = 0;
                    for y in ((bounds.y + 4.0) * scale as f32) as u32
                        ..((bounds.y + bounds.height - 4.0) * scale as f32) as u32
                    {
                        for x in ((bounds.x + 4.0) * scale as f32) as u32
                            ..((bounds.x + bounds.width - 4.0) * scale as f32) as u32
                        {
                            let color = pixel(&pixels, width, x, y);
                            if (0..3)
                                .any(|channel| color[channel].abs_diff(reference[channel]) > 18)
                            {
                                ink += 1;
                            }
                        }
                    }
                    assert!(
                        ink > 3,
                        "badge glyphs disappeared into the backplate: {theme_name}, {scale}x, recording={recording}, node={node:?}"
                    );
                    assert!(
                        primitives.iter().enumerate().all(|(index, primitive)| {
                            !matches!(
                                primitive.kind,
                                ScenePrimitiveKind::Quad {
                                    background: Some(_),
                                    ..
                                }
                            ) || index < text_index
                        }),
                        "every key badge backplate must paint before its glyphs"
                    );
                }
            }
        }
    }
}
