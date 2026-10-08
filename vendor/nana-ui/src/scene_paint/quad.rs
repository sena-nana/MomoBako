use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use nana_gpu::{__framework, LogicalBinding, LogicalBindingType, ResourceTable, ShaderStage};
use nana_ui_core::{
    BackgroundImage, BackgroundImageFit, BackgroundRepeat, BorderImageSpec, BorderImageTile,
    CssGradient, GradientStop, LengthSpec, LinearGradient, MAX_BACKGROUND_LAYERS, MaskImage,
};
use nana_ui_runtime::ComponentElevation;
use nana_ui_scene::QuadSurfacePaint;

use super::{
    clip::LogicalRect,
    color::{orthographic, pack_linear, with_opacity},
    url_texture_cache::{Demand, LayerDemand, UrlTextureCache, cache_key},
};
use crate::{PhysicalRect, gpu_work::ManagedBuffer};

const INITIAL_INSTANCES: usize = 256;
const PAINT_GRADIENT: u32 = 1;
const PAINT_MASK: u32 = 2;
const PAINT_URL: u32 = 4;
const PAINT_FILTER: u32 = 8;
const PAINT_POLYGON: u32 = 16;
const PAINT_RADIAL: u32 = 32;
const PAINT_MASK_RADIAL: u32 = 64;
const PAINT_SHADOW_INSET: u32 = 128;
const PAINT_MASK_URL: u32 = 256;
const QUAD_VERTEX_FRAGMENT: &[ShaderStage] = &[ShaderStage::Vertex, ShaderStage::Fragment];
const QUAD_FRAGMENT: &[ShaderStage] = &[ShaderStage::Fragment];
fn quad_layout_key() -> u64 {
    ResourceTable::new(vec![
        LogicalBinding::new(0, LogicalBindingType::UniformBuffer, QUAD_VERTEX_FRAGMENT)
            .min_size(std::mem::size_of::<Uniforms>() as u64),
        LogicalBinding::new(
            1,
            LogicalBindingType::StorageBuffer { read_only: true },
            QUAD_FRAGMENT,
        ),
        LogicalBinding::new(2, LogicalBindingType::SampledTexture, QUAD_FRAGMENT),
        LogicalBinding::new(3, LogicalBindingType::Sampler, QUAD_FRAGMENT),
    ])
    .expect("quad table")
    .layout_key()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SolidInstance {
    color: [f32; 4],
    position: [f32; 2],
    size: [f32; 2],
    border_color: [f32; 4],
    border_radius: [f32; 4],
    border_widths: [f32; 4],
    shadow_color: [f32; 4],
    shadow_offset: [f32; 2],
    shadow_blur_radius: f32,
    /// Outline width plus the spread, negated for an inset shadow.
    shadow_spread_radius: f32,
    /// Scene-space pivot of the GPU transform overlay.
    motion_origin: [f32; 2],
    /// Pixel snap and transform id, the vertex stage's half of the packed
    /// motion snap; the high half carries the paint's inset shadow flag.
    snap: u32,
    affine_abcd: [f32; 4],
    affine_ef: [f32; 4],
    clip_rect: [f32; 4],
    clip_inv_abcd: [f32; 4],
    clip_inv_ef: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct QuadPaintData {
    flags: u32,
    grad_angle: f32,
    grad_stop_count: u32,
    mask_stop_count: u32,
    mask_angle: f32,
    polygon_count: u32,
    url_tex_index: u32,
    url_fit: u32,
    filter_b: f32,
    filter_s: f32,
    filter_c: f32,
    filter_hue: f32,
    grad_stops0: [f32; 4],
    grad_stops1: [f32; 4],
    grad_stops2: [f32; 4],
    grad_stops3: [f32; 4],
    grad_pos: [f32; 4],
    grad_pos2: [f32; 4],
    mask_stops0: [f32; 4],
    mask_stops1: [f32; 4],
    mask_pos: [f32; 4],
    poly0: [f32; 4],
    poly1: [f32; 4],
    poly2: [f32; 4],
    poly3: [f32; 4],
    grad_stops4: [f32; 4],
    grad_stops5: [f32; 4],
    grad_stops6: [f32; 4],
    grad_stops7: [f32; 4],
    mask_stops2: [f32; 4],
    mask_stops3: [f32; 4],
    mask_stops4: [f32; 4],
    mask_stops5: [f32; 4],
    mask_stops6: [f32; 4],
    mask_stops7: [f32; 4],
    mask_pos2: [f32; 4],
    grad_center_x: f32,
    grad_center_y: f32,
    grad_radial_shape: u32,
    _pad_tail0: u32,
    mask_center_x: f32,
    mask_center_y: f32,
    mask_radial_shape: u32,
    _pad_tail1: u32,
    url_dest: [f32; 4],
    outline_width: f32,
    border_styles: u32,
    filter_invert: f32,
    filter_opacity: f32,
    outline_color: [f32; 4],
    border_color_right: [f32; 4],
    border_color_bottom: [f32; 4],
    border_color_left: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<QuadPaintData>() == 560);
const _: () = assert!(std::mem::align_of::<QuadPaintData>() == 4);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct Uniforms {
    transform: [f32; 16],
    scale: f32,
    _padding: [f32; 3],
}

pub(super) struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    pipeline_msaa: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    /// What `uniforms` holds, so a frame of the same size writes nothing.
    uploaded_uniforms: Option<Uniforms>,
    paint_buffer: ManagedBuffer,
    paint_capacity: usize,
    url_view: wgpu::TextureView,
    url_sampler: wgpu::Sampler,
    url_bind_groups: HashMap<Option<String>, wgpu::BindGroup>,
    instances: ManagedBuffer,
    instance_capacity: usize,
    pending: Vec<SolidInstance>,
    uploaded: Vec<SolidInstance>,
    pending_paint: Vec<QuadPaintData>,
    uploaded_paint: Vec<QuadPaintData>,
    pending_urls: Vec<Option<String>>,
    motion_layout: wgpu::BindGroupLayout,
    #[allow(
        dead_code,
        reason = "keeps dummy motion buffers alive for the shared layout"
    )]
    motion_dummy: wgpu::BindGroup,
    motion_ids: (u32, u32),
    motion_origin: [f32; 2],
    /// Physical px per logical px this frame: the grid a translated quad's
    /// position is put on (`clip::on_grid`).
    grid_scale: f32,
}

