#![allow(clippy::too_many_arguments)] // Flat extracted-node fixtures mirror the public projection fields.
use std::sync::Arc;

use nana_ui_core::{ButtonKind, LengthSpec, OverflowSpec, PaintTransform, SemanticColorRole};
use nana_ui_runtime::{
    AppContext, Avatar, Button as RuntimeButton, ComponentGeometry, ComputedStyle,
    CustomRenderNode, DocumentId, ExtractedNode, GpuTextureView, HoverCard, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, TextContent,
};
#[cfg(feature = "graph-canvas")]
use nana_ui_scene::StrokePattern;
use nana_ui_scene::{
    AffineTransform, ClipRegion, ScenePrimitiveKind, SceneRect, StrokeCap, UiScene,
};

use super::*;
use crate::HostTextureRegistry;

#[test]
fn scene_renderers_share_static_pipelines_across_painters() {
    let host = crate::test_gpu::context();
    let gpu = __framework::adopt(
        __framework::adapter(&host).clone(),
        __framework::device(&host).clone(),
        __framework::queue(&host).clone(),
    );
    let before = gpu.policy().stats();
    let first = SceneWgpuPainter::new(&gpu, nana_gpu::GpuTextureFormat::RGBA8_UNORM);
    let second = SceneWgpuPainter::new(&gpu, nana_gpu::GpuTextureFormat::RGBA8_UNORM);
    assert_eq!(
        first.quads.cached_pipeline(),
        second.quads.cached_pipeline()
    );
    assert_eq!(
        first.icons.cached_pipeline(),
        second.icons.cached_pipeline()
    );
    assert_eq!(
        first.meshes.cached_pipeline(),
        second.meshes.cached_pipeline()
    );
    let after = gpu.policy().stats();
    assert!(after.pipeline_registry_hits > before.pipeline_registry_hits);
    assert!(after.pipeline_registry_misses > before.pipeline_registry_misses);
}

#[derive(Debug)]
struct FillClipRenderer {
    pipeline: wgpu::RenderPipeline,
}

impl FillClipRenderer {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.test.fill.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                r#"
                    @vertex
                    fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
                        var positions = array<vec2<f32>, 3>(
                            vec2<f32>(-1.0, -1.0),
                            vec2<f32>(3.0, -1.0),
                            vec2<f32>(-1.0, 3.0),
                        );
                        return vec4<f32>(positions[index], 0.0, 1.0);
                    }

                    @fragment
                    fn fs_main() -> @location(0) vec4<f32> {
                        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
                    }
                    "#,
            )),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.test.fill.pipeline"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nana-ui.test.fill.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }
}

impl SceneGpuRenderer for FillClipRenderer {
    fn prepare(&self, _node: &SceneGpuNode, _context: SceneGpuPrepareContext<'_>) {}

    fn render(&self, node: &SceneGpuNode, mut context: SceneGpuRenderContext<'_>) {
        if context.bounds.width == 0 || context.bounds.height == 0 {
            return;
        }
        let pass_context = SceneGpuPassContext {
            gpu: context.gpu,
            target_format: context.target_format,
            bounds: context.bounds,
            clip: context.clip,
            dest_size: context.dest_size,
            gpu_work: context.gpu_work,
        };
        context.with_pass("nana-ui.test.fill", |pass| {
            self.draw_in_pass(node, pass, pass_context);
        });
    }

    fn draw_in_pass(
        &self,
        _node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        context: SceneGpuPassContext<'_>,
    ) -> bool {
        if context.clip.width == 0 || context.clip.height == 0 {
            return false;
        }
        pass.set_scissor(context.clip);
        let pass = pass.raw();
        pass.set_pipeline(&self.pipeline);
        pass.draw(0..3, 0..1);
        true
    }
}

#[test]
fn empty_scene_validates() {
    let resolved = validate_scene(
        &UiScene::new(),
        None,
        None,
        crate::test_gpu::context().generation(),
    )
    .unwrap();
    assert!(resolved.resources.is_empty());
    assert!(resolved.renderers.is_empty());
    assert!(resolved.cacheable);
}

#[test]
fn validate_scene_rejects_unregistered_host_texture() {
    let mut context = AppContext::new();
    let button = context
        .create_component(DocumentId::new(1).unwrap(), RuntimeButton::new("Preview"))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        },
    );
    layout.set_custom_render(
        button.stable_id(),
        Some(CustomRenderNode::new("nana.host-texture", "preview", 1)),
    );
    context.commit_mutations(layout).unwrap();
    let work = context.take_system_work();
    context.resolve_styles(&work.style).unwrap();
    let mut scene = UiScene::new();
    scene.apply_delta(
        context.world().extract_nodes(&work.render_extraction),
        work.render_removals,
    );
    assert!(matches!(
        validate_scene(&scene, None, None, crate::test_gpu::context().generation()),
        Err(ScenePaintError::CustomPrimitive(_))
    ));
    assert!(matches!(
        validate_scene(
            &scene,
            Some(&HostTextureRegistry::new()),
            None,
            crate::test_gpu::context().generation()
        ),
        Err(ScenePaintError::MissingCustomResource(_))
    ));
}

#[test]
fn paint_records_gpu_work_only_after_encode_and_submit() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    assert!(painter.last_gpu_work().is_none());

    let skipped = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [0, 0],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let target = test_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui gpu work skip"),
    });
    painter
        .paint_encoder(&UiScene::new(), &mut encoder, &target, skipped, None, None)
        .unwrap();
    assert!(painter.last_gpu_work().is_none());
    assert!(painter.last_gpu_timings().is_none());

    let (scene, registry) = hosted_preview_scene(&device);
    let policy_uploads_before = crate::test_gpu::context().policy().stats().upload_bytes;
    let viewport = ScenePaintViewport {
        logical_size: [128.0, 96.0],
        physical_size: [128, 96],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.1, 0.1, 0.1, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui gpu work encode"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    let observed = painter
        .last_gpu_work()
        .expect("encoded frame records GPU work");
    assert!(
        observed.gpu_upload_bytes > 0,
        "host texture / quad write_buffer must be counted"
    );
    assert!(observed.draw_calls > 0);
    assert!(observed.draw_batches > 0);
    assert!(
        crate::test_gpu::context().policy().stats().upload_bytes > policy_uploads_before,
        "scene uploads must be visible through the shared GPU policy"
    );
    let timings = painter
        .last_gpu_timings()
        .expect("encoded frame times GPU stages");
    assert!(timings.submit.is_zero());
    queue.submit([encoder.finish()]);
    painter.record_submit(
        &crate::test_gpu::context()
            .begin_frame("record submit")
            .submit(),
    );
    let submitted = painter.last_gpu_timings().unwrap();
    assert!(
        !submitted.encode.is_zero()
            || !submitted.gpu_upload.is_zero()
            || !submitted.batch.is_zero()
    );
    assert_eq!(
        painter.last_gpu_work().unwrap().gpu_upload_bytes,
        observed.gpu_upload_bytes
    );
}

#[test]
fn frame_uploads_paint_the_same_pixels_as_direct_queue_writes() {
    let (device, queue) = test_device();
    let gpu = crate::test_gpu::context();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let scene = labeled_selected_button_scene();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let read = |texture: &wgpu::Texture| {
        readback_rgba(
            &device,
            &queue,
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame upload readback"),
            }),
            texture,
            64,
            64,
        )
    };

    let (direct_texture, direct_view) = test_copy_target(&device, format, 64, 64);
    let mut direct = SceneWgpuPainter::for_test(format);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("direct writes"),
    });
    direct
        .paint_encoder(&scene, &mut encoder, &direct_view, viewport, None, None)
        .unwrap();
    queue.submit([encoder.finish()]);
    let expected = read(&direct_texture);

    let (framed_texture, framed_view) = test_copy_target(&device, format, 64, 64);
    let target = __framework::render_target(&gpu, framed_view, format, [64, 64]);
    let mut framed = SceneWgpuPainter::for_test(format);
    // Twice: the second frame writes only what changed, through the same
    // recorder, and must still match.
    for _ in 0..2 {
        let before = gpu.policy().stats();
        let mut frame = gpu.begin_frame("framed paint");
        framed
            .paint(&scene.clone(), &mut frame, &target, viewport, None, None)
            .unwrap();
        frame.submit();
        let after = gpu.policy().stats();
        assert!(after.upload_flushes - before.upload_flushes <= 1);
        assert_eq!(read(&framed_texture), expected);
    }
}

#[test]
fn text_shape_cache_hits_on_repaint_with_identical_pixels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = labeled_selected_button_scene();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);

    let paint_once = |painter: &mut SceneWgpuPainter, scene: &UiScene| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui shape cache test"),
        });
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        queue.submit([encoder.finish()]);
        readback_rgba(
            &device,
            &queue,
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nana-ui shape cache readback"),
            }),
            &texture,
            64,
            64,
        )
    };

    let first = paint_once(&mut painter, &scene);
    let (hits_after_first, misses_after_first, _) = painter.text_shape_cache_stats();
    assert!(
        misses_after_first > 0,
        "labeled button scene contains text that must shape once"
    );
    assert_eq!(hits_after_first, 0);

    // A clone carries the same text under a fresh scene instance, so the
    // frame is rebatched and the shaping cache is the only thing that can
    // save the work.
    let second = paint_once(&mut painter, &scene.clone());
    let (hits_after_second, misses_after_second, _) = painter.text_shape_cache_stats();
    assert_eq!(
        misses_after_second, misses_after_first,
        "repaint of an unchanged scene must not reshape any paragraph"
    );
    assert!(
        hits_after_second >= misses_after_first,
        "every paragraph from the first frame must be a cache hit on repaint"
    );
    assert_eq!(
        first, second,
        "cached shaping must produce identical pixels"
    );
}

#[test]
fn repaint_of_an_unchanged_scene_reblits_dest_without_rebatching() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = labeled_selected_button_scene();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);

    let paint_once = |painter: &mut SceneWgpuPainter| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui unchanged repaint"),
        });
        painter
            .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        queue.submit([encoder.finish()]);
        readback_rgba(
            &device,
            &queue,
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nana-ui unchanged repaint readback"),
            }),
            &texture,
            64,
            64,
        )
    };

    let first = paint_once(&mut painter);
    let encoded = painter
        .last_dest_pass_counts
        .expect("first frame encodes the scene");
    assert!(
        encoded.msaa > 0 && encoded.blit == 1,
        "first frame must paint the scene into dest, got {encoded:?}"
    );
    assert!(
        painter.last_gpu_work().unwrap().draw_calls > 0,
        "first frame must issue draws"
    );

    let second = paint_once(&mut painter);
    let reused = painter
        .last_dest_pass_counts
        .expect("repaint still encodes the blit");
    assert_eq!(
        (reused.msaa, reused.color, reused.blit),
        (0, 0, 1),
        "unchanged scene and viewport must only re-blit dest, got {reused:?}"
    );
    let work = painter.last_gpu_work().unwrap();
    assert_eq!(
        work.gpu_upload_bytes, 0,
        "reused frame must not re-upload quads or meshes"
    );
    assert_eq!(
        first, second,
        "re-blitting dest must produce the same pixels as encoding it"
    );

    // A scene change gives up the reuse and paints again.
    let mut changed = scene.clone();
    changed.apply_delta(
        [colored_quad_node(
            99,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 1.0, 0.0, 1.0],
        )],
        [],
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui changed repaint"),
    });
    painter
        .paint_encoder(&changed, &mut encoder, &view, viewport, None, None)
        .unwrap();
    queue.submit([encoder.finish()]);
    let repainted = painter
        .last_dest_pass_counts
        .expect("changed scene encodes again");
    assert!(
        repainted.msaa + repainted.color > 0,
        "a changed scene must give up the reuse and repaint dest, got {repainted:?}"
    );
    let pixels = readback_rgba(
        &device,
        &queue,
        device.create_command_encoder(&Default::default()),
        &texture,
        64,
        64,
    );
    assert!(
        is_green_slot(pixel(&pixels, 64, 32, 32)),
        "the new panel covers the earlier label in document order"
    );
}

#[test]
fn paint_draws_node_inserted_by_in_place_apply_delta() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [colored_quad_node(
            1,
            0.0,
            0.0,
            32.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
        )],
        [],
    );
    let first_instance = scene.instance_id();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);

    let paint = |painter: &mut SceneWgpuPainter, scene: &UiScene| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui in-place delta paint"),
        });
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        readback_rgba(&device, &queue, encoder, &texture, 64, 64)
    };

    let first = paint(&mut painter, &scene);
    let left = pixel(&first, 64, 8, 32);
    let right = pixel(&first, 64, 48, 32);
    assert!(
        is_red_slot(left),
        "first paint must draw the existing quad, got {left:?}"
    );
    assert!(
        right[0] < 40 && right[1] < 40 && right[2] < 40,
        "right half must stay clear before the inserted node, got {right:?}"
    );

    scene.apply_delta(
        [colored_quad_node(
            2,
            32.0,
            0.0,
            32.0,
            64.0,
            [0.0, 1.0, 0.0, 1.0],
        )],
        [],
    );
    assert_ne!(
        scene.instance_id(),
        first_instance,
        "in-place apply_delta must refresh instance so the painter cannot reuse the stale op stream"
    );

    let second = paint(&mut painter, &scene);
    let left = pixel(&second, 64, 8, 32);
    let right = pixel(&second, 64, 48, 32);
    assert!(
        is_red_slot(left),
        "existing quad must still draw after the in-place insert, got {left:?}"
    );
    assert!(
        is_green_slot(right),
        "new primitive must be encoded after in-place apply_delta, got {right:?}"
    );
}

#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_node(
    value: u64,
    edges: Vec<(Vec<[f32; 2]>, [f32; 4])>,
    background: [f32; 4],
) -> ExtractedNode {
    let mut node = extracted_div(
        value,
        &[],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle::default(),
        Some(background),
    );
    node.standard_visual = Some(StandardVisual::GraphCanvas {
        nodes: Arc::from([]),
        ports: Arc::from([]),
        edges: Arc::from([]),
        connecting: None,
        grid_spacing: 24.0,
        viewport_offset_x: 0.0,
        viewport_offset_y: 0.0,
        viewport_zoom: 1.0,
    });
    node.component_geometry = Some(Box::new(ComponentGeometry::GraphCanvas {
        nodes: Vec::new(),
        separators: Vec::new(),
        ports: Vec::new(),
        port_labels: Vec::new(),
        edges,
        edge_labels: Vec::new(),
        grid: Vec::new(),
        background,
        grid_color: [0.0, 0.0, 0.0, 0.0],
        separator_color: [0.0, 0.0, 0.0, 0.0],
    }));
    node
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_paints_capsule_coverage_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let background = [0.0, 0.0, 1.0, 1.0];
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(
                vec![[8.0, 32.0], [56.0, 32.0], [56.0, 12.0]],
                [1.0, 0.0, 0.0, 1.0],
            )],
            background,
        )],
        [],
    );
    assert!(
        scene
            .primitives()
            .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. })),
        "GraphCanvas edges must extract as Stroke, not TimeSeriesChart quads"
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui articulated stroke"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    // The 1.6px line is centred on a pixel boundary: each row it straddles
    // is 0.8 covered.
    let midline = pixel(&pixels, 64, 32, 32);
    assert!(
        (195..=213).contains(&midline[0]),
        "horizontal capsule midline must ink 0.8 of the row, got {midline:?}"
    );
    let join = pixel(&pixels, 64, 56, 32);
    assert!(
        join[0] > 120,
        "articulated join must keep the shared endpoint disc, got {join:?}"
    );
    let far = pixel(&pixels, 64, 32, 8);
    assert!(
        is_blue_slot(far),
        "pixels outside the 1.6px capsule must stay GraphCanvas fill, got {far:?}"
    );
    let covering_corner = pixel(&pixels, 64, 32, 28);
    assert!(
        covering_corner[2] > 120 && covering_corner[0] < 80,
        "covering-quad corners 4px off the 0.8px radius must be discarded, got {covering_corner:?}"
    );
    drop(texture);
}

#[test]
#[cfg(feature = "graph-canvas")]
fn a_thin_stroke_ramps_over_one_device_pixel_at_any_phase() {
    // A 1.6px line (radius 0.8) on both parities of pixel row and between,
    // and on a diagonal. A ramp taken from `dpdx` of the distance collapses
    // where both sides of the centre line share a 2x2 quad, and a smoothstep
    // one inks more than the line covers; every pixel near it must be the
    // one device pixel linear ramp over its distance to the line.
    let (device, queue) = test_device();
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let lines = [
        [[8.0, 32.0], [56.0, 32.0]],
        [[8.0, 33.0], [56.0, 33.0]],
        [[8.0, 32.25], [56.0, 32.25]],
        [[8.0, 33.25], [56.0, 33.25]],
        [[8.0, 32.5], [56.0, 32.5]],
        [
            [32.0 - 24.0 * cos, 32.0 - 24.0 * sin],
            [32.0 + 24.0 * cos, 32.0 + 24.0 * sin],
        ],
    ];
    for [a, b] in lines {
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let mut scene = UiScene::new();
        scene.apply_delta(
            [graph_canvas_stroke_node(
                1,
                vec![(vec![a, b], [1.0, 0.0, 0.0, 1.0])],
                [0.0, 0.0, 0.0, 1.0],
            )],
            [],
        );
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0, 64.0],
            [64, 64],
            1.0,
        );
        let [dx, dy] = [b[0] - a[0], b[1] - a[1]];
        let length = dx.hypot(dy);
        let [tx, ty] = [dx / length, dy / length];
        for py in 0..64 {
            for px in 0..64 {
                let [vx, vy] = [px as f32 + 0.5 - a[0], py as f32 + 0.5 - a[1]];
                let along = vx * tx + vy * ty;
                // Away from the round ends, where the line is its side.
                if along < 4.0 || along > length - 4.0 {
                    continue;
                }
                let outside = (vx * ty - vy * tx).abs() - 0.8;
                if outside.abs() > 1.2 {
                    continue;
                }
                let expected = (0.5 - outside).clamp(0.0, 1.0) * 255.0;
                let red = f32::from(pixel(&pixels, 64, px, py)[0]);
                assert!(
                    (red - expected).abs() <= 8.0,
                    "line {a:?}-{b:?}: ({px},{py}) is {outside:+.3} px from its edge, \
                     expected red {expected:.0}, got {red}"
                );
            }
        }
    }
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_antialiases_silhouette_on_msaa_dest() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[16.0, 32.0], [48.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let (texture, view) = test_copy_target(&device, format, 256, 256);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [256, 256],
        scale_factor: 4.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui stroke msaa coverage"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert_eq!(
        counts.msaa, 1,
        "stroke-only frames must render through the 4x MSAA dest, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 256, 256);
    drop(texture);
    // A column crossing the 1.6px capsule at 4× scale must pass through
    // intermediate stroke/fill blends. WebGPU evaluates the fragment at the
    // pixel center and the hull is padded past the silhouette, so MSAA
    // itself cannot provide that blend — only analytic coverage can.
    let mut soft = 0;
    for y in 0..256u32 {
        let color = pixel(&pixels, 256, 128, y);
        let stroke_share = f32::from(color[0]) / 255.0;
        if stroke_share > 0.1 && stroke_share < 0.9 {
            soft += 1;
        }
    }
    assert!(
        soft >= 2,
        "MSAA-path stroke silhouette must blend into the fill on both edges, got {soft} soft pixels"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_paints_round_end_caps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[16.0, 32.0], [48.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [256, 256],
        4.0,
    );
    let midline = pixel(&pixels, 256, 128, 128);
    assert!(
        is_red_slot(midline),
        "4× physical midline must ink the 1.6px capsule, got {midline:?}"
    );
    let start_cap = pixel(&pixels, 256, 62, 128);
    assert!(
        start_cap[0] > 120,
        "round cap 0.5 logical px before the start must stay inside the disc, got {start_cap:?}"
    );
    let end_cap = pixel(&pixels, 256, 194, 128);
    assert!(
        end_cap[0] > 120,
        "round cap 0.5 logical px past the end must stay inside the disc, got {end_cap:?}"
    );
    let beyond_cap = pixel(&pixels, 256, 48, 128);
    assert!(
        is_blue_slot(beyond_cap),
        "4 logical px before the start must stay GraphCanvas fill, got {beyond_cap:?}"
    );
    let far_normal = pixel(&pixels, 256, 128, 112);
    assert!(
        is_blue_slot(far_normal),
        "4 logical px off the 0.8px radius must stay fill, got {far_normal:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn tapered_stroke_paints_uneven_capsule_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[8.0, 32.0], [56.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    assert!(scene.replace_primitive_kind(
        primitive.id,
        ScenePrimitiveKind::Stroke {
            points: vec![[8.0, 32.0], [56.0, 32.0]],
            width: 2.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: vec![12.0, 4.0],
            cap: StrokeCap::Round,
            pattern: None,
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let thick_end = pixel(&pixels, 64, 8, 32);
    assert!(
        is_red_slot(thick_end),
        "6px radius at the start must ink, got {thick_end:?}"
    );
    let thick_side = pixel(&pixels, 64, 8, 28);
    assert!(
        thick_side[0] > 120,
        "4px off the start must stay inside the 6px disc, got {thick_side:?}"
    );
    let thin_side = pixel(&pixels, 64, 56, 28);
    assert!(
        is_blue_slot(thin_side),
        "4px off the 2px end must stay fill, got {thin_side:?}"
    );
    let thin_mid = pixel(&pixels, 64, 56, 32);
    assert!(
        thin_mid[0] > 120,
        "2px radius end midline must ink, got {thin_mid:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn non_uniform_affine_stroke_covers_stretched_ellipse_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let mut canvas = graph_canvas_stroke_node(
        1,
        vec![(vec![[24.0, 32.0], [40.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
        [0.0, 0.0, 1.0, 1.0],
    );
    // scale-x 2, scale-y 0.5 about the 64×64 node center. Local disc r=4
    // becomes an ellipse 8 × 2, not a min-axis disc of 2. Affine reaches
    // push_stroke via ScenePrimitive.transform (same path as Quad/Mesh).
    Arc::make_mut(&mut canvas.source_style.layout).transform = Some(PaintTransform {
        a: 2.0,
        b: 0.0,
        c: 0.0,
        d: 0.5,
        e: 0.0,
        f: 0.0,
    });
    scene.apply_delta([canvas], []);
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    let stroke_id = primitive.id;
    let [a, b, c, d, ..] = primitive.transform.0;
    assert!(
        (a - 2.0).abs() < 1e-5 && b.abs() < 1e-5 && c.abs() < 1e-5 && (d - 0.5).abs() < 1e-5,
        "stroke must keep the non-uniform node affine, got {:?}",
        primitive.transform.0
    );
    assert!(scene.replace_primitive_kind(
        stroke_id,
        ScenePrimitiveKind::Stroke {
            points: vec![[24.0, 32.0], [40.0, 32.0]],
            width: 8.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: Vec::new(),
            cap: StrokeCap::Round,
            pattern: None,
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    // World start is (16, 32). 4px left sits outside the old min-axis
    // disc (radius 2) and inside the true ellipse (rx 8).
    let stretched = pixel(&pixels, 64, 12, 32);
    assert!(
        is_red_slot(stretched),
        "stretched-axis sample must ink the ellipse, got {stretched:?}"
    );
    let outside = pixel(&pixels, 64, 4, 32);
    assert!(
        is_blue_slot(outside),
        "12px left of the start must stay GraphCanvas fill, got {outside:?}"
    );
    let squashed_ink = pixel(&pixels, 64, 16, 33);
    assert!(
        squashed_ink[0] > 120,
        "1px off the start along the squashed axis must ink the ellipse, got {squashed_ink:?}"
    );
    let squashed_out = pixel(&pixels, 64, 16, 36);
    assert!(
        is_blue_slot(squashed_out),
        "4px off the start along the squashed axis must stay fill, got {squashed_out:?}"
    );
    // AA-fringe evenness along the rim is not asserted (fwidth thresholds
    // are flaky). The stretched/squashed ink+outside pair is the covering
    // check that the 1/σ_min pad reached the GPU.
}

#[test]
#[cfg(feature = "graph-canvas")]
fn butt_stroke_cuts_round_end_caps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[16.0, 32.0], [48.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    assert!(scene.replace_primitive_kind(
        primitive.id,
        ScenePrimitiveKind::Stroke {
            points: vec![[16.0, 32.0], [48.0, 32.0]],
            width: 8.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: Vec::new(),
            cap: StrokeCap::Butt,
            pattern: None,
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [256, 256],
        4.0,
    );
    let midline = pixel(&pixels, 256, 128, 128);
    assert!(
        is_red_slot(midline),
        "butt midline must ink the 8px stroke, got {midline:?}"
    );
    let past_start = pixel(&pixels, 256, 58, 128);
    assert!(
        is_blue_slot(past_start),
        "1.5 logical px before a butt start must stay fill, got {past_start:?}"
    );
    let start_edge = pixel(&pixels, 256, 64, 128);
    assert!(
        start_edge[0] > 80,
        "the butt start cut must still ink on the endpoint, got {start_edge:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn square_stroke_extends_flat_caps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[16.0, 32.0], [48.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    assert!(scene.replace_primitive_kind(
        primitive.id,
        ScenePrimitiveKind::Stroke {
            points: vec![[16.0, 32.0], [48.0, 32.0]],
            width: 8.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: Vec::new(),
            cap: StrokeCap::Square,
            pattern: None,
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [256, 256],
        4.0,
    );
    let outside_round = pixel(&pixels, 256, 52, 116);
    assert!(
        is_red_slot(outside_round),
        "square cap must ink the half-width box past the start, got {outside_round:?}"
    );
    let past_square = pixel(&pixels, 256, 44, 128);
    assert!(
        is_blue_slot(past_square),
        "beyond the square extension must stay fill, got {past_square:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn dashed_stroke_skips_gaps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[8.0, 32.0], [56.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    assert!(scene.replace_primitive_kind(
        primitive.id,
        ScenePrimitiveKind::Stroke {
            points: vec![[8.0, 32.0], [56.0, 32.0]],
            width: 8.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: Vec::new(),
            cap: StrokeCap::Butt,
            pattern: Some(Box::new(StrokePattern {
                dash: vec![8.0, 8.0],
                dash_offset: 0.0,
                path_length: 0.0,
                colors: Vec::new(),
            })),
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let dash = pixel(&pixels, 64, 12, 32);
    assert!(
        is_red_slot(dash),
        "first 8px on-dash must ink, got {dash:?}"
    );
    let gap = pixel(&pixels, 64, 20, 32);
    assert!(
        is_blue_slot(gap),
        "8px off-dash gap must stay fill, got {gap:?}"
    );
    let next = pixel(&pixels, 64, 28, 32);
    assert!(is_red_slot(next), "second on-dash must ink, got {next:?}");
}

#[test]
fn sdf_dashed_border_skips_gaps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            0.0,
            0.0,
            80.0,
            80.0,
            nana_ui_core::LayoutStyle {
                background: Some([1.0, 1.0, 1.0, 1.0]),
                border_width: Some(4.0),
                border_color: Some([1.0, 0.0, 0.0, 1.0]),
                border_style: Some(nana_ui_core::BorderStyle::Dashed),
                ..nana_ui_core::LayoutStyle::default()
            },
            Some([1.0, 1.0, 1.0, 1.0]),
        )],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [80.0, 80.0],
        [80, 80],
        1.0,
    );
    // w=4 → dash 12, gap 8, period 20. Sample the top ring, away from corners.
    let dash = pixel(&pixels, 80, 10, 2);
    assert!(
        is_red_slot(dash),
        "dashed on-segment must stroke, got {dash:?}"
    );
    let gap = pixel(&pixels, 80, 16, 2);
    assert!(
        gap[0] > 200 && gap[1] > 200 && gap[2] > 200,
        "dashed gap must show fill, got {gap:?}"
    );
}

#[test]
fn sdf_dotted_border_skips_gaps_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            0.0,
            0.0,
            80.0,
            80.0,
            nana_ui_core::LayoutStyle {
                background: Some([1.0, 1.0, 1.0, 1.0]),
                border_width: Some(4.0),
                border_color: Some([1.0, 0.0, 0.0, 1.0]),
                border_style: Some(nana_ui_core::BorderStyle::Dotted),
                ..nana_ui_core::LayoutStyle::default()
            },
            Some([1.0, 1.0, 1.0, 1.0]),
        )],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [80.0, 80.0],
        [80, 80],
        1.0,
    );
    // w=4 → round dots period 8, radius 2, on the top centerline.
    let on = pixel(&pixels, 80, 40, 2);
    assert!(is_red_slot(on), "dotted center must stroke, got {on:?}");
    let off = pixel(&pixels, 80, 44, 2);
    assert!(
        off[0] > 200 && off[1] > 200 && off[2] > 200,
        "dotted gap must show fill, got {off:?}"
    );
}

#[test]
fn dash_ends_and_dot_rims_stay_one_device_pixel_under_transform_scale() {
    // An 80px box with a 4px red border on white, scaled 0.6 about its centre:
    // local x lands at device 16 + 0.6x, and device row 17 is local y 2.5,
    // mid-border on the top side.
    let scale = 0.6;
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    for style in [
        nana_ui_core::BorderStyle::Dashed,
        nana_ui_core::BorderStyle::Dotted,
    ] {
        let white = [1.0, 1.0, 1.0, 1.0];
        let layout = nana_ui_core::LayoutStyle {
            background: Some(white),
            border_width: Some(4.0),
            border_color: Some([1.0, 0.0, 0.0, 1.0]),
            border_style: Some(style),
            transform: scale_transform(scale),
            ..Default::default()
        };
        let node = extracted_div(1, &[], 0.0, 0.0, 80.0, 80.0, layout, Some(white));
        let mut scene = UiScene::new();
        scene.apply_delta([node], []);
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [80.0, 80.0],
            [80, 80],
            1.0,
        );
        for x in 22..58 {
            let along = (x as f32 + 0.5 - 16.0) / scale;
            // The shader's signed distance, in local px: dashes 12 on 8 off,
            // dots of radius 2 every 8, the top centreline at local y 2.
            let local = if style == nana_ui_core::BorderStyle::Dashed {
                let t = along.rem_euclid(20.0);
                if t < 12.0 {
                    -t.min(12.0 - t)
                } else {
                    (t - 12.0).min(20.0 - t)
                }
            } else {
                let nearest = (along / 8.0).round() * 8.0;
                (along - nearest).hypot(0.5) - 2.0
            };
            let cover = (0.5 - local * scale).clamp(0.0, 1.0);
            let expected = (1.0 - cover) * 255.0;
            let green = f32::from(pixel(&pixels, 80, x, 17)[1]);
            assert!(
                (green - expected).abs() <= 16.0,
                "{style:?}: ({x},17) is {:+.3} device px from the edge, \
                 expected green {expected:.0}, got {green}",
                local * scale
            );
        }
    }
}

#[test]
#[cfg(feature = "graph-canvas")]
fn per_point_stroke_colors_paint_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(
                vec![[8.0, 32.0], [32.0, 32.0], [56.0, 32.0]],
                [1.0, 0.0, 0.0, 1.0],
            )],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. }))
        .expect("extracted stroke");
    assert!(scene.replace_primitive_kind(
        primitive.id,
        ScenePrimitiveKind::Stroke {
            points: vec![[8.0, 32.0], [32.0, 32.0], [56.0, 32.0]],
            width: 8.0,
            color: [1.0, 0.0, 0.0, 1.0],
            widths: Vec::new(),
            cap: StrokeCap::Butt,
            pattern: Some(Box::new(StrokePattern {
                dash: Vec::new(),
                dash_offset: 0.0,
                path_length: 0.0,
                colors: vec![
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                ],
            })),
        },
    ));
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let red = pixel(&pixels, 64, 16, 32);
    assert!(
        is_red_slot(red),
        "first segment midpoint color must stay red, got {red:?}"
    );
    let green = pixel(&pixels, 64, 48, 32);
    assert!(
        is_green_slot(green),
        "second segment must use the green start color, got {green:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_diagonal_stroke_paints_capsule_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(
            1,
            vec![(vec![[8.0, 8.0], [56.0, 56.0]], [1.0, 0.0, 0.0, 1.0])],
            [0.0, 0.0, 1.0, 1.0],
        )],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let midline = pixel(&pixels, 64, 32, 32);
    assert!(
        is_red_slot(midline),
        "diagonal capsule midline must ink, got {midline:?}"
    );
    let far = pixel(&pixels, 64, 8, 56);
    assert!(
        is_blue_slot(far),
        "pixels far from the diagonal must stay GraphCanvas fill, got {far:?}"
    );
    let off_normal = pixel(&pixels, 64, 32, 28);
    assert!(
        off_normal[2] > 120 && off_normal[0] < 80,
        "4px off the diagonal must discard covering-quad corners, got {off_normal:?}"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_respects_ancestor_overflow_clip_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let mut canvas = graph_canvas_stroke_node(
        3,
        vec![(vec![[8.0, 32.0], [56.0, 32.0]], [1.0, 0.0, 0.0, 1.0])],
        [0.0, 1.0, 0.0, 1.0],
    );
    canvas.parent = Some(StableNodeId::new(2).unwrap());
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]),
            overflow_parent(2, &[3], 20.0, 20.0, 24.0, 24.0, None),
            canvas,
        ],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    // A 1.6px line centred on a pixel boundary covers 0.8 of the row.
    let inside_stroke = pixel(&pixels, 64, 32, 32);
    assert!(
        (195..=213).contains(&inside_stroke[0]),
        "stroke inside the overflow clip must ink, got {inside_stroke:?}"
    );
    let inside_fill = pixel(&pixels, 64, 32, 26);
    assert!(
        is_green_slot(inside_fill),
        "GraphCanvas fill inside the clip and off the 1.6px stroke must stay green, got {inside_fill:?}"
    );
    let leaked_start = pixel(&pixels, 64, 8, 32);
    assert!(
        is_blue_slot(leaked_start),
        "stroke past the ancestor clip must not ink, got {leaked_start:?}"
    );
    let leaked_end = pixel(&pixels, 64, 56, 32);
    assert!(
        is_blue_slot(leaked_end),
        "stroke past the far clip edge must not ink, got {leaked_end:?}"
    );
}

#[test]
fn spinner_ticks_paint_capsule_coverage_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let mut spinner = extracted_div(
        2,
        &[],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle::default(),
        None,
    );
    spinner.standard_visual = Some(StandardVisual::Spinner {
        label: Arc::from(""),
        size: 24.0,
        phase: 0.0,
    });
    spinner.standard_visual_foreground = Some([1.0, 0.0, 0.0, 1.0]);
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]),
            spinner,
        ],
        [],
    );
    assert!(
        scene
            .primitives()
            .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Spinner { .. })),
        "Spinner visual must extract as Spinner capsules"
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let tick = pixel(&pixels, 64, 40, 32);
    assert!(
        tick[0] > 120,
        "phase-0 east tick midline (40,32) must ink, got {tick:?}"
    );
    let hub = pixel(&pixels, 64, 32, 32);
    assert!(
        is_blue_slot(hub),
        "spinner hub is inside the ring, not a tick, got {hub:?}"
    );
    let far = pixel(&pixels, 64, 8, 16);
    assert!(
        is_blue_slot(far),
        "pixels outside the centered 24px spinner must stay the sibling fill, got {far:?}"
    );
}

#[test]
#[cfg(feature = "charts")]
fn time_series_line_paints_capsule_coverage_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]),
            time_series_stroke_node(
                2,
                vec![[8.0, 32.0], [56.0, 32.0], [56.0, 12.0]],
                [1.0, 0.0, 0.0, 1.0],
            ),
        ],
        [],
    );
    assert!(
        scene
            .primitives()
            .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Stroke { .. })),
        "TimeSeriesChart line must extract as Stroke, not tiled QuadBatch"
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let midline = pixel(&pixels, 64, 32, 32);
    assert!(
        is_red_slot(midline),
        "chart line midline must ink the 2px capsule, got {midline:?}"
    );
    let join = pixel(&pixels, 64, 56, 32);
    assert!(
        join[0] > 120,
        "articulated join must keep the shared endpoint disc, got {join:?}"
    );
    let far = pixel(&pixels, 64, 32, 8);
    assert!(
        is_blue_slot(far),
        "pixels outside the 2px capsule must stay the sibling fill, got {far:?}"
    );
    let covering_corner = pixel(&pixels, 64, 32, 28);
    assert!(
        covering_corner[2] > 120 && covering_corner[0] < 80,
        "covering-quad corners 4px off the 1px radius must be discarded, got {covering_corner:?}"
    );
}

#[cfg(feature = "charts")]
fn time_series_stroke_node(value: u64, points: Vec<[f32; 2]>, color: [f32; 4]) -> ExtractedNode {
    let mut node = extracted_div(
        value,
        &[],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle::default(),
        None,
    );
    node.standard_visual = Some(StandardVisual::TimeSeriesChart {
        values: Arc::from([0.0, 1.0]),
    });
    node.component_geometry = Some(Box::new(ComponentGeometry::TimeSeriesChart {
        grid: Vec::new(),
        area: Vec::new(),
        line: points,
        grid_color: [0.0, 0.0, 0.0, 0.0],
        area_color: [0.0, 0.0, 0.0, 0.0],
        line_color: color,
    }));
    node
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_gpu_upload_scales_with_segment_count() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    // Compare cold uploads. Reusing the previous scene's buffers measures only
    // changed byte ranges, so subtracting a cold fill baseline is not valid.
    let cold_work = |scene: UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        encode_scene_gpu_work(&device, &queue, &mut painter, &scene)
    };
    let fill = cold_work(graph_canvas_scene(Vec::new()));
    let work_16 = cold_work(graph_canvas_scene(l_stroke_edges(16)));
    let work_32 = cold_work(graph_canvas_scene(l_stroke_edges(32)));
    let work_64 = cold_work(graph_canvas_scene(l_stroke_edges(64)));
    let mesh_16 = work_16
        .gpu_upload_bytes
        .saturating_sub(fill.gpu_upload_bytes);
    let mesh_32 = work_32
        .gpu_upload_bytes
        .saturating_sub(fill.gpu_upload_bytes);
    let mesh_64 = work_64
        .gpu_upload_bytes
        .saturating_sub(fill.gpu_upload_bytes);
    // Isolation still includes the interned clip palette: every L-stroke
    // shares one FragmentClip, so GpuClip bytes stay constant.
    let instance_32 = mesh_32.saturating_sub(super::mesh::GPU_CLIP_BYTES);
    let instance_64 = mesh_64.saturating_sub(super::mesh::GPU_CLIP_BYTES);
    let instance_16 = mesh_16.saturating_sub(super::mesh::GPU_CLIP_BYTES);
    assert_eq!(instance_32, instance_16 * 2);
    assert!(instance_32 > 0, "strokes must add mesh instance bytes");
    assert_eq!(
        instance_64,
        instance_32 * 2,
        "doubling GraphCanvas edges must double isolated mesh instance bytes"
    );
    assert_eq!(
        work_32.draw_calls.saturating_sub(fill.draw_calls),
        1,
        "consecutive same-scissor Stroke primitives batch into one mesh draw"
    );
    assert_eq!(
        work_64.draw_calls.saturating_sub(fill.draw_calls),
        1,
        "64 batched GraphCanvas edges still issue one mesh draw"
    );
}

