//! Dest-local color target; MSAA is only for frames without GPU nodes.

use std::num::NonZeroU64;

use nana_gpu::{LogicalBinding, LogicalBindingType, ResourceTable, ShaderStage};

use super::AlphaEncoding;
use super::clip::FragmentClip;
use crate::gpu_work::ManagedBuffer;

fn cached_dest_pipeline(
    policy: &nana_gpu::GpuDeviceState,
    format: wgpu::TextureFormat,
    material: u64,
    layout_key: u64,
    create: impl FnOnce() -> wgpu::RenderPipeline,
) -> wgpu::RenderPipeline {
    nana_gpu::__framework::render_pipeline_state(
        policy,
        nana_gpu::PipelineKey {
            generation: policy.generation(),
            target_format: nana_gpu::__framework::format_from_wgpu(format),
            sample_count: 1,
            shader: 0x6465_7374_7069_7065,
            layout: layout_key,
            material,
            primitive: 0,
            blend: 1,
            depth: 0,
            vertex_layout: 0,
        },
        create,
    )
    .expect("dest pipeline uses this context generation")
}

fn dest_blit_layout_key() -> u64 {
    ResourceTable::new(vec![
        LogicalBinding::new(
            0,
            LogicalBindingType::SampledTexture,
            &[ShaderStage::Fragment],
        ),
        LogicalBinding::new(1, LogicalBindingType::Sampler, &[ShaderStage::Fragment]),
    ])
    .expect("dest blit table")
    .layout_key()
}

fn dest_group_layout_key() -> u64 {
    ResourceTable::new(vec![
        LogicalBinding::new(
            0,
            LogicalBindingType::SampledTexture,
            &[ShaderStage::Fragment],
        ),
        LogicalBinding::new(1, LogicalBindingType::Sampler, &[ShaderStage::Fragment]),
        LogicalBinding::new(
            2,
            LogicalBindingType::DynamicBufferSlice,
            &[ShaderStage::Fragment],
        )
        .min_size(GROUP_UNIFORM_SIZE),
    ])
    .expect("dest group table")
    .layout_key()
}

const GROUP_UNIFORM_STRIDE: u64 = 256;
const GROUP_UNIFORM_SLOTS: u64 = 64;
const GROUP_UNIFORM_SIZE: u64 = 176;

#[derive(Debug, Clone, Copy)]
pub(super) struct GroupSlot {
    pub opacity: f32,
    pub clip: FragmentClip,
    pub filter: [f32; 3],
    pub filter_hue: f32,
    pub filter_blur: f32,
    pub mix_blend: u32,
    pub drop_shadow_offset: [f32; 2],
    pub drop_shadow_blur: f32,
    pub filter_invert: f32,
    pub drop_shadow_color: [f32; 4],
}

impl GroupSlot {
    /// Whether compositing can put pixels where the group drew none: a blur
    /// or drop shadow spreads them, and a colour filter is not relied on to
    /// leave a transparent pixel transparent.
    pub fn spreads(&self) -> bool {
        self.filter_blur > 0.5
            || self.drop_shadow_color[3] > 0.001
            || self.filter != [1.0, 1.0, 1.0]
            || self.filter_hue.abs() > 0.0001
            || self.filter_invert > 0.0001
    }