impl QuadPipeline {
    #[cfg(test)]
    pub(super) fn cached_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }
    #[cfg(test)]
    pub(super) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::new_with_policy(device, format, None)
    }

    pub(super) fn new_with_policy(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.quad.solid.shader"),
            source: wgpu::ShaderSource::Wgsl(super::corner_shape::specialize(concat!(
                include_str!("shader/motion.wgsl"),
                "\n",
                include_str!("shader/color.wgsl"),
                "\n",
                include_str!("shader/quad.wgsl"),
                "\n",
                include_str!("shader/vertex.wgsl"),
                "\n",
                include_str!("shader/quad_paint_data.wgsl"),
                "\n",
                include_str!("shader/quad_paint.wgsl"),
                "\n",
                include_str!("shader/quad_solid.wgsl"),
            ), super::corner_shape::corner_exponent())),
        });
        // Trilinear. A default (`ImageSampling::Resample`) texture has one
        // level at its painted size, so this is plain bilinear for it; only an
        // `ImageSampling::Mipmap` texture has levels to blend.
        let url_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nana-ui.scene.quad.url.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let url_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("nana-ui.scene.quad.url.fallback"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let bind_layout = if let Some(policy) = policy {
            let table = ResourceTable::new(vec![
                LogicalBinding::new(0, LogicalBindingType::UniformBuffer, QUAD_VERTEX_FRAGMENT)
                    .min_size(std::mem::size_of::<Uniforms>() as u64),
                LogicalBinding::new(
                    1,
                    LogicalBindingType::StorageBuffer { read_only: true },
                    QUAD_FRAGMENT,
                ),
                LogicalBinding::new(2, LogicalBindingType::SampledTexture, QUAD_FRAGMENT),
                LogicalBinding::new(3, LogicalBindingType::Sampler, QUAD_FRAGMENT),
            ])
            .expect("quad logical resource table is valid")
            .for_generation(policy.generation());
            let logical = __framework::logical_layout(device, policy.generation(), &table)
                .expect("quad logical layout matches the device");
            __framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.quad.layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<Uniforms>() as u64,
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            })
        };
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let paint_capacity = INITIAL_INSTANCES;
        let paint_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.paint"),
            size: (paint_capacity * std::mem::size_of::<QuadPaintData>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.quad.bind_group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: paint_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&url_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&url_sampler),
                },
            ],
        });
        let mut url_bind_groups = HashMap::new();
        url_bind_groups.insert(None, bind_group);
        let motion_layout = super::motion::motion_bind_layout_with_policy(device, policy);
        let dummy_desc = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.motion.descriptors"),
            size: 272,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_kf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.motion.keyframes"),
            size: 80,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_time = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.motion.time"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let motion_dummy = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.quad.motion.dummy"),
            layout: &motion_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: dummy_desc.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: dummy_kf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: dummy_time.as_entire_binding(),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.quad.solid.pipeline"),
            bind_group_layouts: &[Some(&bind_layout), Some(&motion_layout)],
            immediate_size: 0,
        });
        let pipeline = cached_solid_pipeline(policy, device, &shader, &layout, format, 1, 1);
        let pipeline_msaa = cached_solid_pipeline(policy, device, &shader, &layout, format, 4, 2);
        let instance_capacity = INITIAL_INSTANCES;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.quad.instances"),
            size: (instance_capacity * std::mem::size_of::<SolidInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            pipeline_msaa,
            bind_layout,
            uniforms,
            uploaded_uniforms: None,
            paint_buffer: ManagedBuffer::new(paint_buffer),
            paint_capacity,
            url_view,
            url_sampler,
            url_bind_groups,
            instances: ManagedBuffer::new(instances),
            instance_capacity,
            pending: Vec::new(),
            uploaded: Vec::new(),
            pending_paint: Vec::new(),
            uploaded_paint: Vec::new(),
            pending_urls: Vec::new(),
            motion_layout,
            motion_dummy,
            motion_ids: (0, 0),
            motion_origin: [0.0, 0.0],
            grid_scale: 1.0,
        }
    }

    pub(super) fn begin_frame(&mut self, scale: f32) {
        self.grid_scale = scale;
        self.pending.clear();
        self.pending_paint.clear();
        self.pending_urls.clear();
        self.motion_ids = (0, 0);
        self.motion_origin = [0.0, 0.0];
    }

    pub(super) fn motion_layout(&self) -> &wgpu::BindGroupLayout {
        &self.motion_layout
    }

    /// GPU motion ids `(transform, opacity)` and the transform overlay pivot
    /// for the primitives pushed next.
    pub(super) fn set_motion(&mut self, ids: (u32, u32), origin: [f32; 2]) {
        self.motion_ids = ids;
        self.motion_origin = origin;
    }

    pub(super) fn invalidate_image_bindings(&mut self) {
        self.url_bind_groups.clear();
    }

    /// Drop target-local URL bindings after the shared cache publishes a new
    /// image. Bind groups capture a texture view, so retaining them would keep
    /// a target sampling the fallback or a previous resource generation.
    pub(super) fn invalidate_target_image_bindings(target: &mut Option<QuadPipelineTarget>) {
        if let Some(target) = target {
            target.url_bind_groups.clear();
        }
    }
    /// Drop bindings to URL textures `url_cache` no longer retains; run after
    /// the painter trims it.
    pub(super) fn finish_frame(&mut self, url_cache: &UrlTextureCache) {
        self.url_bind_groups.retain(|key, _| {
            key.as_deref()
                .is_none_or(|key| url_cache.contains_retained(key))
        });
    }

    pub(super) fn pending_len(&self) -> u32 {
        self.pending.len() as u32
    }

    pub(super) fn paint_buffer(&self) -> &wgpu::Buffer {
        &self.paint_buffer
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url_cache: &mut UrlTextureCache,
        bounds: LogicalRect,
        clip: LogicalRect,
        fragment_clip: super::clip::FragmentClip,
        affine: [f32; 6],
        persp: [f32; 2],
        background: Option<[f32; 4]>,
        border_color: Option<[f32; 4]>,
        border_width: f32,
        corner_radius: [f32; 4],
        shadow: Option<ComponentElevation>,
        opacity: f32,
        surface: &QuadSurfacePaint,
    ) -> Option<u32> {
        self.push_with_work(
            device,
            queue,
            url_cache,
            bounds,
            clip,
            fragment_clip,
            affine,
            persp,
            background,
            border_color,
            border_width,
            corner_radius,
            shadow,
            opacity,
            surface,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push_with_work(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url_cache: &mut UrlTextureCache,
        bounds: LogicalRect,
        clip: LogicalRect,
        fragment_clip: super::clip::FragmentClip,
        affine: [f32; 6],
        persp: [f32; 2],
        background: Option<[f32; 4]>,
        border_color: Option<[f32; 4]>,
        border_width: f32,
        corner_radius: [f32; 4],
        shadow: Option<ComponentElevation>,
        opacity: f32,
        surface: &QuadSurfacePaint,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> Option<u32> {
        if self.motion_ids.0 == 0 {
            let world = super::clip::transformed_aabb_projective(bounds, affine, persp);
            let _ = world.intersection(clip)?;
        } else {
            let _ = clip;
        }
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let translation =
            super::clip::is_translation_projective(affine, persp) && self.motion_ids.0 == 0;
        let (position, instance_affine, instance_persp, pixel_snap) = if translation {
            (
                [
                    super::clip::on_grid(bounds.x, affine[4], self.grid_scale),
                    super::clip::on_grid(bounds.y, affine[5], self.grid_scale),
                ],
                super::clip::IDENTITY_AFFINE,
                [0.0, 0.0],
                1u32,
            )
        } else {
            ([bounds.x, bounds.y], affine, persp, 0u32)
        };
        let snap =
            super::motion::pack_motion_snap(pixel_snap, self.motion_ids.0, self.motion_ids.1);
        let index = self.pending.len() as u32;
        let shadows = collect_shadows(shadow, surface);
        let split_shadows = shadows.len() > 1 || shadows.iter().any(|layer| layer.inset);
        let outsets = shadows
            .iter()
            .copied()
            .filter(|layer| !layer.inset)
            .collect::<Vec<_>>();
        let insets = shadows
            .iter()
            .copied()
            .filter(|layer| layer.inset)
            .collect::<Vec<_>>();
        let fill_shadow = if split_shadows {
            None
        } else {
            outsets.first().copied()
        };
        let (mut edge_widths, edge_colors) =
            resolved_instance_border(border_color, border_width, surface);
        let zero_widths = [0.0f32; 4];
        let zero_colors = [[0.0f32; 4]; 4];
        let image_scale = image_device_scale(affine, self.grid_scale);
        let mut layers = surface_paint_layers(surface);
        let content_layer = if surface.content_image.is_some() {
            layers.pop()
        } else {
            None
        };
        let border_tiles = prepare_border_image_tiles(
            device,
            queue,
            url_cache,
            surface.border_image.as_ref(),
            bounds.width,
            bounds.height,
            work,
        );
        if border_tiles.is_some() {
            edge_widths = zero_widths;
        }
        if split_shadows {
            for layer in outsets.iter().rev() {
                push_solid_instance(
                    &mut self.pending,
                    &mut self.pending_paint,
                    &mut self.pending_urls,
                    shadow_only_paint(false),
                    None,
                    position,
                    [bounds.width, bounds.height],
                    None,
                    zero_colors,
                    zero_widths,
                    corner_radius,
                    Some(*layer),
                    opacity,
                    snap,
                    self.motion_origin,
                    instance_affine,
                    instance_persp,
                    fragment_clip,
                );
            }
        }
        if layers.is_empty() {
            let paint = pack_shared(
                device,
                queue,
                url_cache,
                surface,
                bounds.width,
                bounds.height,
                image_scale,
                None,
                work,
            );
            let mask_url = packed_mask_url(&paint, surface);
            push_solid_instance(
                &mut self.pending,
                &mut self.pending_paint,
                &mut self.pending_urls,
                paint,
                mask_url,
                position,
                [bounds.width, bounds.height],
                background,
                edge_colors,
                edge_widths,
                corner_radius,
                fill_shadow,
                opacity,
                snap,
                self.motion_origin,
                instance_affine,
                instance_persp,
                fragment_clip,
            );
        } else {
            for (layer_index, layer) in layers.iter().enumerate() {
                let (paint, paint_url) = pack_layer(
                    device,
                    queue,
                    url_cache,
                    surface,
                    layer,
                    bounds.width,
                    bounds.height,
                    image_scale,
                    work,
                );
                let first = layer_index == 0;
                push_solid_instance(
                    &mut self.pending,
                    &mut self.pending_paint,
                    &mut self.pending_urls,
                    paint,
                    paint_url,
                    position,
                    [bounds.width, bounds.height],
                    if first { background } else { None },
                    if first { edge_colors } else { zero_colors },
                    if first { edge_widths } else { zero_widths },
                    corner_radius,
                    if first { fill_shadow } else { None },
                    opacity,
                    snap,
                    self.motion_origin,
                    instance_affine,
                    instance_persp,
                    fragment_clip,
                );
            }
        }
        if let Some((paint_url, tiles)) = border_tiles.as_ref() {
            for tile in tiles {
                let paint = QuadPaintData {
                    flags: PAINT_URL,
                    url_dest: url_dest_for_uv(tile.u0, tile.v0, tile.u1, tile.v1),
                    ..Default::default()
                };
                push_solid_instance(
                    &mut self.pending,
                    &mut self.pending_paint,
                    &mut self.pending_urls,
                    paint,
                    Some(paint_url.clone()),
                    [position[0] + tile.dest_x, position[1] + tile.dest_y],
                    [tile.dest_w, tile.dest_h],
                    None,
                    zero_colors,
                    zero_widths,
                    [0.0; 4],
                    None,
                    opacity,
                    snap,
                    self.motion_origin,
                    instance_affine,
                    instance_persp,
                    fragment_clip,
                );
            }
        }
        if let Some(layer) = content_layer {
            let (paint, paint_url) = pack_layer(
                device,
                queue,
                url_cache,
                surface,
                layer,
                bounds.width,
                bounds.height,
                image_scale,
                work,
            );
            push_solid_instance(
                &mut self.pending,
                &mut self.pending_paint,
                &mut self.pending_urls,
                paint,
                paint_url,
                position,
                [bounds.width, bounds.height],
                None,
                zero_colors,
                zero_widths,
                corner_radius,
                None,
                opacity,
                snap,
                self.motion_origin,
                instance_affine,
                instance_persp,
                fragment_clip,
            );
        }
        if split_shadows {
            for layer in insets.iter().rev() {
                push_solid_instance(
                    &mut self.pending,
                    &mut self.pending_paint,
                    &mut self.pending_urls,
                    shadow_only_paint(true),
                    None,
                    position,
                    [bounds.width, bounds.height],
                    None,
                    zero_colors,
                    zero_widths,
                    corner_radius,
                    Some(*layer),
                    opacity,
                    snap,
                    self.motion_origin,
                    instance_affine,
                    instance_persp,
                    fragment_clip,
                );
            }
        }
        Some(index)
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url_cache: &UrlTextureCache,
        physical_size: [u32; 2],
        scale_factor: f32,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let uniforms = Uniforms {
            transform: orthographic(physical_size[0], physical_size[1]),
            scale: scale_factor,
            _padding: [0.0; 3],
        };
        // A write has a fixed cost far above these bytes; the size rarely changes.
        if self.uploaded_uniforms != Some(uniforms) {
            let uniform_bytes = bytemuck::bytes_of(&uniforms);
            if let Some(work) = gpu_work {
                work.write_buffer(queue, &self.uniforms, 0, uniform_bytes);
            } else {
                queue.write_buffer(&self.uniforms, 0, uniform_bytes);
            }
            self.uploaded_uniforms = Some(uniforms);
        }
        if self.pending.is_empty() {
            return;
        }
        if self.pending.len() > self.instance_capacity {
            self.uploaded.clear();
            self.instance_capacity = self.pending.len().next_power_of_two();
            let size = (self.instance_capacity * std::mem::size_of::<SolidInstance>()) as u64;
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.instances,
                    size,
                    usage,
                    "nana-ui.scene.quad.instances",
                );
            } else {
                self.instances = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana-ui.scene.quad.instances"),
                    size,
                    usage,
                    mapped_at_creation: false,
                }))
            };
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        let paint_reallocated = self.pending_paint.len() > self.paint_capacity;
        if paint_reallocated {
            self.uploaded_paint.clear();
            self.url_bind_groups.clear();
            self.paint_capacity = self.pending_paint.len().next_power_of_two();
            let size = (self.paint_capacity * std::mem::size_of::<QuadPaintData>()) as u64;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.paint_buffer,
                    size,
                    usage,
                    "nana-ui.scene.quad.paint",
                );
            } else {
                self.paint_buffer =
                    ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("nana-ui.scene.quad.paint"),
                        size,
                        usage,
                        mapped_at_creation: false,
                    }))
            };
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.instances,
            bytemuck::cast_slice(&self.uploaded),
            bytemuck::cast_slice(&self.pending),
            gpu_work,
        );
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.paint_buffer,
            bytemuck::cast_slice(&self.uploaded_paint),
            bytemuck::cast_slice(&self.pending_paint),
            gpu_work,
        );
        self.uploaded.clone_from(&self.pending);
        self.uploaded_paint.clone_from(&self.pending_paint);
        self.rebuild_url_bind_groups(device, url_cache);
        if let Some(work) = gpu_work {
            work.record_batch_rebuild();
        }
    }

    fn rebuild_url_bind_groups(&mut self, device: &wgpu::Device, url_cache: &UrlTextureCache) {
        let mut unique: HashSet<_> = self.pending_urls.iter().cloned().collect();
        unique.insert(None);
        for url in unique {
            if self.url_bind_groups.contains_key(&url) {
                continue;
            }
            let bind_group = self.create_url_bind_group(device, url_cache, url.as_deref());
            self.url_bind_groups.insert(url, bind_group);
        }
    }

    fn create_url_bind_group(
        &self,
        device: &wgpu::Device,
        url_cache: &UrlTextureCache,
        url: Option<&str>,
    ) -> wgpu::BindGroup {
        let url_view = url
            .and_then(|key| url_cache.get(key))
            .and_then(|cached| cached.as_ref())
            .map(|cached| &cached.view)
            .unwrap_or(&self.url_view);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.quad.bind_group"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.paint_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(url_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.url_sampler),
                },
            ],
        })
    }

    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        range: std::ops::Range<u32>,
        scissor: PhysicalRect,
        sample_count: u32,
        motion: &wgpu::BindGroup,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if range.start >= range.end {
            return;
        }
        pass.set_pipeline(if sample_count > 1 {
            &self.pipeline_msaa
        } else {
            &self.pipeline
        });
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.set_bind_group(1, motion, &[]);
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        let start = range.start as usize;
        let end = range.end as usize;
        let mut index = start;
        while index < end {
            let url = self.pending_urls.get(index).cloned().unwrap_or(None);
            let batch_start = index;
            index += 1;
            while index < end && self.pending_urls.get(index).cloned().unwrap_or(None) == url {
                index += 1;
            }
            let bind_group = self
                .url_bind_groups
                .get(&url)
                .or_else(|| self.url_bind_groups.get(&None))
                .expect("quad url bind groups must be rebuilt in upload");
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..6, batch_start as u32..index as u32);
            if let Some(work) = gpu_work {
                work.record_draw_batch();
                work.record_draw_call();
            }
        }
    }
}