#[test]
#[cfg(feature = "graph-canvas")]
fn graph_canvas_stroke_skips_identical_instance_upload() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = graph_canvas_scene(l_stroke_edges(16));
    let first = encode_scene_gpu_work(&device, &queue, &mut painter, &scene);
    let second = encode_scene_gpu_work(&device, &queue, &mut painter, &scene.clone());
    assert!(
        second.gpu_upload_bytes < first.gpu_upload_bytes,
        "rebuilding the same stroke instances must skip the mesh upload, first {} second {}",
        first.gpu_upload_bytes,
        second.gpu_upload_bytes
    );
}

#[cfg(feature = "graph-canvas")]
fn graph_canvas_scene(edges: Vec<(Vec<[f32; 2]>, [f32; 4])>) -> UiScene {
    let mut scene = UiScene::new();
    scene.apply_delta(
        [graph_canvas_stroke_node(1, edges, [0.0, 0.0, 1.0, 1.0])],
        [],
    );
    scene
}

#[cfg(feature = "graph-canvas")]
fn l_stroke_edges(count: usize) -> Vec<(Vec<[f32; 2]>, [f32; 4])> {
    (0..count)
        .map(|index| {
            let x = 8.0 + (index % 8) as f32 * 6.0;
            let y = 8.0 + (index / 8) as f32 * 6.0;
            (
                vec![[x, y], [x + 4.0, y], [x + 4.0, y + 4.0]],
                [1.0, 0.0, 0.0, 1.0],
            )
        })
        .collect()
}

pub(super) fn paint_scene_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut SceneWgpuPainter,
    scene: &UiScene,
    logical_size: [f32; 2],
    physical_size: [u32; 2],
    scale_factor: f32,
) -> Vec<u8> {
    let viewport = ScenePaintViewport {
        logical_size,
        physical_size,
        scale_factor,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(
        device,
        wgpu::TextureFormat::Rgba8Unorm,
        physical_size[0],
        physical_size[1],
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui articulated stroke probe"),
    });
    painter
        .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(
        device,
        queue,
        encoder,
        &texture,
        physical_size[0],
        physical_size[1],
    );
    drop(texture);
    pixels
}

#[cfg(feature = "graph-canvas")]
fn encode_scene_gpu_work(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &mut SceneWgpuPainter,
    scene: &UiScene,
) -> nana_ui_core::GpuWorkObservation {
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let target = test_target(device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui articulated stroke work"),
    });
    painter
        .paint_encoder(scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    queue.submit([encoder.finish()]);
    painter
        .last_gpu_work()
        .expect("encoded GraphCanvas frame records GPU work")
}

#[test]
fn rotated_clip_does_not_paint_sibling_in_aabb_outside_rect() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 8.0, 8.0, 16.0, 16.0, [0.0, 0.0, 1.0, 1.0]),
            rotated_overflow_parent(2, &[3], 16.0, 16.0, 32.0, 32.0),
            colored_quad_child(3, 2, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let (probe_x, probe_y) = aabb_outside_rotated_overflow_probe();

    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated clip sibling"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let sibling = pixel(&pixels, 64, probe_x, probe_y);
    let inside = pixel(&pixels, 64, 32, 32);
    assert!(
        is_blue_slot(sibling),
        "rotated overflow must not cover sibling in AABB-outside-rect, pixel ({probe_x},{probe_y})={sibling:?}"
    );
    assert!(
        is_red_slot(inside),
        "rotated clip interior must still paint the overflowing child, got {inside:?}"
    );
    drop(texture);
}

#[test]
fn rotated_clip_does_not_paint_text_sibling_in_aabb_outside_rect() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 8.0, 8.0, 16.0, 16.0, [0.0, 0.0, 1.0, 1.0]),
            rotated_overflow_parent(2, &[3], 16.0, 16.0, 32.0, 32.0),
            overflowing_text_child(3, 2, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let (probe_x, probe_y) = aabb_outside_rotated_overflow_probe();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated clip text sibling"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let sibling = pixel(&pixels, 64, probe_x, probe_y);
    let inside = pixel(&pixels, 64, 32, 32);
    assert!(
        is_blue_slot(sibling),
        "rotated overflow must not cover sibling with text in AABB-outside-rect, pixel ({probe_x},{probe_y})={sibling:?}"
    );
    assert!(
        inside[0] > 40,
        "rotated clip interior must still paint overflowing text, got {inside:?}"
    );
    drop(texture);
}

#[test]
fn rotated_clip_does_not_paint_host_texture_sibling_in_aabb_outside_rect() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 8.0, 8.0, 16.0, 16.0, [0.0, 0.0, 1.0, 1.0]),
            rotated_overflow_parent(2, &[3], 16.0, 16.0, 32.0, 32.0),
            host_texture_child(3, 2, 0.0, 0.0, 64.0, 64.0, "layer"),
        ],
        [],
    );
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::RED);
    let registry = register_host_texture("layer", &view, 64, 64);
    let (probe_x, probe_y) = aabb_outside_rotated_overflow_probe();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated clip host texture sibling"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let sibling = pixel(&pixels, 64, probe_x, probe_y);
    let inside = pixel(&pixels, 64, 32, 32);
    assert!(
        is_blue_slot(sibling),
        "rotated overflow must not cover sibling with HostTexture in AABB-outside-rect, pixel ({probe_x},{probe_y})={sibling:?}"
    );
    assert!(
        is_red_slot(inside),
        "rotated clip interior must still sample HostTexture, got {inside:?}"
    );
    drop(view);
    drop(texture);
}

#[test]
fn nested_rotated_clips_reject_quad_inside_inner_outside_outer() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let mut inner = overflow_parent(3, &[4], 0.0, 0.0, 64.0, 64.0, None);
    inner.parent = Some(StableNodeId::new(2).unwrap());
    scene.apply_delta(
        [
            colored_quad_node(1, 8.0, 8.0, 16.0, 16.0, [0.0, 0.0, 1.0, 1.0]),
            rotated_overflow_parent(2, &[3], 16.0, 16.0, 32.0, 32.0),
            inner,
            colored_quad_child(4, 3, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let child = scene
        .primitives()
        .find(|primitive| {
            matches!(
                primitive.kind,
                ScenePrimitiveKind::Quad {
                    background: Some([1.0, 0.0, 0.0, 1.0]),
                    ..
                }
            )
        })
        .expect("overflowing child quad");
    assert!(
        super::clip::rotated_fragment_clips(
            &child.clips,
            super::clip::PaintOrigin::from(super::clip::paint_origin([0.0, 0.0], [0.0, 0.0]))
        )
        .len()
            >= 2,
        "child must carry two 45° parallelograms, got {:?}",
        child.clips
    );
    let (probe_x, probe_y) = nested_rotated_overflow_probe(&child.clips);

    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui nested rotated clip"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert!(
        counts.group >= 1,
        "two rotated clips must dest-composite the extra parallelogram, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let probe = pixel(&pixels, 64, probe_x, probe_y);
    let inside = pixel(&pixels, 64, 32, 32);
    assert!(
        !is_red_slot(probe),
        "nested extra clip must reject inner-inside/outer-outside, pixel ({probe_x},{probe_y})={probe:?}"
    );
    assert!(
        is_red_slot(inside),
        "intersection of both 45° clips must still paint, got {inside:?}"
    );
    drop(texture);
}

#[test]
fn rotated_clip_does_not_paint_custom_in_aabb_outside_rect() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 8.0, 8.0, 16.0, 16.0, [0.0, 0.0, 1.0, 1.0]),
            rotated_overflow_parent(2, &[3], 16.0, 16.0, 32.0, 32.0),
            custom_render_child(3, 2, 0.0, 0.0, 64.0, 64.0, "test.fill"),
        ],
        [],
    );
    let mut renderers = SceneGpuRendererRegistry::new();
    renderers.insert(
        "test.fill",
        Arc::new(FillClipRenderer::new(&device, format)),
    );
    let (probe_x, probe_y) = aabb_outside_rotated_overflow_probe();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated clip custom"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &view,
            viewport,
            None,
            Some(&renderers),
        )
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert!(
        counts.group >= 1,
        "rotated Custom must dest-wrap fragment clip, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let sibling = pixel(&pixels, 64, probe_x, probe_y);
    let inside = pixel(&pixels, 64, 32, 32);
    assert!(
        is_blue_slot(sibling),
        "Custom AABB scissor must not leak rotated overflow, pixel ({probe_x},{probe_y})={sibling:?}"
    );
    assert!(
        is_red_slot(inside),
        "rotated clip interior must still paint Custom, got {inside:?}"
    );
    drop(texture);
}

#[test]
fn axis_aligned_clips_do_not_dest_wrap() {
    let origin = super::clip::PaintOrigin::from(super::clip::paint_origin([0.0, 0.0], [0.0, 0.0]));
    let aligned = [ClipRegion {
        bounds: SceneRect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        },
        transform: AffineTransform::IDENTITY,
        corner_radius: 0.0,
        polygon_clip: None,
    }];
    let rotated = {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        [ClipRegion {
            bounds: SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            transform: AffineTransform::from_matrix(
                PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    ..PaintTransform::default()
                }
                .around_center(16.0, 16.0, 32.0, 32.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        }]
    };
    let custom = ScenePrimitiveKind::Custom {
        node: CustomRenderNode::new("test.fill", "slot", 1),
        mask: None,
        corner_radius: [0.0; 4],
    };
    let quad = ScenePrimitiveKind::Quad {
        background: Some([1.0, 0.0, 0.0, 1.0]),
        border_color: None,
        border_width: 0.0,
        corner_radius: [0.0; 4],
        shadow: None,
        surface: nana_ui_scene::QuadSurfacePaint::default(),
    };
    assert!(clip_dests_for(&custom, &aligned, origin).is_empty());
    assert!(clip_dests_for(&quad, &aligned, origin).is_empty());
    assert_eq!(clip_dests_for(&custom, &rotated, origin).len(), 1);
    assert!(
        clip_dests_for(&quad, &rotated, origin).is_empty(),
        "single rotated clip stays in Quad vertex attrs"
    );
    assert_eq!(
        super::clip::fragment_clip(&aligned, origin),
        super::clip::FragmentClip::PASS
    );

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            overflow_parent(1, &[2], 0.0, 0.0, 64.0, 64.0, None),
            colored_quad_child(2, 1, 8.0, 8.0, 48.0, 48.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let target = test_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui axis-aligned clip no dest wrap"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert_eq!(
        counts.group, 0,
        "axis-aligned overflow must stay PASS fragment_clip, got {counts:?}"
    );
    assert_eq!(
        counts.msaa, 1,
        "axis-aligned clip must not force dest-group interleave, got {counts:?}"
    );
    queue.submit([encoder.finish()]);
}

#[test]
fn mesh_polygon_clip_stays_in_gpu_clip_not_dest() {
    let origin = super::clip::PaintOrigin::from(super::clip::paint_origin([0.0, 0.0], [0.0, 0.0]));
    let polygon = [ClipRegion {
        bounds: SceneRect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
        transform: AffineTransform::IDENTITY,
        corner_radius: 0.0,
        polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
    }];
    let stroke = ScenePrimitiveKind::Stroke {
        points: vec![[8.0, 32.0], [56.0, 32.0]],
        width: 8.0,
        color: [1.0, 0.0, 0.0, 1.0],
        widths: Vec::new(),
        cap: StrokeCap::Round,
        pattern: None,
    };
    let quad = ScenePrimitiveKind::Quad {
        background: Some([1.0, 0.0, 0.0, 1.0]),
        border_color: None,
        border_width: 0.0,
        corner_radius: [0.0; 4],
        shadow: None,
        surface: nana_ui_scene::QuadSurfacePaint::default(),
    };
    assert_eq!(
        clip_dests_for(&quad, &polygon, origin).len(),
        1,
        "quads dest-wrap ancestor polygon (vertex locations full)"
    );
    assert!(
        clip_dests_for(&stroke, &polygon, origin).is_empty(),
        "mesh GpuClip owns the innermost polygon; no dest pass"
    );
    let inset_and_polygon = [
        ClipRegion {
            bounds: SceneRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            transform: AffineTransform::IDENTITY,
            corner_radius: 16.0,
            polygon_clip: None,
        },
        polygon[0].clone(),
    ];
    let mesh_dests = clip_dests_for(&stroke, &inset_and_polygon, origin);
    assert_eq!(
        mesh_dests.len(),
        1,
        "mesh dest must keep displaced inset(round) when GpuClip owns the polygon"
    );
    assert!(
        mesh_dests[0].corner_radius > 0.0 && mesh_dests[0].polygon_count < 3,
        "mesh dest is the inset(round), not a second polygon, got {mesh_dests:?}"
    );
    assert_eq!(
        clip_dests_for(&quad, &inset_and_polygon, origin).len(),
        2,
        "quads dest-wrap inset(round) and the polygon"
    );
}

#[test]
fn translucent_parent_composites_overlapping_children_as_a_group() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            translucent_parent(1, &[2, 3], 0.0, 0.0, 64.0, 32.0, 0.5),
            colored_quad_child(2, 1, 0.0, 0.0, 40.0, 32.0, [1.0, 0.0, 0.0, 1.0]),
            colored_quad_child(3, 1, 24.0, 0.0, 40.0, 32.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    assert_eq!(
        scene
            .opacity_groups(nana_ui_runtime::StableNodeId::new(2).unwrap())
            .to_vec(),
        vec![nana_ui_scene::OpacityGroup {
            node: nana_ui_runtime::StableNodeId::new(1).unwrap(),
            opacity: 0.5,
            filter: nana_ui_core::ColorFilter::default(),
            mix_blend: nana_ui_core::MixBlendMode::Normal,
            inset_shadow: None,
        }]
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 32.0],
        physical_size: [64, 32],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 32);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui group opacity paint"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert!(
        counts.group >= 1,
        "overlapping translucent children must use a group layer, got {counts:?}"
    );
    assert_eq!(
        counts.msaa, 0,
        "group frames share sample_count=1 with GPU-interleaved dest, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 32);
    let left = pixel(&pixels, 64, 8, 16);
    let overlap = pixel(&pixels, 64, 32, 16);
    let right = pixel(&pixels, 64, 56, 16);
    assert!(
        left[0] > 90 && left[0] < 160 && left[1] < 40 && left[2] < 40,
        "non-overlap must be parent opacity over black, got {left:?}"
    );
    assert!(
        right[0] > 90 && right[0] < 160 && right[1] < 40 && right[2] < 40,
        "non-overlap must be parent opacity over black, got {right:?}"
    );
    let overlap_delta = (overlap[0] as i16 - left[0] as i16).unsigned_abs();
    assert!(
        overlap_delta < 20 && overlap[1] < 40 && overlap[2] < 40,
        "group opacity must not darken overlapping children, left={left:?} overlap={overlap:?}"
    );
    drop(texture);
}

#[test]
fn drop_shadow_samples_dest_group_alpha_not_box_shadow_quads() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let shadow_srgb = [0.5, 0.5, 0.5, 1.0];
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            8.0,
            8.0,
            16.0,
            16.0,
            nana_ui_core::LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                paint: nana_ui_core::PaintStyle {
                    filter: Some(nana_ui_core::ColorFilter {
                        drop_shadow: Some(nana_ui_core::FilterDropShadow {
                            offset_x: 16.0,
                            offset_y: 0.0,
                            blur_radius: 0.0,
                            color: shadow_srgb,
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        [],
    );
    assert_eq!(
        scene
            .opacity_groups(nana_ui_runtime::StableNodeId::new(1).unwrap())
            .len(),
        1,
        "drop-shadow must isolate a dest-group"
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui drop-shadow dest-group paint"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert!(
        counts.group >= 1,
        "drop-shadow must composite via dest-group, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let fill = pixel(&pixels, 64, 12, 16);
    let shadow = pixel(&pixels, 64, 32, 16);
    let left = pixel(&pixels, 64, 4, 16);
    let linearized = super::color::to_rgba8(super::color::pack_linear(shadow_srgb));
    let raw_srgb = super::color::to_rgba8(shadow_srgb);
    assert!(
        fill[0] > 200 && fill[1] < 40 && fill[2] < 40,
        "source silhouette must stay red, got {fill:?}"
    );
    assert!(
        (shadow[0] as i16 - linearized[0] as i16).unsigned_abs() < 20
            && (shadow[1] as i16 - linearized[1] as i16).unsigned_abs() < 20
            && (shadow[2] as i16 - linearized[2] as i16).unsigned_abs() < 20,
        "mid-channel drop-shadow must pack_linear like box-shadow, got {shadow:?} expected {linearized:?}"
    );
    assert!(
        (shadow[0] as i16 - raw_srgb[0] as i16).unsigned_abs() > 40,
        "raw sRGB gray must not pass as linearized, got {shadow:?} raw {raw_srgb:?}"
    );
    assert!(
        left[0] < 20 && left[1] < 20 && left[2] < 20,
        "no box-shadow geometry left of the silhouette, got {left:?}"
    );
    drop(texture);
}

#[test]
fn drop_shadow_dest_group_follows_scene_origin() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            40.0,
            8.0,
            16.0,
            16.0,
            nana_ui_core::LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                paint: nana_ui_core::PaintStyle {
                    filter: Some(nana_ui_core::ColorFilter {
                        drop_shadow: Some(nana_ui_core::FilterDropShadow {
                            offset_x: 16.0,
                            offset_y: 0.0,
                            blur_radius: 0.0,
                            color: [0.5, 0.5, 0.5, 1.0],
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        [],
    );
    // A view of the scene from x = 32: the node lands at dest x 8..24 and
    // its shadow at 24..40.
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [32.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui drop-shadow scene origin"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let fill = pixel(&pixels, 64, 16, 16);
    let shadow = pixel(&pixels, 64, 32, 16);
    assert!(
        fill[0] > 200 && fill[1] < 40 && fill[2] < 40,
        "the filtered node must paint at its dest position, got {fill:?}"
    );
    assert!(
        shadow[1] > 40 && shadow[1] == shadow[2],
        "its drop-shadow must paint beside it, got {shadow:?}"
    );
    drop(texture);
}

#[test]
fn outline_and_shadow_spread_stay_css_px_at_hidpi() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            24.0,
            24.0,
            16.0,
            16.0,
            nana_ui_core::LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                paint: nana_ui_core::PaintStyle {
                    outline: nana_ui_core::OutlineSpec {
                        width: 2.0,
                        color: Some([0.0, 1.0, 0.0, 1.0]),
                        style: nana_ui_core::OutlineStyle::Solid,
                    },
                    box_shadows: vec![nana_ui_core::BoxShadowSpec {
                        offset_x: 0.0,
                        offset_y: 0.0,
                        blur_radius: 0.0,
                        spread_radius: 4.0,
                        color: [0.0, 0.0, 1.0, 1.0],
                        inset: false,
                    }],
                    ..Default::default()
                }
                .into(),
                ..Default::default()
            },
            Some([1.0, 0.0, 0.0, 1.0]),
        )],
        [],
    );

    let assert_band = |label: &str, pixels: &[u8], width: u32, scale: f32| {
        let sx = |logical: f32| (logical * scale).floor() as u32;
        let fill = pixel(pixels, width, sx(32.0), sx(32.0));
        let outline = pixel(pixels, width, sx(23.0), sx(32.0));
        let spread = pixel(pixels, width, sx(21.0), sx(32.0));
        let beyond = pixel(pixels, width, sx(19.5), sx(32.0));
        assert!(
            is_red_slot(fill),
            "{label} fill must stay red, got {fill:?}"
        );
        assert!(
            is_green_slot(outline),
            "{label} 2px outline must cover 1 CSS px outside the box, got {outline:?}"
        );
        assert!(
            is_blue_slot(spread),
            "{label} spread 4px must cover 3 CSS px outside the box, got {spread:?}"
        );
        assert!(
            beyond[0] < 20 && beyond[1] < 20 && beyond[2] < 20,
            "{label} must not enlarge spread at HiDPI (4.5 CSS px outside), got {beyond:?}"
        );
    };

    let one_x = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    assert_band("1x", &one_x, 64, 1.0);
    let two_x = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [128, 128],
        2.0,
    );
    assert_band("2x", &two_x, 128, 2.0);
}

#[test]
fn host_texture_paints_in_document_order_with_runtime_button() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let (scene, registry, texture, overlay_fill) =
        runtime_button_over_host_texture_scene(&device, &queue, format);
    assert!(
        overlay_fill[3] > 0.99,
        "Selected Button theme fill must be opaque, got {overlay_fill:?}"
    );
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui runtime button order paint"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    assert_mixed_gpu_pass_counts(&painter);
    let order = scene
        .primitives()
        .map(|primitive| match &primitive.kind {
            ScenePrimitiveKind::Quad { .. } => "quad",
            ScenePrimitiveKind::Custom { .. } => "custom",
            ScenePrimitiveKind::Text { .. } => "text",
            _ => "other",
        })
        .collect::<Vec<_>>();
    assert!(
        order.iter().filter(|kind| **kind == "custom").count() == 1 && order.contains(&"quad"),
        "runtime tree must emit background/overlay quads and one host texture, got {order:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
    let background = pixel(&pixels, 64, 32, 8);
    let texture_only = pixel(&pixels, 64, 32, 18);
    let foreground = pixel(&pixels, 64, 32, 36);
    assert!(
        is_danger_fill(background),
        "background quad below the texture must stay the Danger fill, got {background:?} order={order:?}"
    );
    assert!(
        is_green_slot(texture_only),
        "texture-only band should stay green, got {texture_only:?} order={order:?}"
    );
    assert!(
        !is_green_slot(foreground) && !is_danger_fill(foreground),
        "Selected Button fill must paint over the host texture, got {foreground:?} order={order:?} fill={overlay_fill:?}"
    );
    drop(texture);
}

#[test]
fn two_host_texture_layers_paint_with_opaque_chrome_between() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let (scene, registry, layers) =
        two_host_texture_layers_with_chrome_scene(&device, &queue, format);
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui layered host texture paint"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    assert_mixed_gpu_pass_counts(&painter);
    let custom = scene
        .primitives()
        .filter(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Custom { .. }))
        .count();
    assert_eq!(custom, 2, "layered scene must sample two HostTexture nodes");
    let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
    let background_only = pixel(&pixels, 64, 32, 8);
    let chrome = pixel(&pixels, 64, 32, 20);
    let foreground = pixel(&pixels, 64, 32, 36);
    assert!(
        is_blue_slot(background_only),
        "bg-only region must stay the background texture, got {background_only:?}"
    );
    assert!(
        !is_blue_slot(chrome) && !is_red_slot(chrome),
        "chrome not covered by the foreground texture must stay the Button fill, got {chrome:?}"
    );
    assert!(
        is_red_slot(foreground),
        "foreground HostTexture must cover chrome where they overlap, got {foreground:?}"
    );
    drop(layers);
}

#[test]
fn geometry_and_text_keep_msaa_then_text_after_resolve() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = labeled_selected_button_scene();
    let has_text = scene
        .primitives()
        .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Text { .. }));
    let has_quad = scene
        .primitives()
        .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Quad { .. }));
    assert!(
        has_text && has_quad,
        "Selected Button must emit fill + label"
    );
    let has_gpu = scene
        .primitives()
        .any(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Custom { .. }));
    assert!(!has_gpu);
    let target = test_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui msaa then text"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert_eq!(
        counts.msaa, 1,
        "pure UI may keep 4x MSAA for Quad/Mesh, got {counts:?}"
    );
    assert!(
        counts.msaa_allocated,
        "pure UI dest must allocate 4x MSAA, got {counts:?}"
    );
    assert_eq!(
        counts.color, 1,
        "Text must paint after MSAA resolve with Load, got {counts:?}"
    );
    assert_eq!(
        counts.blit, 1,
        "dest→window blit is one extra pass, got {counts:?}"
    );
    queue.submit([encoder.finish()]);
}

#[test]
fn paint_accepts_rotation_and_letter_spacing() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            RuntimeButton::new("Hi")
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, button.stable_id(), 8.0, 16.0, 48.0, 32.0);
    let mut style = nana_ui_runtime::NodeStyle {
        layout: square_button_layout(),
        ..Default::default()
    };
    Arc::make_mut(&mut style.layout).transform = Some(nana_ui_core::PaintTransform {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
        e: 0.0,
        f: 0.0,
    });
    Arc::make_mut(&mut style.layout).letter_spacing = Some(0.5);
    Arc::make_mut(&mut style.layout).font_family = Some("Noto Sans SC".into());
    layout.set_style(button.stable_id(), style);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    validate_scene(&scene, None, None, crate::test_gpu::context().generation())
        .expect("rotation and tracking must validate");
    let target = test_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui affine tracking paint"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .expect("supported affine and tracking must paint");
    queue.submit([encoder.finish()]);
}

#[test]
fn letter_spacing_changes_painted_pixels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let tight = labeled_selected_button_scene();
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            RuntimeButton::new("Hi")
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, button.stable_id(), 8.0, 16.0, 48.0, 32.0);
    let mut style = nana_ui_runtime::NodeStyle {
        layout: square_button_layout(),
        ..Default::default()
    };
    Arc::make_mut(&mut style.layout).letter_spacing = Some(8.0);
    layout.set_style(button.stable_id(), style);
    context.commit_mutations(layout).unwrap();
    let tracked = commit_scene(&mut context);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let paint = |painter: &mut SceneWgpuPainter, scene: &UiScene| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui tracking paint"),
        });
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        readback_rgba(&device, &queue, encoder, &texture, 64, 64)
    };
    let first = paint(&mut painter, &tight);
    let second = paint(&mut painter, &tracked);
    assert_ne!(
        first, second,
        "8px letter-spacing must change painted pixels versus default tracking"
    );
    drop(texture);
}

#[test]
fn painted_extent_reports_the_fitted_device_pixels_not_the_layout_box() {
    // 消费方按「这张宿主纹理实际被画到多少设备像素」准备内容(超分、
    // 重新解码)。布局盒不是这个数:`ContentFit` 会把 64x32 的源按比例
    // 放进 64x64 的盒子里,缩放因子还要再乘一遍。
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let preview = context
        .create_component(document, GpuTextureView::new("layer").contain())
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, preview.stable_id(), 0.0, 0.0, 64.0, 64.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let view = solid_texture_view(&device, &queue, format, 64, 32, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 32);
    assert_eq!(
        registry.painted_extent("layer"),
        None,
        "还没画过就没有绘制尺寸可报"
    );
    let (_target, target_view) = test_copy_target(&device, format, 128, 128);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [128, 128],
        scale_factor: 2.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture painted extent"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    queue.submit(std::iter::once(encoder.finish()));
    assert_eq!(
        registry.painted_extent("layer"),
        Some([128, 64]),
        "64x32 的源 contain 进 64x64 的盒子是 64x32 逻辑像素,2 倍缩放后是 128x64"
    );
    registry.remove("layer");
    assert_eq!(
        registry.painted_extent("layer"),
        None,
        "slot 被移除后不该留下上一帧的绘制尺寸"
    );
}

#[test]
fn painted_extent_is_the_largest_consumer_of_a_shared_slot_in_any_paint_order() {
    // 同一个 slot 被两个不同尺寸的节点采样:纹理只有一份,它的需求要满足
    // 大的那个,且与谁后画无关;大的那个缩小后,下一次全新绘制随之回落。
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let (_target, target_view) = test_copy_target(&device, format, 128, 128);
    let viewport = ScenePaintViewport {
        logical_size: [128.0, 128.0],
        physical_size: [128, 128],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    for large_first in [true, false] {
        let mut painter = SceneWgpuPainter::for_test(format);
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let first = context
            .create_component(document, GpuTextureView::new("cover"))
            .unwrap();
        let second = context
            .create_component(document, GpuTextureView::new("cover"))
            .unwrap();
        let (large, small) = if large_first {
            (first, second)
        } else {
            (second, first)
        };
        let mut layout = MutationQueue::new();
        write_box(&mut layout, large.stable_id(), 0.0, 0.0, 96.0, 96.0);
        write_box(&mut layout, small.stable_id(), 100.0, 100.0, 24.0, 24.0);
        context.commit_mutations(layout).unwrap();
        let registry = register_host_texture("cover", &view, 64, 64);
        let paint = |painter: &mut SceneWgpuPainter, scene: &UiScene| {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nana-ui shared slot painted extent"),
            });
            painter
                .paint_encoder(
                    scene,
                    &mut encoder,
                    &target_view,
                    viewport,
                    Some(&registry),
                    None,
                )
                .unwrap();
            queue.submit(std::iter::once(encoder.finish()));
        };
        let scene = commit_scene(&mut context);
        paint(&mut painter, &scene);
        assert_eq!(registry.painted_extent("cover"), Some([96, 96]));

        let seen = registry.painted_revision();
        let mut layout = MutationQueue::new();
        write_box(&mut layout, large.stable_id(), 0.0, 0.0, 48.0, 48.0);
        context.commit_mutations(layout).unwrap();
        let scene = commit_scene(&mut context);
        paint(&mut painter, &scene);
        assert_eq!(registry.painted_extent("cover"), Some([48, 48]));
        let mut changed = Vec::new();
        registry.painted_changes_since(seen, &mut changed);
        assert_eq!(changed, vec![std::sync::Arc::<str>::from("cover")]);

        drop(painter);
        assert_eq!(
            registry.painted_extent("cover"),
            None,
            "画家没了,它登记的需求随之撤回"
        );
    }
}

#[cfg(feature = "wgpu-interop")]
#[test]
fn host_texture_wrapped_with_an_srgb_view_is_decoded_when_sampled() {
    // 计算着色器只能写 `Rgba8Unorm` 存储纹理,内容却是 sRGB 编码的;场景必须
    // 按 `Rgba8UnormSrgb` 采样同一份字节才会解码。按默认视图包装会把编码值
    // 当线性值,画面整体发白。
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui srgb view probe"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &[128u8, 128, 128, 255].repeat(16),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: Some(4),
        },
        wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
    );
    let gpu = crate::test_gpu::context();
    assert!(
        nana_gpu::GpuTexture::from_wgpu_with_view_format(
            &gpu,
            texture.clone(),
            wgpu::TextureFormat::Bgra8UnormSrgb,
        )
        .is_err(),
        "只能换 sRGB/线性两种写法,不能换存储格式"
    );
    let wrapped = nana_gpu::GpuTexture::from_wgpu_with_view_format(
        &gpu,
        texture,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
    .unwrap();
    let registry = HostTextureRegistry::new();
    registry.register(
        "layer",
        crate::HostTexture::new(1, 1, &wrapped),
        4,
        4,
        crate::HostTextureAlphaMode::Opaque,
    );
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let preview = context
        .create_component(document, GpuTextureView::new("layer"))
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, preview.stable_id(), 0.0, 0.0, 16.0, 16.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let (target, target_view) = test_copy_target(&device, format, 16, 16);
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui srgb view paint"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            ScenePaintViewport {
                logical_size: [16.0, 16.0],
                physical_size: [16, 16],
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &target, 16, 16);
    let center = ((8 * 16 + 8) * 4) as usize;
    // sRGB 128 解码为线性约 0.216,写进线性目标约 55。
    assert!(
        (50..=60).contains(&pixels[center]),
        "sampled {} instead of the decoded ~55",
        pixels[center]
    );
}

#[test]
fn host_texture_rounded_clip_matches_sibling_quad_not_fitted_dest() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let preview = context
        .create_component(
            document,
            GpuTextureView::new("layer")
                .with_corner_radius(32.0)
                .contain(),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, preview.stable_id(), 0.0, 0.0, 64.0, 64.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let view = solid_texture_view(&device, &queue, format, 64, 32, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 32);
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture rounded clip"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
    let center = pixel(&pixels, 64, 32, 32);
    let dest_corner = pixel(&pixels, 64, 2, 18);
    assert!(
        is_green_slot(center),
        "contain dest center must stay inside the sibling Quad circle, got {center:?}"
    );
    assert!(
        dest_corner[1] < 40,
        "letterboxed dest corner is outside the 32px node circle and must not round the dest, got {dest_corner:?}"
    );
    drop(view);
}

#[test]
fn host_texture_clips_each_corner_with_its_own_radius() {
    // 只圆顶部两角:底部两角必须保持直角,不能取四角最大值一起圆掉。
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let preview = context
        .create_component(
            document,
            GpuTextureView::new("layer").with_corner_radii([24.0, 24.0, 0.0, 0.0]),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, preview.stable_id(), 0.0, 0.0, 64.0, 64.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture per-corner clip"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            ScenePaintViewport {
                logical_size: [64.0, 64.0],
                physical_size: [64, 64],
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
    for (x, y) in [(1, 1), (62, 1)] {
        let corner = pixel(&pixels, 64, x, y);
        assert!(
            corner[1] < 40,
            "top corner ({x},{y}) must be clipped, got {corner:?}"
        );
    }
    for (x, y) in [(1, 62), (62, 62)] {
        let corner = pixel(&pixels, 64, x, y);
        assert!(
            is_green_slot(corner),
            "bottom corner ({x},{y}) stays square, got {corner:?}"
        );
    }
    drop(view);
}

/// A 64px circle (`overflow: hidden`, radius 32) at the origin around `child`,
/// which covers it edge to edge.
fn rounded_clip_around(child: ExtractedNode) -> [ExtractedNode; 2] {
    let parent = extracted_div(
        1,
        &[child.id.get()],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle {
            overflow_x: OverflowSpec::Hidden,
            overflow_y: OverflowSpec::Hidden,
            border_radius: Some(32.0),
            ..nana_ui_core::LayoutStyle::default()
        },
        None,
    );
    [parent, child]
}

/// The pixels inside, on and outside the 64px circle's curve.
fn rounded_clip_edge_samples(pixels: &[u8]) -> ([u8; 4], [u8; 4], [u8; 4]) {
    // Pixel (6, 12) has its center 0.1px outside the circle: a binary test
    // clears it, a one-pixel ramp keeps about 40%. (1, 1) is well outside,
    // (32, 32) well inside.
    (
        pixel(pixels, 64, 32, 32),
        pixel(pixels, 64, 6, 12),
        pixel(pixels, 64, 1, 1),
    )
}

#[test]
fn child_quad_is_antialiased_by_its_parents_rounded_clip() {
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = UiScene::new();
    scene.apply_delta(
        rounded_clip_around(colored_quad_child(
            2,
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 1.0, 0.0, 1.0],
        )),
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let (inside, edge, outside) = rounded_clip_edge_samples(&pixels);
    assert_eq!(inside[1], 255, "inside the clip, got {inside:?}");
    assert!(
        (40..=200).contains(&edge[1]),
        "a pixel on the clip's curve must be partially covered, got {edge:?}"
    );
    assert_eq!(outside[1], 0, "outside the clip, got {outside:?}");
}

#[test]
fn a_rounded_clip_edge_stays_one_device_pixel_under_an_anisotropic_transform() {
    // A round `overflow: hidden` parent under `rotate(30deg) scale(1.6, 0.6)`
    // clipping a larger green child, a quad or a texture: the clip's edge is
    // the only one in sight. Its ramp must be one device pixel across the
    // ellipse's own normal, which an isotropic `1 / sqrt|det|` gets wrong by
    // up to the square root of the anisotropy.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("fill", &view, 64, 64);
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let [a, b, c, d] = [cos * 1.6, sin * 1.6, -sin * 0.6, cos * 0.6];
    let parent = |child: u64| {
        let mut parent = extracted_div(
            1,
            &[child],
            16.0,
            16.0,
            32.0,
            32.0,
            nana_ui_core::LayoutStyle {
                overflow_x: OverflowSpec::Hidden,
                overflow_y: OverflowSpec::Hidden,
                border_radius: Some(16.0),
                ..nana_ui_core::LayoutStyle::default()
            },
            None,
        );
        Arc::make_mut(&mut parent.source_style.layout).transform = Some(PaintTransform {
            a,
            b,
            c,
            d,
            ..PaintTransform::default()
        });
        parent
    };
    let children = [
        colored_quad_child(2, 1, 8.0, 8.0, 48.0, 48.0, [0.0, 1.0, 0.0, 1.0]),
        host_texture_child(2, 1, 8.0, 8.0, 48.0, 48.0, "fill"),
    ];
    for (kind, child) in ["quad", "texture"].into_iter().zip(children) {
        let mut scene = UiScene::new();
        scene.apply_delta([parent(2), child], []);
        let (target, target_view) = test_copy_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        SceneWgpuPainter::for_test(format)
            .paint_encoder(
                &scene,
                &mut encoder,
                &target_view,
                ScenePaintViewport {
                    logical_size: [64.0, 64.0],
                    physical_size: [64, 64],
                    scale_factor: 1.0,
                    scene_origin: [0.0, 0.0],
                    target_origin: [0.0, 0.0],
                    clear_color: [0.0, 0.0, 0.0, 1.0],
                    clear: true,
                },
                Some(&registry),
                None,
            )
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
        let det = a * d - b * c;
        let inverse = [d / det, -b / det, -c / det, a / det];
        for py in 0..64 {
            for px in 0..64 {
                let [sx, sy] = [px as f32 + 0.5 - 32.0, py as f32 + 0.5 - 32.0];
                let [lx, ly] = [
                    inverse[0] * sx + inverse[2] * sy,
                    inverse[1] * sx + inverse[3] * sy,
                ];
                let length = lx.hypot(ly).max(1e-6);
                let [nx, ny] = [lx / length, ly / length];
                // First order: the local distance over its screen gradient, M⁻ᵀ n.
                let gradient =
                    (inverse[0] * nx + inverse[1] * ny).hypot(inverse[2] * nx + inverse[3] * ny);
                let outside = (length - 16.0) / gradient;
                if outside.abs() > 1.2 {
                    continue;
                }
                let expected = (0.5 - outside).clamp(0.0, 1.0) * 255.0;
                let green = f32::from(pixel(&pixels, 64, px, py)[1]);
                assert!(
                    (green - expected).abs() <= 8.0,
                    "{kind} clipped: ({px},{py}) is {outside:+.3} px from the clip's rim, \
                     expected green {expected:.0}, got {green}"
                );
            }
        }
    }
    drop(view);
}

#[test]
fn rectangle_and_polygon_clip_edges_stay_one_device_pixel_under_a_transform() {
    // Under `rotate(20deg) scale(1.3, 0.8)`: a rectangular `overflow: hidden`
    // parent clipping a larger quad or texture, a `clip-path: polygon()`
    // parent clipping a larger quad, and a quad clipped by its own polygon.
    // Every edge is the clip's, compared with a one device pixel ramp over
    // its exact distance.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("fill", &view, 64, 64);
    let green = [0.0, 1.0, 0.0, 1.0];
    let (sin, cos) = 20f32.to_radians().sin_cos();
    let [a, b, c, d] = [cos * 1.3, sin * 1.3, -sin * 0.8, cos * 0.8];
    let transform = Some(PaintTransform {
        a,
        b,
        c,
        d,
        ..PaintTransform::default()
    });
    let on_screen = |points: &[[f32; 2]]| -> Vec<[f32; 2]> {
        points
            .iter()
            .map(|[x, y]| {
                let [u, v] = [x - 32.0, y - 32.0];
                [32.0 + a * u + c * v, 32.0 + b * u + d * v]
            })
            .collect()
    };
    let square = on_screen(&[[16.0, 16.0], [48.0, 16.0], [48.0, 48.0], [16.0, 48.0]]);
    let triangle = on_screen(&[[16.0, 16.0], [48.0, 16.0], [32.0, 48.0]]);
    let mut rectangle = extracted_div(
        1,
        &[2],
        16.0,
        16.0,
        32.0,
        32.0,
        nana_ui_core::LayoutStyle {
            overflow_x: OverflowSpec::Hidden,
            overflow_y: OverflowSpec::Hidden,
            ..nana_ui_core::LayoutStyle::default()
        },
        None,
    );
    Arc::make_mut(&mut rectangle.source_style.layout).transform = transform;
    let mut polygon = clip_path_polygon_parent(1, &[2], 16.0, 16.0, 32.0, 32.0);
    Arc::make_mut(&mut polygon.source_style.layout).transform = transform;
    let mut own = clip_path_polygon_parent(1, &[], 16.0, 16.0, 32.0, 32.0);
    own.style = Arc::new(ComputedStyle {
        background: Some(green),
        ..ComputedStyle::default()
    });
    let layout = Arc::make_mut(&mut own.source_style.layout);
    layout.background = Some(green);
    layout.transform = transform;
    let quad_child = colored_quad_child(2, 1, 8.0, 8.0, 48.0, 48.0, green);
    let texture_child = host_texture_child(2, 1, 8.0, 8.0, 48.0, 48.0, "fill");
    type Case<'a> = (&'a str, Vec<ExtractedNode>, &'a [[f32; 2]]);
    let cases: [Case; 4] = [
        (
            "rectangle clip, quad",
            vec![rectangle.clone(), quad_child.clone()],
            &square,
        ),
        (
            "rectangle clip, texture",
            vec![rectangle, texture_child],
            &square,
        ),
        ("polygon clip, quad", vec![polygon, quad_child], &triangle),
        ("own polygon", vec![own], &triangle),
    ];
    for (label, nodes, corners) in cases {
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        let (target, target_view) = test_copy_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        SceneWgpuPainter::for_test(format)
            .paint_encoder(
                &scene,
                &mut encoder,
                &target_view,
                ScenePaintViewport {
                    logical_size: [64.0, 64.0],
                    physical_size: [64, 64],
                    scale_factor: 1.0,
                    scene_origin: [0.0, 0.0],
                    target_origin: [0.0, 0.0],
                    clear_color: [0.0, 0.0, 0.0, 1.0],
                    clear: true,
                },
                Some(&registry),
                None,
            )
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
        for py in 0..64 {
            for px in 0..64 {
                let outside = convex_polygon_distance([px as f32 + 0.5, py as f32 + 0.5], corners);
                if outside.abs() > 1.2 {
                    continue;
                }
                let expected = (0.5 - outside).clamp(0.0, 1.0) * 255.0;
                let green = f32::from(pixel(&pixels, 64, px, py)[1]);
                assert!(
                    (green - expected).abs() <= 8.0,
                    "{label}: ({px},{py}) is {outside:+.3} px from the edge, \
                     expected green {expected:.0}, got {green}"
                );
            }
        }
    }
    drop(view);
}

#[cfg(feature = "charts")]
#[test]
fn a_donut_slice_is_one_solid_colour_across_its_whole_sweep() {
    // A clip edge ramps over one device pixel, so a slice cut into wedges
    // painted edge to edge leaves each side of a cut about half its colour:
    // over each other, a seam at three quarters. Off the box's pixel grid,
    // a cut along an axis shows as well as a diagonal one.
    use nana_ui_runtime::{DonutChart, DonutSlice};
    let chart = DonutChart::new([5.0, 3.0, 2.0].map(|value| DonutSlice {
        value,
        color: SemanticColorRole::Accent,
    }));
    let bounds = LayoutBox {
        x: 4.3,
        y: 6.7,
        width: 116.0,
        height: 116.0,
    };
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let entity = context.create_component(document, chart.clone()).unwrap();
    let mut layout = MutationQueue::new();
    write_box(
        &mut layout,
        entity.stable_id(),
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
    );
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [128.0, 128.0],
        [128, 128],
        1.0,
    );
    let mut solid: [Option<[u8; 4]>; 3] = [None; 3];
    let mut checked = [0; 3];
    for py in 0..128 {
        for px in 0..128 {
            let [x, y] = [px as f32 + 0.5, py as f32 + 0.5];
            // At least 1.4 px inside its slice: clear of the gaps between
            // slices and of the ring's own edges.
            let Some(slice) = chart.slice_at(bounds, x, y) else {
                continue;
            };
            let inside = (0..8).all(|step| {
                let angle = step as f32 * std::f32::consts::FRAC_PI_4;
                chart.slice_at(bounds, x + 1.5 * angle.cos(), y + 1.5 * angle.sin()) == Some(slice)
            });
            if !inside {
                continue;
            }
            let got = pixel(&pixels, 128, px, py);
            let colour = *solid[slice].get_or_insert(got);
            assert!(
                got.iter().zip(colour).all(|(a, b)| a.abs_diff(b) <= 2),
                "slice {slice}: ({px},{py}) is {got:?}, the rest of it {colour:?}"
            );
            checked[slice] += 1;
        }
    }
    assert!(checked.iter().all(|&count| count > 300), "{checked:?}");
}

