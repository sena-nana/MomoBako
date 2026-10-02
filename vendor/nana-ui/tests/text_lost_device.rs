//! A frame painted after the device is lost, before the host notices, must
//! not bring the process down: the host's recovery (#97) replaces the device
//! and every painter on the next frame, and a panic here would pre-empt it.
//!
//! One test in its own binary: it owns and destroys its device, which must
//! not happen while other test threads come and go (see `test_gpu.rs`).

use nana_ui::{
    GpuContext, GpuRenderTarget, GpuTextureFormat, NanaTextShaper, ScenePaintViewport,
    SceneWgpuPainter, runtime::*, wgpu,
};
use nana_ui_scene::UiScene;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [320, 160];

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
        label: Some("text lost device"),
        ..Default::default()
    }))
    .expect("GPU tests require a WGPU device");
    GpuContext::from_wgpu(adapter, device, queue)
}

/// A settled document of `labels`, and the scene it extracts to.
fn scene(labels: &[String]) -> UiScene {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::column(8.0)).unwrap();
    let mut ids = vec![root.stable_id()];
    for text in labels {
        let label = cx.create_component(doc, Text::new(text.as_str())).unwrap();
        cx.append_child(root, label).unwrap();
        ids.push(label.stable_id());
    }
    let viewport = LayoutViewport::new(SIZE[0] as f32, SIZE[1] as f32);
    cx.resolve_styles(&ids).unwrap();
    cx.shape_text(&ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, viewport).unwrap();
    let mut scene = UiScene::new();
    scene.apply_delta(cx.world().extract_document(doc), []);
    scene
}

fn paint(gpu: &GpuContext, painter: &mut SceneWgpuPainter, scene: &UiScene) {
    let texture = gpu
        .wgpu()
        .device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("text lost device target"),
            size: wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let target = GpuRenderTarget::from_wgpu(gpu, view, FORMAT, SIZE);
    let mut frame = gpu.begin_frame("text lost device frame");
    let viewport = ScenePaintViewport {
        logical_size: [SIZE[0] as f32, SIZE[1] as f32],
        physical_size: SIZE,
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [1.0, 1.0, 1.0, 1.0],
        clear: true,
    };
    // Whether the lost device refuses the frame or encodes it into nothing is
    // up to wgpu; what matters is that it returns.
    let _ = painter.paint(scene, &mut frame, &target, viewport, None, None);
    frame.submit();
}

#[test]
fn a_frame_painted_on_a_lost_device_returns() {
    let gpu = device();
    let mut painter = SceneWgpuPainter::new(&gpu, GpuTextureFormat::from_wgpu(FORMAT));
    let labels = (0..40).map(|row| format!("Row {row}")).collect::<Vec<_>>();
    paint(&gpu, &mut painter, &scene(&labels));
    gpu.wgpu().device().destroy();
    // New text on the lost device: blocks and ranges to stage and copy.
    let labels = (0..40)
        .map(|row| format!("Changed row {row} after the loss"))
        .collect::<Vec<_>>();
    paint(&gpu, &mut painter, &scene(&labels));
}
