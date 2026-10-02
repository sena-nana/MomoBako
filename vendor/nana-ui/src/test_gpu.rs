//! GPU device for unit tests.
//!
//! The whole test process shares one `wgpu::Device`, created on first use.
//! Creating devices while tests run deadlocks on Windows with the NVIDIA Vulkan
//! ICD: device creation loads libraries under the driver's lock while
//! libtest's per-test threads exit under the loader lock, and `cargo test`
//! hangs with every worker stuck in a GPU test. Tests only rely on their own
//! painters, targets and resources, never on owning the device.

use std::sync::OnceLock;

use nana_gpu::{
    __framework, GpuContext, GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureUsages,
};

/// The process-wide test device and queue.
pub(crate) fn device() -> (wgpu::Device, wgpu::Queue) {
    let gpu = context();
    (
        __framework::device(&gpu).clone(),
        __framework::queue(&gpu).clone(),
    )
}

/// A sampled RGBA8 texture on the test device.
pub(crate) fn texture(width: u32, height: u32) -> GpuTexture {
    context()
        .create_texture(&GpuTextureDescriptor {
            label: Some("nana-ui test texture"),
            width,
            height,
            format: GpuTextureFormat::RGBA8_UNORM,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
        })
        .expect("test texture")
}

/// The texture behind a test view, on the test device.
pub(crate) fn wrap_view(view: &wgpu::TextureView) -> GpuTexture {
    __framework::texture_from_wgpu(&context(), view.texture().clone())
}

/// The process-wide test device as the GPU contract sees it.
pub(crate) fn context() -> GpuContext {
    static CONTEXT: OnceLock<GpuContext> = OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::from_env().unwrap_or_default(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
                &instance, None,
            ))
            .expect("GPU tests require a WGPU adapter");
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: Some("nana-ui test device"),
                    // What the hosted context asks for too, when the adapter has it.
                    required_features: adapter.features() & wgpu::Features::DUAL_SOURCE_BLENDING,
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                }))
                .expect("GPU tests require a WGPU device");
            __framework::adopt(adapter, device, queue)
        })
        .clone()
}