#[test]
fn child_host_texture_is_antialiased_by_its_parents_rounded_clip() {
    // A host texture takes the frame off MSAA: the clip's own ramp is the
    // only thing that can smooth this edge.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        rounded_clip_around(host_texture_child(2, 1, 0.0, 0.0, 64.0, 64.0, "layer")),
        [],
    );
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture under a rounded parent clip"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            ScenePaintViewport {
                logical_size: [64.0, 64.0],
                physical_size: [64, 64],
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
    let (inside, edge, outside) = rounded_clip_edge_samples(&pixels);
    assert!(is_green_slot(inside), "inside the clip, got {inside:?}");
    assert!(
        (40..=200).contains(&edge[1]),
        "a pixel on the clip's curve must be partially covered, got {edge:?}"
    );
    assert_eq!(outside[1], 0, "outside the clip, got {outside:?}");
    drop(view);
}

#[test]
fn custom_node_is_cut_by_its_parents_rounded_clip() {
    // A renderer draws its whole region; the rounding is the painter's.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        rounded_clip_around(custom_render_child(2, 1, 0.0, 0.0, 64.0, 64.0, "test.fill")),
        [],
    );
    let mut renderers = SceneGpuRendererRegistry::new();
    renderers.insert(
        "test.fill",
        Arc::new(FillClipRenderer::new(&device, format)),
    );
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui custom under a rounded parent clip"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &view,
            ScenePaintViewport {
                logical_size: [64.0, 64.0],
                physical_size: [64, 64],
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
            None,
            Some(&renderers),
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let (inside, edge, outside) = rounded_clip_edge_samples(&pixels);
    assert!(is_red_slot(inside), "inside the clip, got {inside:?}");
    assert!(
        (40..=200).contains(&edge[0]),
        "a pixel on the clip's curve must be partially covered, got {edge:?}"
    );
    assert_eq!(outside[0], 0, "outside the clip, got {outside:?}");
    drop(texture);
}

/// 64×64 纯绿宿主纹理 `face`,按 `component` 画在黑底上,读回像素。
fn paint_green_face<C: nana_ui_runtime::ComponentView>(component: C) -> Vec<u8> {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let entity = context.create_component(document, component).unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, entity.stable_id(), 0.0, 0.0, 64.0, 64.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("face", &view, 64, 64);
    let (target, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui green face"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            ScenePaintViewport {
                logical_size: [64.0, 64.0],
                physical_size: [64, 64],
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
            Some(&registry),
            None,
        )
        .unwrap();
    readback_rgba(&device, &queue, encoder, &target, 64, 64)
}

#[test]
fn avatar_edge_keeps_its_antialiasing_under_its_own_overflow_clip() {
    let pixels = paint_green_face(Avatar::new("face").size(64.0));
    // 像素中心在圆外 0.1px:自身 overflow 的硬裁剪会把它清成 0。
    let edge = pixel(&pixels, 64, 6, 12);
    assert!(
        (40..=200).contains(&edge[1]),
        "edge pixel just outside the circle must be partially covered, got {edge:?}"
    );
}

#[test]
fn hover_card_image_trigger_is_round_without_a_surface_quad() {
    // 菜单面的触发节点不出底色 Quad,圆角只能来自节点自身。
    let pixels = paint_green_face(
        HoverCard::new()
            .trigger_image("face", "account")
            .trigger_size(64.0),
    );
    assert!(is_green_slot(pixel(&pixels, 64, 32, 32)));
    let corner = pixel(&pixels, 64, 2, 2);
    assert!(corner[1] < 8, "corner outside the circle, got {corner:?}");
}

/// `(label, logical box, transform scale, scale factor)`: the box's inscribed
/// circle, scaled about its centre, in a 64 logical px target.
type ScaledCircle = (&'static str, [f32; 4], f32, f32);

const SCALED_CIRCLES: [ScaledCircle; 3] = [
    ("scale(0.5)", [0.0, 0.0, 64.0, 64.0], 0.5, 1.0),
    ("scale(0.5) at 150%", [0.0, 0.0, 64.0, 64.0], 0.5, 1.5),
    ("scale(2)", [24.0, 24.0, 16.0, 16.0], 2.0, 1.0),
];

/// Which side of a [`ScaledCircle`]'s rim is painted green.
#[derive(Clone, Copy)]
enum Painted {
    Inside,
    Outside,
}

/// Every pixel near the circle's rim against a one device pixel ramp,
/// `0.5 - (distance - radius)`, mirrored when the outside is painted; a ramp
/// of `1 / scale` px misses it by a quarter of the range a third of a pixel
/// from the rim.
fn assert_one_pixel_rim(
    pixels: &[u8],
    (label, [x, y, w, h], scale, factor): ScaledCircle,
    painted: Painted,
) {
    let center = [(x + w * 0.5) * factor, (y + h * 0.5) * factor];
    let radius = w.min(h) * 0.5 * scale * factor;
    let side = (64.0 * factor) as u32;
    for py in 0..side {
        for px in 0..side {
            let from_rim =
                (px as f32 + 0.5 - center[0]).hypot(py as f32 + 0.5 - center[1]) - radius;
            if from_rim.abs() > 1.5 {
                continue;
            }
            let from_rim = match painted {
                Painted::Inside => from_rim,
                Painted::Outside => -from_rim,
            };
            let expected = (0.5 - from_rim).clamp(0.0, 1.0) * 255.0;
            let green = f32::from(pixel(pixels, side, px, py)[1]);
            assert!(
                (green - expected).abs() <= 16.0,
                "{label}: ({px},{py}) is {from_rim:+.3} px from the rim, \
                 expected green {expected:.0}, got {green}"
            );
        }
    }
}

fn scale_transform(scale: f32) -> Option<PaintTransform> {
    Some(PaintTransform {
        a: scale,
        d: scale,
        ..PaintTransform::default()
    })
}

#[test]
fn rounded_quad_edge_stays_one_device_pixel_under_transform_scale() {
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let green = [0.0, 1.0, 0.0, 1.0];
    for circle @ (_, [x, y, w, h], scale, factor) in SCALED_CIRCLES {
        let mut scene = UiScene::new();
        let style = nana_ui_core::LayoutStyle {
            background: Some(green),
            border_radius: Some(w.min(h) * 0.5),
            transform: scale_transform(scale),
            ..Default::default()
        };
        scene.apply_delta([extracted_div(1, &[], x, y, w, h, style, Some(green))], []);
        let side = (64.0 * factor) as u32;
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0, 64.0],
            [side, side],
            factor,
        );
        assert_one_pixel_rim(&pixels, circle, Painted::Inside);
    }
}

#[test]
fn rounded_host_texture_edge_stays_one_device_pixel_under_scale_and_fractional_dpi() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    // The box's left edge at device x = 3.75: the pixel centred a quarter
    // pixel outside, past the fitted dest quad, keeps its quarter coverage.
    let fractional = ("150%, fractional edge", [2.5, 2.5, 30.0, 30.0], 1.0, 1.5);
    for circle @ (_, [x, y, w, h], scale, factor) in SCALED_CIRCLES.into_iter().chain([fractional])
    {
        let mut context = AppContext::new();
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).transform = scale_transform(scale);
        let preview = context
            .create_component(
                DocumentId::new(1).unwrap(),
                GpuTextureView::new("layer")
                    .style(style)
                    .with_corner_radius(w.min(h) * 0.5),
            )
            .unwrap();
        let mut layout = MutationQueue::new();
        write_box(&mut layout, preview.stable_id(), x, y, w, h);
        context.commit_mutations(layout).unwrap();
        let scene = commit_scene(&mut context);
        let side = (64.0 * factor) as u32;
        let (target, target_view) = test_copy_target(&device, format, side, side);
        let mut encoder = device.create_command_encoder(&Default::default());
        SceneWgpuPainter::for_test(format)
            .paint_encoder(
                &scene,
                &mut encoder,
                &target_view,
                ScenePaintViewport {
                    logical_size: [64.0, 64.0],
                    physical_size: [side, side],
                    scale_factor: factor,
                    scene_origin: [0.0, 0.0],
                    target_origin: [0.0, 0.0],
                    clear_color: [0.0, 0.0, 0.0, 1.0],
                    clear: true,
                },
                Some(&registry),
                None,
            )
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &target, side, side);
        assert_one_pixel_rim(&pixels, circle, Painted::Inside);
    }
    drop(view);
}

/// Signed device px from `p` to the convex polygon `corners`: the exact
/// distance a one device pixel ramp is measured against.
fn convex_polygon_distance(p: [f32; 2], corners: &[[f32; 2]]) -> f32 {
    let n = corners.len();
    let (mut outside, mut inside, mut nearest_side) = (f32::INFINITY, true, f32::INFINITY);
    let orientation = (0..n)
        .map(|i| {
            let [a, b] = [corners[i], corners[(i + 1) % n]];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        .signum();
    for i in 0..n {
        let [a, b] = [corners[i], corners[(i + 1) % n]];
        let side = [b[0] - a[0], b[1] - a[1]];
        let to_p = [p[0] - a[0], p[1] - a[1]];
        let length = side[0].hypot(side[1]);
        let t = ((to_p[0] * side[0] + to_p[1] * side[1]) / (length * length)).clamp(0.0, 1.0);
        outside = outside.min((to_p[0] - side[0] * t).hypot(to_p[1] - side[1] * t));
        let across = (side[0] * to_p[1] - side[1] * to_p[0]) * orientation / length;
        inside &= across >= 0.0;
        nearest_side = nearest_side.min(across.abs());
    }
    if inside { -nearest_side } else { outside }
}

/// The local point `mat` projects onto `screen`, and there the Jacobian of
/// the projection (columns: screen per local x, per local y), by Newton.
fn unproject(mat: nana_ui_core::PaintMat4, screen: [f32; 2]) -> ([f32; 2], [[f32; 2]; 2]) {
    let jacobian = |[x, y]: [f32; 2]| {
        let step = 1e-2;
        let at = mat.project_xy(x, y).unwrap();
        let along_x = mat.project_xy(x + step, y).unwrap();
        let along_y = mat.project_xy(x, y + step).unwrap();
        (
            at,
            [
                [(along_x[0] - at[0]) / step, (along_x[1] - at[1]) / step],
                [(along_y[0] - at[0]) / step, (along_y[1] - at[1]) / step],
            ],
        )
    };
    let mut local = screen;
    for _ in 0..30 {
        let (at, [jx, jy]) = jacobian(local);
        let [rx, ry] = [screen[0] - at[0], screen[1] - at[1]];
        let det = jx[0] * jy[1] - jy[0] * jx[1];
        local[0] += (jy[1] * rx - jy[0] * ry) / det;
        local[1] += (jx[0] * ry - jx[1] * rx) / det;
    }
    (local, jacobian(local).1)
}

#[test]
fn rounded_edges_stay_one_device_pixel_under_perspective() {
    // A round quad and a round texture under `perspective(120px) rotateY`:
    // interpolated affinely per triangle, the shape bends along the quad's
    // diagonal. Each pixel near the rim is compared with a one device pixel
    // ramp over the local distance through the projection's Jacobian there.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    let green = [0.0, 1.0, 0.0, 1.0];
    for degrees in [30f32, 55.0] {
        let mat = nana_ui_core::PaintMat4::perspective(120.0)
            .expect("finite distance")
            .then(nana_ui_core::PaintMat4::rotate_y(degrees.to_radians()));
        let mut quad = colored_quad_node(1, 16.0, 16.0, 32.0, 32.0, green);
        let layout = Arc::make_mut(&mut quad.source_style.layout);
        layout.border_radius = Some(16.0);
        layout.transform_3d = Some(mat);
        let mut quad_scene = UiScene::new();
        quad_scene.apply_delta([quad], []);
        let mut context = AppContext::new();
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).transform_3d = Some(mat);
        let preview = context
            .create_component(
                DocumentId::new(1).unwrap(),
                GpuTextureView::new("layer")
                    .style(style)
                    .with_corner_radius(16.0),
            )
            .unwrap();
        let mut layout = MutationQueue::new();
        write_box(&mut layout, preview.stable_id(), 16.0, 16.0, 32.0, 32.0);
        context.commit_mutations(layout).unwrap();
        let texture_scene = commit_scene(&mut context);
        let projection = mat.around_origin(16.0, 16.0, 16.0, 16.0);
        for (kind, scene) in [("quad", &quad_scene), ("texture", &texture_scene)] {
            let (target, target_view) = test_copy_target(&device, format, 64, 64);
            let mut encoder = device.create_command_encoder(&Default::default());
            SceneWgpuPainter::for_test(format)
                .paint_encoder(
                    scene,
                    &mut encoder,
                    &target_view,
                    ScenePaintViewport {
                        logical_size: [64.0, 64.0],
                        physical_size: [64, 64],
                        scale_factor: 1.0,
                        scene_origin: [0.0, 0.0],
                        target_origin: [0.0, 0.0],
                        clear_color: [0.0, 0.0, 0.0, 1.0],
                        clear: true,
                    },
                    Some(&registry),
                    None,
                )
                .unwrap();
            let pixels = readback_rgba(&device, &queue, encoder, &target, 64, 64);
            for py in 0..64 {
                for px in 0..64 {
                    let ([lx, ly], [jx, jy]) =
                        unproject(projection, [px as f32 + 0.5, py as f32 + 0.5]);
                    let [vx, vy] = [lx - 32.0, ly - 32.0];
                    let length = vx.hypot(vy).max(1e-6);
                    let [nx, ny] = [vx / length, vy / length];
                    // The rim distance's screen gradient: J⁻ᵀ n.
                    let det = jx[0] * jy[1] - jy[0] * jx[1];
                    let gradient =
                        ((jy[1] * nx - jx[1] * ny) / det).hypot((jx[0] * ny - jy[0] * nx) / det);
                    let outside = (length - 16.0) / gradient;
                    if outside.abs() > 1.2 {
                        continue;
                    }
                    let expected = (0.5 - outside).clamp(0.0, 1.0) * 255.0;
                    let green = f32::from(pixel(&pixels, 64, px, py)[1]);
                    assert!(
                        (green - expected).abs() <= 8.0,
                        "{kind} under rotateY({degrees}deg): ({px},{py}) is {outside:+.3} px \
                         from the rim, expected green {expected:.0}, got {green}"
                    );
                }
            }
        }
    }
    drop(view);
}

#[test]
fn square_edges_stay_one_device_pixel_under_any_affine_transform() {
    // A square green quad and a square green texture filling the same box,
    // off the pixel grid, rotated, 10:1 anisotropic, skewed to a 30° corner,
    // and at 150%. Every pixel near the edge, corners included, is compared
    // with a one device pixel ramp over its exact distance to the shape.
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    let green = [0.0, 1.0, 0.0, 1.0];
    let turn = |degrees: f32, sx: f32, sy: f32| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        [cos * sx, sin * sx, -sin * sy, cos * sy]
    };
    let cases = [
        ("scale(0.45)", [0.0, 0.0, 64.0], turn(0.0, 0.45, 0.45), 1.0),
        (
            "rotate(30deg)",
            [16.0, 16.0, 32.0],
            turn(30.0, 1.0, 1.0),
            1.0,
        ),
        (
            "rotate(10deg) scale(3, 0.3)",
            [22.0, 22.0, 20.0],
            turn(10.0, 3.0, 0.3),
            1.0,
        ),
        (
            "skewX(60deg)",
            [22.4, 22.4, 20.0],
            [1.0, 0.0, 60f32.to_radians().tan(), 1.0],
            1.0,
        ),
        ("150%", [2.5, 2.5, 30.0], turn(0.0, 1.0, 1.0), 1.5),
    ];
    for (label, [x, y, w], [a, b, c, d], factor) in cases {
        let transform = PaintTransform {
            a,
            b,
            c,
            d,
            ..PaintTransform::default()
        };
        let [cx, cy, h] = [x + w * 0.5, y + w * 0.5, w * 0.5];
        let corners = [[-h, -h], [h, -h], [h, h], [-h, h]]
            .map(|[u, v]| [(cx + a * u + c * v) * factor, (cy + b * u + d * v) * factor]);
        let style = nana_ui_core::LayoutStyle {
            background: Some(green),
            transform: Some(transform),
            ..Default::default()
        };
        let mut quad = UiScene::new();
        quad.apply_delta([extracted_div(1, &[], x, y, w, w, style, Some(green))], []);
        let mut context = AppContext::new();
        let mut node_style = NodeStyle::default();
        Arc::make_mut(&mut node_style.layout).transform = Some(transform);
        let preview = context
            .create_component(
                DocumentId::new(1).unwrap(),
                GpuTextureView::new("layer").style(node_style),
            )
            .unwrap();
        let mut layout = MutationQueue::new();
        write_box(&mut layout, preview.stable_id(), x, y, w, w);
        context.commit_mutations(layout).unwrap();
        let texture = commit_scene(&mut context);
        // A quad under a pure translation snaps to whole pixels by design;
        // only the texture keeps its box's fractional edge there.
        let translated = [a, b, c, d] == [1.0, 0.0, 0.0, 1.0];
        let scenes = [("quad", &quad), ("texture", &texture)];
        for (kind, scene) in scenes.into_iter().skip(usize::from(translated)) {
            let side = (64.0 * factor) as u32;
            let (target, target_view) = test_copy_target(&device, format, side, side);
            let mut encoder = device.create_command_encoder(&Default::default());
            SceneWgpuPainter::for_test(format)
                .paint_encoder(
                    scene,
                    &mut encoder,
                    &target_view,
                    ScenePaintViewport {
                        logical_size: [64.0, 64.0],
                        physical_size: [side, side],
                        scale_factor: factor,
                        scene_origin: [0.0, 0.0],
                        target_origin: [0.0, 0.0],
                        clear_color: [0.0, 0.0, 0.0, 1.0],
                        clear: true,
                    },
                    Some(&registry),
                    None,
                )
                .unwrap();
            let pixels = readback_rgba(&device, &queue, encoder, &target, side, side);
            for py in 0..side {
                for px in 0..side {
                    let center = [px as f32 + 0.5, py as f32 + 0.5];
                    let outside = convex_polygon_distance(center, &corners);
                    if outside.abs() > 1.5 {
                        continue;
                    }
                    let expected = (0.5 - outside).clamp(0.0, 1.0) * 255.0;
                    let green = f32::from(pixel(&pixels, side, px, py)[1]);
                    assert!(
                        (green - expected).abs() <= 8.0,
                        "{kind} under {label}: ({px},{py}) is {outside:+.3} px from the edge, \
                         expected green {expected:.0}, got {green}"
                    );
                }
            }
        }
    }
    drop(view);
}

#[test]
fn unblurred_shadow_and_outline_edges_stay_one_device_pixel_under_transform_scale() {
    // Each circle is the outer edge of a band `w / 8` wide around a smaller
    // round box: a black box's spread shadow, or a green box's green outline.
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let green = [0.0, 1.0, 0.0, 1.0];
    for outline in [false, true] {
        for circle @ (_, [x, y, w, h], scale, factor) in SCALED_CIRCLES {
            let band = w / 8.0;
            let (fill, paint) = if outline {
                let outline = nana_ui_core::OutlineSpec {
                    width: band,
                    color: Some(green),
                    style: nana_ui_core::OutlineStyle::Solid,
                };
                let paint = nana_ui_core::PaintStyle {
                    outline,
                    ..Default::default()
                };
                (green, paint)
            } else {
                let shadow = nana_ui_core::BoxShadowSpec {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur_radius: 0.0,
                    spread_radius: band,
                    color: green,
                    inset: false,
                };
                let paint = nana_ui_core::PaintStyle {
                    box_shadows: vec![shadow],
                    ..Default::default()
                };
                ([0.0, 0.0, 0.0, 1.0], paint)
            };
            let layout = nana_ui_core::LayoutStyle {
                background: Some(fill),
                border_radius: Some(w.min(h) * 0.5 - band),
                transform: scale_transform(scale),
                paint: paint.into(),
                ..Default::default()
            };
            let [x, y, w, h] = [x + band, y + band, w - band * 2.0, h - band * 2.0];
            let node = extracted_div(1, &[], x, y, w, h, layout, Some(fill));
            let mut scene = UiScene::new();
            scene.apply_delta([node], []);
            let side = (64.0 * factor) as u32;
            let pixels = paint_scene_rgba(
                &device,
                &queue,
                &mut painter,
                &scene,
                [64.0, 64.0],
                [side, side],
                factor,
            );
            assert_one_pixel_rim(&pixels, circle, Painted::Inside);
        }
    }
}

#[test]
fn inset_shadow_spread_shrinks_its_shape_inside_the_box() {
    // CSS `inset 0 0 0 <spread>`: a band that wide inside each round black
    // box, so the circle is the box's shrunk by the spread, painted outside.
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let black = [0.0, 0.0, 0.0, 1.0];
    for (label, [x, y, w, h], scale, factor) in SCALED_CIRCLES {
        let spread = w / 8.0;
        let shadow = nana_ui_core::BoxShadowSpec {
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 0.0,
            spread_radius: spread,
            color: [0.0, 1.0, 0.0, 1.0],
            inset: true,
        };
        let layout = nana_ui_core::LayoutStyle {
            background: Some(black),
            border_radius: Some(w.min(h) * 0.5),
            transform: scale_transform(scale),
            paint: nana_ui_core::PaintStyle {
                box_shadows: vec![shadow],
                ..Default::default()
            }
            .into(),
            ..Default::default()
        };
        let node = extracted_div(1, &[], x, y, w, h, layout, Some(black));
        let mut scene = UiScene::new();
        scene.apply_delta([node], []);
        let side = (64.0 * factor) as u32;
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0, 64.0],
            [side, side],
            factor,
        );
        let hole = [x + spread, y + spread, w - spread * 2.0, h - spread * 2.0];
        assert_one_pixel_rim(&pixels, (label, hole, scale, factor), Painted::Outside);
    }
}

/// A black box at `[x, y, w, h]` with one green `box-shadow`, in a 64 px target.
fn paint_box_shadow(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    [x, y, w, h]: [f32; 4],
    blur_radius: f32,
    spread_radius: f32,
    inset: bool,
) -> Vec<u8> {
    let black = [0.0, 0.0, 0.0, 1.0];
    let shadow = nana_ui_core::BoxShadowSpec {
        offset_x: 0.0,
        offset_y: 0.0,
        blur_radius,
        spread_radius,
        color: [0.0, 1.0, 0.0, 1.0],
        inset,
    };
    let layout = nana_ui_core::LayoutStyle {
        background: Some(black),
        paint: nana_ui_core::PaintStyle {
            box_shadows: vec![shadow],
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta([extracted_div(1, &[], x, y, w, h, layout, Some(black))], []);
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    paint_scene_rgba(
        device,
        queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    )
}

#[test]
fn blurred_shadow_is_full_strength_its_blur_inside_its_edge() {
    // A 4 px blur: full strength 4 px inside the shape's edge, half on it,
    // none 4 px past it. Spread 12 puts the outer shape's left edge at x 8
    // and the inset one's at x 20.
    let (device, queue) = test_device();
    let outer = paint_box_shadow(&device, &queue, [20.0, 20.0, 24.0, 24.0], 4.0, 12.0, false);
    let inset = paint_box_shadow(&device, &queue, [8.0, 8.0, 48.0, 48.0], 4.0, 12.0, true);
    // Each shape's left edge, and which way from it is away from the paint.
    for (label, pixels, columns, edge, away) in [
        ("outer", &outer, 0..20, 8.0, -1.0),
        ("inset", &inset, 9..32, 20.0, 1.0),
    ] {
        for px in columns {
            let t = (((px as f32 + 0.5 - edge) * away + 4.0) / 8.0).clamp(0.0, 1.0);
            let expected = (1.0 - t * t * (3.0 - 2.0 * t)) * 255.0;
            let green = f32::from(pixel(pixels, 64, px, 32)[1]);
            assert!(
                (green - expected).abs() <= 3.0,
                "{label}: x {px} expected green {expected:.0}, got {green}"
            );
        }
    }
}

#[test]
fn a_shadow_thinner_than_its_blur_peaks_below_full_strength() {
    // 2.5 px above a 2 px strip with an 8 px blur: the ramp's 0.273 of the
    // Gaussian's peak, erf(2 / (√2·8)) ≈ 0.197.
    let (device, queue) = test_device();
    let pixels = paint_box_shadow(&device, &queue, [8.0, 31.0, 48.0, 2.0], 8.0, 0.0, false);
    let expected = 0.197 * 0.273 * 255.0;
    let green = f32::from(pixel(&pixels, 64, 32, 28)[1]);
    assert!(
        (green - expected).abs() <= 3.0,
        "expected green {expected:.0}, got {green}"
    );
}

#[test]
fn a_blurred_shadow_snaps_as_its_box_does() {
    // A 1.5 px blur grows the quad to a half-pixel edge; snapping that edge
    // slid the 10..30 box half a pixel, with the shadow cut to it.
    let (device, queue) = test_device();
    for inset in [false, true] {
        let pixels = paint_box_shadow(&device, &queue, [10.0, 10.0, 20.0, 20.0], 1.5, 0.0, inset);
        let green = |x| pixel(&pixels, 64, x, 20)[1];
        let row = (4..36).map(green).collect::<Vec<_>>();
        // Symmetric about the box, and clear on the side it does not paint.
        assert!(
            green(9).max(green(10)) > 0
                && (4..36).all(|x| {
                    green(x).abs_diff(green(39 - x)) <= 1
                        && ((10..30).contains(&x) == inset || green(x) == 0)
                }),
            "inset {inset}: x 4..36 green {row:?}"
        );
    }
}

fn hosted_preview_scene(device: &wgpu::Device) -> (UiScene, HostTextureRegistry) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, RuntimeButton::new("刷新预览"))
        .unwrap();
    let preview = context
        .create_component(document, nana_ui_runtime::GpuTextureView::new("preview"))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 8.0,
            y: 8.0,
            width: 112.0,
            height: 28.0,
        },
    );
    layout.write_layout(
        preview.stable_id(),
        LayoutBox {
            x: 8.0,
            y: 40.0,
            width: 112.0,
            height: 48.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    let work = context.take_system_work();
    context.resolve_styles(&work.style).unwrap();
    let mut scene = UiScene::new();
    scene.apply_delta(
        context.world().extract_nodes(&work.render_extraction),
        work.render_removals,
    );
    let texture = crate::HostTexture::new(
        1,
        1,
        &crate::test_gpu::wrap_view(&test_target(
            device,
            wgpu::TextureFormat::Rgba8Unorm,
            64,
            48,
        )),
    );
    let registry = HostTextureRegistry::new();
    registry.register(
        "preview",
        texture,
        64,
        48,
        crate::HostTextureAlphaMode::Premultiplied,
    );
    (scene, registry)
}

/// A GPU view, with a label beside it when `labelled`: the scene cannot be
/// reblitted, so every frame draws, and an unchanged one reuses last frame's
/// batch without flushing the text (#230). Each call of the returned frame
/// paints once and says whether it rebuilt the batch and how many index
/// bytes it wrote.
fn beside_a_gpu_view(
    labelled: bool,
) -> (
    SceneWgpuPainter,
    impl FnMut(&mut SceneWgpuPainter) -> (bool, u64),
) {
    use crate::DefaultGpuViewRenderer;
    use nana_ui_runtime::GpuView;

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let painter = SceneWgpuPainter::for_test(format);
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("gpu-view", Arc::new(DefaultGpuViewRenderer::new()));
    let mut view = host_texture_child(2, 1, 0.0, 0.0, 32.0, 32.0, "0");
    view.custom_render = Some(GpuView::new(0).custom_render());
    let mut root = colored_quad_node(1, 0.0, 0.0, 128.0, 96.0, [0.0, 0.0, 1.0, 1.0]);
    let mut nodes = vec![view];
    if labelled {
        nodes.push(overflowing_text_child(
            3, 1, 0.0, 32.0, 128.0, 64.0, [1.0; 4],
        ));
    }
    root.children = Arc::new(nodes.iter().map(|node| node.id).collect());
    nodes.push(root);
    let mut scene = UiScene::new();
    scene.apply_delta(nodes, []);
    let (_, target) = test_copy_target(&device, format, 128, 96);
    let viewport = ScenePaintViewport {
        logical_size: [128.0, 96.0],
        physical_size: [128, 96],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let frame = move |painter: &mut SceneWgpuPainter| {
        let before = painter.text_glyph_counters();
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                &scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        queue.submit([encoder.finish()]);
        let after = painter.text_glyph_counters();
        assert_eq!(
            after.text_pipeline_draws > before.text_pipeline_draws,
            labelled,
            "the label is drawn every frame, and nothing else is text"
        );
        (
            painter.last_gpu_work().expect("painted").batch_rebuilds > 0,
            after.text_index_upload_bytes - before.text_index_upload_bytes,
        )
    };
    (painter, frame)
}

#[test]
fn a_reused_batch_gives_back_the_gaps_of_text_that_stopped_growing() {
    let (mut painter, mut paint) = beside_a_gpu_view(true);
    paint(&mut painter);
    assert_eq!(
        paint(&mut painter),
        (false, 0),
        "an unchanged frame reuses its batch"
    );
    painter.text.assume_gapped_order();
    let mut reused = 0;
    let settled = loop {
        let frame = paint(&mut painter);
        if frame != (false, 0) {
            break frame;
        }
        reused += 1;
        assert!(reused < 1_000, "the gaps were never given back");
    };
    assert!(
        reused + 2 >= text::SETTLE_FRAMES,
        "only once the text has stood still: {reused} frames"
    );
    assert!(
        settled.0,
        "the frame that gives them back rebuilds its batch"
    );
    assert!(settled.1 > 0, "and rewrites the index table");
    assert!(!painter.text.order_gapped());
    assert_eq!(
        paint(&mut painter),
        (false, 0),
        "after which the batch is reused again"
    );
}

#[test]
fn a_reused_batch_without_text_is_not_rebuilt_for_gaps() {
    // The order held gaps when the text went away. Nothing draws them, so
    // there is nothing to give back, and the batch must keep being reused
    // rather than rebuilt every frame for a flush with no text to repack.
    let (mut painter, mut paint) = beside_a_gpu_view(false);
    painter.text.assume_gapped_order();
    let rebuilds = (0..300).filter(|_| paint(&mut painter).0).count();
    assert_eq!(rebuilds, 1, "only the first frame builds a batch");
}

fn runtime_button_over_host_texture_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> (UiScene, HostTextureRegistry, wgpu::TextureView, [f32; 4]) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let background = context
        .create_component(
            document,
            RuntimeButton::new("").style(opaque_fill_style(SemanticColorRole::Danger)),
        )
        .unwrap();
    let preview = context
        .create_component(document, GpuTextureView::new("layer"))
        .unwrap();
    let overlay = context
        .create_component(
            document,
            RuntimeButton::new("")
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, background.stable_id(), 0.0, 0.0, 64.0, 64.0);
    write_box(&mut layout, preview.stable_id(), 0.0, 16.0, 64.0, 32.0);
    write_box(&mut layout, overlay.stable_id(), 8.0, 28.0, 48.0, 24.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let overlay_fill = selected_button_fill(&context, overlay.stable_id());
    let view = solid_texture_view(device, queue, format, 64, 32, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 32);
    (scene, registry, view, overlay_fill)
}

fn two_host_texture_layers_with_chrome_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> (UiScene, HostTextureRegistry, [wgpu::TextureView; 2]) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let background = context
        .create_component(document, GpuTextureView::new("bg"))
        .unwrap();
    let chrome = context
        .create_component(
            document,
            RuntimeButton::new("")
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let foreground = context
        .create_component(document, GpuTextureView::new("fg"))
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, background.stable_id(), 0.0, 0.0, 64.0, 64.0);
    write_box(&mut layout, chrome.stable_id(), 8.0, 16.0, 48.0, 24.0);
    write_box(&mut layout, foreground.stable_id(), 16.0, 28.0, 32.0, 28.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let fill = selected_button_fill(&context, chrome.stable_id());
    assert!(
        fill[3] > 0.99,
        "Selected chrome Button fill must be opaque, got {fill:?}"
    );
    let bg = solid_texture_view(device, queue, format, 64, 64, wgpu::Color::BLUE);
    let fg = solid_texture_view(device, queue, format, 32, 28, wgpu::Color::RED);
    let registry = HostTextureRegistry::new();
    register_into(&registry, "bg", &bg, 64, 64);
    register_into(&registry, "fg", &fg, 32, 28);
    (scene, registry, [bg, fg])
}

/// A labeled button under a rotation, so its text takes the affine glyph
/// path rather than the axis-aligned instanced one.
fn rotated_label_scene() -> UiScene {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let k = std::f32::consts::FRAC_1_SQRT_2;
    let button = context
        .create_component(
            document,
            RuntimeButton::new("Hi")
                .kind(ButtonKind::Selected)
                .layout(Arc::new(nana_ui_core::LayoutStyle {
                    transform: Some(PaintTransform {
                        a: k,
                        b: k,
                        c: -k,
                        d: k,
                        ..PaintTransform::default()
                    }),
                    ..(*square_button_layout()).clone()
                })),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, button.stable_id(), 8.0, 16.0, 48.0, 32.0);
    context.commit_mutations(layout).unwrap();
    commit_scene(&mut context)
}

#[test]
fn text_counters_see_every_window_and_survive_one_closing() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = rotated_label_scene();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (_first, first_view) = test_copy_target(&device, format, 64, 64);
    let (_second, second_view) = test_copy_target(&device, format, 64, 64);
    let paint = |painter: &mut SceneWgpuPainter, id: u64, view: &wgpu::TextureView| {
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_target_encoder(
                RenderTargetId(id),
                &scene.clone(),
                &mut encoder,
                view,
                viewport,
                None,
                None,
            )
            .unwrap();
        queue.submit([encoder.finish()]);
    };
    paint(&mut painter, 1, &first_view);
    let one = painter.text_glyph_counters();
    paint(&mut painter, 2, &second_view);
    let two = painter.text_glyph_counters();
    // The retained half of the counters lives per window. Read through the
    // painter, a second window's entries and rebuilds are not invisible just
    // because the last `paint_target` put the first one's state away.
    assert_eq!(one.text_gpu_entries_active, 1);
    assert_eq!(
        two.text_gpu_entries_active, 2,
        "each window holds its own entry"
    );
    assert_eq!(
        two.text_instance_rebuilds,
        one.text_instance_rebuilds + 1,
        "and the second window's build is counted"
    );
    assert_eq!(
        two.glyph_rasterized, one.glyph_rasterized,
        "while the glyphs themselves are shared"
    );
    painter.remove_target(RenderTargetId(1));
    let closed = painter.text_glyph_counters();
    assert_eq!(
        closed.text_gpu_entries_active, 1,
        "the closed window's entry is gone"
    );
    assert_eq!(
        closed.text_instance_rebuilds, two.text_instance_rebuilds,
        "but what it did is still counted: totals never run backwards"
    );
    assert_eq!(
        closed.text_gpu_entries_destroyed,
        two.text_gpu_entries_destroyed + 1
    );
}

#[test]
fn affine_text_allocates_no_gpu_resources_across_repaints_with_identical_pixels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let scene = rotated_label_scene();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);

    let paint_once = |painter: &mut SceneWgpuPainter, scene: &UiScene| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui affine cache test"),
        });
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        queue.submit([encoder.finish()]);
        readback_rgba(
            &device,
            &queue,
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nana-ui affine cache readback"),
            }),
            &texture,
            64,
            64,
        )
    };

    let first = paint_once(&mut painter, &scene);
    let warm = painter.text_glyph_counters();
    assert!(
        warm.glyph_rasterized > 0,
        "the first paint of a rotated label must rasterize its glyphs"
    );
    assert!(
        warm.glyph_upload_regions > 0,
        "and upload them to the atlas"
    );

    // Clones carry the same label under a fresh scene instance, so every frame
    // is rebatched and the shared caches are what have to absorb it. A rotated
    // label used to allocate a texture, a bind group and a vertex buffer per
    // distinct transform; now it re-rasterizes nothing, re-uploads nothing and
    // allocates nothing.
    for _ in 0..4 {
        let repaint = paint_once(&mut painter, &scene.clone());
        assert_eq!(
            first, repaint,
            "reused glyph atlas entries must produce identical pixels"
        );
        assert_eq!(
            painter
                .last_gpu_work()
                .expect("encoded frame")
                .gpu_buffer_reallocations,
            0,
            "static affine text must not create GPU resources per frame"
        );
    }
    let steady = painter.text_glyph_counters();
    assert_eq!(
        steady.glyph_rasterized, warm.glyph_rasterized,
        "a repainted rotated label must not re-rasterize a glyph"
    );
    assert_eq!(
        steady.glyph_upload_regions, warm.glyph_upload_regions,
        "nor re-upload one"
    );
    assert_eq!(
        steady.glyph_resolve_requests, warm.glyph_resolve_requests,
        "a repaint must not resolve a glyph: the entry already holds them"
    );
    assert!(
        steady.text_gpu_entries_reused >= warm.text_gpu_entries_reused + 4,
        "every repaint must be answered by the retained entry"
    );
    assert_eq!(
        steady.text_instance_rebuilds, warm.text_instance_rebuilds,
        "and rebuild no instance"
    );
    assert_eq!(steady.glyph_atlas_evict, 0);
    assert_eq!(steady.atlas_stale_handle_rejects, 0);
}