    pub fn dest(
        opacity: f32,
        filter: [f32; 3],
        filter_hue: f32,
        filter_blur: f32,
        mix_blend: u32,
        clip: FragmentClip,
    ) -> Self {
        Self {
            opacity,
            clip,
            filter,
            filter_hue,
            filter_blur,
            mix_blend,
            drop_shadow_offset: [0.0, 0.0],
            drop_shadow_blur: 0.0,
            filter_invert: 0.0,
            drop_shadow_color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn clip(clip: FragmentClip) -> Self {
        Self::dest(1.0, [1.0, 1.0, 1.0], 0.0, 0.0, 0, clip)
    }
}

fn pack_polygon(polygon: &[[f32; 2]; 8], count: u8) -> [[f32; 4]; 4] {
    let mut packed = [[0.0; 4]; 4];
    for (index, point) in polygon.iter().enumerate().take(count.min(8) as usize) {
        let slot = index / 2;
        let component = (index % 2) * 2;
        packed[slot][component] = point[0];
        packed[slot][component + 1] = point[1];
    }
    packed
}

fn pack_group_slot(slot: &GroupSlot) -> [u8; GROUP_UNIFORM_SIZE as usize] {
    let mut bytes = [0u8; GROUP_UNIFORM_SIZE as usize];
    let fields = [
        (0, slot.opacity),
        (4, slot.filter[0]),
        (8, slot.filter[1]),
        (12, slot.filter[2]),
        (16, slot.clip.rect[0]),
        (20, slot.clip.rect[1]),
        (24, slot.clip.rect[2]),
        (28, slot.clip.rect[3]),
        (32, slot.clip.inv_abcd[0]),
        (36, slot.clip.inv_abcd[1]),
        (40, slot.clip.inv_abcd[2]),
        (44, slot.clip.inv_abcd[3]),
        (48, slot.clip.inv_ef[0]),
        (52, slot.clip.inv_ef[1]),
        (56, slot.clip.corner_radius),
    ];
    for (offset, value) in fields {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[60..64].copy_from_slice(&(slot.clip.polygon_count as u32).to_le_bytes());
    let polys = pack_polygon(&slot.clip.polygon, slot.clip.polygon_count);
    for (index, poly) in polys.iter().enumerate() {
        let base = 64 + index * 16;
        for (component, value) in poly.iter().enumerate() {
            let offset = base + component * 4;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[128..132].copy_from_slice(&slot.filter_hue.to_le_bytes());
    bytes[132..136].copy_from_slice(&slot.filter_blur.to_le_bytes());
    bytes[136..140].copy_from_slice(&slot.mix_blend.to_le_bytes());
    bytes[144..148].copy_from_slice(&slot.drop_shadow_offset[0].to_le_bytes());
    bytes[148..152].copy_from_slice(&slot.drop_shadow_offset[1].to_le_bytes());
    bytes[152..156].copy_from_slice(&slot.drop_shadow_blur.to_le_bytes());
    bytes[156..160].copy_from_slice(&slot.filter_invert.to_le_bytes());
    for (index, value) in slot.drop_shadow_color.iter().enumerate() {
        let offset = 160 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn blend_multiply() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn blend_screen() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

fn group_layer_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nana-ui.scene.group.bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: NonZeroU64::new(GROUP_UNIFORM_SIZE),
                }),
            },
        ],
    })
}

/// [`nana_ui_core::MixBlendMode::gpu_index`] of the first blend mode the blend
/// unit cannot do.
const FIRST_READING_BLEND: u32 = 3;

fn group_blend_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
    view: &wgpu::TextureView,
    backdrop: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nana-ui.scene.group.blend.bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: NonZeroU64::new(GROUP_UNIFORM_SIZE),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(backdrop),
            },
        ],
    })
}

