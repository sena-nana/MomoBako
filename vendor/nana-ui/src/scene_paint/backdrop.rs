//! Per-node CSS backdrop-filter: sample dest.color, separable blur, composite.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use nana_ui_core::BackdropFilter;

use super::clip::FragmentClip;
use crate::gpu_work::ManagedBuffer;

fn cached_fullscreen_pipeline(
    policy: &nana_gpu::GpuDeviceState,
    format: wgpu::TextureFormat,
    material: u64,
    create: impl FnOnce() -> wgpu::RenderPipeline,
) -> wgpu::RenderPipeline {
    nana_gpu::__framework::render_pipeline_state(
        policy,
        nana_gpu::PipelineKey {
            generation: policy.generation(),
            target_format: nana_gpu::__framework::format_from_wgpu(format),
            sample_count: 1,
            shader: 0x6261_636b_6472_6f70,
            layout: 8,
            material,
            primitive: 0,
            blend: 1,
            depth: 0,
            vertex_layout: 0,
        },
        create,
    )
    .expect("backdrop pipeline uses this context generation")
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CopyUniforms {
    src_origin: [f32; 2],
    src_size: [f32; 2],
    dest_size: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BlurUniforms {
    direction: [f32; 2],
    radius: f32,
    _pad0: f32,
    texel_size: [f32; 2],
    region_origin: [f32; 2],
    region_size: [f32; 2],
    dest_size: [f32; 2],
}

fn pack_clip_polygon(clip: &FragmentClip) -> [[f32; 4]; 4] {
    let mut packed = [[0.0; 4]; 4];
    for index in 0..clip.polygon_count.min(8) as usize {
        let slot = index / 2;
        let component = (index % 2) * 2;
        packed[slot][component] = clip.polygon[index][0];
        packed[slot][component + 1] = clip.polygon[index][1];
    }
    packed
}

const BLUR_UNIFORM_SLOT_SIZE: u64 = 48;
const UNIFORM_STRIDE: u64 = 256;
const PASSES_PER_FROST: u64 = 4;
const INITIAL_FROST_SLOTS: u64 = 64;

const PASS_COPY: u64 = 0;
const PASS_BLUR_H: u64 = 1;
const PASS_BLUR_V: u64 = 2;
const PASS_COMPOSITE: u64 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CompositeUniforms {
    quad_origin: [f32; 2],
    quad_size: [f32; 2],
    corner_radius: [f32; 4],
    padded_origin: [f32; 2],
    padded_size: [f32; 2],
    dest_size: [f32; 2],
    saturate: f32,
    clip_corner_radius: f32,
    clip_rect: [f32; 4],
    clip_inv_abcd: [f32; 4],
    clip_inv_ef: [f32; 2],
    clip_polygon_count: u32,
    _pad_poly: u32,
    clip_poly0: [f32; 4],
    clip_poly1: [f32; 4],
    clip_poly2: [f32; 4],
    clip_poly3: [f32; 4],
    quad_logical_origin: [f32; 2],
    quad_logical_size: [f32; 2],
    quad_abcd: [f32; 4],
    quad_ef: [f32; 2],
    paint_index: u32,
    _pad_end: u32,
}

const COMPOSITE_UNIFORM_SIZE: usize = 224;
const _: () = assert!(std::mem::size_of::<CompositeUniforms>() == COMPOSITE_UNIFORM_SIZE);

#[derive(Clone, Copy)]
pub(super) struct BackdropRequest {
    pub uniform_slot: u32,
    pub paint_index: u32,
    pub physical_bounds: [f32; 4],
    pub corner_radius: [f32; 4],
    pub blur_radius: f32,
    pub saturate: f32,
    pub clip: FragmentClip,
    pub padded_origin: [f32; 2],
    pub padded_size: [u32; 2],
    pub quad_logical_origin: [f32; 2],
    pub quad_logical_size: [f32; 2],
    pub quad_abcd: [f32; 4],
    pub quad_ef: [f32; 2],
}

struct PingPong {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub(super) struct BackdropPipeline {
    ping: Option<PingPong>,
    pong: Option<PingPong>,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    copy_pipeline: wgpu::RenderPipeline,
    copy_bind_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::RenderPipeline,
    blur_bind_layout: wgpu::BindGroupLayout,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_layout: wgpu::BindGroupLayout,
    uniform_slab: ManagedBuffer,
    uniform_slab_passes: u64,
    pending: Vec<BackdropRequest>,
}

impl BackdropPipeline {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        policy: &nana_gpu::GpuDeviceState,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nana-ui.scene.backdrop.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let copy_bind_layout = Self::texture_uniform_layout(
            device,
            "copy",
            NonZeroU64::new(std::mem::size_of::<CopyUniforms>() as u64),
            Some(policy),
        );
        let blur_bind_layout = Self::texture_uniform_layout(
            device,
            "blur",
            NonZeroU64::new(BLUR_UNIFORM_SLOT_SIZE),
            Some(policy),
        );
        let composite_bind_layout = if policy.generation().get() != 0 {
            let fragment = &[nana_gpu::ShaderStage::Fragment];
            let vertex_fragment = &[
                nana_gpu::ShaderStage::Vertex,
                nana_gpu::ShaderStage::Fragment,
            ];
            let table = nana_gpu::ResourceTable::new(vec![
                nana_gpu::LogicalBinding::new(
                    0,
                    nana_gpu::LogicalBindingType::SampledTexture,
                    fragment,
                ),
                nana_gpu::LogicalBinding::new(1, nana_gpu::LogicalBindingType::Sampler, fragment),
                nana_gpu::LogicalBinding::new(
                    2,
                    nana_gpu::LogicalBindingType::UniformBuffer,
                    vertex_fragment,
                ),
                nana_gpu::LogicalBinding::new(
                    3,
                    nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                    fragment,
                ),
            ])
            .expect("backdrop composite logical table is valid")
            .for_generation(policy.generation());
            let logical =
                nana_gpu::__framework::logical_layout(device, policy.generation(), &table)
                    .expect("backdrop composite logical layout matches the device");
            nana_gpu::__framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.backdrop.composite.layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            })
        };
        let copy_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.backdrop.copy.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader/backdrop_copy.wgsl"
            ))),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.backdrop.blur.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader/backdrop_blur.wgsl"
            ))),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.backdrop.composite.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(concat!(
                include_str!("shader/color.wgsl"),
                "\n",
                include_str!("shader/quad_paint_data.wgsl"),
                "\n",
                include_str!("shader/backdrop_composite.wgsl"),
            ))),
        });
        let copy_pipeline = cached_fullscreen_pipeline(policy, format, 1, || {
            Self::fullscreen_pipeline(
                device,
                &copy_shader,
                &copy_bind_layout,
                format,
                "nana-ui.scene.backdrop.copy",
            )
        });
        let blur_pipeline = cached_fullscreen_pipeline(policy, format, 2, || {
            Self::fullscreen_pipeline(
                device,
                &blur_shader,
                &blur_bind_layout,
                format,
                "nana-ui.scene.backdrop.blur",
            )
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nana-ui.scene.backdrop.composite.pipeline"),
                bind_group_layouts: &[Some(&composite_bind_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = cached_fullscreen_pipeline(policy, format, 3, || {
            wgpu::Device::create_render_pipeline(
                device,
                &wgpu::RenderPipelineDescriptor {
                    label: Some("nana-ui.scene.backdrop.composite.pipeline"),
                    layout: Some(&composite_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &composite_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &composite_shader,
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
                },
            )
        });
        let uniform_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let initial_passes = INITIAL_FROST_SLOTS * PASSES_PER_FROST;
        Self {
            ping: None,
            pong: None,
            width: 0,
            height: 0,
            format,
            sampler,
            copy_pipeline,
            copy_bind_layout,
            blur_pipeline,
            blur_bind_layout,
            composite_pipeline,
            composite_bind_layout,
            uniform_slab: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.backdrop.uniforms"),
                size: initial_passes * UNIFORM_STRIDE,
                usage: uniform_usage,
                mapped_at_creation: false,
            })),
            uniform_slab_passes: initial_passes,
            pending: Vec::new(),
        }
    }

    fn uniform_offset(slot: u32, pass: u64) -> u64 {
        slot as u64 * PASSES_PER_FROST * UNIFORM_STRIDE + pass * UNIFORM_STRIDE
    }

    fn uniform_binding(
        buffer: &wgpu::Buffer,
        slot: u32,
        pass: u64,
        size: u64,
    ) -> wgpu::BufferBinding<'_> {
        wgpu::BufferBinding {
            buffer,
            offset: Self::uniform_offset(slot, pass),
            size: NonZeroU64::new(size),
        }
    }

    fn ensure_uniform_slab(
        &mut self,
        device: &wgpu::Device,
        frost_count: u64,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let needed_passes = frost_count * PASSES_PER_FROST;
        if needed_passes <= self.uniform_slab_passes {
            return;
        }
        let new_passes = needed_passes.max(self.uniform_slab_passes * 2);
        let size = new_passes * UNIFORM_STRIDE;
        let usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        if let Some(work) = gpu_work {
            work.replace_buffer(
                device,
                &mut self.uniform_slab,
                size,
                usage,
                "nana-ui.scene.backdrop.uniforms",
            );
        } else {
            self.uniform_slab = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.backdrop.uniforms"),
                size,
                usage,
                mapped_at_creation: false,
            }));
        }
        self.uniform_slab_passes = new_passes;
    }

    fn texture_uniform_layout(
        device: &wgpu::Device,
        label: &str,
        min_binding_size: Option<NonZeroU64>,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> wgpu::BindGroupLayout {
        if let Some(policy) = policy {
            let stages = &[
                nana_gpu::ShaderStage::Vertex,
                nana_gpu::ShaderStage::Fragment,
            ];
            let table = nana_gpu::ResourceTable::new(vec![
                nana_gpu::LogicalBinding::new(
                    0,
                    nana_gpu::LogicalBindingType::SampledTexture,
                    stages,
                ),
                nana_gpu::LogicalBinding::new(1, nana_gpu::LogicalBindingType::Sampler, stages),
                nana_gpu::LogicalBinding::new(
                    2,
                    nana_gpu::LogicalBindingType::UniformBuffer,
                    stages,
                )
                .min_size(min_binding_size.map_or(0, NonZeroU64::get)),
            ])
            .expect("backdrop logical table is valid")
            .for_generation(policy.generation());
            let logical =
                nana_gpu::__framework::logical_layout(device, policy.generation(), &table)
                    .expect("backdrop logical layout matches the device");
            return nana_gpu::__framework::resource_layout(&logical).clone();
        }
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("nana-ui.scene.backdrop.{label}.layout")),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size,
                    },
                    count: None,
                },
            ],
        })
    }

    fn fullscreen_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        bind_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> wgpu::RenderPipeline {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{label}.pipeline")),
            bind_group_layouts: &[Some(bind_layout)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label}.pipeline")),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    fn ensure_textures(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.width == width && self.height == height && self.ping.is_some() {
            return;
        }
        self.width = width.max(1);
        self.height = height.max(1);
        self.ping = Some(Self::make_texture(
            device,
            self.format,
            self.width,
            self.height,
            "ping",
        ));
        self.pong = Some(Self::make_texture(
            device,
            self.format,
            self.width,
            self.height,
            "pong",
        ));
    }

    fn make_texture(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        label: &str,
    ) -> PingPong {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("nana-ui.scene.backdrop.{label}")),
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
        let view = texture.create_view(&Default::default());
        PingPong {
            _texture: texture,
            view,
        }
    }

    pub(super) fn begin_frame(&mut self) {
        self.pending.clear();
    }

    pub(super) fn needs_backdrop(&self) -> bool {
        !self.pending.is_empty()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn push(
        &mut self,
        paint_index: u32,
        physical_bounds: [f32; 4],
        corner_radius: [f32; 4],
        filter: BackdropFilter,
        clip: FragmentClip,
        scale: f32,
        dest_physical: [u32; 2],
        logical_bounds: super::clip::LogicalRect,
        affine: [f32; 6],
    ) -> u32 {
        let blur_physical = filter.blur_radius * scale;
        let pad = blur_physical.ceil() as u32 + 2;
        let x = physical_bounds[0].floor().max(0.0) as u32;
        let y = physical_bounds[1].floor().max(0.0) as u32;
        let w = physical_bounds[2].ceil().max(1.0) as u32;
        let h = physical_bounds[3].ceil().max(1.0) as u32;
        let padded_x = x.saturating_sub(pad);
        let padded_y = y.saturating_sub(pad);
        let padded_w = (w + pad * 2)
            .min(dest_physical[0].saturating_sub(padded_x))
            .max(1);
        let padded_h = (h + pad * 2)
            .min(dest_physical[1].saturating_sub(padded_y))
            .max(1);
        let index = self.pending.len() as u32;
        self.pending.push(BackdropRequest {
            uniform_slot: index,
            paint_index,
            physical_bounds,
            corner_radius,
            blur_radius: blur_physical,
            saturate: filter.saturate,
            clip,
            padded_origin: [padded_x as f32, padded_y as f32],
            padded_size: [padded_w, padded_h],
            quad_logical_origin: [logical_bounds.x * scale, logical_bounds.y * scale],
            quad_logical_size: [logical_bounds.width * scale, logical_bounds.height * scale],
            quad_abcd: [affine[0], affine[1], affine[2], affine[3]],
            quad_ef: [affine[4] * scale, affine[5] * scale],
        });
        index
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dest_physical: [u32; 2],
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let count = self.pending.len();
        if count == 0 {
            return;
        }
        self.ensure_uniform_slab(device, count as u64, gpu_work);
        let texel = [
            1.0 / dest_physical[0].max(1) as f32,
            1.0 / dest_physical[1].max(1) as f32,
        ];
        let dest_size = [dest_physical[0] as f32, dest_physical[1] as f32];
        for request in &self.pending {
            let slot = request.uniform_slot;
            let copy_uniforms = CopyUniforms {
                src_origin: request.padded_origin,
                src_size: [request.padded_size[0] as f32, request.padded_size[1] as f32],
                dest_size,
                _pad: [0.0, 0.0],
            };
            let copy_offset = Self::uniform_offset(slot, PASS_COPY);
            let copy_bytes = bytemuck::bytes_of(&copy_uniforms);
            if let Some(work) = gpu_work {
                work.write_buffer(queue, &self.uniform_slab, copy_offset, copy_bytes);
            } else {
                queue.write_buffer(&self.uniform_slab, copy_offset, copy_bytes);
            }

            let region_origin = request.padded_origin;
            let region_size = [request.padded_size[0] as f32, request.padded_size[1] as f32];
            for (pass, direction) in [(PASS_BLUR_H, [1.0, 0.0]), (PASS_BLUR_V, [0.0, 1.0])] {
                let blur_uniforms = BlurUniforms {
                    direction,
                    radius: request.blur_radius,
                    _pad0: 0.0,
                    texel_size: texel,
                    region_origin,
                    region_size,
                    dest_size,
                };
                let blur_offset = Self::uniform_offset(slot, pass);
                let blur_bytes = bytemuck::bytes_of(&blur_uniforms);
                if let Some(work) = gpu_work {
                    work.write_buffer(queue, &self.uniform_slab, blur_offset, blur_bytes);
                } else {
                    queue.write_buffer(&self.uniform_slab, blur_offset, blur_bytes);
                }
            }

            let polys = pack_clip_polygon(&request.clip);
            let composite_uniforms = CompositeUniforms {
                quad_origin: [request.physical_bounds[0], request.physical_bounds[1]],
                quad_size: [request.physical_bounds[2], request.physical_bounds[3]],
                corner_radius: request.corner_radius,
                padded_origin: request.padded_origin,
                padded_size: [request.padded_size[0] as f32, request.padded_size[1] as f32],
                dest_size,
                saturate: request.saturate,
                clip_corner_radius: request.clip.corner_radius,
                clip_rect: request.clip.rect,
                clip_inv_abcd: request.clip.inv_abcd,
                clip_inv_ef: request.clip.inv_ef,
                clip_polygon_count: request.clip.polygon_count as u32,
                _pad_poly: 0,
                clip_poly0: polys[0],
                clip_poly1: polys[1],
                clip_poly2: polys[2],
                clip_poly3: polys[3],
                quad_logical_origin: request.quad_logical_origin,
                quad_logical_size: request.quad_logical_size,
                quad_abcd: request.quad_abcd,
                quad_ef: request.quad_ef,
                paint_index: request.paint_index,
                _pad_end: 0,
            };
            let composite_offset = Self::uniform_offset(slot, PASS_COMPOSITE);
            let composite_bytes = bytemuck::bytes_of(&composite_uniforms);
            if let Some(work) = gpu_work {
                work.write_buffer(queue, &self.uniform_slab, composite_offset, composite_bytes);
            } else {
                queue.write_buffer(&self.uniform_slab, composite_offset, composite_bytes);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        dest_view: &wgpu::TextureView,
        dest_physical: [u32; 2],
        target_view: &wgpu::TextureView,
        request: &BackdropRequest,
        paint_buffer: &wgpu::Buffer,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
        backdrop_passes: &mut u32,
    ) {
        self.ensure_textures(device, dest_physical[0], dest_physical[1]);
        let ping = self.ping.as_ref().expect("backdrop ping");
        let pong = self.pong.as_ref().expect("backdrop pong");
        let slot = request.uniform_slot;
        let copy_binding = Self::uniform_binding(
            &self.uniform_slab,
            slot,
            PASS_COPY,
            std::mem::size_of::<CopyUniforms>() as u64,
        );
        let copy_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.backdrop.copy.bind"),
            layout: &self.copy_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(dest_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(copy_binding),
                },
            ],
        });
        *backdrop_passes = backdrop_passes.saturating_add(1);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui.scene.backdrop.copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &ping.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(
                request.padded_origin[0],
                request.padded_origin[1],
                request.padded_size[0] as f32,
                request.padded_size[1] as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.copy_pipeline);
            pass.set_bind_group(0, &copy_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        for (pass, src, dst) in [
            (PASS_BLUR_H, &ping.view, &pong.view),
            (PASS_BLUR_V, &pong.view, &ping.view),
        ] {
            let blur_binding =
                Self::uniform_binding(&self.uniform_slab, slot, pass, BLUR_UNIFORM_SLOT_SIZE);
            let blur_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nana-ui.scene.backdrop.blur.bind"),
                layout: &self.blur_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(blur_binding),
                    },
                ],
            });
            *backdrop_passes = backdrop_passes.saturating_add(1);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui.scene.backdrop.blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(
                request.padded_origin[0],
                request.padded_origin[1],
                request.padded_size[0] as f32,
                request.padded_size[1] as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &blur_bind, &[]);
            pass.draw(0..3, 0..1);
        }

        let composite_binding = Self::uniform_binding(
            &self.uniform_slab,
            slot,
            PASS_COMPOSITE,
            COMPOSITE_UNIFORM_SIZE as u64,
        );
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.backdrop.composite.bind"),
            layout: &self.composite_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&ping.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(composite_binding),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: paint_buffer.as_entire_binding(),
                },
            ],
        });
        *backdrop_passes = backdrop_passes.saturating_add(1);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nana-ui.scene.backdrop.composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            dest_physical[0].max(1) as f32,
            dest_physical[1].max(1) as f32,
            0.0,
            1.0,
        );
        // Every pixel the panel touches: the outer half of its edge ramp lies
        // in the partly covered ones.
        let [x, y, width, height] = request.physical_bounds;
        let [target_w, target_h] = dest_physical.map(|edge| edge.max(1) as f32);
        let left = x.floor().clamp(0.0, target_w - 1.0);
        let top = y.floor().clamp(0.0, target_h - 1.0);
        let right = (x + width).ceil().min(target_w).max(left + 1.0);
        let bottom = (y + height).ceil().min(target_h).max(top + 1.0);
        pass.set_scissor_rect(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        );
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &composite_bind, &[]);
        pass.draw(0..6, 0..1);
        drop(pass);

        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }

    pub(super) fn request(&self, index: u32) -> Option<&BackdropRequest> {
        self.pending.get(index as usize)
    }
}