fn labeled_selected_button_scene() -> UiScene {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            RuntimeButton::new("Hi")
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, button.stable_id(), 8.0, 16.0, 48.0, 32.0);
    context.commit_mutations(layout).unwrap();
    commit_scene(&mut context)
}

fn nested_rotated_overflow_probe(clips: &[ClipRegion]) -> (u32, u32) {
    let origin = super::clip::PaintOrigin::from(super::clip::paint_origin([0.0, 0.0], [0.0, 0.0]));
    let aabb = super::clip::intersect_clips(
        super::clip::LogicalRect::viewport([0.0, 0.0], [64.0, 64.0]),
        clips,
        origin,
    )
    .unwrap();
    let rotated = super::clip::rotated_fragment_clips(clips, origin);
    let inner = *rotated.last().expect("nested probe needs a rotated clip");
    for y in 0..64u32 {
        for x in 0..64u32 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if px >= aabb.x
                && py >= aabb.y
                && px < aabb.x + aabb.width
                && py < aabb.y + aabb.height
                && super::clip::point_in_fragment_clip(px, py, inner)
                && !super::clip::point_in_fragment_clips(px, py, &rotated)
            {
                return (x, y);
            }
        }
    }
    panic!(
        "nested clips must include a pixel inside the inner parallelogram but outside the outer"
    );
}

fn aabb_outside_rotated_overflow_probe() -> (u32, u32) {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    let clips = [ClipRegion {
        bounds: SceneRect {
            x: 16.0,
            y: 16.0,
            width: 32.0,
            height: 32.0,
        },
        transform: AffineTransform::from_matrix(
            PaintTransform {
                a: k,
                b: k,
                c: -k,
                d: k,
                ..PaintTransform::default()
            }
            .around_center(16.0, 16.0, 32.0, 32.0),
        ),
        corner_radius: 0.0,
        polygon_clip: None,
    }];
    let origin = super::clip::PaintOrigin::from(super::clip::paint_origin([0.0, 0.0], [0.0, 0.0]));
    let aabb = super::clip::intersect_clips(
        super::clip::LogicalRect::viewport([0.0, 0.0], [64.0, 64.0]),
        &clips,
        origin,
    )
    .unwrap();
    let frag = super::clip::fragment_clip(&clips, origin);
    for y in 8..24 {
        for x in 8..24 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            if px >= aabb.x
                && py >= aabb.y
                && px < aabb.x + aabb.width
                && py < aabb.y + aabb.height
                && !super::clip::point_in_fragment_clip(px, py, frag)
            {
                return (x, y);
            }
        }
    }
    panic!("sibling must overlap a pixel inside the rotated AABB but outside the parallelogram");
}

fn rotated_overflow_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> ExtractedNode {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    overflow_parent(
        value,
        children,
        x,
        y,
        width,
        height,
        Some(PaintTransform {
            a: k,
            b: k,
            c: -k,
            d: k,
            ..PaintTransform::default()
        }),
    )
}

fn overflow_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    transform: Option<PaintTransform>,
) -> ExtractedNode {
    ExtractedNode {
        custom_paint: None,
        chrome_radii: nana_ui_core::ChromeRadii::default(),
        id: StableNodeId::new(value).unwrap(),
        kind: Arc::new(NodeKind::Element { tag: "div".into() }),
        parent: None,
        children: Arc::new(
            children
                .iter()
                .copied()
                .map(|child| StableNodeId::new(child).unwrap())
                .collect(),
        ),
        layout: LayoutBox {
            x,
            y,
            width,
            height,
        },
        scroll_offset: nana_ui_runtime::ScrollOffset::default(),
        source_style: NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                overflow_x: OverflowSpec::Hidden,
                overflow_y: OverflowSpec::Hidden,
                transform,
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
        style: Arc::new(ComputedStyle::default()),
        text: None,
        text_metrics: None,
        text_layout: None,
        text_preserve_lines: false,
        z_index: 0,
        focused: false,
        editable: false,
        text_spans: Vec::new(),
        standard_visual: None,
        component_geometry: None,
        standard_visual_foreground: None,
        custom_render: None,
        drop_hover: None,
        document_text_selection: Vec::new(),
        document_text_selection_color: [0.0; 4],
        compositor: Default::default(),
    }
}

fn translucent_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    opacity: f32,
) -> ExtractedNode {
    ExtractedNode {
        custom_paint: None,
        chrome_radii: nana_ui_core::ChromeRadii::default(),
        id: StableNodeId::new(value).unwrap(),
        kind: Arc::new(NodeKind::Element { tag: "div".into() }),
        parent: None,
        children: Arc::new(
            children
                .iter()
                .copied()
                .map(|child| StableNodeId::new(child).unwrap())
                .collect(),
        ),
        layout: LayoutBox {
            x,
            y,
            width,
            height,
        },
        scroll_offset: nana_ui_runtime::ScrollOffset::default(),
        source_style: NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                opacity: Some(opacity),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
        style: Arc::new(ComputedStyle::default()),
        text: None,
        text_metrics: None,
        text_layout: None,
        text_preserve_lines: false,
        z_index: 0,
        focused: false,
        editable: false,
        text_spans: Vec::new(),
        standard_visual: None,
        component_geometry: None,
        standard_visual_foreground: None,
        custom_render: None,
        drop_hover: None,
        document_text_selection: Vec::new(),
        document_text_selection_color: [0.0; 4],
        compositor: Default::default(),
    }
}

fn overflowing_text_child(
    value: u64,
    parent: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) -> ExtractedNode {
    ExtractedNode {
        custom_paint: None,
        chrome_radii: nana_ui_core::ChromeRadii::default(),
        id: StableNodeId::new(value).unwrap(),
        kind: Arc::new(NodeKind::Text),
        parent: Some(StableNodeId::new(parent).unwrap()),
        children: Arc::new(Vec::new()),
        layout: LayoutBox {
            x,
            y,
            width,
            height,
        },
        scroll_offset: nana_ui_runtime::ScrollOffset::default(),
        source_style: NodeStyle::default(),
        style: Arc::new(ComputedStyle {
            color: Some(color),
            font_size: 64.0,
            ..ComputedStyle::default()
        }),
        text: Some(TextContent {
            value: "██".into()
        }),
        text_metrics: None,
        text_layout: None,
        text_preserve_lines: false,
        z_index: 0,
        focused: false,
        editable: false,
        text_spans: Vec::new(),
        standard_visual: None,
        component_geometry: None,
        standard_visual_foreground: None,
        custom_render: None,
        drop_hover: None,
        document_text_selection: Vec::new(),
        document_text_selection_color: [0.0; 4],
        compositor: Default::default(),
    }
}

fn custom_render_child(
    value: u64,
    parent: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    renderer: &str,
) -> ExtractedNode {
    let mut node = host_texture_child(value, parent, x, y, width, height, "slot");
    node.custom_render = Some(CustomRenderNode::new(renderer, "slot", 1));
    node
}

fn host_texture_child(
    value: u64,
    parent: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    resource: &str,
) -> ExtractedNode {
    ExtractedNode {
        custom_paint: None,
        chrome_radii: nana_ui_core::ChromeRadii::default(),
        id: StableNodeId::new(value).unwrap(),
        kind: Arc::new(NodeKind::Element { tag: "div".into() }),
        parent: Some(StableNodeId::new(parent).unwrap()),
        children: Arc::new(Vec::new()),
        layout: LayoutBox {
            x,
            y,
            width,
            height,
        },
        scroll_offset: nana_ui_runtime::ScrollOffset::default(),
        source_style: NodeStyle::default(),
        style: Arc::new(ComputedStyle::default()),
        text: None,
        text_metrics: None,
        text_layout: None,
        text_preserve_lines: false,
        z_index: 0,
        focused: false,
        editable: false,
        text_spans: Vec::new(),
        standard_visual: None,
        component_geometry: None,
        standard_visual_foreground: None,
        custom_render: Some(CustomRenderNode::new("nana.host-texture", resource, 1)),
        drop_hover: None,
        document_text_selection: Vec::new(),
        document_text_selection_color: [0.0; 4],
        compositor: Default::default(),
    }
}

fn colored_quad_child(
    value: u64,
    parent: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) -> ExtractedNode {
    let mut node = colored_quad_node(value, x, y, width, height, color);
    node.parent = Some(StableNodeId::new(parent).unwrap());
    node
}

fn extracted_div(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    layout: nana_ui_core::LayoutStyle,
    background: Option<[f32; 4]>,
) -> ExtractedNode {
    ExtractedNode {
        custom_paint: None,
        chrome_radii: nana_ui_core::ChromeRadii::default(),
        id: StableNodeId::new(value).unwrap(),
        kind: Arc::new(NodeKind::Element { tag: "div".into() }),
        parent: None,
        children: Arc::new(
            children
                .iter()
                .copied()
                .map(|child| StableNodeId::new(child).unwrap())
                .collect(),
        ),
        layout: LayoutBox {
            x,
            y,
            width,
            height,
        },
        scroll_offset: nana_ui_runtime::ScrollOffset::default(),
        source_style: NodeStyle {
            layout: Arc::new(layout),
            ..NodeStyle::default()
        },
        style: Arc::new(ComputedStyle {
            background,
            ..ComputedStyle::default()
        }),
        text: None,
        text_metrics: None,
        text_layout: None,
        text_preserve_lines: false,
        z_index: 0,
        focused: false,
        editable: false,
        text_spans: Vec::new(),
        standard_visual: None,
        component_geometry: None,
        standard_visual_foreground: None,
        custom_render: None,
        drop_hover: None,
        document_text_selection: Vec::new(),
        document_text_selection_color: [0.0; 4],
        compositor: Default::default(),
    }
}

fn colored_quad_node(
    value: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) -> ExtractedNode {
    extracted_div(
        value,
        &[],
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            background: Some(color),
            ..nana_ui_core::LayoutStyle::default()
        },
        Some(color),
    )
}

fn commit_scene(context: &mut AppContext) -> UiScene {
    let work = context.take_system_work();
    context.resolve_styles(&work.style).unwrap();
    context
        .shape_text(&work.text, &mut crate::NanaTextShaper::default())
        .unwrap();
    let mut scene = UiScene::new();
    scene.apply_delta(
        context.world().extract_nodes(&work.render_extraction),
        work.render_removals,
    );
    scene
}

fn selected_button_fill(context: &AppContext, id: nana_ui_runtime::StableNodeId) -> [f32; 4] {
    let work = context.world().extract_nodes(&[id]);
    match work[0].component_geometry.as_deref() {
        Some(ComponentGeometry::Button {
            background: Some(fill),
            ..
        }) => *fill,
        other => panic!("expected opaque Button geometry fill, got {other:?}"),
    }
}

fn write_box(
    queue: &mut MutationQueue,
    id: nana_ui_runtime::StableNodeId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    queue.write_layout(
        id,
        LayoutBox {
            x,
            y,
            width,
            height,
        },
    );
}

fn square_button_layout() -> Arc<nana_ui_core::LayoutStyle> {
    Arc::new(nana_ui_core::LayoutStyle {
        border_radius: Some(0.0),
        border_width: Some(0.0),
        padding_left: Some(LengthSpec::Px(0.0)),
        padding_right: Some(LengthSpec::Px(0.0)),
        padding_top: Some(LengthSpec::Px(0.0)),
        padding_bottom: Some(LengthSpec::Px(0.0)),
        min_height: Some(LengthSpec::Px(0.0)),
        ..nana_ui_core::LayoutStyle::default()
    })
}

fn opaque_fill_style(background: SemanticColorRole) -> NodeStyle {
    NodeStyle {
        layout: square_button_layout(),
        background: Some(background),
        ..NodeStyle::default()
    }
}

fn register_host_texture(
    resource: &str,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
) -> HostTextureRegistry {
    let registry = HostTextureRegistry::new();
    register_into(&registry, resource, view, width, height);
    registry
}

fn register_into(
    registry: &HostTextureRegistry,
    resource: &str,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
) {
    registry.register(
        resource,
        crate::HostTexture::new(1, 1, &crate::test_gpu::wrap_view(view)),
        width,
        height,
        crate::HostTextureAlphaMode::Premultiplied,
    );
}

fn assert_mixed_gpu_pass_counts(painter: &SceneWgpuPainter) {
    let counts = painter
        .last_dest_pass_counts
        .expect("encoded frame records dest passes");
    assert_eq!(
        counts.color, 1,
        "HostTexture composition must stay in one dest color pass, not 1+N textures, got {counts:?}"
    );
    assert_eq!(
        counts.msaa, 0,
        "mixed GPU frames must not open an MSAA pass, got {counts:?}"
    );
    assert!(
        !counts.msaa_allocated,
        "mixed GPU frames must not allocate unused 4x MSAA, got {counts:?}"
    );
    assert_eq!(
        counts.blit, 1,
        "dest→window blit may be one extra pass, got {counts:?}"
    );
}

fn solid_texture_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    color: wgpu::Color,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui scene order fill"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui scene order fill"),
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("nana-ui scene order fill"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    queue.submit([encoder.finish()]);
    view
}

fn test_copy_target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui scene order target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn readback_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let unpadded = width as usize * 4;
    let padded = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui scene order readback"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })
        .expect("document-order readback poll");
    let mapped = slice
        .get_mapped_range()
        .expect("document-order readback must be mapped");
    let mut pixels = Vec::with_capacity(unpadded * height as usize);
    for row in mapped.chunks_exact(padded) {
        pixels.extend_from_slice(&row[..unpadded]);
    }
    pixels
}

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let index = ((y * width + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

fn is_green_slot(color: [u8; 4]) -> bool {
    color[1] > 180 && color[0] < 40 && color[2] < 40
}

fn is_blue_slot(color: [u8; 4]) -> bool {
    color[2] > 180 && color[0] < 40 && color[1] < 40
}

fn is_red_slot(color: [u8; 4]) -> bool {
    color[0] > 180 && color[1] < 40 && color[2] < 40
}

fn is_danger_fill(color: [u8; 4]) -> bool {
    color[0] > 150 && color[1] < 80 && color[2] < 80
}

fn test_target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui scene gpu work target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

pub(super) fn test_device() -> (wgpu::Device, wgpu::Queue) {
    crate::test_gpu::device()
}

fn paint_surface_quad_node(
    value: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    background: [f32; 4],
    surface: nana_ui_scene::QuadSurfacePaint,
) -> ExtractedNode {
    extracted_div(
        value,
        &[],
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            background: Some(background),
            paint: nana_ui_core::PaintStyle {
                background_image: surface.background_image.clone(),
                background_layers: surface.background_layers.clone(),
                content_image: surface.content_image.clone(),
                mask: surface.mask.clone(),
                border_image: surface.border_image.clone(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        Some(background),
    )
}

fn frost_quad_node_with_fill(
    value: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: [f32; 4],
    filter: nana_ui_core::BackdropFilter,
) -> ExtractedNode {
    extracted_div(
        value,
        &[],
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            background: Some(fill),
            paint: nana_ui_core::PaintStyle {
                backdrop_filter: Some(filter),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        Some(fill),
    )
}

fn clip_path_inset_round_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    inset_px: f32,
    round_px: f32,
) -> ExtractedNode {
    extracted_div(
        value,
        children,
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            border_radius: Some(0.0),
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Inset(nana_ui_core::ClipInset {
                    top: LengthSpec::Px(inset_px),
                    right: LengthSpec::Px(inset_px),
                    bottom: LengthSpec::Px(inset_px),
                    left: LengthSpec::Px(inset_px),
                    round: Some(LengthSpec::Px(round_px)),
                })),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        None,
    )
}

fn clip_path_circle_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> ExtractedNode {
    extracted_div(
        value,
        children,
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Circle(nana_ui_core::ClipCircle {
                    radius: nana_ui_core::ClipShapeRadius::ClosestSide,
                    cx: LengthSpec::Percent(50.0),
                    cy: LengthSpec::Percent(50.0),
                })),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        None,
    )
}

fn clip_path_polygon_parent(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> ExtractedNode {
    extracted_div(
        value,
        children,
        x,
        y,
        width,
        height,
        nana_ui_core::LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                clip_path: Some(nana_ui_core::ClipPath::Polygon(vec![
                    nana_ui_core::ClipPoint {
                        x: LengthSpec::Percent(0.0),
                        y: LengthSpec::Percent(0.0),
                    },
                    nana_ui_core::ClipPoint {
                        x: LengthSpec::Percent(100.0),
                        y: LengthSpec::Percent(0.0),
                    },
                    nana_ui_core::ClipPoint {
                        x: LengthSpec::Percent(50.0),
                        y: LengthSpec::Percent(100.0),
                    },
                ])),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        None,
    )
}

fn clip_path_inset_round_parent_rotated(
    value: u64,
    children: &[u64],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    round_px: f32,
    transform: nana_ui_core::PaintTransform,
) -> ExtractedNode {
    let mut node =
        clip_path_inset_round_parent(value, children, x, y, width, height, 0.0, round_px);
    Arc::make_mut(&mut node.source_style.layout).transform = Some(transform);
    node
}

fn frost_quad_child(
    value: u64,
    parent: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: [f32; 4],
    filter: nana_ui_core::BackdropFilter,
) -> ExtractedNode {
    let mut node = frost_quad_node_with_fill(value, x, y, width, height, fill, filter);
    node.parent = Some(StableNodeId::new(parent).unwrap());
    node
}

fn frost_quad_node_with_fill_and_transform(
    value: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fill: [f32; 4],
    filter: nana_ui_core::BackdropFilter,
    transform: nana_ui_core::PaintTransform,
) -> ExtractedNode {
    let mut node = frost_quad_node_with_fill(value, x, y, width, height, fill, filter);
    Arc::make_mut(&mut node.source_style.layout).transform = Some(transform);
    node
}

#[test]
fn backdrop_blurs_content_behind_frost_panel() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let frost = nana_ui_core::BackdropFilter {
        blur_radius: 8.0,
        saturate: 1.0,
    };
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 128.0, [1.0, 0.0, 0.0, 1.0]),
            colored_quad_node(2, 64.0, 0.0, 192.0, 128.0, [0.0, 1.0, 0.0, 1.0]),
            frost_quad_node_with_fill(3, 48.0, 48.0, 32.0, 32.0, [0.0, 0.0, 0.0, 0.0], frost),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [256.0, 128.0],
        physical_size: [256, 128],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 256, 128);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui frost blur split"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 256, 128);
    let center = pixel(&pixels, 256, 64, 64);
    assert!(
        center[0] > 40 && center[1] > 40,
        "frost center on red/green split must blend both channels, got {center:?}"
    );
    let dest_center = pixel(&pixels, 256, 128, 64);
    assert!(
        dest_center[1] > 180 && dest_center[0] < 40,
        "dest-center UV without frost must stay pure green, got {dest_center:?}"
    );
    drop(texture);
}

#[test]
fn backdrop_two_frost_panels_use_independent_regions() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let desaturate = nana_ui_core::BackdropFilter {
        blur_radius: 4.0,
        saturate: 0.0,
    };
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 128.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            colored_quad_node(2, 128.0, 0.0, 128.0, 64.0, [0.0, 1.0, 0.0, 1.0]),
            frost_quad_node_with_fill(3, 16.0, 16.0, 32.0, 32.0, [0.0, 0.0, 0.0, 0.0], desaturate),
            frost_quad_node_with_fill(4, 208.0, 16.0, 32.0, 32.0, [0.0, 0.0, 0.0, 0.0], desaturate),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [256.0, 64.0],
        physical_size: [256, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 256, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui frost independent regions"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 256, 64);
    let left = pixel(&pixels, 256, 32, 32);
    let right = pixel(&pixels, 256, 224, 32);
    assert!(
        left[1] > 30 && left[2] > 30 && left[0] < 120,
        "left frost over red with saturate(0) must gray out, got {left:?}"
    );
    assert!(
        right[0] > 100 && right[2] > 100 && right[1] > 150,
        "right frost over green with saturate(0) must stay green-weighted, got {right:?}"
    );
    assert!(
        left[0] + 40 < right[0],
        "independent frost regions must not last-write-wins, left={left:?} right={right:?}"
    );
    drop(texture);
}

#[test]
fn backdrop_first_dest_command_survives_transparent_quad() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let desaturate = nana_ui_core::BackdropFilter {
        blur_radius: 6.0,
        saturate: 0.0,
    };
    scene.apply_delta(
        [
            frost_quad_node_with_fill(1, 16.0, 16.0, 32.0, 32.0, [0.0, 0.0, 0.0, 0.0], desaturate),
            colored_quad_node(2, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 0.0, 0.0]),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui frost survives transparent quad"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let center = pixel(&pixels, 64, 32, 32);
    assert!(
        center[2] < 80 && center[0] > 8 && center[1] > 8,
        "frost over clear blue must survive a later transparent quad, got {center:?}"
    );
    let rb_delta = center[0]
        .abs_diff(center[1])
        .max(center[1].abs_diff(center[2]));
    assert!(
        rb_delta < 40,
        "saturate(0) over blue must stay near-neutral, got {center:?}"
    );
    drop(texture);
}

#[test]
fn backdrop_filter_forces_sample_count_one_dest_path() {
    let (device, _queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [frost_quad_node_with_fill(
            1,
            8.0,
            8.0,
            48.0,
            48.0,
            [1.0, 1.0, 1.0, 0.35],
            nana_ui_core::BackdropFilter {
                blur_radius: 12.0,
                saturate: 1.0,
            },
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.1, 0.1, 0.1, 1.0],
        clear: true,
    };
    let view = test_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui frost dest path"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("frost frame records dest passes");
    assert_eq!(counts.msaa, 0, "backdrop frames must stay sample_count=1");
    assert!(
        counts.backdrop >= 3,
        "frost must encode copy+blur+composite passes, got {counts:?}"
    );
    drop(encoder);
}

#[test]
fn inset_round_clip_corners_are_transparent_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            clip_path_inset_round_parent(1, &[2], 0.0, 0.0, 64.0, 64.0, 0.0, 32.0),
            colored_quad_child(2, 1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui inset round clip"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let corner = pixel(&pixels, 64, 2, 2);
    assert!(
        corner[2] > 180 && corner[0] < 80,
        "FragmentClip inset(0) round 32px must cut sharp child corners to clear blue; \
             AABB-only scissor would stay red, got {corner:?}"
    );
    let center = pixel(&pixels, 64, 32, 32);
    assert!(
        center[0] > 200 && center[1] < 40,
        "sharp red child interior must stay opaque red under rounded FragmentClip, got {center:?}"
    );
    drop(texture);
}

#[test]
fn circle_clip_path_cuts_child_corners_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            clip_path_circle_parent(1, &[2], 0.0, 0.0, 64.0, 64.0),
            colored_quad_child(2, 1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui circle clip"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let corner = pixel(&pixels, 64, 2, 2);
    assert!(
        corner[2] > 180 && corner[0] < 80,
        "circle() must clip square corners to clear, not a silent AABB, got {corner:?}"
    );
    let center = pixel(&pixels, 64, 32, 32);
    assert!(
        center[0] > 200 && center[1] < 40,
        "circle interior must stay child-red, got {center:?}"
    );
    drop(texture);
}

#[test]
fn polygon_clip_path_cuts_child_corners_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            clip_path_polygon_parent(1, &[2], 0.0, 0.0, 64.0, 64.0),
            colored_quad_child(2, 1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    let child = scene
        .primitive(nana_ui_scene::PrimitiveId {
            node: StableNodeId::new(2).unwrap(),
            slot: 0,
        })
        .expect("child quad");
    assert!(
        child
            .clips
            .iter()
            .any(|clip| clip.polygon_clip.as_ref().is_some_and(|p| p.len() >= 3)),
        "ancestor polygon clip must reach child primitive clips"
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui polygon ancestor clip"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let lower_left = pixel(&pixels, 64, 4, 60);
    assert!(
        lower_left[0] < 80,
        "polygon clip must cut child fill outside the triangle; AABB-only would stay red at {lower_left:?}"
    );
    let center = pixel(&pixels, 64, 32, 24);
    assert!(
        center[0] > 200 && center[1] < 40,
        "triangle interior must stay opaque red, got {center:?}"
    );
    drop(texture);
}

#[test]
fn perspective_rotate_y_paints_trapezoid_on_gpu() {
    let mat = nana_ui_core::PaintMat4::perspective(800.0)
        .expect("d")
        .then(nana_ui_core::PaintMat4::rotate_y(30_f32.to_radians()));
    let x = 28.0;
    let y = 40.0;
    let width = 200.0;
    let height = 80.0;
    let corners = mat
        .around_origin(x, y, width * 0.5, height * 0.5)
        .projected_corners(x, y, width, height)
        .expect("corners");
    let mut node = colored_quad_node(1, x, y, width, height, [1.0, 0.0, 0.0, 1.0]);
    Arc::make_mut(&mut node.source_style.layout).transform_3d = Some(mat);
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta([node], []);
    let viewport = ScenePaintViewport {
        logical_size: [256.0, 160.0],
        physical_size: [256, 160],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 256, 160);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui perspective rotateY trapezoid"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .expect("planar 3D must paint in the existing quad pass");
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 256, 160);
    let pivoted = mat.around_origin(x, y, width * 0.5, height * 0.5);
    let left_pt = pivoted
        .project_xy(x + width * 0.2, y + height * 0.5)
        .expect("left interior");
    let right_pt = pivoted
        .project_xy(x + width * 0.8, y + height * 0.5)
        .expect("right interior");
    let left_x = left_pt[0].round().clamp(1.0, 254.0) as u32;
    let right_x = right_pt[0].round().clamp(1.0, 254.0) as u32;
    let left_span = painted_column_span(&pixels, 256, 160, left_x);
    let right_span = painted_column_span(&pixels, 256, 160, right_x);
    assert!(
        (left_span as i32 - right_span as i32).abs() > 4,
        "GPU rotateY+perspective must paint a trapezoid, left_span={left_span} right_span={right_span} left_x={left_x} right_x={right_x} corners={corners:?}"
    );
    drop(texture);
}

fn painted_column_span(pixels: &[u8], width: u32, height: u32, x: u32) -> u32 {
    let mut min_y = height;
    let mut max_y = 0;
    let mut any = false;
    for y in 0..height {
        let color = pixel(pixels, width, x, y);
        if color[0] > 80 {
            any = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    assert!(any, "expected painted pixels in column {x}");
    max_y - min_y + 1
}

#[test]
fn rotated_inset_round_clip_keeps_corner_radius_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let k = std::f32::consts::FRAC_1_SQRT_2;
    scene.apply_delta(
        [
            clip_path_inset_round_parent_rotated(
                1,
                &[2],
                0.0,
                0.0,
                64.0,
                64.0,
                24.0,
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    ..Default::default()
                },
            ),
            colored_quad_child(2, 1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
        ],
        [],
    );
    // around_center(rotate 45°, box 0,0,64,64), k = 1/sqrt(2):
    //   scene_x = k*lx - k*ly + 32
    //   scene_y = k*lx + k*ly + 32(1-sqrt(2))   // ~ -13.255
    // Diamond vertices ~ (32,-13.3), (77.3,32), (32,77.3), (-13.3,32) sit
    // outside a 64x64 origin-0 viewport. dest = scene - scene_origin, so a
    // 96x96 view at origin (-16,-16) puts the top vertex at dest ~ (48, 2.7).
    //
    // Probe dest (48, 8): pixel center (48.5, 8.5) -> scene (32.5, -7.5) ->
    // local ~ (4.42, 3.72). Inside the sharp [0,64]^2 diamond (r=0 stays
    // child-red) and outside the r=24 rounded-box SDF (dist to corner
    // circle (24,24) ~ 28.2 > 24 -> dest/clear blue). Viewport (2,2)
    // inverse-maps to local ~ (-10.4, 32), already outside the diamond.
    let viewport = ScenePaintViewport {
        logical_size: [96.0, 96.0],
        physical_size: [96, 96],
        scale_factor: 1.0,
        scene_origin: [-16.0, -16.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 96, 96);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated inset round clip"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 96, 96);
    let cutlet = pixel(&pixels, 96, 48, 8);
    assert!(
        cutlet[2] > 180 && cutlet[0] < 80,
        "rotated inset(round) must SDF-round in the rotated frame, not zero radius; got {cutlet:?}"
    );
    let interior = pixel(&pixels, 96, 48, 48);
    assert!(
        interior[0] > 200 && interior[1] < 40,
        "diamond interior must stay child-red under rounded FragmentClip, got {interior:?}"
    );
    drop(texture);
}

#[test]
fn host_texture_under_inset_round_ancestor_clips_corners() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            clip_path_inset_round_parent(1, &[2], 0.0, 0.0, 64.0, 64.0, 0.0, 32.0),
            host_texture_child(2, 1, 0.0, 0.0, 64.0, 64.0, "layer"),
        ],
        [],
    );
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("layer", &view, 64, 64);
    let (texture, target_view) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture inset round ancestor"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let corner = pixel(&pixels, 64, 2, 2);
    assert!(
        corner[2] > 180 && corner[1] < 80,
        "HostTexture overflow clip must inherit ancestor inset(round) radius; got {corner:?}"
    );
    drop(texture);
    drop(view);
}

#[test]
fn rotated_backdrop_filter_mixes_dest_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let k = std::f32::consts::FRAC_1_SQRT_2;
    let frost = nana_ui_core::BackdropFilter {
        blur_radius: 8.0,
        saturate: 1.0,
    };
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            colored_quad_node(2, 64.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]),
            frost_quad_node_with_fill_and_transform(
                3,
                48.0,
                16.0,
                32.0,
                32.0,
                [0.0, 0.0, 0.0, 0.0],
                frost,
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    ..Default::default()
                },
            ),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [128.0, 64.0],
        physical_size: [128, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 128, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui rotated frost"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let counts = painter
        .last_dest_pass_counts
        .expect("rotated frost records dest passes");
    assert!(
        counts.backdrop >= 3,
        "rotated backdrop must encode copy+blur+composite, got {counts:?}"
    );
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 128, 64);
    let center = pixel(&pixels, 128, 64, 32);
    assert!(
        center[0] > 30 && center[2] > 30,
        "rotated frost centered on the red/blue edge must mix dest samples, got {center:?}"
    );
    // 45° around (64,32): diamond tips ≈ (64, 9.37), (86.63, 32), (64, 54.63),
    // (41.37, 32). Unrotated AABB is [48,80]×[16,48]. Dest (64, 12) sits
    // inside the diamond and outside that AABB; AABB composite would skip it.
    let tip = pixel(&pixels, 128, 64, 12);
    assert!(
        tip[0] > 20 && tip[2] > 20,
        "rotated frost must cover the diamond tip outside the unrotated AABB; \
             AABB composite would leave a pure dest color, got {tip:?}"
    );
    drop(texture);
}

#[test]
fn scrolled_backdrop_filter_samples_where_the_quad_is_drawn() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let mut scroller = overflow_parent(2, &[3], 0.0, 0.0, 64.0, 64.0, None);
    scroller.scroll_offset = nana_ui_runtime::ScrollOffset { x: 0.0, y: 24.0 };
    // saturate(0) turns the red under the panel gray, so the pixels the
    // backdrop composites are the ones that lose their red.
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            scroller,
            frost_quad_child(
                3,
                2,
                8.0,
                40.0,
                48.0,
                16.0,
                [0.0, 0.0, 0.0, 0.0],
                nana_ui_core::BackdropFilter {
                    blur_radius: 4.0,
                    saturate: 0.0,
                },
            ),
        ],
        [],
    );
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    // Scrolled up 24, the panel is drawn over y 16..32, not its layout 40..56.
    let drawn = pixel(&pixels, 64, 32, 24);
    assert!(
        drawn[1] > 30 && drawn[2] > 30,
        "the scrolled panel must gray the red it is drawn over, got {drawn:?}"
    );
    let layout = pixel(&pixels, 64, 32, 48);
    assert!(
        layout[0] > 200 && layout[1] < 10 && layout[2] < 10,
        "the panel's unscrolled layout box must stay red, got {layout:?}"
    );
}

#[test]
fn frost_under_ancestor_polygon_clips_outside_triangle_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let frost = nana_ui_core::BackdropFilter {
        blur_radius: 8.0,
        saturate: 0.0,
    };
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            clip_path_polygon_parent(2, &[3], 0.0, 0.0, 64.0, 64.0),
            frost_quad_child(3, 2, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 0.0, 0.0], frost),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui frost ancestor polygon"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let inside = pixel(&pixels, 64, 32, 24);
    assert!(
        inside[0] < 180 && (inside[0] as i16 - inside[1] as i16).abs() < 50,
        "frost saturate(0) must gray dest inside the ancestor triangle, got {inside:?}"
    );
    let outside = pixel(&pixels, 64, 4, 60);
    assert!(
        outside[0] > 200 && outside[1] < 80,
        "ancestor polygon must clip frost; AABB-only composite would desaturate {outside:?}"
    );
    drop(texture);
}

#[test]
fn gradient_white_to_transparent_source_over_red() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let gradient_surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 180.0,
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [1.0, 1.0, 1.0, 0.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            paint_surface_quad_node(
                2,
                0.0,
                0.0,
                64.0,
                64.0,
                [0.0, 0.0, 0.0, 0.0],
                gradient_surface,
            ),
        ],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui gradient source-over"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let center = pixel(&pixels, 64, 32, 32);
    assert!(
        center[0] > 200 && center[1] < 180 && center[2] < 180,
        "white→transparent gradient must source-over red (pink), not replace with black {center:?}"
    );
    drop(texture);
}

#[test]
fn mask_linear_fade_scales_rgb_with_alpha() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    let masked_surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 0.0,
                stops: vec![nana_ui_core::GradientStop {
                    position: 0.0,
                    color: [1.0, 0.0, 0.0, 1.0],
                }],
            }),
        )),
        mask: Some(nana_ui_core::MaskImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 90.0,
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [1.0, 1.0, 1.0, 0.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
            masked_surface,
        )],
        [],
    );
    let primitive = scene
        .primitive(nana_ui_scene::PrimitiveId {
            node: StableNodeId::new(1).unwrap(),
            slot: 0,
        })
        .expect("masked quad primitive");
    match &primitive.kind {
        nana_ui_scene::ScenePrimitiveKind::Quad { surface, .. } => {
            assert!(
                surface.mask.is_some(),
                "mask must travel on quad surface {surface:?}"
            );
        }
        other => panic!("expected quad primitive, got {other:?}"),
    }
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui mask fade"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let left = pixel(&pixels, 64, 4, 32);
    let right = pixel(&pixels, 64, 56, 32);
    assert!(
        left[0] > 200 && left[1] < 40 && left[2] < 40,
        "masked quad left must stay red {left:?}"
    );
    assert!(
        right[2] > 180 && right[0] < 80,
        "mask fade must reveal clear blue, not opaque red {right:?}"
    );
    drop(texture);
}