/// The pipeline for blend modes that read the backdrop, and its bind group
/// layout. Built the first time a frame has such a group: most apps never do.
fn reading_blend_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    policy: &nana_gpu::GpuDeviceState,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let group_blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("nana-ui.scene.group.blend.layout"),
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
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(GROUP_UNIFORM_SIZE),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let group_blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("nana-ui.scene.group.blend.shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(concat!(
            include_str!("shader/color.wgsl"),
            "\n",
            include_str!("shader/layer.wgsl"),
            "\n",
            include_str!("shader/layer_blend.wgsl"),
        ))),
    });
    let group_blend_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.group.blend.pipeline"),
            bind_group_layouts: &[Some(&group_blend_layout)],
            immediate_size: 0,
        });
    let group_pipeline_blend =
        cached_dest_pipeline(policy, format, 6, dest_group_layout_key(), || {
            wgpu::Device::create_render_pipeline(
                device,
                &wgpu::RenderPipelineDescriptor {
                    label: Some("nana-ui.scene.group.pipeline.blend"),
                    layout: Some(&group_blend_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &group_blend_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &group_blend_shader,
                        entry_point: Some("fs_blend"),
                        // The shader already composited against the backdrop copy.
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
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
    (group_blend_layout, group_pipeline_blend)
}

fn make_group_pipeline(
    device: &wgpu::Device,
    pipeline_cache: Option<&wgpu::PipelineCache>,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
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
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: pipeline_cache,
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DestPassCounts {
    pub color: u32,
    pub msaa: u32,
    pub blit: u32,
    pub group: u32,
    pub backdrop: u32,
    pub msaa_allocated: bool,
}

struct ReadingBlend {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    backdrop: wgpu::Texture,
    backdrop_view: wgpu::TextureView,
}

struct GroupLayer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    /// With the backdrop copy bound, for blend modes that read it. Built on
    /// the first frame that needs one.
    blend_bind_group: Option<wgpu::BindGroup>,
}

pub(super) struct DestTarget {
    pub width: u32,
    pub height: u32,
    pub msaa_allocated: bool,
    format: wgpu::TextureFormat,
    msaa: Option<wgpu::TextureView>,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    blit_pipeline: wgpu::RenderPipeline,
    /// The blit that stores gamma-premultiplied pixels; only an sRGB target
    /// has one (see [`AlphaEncoding::Gamma`]).
    blit_gamma_pipeline: Option<wgpu::RenderPipeline>,
    blit_bind_group: wgpu::BindGroup,
    group_layers: Vec<GroupLayer>,
    group_pipeline: wgpu::RenderPipeline,
    group_pipeline_multiply: wgpu::RenderPipeline,
    group_pipeline_screen: wgpu::RenderPipeline,
    /// CSS blend modes past multiply and screen: read a copy of the parent
    /// target and replace it with the blended result.
    /// Its bind group layout and pipeline, and the parent target, copied
    /// before a reading blend composites onto it. Built on the first frame
    /// that has such a group.
    reading_blend: Option<ReadingBlend>,
    group_bind_layout: wgpu::BindGroupLayout,
    group_sampler: wgpu::Sampler,
    group_uniforms: ManagedBuffer,
    group_uniform_slots: u64,
    slot_blend: Vec<u32>,
    policy: nana_gpu::GpuDeviceState,
}

impl DestTarget {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure(
        current: &mut Option<Self>,
        device: &wgpu::Device,
        pipeline_cache: Option<&wgpu::PipelineCache>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        want_msaa: bool,
        policy: &nana_gpu::GpuDeviceState,
    ) {
        if current.as_ref().is_some_and(|target| {
            target.width == width && target.height == height && target.msaa_allocated == want_msaa
        }) {
            return;
        }
        *current = Some(Self::new(
            device,
            pipeline_cache,
            format,
            width,
            height,
            want_msaa,
            policy,
        ));
    }

    fn new(
        device: &wgpu::Device,
        pipeline_cache: Option<&wgpu::PipelineCache>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        want_msaa: bool,
        policy: &nana_gpu::GpuDeviceState,
    ) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let msaa = want_msaa.then(|| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("nana-ui.scene.dest.msaa"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui.scene.dest.color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nana-ui.scene.dest.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let bind_layout = if policy.generation().get() != 0 {
            let stages = &[ShaderStage::Fragment];
            let table = ResourceTable::new(vec![
                LogicalBinding::new(0, LogicalBindingType::SampledTexture, stages),
                LogicalBinding::new(1, LogicalBindingType::Sampler, stages),
            ])
            .expect("dest blit logical table is valid")
            .for_generation(policy.generation());
            let logical =
                nana_gpu::__framework::logical_layout(device, policy.generation(), &table)
                    .expect("dest blit logical layout matches the device");
            nana_gpu::__framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.dest.blit.layout"),
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
                ],
            })
        };
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.dest.blit.bind"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.dest.blit.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader/blit.wgsl"
            ))),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.dest.blit.pipeline"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let make_blit = |entry_point: &str, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
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
                cache: pipeline_cache,
            })
        };
        let blit_pipeline = cached_dest_pipeline(policy, format, 1, dest_blit_layout_key(), || {
            make_blit("fs_main", "nana-ui.scene.dest.blit.pipeline")
        });
        let blit_gamma_pipeline = format.is_srgb().then(|| {
            cached_dest_pipeline(policy, format, 2, dest_blit_layout_key(), || {
                make_blit(
                    "fs_gamma_premultiplied",
                    "nana-ui.scene.dest.blit.gamma.pipeline",
                )
            })
        });
        let group_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nana-ui.scene.group.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let group_table = ResourceTable::new(vec![
            LogicalBinding::new(
                0,
                LogicalBindingType::SampledTexture,
                &[ShaderStage::Fragment],
            ),
            LogicalBinding::new(1, LogicalBindingType::Sampler, &[ShaderStage::Fragment]),
            LogicalBinding::new(
                2,
                LogicalBindingType::DynamicBufferSlice,
                &[ShaderStage::Fragment],
            )
            .min_size(GROUP_UNIFORM_SIZE),
        ])
        .expect("dest group logical table is valid")
        .for_generation(policy.generation());
        let group_logical =
            nana_gpu::__framework::logical_layout(device, policy.generation(), &group_table)
                .expect("dest group logical layout matches the device");
        let group_bind_layout = nana_gpu::__framework::resource_layout(&group_logical).clone();
        let group_uniforms = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.group.uniforms"),
            size: GROUP_UNIFORM_STRIDE * GROUP_UNIFORM_SLOTS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        let group_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.group.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(concat!(
                include_str!("shader/color.wgsl"),
                "\n",
                include_str!("shader/layer.wgsl"),
            ))),
        });
        let group_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("nana-ui.scene.group.pipeline"),
                bind_group_layouts: &[Some(&group_bind_layout)],
                immediate_size: 0,
            });
        let group_pipeline =
            cached_dest_pipeline(policy, format, 3, dest_group_layout_key(), || {
                make_group_pipeline(
                    device,
                    pipeline_cache,
                    &group_pipeline_layout,
                    &group_shader,
                    format,
                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                    "nana-ui.scene.group.pipeline",
                )
            });
        let group_pipeline_multiply =
            cached_dest_pipeline(policy, format, 4, dest_group_layout_key(), || {
                make_group_pipeline(
                    device,
                    pipeline_cache,
                    &group_pipeline_layout,
                    &group_shader,
                    format,
                    blend_multiply(),
                    "nana-ui.scene.group.pipeline.multiply",
                )
            });
        let group_pipeline_screen =
            cached_dest_pipeline(policy, format, 5, dest_group_layout_key(), || {
                make_group_pipeline(
                    device,
                    pipeline_cache,
                    &group_pipeline_layout,
                    &group_shader,
                    format,
                    blend_screen(),
                    "nana-ui.scene.group.pipeline.screen",
                )
            });
        Self {
            width,
            height,
            msaa_allocated: want_msaa,
            format,
            msaa,
            color,
            color_view,
            blit_pipeline,
            blit_gamma_pipeline,
            blit_bind_group,
            group_layers: Vec::new(),
            group_pipeline,
            group_pipeline_multiply,
            group_pipeline_screen,
            reading_blend: None,
            group_bind_layout,
            group_sampler,
            group_uniforms,
            group_uniform_slots: GROUP_UNIFORM_SLOTS,
            slot_blend: Vec::new(),
            policy: policy.clone(),
        }
    }

    pub(super) fn color_view(&self) -> &wgpu::TextureView {
        &self.color_view
    }

    pub(super) fn group_view(&self, layer: usize) -> &wgpu::TextureView {
        &self.group_layers[layer].view
    }

    pub(super) fn prepare_groups(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: usize,
        slots: &[GroupSlot],
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        while self.group_layers.len() < layers {
            self.push_group_layer(device);
        }
        let needed = (slots.len() as u64).max(1);
        if needed > self.group_uniform_slots {
            self.resize_group_uniforms(device, needed, gpu_work);
        }
        for (index, slot) in slots.iter().enumerate() {
            let bytes = pack_group_slot(slot);
            let offset = index as u64 * GROUP_UNIFORM_STRIDE;
            if let Some(work) = gpu_work {
                work.write_buffer(queue, &self.group_uniforms, offset, &bytes);
            } else {
                queue.write_buffer(&self.group_uniforms, offset, &bytes);
            }
        }
        self.slot_blend = slots.iter().map(|slot| slot.mix_blend).collect();
        if self
            .slot_blend
            .iter()
            .any(|blend| *blend >= FIRST_READING_BLEND)
        {
            self.prepare_reading_blends(device);
        }
    }

    /// The backdrop copy and a bind group per layer that reads it.
    fn prepare_reading_blends(&mut self, device: &wgpu::Device) {
        if self.reading_blend.is_none() {
            let (layout, pipeline) = reading_blend_pipeline(device, self.format, &self.policy);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("nana-ui.scene.group.backdrop"),
                size: wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.reading_blend = Some(ReadingBlend {
                layout,
                pipeline,
                backdrop: texture,
                backdrop_view: view,
            });
            for layer in &mut self.group_layers {
                layer.blend_bind_group = None;
            }
        }
        let reading = self.reading_blend.as_ref().expect("built above");
        for layer in &mut self.group_layers {
            if layer.blend_bind_group.is_none() {
                layer.blend_bind_group = Some(group_blend_bind_group(
                    device,
                    &reading.layout,
                    &self.group_sampler,
                    &self.group_uniforms,
                    &layer.view,
                    &reading.backdrop_view,
                ));
            }
        }
    }

    /// Whether `slot` composites with a blend mode that reads the backdrop.
    pub(super) fn reads_backdrop(&self, slot: u32) -> bool {
        self.slot_blend
            .get(slot as usize)
            .is_some_and(|blend| *blend >= FIRST_READING_BLEND)
    }

    /// Copy `area` of what a reading blend composites onto — `parent`'s
    /// layer, or the dest itself — into the same place in the backdrop
    /// texture. The composite is scissored to `area`, so nothing else of the
    /// backdrop is read.
    pub(super) fn copy_backdrop(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        parent: Option<usize>,
        area: crate::PhysicalRect,
    ) {
        let Some(ReadingBlend { backdrop, .. }) = self.reading_blend.as_ref() else {
            return;
        };
        let source = match parent {
            Some(layer) => &self.group_layers[layer].texture,
            None => &self.color,
        };
        let x = area.x.min(self.width);
        let y = area.y.min(self.height);
        let width = area.width.min(self.width - x);
        let height = area.height.min(self.height - y);
        if width == 0 || height == 0 {
            return;
        }
        let origin = wgpu::Origin3d { x, y, z: 0 };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                origin,
                ..source.as_image_copy()
            },
            wgpu::TexelCopyTextureInfo {
                origin,
                ..backdrop.as_image_copy()
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn resize_group_uniforms(
        &mut self,
        device: &wgpu::Device,
        slots: u64,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let slots = slots.max(1);
        let size = GROUP_UNIFORM_STRIDE * slots;
        let usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        if let Some(work) = gpu_work {
            work.replace_buffer(
                device,
                &mut self.group_uniforms,
                size,
                usage,
                "nana-ui.scene.group.uniforms",
            );
        } else {
            self.group_uniforms =
                ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana-ui.scene.group.uniforms"),
                    size,
                    usage,
                    mapped_at_creation: false,
                }));
        }
        self.group_uniform_slots = slots;
        // Only the bind groups reference `group_uniforms`. Recreating the layer
        // textures here would throw away full dest-sized attachments that the
        // resize does not touch.
        let rebound = self
            .group_layers
            .iter()
            .map(|layer| {
                group_layer_bind_group(
                    device,
                    &self.group_bind_layout,
                    &self.group_sampler,
                    &self.group_uniforms,
                    &layer.view,
                )
            })
            .collect::<Vec<_>>();
        for (layer, bind_group) in self.group_layers.iter_mut().zip(rebound) {
            layer.bind_group = bind_group;
            layer.blend_bind_group = None;
        }
    }

    fn push_group_layer(&mut self, device: &wgpu::Device) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui.scene.group.color"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = group_layer_bind_group(
            device,
            &self.group_bind_layout,
            &self.group_sampler,
            &self.group_uniforms,
            &view,
        );
        self.group_layers.push(GroupLayer {
            texture,
            view,
            bind_group,
            blend_bind_group: None,
        });
    }

    pub(super) fn begin_group_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        layer: usize,
        load: wgpu::LoadOp<wgpu::Color>,
        counts: &mut DestPassCounts,
    ) -> wgpu::RenderPass<'a> {
        counts.group = counts.group.saturating_add(1);
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nana-ui.scene.group"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.group_layers[layer].view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub(super) fn composite_group(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        layer: usize,
        slot: u32,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let max_slot = self.group_uniform_slots.saturating_sub(1) as u32;
        let slot = slot.min(max_slot);
        let offset = (slot as u64 * GROUP_UNIFORM_STRIDE) as u32;
        let blend = self.slot_blend.get(slot as usize).copied().unwrap_or(0);
        let group = &self.group_layers[layer];
        match (
            blend,
            group
                .blend_bind_group
                .as_ref()
                .zip(self.reading_blend.as_ref()),
        ) {
            (1, _) => {
                pass.set_pipeline(&self.group_pipeline_multiply);
                pass.set_bind_group(0, &group.bind_group, &[offset]);
            }
            (2, _) => {
                pass.set_pipeline(&self.group_pipeline_screen);
                pass.set_bind_group(0, &group.bind_group, &[offset]);
            }
            (blend, Some((blend_bind_group, reading))) if blend >= FIRST_READING_BLEND => {
                pass.set_pipeline(&reading.pipeline);
                pass.set_bind_group(0, blend_bind_group, &[offset]);
            }
            _ => {
                pass.set_pipeline(&self.group_pipeline);
                pass.set_bind_group(0, &group.bind_group, &[offset]);
            }
        }
        pass.draw(0..3, 0..1);
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }

    pub(super) fn begin_color_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        load: wgpu::LoadOp<wgpu::Color>,
        counts: &mut DestPassCounts,
    ) -> wgpu::RenderPass<'a> {
        counts.color = counts.color.saturating_add(1);
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nana-ui.scene.dest.color"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub(super) fn begin_msaa_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        clear: wgpu::Color,
        counts: &mut DestPassCounts,
    ) -> wgpu::RenderPass<'a> {
        counts.msaa = counts.msaa.saturating_add(1);
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nana-ui.scene.dest.msaa"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.msaa.as_ref().expect("UI-only dest allocates 4x MSAA"),
                depth_slice: None,
                resolve_target: Some(&self.color_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn blit(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        dest_x: f32,
        dest_y: f32,
        window: [u32; 2],
        clear_window: Option<wgpu::Color>,
        encoding: AlphaEncoding,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
        counts: &mut DestPassCounts,
    ) {
        counts.blit = counts.blit.saturating_add(1);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nana-ui.scene.dest.blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: clear_window.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let x = dest_x.max(0.0);
        let y = dest_y.max(0.0);
        let max_w = window[0] as f32 - x;
        let max_h = window[1] as f32 - y;
        if max_w < 1.0 || max_h < 1.0 {
            return;
        }
        pass.set_viewport(
            x,
            y,
            (self.width as f32).min(max_w),
            (self.height as f32).min(max_h),
            0.0,
            1.0,
        );
        let gamma = self
            .blit_gamma_pipeline
            .as_ref()
            .filter(|_| encoding == AlphaEncoding::Gamma);
        pass.set_pipeline(gamma.unwrap_or(&self.blit_pipeline));
        pass.set_bind_group(0, &self.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }
}
