//! Device loss / recreation recovers text from the state Runtime retains (#97).
//!
//! A host that loses its device drops every painter and builds new ones on
//! the replacement (`switch_gpu`). Everything the new painter needs to draw
//! text is on the CPU side of that line: the paragraphs Runtime measured ride
//! in the scene, and the faces live in the process-wide engine. So the first
//! frame on the new device rasterizes and uploads its glyphs once — the atlas
//! died with the old device — and lays nothing out.
//!
//! A DPI change on that device is the same kind of event: Runtime lays text
//! out in logical px, so the painter re-rasterizes at the new scale and still
//! draws the paragraphs it was handed.
//!
//! One test in its own binary: it creates two devices, which must not happen
//! while other test threads come and go (see `test_gpu.rs`).

use nana_ui::{
    GpuContext, GpuRenderTarget, GpuTextureFormat, NanaTextShaper, ScenePaintViewport,
    SceneWgpuPainter, runtime::*, wgpu,
};
use nana_ui_scene::{ScenePrimitiveKind, UiScene};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [320, 160];
const LABELS: [&str; 3] = [
    "Recovered after device loss",
    "装置が失われても段落は残る",
    "النص المحفوظ",
];

fn device() -> GpuContext {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
        &instance, None,
    ))
    .expect("GPU tests require a WGPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("text device recreation"),
        ..Default::default()
    }))
    .expect("GPU tests require a WGPU device");
    GpuContext::from_wgpu(adapter, device, queue)
}

/// A settled document with a few paragraphs, and the scene it extracts to.
fn retained_scene() -> UiScene {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::column(8.0)).unwrap();
    let mut ids = vec![root.stable_id()];
    for text in LABELS {
        let label = cx.create_component(doc, Text::new(text)).unwrap();
        cx.append_child(root, label).unwrap();
        ids.push(label.stable_id());
    }
    let viewport = LayoutViewport::new(SIZE[0] as f32, SIZE[1] as f32);
    cx.resolve_styles(&ids).unwrap();
    cx.shape_text(&ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, viewport).unwrap();
    if cx
        .shape_text_for_layout(doc, &mut NanaTextShaper::default())
        .unwrap()
    {
        cx.layout_document(doc, viewport).unwrap();
    }
    let mut scene = UiScene::new();
    scene.apply_delta(cx.world().extract_document(doc), []);
    let retained = scene
        .primitives()
        .filter(|primitive| {
            matches!(
                primitive.kind,
                ScenePrimitiveKind::Text {
                    layout: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        retained,
        LABELS.len(),
        "every label carries the paragraph Runtime measured"
    );
    scene
}

/// Paint `scene` once at `scale` and read the pixels back.
fn paint(gpu: &GpuContext, painter: &mut SceneWgpuPainter, scene: &UiScene, scale: u32) -> Vec<u8> {
    let device = gpu.wgpu().device();
    let size = [SIZE[0] * scale, SIZE[1] * scale];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("text device recreation target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let target = GpuRenderTarget::from_wgpu(gpu, view, FORMAT, size);
    let mut frame = gpu.begin_frame("text device recreation");
    painter
        .paint(
            scene,
            &mut frame,
            &target,
            ScenePaintViewport {
                logical_size: [SIZE[0] as f32, SIZE[1] as f32],
                physical_size: size,
                scale_factor: scale as f32,
                scene_origin: [0.0, 0.0],
                target_origin: [0.0, 0.0],
                clear_color: [1.0, 1.0, 1.0, 1.0],
                clear: true,
            },
            None,
            None,
        )
        .expect("paint");

    let row = size[0] as usize * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("text device recreation readback"),
        size: (padded * size[1] as usize) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    frame.wgpu_encoder().copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    frame.submit();
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("readback poll");
    let mapped = slice.get_mapped_range().expect("readback is mapped");
    let mut pixels = Vec::with_capacity(row * size[1] as usize);
    for line in mapped.chunks_exact(padded) {
        pixels.extend_from_slice(&line[..row]);
    }
    drop(mapped);
    pixels
}

#[test]
fn a_replacement_device_draws_the_retained_paragraphs_without_laying_them_out() {
    let scene = retained_scene();

    let lost = device();
    let mut painter = SceneWgpuPainter::new(&lost, GpuTextureFormat::from_wgpu(FORMAT));
    let before = paint(&lost, &mut painter, &scene, 1);
    let first = painter.text_glyph_counters();
    assert!(
        before.iter().any(|&channel| channel != u8::MAX),
        "the fixture has to draw some text"
    );
    // The device goes away, and with it the atlas and every GPU buffer. What
    // survives is exactly what a host keeps across `switch_gpu`: the world,
    // its scene, and the process-wide text engine.
    drop(painter);
    lost.wgpu().device().destroy();
    drop(lost);

    let replacement = device();
    let mut painter = SceneWgpuPainter::new(&replacement, GpuTextureFormat::from_wgpu(FORMAT));
    let after = paint(&replacement, &mut painter, &scene, 1);
    let second = painter.text_glyph_counters();
    let shape_cache = painter.text_shape_cache_stats();
    assert_eq!(
        second.text_retained_layouts_drawn,
        LABELS.len() as u64,
        "the new painter draws the paragraphs Runtime retained: {second:?}"
    );
    assert_eq!(
        (shape_cache.0, shape_cache.1),
        (0, 0),
        "and never lays one out of its own"
    );
    assert_eq!(
        (second.glyph_rasterized, second.glyph_upload_regions),
        (first.glyph_rasterized, first.glyph_upload_regions),
        "the atlas died with the device, so every glyph is rasterized and \
         uploaded again — once, as on the first device"
    );
    assert!(
        before == after,
        "the recovered frame is the frame that was lost"
    );

    // The window moves to a 2x display. The layouts are in logical px, so
    // the same paragraphs are drawn again, rasterized for the new scale.
    paint(&replacement, &mut painter, &scene, 2);
    let doubled = painter.text_glyph_counters();
    assert_eq!(
        doubled.text_retained_layouts_drawn - second.text_retained_layouts_drawn,
        LABELS.len() as u64,
        "the painter draws the same retained paragraphs at 2x: {doubled:?}"
    );
    assert_eq!(
        painter.text_shape_cache_stats(),
        shape_cache,
        "and still lays none out of its own"
    );
    assert!(
        doubled.glyph_rasterized > second.glyph_rasterized,
        "the glyphs are rasterized for the pixels they now cover"
    );
}
