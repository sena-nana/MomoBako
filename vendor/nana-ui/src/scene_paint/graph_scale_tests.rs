use super::*;

#[test]
fn graph_canvas_large_families_survive_gpu_updates_and_shrink() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let viewport = ScenePaintViewport {
        logical_size: [96.0, 1200.0],
        physical_size: [96, 1200],
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 96, 1200);
    for count in [300, 2, 300, 0] {
        let mut node = graph_canvas_stroke_node(1, Vec::new(), [0.0, 0.0, 1.0, 1.0]);
        node.layout.width = 96.0;
        node.layout.height = 1200.0;
        let Some(ComponentGeometry::GraphCanvas {
            nodes,
            ports,
            edges,
            ..
        }) = node.component_geometry.as_deref_mut()
        else {
            unreachable!()
        };
        for index in 0..count {
            let y = index as f32 * 4.0;
            edges.push((vec![[4.0, y + 2.5], [28.0, y + 2.5]], [1.0, 0.0, 0.0, 1.0]));
            let bounds = LayoutBox {
                x: 40.0,
                y,
                width: 16.0,
                height: 4.0,
            };
            nodes.push((
                bounds,
                nana_ui_runtime::ComponentTextRegion {
                    bounds,
                    content: "".into(),
                    color: None,
                    font_size: 13.0,
                    font_weight: None,
                },
                [0.0, 1.0, 0.0, 1.0],
                None,
            ));
            ports.push((
                LayoutBox {
                    x: 80.0,
                    y,
                    width: 8.0,
                    height: 4.0,
                },
                [1.0; 4],
                [1.0; 4],
                0.0,
            ));
        }
        scene.apply_delta([node], []);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("large graph families"),
        });
        painter
            .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &texture, 96, 1200);
        if let Some(directory) = std::env::var_os("NANA_GRAPH_SCALE_SNAPSHOTS") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            image::save_buffer(
                directory.join(format!("graph-{count}.png")),
                &pixels,
                96,
                1200,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }

        for index in 0..300 {
            let y = index * 4 + 2;
            let edge = pixel(&pixels, 96, 16, y);
            let body = pixel(&pixels, 96, 48, y);
            let port = pixel(&pixels, 96, 84, y);
            if index < count {
                assert!(
                    edge[0] > 180 && edge[2] < 80,
                    "edge {index}/{count}: {edge:?}"
                );
                assert!(
                    body[1] > 180 && body[2] < 80,
                    "node {index}/{count}: {body:?}"
                );
                assert!(
                    port[0] > 180 && port[1] > 180,
                    "port {index}/{count}: {port:?}"
                );
            } else {
                for color in [edge, body, port] {
                    assert!(
                        color[2] > 180 && color[0] < 80 && color[1] < 80,
                        "stale graph row {index}/{count}: {color:?}"
                    );
                }
            }
        }
    }
}