#[test]
fn host_texture_mask_linear_fade_samples_in_document_order() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut node = extracted_div(
        1,
        &[],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                mask: Some(nana_ui_core::MaskImage::Gradient(
                    nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                        angle_deg: 90.0,
                        stops: vec![
                            nana_ui_core::GradientStop {
                                position: 0.0,
                                color: [1.0, 1.0, 1.0, 1.0],
                            },
                            nana_ui_core::GradientStop {
                                position: 1.0,
                                color: [1.0, 1.0, 1.0, 0.0],
                            },
                        ],
                    }),
                )),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        None,
    );
    node.custom_render = Some(CustomRenderNode::new("nana.host-texture", "layer", 1));
    let mut scene = UiScene::new();
    scene.apply_delta([node], []);
    let primitive = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Custom { .. }))
        .expect("host texture custom");
    match &primitive.kind {
        ScenePrimitiveKind::Custom { mask, .. } => {
            assert!(mask.is_some(), "mask must travel onto HostTexture custom");
        }
        other => panic!("expected custom, got {other:?}"),
    }
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::RED);
    let registry = register_host_texture("layer", &view, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture mask fade"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let left = pixel(&pixels, 64, 4, 32);
    let right = pixel(&pixels, 64, 56, 32);
    assert!(
        left[0] > 200 && left[1] < 40 && left[2] < 40,
        "masked HostTexture left must stay red {left:?}"
    );
    assert!(
        right[2] > 180 && right[0] < 80,
        "HostTexture mask fade must reveal clear blue, not opaque red {right:?}"
    );
    drop(view);
    drop(texture);
}

fn alpha_split_mask_png_data_url() -> String {
    let mut img = image::RgbaImage::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let a = if x < 32 { 255 } else { 0 };
            img.put_pixel(x, y, image::Rgba([255, 255, 255, a]));
        }
    }
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("encode mask png");
    use base64::Engine as _;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[test]
fn mask_url_alpha_scales_quad_in_document_order() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let masked_surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 0.0,
                stops: vec![nana_ui_core::GradientStop {
                    position: 0.0,
                    color: [1.0, 0.0, 0.0, 1.0],
                }],
            }),
        )),
        mask: Some(nana_ui_core::MaskImage::Url(alpha_split_mask_png_data_url())),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
            masked_surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui mask url alpha"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let left = pixel(&pixels, 64, 4, 32);
    let right = pixel(&pixels, 64, 56, 32);
    assert!(
        left[0] > 200 && left[1] < 40 && left[2] < 40,
        "url-mask quad left must stay red (image alpha=1) {left:?}"
    );
    assert!(
        right[2] > 180 && right[0] < 80,
        "url-mask uses image alpha, not luminance: right must reveal clear blue {right:?}"
    );
    drop(texture);
}

#[test]
fn host_texture_mask_url_alpha_samples_in_document_order() {
    check_host_texture_url_mask(false);
}

#[test]
fn async_host_texture_mask_rebinds_after_image_completion() {
    check_host_texture_url_mask(true);
}

fn check_host_texture_url_mask(remote: bool) {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    painter.set_resource_fetch_host(Some(super::image_url::loopback_fetch_host()));
    let uploads = super::url_texture_cache::UPLOADS.with(std::cell::Cell::get);
    let mask = alpha_split_mask_png_data_url();
    let server = remote.then(|| {
        use base64::Engine;
        LocalPngServer::serve(
            base64::engine::general_purpose::STANDARD
                .decode(mask.split_once(',').unwrap().1)
                .unwrap(),
        )
    });
    let mask = server.as_ref().map_or(mask, |server| server.url.clone());
    let (wake, awoken) = std::sync::mpsc::channel();
    painter.set_image_waker(Arc::new(move || {
        let _ = wake.send(());
    }));
    let mut node = extracted_div(
        1,
        &[],
        0.0,
        0.0,
        64.0,
        64.0,
        nana_ui_core::LayoutStyle {
            paint: nana_ui_core::PaintStyle {
                mask: Some(nana_ui_core::MaskImage::Url(mask)),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        },
        None,
    );
    node.custom_render = Some(CustomRenderNode::new("nana.host-texture", "layer", 1));
    let mut scene = UiScene::new();
    scene.apply_delta([node], []);
    let view = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::RED);
    let registry = register_host_texture("layer", &view, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, target_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui host texture mask url"),
    });
    painter
        .paint_encoder(
            &scene,
            &mut encoder,
            &target_view,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    if remote {
        let first = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
        assert!(
            pixel(&first, 64, 56, 32)[0] > 200,
            "pending mask has not been applied yet"
        );
        // The quad and the HostTexture share one URL cache, so a single
        // fetch serves both; keep painting until nothing is pending.
        loop {
            awoken
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("every pending image fetch wakes the painter");
            encoder = device.create_command_encoder(&Default::default());
            painter
                .paint_encoder(
                    &scene,
                    &mut encoder,
                    &target_view,
                    viewport,
                    Some(&registry),
                    None,
                )
                .unwrap();
            if !painter.has_pending_images() {
                break;
            }
        }
    }
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let left = pixel(&pixels, 64, 4, 32);
    let right = pixel(&pixels, 64, 56, 32);
    assert!(
        left[0] > 200 && left[1] < 40 && left[2] < 40,
        "url-mask HostTexture left must stay red {left:?}"
    );
    assert!(
        right[2] > 180 && right[0] < 80,
        "HostTexture url-mask uses image alpha; right must reveal clear blue {right:?}"
    );
    assert_eq!(
        super::url_texture_cache::UPLOADS.with(std::cell::Cell::get) - uploads,
        1,
        "the quad and the HostTexture mask share one fetch, decode and upload"
    );
    drop(view);
    drop(texture);
}

#[test]
fn mask_url_unloadable_is_ignored_not_gradient() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let masked_surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 0.0,
                stops: vec![nana_ui_core::GradientStop {
                    position: 0.0,
                    color: [1.0, 0.0, 0.0, 1.0],
                }],
            }),
        )),
        mask: Some(nana_ui_core::MaskImage::Url(
            "nana-missing-mask-image-xyz.png".into(),
        )),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
            masked_surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui mask url ignored"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let left = pixel(&pixels, 64, 4, 32);
    let right = pixel(&pixels, 64, 56, 32);
    assert!(
        left[0] > 200 && left[1] < 40 && left[2] < 40,
        "unloadable mask url must not hide the fill {left:?}"
    );
    assert!(
        right[0] > 200 && right[1] < 40 && right[2] < 40,
        "unloadable mask url must not pretend to be a gradient fade {right:?}"
    );
    drop(texture);
}

#[test]
fn mask_linear_six_stops_uses_stop_five_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let masked_surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 0.0,
                stops: vec![nana_ui_core::GradientStop {
                    position: 0.0,
                    color: [1.0, 0.0, 0.0, 1.0],
                }],
            }),
        )),
        mask: Some(nana_ui_core::MaskImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 180.0,
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.2,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.4,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.6,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.8,
                        color: [1.0, 1.0, 1.0, 0.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [1.0, 1.0, 1.0, 0.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
            masked_surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 1.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui six-stop mask"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let upper = pixel(&pixels, 64, 32, 8);
    let lower = pixel(&pixels, 64, 32, 56);
    assert!(
        upper[0] > 200 && upper[1] < 40 && upper[2] < 40,
        "top must stay unmasked red before stop 5, got {upper:?}"
    );
    assert!(
        lower[2] > 180 && lower[0] < 80,
        "bottom must hide via stop 5+ mask alpha, not stay red {lower:?}"
    );
    drop(texture);
}

#[test]
fn radial_gradient_center_differs_from_linear_edge() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let radial = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Radial(nana_ui_core::RadialGradient {
                circle: true,
                center: [nana_ui_core::LengthSpec::Percent(50.0); 2],
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [0.0, 0.0, 1.0, 1.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    let linear = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 90.0,
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [0.0, 0.0, 1.0, 1.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };

    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 0.0, 0.0, 0.0],
            radial,
        )],
        [],
    );
    let (radial_tex, radial_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui radial gradient"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &radial_view, viewport, None, None)
        .unwrap();
    let radial_pixels = readback_rgba(&device, &queue, encoder, &radial_tex, 64, 64);
    let radial_center = pixel(&radial_pixels, 64, 32, 32);
    let radial_corner = pixel(&radial_pixels, 64, 4, 4);

    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            2,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 0.0, 0.0, 0.0],
            linear,
        )],
        [],
    );
    let (linear_tex, linear_view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui linear gradient"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &linear_view, viewport, None, None)
        .unwrap();
    let linear_pixels = readback_rgba(&device, &queue, encoder, &linear_tex, 64, 64);
    let linear_left = pixel(&linear_pixels, 64, 4, 32);
    let linear_right = pixel(&linear_pixels, 64, 60, 32);

    assert!(
        radial_center[0] > 200 && radial_center[2] < 80,
        "radial center must be red, got {radial_center:?}"
    );
    assert!(
        radial_corner[2] > 200 && radial_corner[0] < 80,
        "radial corner must be blue, got {radial_corner:?}"
    );
    assert!(
        linear_left[0] > 200 && linear_left[2] < 80,
        "linear left must be red, got {linear_left:?}"
    );
    assert!(
        linear_right[2] > 200 && linear_right[0] < 80,
        "linear right must be blue, got {linear_right:?}"
    );
    drop(radial_tex);
    drop(linear_tex);
}

#[test]
fn linear_gradient_five_stops_uses_stop_five_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Gradient(
            nana_ui_core::CssGradient::Linear(nana_ui_core::LinearGradient {
                angle_deg: 180.0,
                stops: vec![
                    nana_ui_core::GradientStop {
                        position: 0.0,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.2,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.4,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.6,
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 0.8,
                        color: [0.0, 0.0, 1.0, 1.0],
                    },
                    nana_ui_core::GradientStop {
                        position: 1.0,
                        color: [0.0, 0.0, 1.0, 1.0],
                    },
                ],
            }),
        )),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 0.0, 0.0, 0.0],
            surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui five-stop gradient"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let upper = pixel(&pixels, 64, 32, 8);
    let lower = pixel(&pixels, 64, 32, 56);
    assert!(
        upper[0] > 200 && upper[2] < 80,
        "top must stay red before stop 5, got {upper:?}"
    );
    assert!(
        lower[2] > 200 && lower[0] < 80,
        "bottom must reach blue from stop 5+, got {lower:?}"
    );
    drop(texture);
}

fn blue_tile_fixture_png() -> (std::path::PathBuf, std::path::PathBuf) {
    static FIXTURE: std::sync::OnceLock<(std::path::PathBuf, std::path::PathBuf)> =
        std::sync::OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let fixture_dir =
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            std::fs::create_dir_all(&fixture_dir).expect("fixture dir");
            let png_path = fixture_dir.join("blue-tile.png");
            if !png_path.exists() {
                let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]));
                img.save(&png_path).expect("write fixture png");
            }
            (fixture_dir, png_path)
        })
        .clone()
}

fn paint_url_quad_and_sample_center(
    url: String,
    fetch_host: Option<nana_ui_platform::SharedFetchHost>,
) -> [u8; 4] {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    painter.set_resource_fetch_host(fetch_host);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
            url,
            nana_ui_core::BackgroundImageFit::Stretch,
        )),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 0.0, 0.0, 0.0],
            surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, 64, 64);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui url png"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    queue.submit([encoder.finish()]);
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while painter.has_pending_images() {
        assert!(Instant::now() < deadline, "image did not complete");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        queue.submit([encoder.finish()]);
    }
    let encoder = device.create_command_encoder(&Default::default());
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    let sample = pixel(&pixels, 64, 32, 32);
    drop(texture);
    sample
}

/// A window whose document is closed mid-request while the remaining window
/// never rebuilds a batch, so nothing but the close itself can reclaim it.
#[test]
fn closing_a_window_cancels_its_image_requests_and_releases_its_fetch_host() {
    // Connections queue in the backlog and are never answered, so the request
    // stays in flight until something cancels it.
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/stalled.png", listener.local_addr().unwrap());
    let closing = super::image_url::loopback_fetch_host();
    let released = Arc::downgrade(&closing);
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0; 4],
            nana_ui_scene::QuadSurfacePaint {
                background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
                    url,
                    nana_ui_core::BackgroundImageFit::Stretch,
                )),
                ..Default::default()
            },
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0; 2],
        physical_size: [64; 2],
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: [0.0; 4],
        clear: true,
    };
    let (_texture, view) = test_copy_target(&device, format, 64, 64);
    for (id, host) in [(1, Some(closing)), (2, None)] {
        painter.set_resource_fetch_host(host);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_target_encoder(
                RenderTargetId(id),
                &scene,
                &mut encoder,
                &view,
                viewport,
                None,
                None,
            )
            .unwrap();
        queue.submit([encoder.finish()]);
    }
    assert!(
        painter.has_pending_images(),
        "the request must be in flight"
    );

    painter.remove_target(RenderTargetId(1));

    assert!(
        !painter.has_pending_images(),
        "closing must stop the document's requests without waiting for expiry"
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while released.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "the closed document's host must be released once its worker is cancelled"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(listener);
}

/// Records every URL a document's egress is asked for, then lets `inner` decide.
#[derive(Debug)]
struct RecordingFetchHost {
    inner: nana_ui_platform::SharedFetchHost,
    requests: std::sync::Mutex<Vec<String>>,
}

impl RecordingFetchHost {
    fn new(inner: nana_ui_platform::SharedFetchHost) -> Arc<Self> {
        Arc::new(Self {
            inner,
            requests: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl nana_ui_platform::FetchHost for RecordingFetchHost {
    fn fetch(
        &self,
        request: nana_ui_platform::FetchRequest,
    ) -> Result<nana_ui_platform::FetchResponse, nana_ui_platform::FetchError> {
        self.requests.lock().unwrap().push(request.url.clone());
        self.inner.fetch(request)
    }

    fn fetch_cancellable(
        &self,
        request: nana_ui_platform::FetchRequest,
        cancellation: nana_ui_platform::FetchCancellation,
    ) -> Result<nana_ui_platform::FetchResponse, nana_ui_platform::FetchError> {
        self.requests.lock().unwrap().push(request.url.clone());
        self.inner.fetch_cancellable(request, cancellation)
    }

    fn policy(&self) -> &nana_ui_platform::FetchPolicy {
        self.inner.policy()
    }
}

/// Two documents share one painter, as windows of one surface format do. The
/// strict one paints first and the permissive one second, then both repaint
/// after the permissive load has landed in the shared cache.
#[test]
fn url_images_go_only_through_the_fetch_host_of_the_document_being_painted() {
    let (_, path) = blue_tile_fixture_png();
    let server = LocalPngServer::serve(std::fs::read(path).unwrap());
    let strict = RecordingFetchHost::new(nana_ui_platform::shared_fetch_host(
        nana_ui_platform::NativeFetchHost::new(nana_ui_platform::FetchPolicy::default()),
    ));
    let permissive = RecordingFetchHost::new(super::image_url::loopback_fetch_host());
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0; 4],
            nana_ui_scene::QuadSurfacePaint {
                background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
                    server.url.clone(),
                    nana_ui_core::BackgroundImageFit::Stretch,
                )),
                ..Default::default()
            },
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0; 2],
        physical_size: [64; 2],
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (strict_texture, strict_view) = test_copy_target(&device, format, 64, 64);
    let (permissive_texture, permissive_view) = test_copy_target(&device, format, 64, 64);
    let documents: [(u64, nana_ui_platform::SharedFetchHost, &wgpu::TextureView); 2] = [
        (1, strict.clone(), &strict_view),
        (2, permissive.clone(), &permissive_view),
    ];
    let paint_both = |painter: &mut SceneWgpuPainter| {
        for (id, host, view) in &documents {
            painter.set_resource_fetch_host(Some(host.clone()));
            let mut encoder = device.create_command_encoder(&Default::default());
            painter
                .paint_target_encoder(
                    RenderTargetId(*id),
                    &scene,
                    &mut encoder,
                    view,
                    viewport,
                    None,
                    None,
                )
                .unwrap();
            queue.submit([encoder.finish()]);
        }
    };

    paint_both(&mut painter);
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while painter.has_pending_images() {
        assert!(Instant::now() < deadline, "images did not settle");
        std::thread::sleep(std::time::Duration::from_millis(5));
        paint_both(&mut painter);
    }
    paint_both(&mut painter);

    let encoder = device.create_command_encoder(&Default::default());
    let strict_pixels = readback_rgba(&device, &queue, encoder, &strict_texture, 64, 64);
    let encoder = device.create_command_encoder(&Default::default());
    let permissive_pixels = readback_rgba(&device, &queue, encoder, &permissive_texture, 64, 64);
    assert!(
        is_blue_slot(pixel(&permissive_pixels, 64, 32, 32)),
        "the permissive document loads its image whichever document came first"
    );
    assert!(
        !is_blue_slot(pixel(&strict_pixels, 64, 32, 32)),
        "the strict document must not show an image only another policy admitted"
    );
    assert_eq!(
        permissive.requests(),
        std::slice::from_ref(&server.url),
        "only the permissive document's request may reach its host"
    );
    assert_eq!(
        strict.requests(),
        std::slice::from_ref(&server.url),
        "the strict document's request is decided by its own policy"
    );
}

struct LocalPngServer {
    url: String,
    shutdown: std::sync::mpsc::Sender<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LocalPngServer {
    fn serve(png: Vec<u8>) -> Self {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("bind 127.0.0.1");
        let addr = listener.local_addr().expect("local addr");
        listener.set_nonblocking(true).expect("nonblocking");
        let (shutdown, rx) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                            .unwrap();
                        let mut buf = [0u8; 1024];
                        let mut request = Vec::new();
                        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(read) => request.extend_from_slice(&buf[..read]),
                            }
                        }
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            png.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&png);
                        let _ = stream.flush();
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{addr}/blue-tile.png"),
            shutdown,
            handle: Some(handle),
        }
    }
}

impl Drop for LocalPngServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn background_image_file_url_paints_fixture_png() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let file_url = nana_ui_core::url_jail::path_to_file_url(&png_path);
    let sample = paint_url_quad_and_sample_center(file_url, None);
    super::image_url::reset_test_url_base();
    assert!(
        sample[2] > 200 && sample[0] < 80,
        "file url png must paint blue tile, got {sample:?}"
    );
}

#[test]
fn background_image_http_url_paints_fixture_png() {
    let (_fixture_dir, png_path) = blue_tile_fixture_png();
    let png = std::fs::read(&png_path).expect("read fixture png");
    let server = LocalPngServer::serve(png);
    let sample = paint_url_quad_and_sample_center(
        server.url.clone(),
        Some(super::image_url::loopback_fetch_host()),
    );
    assert!(
        sample[2] > 200 && sample[0] < 80,
        "http url png must paint blue tile, got {sample:?}"
    );
}

#[test]
fn more_http_images_than_fetch_slots_eventually_paint() {
    let (_, path) = blue_tile_fixture_png();
    let server = LocalPngServer::serve(std::fs::read(path).unwrap());
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    painter.set_resource_fetch_host(Some(super::image_url::loopback_fetch_host()));
    let (wake, awoken) = std::sync::mpsc::channel();
    painter.set_image_waker(Arc::new(move || {
        let _ = wake.send(());
    }));
    let mut scene = UiScene::new();
    scene.apply_delta(
        (0..9).map(|id| {
            paint_surface_quad_node(
                id + 1,
                (id % 3) as f32 * 20.0,
                (id / 3) as f32 * 20.0,
                20.0,
                20.0,
                [0.0; 4],
                nana_ui_scene::QuadSurfacePaint {
                    background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
                        format!("{}?id={id}", server.url),
                        nana_ui_core::BackgroundImageFit::Stretch,
                    )),
                    ..Default::default()
                },
            )
        }),
        [],
    );
    let mut pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0; 2],
        [64; 2],
        1.0,
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while painter.has_pending_images() {
        awoken
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0; 2],
            [64; 2],
            1.0,
        );
    }
    for id in 0..9 {
        assert!(pixel(&pixels, 64, (id % 3) * 20 + 10, (id / 3) * 20 + 10)[2] > 200);
    }
}

#[test]
fn slow_http_image_returns_before_response_and_invalidates_cached_dest_on_completion() {
    use std::{
        io::{Read, Write},
        sync::mpsc,
        time::Duration,
    };
    let (_, path) = blue_tile_fixture_png();
    let png = std::fs::read(path).unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/image.png", listener.local_addr().unwrap());
    let (release, gated) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 256];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        assert!(
            request.windows(4).any(|window| window == b"\r\n\r\n"),
            "HTTP request headers must be complete"
        );
        gated
            .recv_timeout(Duration::from_secs(10))
            .expect("paint waited for the HTTP response");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        )
        .unwrap();
        stream.write_all(&png).unwrap();
    });
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    painter.set_resource_fetch_host(Some(super::image_url::loopback_fetch_host()));
    let (wake, awoken) = mpsc::channel();
    painter.set_image_waker(Arc::new(move || {
        let _ = wake.send(());
    }));
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0; 4],
            nana_ui_scene::QuadSurfacePaint {
                background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
                    url,
                    nana_ui_core::BackgroundImageFit::Stretch,
                )),
                ..Default::default()
            },
        )],
        [],
    );
    let before = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0; 2],
        [64; 2],
        1.0,
    );
    assert!(painter.has_pending_images());
    assert!(pixel(&before, 64, 32, 32)[2] < 80);
    release.send(()).unwrap();
    awoken
        .recv_timeout(Duration::from_secs(5))
        .expect("image completion must wake an idle host");
    assert!(painter.has_image_updates());
    let after = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0; 2],
        [64; 2],
        1.0,
    );
    assert!(
        pixel(&after, 64, 32, 32)[2] > 200,
        "unchanged scene must paint the completed image"
    );
    assert!(!painter.has_pending_images());
    server.join().unwrap();
}

#[test]
fn async_http_image_rebinds_each_render_target_after_shared_completion() {
    use std::{
        io::{Read, Write},
        sync::mpsc,
        time::Duration,
    };
    let (_, path) = blue_tile_fixture_png();
    let png = std::fs::read(path).unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/image.png", listener.local_addr().unwrap());
    let (release, gated) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 256];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
        }
        assert!(request.windows(4).any(|window| window == b"\r\n\r\n"));
        gated
            .recv_timeout(Duration::from_secs(10))
            .expect("paint waited for the HTTP response");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        )
        .unwrap();
        stream.write_all(&png).unwrap();
    });

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    painter.set_resource_fetch_host(Some(super::image_url::loopback_fetch_host()));
    let (wake, awoken) = mpsc::channel();
    painter.set_image_waker(Arc::new(move || {
        let _ = wake.send(());
    }));
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0; 4],
            nana_ui_scene::QuadSurfacePaint {
                background_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
                    url,
                    nana_ui_core::BackgroundImageFit::Stretch,
                )),
                ..Default::default()
            },
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0; 2],
        physical_size: [64; 2],
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: [0.0; 4],
        clear: true,
    };
    let (first_texture, first_view) = test_copy_target(&device, format, 64, 64);
    let (second_texture, second_view) = test_copy_target(&device, format, 64, 64);
    for (id, target) in [(1, &first_view), (2, &second_view)] {
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_target_encoder(
                RenderTargetId(id),
                &scene,
                &mut encoder,
                target,
                viewport,
                None,
                None,
            )
            .unwrap();
        queue.submit([encoder.finish()]);
    }
    assert!(painter.has_pending_images());

    release.send(()).unwrap();
    awoken
        .recv_timeout(Duration::from_secs(5))
        .expect("image completion must wake both target owners");

    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_target_encoder(
            RenderTargetId(1),
            &scene,
            &mut encoder,
            &first_view,
            viewport,
            None,
            None,
        )
        .unwrap();
    let first_pixels = readback_rgba(&device, &queue, encoder, &first_texture, 64, 64);
    assert!(is_blue_slot(pixel(&first_pixels, 64, 32, 32)));

    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_target_encoder(
            RenderTargetId(2),
            &scene,
            &mut encoder,
            &second_view,
            viewport,
            None,
            None,
        )
        .unwrap();
    let second_pixels = readback_rgba(&device, &queue, encoder, &second_texture, 64, 64);
    assert!(is_blue_slot(pixel(&second_pixels, 64, 32, 32)));
    server.join().unwrap();
}

fn red_tile_fixture_png() -> std::path::PathBuf {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixture_dir).expect("fixture dir");
    let png_path = fixture_dir.join("red-tile.png");
    if !png_path.exists() {
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]));
        img.save(&png_path).expect("write red fixture png");
    }
    png_path
}

fn stripe_tile_fixture_png() -> std::path::PathBuf {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixture_dir).expect("fixture dir");
    let png_path = fixture_dir.join("stripe-tile.png");
    if !png_path.exists() {
        let mut img = image::RgbaImage::new(8, 8);
        for (x, _y, pixel) in img.enumerate_pixels_mut() {
            *pixel = if x < 4 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        img.save(&png_path).expect("write stripe fixture png");
    }
    png_path
}

fn paint_surface_sample(
    surface: nana_ui_scene::QuadSurfacePaint,
    logical: [f32; 2],
    physical: [u32; 2],
    fill: [f32; 4],
    sample_x: u32,
    sample_y: u32,
) -> [u8; 4] {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1, 0.0, 0.0, logical[0], logical[1], fill, surface,
        )],
        [],
    );
    let viewport = ScenePaintViewport {
        logical_size: logical,
        physical_size: physical,
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, physical[0], physical[1]);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui image layer png"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, physical[0], physical[1]);
    let sample = pixel(&pixels, physical[0], sample_x, sample_y);
    drop(texture);
    sample
}

#[test]
fn img_src_content_image_paints_fixture_png() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        content_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
            png_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
            nana_ui_core::BackgroundImageFit::Stretch,
        )),
        ..Default::default()
    };
    let sample = paint_surface_sample(
        surface,
        [64.0, 64.0],
        [64, 64],
        [0.0, 0.0, 0.0, 1.0],
        32,
        32,
    );
    super::image_url::reset_test_url_base();
    assert!(
        sample[2] > 200 && sample[0] < 80,
        "img src must paint through content_image, got {sample:?}"
    );
}

#[test]
fn inline_svg_data_url_paints_through_content_image() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><path d=\"M0 0 H8 V8 H0 Z\" fill=\"#00ff00\"/></svg>";
    use base64::Engine as _;
    let url = format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    );
    let surface = nana_ui_scene::QuadSurfacePaint {
        content_image: Some(nana_ui_core::BackgroundImage::url_with_fit(
            url,
            nana_ui_core::BackgroundImageFit::Stretch,
        )),
        ..Default::default()
    };
    let sample = paint_surface_sample(surface, [8.0, 8.0], [8, 8], [0.0, 0.0, 0.0, 1.0], 4, 4);
    assert!(
        sample[1] > 200 && sample[0] < 80 && sample[2] < 80,
        "inline svg path must paint via the url texture cache, got {sample:?}"
    );
}

#[test]
fn object_fit_contain_letterboxes_on_wide_box() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        content_image: Some(nana_ui_core::BackgroundImage::Url {
            url: png_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
            fit: nana_ui_core::BackgroundImageFit::Contain,
            size_width: None,
            size_height: None,
            position: nana_ui_core::BackgroundPosition::center(),
            repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
            sampling: nana_ui_core::ImageSampling::Resample,
        }),
        ..Default::default()
    };
    let fill = [1.0, 0.0, 0.0, 1.0];
    let center = paint_surface_sample(surface.clone(), [64.0, 32.0], [64, 32], fill, 32, 16);
    let edge = paint_surface_sample(surface, [64.0, 32.0], [64, 32], fill, 2, 16);
    super::image_url::reset_test_url_base();
    assert!(
        center[2] > 200 && center[0] < 80,
        "contain center must sample the image, got {center:?}"
    );
    assert!(
        edge[0] > 200 && edge[2] < 80,
        "contain letterbox must keep background, got {edge:?}"
    );
}

#[test]
fn two_layer_background_paints_top_over_bottom() {
    let (fixture_dir, blue_path) = blue_tile_fixture_png();
    let red_path = red_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Url {
            url: red_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("red-tile.png")
                .to_string(),
            fit: nana_ui_core::BackgroundImageFit::Auto,
            size_width: None,
            size_height: None,
            position: nana_ui_core::BackgroundPosition::default(),
            repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
            sampling: nana_ui_core::ImageSampling::Resample,
        }),
        background_layers: vec![nana_ui_core::BackgroundImage::url_with_fit(
            blue_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
            nana_ui_core::BackgroundImageFit::Stretch,
        )],
        ..Default::default()
    };
    let top_left = paint_surface_sample(
        surface.clone(),
        [64.0, 64.0],
        [64, 64],
        [0.0, 0.0, 0.0, 1.0],
        4,
        4,
    );
    let far = paint_surface_sample(
        surface,
        [64.0, 64.0],
        [64, 64],
        [0.0, 0.0, 0.0, 1.0],
        48,
        48,
    );
    super::image_url::reset_test_url_base();
    assert!(
        top_left[0] > 200 && top_left[2] < 80,
        "8px auto red layer must cover the top-left, got {top_left:?}"
    );
    assert!(
        far[2] > 200 && far[0] < 80,
        "stretch blue layer must show outside the red tile, got {far:?}"
    );
}

#[test]
fn background_repeat_x_tiles_stripe() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let stripe = stripe_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Url {
            url: stripe
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("stripe-tile.png")
                .to_string(),
            fit: nana_ui_core::BackgroundImageFit::Auto,
            size_width: None,
            size_height: None,
            position: nana_ui_core::BackgroundPosition::default(),
            repeat: nana_ui_core::BackgroundRepeat::RepeatX,
            sampling: nana_ui_core::ImageSampling::Resample,
        }),
        ..Default::default()
    };
    let left = paint_surface_sample(
        surface.clone(),
        [16.0, 8.0],
        [16, 8],
        [0.0, 0.0, 0.0, 1.0],
        2,
        4,
    );
    let mid = paint_surface_sample(surface, [16.0, 8.0], [16, 8], [0.0, 0.0, 0.0, 1.0], 10, 4);
    super::image_url::reset_test_url_base();
    assert!(
        left[0] > 200 && left[2] < 80,
        "first tile left must be red, got {left:?}"
    );
    assert!(
        mid[0] > 200 && mid[2] < 80,
        "second tile left must be red from repeat-x, got {mid:?}"
    );
}

#[test]
fn default_background_repeat_tiles_sized_url() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Url {
            url: png_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
            fit: nana_ui_core::BackgroundImageFit::Length,
            size_width: Some(nana_ui_core::LengthSpec::Px(32.0)),
            size_height: None,
            position: nana_ui_core::BackgroundPosition::default(),
            repeat: nana_ui_core::BackgroundRepeat::Repeat,
            sampling: nana_ui_core::ImageSampling::Resample,
        }),
        ..Default::default()
    };
    let first = paint_surface_sample(
        surface.clone(),
        [64.0, 32.0],
        [64, 32],
        [1.0, 0.0, 0.0, 1.0],
        8,
        16,
    );
    let tiled = paint_surface_sample(
        surface,
        [64.0, 32.0],
        [64, 32],
        [1.0, 0.0, 0.0, 1.0],
        40,
        16,
    );
    super::image_url::reset_test_url_base();
    assert!(
        first[2] > 200 && first[0] < 80,
        "first 32px tile must be the image, got {first:?}"
    );
    assert!(
        tiled[2] > 200 && tiled[0] < 80,
        "unspecified repeat + size 32px must tile, got {tiled:?}"
    );
}

#[test]
fn background_repeat_space_does_not_paint_as_repeat() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(nana_ui_core::BackgroundImage::Url {
            url: png_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
            fit: nana_ui_core::BackgroundImageFit::Length,
            size_width: Some(nana_ui_core::LengthSpec::Px(32.0)),
            size_height: None,
            position: nana_ui_core::BackgroundPosition::default(),
            repeat: nana_ui_core::BackgroundRepeat::Unsupported,
            sampling: nana_ui_core::ImageSampling::Resample,
        }),
        ..Default::default()
    };
    let first = paint_surface_sample(surface, [64.0, 32.0], [64, 32], [1.0, 0.0, 0.0, 1.0], 8, 16);
    super::image_url::reset_test_url_base();
    assert!(
        first[0] > 200 && first[2] < 80,
        "space/unsupported must fail-closed the url layer (fill red), not tile blue, got {first:?}"
    );
}

#[test]
fn mask_image_url_reuses_jail_texture() {
    let (fixture_dir, png_path) = blue_tile_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let surface = nana_ui_scene::QuadSurfacePaint {
        mask: Some(nana_ui_core::MaskImage::Url(
            png_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("blue-tile.png")
                .to_string(),
        )),
        ..Default::default()
    };
    let sample = paint_surface_sample(surface, [16.0, 16.0], [16, 16], [1.0, 0.0, 0.0, 1.0], 8, 8);
    super::image_url::reset_test_url_base();
    assert!(
        sample[0] > 200 && sample[2] < 80,
        "opaque mask url must keep the fill, got {sample:?}"
    );
}

fn nine_slice_fixture_png() -> (std::path::PathBuf, std::path::PathBuf) {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixture_dir).expect("fixture dir");
    let png_path = fixture_dir.join("nine-slice.png");
    if !png_path.exists() {
        let mut img = image::RgbaImage::new(8, 8);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            let region_x = if x < 2 {
                0
            } else if x < 6 {
                1
            } else {
                2
            };
            let region_y = if y < 2 {
                0
            } else if y < 6 {
                1
            } else {
                2
            };
            *pixel = match (region_x, region_y) {
                (0, 0) => image::Rgba([255, 0, 0, 255]),
                (1, 0) => image::Rgba([0, 255, 0, 255]),
                (2, 0) => image::Rgba([0, 0, 255, 255]),
                (0, 1) => image::Rgba([0, 255, 255, 255]),
                (1, 1) => image::Rgba([255, 255, 255, 255]),
                (2, 1) => image::Rgba([255, 0, 255, 255]),
                (0, 2) => image::Rgba([255, 255, 0, 255]),
                (1, 2) => image::Rgba([0, 0, 0, 255]),
                _ => image::Rgba([128, 128, 128, 255]),
            };
        }
        img.save(&png_path).expect("write nine-slice png");
    }
    (fixture_dir, png_path)
}

fn paint_layout_sample(
    layout: nana_ui_core::LayoutStyle,
    logical: [f32; 2],
    sample_x: u32,
    sample_y: u32,
) -> [u8; 4] {
    let fill = layout.background.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        [extracted_div(
            1,
            &[],
            0.0,
            0.0,
            logical[0],
            logical[1],
            layout,
            Some(fill),
        )],
        [],
    );
    let physical = [logical[0] as u32, logical[1] as u32];
    let viewport = ScenePaintViewport {
        logical_size: logical,
        physical_size: physical,
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, physical[0], physical[1]);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui layout sample"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, physical[0], physical[1]);
    let sample = pixel(&pixels, physical[0], sample_x, sample_y);
    drop(texture);
    sample
}

#[test]
fn four_side_solid_borders_still_paint() {
    let layout = nana_ui_core::LayoutStyle {
        background: Some([0.1, 0.1, 0.1, 1.0]),
        border_top_width: Some(6.0),
        border_right_width: Some(6.0),
        border_bottom_width: Some(6.0),
        border_left_width: Some(6.0),
        border_top_color: Some([1.0, 0.0, 0.0, 1.0]),
        border_right_color: Some([0.0, 1.0, 0.0, 1.0]),
        border_bottom_color: Some([0.0, 0.0, 1.0, 1.0]),
        border_left_color: Some([1.0, 1.0, 0.0, 1.0]),
        ..Default::default()
    };
    let top = paint_layout_sample(layout.clone(), [40.0, 40.0], 20, 2);
    let right = paint_layout_sample(layout.clone(), [40.0, 40.0], 38, 20);
    let bottom = paint_layout_sample(layout.clone(), [40.0, 40.0], 20, 38);
    let left = paint_layout_sample(layout, [40.0, 40.0], 2, 20);
    assert!(
        top[0] > 200 && top[1] < 80,
        "top must stay red, got {top:?}"
    );
    assert!(
        right[1] > 200 && right[0] < 80,
        "right must stay green, got {right:?}"
    );
    assert!(
        bottom[2] > 200 && bottom[0] < 80,
        "bottom must stay blue, got {bottom:?}"
    );
    assert!(
        left[0] > 200 && left[1] > 200 && left[2] < 80,
        "left must stay yellow, got {left:?}"
    );
}

#[test]
fn border_image_url_nine_slice_paints_corners_and_fill() {
    let (fixture_dir, png_path) = nine_slice_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let url = png_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("nine-slice.png")
        .to_string();
    let surface = nana_ui_scene::QuadSurfacePaint {
        border_image: Some(nana_ui_core::BorderImageSpec {
            source: nana_ui_core::BackgroundImage::url(url),
            slice: [nana_ui_core::BorderImageSlice::Number(2.0); 4],
            fill: true,
        }),
        ..Default::default()
    };
    let tl = paint_surface_sample(
        surface.clone(),
        [40.0, 40.0],
        [40, 40],
        [0.1, 0.1, 0.1, 1.0],
        1,
        1,
    );
    let top = paint_surface_sample(
        surface.clone(),
        [40.0, 40.0],
        [40, 40],
        [0.1, 0.1, 0.1, 1.0],
        20,
        1,
    );
    let left = paint_surface_sample(
        surface.clone(),
        [40.0, 40.0],
        [40, 40],
        [0.1, 0.1, 0.1, 1.0],
        1,
        20,
    );
    let center = paint_surface_sample(
        surface.clone(),
        [40.0, 40.0],
        [40, 40],
        [0.1, 0.1, 0.1, 1.0],
        20,
        20,
    );
    let br = paint_surface_sample(
        surface,
        [40.0, 40.0],
        [40, 40],
        [0.1, 0.1, 0.1, 1.0],
        38,
        38,
    );
    super::image_url::reset_test_url_base();
    assert!(
        tl[0] > 200 && tl[1] < 80,
        "tl slice must be red, got {tl:?}"
    );
    assert!(
        top[1] > 200 && top[0] < 80,
        "top slice must be green, got {top:?}"
    );
    assert!(
        left[1] > 200 && left[2] > 200 && left[0] < 80,
        "left slice must be cyan, got {left:?}"
    );
    assert!(
        center[0] > 200 && center[1] > 200 && center[2] > 200,
        "fill center must be white, got {center:?}"
    );
    assert!(
        br[0] > 80 && br[0] < 180 && br[1] > 80 && br[1] < 180,
        "br slice must be gray, got {br:?}"
    );
}

