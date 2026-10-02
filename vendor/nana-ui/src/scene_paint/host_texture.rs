use super::clip::{self, LogicalRect, physical_scissor};
use crate::gpu_texture::{GpuTexturePipeline, GpuTexturePrimitive, HostTextureLayer};
use crate::gpu_view::intersect_physical;
use crate::{HostTextureBinding, PhysicalRect};

pub(super) struct HostTexturePipeline {
    pipeline: GpuTexturePipeline,
}

pub(super) struct PreparedHostTexture {
    primitive: GpuTexturePrimitive,
    clip: PhysicalRect,
    /// 这一帧这张宿主纹理实际覆盖的设备像素范围(变换后 AABB × 缩放)。
    /// 消费方据此知道该按多少像素准备内容,不必从布局盒自己反推。
    pub(super) painted: [u32; 2],
}

impl HostTexturePipeline {
    pub(super) fn invalidate_image_bindings(&mut self) {
        self.pipeline.invalidate_image_bindings();
    }

    pub(super) fn invalidate_target_image_bindings(
        target: &mut crate::gpu_texture::GpuTextureTarget,
    ) {
        GpuTexturePipeline::invalidate_target_image_bindings(target);
    }
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        policy: &nana_gpu::GpuDeviceState,
        gpu: &nana_gpu::GpuContext,
    ) -> Self {
        Self {
            pipeline: GpuTexturePipeline::new_with_policy(
                device,
                queue,
                format,
                Some(policy),
                Some(gpu),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url_cache: &mut super::url_texture_cache::UrlTextureCache,
        binding: HostTextureBinding,
        node: u64,
        slot: u64,
        bounds: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        clip: PhysicalRect,
        opacity: f32,
        corner_radii: [f32; 4],
        rounded_clip: LogicalRect,
        fragment_clip: super::clip::FragmentClip,
        physical_size: [u32; 2],
        scale_factor: f32,
        mask: Option<nana_ui_core::MaskImage>,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
        checkerboard: bool,
        zoom: f32,
        sampling: nana_ui_core::ImageSampling,
    ) -> PreparedHostTexture {
        let primitive = GpuTexturePrimitive::from_scene(
            node,
            slot,
            HostTextureLayer::from_binding(binding)
                .with_opacity(opacity)
                .with_corner_radii(corner_radii)
                .with_clip(crate::geometry::LogicalRect::new(
                    rounded_clip.x,
                    rounded_clip.y,
                    rounded_clip.width,
                    rounded_clip.height,
                ))
                .with_fragment_clip(
                    fragment_clip.rect,
                    fragment_clip.inv_abcd,
                    fragment_clip.inv_ef,
                    fragment_clip.corner_radius,
                )
                .with_mask(mask)
                .with_checkerboard(checkerboard)
                .with_zoom(zoom)
                .with_sampling(sampling),
        );
        primitive.prepare(
            &mut self.pipeline,
            url_cache,
            device,
            queue,
            crate::geometry::LogicalRect::new(bounds.x, bounds.y, bounds.width, bounds.height),
            affine,
            persp,
            scale_factor,
            physical_size,
            gpu_work,
        );
        let world = if clip::is_translation_projective(affine, persp) {
            clip::translated_on_grid(bounds, affine, scale_factor)
        } else {
            clip::transformed_aabb_projective(bounds, affine, persp)
        };
        let painted = [
            physical_extent(world.width, scale_factor),
            physical_extent(world.height, scale_factor),
        ];
        let clip = physical_scissor(world, scale_factor, physical_size)
            .map(|world| intersect_physical(world, clip))
            .unwrap_or(PhysicalRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        PreparedHostTexture {
            primitive,
            clip,
            painted,
        }
    }

    pub(super) fn draw(
        &self,
        prepared: &PreparedHostTexture,
        pass: &mut wgpu::RenderPass<'_>,
        dest_size: [u32; 2],
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        prepared
            .primitive
            .draw_in_pass(&self.pipeline, pass, prepared.clip, dest_size, gpu_work);
    }

    pub(super) fn trim(&mut self) {
        self.pipeline.trim();
    }
}

/// 逻辑长度换成设备像素。非有限值与负值当作 0——没画到就是没画到,
/// 让消费方拿到一个「无」而不是一个巨大的假尺寸。
fn physical_extent(logical: f32, scale_factor: f32) -> u32 {
    let pixels = logical * scale_factor;
    if !pixels.is_finite() || pixels <= 0.0 {
        return 0;
    }
    pixels.round() as u32
}

impl HostTexturePipeline {
    pub(super) fn swap_target(&mut self, target: &mut crate::gpu_texture::GpuTextureTarget) {
        self.pipeline.swap_target(target);
    }
}