pub(super) struct BackdropPipelineTarget {
    ping: Option<PingPong>,
    pong: Option<PingPong>,
    width: u32,
    height: u32,
    uniform_slab: ManagedBuffer,
    uniform_slab_passes: u64,
    pending: Vec<BackdropRequest>,
}

impl BackdropPipeline {
    pub(super) fn swap_target(
        &mut self,
        target: &mut Option<BackdropPipelineTarget>,
        device: &wgpu::Device,
    ) {
        let target = target.get_or_insert_with(|| BackdropPipelineTarget {
            ping: None,
            pong: None,
            width: 0,
            height: 0,
            uniform_slab: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.backdrop.uniforms"),
                size: INITIAL_FROST_SLOTS * PASSES_PER_FROST * UNIFORM_STRIDE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            uniform_slab_passes: INITIAL_FROST_SLOTS * PASSES_PER_FROST,
            pending: Vec::new(),
        });
        std::mem::swap(&mut self.ping, &mut target.ping);
        std::mem::swap(&mut self.pong, &mut target.pong);
        std::mem::swap(&mut self.width, &mut target.width);
        std::mem::swap(&mut self.height, &mut target.height);
        std::mem::swap(&mut self.uniform_slab, &mut target.uniform_slab);
        std::mem::swap(
            &mut self.uniform_slab_passes,
            &mut target.uniform_slab_passes,
        );
        std::mem::swap(&mut self.pending, &mut target.pending);
    }
}