#[test]
fn unsupported_border_image_does_not_paint_nine_slice() {
    let (fixture_dir, png_path) = nine_slice_fixture_png();
    super::set_background_image_url_base(fixture_dir);
    let url = png_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("nine-slice.png")
        .to_string();
    let layout = nana_ui_core::LayoutStyle {
        background: Some([0.1, 0.1, 0.1, 1.0]),
        paint: nana_ui_core::PaintStyle {
            unsupported_border_image: true,
            border_image: Some(nana_ui_core::BorderImageSpec {
                source: nana_ui_core::BackgroundImage::url(url),
                slice: [nana_ui_core::BorderImageSlice::Number(2.0); 4],
                fill: true,
            }),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    let center = paint_layout_sample(layout, [40.0, 40.0], 20, 20);
    super::image_url::reset_test_url_base();
    assert!(
        center[0] < 80 && center[1] < 80 && center[2] < 80,
        "sticky unsupported must not 9-slice-fill white, got {center:?}"
    );
}

#[test]
fn border_image_linear_gradient_nine_slice_paints() {
    let surface = nana_ui_scene::QuadSurfacePaint {
        border_image: Some(nana_ui_core::BorderImageSpec {
            source: nana_ui_core::BackgroundImage::Gradient(nana_ui_core::CssGradient::Linear(
                nana_ui_core::LinearGradient {
                    angle_deg: 180.0,
                    stops: vec![
                        nana_ui_core::GradientStop {
                            position: 0.0,
                            color: [1.0, 0.0, 0.0, 1.0],
                        },
                        nana_ui_core::GradientStop {
                            position: 1.0,
                            color: [0.0, 0.0, 1.0, 1.0],
                        },
                    ],
                },
            )),
            slice: [nana_ui_core::BorderImageSlice::Percent(25.0); 4],
            fill: true,
        }),
        ..Default::default()
    };
    let top = paint_surface_sample(
        surface.clone(),
        [40.0, 40.0],
        [40, 40],
        [0.0, 0.0, 0.0, 1.0],
        20,
        4,
    );
    let bottom = paint_surface_sample(
        surface,
        [40.0, 40.0],
        [40, 40],
        [0.0, 0.0, 0.0, 1.0],
        20,
        36,
    );
    assert!(
        top[0] > 180 && top[2] < 80,
        "gradient top slice must stay red, got {top:?}"
    );
    assert!(
        bottom[2] > 180 && bottom[0] < 80,
        "gradient bottom slice must stay blue, got {bottom:?}"
    );
}

#[test]
fn texture_content_reuses_prepared_ui_and_replacement_rebinds() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let source = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    let registry = register_host_texture("live", &source, 64, 64);
    let mut scene = UiScene::new();
    let mut node = host_texture_child(1, 99, 0.0, 0.0, 64.0, 64.0, "live");
    node.parent = None;
    scene.apply_delta([node], []);
    let mut painter = SceneWgpuPainter::for_test(format);
    let (texture, target) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |painter: &mut SceneWgpuPainter| {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .paint_encoder(
                &scene,
                &mut encoder,
                &target,
                viewport,
                Some(&registry),
                None,
            )
            .unwrap();
        readback_rgba(&device, &queue, encoder, &texture, 64, 64)
    };
    assert!(is_green_slot(pixel(&paint(&mut painter), 64, 32, 32)));
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &source,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
    registry.slot("live").invalidate().unwrap();
    assert!(is_red_slot(pixel(&paint(&mut painter), 64, 32, 32)));
    let work = painter.last_gpu_work().unwrap();
    assert_eq!(work.gpu_upload_bytes, 0);
    assert!(painter.last_gpu_timings().unwrap().batch.is_zero());
    let replacement = solid_texture_view(&device, &queue, format, 64, 64, wgpu::Color::GREEN);
    registry
        .get("live")
        .unwrap()
        .texture
        .replace_texture(&crate::test_gpu::wrap_view(&replacement));
    assert!(is_green_slot(pixel(&paint(&mut painter), 64, 32, 32)));
    registry.remove("live");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(matches!(
        painter.paint_encoder(
            &scene,
            &mut encoder,
            &target,
            viewport,
            Some(&registry),
            None
        ),
        Err(ScenePaintError::MissingCustomResource(_))
    ));
    assert!(painter.last_gpu_work().is_none());
    assert!(painter.last_gpu_timings().is_none());
}

#[test]
fn same_format_targets_retain_independent_composition() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut red = UiScene::new();
    red.apply_delta(
        [colored_quad_node(
            1,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
        )],
        [],
    );
    let mut green = UiScene::new();
    green.apply_delta(
        [colored_quad_node(
            2,
            0.0,
            0.0,
            64.0,
            64.0,
            [0.0, 1.0, 0.0, 1.0],
        )],
        [],
    );
    let (texture, target) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    for (id, scene, is_red) in [
        (1, &red, true),
        (2, &green, false),
        (1, &red, true),
        (2, &green, false),
    ] {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .paint_target_encoder(
                RenderTargetId(id),
                scene,
                &mut encoder,
                &target,
                viewport,
                None,
                None,
            )
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
        assert_eq!(is_red_slot(pixel(&pixels, 64, 32, 32)), is_red);
    }
    assert!(painter.last_gpu_timings().unwrap().batch.is_zero());
    painter.remove_target(RenderTargetId(1));
    painter.remove_target(RenderTargetId(2));
    assert!(painter.targets.is_empty());
}

#[test]
fn local_color_update_uploads_changed_ranges_and_paints_latest_pixels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut scene = UiScene::new();
    scene.apply_delta(
        (1..=100).map(|id| colored_quad_node(id, 0.0, 0.0, 64.0, 64.0, [0.0, 1.0, 0.0, 1.0])),
        [],
    );
    let (texture, target) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0; 4],
        clear: true,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    assert!(is_green_slot(pixel(&pixels, 64, 32, 32)));
    let initial = painter.last_gpu_work().unwrap().gpu_upload_bytes;
    scene.apply_delta(
        [colored_quad_node(
            100,
            0.0,
            0.0,
            64.0,
            64.0,
            [1.0, 0.0, 0.0, 1.0],
        )],
        [],
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    assert!(is_red_slot(pixel(&pixels, 64, 32, 32)));
    assert!(painter.last_gpu_work().unwrap().gpu_upload_bytes < initial / 4);
}

#[test]
fn quad_color_batch_uploads_only_changed_instances() {
    use crate::gpu_work::GpuWorkSink;
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = test_device();
    let mut pipeline = super::quad::QuadPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut url_cache = super::url_texture_cache::UrlTextureCache::default();
    let clip = super::clip::FragmentClip::PASS;
    let bounds =
        |index: usize| super::clip::LogicalRect::from_xywh(index as f32 * 8.0, 0.0, 6.0, 6.0);
    let mut push_batch = |pipeline: &mut super::quad::QuadPipeline, colors: &[[f32; 4]]| {
        pipeline.begin_frame(1.0);
        for (index, color) in colors.iter().enumerate() {
            pipeline.push(
                &device,
                &queue,
                &mut url_cache,
                bounds(index),
                super::clip::LogicalRect::from_xywh(0.0, 0.0, 128.0, 64.0),
                clip,
                super::clip::IDENTITY_AFFINE,
                [0.0, 0.0],
                Some(*color),
                None,
                0.0,
                [0.0; 4],
                None,
                1.0,
                &QuadSurfacePaint::default(),
            );
        }
        let sink = GpuWorkSink::new();
        pipeline.upload(&device, &queue, &url_cache, [128, 64], 1.0, Some(&sink));
        sink.snapshot()
    };

    let colors = vec![[0.1, 0.2, 0.3, 1.0]; 32];
    let initial = push_batch(&mut pipeline, &colors);
    let unchanged = push_batch(&mut pipeline, &colors);
    assert!(
        unchanged.gpu_upload_bytes < initial.gpu_upload_bytes,
        "a stable QuadColorBatch must only upload its fixed frame uniform"
    );

    let mut changed_colors = colors;
    changed_colors[5] = [0.9, 0.1, 0.2, 1.0];
    let changed = push_batch(&mut pipeline, &changed_colors);
    assert!(
        changed.gpu_upload_bytes > unchanged.gpu_upload_bytes,
        "initial={}, unchanged={}, changed={}",
        initial.gpu_upload_bytes,
        unchanged.gpu_upload_bytes,
        changed.gpu_upload_bytes
    );
    assert!(
        changed.gpu_upload_bytes < initial.gpu_upload_bytes,
        "changing one batch item should upload less than the cold batch: {} vs {}",
        changed.gpu_upload_bytes,
        initial.gpu_upload_bytes
    );
}

#[test]
fn icon_batch_reuses_atlas_and_vertex_uploads() {
    use crate::gpu_work::GpuWorkSink;
    use nana_ui_core::Icon;

    let (device, queue) = test_device();
    let mut pipeline = super::icon::IconPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let push_batch = |pipeline: &mut super::icon::IconPipeline| {
        pipeline.begin_frame([128, 64]);
        for index in 0..24 {
            pipeline.prepare(
                &device,
                &queue,
                super::clip::LogicalRect::from_xywh(index as f32 * 5.0, 4.0, 4.0, 4.0),
                super::clip::IDENTITY_AFFINE,
                [0.0, 0.0],
                1.0,
                Icon::Close,
                [0.2, 0.4, 0.8, 1.0],
                1.0,
                super::clip::FragmentClip::PASS,
            );
        }
        let sink = GpuWorkSink::new();
        pipeline.upload(&device, &queue, Some(&sink));
        sink.snapshot()
    };

    let first = push_batch(&mut pipeline);
    let second = push_batch(&mut pipeline);
    assert!(first.gpu_upload_bytes > 0);
    assert_eq!(
        second.gpu_upload_bytes, 0,
        "a stable IconBatch must reuse its atlas and vertex buffer"
    );
}

#[test]
fn an_icon_a_transform_scales_is_rasterized_at_the_size_it_is_seen() {
    // Scaled about its centre onto the 16 px (or 32 px) square an upright icon
    // of that size covers: drawn from the bitmap of its own size, a shrunk
    // glyph would alias and a magnified one blur.
    let (device, queue) = test_device();
    let paint = |size: f32, scale: f32| {
        let at = 32.0 - size * 0.5;
        let mut icon = colored_quad_node(1, at, at, size, size, [0.0; 4]);
        icon.standard_visual = Some(StandardVisual::Icon {
            icon: nana_ui_core::Icon::Search,
            size,
            tooltip: None,
        });
        icon.standard_visual_foreground = Some([1.0; 4]);
        Arc::make_mut(&mut icon.source_style.layout).transform = scale_transform(scale);
        let mut scene = UiScene::new();
        scene.apply_delta([icon], []);
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0; 2],
            [64, 64],
            1.0,
        )
    };
    for (size, scale) in [(32.0, 0.5), (16.0, 2.0)] {
        let (scaled, upright) = (paint(size, scale), paint(size * scale, 1.0));
        let (scaled, upright) = (scaled.as_chunks::<4>().0, upright.as_chunks::<4>().0);
        let differing = scaled
            .iter()
            .zip(upright)
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24))
            .count();
        let inked = upright.iter().filter(|pixel| pixel[0] > 24).count();
        assert!(inked > 30, "the reference must paint something");
        assert!(
            differing * 20 < inked,
            "a {size} px icon under scale({scale}) must be the {} px glyph: \
             {differing} of {inked} inked pixels differ",
            size * scale
        );
    }
}

#[test]
fn an_icon_squeezed_along_one_axis_keeps_its_ink_at_any_sub_pixel_phase() {
    // As text: rasterized for `scale(1, 0.3)`'s larger scale, the other axis
    // is shrunk 3.3 times further; filtered over each pixel's footprint, the
    // ink the glyph lays down does not depend on its sub-pixel phase.
    let (device, queue) = test_device();
    let ink = |phase: f32| {
        let mut icon = colored_quad_node(1, 16.0, 16.0 + phase, 32.0, 32.0, [0.0; 4]);
        icon.standard_visual = Some(StandardVisual::Icon {
            icon: nana_ui_core::Icon::Search,
            size: 32.0,
            tooltip: None,
        });
        icon.standard_visual_foreground = Some([1.0; 4]);
        Arc::make_mut(&mut icon.source_style.layout).transform = Some(PaintTransform {
            d: 0.3,
            ..PaintTransform::default()
        });
        let mut scene = UiScene::new();
        scene.apply_delta([icon], []);
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0; 2],
            [64, 64],
            1.0,
        );
        pixels
            .chunks(4)
            .map(|pixel| u64::from(pixel[1]))
            .sum::<u64>() as f32
    };
    let inks = [0.0, 0.25, 0.5, 0.75].map(ink);
    let mean = inks.iter().sum::<f32>() / 4.0;
    let spread = inks
        .iter()
        .fold(0.0f32, |spread, ink| spread.max((ink - mean).abs()))
        / mean;
    assert!(
        spread < 0.01,
        "ink varies {:.1}% with the sub-pixel phase: {inks:?}",
        spread * 100.0
    );
}

#[test]
fn a_frosted_panel_edge_stays_one_device_pixel_under_transform_scale() {
    // A round frosted panel over green, desaturated to grey (luminance
    // 0.7152) with no fill of its own: its red channel is 0.7152 of its
    // coverage, whose rim must ramp over one device pixel.
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let filter = nana_ui_core::BackdropFilter {
        blur_radius: 2.0,
        saturate: 0.0,
    };
    for (label, [x, y, w, h], scale, factor) in SCALED_CIRCLES {
        let mut panel = frost_quad_node_with_fill(2, x, y, w, h, [0.0; 4], filter);
        let layout = Arc::make_mut(&mut panel.source_style.layout);
        layout.border_radius = Some(w.min(h) * 0.5);
        layout.transform = scale_transform(scale);
        let mut scene = UiScene::new();
        scene.apply_delta(
            [
                colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 1.0, 0.0, 1.0]),
                panel,
            ],
            [],
        );
        let side = (64.0 * factor) as u32;
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0, 64.0],
            [side, side],
            factor,
        );
        let center = [(x + w * 0.5) * factor, (y + h * 0.5) * factor];
        let radius = w.min(h) * 0.5 * scale * factor;
        for py in 0..side {
            for px in 0..side {
                let from_rim =
                    (px as f32 + 0.5 - center[0]).hypot(py as f32 + 0.5 - center[1]) - radius;
                if from_rim.abs() > 1.5 {
                    continue;
                }
                let expected = (0.5 - from_rim).clamp(0.0, 1.0) * 0.7152 * 255.0;
                let red = f32::from(pixel(&pixels, side, px, py)[0]);
                assert!(
                    (red - expected).abs() <= 16.0,
                    "{label}: ({px},{py}) is {from_rim:+.3} px from the rim, \
                     expected red {expected:.0}, got {red}"
                );
            }
        }
    }
}

#[test]
fn custom_preparation_reuse_requires_explicit_version_and_tracks_changes() {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    #[derive(Debug, Default)]
    struct Renderer {
        versioned: AtomicBool,
        version: AtomicU64,
        prepared: AtomicUsize,
    }
    impl SceneGpuRenderer for Renderer {
        fn preparation_version(&self, _: &CustomRenderNode) -> Option<u64> {
            self.versioned
                .load(Ordering::Relaxed)
                .then(|| self.version.load(Ordering::Relaxed))
        }
        fn prepare(&self, _: &SceneGpuNode, _: SceneGpuPrepareContext<'_>) {
            self.prepared.fetch_add(1, Ordering::Relaxed);
        }
        fn render(&self, _: &SceneGpuNode, _: SceneGpuRenderContext<'_>) {}
    }
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut node = host_texture_child(1, 99, 0.0, 0.0, 64.0, 64.0, "resource");
    node.parent = None;
    node.custom_render.as_mut().unwrap().renderer = Arc::from("versioned");
    let mut scene = UiScene::new();
    scene.apply_delta([node], []);
    let renderer = Arc::new(Renderer::default());
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("versioned", renderer.clone());
    let (_, target) = test_copy_target(&device, format, 64, 64);
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0; 4],
        clear: true,
    };
    let mut paint = || {
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                &scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        queue.submit([encoder.finish()]);
    };
    paint();
    paint();
    assert_eq!(renderer.prepared.load(Ordering::Relaxed), 2);
    renderer.versioned.store(true, Ordering::Relaxed);
    paint();
    paint();
    assert_eq!(renderer.prepared.load(Ordering::Relaxed), 3);
    renderer.version.store(1, Ordering::Relaxed);
    paint();
    assert_eq!(renderer.prepared.load(Ordering::Relaxed), 4);
}

#[test]
fn default_gpu_view_versions_preparation_and_tracks_param_changes() {
    use crate::DefaultGpuViewRenderer;
    use nana_ui_runtime::{GpuView, GpuViewPalette};

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("gpu-view", Arc::new(DefaultGpuViewRenderer::new()));

    // A quad sibling makes this the real shape: a shader node inside ordinary
    // UI. batch_rebuilds is recorded from the quad/mesh upload, so a lone
    // custom node would leave the counter silent whether or not the display
    // list was rebuilt.
    let scene_with = |view: &GpuView| {
        let mut node = host_texture_child(2, 1, 0.0, 0.0, 32.0, 32.0, "0");
        node.custom_render = Some(view.custom_render());
        let mut root = colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]);
        root.children = Arc::new(vec![StableNodeId::new(2).unwrap()]);
        let mut scene = UiScene::new();
        scene.apply_delta([root, node], []);
        scene
    };
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |painter: &mut SceneWgpuPainter, scene: &UiScene| {
        let (texture, target) = test_copy_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded gpu-view frame");
        let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
        (work, pixels)
    };

    let view = GpuView::new(0).palette(GpuViewPalette {
        background: [0.0, 0.0, 0.0, 1.0],
        accent: [1.0, 1.0, 1.0, 1.0],
    });
    let scene = scene_with(&view);
    let (cold, _) = paint(&mut painter, &scene);
    let (warm, first_pixels) = paint(&mut painter, &scene);
    assert!(
        cold.batch_rebuilds > 0,
        "the first frame must build a batch"
    );
    assert_eq!(
        warm.batch_rebuilds, 0,
        "an unchanged gpu-view node must not rebuild the whole display list"
    );

    // The version must move with `params`, or a palette change would silently
    // reuse the previous frame's uniforms.
    let recolored = view.palette(GpuViewPalette {
        background: [1.0, 0.0, 0.0, 1.0],
        accent: [1.0, 0.0, 0.0, 1.0],
    });
    let recolored_scene = scene_with(&recolored);
    let (_, recolored_pixels) = paint(&mut painter, &recolored_scene);
    assert_ne!(
        first_pixels, recolored_pixels,
        "changing gpu-view params must repaint, not reuse the prepared uniforms"
    );
}

#[test]
fn default_gpu_view_evicts_slots_for_nodes_that_left_the_scene() {
    use crate::DefaultGpuViewRenderer;
    use nana_ui_runtime::GpuView;

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let renderer = Arc::new(DefaultGpuViewRenderer::new());
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("gpu-view", renderer.clone());
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let (_texture, target) = test_copy_target(&device, format, 64, 64);

    // A scrolling list retires one PrimitiveId per frame. Slots are keyed by
    // that id, so without eviction every id that ever painted keeps a buffer
    // and a bind group alive.
    const FRAMES: u64 = 24;
    for index in 0..FRAMES {
        let id = index + 2;
        let mut node = host_texture_child(id, 1, 0.0, 0.0, 32.0, 32.0, "0");
        node.custom_render = Some(GpuView::new(0).custom_render());
        let mut root = colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]);
        root.children = Arc::new(vec![StableNodeId::new(id).unwrap()]);
        let mut scene = UiScene::new();
        scene.apply_delta([root, node], []);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                &scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        queue.submit([encoder.finish()]);
    }
    // The renderer keeps a few passes of grace so scrolling a node just out of
    // view and back does not rebuild it; what must not happen is growth with
    // the number of ids that ever painted.
    let live = renderer.prepared_slot_count();
    assert!(
        live <= 8,
        "retired gpu-view nodes must not keep their uniform buffers alive: {live} slots after \
         {FRAMES} distinct nodes"
    );
}

#[test]
fn adjacent_icons_batch_into_one_draw_whatever_their_glyphs() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 256;
    let glyphs = [
        nana_ui_core::Icon::About,
        nana_ui_core::Icon::Add,
        nana_ui_core::Icon::Appearance,
        nana_ui_core::Icon::ArrowLeft,
        nana_ui_core::Icon::ArrowRight,
        nana_ui_core::Icon::ArrowUp,
        nana_ui_core::Icon::Bot,
        nana_ui_core::Icon::ChevronDown,
        nana_ui_core::Icon::Chart,
        nana_ui_core::Icon::Close,
        nana_ui_core::Icon::Eye,
        nana_ui_core::Icon::Folder,
    ];
    // `vary` picks each icon's glyph and size, so one builder covers a row of
    // identical icons, a toolbar of distinct glyphs, and one glyph at many
    // sizes. All three used to be one atlas texture per `(glyph, size)`.
    let icon_row = |count: u64, vary: &dyn Fn(usize) -> (nana_ui_core::Icon, f32)| {
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        for index in 0..count {
            let id = index + 2;
            children.push(StableNodeId::new(id).unwrap());
            let (glyph, size) = vary(index as usize);
            // No background: an icon node that also paints a quad would put a
            // Quad command between every pair of icons, which is the dense-list
            // shape, not the run this merge is about.
            let mut icon = extracted_div(
                id,
                &[],
                4.0 + index as f32 * 20.0,
                4.0,
                size,
                size,
                nana_ui_core::LayoutStyle::default(),
                None,
            );
            icon.parent = Some(StableNodeId::new(1).unwrap());
            icon.standard_visual = Some(StandardVisual::Icon {
                icon: glyph,
                size,
                tooltip: None,
            });
            icon.standard_visual_foreground = Some([1.0; 4]);
            nodes.push(icon);
        }
        let mut root =
            colored_quad_node(1, 0.0, 0.0, SIDE as f32, SIDE as f32, [0.0, 0.0, 0.2, 1.0]);
        root.children = Arc::new(children);
        let mut scene = UiScene::new();
        nodes.insert(0, root);
        scene.apply_delta(nodes, []);
        scene
    };
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded icon frame");
        (
            work,
            readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE),
        )
    };
    let same = |_: usize| (nana_ui_core::Icon::Close, 16.0);
    let (one, _) = paint(&icon_row(1, &same));
    assert!(one.draw_calls > 0, "the single-icon scene must draw");
    for (label, vary) in [
        (
            "one glyph",
            &same as &dyn Fn(usize) -> (nana_ui_core::Icon, f32),
        ),
        ("distinct glyphs", &|index: usize| {
            (
                [
                    nana_ui_core::Icon::About,
                    nana_ui_core::Icon::Add,
                    nana_ui_core::Icon::Appearance,
                    nana_ui_core::Icon::ArrowLeft,
                    nana_ui_core::Icon::ArrowRight,
                    nana_ui_core::Icon::ArrowUp,
                    nana_ui_core::Icon::Bot,
                    nana_ui_core::Icon::ChevronDown,
                    nana_ui_core::Icon::Chart,
                    nana_ui_core::Icon::Close,
                    nana_ui_core::Icon::Eye,
                    nana_ui_core::Icon::Folder,
                ][index % 12],
                16.0,
            )
        }),
        ("one glyph at distinct sizes", &|index: usize| {
            (nana_ui_core::Icon::Close, 8.0 + index as f32)
        }),
    ] {
        let (many, pixels) = paint(&icon_row(12, vary));
        assert_eq!(
            many.draw_calls, one.draw_calls,
            "12 adjacent icons ({label}) must collapse into the same single draw as one              icon: {} vs {}",
            many.draw_calls, one.draw_calls
        );
        // One draw is worthless if the shared atlas lost a glyph, so every
        // icon must still ink inside its own cell of the row.
        for index in 0..12u32 {
            let left = 4 + index * 20;
            let mut inked = false;
            for y in 4..24 {
                for x in left..left + 20 {
                    if x < SIDE && pixel(&pixels, SIDE, x, y)[0] > 100 {
                        inked = true;
                    }
                }
            }
            assert!(
                inked,
                "icon {index} ({label}) must paint inside its own slot"
            );
        }
    }
    // Sanity: the glyph list the toolbar case draws from really is distinct.
    let mut pointers: Vec<_> = glyphs.iter().map(|glyph| glyph.as_ptr()).collect();
    pointers.sort();
    pointers.dedup();
    assert_eq!(
        pointers.len(),
        glyphs.len(),
        "the toolbar glyphs must differ"
    );
}

/// `(background quad, label)` rows: the shape overlap-aware batching exists
/// for. Each label sits inside its own row, so no label ever covers a later
/// row's background and every row can join the batch the first row opened.
#[test]
fn quad_and_label_rows_keep_a_constant_draw_count() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 224;
    let rows = |count: u64| {
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut id = 2u64;
        for row in 0..count {
            let y = 4.0 + row as f32 * 20.0;
            let mut quad = colored_quad_node(id, 2.0, y, 200.0, 18.0, [0.15, 0.2, 0.4, 1.0]);
            quad.parent = Some(StableNodeId::new(1).unwrap());
            children.push(StableNodeId::new(id).unwrap());
            nodes.push(quad);
            id += 1;
            let mut label = extracted_div(
                id,
                &[],
                6.0,
                y + 1.0,
                190.0,
                16.0,
                nana_ui_core::LayoutStyle::default(),
                None,
            );
            label.parent = Some(StableNodeId::new(1).unwrap());
            label.style = Arc::new(ComputedStyle {
                color: Some([1.0, 0.9, 0.2, 1.0]),
                font_size: 11.0,
                ..ComputedStyle::default()
            });
            label.text = Some(TextContent {
                value: format!("Row {row}").into(),
            });
            children.push(StableNodeId::new(id).unwrap());
            nodes.push(label);
            id += 1;
        }
        let mut root = colored_quad_node(
            1,
            0.0,
            0.0,
            SIDE as f32,
            SIDE as f32,
            [0.05, 0.06, 0.1, 1.0],
        );
        root.children = Arc::new(children);
        nodes.insert(0, root);
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        scene
    };
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded row frame");
        (
            work,
            readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE),
        )
    };
    let (one, _) = paint(&rows(1));
    let (many, pixels) = paint(&rows(9));
    assert_eq!(
        many.draw_calls, one.draw_calls,
        "a (quad, label) list must not cost a draw per row: {} rows vs 1 row is {} vs {}",
        9, many.draw_calls, one.draw_calls
    );
    // Every row still paints its own background and its own label.
    let page = pixel(&pixels, SIDE, SIDE - 4, SIDE - 4);
    for row in 0..9u32 {
        let band = 4 + row * 20;
        let mut background = false;
        let mut ink = false;
        for y in band..band + 18 {
            for x in 2..202 {
                let color = pixel(&pixels, SIDE, x, y);
                background |= color != page;
                ink |= color[0] > 150 && color[1] > 130 && color[2] < 120;
            }
        }
        assert!(background, "row {row} must paint its background");
        assert!(ink, "row {row} must paint its label over that background");
    }
}

/// The other half of the contract: a quad that *does* cover earlier text must
/// stay behind it in the list, however many quad batches are open.
#[test]
fn a_quad_over_earlier_text_is_not_folded_ahead_of_it() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 128;
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    let mut id = 2u64;
    // Three rows of label, then a scrim over the middle one, then a fourth row.
    for row in 0..4u64 {
        let y = 4.0 + row as f32 * 28.0;
        let mut quad = colored_quad_node(id, 2.0, y, 120.0, 26.0, [0.1, 0.12, 0.3, 1.0]);
        quad.parent = Some(StableNodeId::new(1).unwrap());
        children.push(StableNodeId::new(id).unwrap());
        nodes.push(quad);
        id += 1;
        let mut label = extracted_div(
            id,
            &[],
            6.0,
            y + 4.0,
            110.0,
            20.0,
            nana_ui_core::LayoutStyle::default(),
            None,
        );
        label.parent = Some(StableNodeId::new(1).unwrap());
        label.style = Arc::new(ComputedStyle {
            color: Some([1.0, 0.95, 0.1, 1.0]),
            font_size: 16.0,
            ..ComputedStyle::default()
        });
        label.text = Some(TextContent {
            value: "HHHH".into(),
        });
        children.push(StableNodeId::new(id).unwrap());
        nodes.push(label);
        id += 1;
        if row == 2 {
            // Opaque, and it covers the rows above it.
            let mut scrim = colored_quad_node(id, 0.0, 0.0, 128.0, 92.0, [0.8, 0.05, 0.05, 1.0]);
            scrim.parent = Some(StableNodeId::new(1).unwrap());
            children.push(StableNodeId::new(id).unwrap());
            nodes.push(scrim);
            id += 1;
        }
    }
    let mut root = colored_quad_node(1, 0.0, 0.0, SIDE as f32, SIDE as f32, [0.0, 0.0, 0.0, 1.0]);
    root.children = Arc::new(children);
    nodes.insert(0, root);
    let mut scene = UiScene::new();
    scene.apply_delta(nodes, []);
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut painter = SceneWgpuPainter::for_test(format);
    let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE);
    let label_ink = |pixels: &[u8], top: u32, bottom: u32| {
        (top..bottom)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let color = pixel(pixels, SIDE, *x, *y);
                color[0] > 150 && color[1] > 140 && color[2] < 120
            })
            .count()
    };
    assert_eq!(
        label_ink(&pixels, 0, 92),
        0,
        "the scrim comes after those labels in document order and must cover them"
    );
    assert!(
        label_ink(&pixels, 92, SIDE) > 0,
        "the row below the scrim must still paint its label"
    );
}

/// Folding a quad into an earlier batch can leave glyphs at the end of the
/// list. The dest sample count is decided on document order, so that must not
/// turn a single-sample frame into a 4x MSAA one.
#[test]
fn batch_merging_does_not_flip_the_dest_sample_count() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 64;
    let mut first = colored_quad_node(2, 2.0, 2.0, 24.0, 12.0, [0.2, 0.3, 0.9, 1.0]);
    first.parent = Some(StableNodeId::new(1).unwrap());
    let mut label = extracted_div(
        3,
        &[],
        2.0,
        18.0,
        60.0,
        14.0,
        nana_ui_core::LayoutStyle::default(),
        None,
    );
    label.parent = Some(StableNodeId::new(1).unwrap());
    label.style = Arc::new(ComputedStyle {
        color: Some([1.0, 1.0, 1.0, 1.0]),
        font_size: 10.0,
        ..ComputedStyle::default()
    });
    label.text = Some(TextContent { value: "Hi".into() });
    // Clear of the label, so it folds into the first quad's batch and the
    // merged list ends with the glyphs.
    let mut second = colored_quad_node(4, 2.0, 40.0, 24.0, 12.0, [0.9, 0.3, 0.2, 1.0]);
    second.parent = Some(StableNodeId::new(1).unwrap());
    let mut root = colored_quad_node(1, 0.0, 0.0, SIDE as f32, SIDE as f32, [0.0, 0.0, 0.0, 1.0]);
    root.children = Arc::new(vec![
        StableNodeId::new(2).unwrap(),
        StableNodeId::new(3).unwrap(),
        StableNodeId::new(4).unwrap(),
    ]);
    let mut scene = UiScene::new();
    scene.apply_delta(vec![root, first, label, second], []);
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut painter = SceneWgpuPainter::for_test(format);
    let target = test_target(&device, format, SIDE, SIDE);
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(&scene, &mut encoder, &target, viewport, None, None)
        .unwrap();
    queue.submit([encoder.finish()]);
    let work = painter.last_gpu_work().expect("encoded frame");
    let counts = painter.last_dest_pass_counts.expect("encoded frame");
    assert_eq!(
        work.draw_calls, 3,
        "the two quads must share one draw: quads, glyphs, blit"
    );
    assert_eq!(
        counts.msaa, 0,
        "a quad after a glyph keeps the frame single-sampled even once the \
         quad is folded in front of it, got {counts:?}"
    );
}

#[test]
fn open_text_run_survives_shape_cache_eviction() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 96;
    // More distinct paragraphs than the shape cache holds. The visible ones
    // open a run that names its shaped buffers; shaping the rest must not
    // evict a buffer the open run still needs.
    const LABELS: u64 = 900;
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    for index in 0..LABELS {
        let id = index + 2;
        children.push(StableNodeId::new(id).unwrap());
        let mut node = extracted_div(
            id,
            &[],
            2.0,
            index as f32 * 12.0,
            90.0,
            12.0,
            nana_ui_core::LayoutStyle::default(),
            None,
        );
        node.parent = Some(StableNodeId::new(1).unwrap());
        node.style = Arc::new(ComputedStyle {
            color: Some([1.0, 1.0, 1.0, 1.0]),
            font_size: 10.0,
            ..ComputedStyle::default()
        });
        node.text = Some(TextContent {
            value: format!("row-{index}").into(),
        });
        nodes.push(node);
    }
    let mut root = colored_quad_node(1, 0.0, 0.0, SIDE as f32, SIDE as f32, [0.0, 0.0, 0.3, 1.0]);
    root.children = Arc::new(children);
    nodes.insert(0, root);
    let mut scene = UiScene::new();
    scene.apply_delta(nodes, []);
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let mut painter = SceneWgpuPainter::for_test(format);
    let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE);
    for row in 0..8u32 {
        let inked = (row * 12..row * 12 + 12)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .any(|(x, y)| {
                let color = pixel(&pixels, SIDE, x, y);
                color[0] > 120 && color[1] > 120 && color[2] > 120
            });
        assert!(
            inked,
            "row {row} must survive the shape-cache pressure from the offscreen rows"
        );
    }
}

#[test]
fn merged_text_run_keeps_document_order_between_overlapping_labels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 96;
    // Two solid blocks on the same box: they merge into one run, and the run's
    // glyph order must still be document order or the first label wins.
    let scene_of = |both: bool| {
        let under = overflowing_text_child(2, 1, 4.0, 4.0, 88.0, 88.0, [1.0, 0.0, 0.0, 1.0]);
        let over = overflowing_text_child(3, 1, 4.0, 4.0, 88.0, 88.0, [0.0, 0.0, 1.0, 1.0]);
        let mut root =
            colored_quad_node(1, 0.0, 0.0, SIDE as f32, SIDE as f32, [0.0, 0.0, 0.0, 1.0]);
        let mut nodes = vec![over];
        root.children = Arc::new(if both {
            nodes.insert(0, under);
            vec![StableNodeId::new(2).unwrap(), StableNodeId::new(3).unwrap()]
        } else {
            vec![StableNodeId::new(3).unwrap()]
        });
        nodes.insert(0, root);
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        scene
    };
    let scene = scene_of(true);

    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded frame");
        (
            work,
            readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE),
        )
    };
    let (lone, _) = paint(&scene_of(false));
    let (both, pixels) = paint(&scene);
    assert_eq!(
        both.draw_calls, lone.draw_calls,
        "the two labels must share one run, or this test says nothing about \
         order inside a merged run"
    );
    let covered = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .map(|(x, y)| pixel(&pixels, SIDE, x, y))
        .filter(|color| color[0] > 40 || color[2] > 40)
        .collect::<Vec<_>>();
    assert!(
        !covered.is_empty(),
        "the overlapping blocks must paint something"
    );
    assert!(
        covered.iter().all(|color| color[2] >= color[0]),
        "the later label must stay on top inside the merged run"
    );
}

#[test]
fn text_below_the_clip_band_costs_no_draw_and_no_pixels() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    const SIDE: u32 = 64;
    // `y` places the label's content box; `None` means "no extra label".
    let scene_with = |extra: &[f32]| {
        let mut nodes = Vec::new();
        let mut children = vec![StableNodeId::new(2).unwrap()];
        let label = |id: u64, y: f32| {
            let mut node = extracted_div(
                id,
                &[],
                2.0,
                y,
                60.0,
                20.0,
                nana_ui_core::LayoutStyle::default(),
                None,
            );
            node.parent = Some(StableNodeId::new(1).unwrap());
            node.style = Arc::new(ComputedStyle {
                color: Some([1.0, 1.0, 1.0, 1.0]),
                font_size: 16.0,
                ..ComputedStyle::default()
            });
            node.text = Some(TextContent { value: "AB".into() });
            node
        };
        nodes.push(label(2, 2.0));
        for (index, y) in extra.iter().enumerate() {
            let id = index as u64 + 3;
            children.push(StableNodeId::new(id).unwrap());
            nodes.push(label(id, *y));
        }
        let mut root = colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 1.0, 1.0]);
        root.children = Arc::new(children);
        nodes.insert(0, root);
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        scene
    };
    let viewport = ScenePaintViewport {
        logical_size: [SIDE as f32, SIDE as f32],
        physical_size: [SIDE, SIDE],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let paint = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let (texture, view) = test_copy_target(&device, format, SIDE, SIDE);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded text frame");
        let pixels = readback_rgba(&device, &queue, encoder, &texture, SIDE, SIDE);
        (work, pixels)
    };

    let (lone, lone_pixels) = paint(&scene_with(&[]));
    assert!(lone.draw_calls > 0, "the single-label scene must draw");

    // Eight labels far below the 64px viewport: the resolver drops every one
    // of their layout runs before a glyph is rasterized, so they must cost
    // neither a draw call nor a pixel.
    let (below, below_pixels) = paint(&scene_with(&[
        200.0, 220.0, 240.0, 260.0, 280.0, 300.0, 320.0, 340.0,
    ]));
    assert_eq!(
        below.draw_calls, lone.draw_calls,
        "labels entirely below the clip band must not be prepared or drawn: {} vs {}",
        below.draw_calls, lone.draw_calls
    );
    assert!(
        below_pixels == lone_pixels,
        "labels entirely below the clip band must not change any pixel"
    );

    // One label straddling the bottom edge still has a run in the band, so it
    // must reach the GPU. It shares the first label's scissor and is its
    // document-order neighbour, so it rides the same merged run rather than
    // adding a draw — the ink is what proves it was not dropped.
    let (straddling, straddling_pixels) = paint(&scene_with(&[56.0]));
    assert_eq!(
        straddling.draw_calls, lone.draw_calls,
        "a label crossing the bottom edge merges into the run before it"
    );
    let ink_in_last_rows = |pixels: &[u8]| {
        (SIDE - 6..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let color = pixel(pixels, SIDE, *x, *y);
                color[0] > 120 && color[1] > 120
            })
            .count()
    };
    assert_eq!(
        ink_in_last_rows(&lone_pixels),
        0,
        "the baseline scene must leave the bottom rows empty"
    );
    assert!(
        ink_in_last_rows(&straddling_pixels) > 0,
        "a label crossing the bottom edge must still paint ink there"
    );
}