/// Device px per logical px along an image's own x and y axes: the scale
/// factor times the length of each transformed axis. A perspective term is
/// left out; it only foreshortens part of the quad.
fn image_device_scale(affine: [f32; 6], scale_factor: f32) -> [f32; 2] {
    let axis = |x: f32, y: f32| {
        let length = x.hypot(y) * scale_factor;
        if length.is_finite() && length > 0.0 {
            length
        } else {
            scale_factor
        }
    };
    [axis(affine[0], affine[1]), axis(affine[2], affine[3])]
}

fn collect_shadows(
    primary: Option<ComponentElevation>,
    surface: &QuadSurfacePaint,
) -> Vec<ComponentElevation> {
    let mut shadows = Vec::new();
    if let Some(shadow) = primary {
        shadows.push(shadow);
    }
    shadows.extend(surface.extra_shadows.iter().copied());
    shadows.truncate(nana_ui_core::MAX_BOX_SHADOWS);
    shadows
}

/// A blurred outer shadow thinner than its blur never reaches full strength:
/// a Gaussian over a strip `t` wide peaks at `erf(t / (√2·blur))`, as the
/// Path shadow fades it.
fn shadow_peak(size: [f32; 2], shadow: &ComponentElevation) -> f32 {
    if shadow.inset || shadow.blur_radius <= 0.0 {
        return 1.0;
    }
    let t = (size[0].min(size[1]) + shadow.spread_radius * 2.0).max(0.0);
    let x = t / (std::f32::consts::SQRT_2 * shadow.blur_radius);
    // erf(x) for x >= 0, to 5e-4 (Abramowitz and Stegun 7.1.27).
    1.0 - (1.0 + x * (0.278_393 + x * (0.230_389 + x * (0.000_972 + x * 0.078_108)))).powi(-4)
}