#[test]
fn batched_gpu_view_run_paints_each_node_like_a_lone_node() {
    use crate::DefaultGpuViewRenderer;
    use nana_ui_runtime::{GpuView, GpuViewPalette};

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("gpu-view", Arc::new(DefaultGpuViewRenderer::new()));
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    let palettes = [
        GpuViewPalette {
            background: [0.9, 0.1, 0.1, 1.0],
            accent: [0.9, 0.4, 0.4, 1.0],
        },
        GpuViewPalette {
            background: [0.1, 0.9, 0.1, 1.0],
            accent: [0.4, 0.9, 0.4, 1.0],
        },
        GpuViewPalette {
            background: [0.1, 0.1, 0.9, 1.0],
            accent: [0.4, 0.4, 0.9, 1.0],
        },
    ];
    // Same geometry either way; only how many of the three are in the scene.
    let scene_of = |which: &[usize]| {
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        for index in which {
            let id = *index as u64 + 2;
            children.push(StableNodeId::new(id).unwrap());
            let mut node =
                host_texture_child(id, 1, *index as f32 * 20.0 + 2.0, 2.0, 16.0, 16.0, "0");
            node.custom_render = Some(
                GpuView::new(0)
                    .palette(palettes[*index])
                    .seed(*index as f32 * 0.25)
                    .custom_render(),
            );
            nodes.push(node);
        }
        let mut root = colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 0.0, 1.0]);
        root.children = Arc::new(children);
        nodes.insert(0, root);
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        scene
    };
    let render = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let (texture, target) = test_copy_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        let work = painter.last_gpu_work().expect("encoded gpu-view frame");
        (
            readback_rgba(&device, &queue, encoder, &texture, 64, 64),
            work,
        )
    };

    let (together, batched_work) = render(&scene_of(&[0, 1, 2]));
    let (_, lone_work) = render(&scene_of(&[0]));
    assert_eq!(
        batched_work.draw_calls, lone_work.draw_calls,
        "three adjacent gpu-view nodes must cost the same draws as one"
    );
    for index in 0..3 {
        let (alone, _) = render(&scene_of(&[index]));
        let x = index as u32 * 20 + 10;
        assert_eq!(
            pixel(&together, 64, x, 10),
            pixel(&alone, 64, x, 10),
            "node {index} must paint the same batched as it does alone"
        );
    }
    assert_ne!(
        pixel(&together, 64, 10, 10),
        pixel(&together, 64, 30, 10),
        "each instance must carry its own palette"
    );
}

#[test]
fn ordinary_ui_and_dedicated_passes_split_a_gpu_view_run() {
    use crate::DefaultGpuViewRenderer;
    use nana_ui_runtime::{GpuView, GpuViewMode};

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut registry = SceneGpuRendererRegistry::new();
    registry.insert("gpu-view", Arc::new(DefaultGpuViewRenderer::new()));
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    };
    // `interrupt` names the middle child: a plain gpu-view, one that paints a
    // quad of its own, or one that asked for a dedicated pass.
    let scene_of = |interrupt: &str| {
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        for index in 0..3u64 {
            let id = index + 2;
            children.push(StableNodeId::new(id).unwrap());
            let middle = index == 1;
            let mut node = if middle && interrupt == "quad" {
                colored_quad_child(
                    id,
                    1,
                    index as f32 * 20.0 + 2.0,
                    2.0,
                    16.0,
                    16.0,
                    [1.0, 1.0, 0.0, 1.0],
                )
            } else {
                let mut view = GpuView::new(0);
                if middle && interrupt == "dedicated" {
                    view = view.mode(GpuViewMode::Standalone);
                }
                let mut node =
                    host_texture_child(id, 1, index as f32 * 20.0 + 2.0, 2.0, 16.0, 16.0, "0");
                node.custom_render = Some(view.custom_render());
                node
            };
            node.parent = Some(StableNodeId::new(1).unwrap());
            nodes.push(node);
        }
        let mut root = colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [0.0, 0.0, 0.0, 1.0]);
        root.children = Arc::new(children);
        nodes.insert(0, root);
        let mut scene = UiScene::new();
        scene.apply_delta(nodes, []);
        scene
    };
    let draws = |scene: &UiScene| {
        let mut painter = SceneWgpuPainter::for_test(format);
        let target = test_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&Default::default());
        painter
            .paint_encoder(
                scene,
                &mut encoder,
                &target,
                viewport,
                None,
                Some(&registry),
            )
            .unwrap();
        queue.submit([encoder.finish()]);
        painter.last_gpu_work().expect("encoded frame").draw_calls
    };

    let contiguous = draws(&scene_of("none"));
    let split_by_quad = draws(&scene_of("quad"));
    let split_by_dedicated = draws(&scene_of("dedicated"));
    assert!(
        split_by_quad > contiguous,
        "a quad between two gpu-view nodes must end the run: {split_by_quad} vs {contiguous}"
    );
    assert!(
        split_by_dedicated > contiguous,
        "a dedicated-pass node must end the run: {split_by_dedicated} vs {contiguous}"
    );
}

#[test]
fn resource_encoding_failure_discards_the_whole_unsubmitted_frame() {
    use crate::{SceneResourceEncodeContext, SceneResourceProducer, SceneResourceProducerRegistry};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Producer {
        view: wgpu::TextureView,
        fail: Arc<AtomicBool>,
        submissions: Arc<AtomicUsize>,
    }
    impl SceneResourceProducer for Producer {
        fn encode(
            &self,
            node: &CustomRenderNode,
            mut context: SceneResourceEncodeContext<'_>,
        ) -> Result<(), String> {
            if node.resource.as_ref() == "second" && self.fail.load(Ordering::Relaxed) {
                return Err("production failed".into());
            }
            let _pass = nana_gpu::__framework::encoder(context.frame()).begin_render_pass(
                &wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                },
            );
            Ok(())
        }
        fn submitted(&self, _: &CustomRenderNode, _: &nana_gpu::GpuSubmission) {
            self.submissions.fetch_add(1, Ordering::Relaxed);
        }
    }
    let (device, queue) = test_device();
    let (texture, view) = test_copy_target(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64);
    let fail = Arc::new(AtomicBool::new(true));
    let submissions = Arc::new(AtomicUsize::new(0));
    let producer = Arc::new(Producer {
        view,
        fail: fail.clone(),
        submissions: submissions.clone(),
    });
    let mut registry = SceneResourceProducerRegistry::new();
    registry.insert("first", producer.clone());
    registry.insert("second", producer);
    let mut first = host_texture_child(1, 99, 0.0, 0.0, 64.0, 64.0, "first");
    let mut second = host_texture_child(2, 99, 0.0, 0.0, 64.0, 64.0, "second");
    first.parent = None;
    second.parent = None;
    let mut scene = UiScene::new();
    scene.apply_delta([first, second], []);
    let gpu = crate::test_gpu::context();
    let mut frame = gpu.begin_frame("producer failure");
    assert!(registry.encode_scene(&scene, &mut frame).is_err());
    drop(frame);
    assert_eq!(submissions.load(Ordering::Relaxed), 0);
    let encoder = device.create_command_encoder(&Default::default());
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    assert!(!is_red_slot(pixel(&pixels, 64, 32, 32)));
    fail.store(false, Ordering::Relaxed);
    let mut frame = gpu.begin_frame("producer success");
    let prepared = registry.encode_scene(&scene, &mut frame).unwrap();
    assert_eq!(submissions.load(Ordering::Relaxed), 0);
    prepared.submitted(&frame.submit());
    assert_eq!(submissions.load(Ordering::Relaxed), 2);
    let encoder = device.create_command_encoder(&Default::default());
    let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
    assert!(is_red_slot(pixel(&pixels, 64, 32, 32)));
}

#[test]
#[cfg(feature = "hosted")]
fn native_content_opening_preserves_outside_pixels_and_later_overlays_on_gpu() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let mut native = colored_quad_node(2, 16.0, 16.0, 32.0, 32.0, [0.0; 4]);
    native.custom_render = Some(CustomRenderNode::new(
        nana_ui_runtime::NATIVE_CONTENT_RENDERER,
        "browser",
        1,
    ));
    let mut scene = UiScene::new();
    scene.apply_delta(
        [
            colored_quad_node(1, 0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0]),
            native.clone(),
            colored_quad_node(3, 24.0, 24.0, 8.0, 8.0, [0.0, 1.0, 0.0, 1.0]),
        ],
        [],
    );
    let regions = crate::native_content::native_content_regions(
        &scene,
        SceneRect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        },
    )
    .unwrap();
    assert_eq!(regions.len(), 1);
    let mut renderers = SceneGpuRendererRegistry::new();
    renderers.insert(
        nana_ui_runtime::NATIVE_CONTENT_RENDERER,
        Arc::new(crate::native_content::NativeContentRenderer::default()),
    );
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 64.0],
        physical_size: [64, 64],
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: [0.0; 4],
        clear: true,
    };
    for attached in [false, true, false] {
        native.custom_render.as_mut().unwrap().params =
            Some(Arc::from([if attached { 1.0 } else { 0.0 }]));
        scene.apply_delta([native.clone()], []);
        let (texture, view) = test_copy_target(&device, format, 64, 64);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        painter
            .paint_encoder(
                &scene,
                &mut encoder,
                &view,
                viewport,
                None,
                Some(&renderers),
            )
            .unwrap();
        let pixels = readback_rgba(&device, &queue, encoder, &texture, 64, 64);
        assert_eq!(pixel(&pixels, 64, 4, 4), [255, 0, 0, 255]);
        assert_eq!(
            pixel(&pixels, 64, 20, 20),
            if attached {
                [0, 0, 0, 0]
            } else {
                [255, 0, 0, 255]
            }
        );
        assert_eq!(pixel(&pixels, 64, 28, 28), [0, 255, 0, 255]);
    }
}

#[test]
fn alternating_live_targets_keep_prepared_geometry_text_and_bindings() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let (template, registry, _source, _) =
        runtime_button_over_host_texture_scene(&device, &queue, format);
    let mut painter = SceneWgpuPainter::for_test(format);
    let scenes = (0..16)
        .map(|index| {
            let mut scene = template.clone();
            scene.apply_delta(
                [colored_quad_node(
                    999,
                    0.0,
                    56.0,
                    64.0,
                    8.0,
                    [index as f32 / 15.0, 0.3, 1.0 - index as f32 / 15.0, 1.0],
                )],
                [],
            );
            let mut icon = colored_quad_node(1000, 4.0, 4.0, 16.0, 16.0, [0.0; 4]);
            icon.standard_visual = Some(StandardVisual::Icon {
                icon: if index % 2 == 0 {
                    nana_ui_core::Icon::Search
                } else {
                    nana_ui_core::Icon::Close
                },
                size: 16.0,
                tooltip: None,
            });
            icon.standard_visual_foreground = Some([1.0; 4]);
            let mut label = colored_quad_node(1001, 20.0, 2.0, 40.0, 20.0, [0.0; 4]);
            label.text = Some(TextContent {
                value: format!("T{index}").into(),
            });
            Arc::make_mut(&mut label.source_style.layout).transform = Some(PaintTransform {
                a: 0.94,
                b: 0.34,
                c: -0.34,
                d: 0.94,
                ..PaintTransform::default()
            });
            scene.apply_delta(
                [
                    icon,
                    label,
                    frost_quad_node_with_fill(
                        1002,
                        0.0,
                        40.0,
                        64.0,
                        8.0,
                        [1.0, 1.0, 1.0, 0.2],
                        nana_ui_core::BackdropFilter {
                            blur_radius: 2.0 + index as f32 / 8.0,
                            saturate: 1.0,
                        },
                    ),
                ],
                [],
            );
            #[cfg(feature = "graph-canvas")]
            scene.apply_delta(
                [graph_canvas_stroke_node(
                    1003,
                    vec![(
                        vec![[2.0, 52.0], [10.0 + index as f32, 52.0]],
                        [1.0, 0.0, 0.0, 1.0],
                    )],
                    [0.0; 4],
                )],
                [],
            );
            scene
        })
        .collect::<Vec<_>>();
    let mut reference = Vec::new();
    for round in 0..3 {
        for (index, scene) in scenes.iter().enumerate() {
            let size = 64 + index as u32 * 4;
            let (texture, view) = test_copy_target(&device, format, size, size);
            let viewport = ScenePaintViewport {
                logical_size: [64.0; 2],
                physical_size: [size; 2],
                scale_factor: size as f32 / 64.0,
                scene_origin: [0.0; 2],
                target_origin: [0.0; 2],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            };
            let mut encoder = device.create_command_encoder(&Default::default());
            painter
                .paint_target_encoder(
                    RenderTargetId(index as u64),
                    scene,
                    &mut encoder,
                    &view,
                    viewport,
                    Some(&registry),
                    None,
                )
                .unwrap();
            let pixels = readback_rgba(&device, &queue, encoder, &texture, size, size);
            if round == 0 {
                reference.push(pixels);
            } else {
                assert_eq!(
                    pixels, reference[index],
                    "target {index} reused different target's GPU storage"
                );
                assert!(
                    painter.last_gpu_timings().unwrap().batch.is_zero(),
                    "target {index} rebuilt commands"
                );
                assert_eq!(
                    painter.last_gpu_work().unwrap().gpu_upload_bytes,
                    0,
                    "target {index} uploaded unchanged geometry"
                );
            }
        }
    }
    for index in 0..16 {
        painter.remove_target(RenderTargetId(index));
    }
    assert!(painter.targets.is_empty());
    assert!(
        painter.prepared_batch.is_none(),
        "target commands must not leak into the default painter state"
    );
}

#[test]
fn text_follows_ancestor_compositor_opacity_overlay() {
    use std::time::Duration;

    use nana_ui_core::{
        Easing,
        motion::{AnimatableProperty, MotionTo, MotionValue},
    };
    use nana_ui_runtime::{AnimationId, AnimationSpec, UiWorld};

    let parent = StableNodeId::new(1).unwrap();
    let child = StableNodeId::new(2).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(parent, DocumentId::new(1).unwrap(), NodeKind::Document);
    queue.create(
        child,
        DocumentId::new(1).unwrap(),
        NodeKind::Element { tag: "span".into() },
    );
    queue.insert(parent, child, None);
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            parent,
            Duration::ZERO,
            Duration::from_millis(400),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Opacity)
        .with_range(
            MotionValue::Scalar(0.0),
            MotionTo::Value(MotionValue::Scalar(1.0)),
        ),
    );
    world.commit(queue).unwrap();
    world.advance_animations(Duration::ZERO);
    let mut extracted = world.extract_nodes(&[parent, child]);
    for node in &mut extracted {
        if node.id == child {
            node.text = Some(TextContent { value: "HI".into() });
            node.style = Arc::new(ComputedStyle {
                color: Some([1.0, 1.0, 1.0, 1.0]),
                font_size: 28.0,
                ..ComputedStyle::default()
            });
            node.layout = LayoutBox {
                x: 4.0,
                y: 8.0,
                width: 56.0,
                height: 40.0,
            };
        }
    }
    let mut scene = UiScene::new();
    scene.apply_delta(extracted, []);
    scene.apply_presentation(
        world.presentation_store(),
        Duration::from_millis(200),
        Some(world.motion_descriptors()),
    );
    let layer = scene
        .compositor_layer(parent)
        .expect("parent opacity layer");
    let text = scene
        .primitives()
        .find(|primitive| matches!(primitive.kind, ScenePrimitiveKind::Text { .. }))
        .expect("child text primitive");
    let draw = scene.draw_primitive(text.id).expect("text draw");
    let encode =
        scene.compositor_paint_encode(draw.node, &draw.kind, draw.transform, draw.paint_opacity);
    assert!(
        (encode.opacity - draw.paint_opacity).abs() < 1e-5,
        "Text encode must keep CPU overlay"
    );
    assert!(
        (draw.paint_opacity - layer.opacity).abs() < 0.05,
        "drawn text opacity {} must track presentation {}",
        draw.paint_opacity,
        layer.opacity
    );

    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let pixels = paint_scene_rgba(
        &device,
        &queue,
        &mut painter,
        &scene,
        [64.0, 64.0],
        [64, 64],
        1.0,
    );
    let mut brightest = 0u8;
    for chunk in pixels.as_chunks::<4>().0 {
        brightest = brightest.max(chunk[0]).max(chunk[1]).max(chunk[2]);
    }
    assert!(
        brightest > 20,
        "text must paint some ink under the overlay, got {brightest}"
    );
    assert!(
        brightest < 200,
        "text must follow ancestor presentation opacity {}, not logical 1.0; brightest {brightest}",
        layer.opacity
    );
}

#[cfg(feature = "graph-canvas")]
#[path = "graph_scale_tests.rs"]
mod graph_scale_tests;

#[path = "key_badge_tests.rs"]
mod key_badge_tests;

#[path = "image_sampling_tests.rs"]
mod image_sampling_tests;

/// Issue #217: one `Painter` on one node draws a non-rectangular outline —
/// a raised step on the left, a slope down to the lower edge with its
/// concave foot rounded — a layer *beneath* its own fill that the fill
/// partly covers, and a shadow that follows the outline. Ordinary children
/// lay out and paint above all of it.
mod painted_outline {
    use super::*;
    use nana_ui_core::{CardKind, PositionSpec, RadiusTier};
    use nana_ui_runtime::{BoxPaint, Card, PaintContext, PaintPath, PaintShadow, Painter, Stack};

    pub(super) const STEP: f32 = 36.0;
    pub(super) const UNDER_TOP: f32 = 4.0;
    pub(super) const UNDER_INSET: f32 = 10.0;
    /// `tan` of the slope's lean from vertical.
    const SLOPE_LEAN: f32 = 0.84;
    const FOOT_RADIUS: f32 = 18.0;

    pub(super) struct OutlinePainter;

    impl OutlinePainter {
        pub(super) fn tab_right(width: f32) -> f32 {
            width * 0.5
        }

        pub(super) fn foot(width: f32) -> [f32; 2] {
            [Self::tab_right(width) + STEP * SLOPE_LEAN, STEP]
        }

        pub(super) fn outline(cx: &PaintContext<'_>) -> PaintPath {
            let [w, h] = cx.size();
            let outer = cx.radius(RadiusTier::Lg);
            let shoulder = cx.radius(RadiusTier::Md);
            let tab = Self::tab_right(w);
            let foot = Self::foot(w);
            let mut path = PaintPath::new();
            path.move_to(0.0, outer)
                .arc_to(0.0, 0.0, tab, 0.0, outer)
                .arc_to(tab, 0.0, foot[0], foot[1], shoulder)
                // The concave corner where the slope meets the lower edge.
                .arc_to(foot[0], foot[1], w, STEP, FOOT_RADIUS)
                .arc_to(w, STEP, w, h, outer)
                .arc_to(w, h, 0.0, h, outer)
                .arc_to(0.0, h, 0.0, 0.0, outer)
                .close();
            path
        }
    }

    impl Painter for OutlinePainter {
        fn paint(&self, cx: &mut PaintContext<'_>) {
            let [w, _] = cx.size();
            // The layer beneath first: the outline's fill covers its lower part.
            let sm = cx.radius(RadiusTier::Sm);
            cx.rounded_rect(
                LayoutBox {
                    x: w * 0.4,
                    y: UNDER_TOP,
                    width: w * 0.6 - UNDER_INSET,
                    height: STEP + 16.0,
                },
                [sm, sm, 0.0, 0.0],
                BoxPaint::fill(SemanticColorRole::Accent),
            );
            let outline = Self::outline(cx);
            cx.shadow(
                &outline,
                PaintShadow::Custom {
                    color: [0.0, 0.0, 0.0, 0.45].into(),
                    offset: [0.0, 6.0],
                    blur: 12.0,
                    spread: 0.0,
                    inset: false,
                },
            );
            cx.fill_path(&outline, SemanticColorRole::Surface);
        }

        fn paint_key(&self) -> u64 {
            0
        }
    }

    pub(super) fn scene() -> (UiScene, StableNodeId, Vec<StableNodeId>) {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let root = context
            .create_component(
                document,
                Stack::column(0.0).with_layout(|layout| {
                    layout.padding = Some(LengthSpec::Px(24.0));
                    layout.align_items = nana_ui_core::AlignSpec::Start;
                }),
            )
            .unwrap();
        let mut card = Card::new()
            .kind(CardKind::Flat)
            .padding(12.0)
            .painter(OutlinePainter);
        {
            let layout = Arc::make_mut(&mut card.style.layout);
            layout.width = Some(LengthSpec::Px(360.0));
            layout.height = Some(LengthSpec::Px(260.0));
            layout.position = PositionSpec::Relative;
        }
        let card = context.create_component(document, card).unwrap();
        let actions = context
            .create_component(
                document,
                Stack::row(4.0).with_layout(|layout| {
                    layout.position = PositionSpec::Absolute;
                    layout.offset_top = Some(LengthSpec::Px(UNDER_TOP + 4.0));
                    layout.offset_right = Some(LengthSpec::Px(UNDER_INSET + 4.0));
                }),
            )
            .unwrap();
        let first = context
            .create_component(document, RuntimeButton::new("A").kind(ButtonKind::Selected))
            .unwrap();
        let second = context
            .create_component(document, RuntimeButton::new("B").kind(ButtonKind::Selected))
            .unwrap();
        context.append_child(root, card).unwrap();
        context.append_child(card, actions).unwrap();
        context.append_child(actions, first).unwrap();
        context.append_child(actions, second).unwrap();
        let ids = [
            root.stable_id(),
            card.stable_id(),
            actions.stable_id(),
            first.stable_id(),
            second.stable_id(),
        ];
        context.resolve_styles(&ids).unwrap();
        context
            .shape_text(&ids, &mut crate::NanaTextShaper::default())
            .unwrap();
        context
            .layout_document(document, nana_ui_runtime::LayoutViewport::new(420.0, 330.0))
            .unwrap();
        let mut scene = UiScene::new();
        scene.apply_delta(context.world().extract_document(document), []);
        (
            scene,
            card.stable_id(),
            vec![first.stable_id(), second.stable_id()],
        )
    }
}

fn distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    (0..3).map(|i| a[i].abs_diff(b[i]) as u32).max().unwrap()
}

#[test]
fn one_painter_draws_an_outline_a_layer_beneath_it_and_its_shadow() {
    use painted_outline::*;
    let (scene, card, buttons) = scene();
    let card_bounds = scene.node_bounds(card).unwrap();
    assert_eq!((card_bounds.x, card_bounds.y), (24.0, 24.0));
    // One node paints the whole silhouette; the actions are its children and
    // paint above everything the painter put behind them.
    let order: Vec<_> = scene
        .primitives()
        .map(|p| (p.node, p.kind.clone()))
        .collect();
    let last_card = order.iter().rposition(|(node, _)| *node == card).unwrap();
    let first_button = order
        .iter()
        .position(|(node, _)| buttons.contains(node))
        .unwrap();
    assert!(
        last_card < first_button,
        "the node's own paint precedes its children"
    );
    assert_eq!(
        order
            .iter()
            .filter(|(node, kind)| *node == card && matches!(kind, ScenePrimitiveKind::Path { .. }))
            .count(),
        2,
        "shadow and fill are path meshes; the layer beneath is a quad"
    );

    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut painter = SceneWgpuPainter::for_test(format);
    let (width, height) = (420u32, 330u32);
    let viewport = ScenePaintViewport {
        logical_size: [width as f32, height as f32],
        physical_size: [width, height],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.9, 0.9, 0.9, 1.0],
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, width, height);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui painted outline"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    let pixels = readback_rgba(&device, &queue, encoder, &texture, width, height);
    if let Ok(path) = std::env::var("NANA_UI_PAINTED_OUTLINE_RGBA") {
        std::fs::write(path, &pixels).unwrap();
    }
    let at = |x: f32, y: f32| pixel(&pixels, width, (24.0 + x) as u32, (24.0 + y) as u32);
    let background = pixel(&pixels, width, width - 2, 2);
    let surface = at(180.0, 160.0);
    // Above the action buttons, right of the slope.
    let accent = at(200.0, 10.0);
    assert!(
        distance(surface, background) > 30,
        "{surface:?} vs {background:?}"
    );
    assert!(distance(accent, surface) > 30, "{accent:?} vs {surface:?}");

    assert!(
        distance(at(30.0, 12.0), surface) <= 2,
        "the raised step is filled"
    );
    assert!(
        distance(at(340.0, STEP + 8.0), surface) <= 2,
        "the layer beneath runs on under the fill and the fill covers it"
    );
    let cut_out = at(360.0 - UNDER_INSET / 2.0, 20.0);
    assert!(
        distance(cut_out, surface) > 30 && distance(cut_out, accent) > 30,
        "right of the layer the cut-out is empty, got {cut_out:?}"
    );
    let above_layer = at(260.0, 1.0);
    assert!(
        distance(above_layer, surface) > 30 && distance(above_layer, accent) > 30,
        "above the layer the cut-out is empty, got {above_layer:?}"
    );
    let below = at(180.0, 260.0 + 7.0);
    assert!(
        below[0] + 20 < background[0],
        "the shadow falls under the bottom edge: {below:?} vs {background:?}"
    );
    assert!(
        distance(at(180.0, 260.0 + 40.0), background) <= 2,
        "and fades out"
    );
}

/// Paint one node carrying `painter` at the origin of a `w × h` viewport and
/// read the frame back.
fn paint_one_painter(painter: impl nana_ui_runtime::Painter, w: u32, h: u32) -> Vec<u8> {
    paint_one_painter_with(painter, w, h, |_| {})
}

fn paint_one_painter_with(
    painter: impl nana_ui_runtime::Painter,
    w: u32,
    h: u32,
    configure: impl FnOnce(&mut SceneWgpuPainter),
) -> Vec<u8> {
    paint_one_painter_onto(
        painter,
        w,
        h,
        wgpu::TextureFormat::Rgba8Unorm,
        [0.0, 0.0, 0.0, 1.0],
        configure,
    )
}

fn paint_one_painter_onto(
    painter: impl nana_ui_runtime::Painter,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    clear_color: [f32; 4],
    configure: impl FnOnce(&mut SceneWgpuPainter),
) -> Vec<u8> {
    use nana_ui_runtime::Stack;
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let node = context
        .create_component(
            document,
            Stack::column(0.0)
                .with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(w as f32));
                    layout.height = Some(LengthSpec::Px(h as f32));
                })
                .painter(painter),
        )
        .unwrap();
    let ids = [node.stable_id()];
    context.resolve_styles(&ids).unwrap();
    context
        .layout_document(
            document,
            nana_ui_runtime::LayoutViewport::new(w as f32, h as f32),
        )
        .unwrap();
    let mut scene = UiScene::new();
    scene.apply_delta(context.world().extract_document(document), []);
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(format);
    configure(&mut painter);
    let viewport = ScenePaintViewport {
        logical_size: [w as f32, h as f32],
        physical_size: [w, h],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color,
        clear: true,
    };
    let (texture, view) = test_copy_target(&device, format, w, h);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nana-ui one painter"),
    });
    painter
        .paint_encoder(&scene, &mut encoder, &view, viewport, None, None)
        .unwrap();
    readback_rgba(&device, &queue, encoder, &texture, w, h)
}

/// A painter defined by a closure, for tests.
struct PaintFn<F>(F);

impl<F: Fn(&mut nana_ui_runtime::PaintContext<'_>) + Send + Sync + 'static> nana_ui_runtime::Painter
    for PaintFn<F>
{
    fn paint(&self, cx: &mut nana_ui_runtime::PaintContext<'_>) {
        (self.0)(cx);
    }

    fn paint_key(&self) -> u64 {
        0
    }
}

fn full_rect(w: f32, h: f32) -> nana_ui_runtime::PaintPath {
    let mut path = nana_ui_runtime::PaintPath::new();
    path.rect(LayoutBox {
        x: 0.0,
        y: 0.0,
        width: w,
        height: h,
    });
    path
}

#[test]
fn a_linear_gradient_runs_between_its_stops_on_the_gpu() {
    use nana_ui_runtime::Gradient;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(
                &full_rect(100.0, 20.0),
                Gradient::linear([10.0, 0.0], [90.0, 0.0])
                    .stop(0.0, [1.0, 0.0, 0.0, 1.0])
                    .stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            );
        }),
        100,
        20,
    );
    let at = |x| pixel(&pixels, 100, x, 10);
    assert!(is_red_slot(at(3)), "padded before the start: {:?}", at(3));
    assert!(is_blue_slot(at(96)), "padded past the end: {:?}", at(96));
    let (a, b, c) = (at(30), at(50), at(70));
    assert!(a[0] > b[0] && b[0] > c[0], "red falls: {a:?} {b:?} {c:?}");
    assert!(a[2] < b[2] && b[2] < c[2], "blue rises: {a:?} {b:?} {c:?}");
}

#[test]
fn a_radial_gradient_is_evaluated_per_pixel_under_a_rotation() {
    use nana_ui_runtime::Gradient;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.translate(40.0, 40.0);
            cx.rotate(0.7);
            let mut square = nana_ui_runtime::PaintPath::new();
            square.rect(LayoutBox {
                x: -30.0,
                y: -30.0,
                width: 60.0,
                height: 60.0,
            });
            cx.fill_path(
                &square,
                Gradient::radial([0.0, 0.0], 30.0)
                    .stop(0.0, [1.0, 0.0, 0.0, 1.0])
                    .stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            );
        }),
        80,
        80,
    );
    let at = |x, y| pixel(&pixels, 80, x, y);
    assert!(is_red_slot(at(40, 40)), "centre: {:?}", at(40, 40));
    // The rotated square's corners reach past the radius.
    let far = at(40, 40 + 38);
    assert!(is_blue_slot(far), "past the radius: {far:?}");
    // Equal distances from the centre in different directions agree: the
    // gradient is circular, not interpolated across triangles.
    // Pixel centres (55.5, 40.5) and (40.5, 55.5) are both 15.51 from (40, 40).
    let (east, south) = (at(40 + 15, 40), at(40, 40 + 15));
    assert!(distance(east, south) <= 2, "{east:?} vs {south:?}");
}

#[test]
fn a_dashed_stroke_paints_its_dashes_and_skips_its_gaps() {
    use nana_ui_runtime::StrokeStyle;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            let mut line = nana_ui_runtime::PaintPath::new();
            line.move_to(0.0, 10.0).line_to(100.0, 10.0);
            cx.stroke_path(
                &line,
                StrokeStyle::new(6.0).dash(vec![10.0, 10.0], 0.0),
                [0.0, 1.0, 0.0, 1.0],
            );
        }),
        100,
        20,
    );
    let at = |x| pixel(&pixels, 100, x, 10);
    for x in [5, 25, 45, 65, 85] {
        assert!(is_green_slot(at(x)), "dash at {x}: {:?}", at(x));
    }
    for x in [15, 35, 55, 75, 95] {
        assert_eq!(at(x)[1], 0, "gap at {x}: {:?}", at(x));
    }
}

#[test]
fn a_rotated_path_and_an_image_land_where_the_local_transform_puts_them() {
    use base64::Engine as _;
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><path d=\"M0 0 H8 V8 H0 Z\" fill=\"#00ff00\"/></svg>";
    let url = format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    );
    let pixels = paint_one_painter(
        PaintFn(move |cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.save();
            cx.translate(30.0, 30.0);
            cx.rotate(std::f32::consts::FRAC_PI_4);
            let mut square = nana_ui_runtime::PaintPath::new();
            square.rect(LayoutBox {
                x: -10.0,
                y: -10.0,
                width: 20.0,
                height: 20.0,
            });
            cx.fill_path(&square, [1.0, 0.0, 0.0, 1.0]);
            cx.restore();
            cx.translate(60.0, 20.0);
            cx.image(
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                url.clone(),
                nana_ui_runtime::ImageFit::Fill,
                0.0,
            );
        }),
        100,
        60,
    );
    let at = |x, y| pixel(&pixels, 100, x, y);
    // A square turned 45° is a diamond: its tip is past the unturned edge,
    // its bounding box's corner is empty.
    assert!(
        is_red_slot(at(30, 30 + 12)),
        "diamond tip: {:?}",
        at(30, 42)
    );
    assert_eq!(at(30 + 9, 30 + 9)[0], 0, "diamond corner: {:?}", at(39, 39));
    assert!(is_green_slot(at(70, 30)), "image moved: {:?}", at(70, 30));
    assert_eq!(at(10, 30)[1], 0, "nothing at the untranslated spot");
}

#[test]
fn a_conic_gradient_sweeps_clockwise_from_its_start_angle() {
    use nana_ui_runtime::Gradient;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(
                &full_rect(60.0, 60.0),
                Gradient::conic([30.0, 30.0], 0.0)
                    .stop(0.0, [1.0, 0.0, 0.0, 1.0])
                    .stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            );
        }),
        60,
        60,
    );
    let at = |x, y| pixel(&pixels, 60, x, y);
    // Just clockwise of +x (y down) the sweep has barely started; just
    // counter-clockwise of it, it has nearly finished.
    assert!(is_red_slot(at(55, 32)), "{:?}", at(55, 32));
    assert!(is_blue_slot(at(55, 27)), "{:?}", at(55, 27));
}

fn green_svg_url() -> String {
    use base64::Engine as _;
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><path d=\"M0 0 H8 V8 H0 Z\" fill=\"#00ff00\"/></svg>";
    format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    )
}

fn rect_path(x: f32, y: f32, width: f32, height: f32) -> nana_ui_runtime::PaintPath {
    let mut path = nana_ui_runtime::PaintPath::new();
    path.rect(LayoutBox {
        x,
        y,
        width,
        height,
    });
    path
}

#[test]
fn a_layer_composites_its_overlapping_contents_once() {
    use nana_ui_runtime::BlendMode;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.push_layer(0.5, BlendMode::Normal);
            cx.fill_path(&rect_path(0.0, 0.0, 60.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
            cx.fill_path(&rect_path(30.0, 0.0, 60.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
            cx.pop_layer();
            // Per-op opacity, by contrast, stacks where ops overlap.
            cx.set_opacity(0.5);
            cx.fill_path(&rect_path(0.0, 20.0, 60.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
            cx.fill_path(&rect_path(30.0, 20.0, 60.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
        }),
        90,
        40,
    );
    let at = |x, y| pixel(&pixels, 90, x, y);
    assert!(
        distance(at(10, 10), at(45, 10)) <= 2,
        "{:?} {:?}",
        at(10, 10),
        at(45, 10)
    );
    assert!(
        at(10, 10)[0] > 60 && at(10, 10)[0] < 200,
        "half red: {:?}",
        at(10, 10)
    );
    assert!(
        at(45, 30)[0] > at(10, 30)[0] + 20,
        "{:?} {:?}",
        at(45, 30),
        at(10, 30)
    );
}

#[test]
fn a_multiply_blend_darkens_what_is_under_it() {
    use nana_ui_runtime::BlendMode;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(&rect_path(0.0, 0.0, 60.0, 20.0), [1.0, 1.0, 0.0, 1.0]);
            cx.set_blend(BlendMode::Multiply);
            cx.fill_path(&rect_path(30.0, 0.0, 30.0, 20.0), [1.0, 0.0, 1.0, 1.0]);
        }),
        60,
        20,
    );
    let yellow = pixel(&pixels, 60, 10, 10);
    let multiplied = pixel(&pixels, 60, 45, 10);
    assert!(yellow[0] > 200 && yellow[1] > 200, "{yellow:?}");
    // Yellow × magenta = red.
    assert!(is_red_slot(multiplied), "{multiplied:?}");
}

/// The first pixel of a 2×1 transparent sRGB target holding `color`,
/// stored with `encoding`.
fn stored_pixel(color: [f32; 4], encoding: super::AlphaEncoding) -> [u8; 4] {
    let pixels = paint_one_painter_onto(
        PaintFn(move |cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(&full_rect(2.0, 1.0), color);
        }),
        2,
        1,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        [0.0; 4],
        |painter| painter.set_alpha_encoding(encoding),
    );
    [pixels[0], pixels[1], pixels[2], pixels[3]]
}

#[test]
fn gamma_encoding_stores_what_a_window_compositor_blends() {
    use super::AlphaEncoding::{Gamma, Linear};
    let quarter_white = [1.0, 1.0, 1.0, 0.25];
    // enc(0.25) = 137: what a compositor would show as a far brighter veil.
    assert_eq!(stored_pixel(quarter_white, Linear), [137, 137, 137, 64]);
    assert_eq!(stored_pixel(quarter_white, Gamma), [64, 64, 64, 64]);
    let gray = [0.5, 0.5, 0.5, 1.0];
    assert_eq!(stored_pixel(gray, Linear), stored_pixel(gray, Gamma));
}

/// Black text on a white ground, optionally inside a painter layer, painted
/// with subpixel text set to `order`.
fn paint_subpixel_text(order: Option<super::SubpixelOrder>, layered: bool) -> Vec<u8> {
    use nana_ui_runtime::PaintText;
    paint_one_painter_with(
        PaintFn(move |cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(&full_rect(160.0, 40.0), [1.0, 1.0, 1.0, 1.0]);
            if layered {
                cx.push_layer(1.0, nana_ui_runtime::BlendMode::Normal);
            }
            cx.text(
                LayoutBox {
                    x: 4.0,
                    y: 4.0,
                    width: 152.0,
                    height: 32.0,
                },
                PaintText::new("Minimum wHo")
                    .size(15.0)
                    .paint([0.0, 0.0, 0.0, 1.0]),
            );
            if layered {
                cx.pop_layer();
            }
        }),
        160,
        40,
        |painter| painter.set_subpixel_text(order),
    )
}

/// The widest spread between one pixel's color channels.
fn channel_spread(pixels: &[u8]) -> u8 {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]))
        .max()
        .unwrap_or(0)
}

fn device_blends_two_sources() -> bool {
    test_device()
        .0
        .features()
        .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
}

#[test]
fn grayscale_text_paints_equal_channels() {
    let pixels = paint_subpixel_text(None, false);
    assert!(
        pixels.as_chunks::<4>().0.iter().any(|p| p[0] < 128),
        "text was drawn"
    );
    assert!(channel_spread(&pixels) <= 1, "{}", channel_spread(&pixels));
}

#[test]
fn subpixel_text_gives_each_channel_its_own_coverage() {
    if !device_blends_two_sources() {
        return;
    }
    let pixels = paint_subpixel_text(Some(super::SubpixelOrder::Rgb), false);
    assert!(
        pixels.as_chunks::<4>().0.iter().any(|p| p[0] < 128),
        "text was drawn"
    );
    assert!(
        channel_spread(&pixels) > 32,
        "stem edges fringe per subpixel: {}",
        channel_spread(&pixels)
    );
}

#[test]
fn a_bgr_panel_gets_the_rgb_coverage_mirrored() {
    if !device_blends_two_sources() {
        return;
    }
    let rgb = paint_subpixel_text(Some(super::SubpixelOrder::Rgb), false);
    let bgr = paint_subpixel_text(Some(super::SubpixelOrder::Bgr), false);
    for (a, b) in rgb
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bgr.as_chunks::<4>().0.iter())
    {
        assert!(
            a[0].abs_diff(b[2]) <= 1 && a[2].abs_diff(b[0]) <= 1 && a[1].abs_diff(b[1]) <= 1,
            "{a:?} vs {b:?}"
        );
    }
}

#[test]
fn subpixel_text_inside_an_offscreen_group_stays_grayscale() {
    if !device_blends_two_sources() {
        return;
    }
    let pixels = paint_subpixel_text(Some(super::SubpixelOrder::Rgb), true);
    assert!(
        pixels.as_chunks::<4>().0.iter().any(|p| p[0] < 128),
        "text was drawn"
    );
    assert!(channel_spread(&pixels) <= 1, "{}", channel_spread(&pixels));
}

#[test]
fn a_gradient_colours_text_across_its_glyphs() {
    use nana_ui_runtime::{Gradient, PaintText};
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.text(
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 60.0,
                },
                PaintText::new("MMMMMM").size(40.0).paint(
                    Gradient::linear([0.0, 0.0], [200.0, 0.0])
                        .stop(0.0, [1.0, 0.0, 0.0, 1.0])
                        .stop(1.0, [0.0, 0.0, 1.0, 1.0]),
                ),
            );
        }),
        200,
        60,
    );
    // The brightest ink in the left and right thirds.
    let ink = |range: std::ops::Range<u32>| {
        let mut best = [0u8; 4];
        for x in range {
            for y in 0..60 {
                let p = pixel(&pixels, 200, x, y);
                if u32::from(p[0]) + u32::from(p[2]) > u32::from(best[0]) + u32::from(best[2]) {
                    best = p;
                }
            }
        }
        best
    };
    let (left, right) = (ink(0..60), ink(140..200));
    assert!(left[0] > left[2] + 40, "red on the left: {left:?}");
    assert!(right[2] > right[0] + 40, "blue on the right: {right:?}");
}

#[test]
fn a_curved_clip_cuts_an_image_along_its_curve_and_an_image_can_fill_a_path() {
    let url = green_svg_url();
    let pixels = paint_one_painter(
        PaintFn(move |cx: &mut nana_ui_runtime::PaintContext<'_>| {
            let mut disc = nana_ui_runtime::PaintPath::new();
            disc.ellipse(LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 60.0,
            });
            cx.push_clip(&disc);
            cx.image(
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 60.0,
                    height: 60.0,
                },
                url.clone(),
                nana_ui_runtime::ImageFit::Fill,
                0.0,
            );
            cx.pop_clip();
            let mut triangle = nana_ui_runtime::PaintPath::new();
            triangle
                .move_to(70.0, 60.0)
                .line_to(100.0, 0.0)
                .line_to(130.0, 60.0)
                .close();
            cx.fill_path_with_image(
                &triangle,
                LayoutBox {
                    x: 70.0,
                    y: 0.0,
                    width: 60.0,
                    height: 60.0,
                },
                url.clone(),
                nana_ui_runtime::ImageFit::Fill,
            );
        }),
        130,
        60,
    );
    let at = |x, y| pixel(&pixels, 130, x, y);
    assert!(is_green_slot(at(30, 30)), "disc centre: {:?}", at(30, 30));
    assert_eq!(at(3, 3)[1], 0, "outside the disc: {:?}", at(3, 3));
    // Inside the bounding box, 0.4px outside the circle: at most its edge's
    // ramp, where the bounding-box approximation would have painted it.
    assert!(at(8, 8)[1] < 40, "{:?}", at(8, 8));
    assert!(is_green_slot(at(100, 45)), "triangle: {:?}", at(100, 45));
    assert_eq!(at(75, 10)[1], 0, "beside the triangle: {:?}", at(75, 10));
}

#[test]
fn a_clipped_layer_off_the_target_is_skipped_whole_and_one_half_on_is_not() {
    let url = green_svg_url();
    let pixels = paint_one_painter(
        PaintFn(move |cx: &mut nana_ui_runtime::PaintContext<'_>| {
            let disc = |x: f32| {
                let mut disc = nana_ui_runtime::PaintPath::new();
                disc.ellipse(LayoutBox {
                    x,
                    y: 0.0,
                    width: 60.0,
                    height: 60.0,
                });
                disc
            };
            // Wholly left of the target, then straddling its left edge.
            for x in [-200.0, -30.0] {
                cx.push_clip(&disc(x));
                cx.image(
                    LayoutBox {
                        x,
                        y: 0.0,
                        width: 60.0,
                        height: 60.0,
                    },
                    url.clone(),
                    nana_ui_runtime::ImageFit::Fill,
                    0.0,
                );
                cx.pop_clip();
            }
            // What follows draws outside any layer, at full strength.
            cx.fill_path(&rect_path(80.0, 20.0, 20.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
        }),
        130,
        60,
    );
    let at = |x, y| pixel(&pixels, 130, x, y);
    assert!(
        is_green_slot(at(10, 30)),
        "the visible half: {:?}",
        at(10, 30)
    );
    assert_eq!(at(28, 5)[1], 0, "outside the disc: {:?}", at(28, 5));
    assert_eq!(at(90, 30), [255, 0, 0, 255], "after the layers");
}

#[test]
fn an_inset_shadow_darkens_inside_the_edge() {
    use nana_ui_runtime::PaintShadow;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            let card = rect_path(10.0, 10.0, 80.0, 40.0);
            cx.fill_path(&card, [1.0, 1.0, 1.0, 1.0]);
            cx.shadow(
                &card,
                PaintShadow::Custom {
                    color: [0.0, 0.0, 0.0, 0.8].into(),
                    offset: [0.0, 0.0],
                    blur: 6.0,
                    spread: 0.0,
                    inset: true,
                },
            );
        }),
        100,
        60,
    );
    let at = |x, y| pixel(&pixels, 100, x, y);
    // Half the shadow on the edge itself, fading inwards over the blur.
    let edge = u32::from(at(10, 30)[0]);
    assert!(edge + 60 < u32::from(at(50, 30)[0]), "edge {edge}");
    assert!(at(10, 30)[0] < at(13, 30)[0] && at(13, 30)[0] < at(16, 30)[0]);
    assert!(
        at(50, 30)[0] > 240,
        "the middle stays white: {:?}",
        at(50, 30)
    );
    assert_eq!(at(5, 30)[0], 0, "nothing outside: {:?}", at(5, 30));
}

#[test]
fn path_fills_and_strokes_ramp_over_one_device_pixel_centred_on_their_edge() {
    // A fringe laid one pixel outside the outline, as it once was, puts the
    // ramp's midpoint half a pixel out and makes every shape half a pixel
    // fatter a side: a 1px stroke reads nearly 2px wide. Centred, each pixel
    // near a filled ellipse's, a filled triangle's or a 1px stroke's edge is
    // the one device pixel linear ramp over its distance to it, taken at
    // each of the frame's four MSAA samples (WebGPU's standard positions).
    // (A circle or rectangle would be drawn as a quad.)
    let paint =
        |draw: fn(&mut nana_ui_runtime::PaintContext<'_>)| paint_one_painter(PaintFn(draw), 64, 64);
    let ellipse = paint(|cx| {
        let mut path = nana_ui_runtime::PaintPath::new();
        path.ellipse(LayoutBox {
            x: 12.3,
            y: 20.6,
            width: 40.0,
            height: 24.0,
        });
        cx.fill_path(&path, [1.0, 1.0, 1.0, 1.0]);
    });
    let triangle = paint(|cx| {
        let mut path = nana_ui_runtime::PaintPath::new();
        path.move_to(8.3, 56.6)
            .line_to(30.1, 6.2)
            .line_to(57.7, 50.4)
            .close();
        cx.fill_path(&path, [1.0, 1.0, 1.0, 1.0]);
    });
    let stroke = paint(|cx| {
        let mut path = nana_ui_runtime::PaintPath::new();
        path.move_to(8.0, 20.3).line_to(56.0, 20.3);
        cx.stroke_path(
            &path,
            nana_ui_runtime::StrokeStyle::new(1.0),
            [1.0, 1.0, 1.0, 1.0],
        );
    });
    // First order to the ellipse: its implicit function over its gradient.
    let ellipse_edge = |x: f32, y: f32| {
        let n = [(x - 32.3) / 20.0, (y - 32.6) / 12.0];
        let length = n[0].hypot(n[1]);
        (length - 1.0) * length / (n[0] / 20.0).hypot(n[1] / 12.0)
    };
    let corners = [[8.3, 56.6], [30.1, 6.2], [57.7, 50.4]];
    let triangle_edge = |x: f32, y: f32| convex_polygon_distance([x, y], &corners);
    let stroke_edge = |_: f32, y: f32| (y - 20.3).abs() - 0.5;
    // Near a corner the fringe's miter join is its own approximation, and
    // the stroke ends in caps.
    let everywhere = |_: f32, _: f32| true;
    let off_corners = |x: f32, y: f32| corners.iter().all(|c| (x - c[0]).hypot(y - c[1]) >= 10.0);
    let off_ends = |x: f32, _: f32| (12.0..52.0).contains(&x);
    type Edge<'a> = &'a dyn Fn(f32, f32) -> f32;
    type Keep<'a> = &'a dyn Fn(f32, f32) -> bool;
    let cases: [(&str, &[u8], Edge, Keep); 3] = [
        ("filled ellipse", &ellipse, &ellipse_edge, &everywhere),
        ("filled triangle", &triangle, &triangle_edge, &off_corners),
        ("1px stroke", &stroke, &stroke_edge, &off_ends),
    ];
    const SAMPLES: [[f32; 2]; 4] = [
        [0.375, 0.125],
        [0.875, 0.375],
        [0.125, 0.625],
        [0.625, 0.875],
    ];
    for (label, pixels, edge, keep) in cases {
        for py in 0..64 {
            for px in 0..64 {
                let [x, y] = [px as f32 + 0.5, py as f32 + 0.5];
                let outside = edge(x, y);
                if outside.abs() > 1.2 || !keep(x, y) {
                    continue;
                }
                let expected = SAMPLES
                    .iter()
                    .map(|[sx, sy]| (0.5 - edge(px as f32 + sx, py as f32 + sy)).clamp(0.0, 1.0))
                    .sum::<f32>()
                    * 255.0
                    / 4.0;
                let green = f32::from(pixel(pixels, 64, px, py)[1]);
                // A curve is flattened to within 0.05px of itself: 13 of 255.
                assert!(
                    (green - expected).abs() <= 14.0,
                    "{label}: ({px},{py}) is {outside:+.3} px from the edge, \
                     expected green {expected:.0}, got {green}"
                );
            }
        }
    }
}

#[test]
fn a_reading_blend_copies_and_composites_only_where_its_group_drew() {
    use nana_ui_runtime::BlendMode;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(&rect_path(0.0, 0.0, 120.0, 40.0), [1.0, 0.5, 0.0, 1.0]);
            // A small circle off the pixel grid: its anti-aliased rim has to
            // land inside the copied area.
            let mut disc = nana_ui_runtime::PaintPath::new();
            disc.ellipse(LayoutBox {
                x: 10.3,
                y: 10.6,
                width: 19.0,
                height: 19.0,
            });
            cx.set_blend(BlendMode::Difference);
            cx.fill_path(&disc, [1.0, 1.0, 1.0, 1.0]);
            cx.set_blend(BlendMode::Normal);
            // A reading blend inside another: the inner one's area is part of
            // the outer one's.
            cx.push_layer(1.0, BlendMode::Exclusion);
            cx.fill_path(&rect_path(60.0, 10.0, 20.0, 20.0), [0.0, 0.0, 1.0, 1.0]);
            cx.set_blend(BlendMode::Difference);
            cx.fill_path(&rect_path(90.0, 10.0, 20.0, 20.0), [1.0, 1.0, 1.0, 1.0]);
            cx.pop_layer();
        }),
        120,
        40,
    );
    let at = |x, y| pixel(&pixels, 120, x, y);
    let orange = at(2, 2);
    assert!(orange[0] > 250 && orange[2] < 5, "{orange:?}");
    // Difference with white inverts the orange: (0, 0.5, 1).
    let inverted = at(20, 20);
    assert!(inverted[0] < 5 && inverted[2] > 250, "{inverted:?}");
    // The rim (the fringe is centred on the outline, at x = 10.3 here)
    // blends by coverage, and just past it nothing moved.
    let rim = at(10, 20);
    assert!(rim[0] < orange[0] && rim[0] > inverted[0], "{rim:?}");
    for p in [(9, 20), (20, 8), (31, 20), (20, 31), (50, 20)] {
        assert_eq!(at(p.0, p.1), orange, "{p:?}");
    }
    // exclusion(orange, blue) = (1, 0.5, 1); the layer's difference with
    // white over a transparent layer is white, then excluded: (0, 0.5, 1).
    let blue = at(70, 20);
    assert!(blue[0] > 250 && blue[2] > 250, "{blue:?}");
    let white = at(100, 20);
    assert!(white[0] < 5 && white[2] > 250, "{white:?}");
    assert_eq!(at(85, 20), orange, "between the two");
}

#[test]
fn blend_modes_that_read_the_backdrop_blend_with_what_is_under_them() {
    use nana_ui_runtime::BlendMode;
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.fill_path(&rect_path(0.0, 0.0, 90.0, 20.0), [1.0, 0.5, 0.0, 1.0]);
            cx.set_blend(BlendMode::Difference);
            cx.fill_path(&rect_path(0.0, 0.0, 30.0, 20.0), [1.0, 1.0, 1.0, 1.0]);
            cx.set_blend(BlendMode::Darken);
            cx.fill_path(&rect_path(30.0, 0.0, 30.0, 20.0), [0.2, 0.9, 0.9, 1.0]);
            // Nested in a layer, the backdrop is that layer.
            cx.push_layer(1.0, BlendMode::Normal);
            cx.fill_path(&rect_path(60.0, 0.0, 30.0, 20.0), [0.0, 0.0, 1.0, 1.0]);
            cx.set_blend(BlendMode::Lighten);
            cx.fill_path(&rect_path(60.0, 0.0, 30.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
            cx.pop_layer();
        }),
        90,
        20,
    );
    let at = |x| pixel(&pixels, 90, x, 10);
    // |orange - white| = (0, 0.5, 1) in the blend space.
    let difference = at(15);
    assert!(difference[0] < 20 && difference[2] > 230, "{difference:?}");
    // min(orange, (0.2, 0.9, 0.9)) keeps the lower channel of each.
    let darken = at(45);
    assert!(darken[0] < 120 && darken[2] < 20, "{darken:?}");
    // max(blue, red) inside the layer = magenta.
    let lighten = at(75);
    assert!(
        lighten[0] > 230 && lighten[2] > 230 && lighten[1] < 20,
        "{lighten:?}"
    );
}

#[test]
fn group_extents_union_what_each_group_draws_and_widen_for_filters() {
    let rect = |x, y, width, height| PhysicalRect {
        x,
        y,
        width,
        height,
    };
    let path = |scissor| DrawCommand::Path {
        range: PathRange {
            first_index: 0,
            index_count: 3,
        },
        scissor,
    };
    let whole = rect(0, 0, 200, 100);
    let blend = |mix_blend| GroupSlot::dest(1.0, [1.0; 3], 0.0, 0.0, mix_blend, FragmentClip::PASS);
    let blurred = GroupSlot::dest(1.0, [1.0; 3], 0.0, 4.0, 3, FragmentClip::PASS);
    let commands = [
        // Slot 0 holds slot 1 and draws beside it.
        DrawCommand::PushGroup { layer: 0, slot: 0 },
        path(whole),
        DrawCommand::PushGroup { layer: 1, slot: 1 },
        path(whole),
        DrawCommand::PopGroup,
        DrawCommand::PopGroup,
        // Slot 2 draws nothing; slot 3 blurs.
        DrawCommand::PushGroup { layer: 0, slot: 2 },
        DrawCommand::PopGroup,
        DrawCommand::PushGroup { layer: 0, slot: 3 },
        path(whole),
        DrawCommand::PopGroup,
    ];
    let painted = [rect(10, 10, 5, 5), rect(40, 20, 10, 10), rect(0, 0, 1, 1)];
    let extents = group_extents(
        &commands,
        &painted,
        &[blend(3), blend(9), blend(3), blurred],
    );
    assert_eq!(
        extents,
        [
            GroupExtent::Rect(rect(10, 10, 40, 20)),
            GroupExtent::Rect(rect(40, 20, 10, 10)),
            GroupExtent::Empty,
            GroupExtent::All,
        ]
    );
}

#[test]
fn an_empty_layer_composites_nothing_instead_of_what_last_used_its_depth() {
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            cx.push_layer(1.0, nana_ui_runtime::BlendMode::Normal);
            cx.fill_path(&rect_path(0.0, 0.0, 20.0, 20.0), [1.0, 0.0, 0.0, 1.0]);
            cx.pop_layer();
            cx.fill_path(&rect_path(0.0, 0.0, 20.0, 20.0), [1.0, 1.0, 1.0, 1.0]);
            // Same depth, nothing drawn into it.
            cx.push_layer(1.0, nana_ui_runtime::BlendMode::Normal);
            cx.set_opacity(0.0);
            cx.fill_path(&rect_path(40.0, 0.0, 20.0, 20.0), [0.0, 0.0, 1.0, 1.0]);
            cx.set_opacity(1.0);
            cx.pop_layer();
        }),
        60,
        20,
    );
    assert_eq!(pixel(&pixels, 60, 10, 10), [255, 255, 255, 255]);
    assert_eq!(pixel(&pixels, 60, 50, 10), [0, 0, 0, 255]);
}

#[test]
fn a_stroked_rectangle_drawn_as_a_quad_straddles_its_outline() {
    let pixels = paint_one_painter(
        PaintFn(|cx: &mut nana_ui_runtime::PaintContext<'_>| {
            let mut square = nana_ui_runtime::PaintPath::new();
            square.rect(LayoutBox {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 40.0,
            });
            cx.fill_path(&square, [0.0, 0.0, 1.0, 1.0]);
            cx.stroke_path(
                &square,
                nana_ui_runtime::StrokeStyle::new(4.0),
                [1.0, 0.0, 0.0, 1.0],
            );
        }),
        60,
        60,
    );
    let at = |x, y| pixel(&pixels, 60, x, y);
    // The 4px stroke covers 8..12 across the left edge, over the fill.
    for x in [8, 9, 10, 11] {
        assert_eq!(at(x, 30), [255, 0, 0, 255], "stroke at x={x}");
    }
    assert_eq!(at(12, 30), [0, 0, 255, 255], "fill inside the stroke");
    assert_eq!(at(7, 30), [0, 0, 0, 255], "nothing outside the stroke");
    // A miter join keeps the corner square.
    assert_eq!(at(8, 8), [255, 0, 0, 255], "square outer corner");
}

/// A 16 px label under `parent`, for the scroll tests below.
fn scrolled_label(value: u64, parent: u64, x: f32, y: f32, content: &str) -> ExtractedNode {
    let mut node = overflowing_text_child(value, parent, x, y, 64.0, 24.0, [1.0; 4]);
    node.style = Arc::new(ComputedStyle {
        color: Some([1.0; 4]),
        font_size: 16.0,
        ..ComputedStyle::default()
    });
    node.text = Some(TextContent {
        value: content.into(),
    });
    node
}

fn child_of(mut node: ExtractedNode, parent: u64) -> ExtractedNode {
    node.parent = Some(StableNodeId::new(parent).unwrap());
    node
}

/// A viewport scrolled horizontally by `offset`, holding a quad and a label at
/// fractional layout positions, as flex leaves them, and a nested `overflow:
/// hidden` card whose edges come from its clip and sit on whole device pixels
/// at every scale tested — where a snapped translation's ulps would decide
/// which side of the edge the scissor lands on. All of it `start` px into the
/// scrolled content, and the viewport scrolled `start + offset` to reach it.
fn horizontally_scrolled_scene(start: f32, offset: f32) -> [ExtractedNode; 5] {
    let mut viewport = overflow_parent(1, &[2, 3, 4], 0.0, 0.0, 160.0, 48.0, None);
    viewport.scroll_offset = nana_ui_runtime::ScrollOffset {
        x: start + offset,
        y: 0.0,
    };
    let mut card = overflow_parent(4, &[5], start + 80.0, 6.3, 40.0, 20.2, None);
    card.parent = Some(StableNodeId::new(1).unwrap());
    [
        viewport,
        child_of(
            colored_quad_node(2, start + 20.3, 4.6, 30.4, 12.2, [1.0, 0.0, 0.0, 1.0]),
            1,
        ),
        scrolled_label(3, 1, start + 24.6, 20.3, "Scroll"),
        card,
        child_of(
            colored_quad_node(5, start + 70.0, 0.0, 80.0, 40.0, [0.0, 1.0, 0.0, 1.0]),
            4,
        ),
    ]
}

#[test]
fn a_fractional_scroll_moves_quads_text_and_clips_by_the_same_whole_pixels() {
    let (device, queue) = test_device();
    // Whole-pixel steps at 1x; at 120% and 175% the snapped translation comes
    // back through the scale an ulp off the pixel, which must not show — nor
    // forty thousand pixels into the content, where the translation's own
    // rounding error is thousandths of a pixel.
    for (scale, start) in [1.0f32, 1.2, 1.75]
        .into_iter()
        .flat_map(|scale| [(scale, 0.0f32), (scale, 40_000.0)])
    {
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let logical = [160.0, 48.0];
        let physical = [
            (logical[0] * scale).round() as u32,
            (logical[1] * scale).round() as u32,
        ];
        let mut scene = UiScene::new();
        scene.apply_delta(horizontally_scrolled_scene(start, 0.0), []);
        let still = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            logical,
            physical,
            scale,
        );
        let warm = painter.text_glyph_counters();
        for step in 1..24 {
            let offset = step as f32 * 0.37;
            // Only the viewport changes: the scroll takes the Scene's fast
            // path, and its descendants are rebased rather than rebuilt.
            let [viewport, ..] = horizontally_scrolled_scene(start, offset);
            scene.apply_delta([viewport], []);
            let pixels = paint_scene_rgba(
                &device,
                &queue,
                &mut painter,
                &scene,
                logical,
                physical,
                scale,
            );
            // Everything under the scroll lands `shift` device pixels left
            // of where it stood: the same whole pixels for the quad, the
            // label and the card's clipped edges.
            let shift = ((start + offset) * scale).round() - (start * scale).round();
            let shift = shift as u32;
            for y in 0..physical[1] {
                for x in 0..physical[0] - shift {
                    assert_eq!(
                        pixel(&pixels, physical[0], x, y),
                        pixel(&still, physical[0], x + shift, y),
                        "at {scale}x scrolled {offset}, ({x},{y}) is not ({},{y}) moved \
                         {shift} px: something under the scroll moved by a different amount",
                        x + shift
                    );
                }
            }
        }
        let after = painter.text_glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "a fractional scroll at {scale}x, {start} px in, must not re-resolve the label (#223)"
        );
        assert_eq!(after.glyph_rasterized, warm.glyph_rasterized);
    }
}

/// A focused, unwrapped TextArea whose one line runs far past its right edge,
/// a selection and its caret in the middle of what it shows, settled.
fn long_line_editor() -> (AppContext, DocumentId, StableNodeId) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut view =
        nana_ui_runtime::TextArea::new("let horizontally_scrolled = editor.value(); // ".repeat(6));
    {
        let layout = Arc::make_mut(&mut view.style.layout);
        layout.white_space_nowrap = true;
        layout.font_size = Some(16.0);
        layout.margin_left = Some(LengthSpec::Px(10.3));
        layout.margin_top = Some(LengthSpec::Px(4.6));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Px(240.0));
        layout.height = Some(LengthSpec::Px(64.0));
    }
    // At a fractional position, as flex leaves an editor.
    let root = context
        .create_component(document, nana_ui_runtime::Stack::column(0.0))
        .unwrap();
    let area = context.create_component(document, view).unwrap();
    context.append_child(root, area).unwrap();
    let node = area.stable_id();
    let ids = [root.stable_id(), node];
    let viewport = nana_ui_runtime::LayoutViewport::new(260.0, 80.0);
    let mut shaper = crate::NanaTextShaper::default();
    let mut settle = |context: &mut AppContext| {
        context.resolve_styles(&ids).unwrap();
        context.shape_text(&ids, &mut shaper).unwrap();
        context.layout_document(document, viewport).unwrap();
        if context
            .shape_text_for_layout(document, &mut shaper)
            .unwrap()
        {
            context.layout_document(document, viewport).unwrap();
        }
        context.rebuild_hit_test(document);
    };
    settle(&mut context);
    assert!(context.focus_node(document, node).unwrap());
    assert!(context.select_focused_text_range(document, 8, 21).unwrap());
    settle(&mut context);
    (context, document, node)
}

#[test]
fn a_fractional_horizontal_scroll_of_an_editor_keeps_every_glyph() {
    // The editor's own scroll reaches the painter as a translation, so a
    // trackpad's fractional deltas move the value, caret and selection by the
    // same whole device pixels (#223) instead of re-resolving the paragraph.
    let (device, queue) = test_device();
    for scale in [1.0f32, 1.1, 1.2, 1.75] {
        let (mut context, document, node) = long_line_editor();
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let logical = [260.0, 80.0];
        let physical = [
            (logical[0] * scale).round() as u32,
            (logical[1] * scale).round() as u32,
        ];
        let mut scene = UiScene::new();
        let frame = |context: &AppContext, scene: &mut UiScene, painter: &mut SceneWgpuPainter| {
            scene.apply_delta(context.world().extract_document(document), []);
            paint_scene_rgba(&device, &queue, painter, scene, logical, physical, scale)
        };
        let still = frame(&context, &mut scene, &mut painter);
        let warm = painter.text_glyph_counters();
        let (content, _) = context.world().text_input_pointer_context(node).unwrap();
        let wheel = nana_ui_platform::InputPayload::Wheel(nana_ui_platform::WheelInput {
            pointer_id: nana_ui_platform::PointerId(1),
            x: content.x + content.width / 2.0,
            y: content.y + content.height / 2.0,
            delta_x: -0.37,
            delta_y: 0.0,
            unit: nana_ui_platform::WheelUnit::Pixels,
            modifiers: nana_ui_platform::InputModifiers::default(),
        });
        let mut input = nana_ui_runtime::HeadlessInput::bind(&mut context, document);
        for step in 1..24 {
            input.route(&mut context, wheel.clone()).unwrap();
            let offset = context.world().scroll_offset(node).unwrap_or_default().x;
            assert!(
                (offset - step as f32 * 0.37).abs() < 1.0e-3,
                "every slow delta is kept: {offset} after {step} at {scale}x"
            );
            let pixels = frame(&context, &mut scene, &mut painter);
            // Inside the content box everything moved `shift` device pixels
            // left of where it stood: glyphs, caret and selection alike.
            let shift = (offset * scale).round() as u32;
            let left = (content.x * scale).ceil() as u32 + 1;
            let right = ((content.x + content.width) * scale).floor() as u32 - 1;
            let top = (content.y * scale).ceil() as u32 + 1;
            let bottom = ((content.y + content.height) * scale).floor() as u32 - 1;
            for y in top..bottom {
                for x in left..right - shift {
                    assert_eq!(
                        pixel(&pixels, physical[0], x, y),
                        pixel(&still, physical[0], x + shift, y),
                        "at {scale}x scrolled {offset}, ({x},{y}) is not ({},{y}) moved \
                         {shift} px: the value, caret and selection moved apart",
                        x + shift
                    );
                }
            }
        }
        let after = painter.text_glyph_counters();
        assert_eq!(
            after.text_instance_rebuilds, warm.text_instance_rebuilds,
            "a fractional horizontal scroll of an editor at {scale}x must not re-resolve its value"
        );
        assert_eq!(after.glyph_rasterized, warm.glyph_rasterized);
    }
}

#[test]
fn a_frozen_row_stays_put_under_a_fractional_scroll() {
    let (device, queue) = test_device();
    for scale in [1.0f32, 1.2, 1.75] {
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let logical = [160.0, 48.0];
        let physical = [
            (logical[0] * scale).round() as u32,
            (logical[1] * scale).round() as u32,
        ];
        // A frozen column cancels its scroller's offset with a translation of
        // its own, as a virtualized table's does. Snapping the scroll term on
        // its own would leave the two a fraction apart, and the column would
        // shimmer by a pixel and re-resolve its labels on every frame.
        let frozen = |offset: f32| {
            let mut viewport = overflow_parent(1, &[2], 0.0, 0.0, 160.0, 48.0, None);
            viewport.scroll_offset = nana_ui_runtime::ScrollOffset { x: offset, y: 0.0 };
            let mut column = overflow_parent(
                2,
                &[3, 4],
                10.3,
                2.6,
                64.4,
                40.2,
                Some(PaintTransform {
                    e: offset,
                    ..PaintTransform::default()
                }),
            );
            column.parent = Some(StableNodeId::new(1).unwrap());
            [
                viewport,
                column,
                child_of(
                    colored_quad_node(3, 12.7, 4.6, 30.4, 12.2, [1.0, 0.0, 0.0, 1.0]),
                    2,
                ),
                scrolled_label(4, 2, 14.6, 20.3, "Frozen"),
            ]
        };
        let mut scene = UiScene::new();
        scene.apply_delta(frozen(0.0), []);
        let still = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            logical,
            physical,
            scale,
        );
        let warm = painter.text_glyph_counters();
        for step in 1..24 {
            let offset = step as f32 * 0.37;
            let [viewport, column, ..] = frozen(offset);
            scene.apply_delta([viewport, column], []);
            let pixels = paint_scene_rgba(
                &device,
                &queue,
                &mut painter,
                &scene,
                logical,
                physical,
                scale,
            );
            assert!(
                pixels == still,
                "at {scale}x scrolled {offset}, the frozen column moved"
            );
        }
        assert_eq!(
            painter.text_glyph_counters().text_instance_rebuilds,
            warm.text_instance_rebuilds,
            "a frozen label under a fractional scroll keeps its glyphs at {scale}x"
        );
    }
}

#[test]
fn a_custom_node_moves_by_the_scrolls_whole_pixels() {
    // What a custom renderer is told it covers, forty thousand pixels down a
    // scroll at 110%: the same whole pixels a quad there moves by, however the
    // snapped translation's own rounding error falls.
    let scale = 1.1;
    let origin = super::clip::PaintOrigin::new([0.0, 0.0], scale);
    let bounds = LogicalRect::from_xywh(0.0, 40_000.0, 50.0, 50.0);
    let covered = |offset: f32| {
        let affine = super::clip::paint_affine([1.0, 0.0, 0.0, 1.0, 0.0, -offset], origin);
        let rect = super::clip::physical_bounds(
            super::custom_paint_bounds(bounds, affine, [0.0; 2], scale),
            scale,
            PhysicalRect {
                x: 0,
                y: 0,
                width: u32::MAX,
                height: u32::MAX,
            },
        );
        (rect.y as f32, rect.height, (affine[5] * scale).round())
    };
    let (top, height, from) = covered(39_800.0);
    for step in 1..300 {
        let (moved, moved_height, to) = covered(39_800.0 + step as f32 * 0.37);
        assert_eq!((moved, moved_height), (top + to - from, height));
    }
}

fn label_scene(text: &str) -> UiScene {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            RuntimeButton::new(text)
                .kind(ButtonKind::Selected)
                .layout(square_button_layout()),
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, button.stable_id(), 8.0, 16.0, 80.0, 32.0);
    context.commit_mutations(layout).unwrap();
    commit_scene(&mut context)
}

struct FrameProbe {
    gpu: nana_gpu::GpuContext,
    texture: wgpu::Texture,
    target: nana_gpu::GpuRenderTarget,
    viewport: ScenePaintViewport,
}

impl FrameProbe {
    const SIZE: [u32; 2] = [96, 64];

    fn new() -> Self {
        let gpu = crate::test_gpu::context();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let (texture, view) = test_copy_target(
            __framework::device(&gpu),
            format,
            Self::SIZE[0],
            Self::SIZE[1],
        );
        let target = __framework::render_target(&gpu, view, format, Self::SIZE);
        Self {
            gpu,
            texture,
            target,
            viewport: ScenePaintViewport {
                logical_size: [Self::SIZE[0] as f32, Self::SIZE[1] as f32],
                physical_size: Self::SIZE,
                scale_factor: 1.0,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear: true,
            },
        }
    }

    fn painter(&self) -> SceneWgpuPainter {
        SceneWgpuPainter::new(&self.gpu, GpuTextureFormat::RGBA8_UNORM)
    }

    fn pixels(&self) -> Vec<u8> {
        let device = __framework::device(&self.gpu);
        readback_rgba(
            device,
            __framework::queue(&self.gpu),
            device.create_command_encoder(&Default::default()),
            &self.texture,
            Self::SIZE[0],
            Self::SIZE[1],
        )
    }

    fn reference(&self, scene: &UiScene) -> Vec<u8> {
        let mut painter = self.painter();
        let mut frame = self.gpu.begin_frame("reference");
        painter
            .paint(scene, &mut frame, &self.target, self.viewport, None, None)
            .unwrap();
        frame.submit();
        self.pixels()
    }
}

/// A frame dropped after the painter recorded into it never reaches the GPU,
/// though the painter treated its writes as done: the retained dest and text
/// state would be reused on the next paint and show what that frame never
/// drew. Dropping the frame rolls the target back, so the next paint of the
/// same scene is complete.
#[test]
fn a_dropped_frame_rolls_its_targets_back_so_the_next_paint_is_complete() {
    let probe = FrameProbe::new();
    let before = label_scene("Alpha");
    let after = label_scene("Omega wide");
    let expected = probe.reference(&after);
    assert_ne!(probe.reference(&before), expected, "the scenes differ");

    for window in [Some(RenderTargetId(7)), None] {
        let mut painter = probe.painter();
        let paint = |painter: &mut SceneWgpuPainter, scene: &UiScene, label| {
            let mut frame = probe.gpu.begin_frame(label);
            match window {
                Some(id) => painter.paint_target(
                    id,
                    scene,
                    &mut frame,
                    &probe.target,
                    probe.viewport,
                    None,
                    None,
                ),
                None => painter.paint(scene, &mut frame, &probe.target, probe.viewport, None, None),
            }
            .unwrap();
            frame
        };
        paint(&mut painter, &before, "before").submit();
        drop(paint(&mut painter, &after, "dropped"));
        paint(&mut painter, &after, "after").submit();
        assert!(
            probe.pixels() == expected,
            "{window:?}: the paint after a dropped frame must not reuse what it recorded"
        );
    }
}

#[test]
fn a_target_held_by_an_unsubmitted_frame_is_not_painted_again() {
    let probe = FrameProbe::new();
    let scene = label_scene("Held");
    let mut painter = probe.painter();
    let id = RenderTargetId(3);
    let mut first = probe.gpu.begin_frame("first");
    painter
        .paint_target(
            id,
            &scene,
            &mut first,
            &probe.target,
            probe.viewport,
            None,
            None,
        )
        .unwrap();
    let mut second = probe.gpu.begin_frame("second");
    assert_eq!(
        painter.paint_target(
            id,
            &scene,
            &mut second,
            &probe.target,
            probe.viewport,
            None,
            None
        ),
        Err(ScenePaintError::TargetInFlight(Some(id)))
    );
    painter
        .paint_target(
            RenderTargetId(4),
            &scene,
            &mut second,
            &probe.target,
            probe.viewport,
            None,
            None,
        )
        .expect("another target is independent");
    painter
        .paint_target(
            id,
            &scene,
            &mut first,
            &probe.target,
            probe.viewport,
            None,
            None,
        )
        .expect("the frame that holds it may paint it again");
    first.submit();
    painter
        .paint_target(
            id,
            &scene,
            &mut second,
            &probe.target,
            probe.viewport,
            None,
            None,
        )
        .expect("a submitted frame releases the target");
    second.submit();
}

#[test]
fn frames_targets_and_host_textures_from_another_device_are_refused() {
    let probe = FrameProbe::new();
    let other = __framework::adopt(
        __framework::adapter(&probe.gpu).clone(),
        __framework::device(&probe.gpu).clone(),
        __framework::queue(&probe.gpu).clone(),
    );
    let scene = label_scene("Device");
    let mut painter = probe.painter();
    let expected = probe.gpu.generation();
    let found = other.generation();
    let mut foreign_frame = other.begin_frame("foreign frame");
    assert_eq!(
        painter.paint(
            &scene,
            &mut foreign_frame,
            &probe.target,
            probe.viewport,
            None,
            None
        ),
        Err(ScenePaintError::DeviceMismatch { expected, found })
    );
    let foreign_target = __framework::render_target(
        &other,
        __framework::target_view(&probe.target).clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        FrameProbe::SIZE,
    );
    let mut frame = probe.gpu.begin_frame("foreign target");
    assert_eq!(
        painter.paint(
            &scene,
            &mut frame,
            &foreign_target,
            probe.viewport,
            None,
            None
        ),
        Err(ScenePaintError::DeviceMismatch { expected, found })
    );

    let registry = HostTextureRegistry::new();
    let stale = other
        .create_texture(&nana_gpu::GpuTextureDescriptor {
            label: Some("stale host texture"),
            width: 4,
            height: 4,
            format: GpuTextureFormat::RGBA8_UNORM,
            usage: nana_gpu::GpuTextureUsages::SAMPLED,
        })
        .unwrap();
    registry.register(
        "preview",
        crate::HostTexture::new(1, 1, &stale),
        4,
        4,
        crate::HostTextureAlphaMode::Opaque,
    );
    let mut child = host_texture_child(1, 99, 0.0, 0.0, 32.0, 32.0, "preview");
    child.parent = None;
    let mut scene = UiScene::new();
    scene.apply_delta([child], []);
    let id = scene.frame_plan().unwrap().custom_nodes[0];
    assert_eq!(
        painter.paint(
            &scene,
            &mut frame,
            &probe.target,
            probe.viewport,
            Some(&registry),
            None
        ),
        Err(ScenePaintError::StaleHostTexture(id))
    );
}

/// Rebuilding a rolled-back window is not closing it: the fetch host the host
/// installed once keeps serving the window's remote images.
#[test]
fn a_rolled_back_target_keeps_the_fetch_host() {
    let probe = FrameProbe::new();
    let scene = label_scene("Fetch");
    let mut painter = probe.painter();
    let host = super::image_url::loopback_fetch_host();
    painter.set_resource_fetch_host(Some(host.clone()));
    let id = RenderTargetId(5);
    let paint = |painter: &mut SceneWgpuPainter, label| {
        let mut frame = probe.gpu.begin_frame(label);
        painter
            .paint_target(
                id,
                &scene,
                &mut frame,
                &probe.target,
                probe.viewport,
                None,
                None,
            )
            .unwrap();
        frame
    };
    paint(&mut painter, "painted").submit();
    drop(paint(&mut painter, "dropped"));
    paint(&mut painter, "rebuilt").submit();
    assert!(
        egress_of(painter.fetch_host.as_ref()) == egress_of(Some(&host)),
        "the rollback must not release the window's fetch host"
    );
}