fn shadow_only_paint(inset: bool) -> QuadPaintData {
    let mut paint = QuadPaintData::default();
    if inset {
        paint.flags |= PAINT_SHADOW_INSET;
    }
    paint
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
fn push_solid_instance(
    pending: &mut Vec<SolidInstance>,
    pending_paint: &mut Vec<QuadPaintData>,
    pending_urls: &mut Vec<Option<String>>,
    mut paint: QuadPaintData,
    paint_url: Option<String>,
    position: [f32; 2],
    size: [f32; 2],
    background: Option<[f32; 4]>,
    border_colors: [[f32; 4]; 4],
    border_widths: [f32; 4],
    corner_radius: [f32; 4],
    shadow: Option<ComponentElevation>,
    opacity: f32,
    snap: u32,
    motion_origin: [f32; 2],
    instance_affine: [f32; 6],
    instance_persp: [f32; 2],
    fragment_clip: super::clip::FragmentClip,
) {
    paint.border_color_right = pack_linear(with_opacity(border_colors[1], opacity));
    paint.border_color_bottom = pack_linear(with_opacity(border_colors[2], opacity));
    paint.border_color_left = pack_linear(with_opacity(border_colors[3], opacity));
    paint._pad_tail0 = (snap >> 1) & 0x7fff;
    paint._pad_tail1 = snap >> 16;
    pending_paint.push(paint);
    pending_urls.push(paint_url);
    pending.push(SolidInstance {
        color: pack_linear(with_opacity(
            background.unwrap_or([0.0, 0.0, 0.0, 0.0]),
            opacity,
        )),
        position,
        size,
        border_color: pack_linear(with_opacity(border_colors[0], opacity)),
        border_radius: corner_radius,
        border_widths,
        shadow_color: pack_linear(with_opacity(
            shadow.map_or([0.0; 4], |shadow| {
                let [r, g, b, a] = shadow.color;
                [r, g, b, a * shadow_peak(size, &shadow)]
            }),
            opacity,
        )),
        shadow_offset: [
            shadow.map(|shadow| shadow.offset_x).unwrap_or(0.0),
            shadow.map(|shadow| shadow.offset_y).unwrap_or(0.0),
        ],
        shadow_blur_radius: shadow.map(|shadow| shadow.blur_radius).unwrap_or(0.0),
        // An inset spread shrinks the shape instead of growing it.
        shadow_spread_radius: shadow.map_or(0.0, |shadow| match paint.flags & PAINT_SHADOW_INSET {
            0 => shadow.spread_radius,
            _ => -shadow.spread_radius,
        }) + paint.outline_width.max(0.0),
        motion_origin,
        snap: snap & 0xffff | (paint.flags & PAINT_SHADOW_INSET) << 16,
        affine_abcd: [
            instance_affine[0],
            instance_affine[1],
            instance_affine[2],
            instance_affine[3],
        ],
        affine_ef: [
            instance_affine[4],
            instance_affine[5],
            instance_persp[0],
            instance_persp[1],
        ],
        clip_rect: fragment_clip.rect,
        clip_inv_abcd: fragment_clip.inv_abcd,
        clip_inv_ef: [
            fragment_clip.inv_ef[0],
            fragment_clip.inv_ef[1],
            fragment_clip.corner_radius,
        ],
    });
}

fn pack_border_styles(codes: [u8; 4]) -> u32 {
    (u32::from(codes[0]) & 3)
        | ((u32::from(codes[1]) & 3) << 2)
        | ((u32::from(codes[2]) & 3) << 4)
        | ((u32::from(codes[3]) & 3) << 6)
}

fn resolved_instance_border(
    border_color: Option<[f32; 4]>,
    border_width: f32,
    surface: &QuadSurfacePaint,
) -> ([f32; 4], [[f32; 4]; 4]) {
    let fallback = border_color.unwrap_or([0.0, 0.0, 0.0, 0.0]);
    if surface
        .border_widths
        .iter()
        .copied()
        .any(|width| width > 0.0)
    {
        let mut colors = surface.border_colors;
        for color in colors.iter_mut() {
            if color[3] <= 0.0 {
                *color = fallback;
            }
        }
        (surface.border_widths, colors)
    } else {
        ([border_width.max(0.0); 4], [fallback; 4])
    }
}

fn surface_paint_layers(surface: &QuadSurfacePaint) -> Vec<&BackgroundImage> {
    let mut layers = Vec::new();
    for layer in surface.background_layers.iter().rev() {
        layers.push(layer);
    }
    if let Some(image) = surface.background_image.as_ref() {
        layers.push(image);
    }
    let cap = if surface.content_image.is_some() {
        MAX_BACKGROUND_LAYERS.saturating_sub(1)
    } else {
        MAX_BACKGROUND_LAYERS
    };
    if layers.len() > cap {
        let skip = layers.len() - cap;
        layers.drain(..skip);
    }
    if let Some(image) = surface.content_image.as_ref() {
        layers.push(image);
    }
    layers
}

#[cfg(test)]
fn pack_paint(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &mut UrlTextureCache,
    surface: &QuadSurfacePaint,
    width: f32,
    height: f32,
) -> QuadPaintData {
    let layers = surface_paint_layers(surface);
    pack_shared(
        device,
        queue,
        cache,
        surface,
        width,
        height,
        [1.0, 1.0],
        layers.first().copied(),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn pack_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &mut UrlTextureCache,
    surface: &QuadSurfacePaint,
    layer: &BackgroundImage,
    width: f32,
    height: f32,
    image_scale: [f32; 2],
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> (QuadPaintData, Option<String>) {
    let paint = pack_shared(
        device,
        queue,
        cache,
        surface,
        width,
        height,
        image_scale,
        Some(layer),
        work,
    );
    let paint_url = if paint.flags & PAINT_URL != 0 {
        match layer {
            BackgroundImage::Url { url, sampling, .. } => {
                Some(cache_key(url, *sampling).into_owned())
            }
            BackgroundImage::Gradient(_) => None,
        }
    } else {
        packed_mask_url(&paint, surface)
    };
    (paint, paint_url)
}

fn packed_mask_url(paint: &QuadPaintData, surface: &QuadSurfacePaint) -> Option<String> {
    if paint.flags & PAINT_MASK_URL != 0 {
        surface
            .mask
            .as_ref()
            .and_then(|mask| mask.url_str().map(str::to_string))
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn pack_shared(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &mut UrlTextureCache,
    surface: &QuadSurfacePaint,
    width: f32,
    height: f32,
    image_scale: [f32; 2],
    layer: Option<&BackgroundImage>,
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> QuadPaintData {
    let mut paint = QuadPaintData::default();
    if let Some(BackgroundImage::Gradient(grad)) = layer {
        match grad {
            CssGradient::Linear(linear) => {
                paint.flags |= PAINT_GRADIENT;
                paint.grad_angle = linear.angle_deg;
                pack_gradient_stops(&mut paint, &linear.stops);
            }
            CssGradient::Radial(radial) => {
                if let Some(center) = radial.resolved_center(width, height) {
                    paint.flags |= PAINT_GRADIENT | PAINT_RADIAL;
                    paint.grad_center_x = center[0];
                    paint.grad_center_y = center[1];
                    paint.grad_radial_shape = if radial.circle { 0 } else { 1 };
                    pack_gradient_stops(&mut paint, &radial.stops);
                }
            }
        }
    }
    if let Some(mask) = surface.mask.as_ref() {
        match mask {
            MaskImage::Gradient(CssGradient::Linear(linear)) => {
                paint.flags |= PAINT_MASK;
                paint.mask_angle = linear.angle_deg;
                pack_mask_stops(&mut paint, &linear.stops);
            }
            MaskImage::Gradient(CssGradient::Radial(radial)) => {
                if let Some(center) = radial.resolved_center(width, height) {
                    paint.flags |= PAINT_MASK | PAINT_MASK_RADIAL;
                    paint.mask_center_x = center[0];
                    paint.mask_center_y = center[1];
                    paint.mask_radial_shape = if radial.circle { 0 } else { 1 };
                    pack_mask_stops(&mut paint, &radial.stops);
                }
            }
            MaskImage::Url(_) => {}
        }
    }
    if let Some(filter) = surface.filter
        && !filter.is_identity()
    {
        paint.flags |= PAINT_FILTER;
        paint.filter_b = filter.brightness;
        paint.filter_s = filter.saturate;
        paint.filter_c = filter.contrast;
        paint.filter_hue = filter.hue_rotate_deg;
        paint.filter_invert = filter.invert;
        paint.filter_opacity = filter.opacity;
    }
    if surface.outline_width > 0.0 {
        paint.outline_width = surface.outline_width;
        paint.outline_color = surface.outline_color.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    }
    paint.border_styles = pack_border_styles(surface.border_styles);
    if let Some(points) = surface.polygon_clip.as_ref()
        && points.len() >= 3
    {
        paint.flags |= PAINT_POLYGON;
        paint.polygon_count = points.len().min(8) as u32;
        let mut flat = [[0.0f32; 2]; 8];
        for (index, point) in points.iter().take(8).enumerate() {
            flat[index] = [point[0] / width.max(1.0), point[1] / height.max(1.0)];
        }
        paint.poly0 = [flat[0][0], flat[0][1], flat[1][0], flat[1][1]];
        paint.poly1 = [flat[2][0], flat[2][1], flat[3][0], flat[3][1]];
        paint.poly2 = [flat[4][0], flat[4][1], flat[5][0], flat[5][1]];
        paint.poly3 = [flat[6][0], flat[6][1], flat[7][0], flat[7][1]];
    }
    if let Some(BackgroundImage::Url {
        url,
        fit,
        size_width,
        size_height,
        position,
        repeat,
        sampling,
    }) = layer
        && let Some((tex_w, tex_h)) = cache.load_image(
            device,
            queue,
            url,
            *sampling,
            Demand::Layer(LayerDemand {
                fit: *fit,
                size_width: *size_width,
                size_height: *size_height,
                position: *position,
                repeat: *repeat,
                box_size: [width, height],
                scale: image_scale,
            }),
            work,
        )
        && let Some(bits) = repeat_bits(*repeat)
    {
        paint.flags |= PAINT_URL;
        paint.url_tex_index = bits;
        paint.url_fit = match fit {
            BackgroundImageFit::Cover => 0,
            BackgroundImageFit::Contain => 1,
            BackgroundImageFit::Stretch => 2,
            BackgroundImageFit::Auto => 3,
            BackgroundImageFit::Length => 4,
            BackgroundImageFit::ScaleDown => 5,
        };
        paint.url_dest = url_dest_rect(
            *fit,
            *size_width,
            *size_height,
            *position,
            *repeat,
            width,
            height,
            tex_w as f32,
            tex_h as f32,
        );
    }
    if paint.flags & PAINT_URL == 0
        && let Some(MaskImage::Url(url)) = surface.mask.as_ref()
        && cache.load_with_work(device, queue, url, work).is_some()
    {
        paint.flags |= PAINT_MASK | PAINT_MASK_URL;
    }
    paint
}

fn pack_stop_arrays(stops: &[nana_ui_core::GradientStop]) -> (u32, [[f32; 4]; 8], [f32; 8]) {
    let count = stops.len().min(8) as u32;
    let mut colors = [[0.0; 4]; 8];
    let mut positions = [0.0; 8];
    for (index, stop) in stops.iter().take(8).enumerate() {
        colors[index] = stop.color;
        positions[index] = stop.position;
    }
    (count, colors, positions)
}

fn pack_gradient_stops(paint: &mut QuadPaintData, stops: &[nana_ui_core::GradientStop]) {
    let (count, colors, positions) = pack_stop_arrays(stops);
    paint.grad_stop_count = count;
    paint.grad_stops0 = colors[0];
    paint.grad_stops1 = colors[1];
    paint.grad_stops2 = colors[2];
    paint.grad_stops3 = colors[3];
    paint.grad_stops4 = colors[4];
    paint.grad_stops5 = colors[5];
    paint.grad_stops6 = colors[6];
    paint.grad_stops7 = colors[7];
    paint.grad_pos = [positions[0], positions[1], positions[2], positions[3]];
    paint.grad_pos2 = [positions[4], positions[5], positions[6], positions[7]];
}

fn pack_mask_stops(paint: &mut QuadPaintData, stops: &[nana_ui_core::GradientStop]) {
    let (count, colors, positions) = pack_stop_arrays(stops);
    paint.mask_stop_count = count;
    paint.mask_stops0 = colors[0];
    paint.mask_stops1 = colors[1];
    paint.mask_stops2 = colors[2];
    paint.mask_stops3 = colors[3];
    paint.mask_stops4 = colors[4];
    paint.mask_stops5 = colors[5];
    paint.mask_stops6 = colors[6];
    paint.mask_stops7 = colors[7];
    paint.mask_pos = [positions[0], positions[1], positions[2], positions[3]];
    paint.mask_pos2 = [positions[4], positions[5], positions[6], positions[7]];
}

const BORDER_IMAGE_LINEAR_SIZE: u32 = 64;

fn prepare_border_image_tiles(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &mut UrlTextureCache,
    spec: Option<&BorderImageSpec>,
    box_w: f32,
    box_h: f32,
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> Option<(String, Vec<BorderImageTile>)> {
    let spec = spec.filter(|spec| spec.paints_linear_or_url())?;
    let (key, image_w, image_h) = match &spec.source {
        BackgroundImage::Url { url, .. } => {
            if url.is_empty() {
                return None;
            }
            let (tex_w, tex_h) = cache.load_with_work(device, queue, url, work)?;
            (url.clone(), tex_w as f32, tex_h as f32)
        }
        BackgroundImage::Gradient(CssGradient::Linear(linear)) => {
            let key = linear_gradient_cache_key(linear);
            if !cache.contains_key(&key) {
                let rgba = rasterize_linear_gradient(linear, BORDER_IMAGE_LINEAR_SIZE);
                insert_rgba_texture(
                    device,
                    queue,
                    cache,
                    &key,
                    BORDER_IMAGE_LINEAR_SIZE,
                    BORDER_IMAGE_LINEAR_SIZE,
                    &rgba,
                    work,
                );
            } else if cache.get(&key).is_some_and(Option::is_none) {
                return None;
            }
            (key, box_w, box_h)
        }
        BackgroundImage::Gradient(CssGradient::Radial(_)) => return None,
    };
    let tiles = spec.tiles(image_w, image_h, box_w, box_h);
    if tiles.is_empty() {
        None
    } else {
        Some((key, tiles))
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cache: &mut UrlTextureCache,
    key: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
    work: Option<&crate::gpu_work::GpuWorkSink>,
) {
    let texture =
        super::url_texture_cache::upload_with_work(device, queue, (width, height, rgba), work);
    cache.insert(key.to_string(), texture);
}

fn linear_gradient_cache_key(linear: &LinearGradient) -> String {
    let mut key = format!("nana:border-image-linear:{:.4}", linear.angle_deg);
    for stop in linear.stops.iter().take(8) {
        key.push_str(&format!(
            ":{:.4},{:.4},{:.4},{:.4},{:.4}",
            stop.position, stop.color[0], stop.color[1], stop.color[2], stop.color[3]
        ));
    }
    key
}

fn rasterize_linear_gradient(linear: &LinearGradient, size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let size_f = size as f32;
    for y in 0..size {
        for x in 0..size {
            let t = cpu_gradient_t(
                (x as f32 + 0.5) / size_f,
                (y as f32 + 0.5) / size_f,
                linear.angle_deg,
            );
            let color = cpu_sample_stops(t, &linear.stops);
            let i = ((y * size + x) * 4) as usize;
            rgba[i] = (color[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[i + 1] = (color[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[i + 2] = (color[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[i + 3] = (color[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    rgba
}

fn cpu_gradient_t(lx: f32, ly: f32, angle_deg: f32) -> f32 {
    let rad = angle_deg * 0.017453292;
    let axis_x = rad.sin();
    let axis_y = -rad.cos();
    let denom = axis_x.abs() + axis_y.abs();
    if denom <= 0.0001 {
        return 0.5;
    }
    ((lx - 0.5) * axis_x / denom + (ly - 0.5) * axis_y / denom + 0.5).clamp(0.0, 1.0)
}

fn cpu_sample_stops(t: f32, stops: &[GradientStop]) -> [f32; 4] {
    if stops.is_empty() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    if stops.len() == 1 || t <= stops[0].position {
        return stops[0].color;
    }
    if t >= stops[stops.len() - 1].position {
        return stops[stops.len() - 1].color;
    }
    for window in stops.windows(2) {
        let a = window[0];
        let b = window[1];
        if t >= a.position && t <= b.position {
            let mix = (t - a.position) / (b.position - a.position).max(0.0001);
            return [
                a.color[0] + (b.color[0] - a.color[0]) * mix,
                a.color[1] + (b.color[1] - a.color[1]) * mix,
                a.color[2] + (b.color[2] - a.color[2]) * mix,
                a.color[3] + (b.color[3] - a.color[3]) * mix,
            ];
        }
    }
    stops[0].color
}

fn url_dest_for_uv(u0: f32, v0: f32, u1: f32, v1: f32) -> [f32; 4] {
    let du = (u1 - u0).max(1.0e-6);
    let dv = (v1 - v0).max(1.0e-6);
    [-u0 / du, -v0 / dv, 1.0 / du, 1.0 / dv]
}

fn repeat_bits(repeat: BackgroundRepeat) -> Option<u32> {
    match repeat {
        BackgroundRepeat::NoRepeat => Some(0),
        BackgroundRepeat::Repeat | BackgroundRepeat::Round => Some(1 | 2),
        BackgroundRepeat::RepeatX | BackgroundRepeat::RoundX => Some(1),
        BackgroundRepeat::RepeatY | BackgroundRepeat::RoundY => Some(2),
        BackgroundRepeat::Unsupported => None,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
pub(super) fn url_dest_rect(
    fit: BackgroundImageFit,
    size_width: Option<LengthSpec>,
    size_height: Option<LengthSpec>,
    position: nana_ui_core::BackgroundPosition,
    repeat: BackgroundRepeat,
    box_w: f32,
    box_h: f32,
    tex_w: f32,
    tex_h: f32,
) -> [f32; 4] {
    let (mut drawn_w, mut drawn_h) = match fit {
        BackgroundImageFit::Stretch => (box_w, box_h),
        BackgroundImageFit::Cover => {
            let scale = (box_w / tex_w.max(1.0)).max(box_h / tex_h.max(1.0));
            (tex_w * scale, tex_h * scale)
        }
        BackgroundImageFit::Contain => {
            let scale = (box_w / tex_w.max(1.0)).min(box_h / tex_h.max(1.0));
            (tex_w * scale, tex_h * scale)
        }
        BackgroundImageFit::Auto => (tex_w, tex_h),
        BackgroundImageFit::ScaleDown => {
            let scale = (box_w / tex_w.max(1.0)).min(box_h / tex_h.max(1.0));
            if scale < 1.0 {
                (tex_w * scale, tex_h * scale)
            } else {
                (tex_w, tex_h)
            }
        }
        BackgroundImageFit::Length => {
            resolve_explicit_size(size_width, size_height, box_w, box_h, tex_w, tex_h)
        }
    };
    match repeat {
        BackgroundRepeat::Round | BackgroundRepeat::RoundX => {
            drawn_w = round_tile_len(box_w, drawn_w);
        }
        _ => {}
    }
    match repeat {
        BackgroundRepeat::Round | BackgroundRepeat::RoundY => {
            drawn_h = round_tile_len(box_h, drawn_h);
        }
        _ => {}
    }
    let origin_x = position_origin(position.x, box_w, drawn_w);
    let origin_y = position_origin(position.y, box_h, drawn_h);
    [
        origin_x / box_w.max(1.0),
        origin_y / box_h.max(1.0),
        drawn_w / box_w.max(1.0),
        drawn_h / box_h.max(1.0),
    ]
}

fn round_tile_len(box_len: f32, image_len: f32) -> f32 {
    if image_len <= 0.0 || box_len <= 0.0 {
        return image_len;
    }
    let n = (box_len / image_len).round().max(1.0);
    box_len / n
}

fn resolve_explicit_size(
    size_width: Option<LengthSpec>,
    size_height: Option<LengthSpec>,
    box_w: f32,
    box_h: f32,
    tex_w: f32,
    tex_h: f32,
) -> (f32, f32) {
    let width = size_width.and_then(|spec| resolve_size_len(spec, box_w));
    let height = size_height.and_then(|spec| resolve_size_len(spec, box_h));
    match (width, height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width * tex_h / tex_w.max(1.0)),
        (None, Some(height)) => (height * tex_w / tex_h.max(1.0), height),
        (None, None) => (tex_w, tex_h),
    }
}

fn resolve_size_len(spec: LengthSpec, box_len: f32) -> Option<f32> {
    match spec {
        LengthSpec::Auto | LengthSpec::Fill | LengthSpec::Shrink => None,
        other => other.resolve_px(Some(box_len)),
    }
}

fn position_origin(spec: LengthSpec, box_len: f32, image_len: f32) -> f32 {
    match spec {
        LengthSpec::Percent(percent) => (box_len - image_len) * (percent / 100.0),
        LengthSpec::Px(value) => value,
        other => other.resolve_px(Some(box_len)).unwrap_or(0.0),
    }
}

fn cached_solid_pipeline(
    policy: Option<&nana_gpu::GpuDeviceState>,
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    material: u64,
) -> wgpu::RenderPipeline {
    if let Some(policy) = policy {
        return nana_gpu::__framework::render_pipeline_state(
            policy,
            nana_gpu::PipelineKey {
                generation: policy.generation(),
                target_format: nana_gpu::__framework::format_from_wgpu(format),
                sample_count,
                shader: 0x7175_6164_736f_6c69 ^ super::corner_shape::cache_salt(),
                layout: quad_layout_key(),
                material,
                primitive: 0,
                blend: 1,
                depth: 0,
                vertex_layout: std::mem::size_of::<SolidInstance>() as u64,
            },
            || solid_pipeline(device, shader, layout, format, sample_count),
        )
        .expect("quad pipeline uses this context generation");
    }
    solid_pipeline(device, shader, layout, format, sample_count)
}

fn solid_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("nana-ui.scene.quad.solid.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("solid_vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SolidInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array!(
                    0 => Float32x4,
                    1 => Float32x2,
                    2 => Float32x2,
                    3 => Float32x4,
                    4 => Float32x4,
                    5 => Float32x4,
                    6 => Float32x4,
                    7 => Float32x2,
                    8 => Float32x2,
                    9 => Float32x2,
                    10 => Uint32,
                    11 => Float32x4,
                    12 => Float32x4,
                    13 => Float32x4,
                    14 => Float32x4,
                    15 => Float32x3,
                ),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("solid_fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Cw,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
fn quad_paint_test_device() -> (wgpu::Device, wgpu::Queue) {
    crate::test_gpu::device()
}

#[cfg(test)]
#[test]
fn stable_frames_reuse_bind_groups_and_storage_growth_rebinds() {
    let (device, queue) = quad_paint_test_device();
    let mut pipeline = QuadPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let url_cache = UrlTextureCache::default();
    let initial = pipeline.url_bind_groups.get(&None).unwrap().clone();
    for _ in 0..3 {
        pipeline.begin_frame(1.0);
        pipeline.pending.push(SolidInstance::zeroed());
        pipeline.pending_paint.push(QuadPaintData::zeroed());
        pipeline.pending_urls.push(None);
        pipeline.upload(&device, &queue, &url_cache, [64, 64], 1.0, None);
        assert_eq!(pipeline.url_bind_groups.get(&None), Some(&initial));
    }
    pipeline.begin_frame(1.0);
    let count = pipeline.paint_capacity + 1;
    pipeline.pending.resize(count, SolidInstance::zeroed());
    pipeline
        .pending_paint
        .resize(count, QuadPaintData::zeroed());
    pipeline.pending_urls.resize(count, None);
    pipeline.upload(&device, &queue, &url_cache, [64, 64], 1.0, None);
    assert_ne!(pipeline.url_bind_groups.get(&None), Some(&initial));
}

#[cfg(test)]
#[test]
fn pack_paint_sets_mask_flag() {
    use nana_ui_core::{CssGradient, GradientStop, LinearGradient};
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        mask: Some(MaskImage::Gradient(CssGradient::Linear(LinearGradient {
            angle_deg: 90.0,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                GradientStop {
                    position: 1.0,
                    color: [1.0, 1.0, 1.0, 0.0],
                },
            ],
        }))),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_ne!(paint.flags & PAINT_MASK, 0, "flags={}", paint.flags);
    assert_eq!(paint.mask_stop_count, 2);
}

#[cfg(test)]
#[test]
fn pack_paint_resolves_radial_mask_px_center_against_used_box() {
    use nana_ui_core::{CssGradient, GradientStop, LengthSpec, RadialGradient};
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        mask: Some(MaskImage::Gradient(CssGradient::Radial(RadialGradient {
            circle: true,
            center: [LengthSpec::Px(10.0), LengthSpec::Px(20.0)],
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
                GradientStop {
                    position: 1.0,
                    color: [1.0, 1.0, 1.0, 0.0],
                },
            ],
        }))),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        200.0,
        100.0,
    );
    assert_ne!(paint.flags & PAINT_MASK_RADIAL, 0, "flags={}", paint.flags);
    assert!((paint.mask_center_x - 0.05).abs() < 1e-5);
    assert!((paint.mask_center_y - 0.20).abs() < 1e-5);
    let missing = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        0.0,
        100.0,
    );
    assert_eq!(missing.flags & PAINT_MASK, 0, "zero width must fail closed");
}

#[cfg(test)]
#[test]
fn pack_paint_sets_hue_rotate() {
    use nana_ui_core::ColorFilter;
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        filter: Some(ColorFilter {
            hue_rotate_deg: 90.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_ne!(paint.flags & PAINT_FILTER, 0, "flags={}", paint.flags);
    assert!((paint.filter_hue - 90.0).abs() < 0.01);
}

#[cfg(test)]
#[test]
fn pack_paint_sets_invert_and_opacity() {
    use nana_ui_core::ColorFilter;
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        filter: Some(ColorFilter {
            invert: 1.0,
            opacity: 0.5,
            ..Default::default()
        }),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_ne!(paint.flags & PAINT_FILTER, 0);
    assert!((paint.filter_invert - 1.0).abs() < 0.01);
    assert!((paint.filter_opacity - 0.5).abs() < 0.01);
}

#[cfg(test)]
#[test]
fn pack_paint_packs_dashed_border_styles() {
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        border_styles: [1, 2, 1, 0],
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_eq!(paint.border_styles, 1 | (2 << 2) | (1 << 4));
}

#[cfg(test)]
#[test]
fn url_dest_for_uv_maps_source_slice() {
    let dest = url_dest_for_uv(0.25, 0.0, 0.75, 0.5);
    let uv0_x = (0.0 - dest[0]) / dest[2];
    let uv1_x = (1.0 - dest[0]) / dest[2];
    let uv0_y = (0.0 - dest[1]) / dest[3];
    let uv1_y = (1.0 - dest[1]) / dest[3];
    assert!((uv0_x - 0.25).abs() < 1.0e-5);
    assert!((uv1_x - 0.75).abs() < 1.0e-5);
    assert!((uv0_y - 0.0).abs() < 1.0e-5);
    assert!((uv1_y - 0.5).abs() < 1.0e-5);
}

#[cfg(test)]
#[test]
fn pack_paint_sets_mask_url_flag_from_png_alpha() {
    use nana_ui_core::MaskImage;
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let url = alpha_split_png_data_url();
    let surface = QuadSurfacePaint {
        mask: Some(MaskImage::Url(url)),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_ne!(paint.flags & PAINT_MASK, 0, "flags={}", paint.flags);
    assert_ne!(paint.flags & PAINT_MASK_URL, 0, "flags={}", paint.flags);
}

#[cfg(test)]
#[test]
fn pack_paint_ignores_unloadable_mask_url() {
    use nana_ui_core::MaskImage;
    use nana_ui_scene::QuadSurfacePaint;

    let (device, queue) = quad_paint_test_device();
    let surface = QuadSurfacePaint {
        mask: Some(MaskImage::Url("nana-missing-mask-image-xyz.png".into())),
        ..Default::default()
    };
    let paint = pack_paint(
        &device,
        &queue,
        &mut UrlTextureCache::default(),
        &surface,
        64.0,
        64.0,
    );
    assert_eq!(
        paint.flags & PAINT_MASK,
        0,
        "unloadable url must not fake a mask"
    );
    assert_eq!(paint.flags & PAINT_MASK_URL, 0);
}

#[cfg(test)]
fn alpha_split_png_data_url() -> String {
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

pub(super) struct QuadPipelineTarget {
    uniforms: wgpu::Buffer,
    uploaded_uniforms: Option<Uniforms>,
    paint_buffer: ManagedBuffer,
    paint_capacity: usize,
    url_bind_groups: HashMap<Option<String>, wgpu::BindGroup>,
    instances: ManagedBuffer,
    instance_capacity: usize,
    pending: Vec<SolidInstance>,
    uploaded: Vec<SolidInstance>,
    pending_paint: Vec<QuadPaintData>,
    uploaded_paint: Vec<QuadPaintData>,
    pending_urls: Vec<Option<String>>,
}

impl QuadPipeline {
    pub(super) fn swap_target(
        &mut self,
        target: &mut Option<QuadPipelineTarget>,
        device: &wgpu::Device,
    ) {
        let target = target.get_or_insert_with(|| QuadPipelineTarget {
            uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.quad.uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            uploaded_uniforms: None,
            paint_buffer: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.quad.paint"),
                size: (INITIAL_INSTANCES * std::mem::size_of::<QuadPaintData>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            paint_capacity: INITIAL_INSTANCES,
            instances: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.quad.instances"),
                size: (INITIAL_INSTANCES * std::mem::size_of::<SolidInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            instance_capacity: INITIAL_INSTANCES,
            url_bind_groups: HashMap::new(),
            pending: Vec::new(),
            uploaded: Vec::new(),
            pending_paint: Vec::new(),
            uploaded_paint: Vec::new(),
            pending_urls: Vec::new(),
        });
        std::mem::swap(&mut self.uniforms, &mut target.uniforms);
        std::mem::swap(&mut self.uploaded_uniforms, &mut target.uploaded_uniforms);
        std::mem::swap(&mut self.paint_buffer, &mut target.paint_buffer);
        std::mem::swap(&mut self.paint_capacity, &mut target.paint_capacity);
        std::mem::swap(&mut self.url_bind_groups, &mut target.url_bind_groups);
        std::mem::swap(&mut self.instances, &mut target.instances);
        std::mem::swap(&mut self.instance_capacity, &mut target.instance_capacity);
        std::mem::swap(&mut self.pending, &mut target.pending);
        std::mem::swap(&mut self.uploaded, &mut target.uploaded);
        std::mem::swap(&mut self.pending_paint, &mut target.pending_paint);
        std::mem::swap(&mut self.uploaded_paint, &mut target.uploaded_paint);
        std::mem::swap(&mut self.pending_urls, &mut target.pending_urls);
    }
}
