use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};
use nana_gpu::{__framework, LogicalBinding, LogicalBindingType, ResourceTable, ShaderStage};

use super::{
    clip::{self, FragmentClip, LogicalRect},
    color::{orthographic, pack_linear, with_opacity},
};
use crate::{PhysicalRect, gpu_work::ManagedBuffer, icons::Icon};

/// Per-glyph raster edge cap. A glyph is rasterized at twice its dest size,
/// so this also caps the biggest cell the shared atlas has to place.
const MAX_ATLAS_PX: u32 = 256;
/// Starting edge of the shared atlas, one sheet per render target. Not a cap:
/// [`IconPipeline::repack`] doubles it when one frame's glyphs do not fit.
///
/// 256² RGBA is 256 KiB, and it is spent up front rather than per glyph. That
/// is *more* than the per-glyph textures cost a typical window — a 20-glyph
/// 16px toolbar was ~82 KiB — and it is the price of the single bind group
/// that lets those 20 glyphs be one draw. It is sized so a real shell needs no
/// repack at all: 20 cells of 34px plus a row glyph is 24 KiB of the sheet.
const ATLAS_START_PX: u32 = 256;
/// Largest atlas edge we will grow to, independent of what the device allows.
/// 2048² RGBA is 16 MiB, which is already far past any real icon working set.
const MAX_ATLAS_EDGE: u32 = 2048;
fn icon_layout_key() -> u64 {
    let vertex = &[ShaderStage::Vertex];
    let fragment = &[ShaderStage::Fragment];
    let uniforms = ResourceTable::new(vec![
        LogicalBinding::new(0, LogicalBindingType::UniformBuffer, vertex)
            .min_size(std::mem::size_of::<Uniforms>() as u64),
    ])
    .expect("icon uniform table")
    .layout_key();
    let atlas = ResourceTable::new(vec![
        LogicalBinding::new(0, LogicalBindingType::SampledTexture, fragment),
        LogicalBinding::new(1, LogicalBindingType::Sampler, fragment),
    ])
    .expect("icon atlas table")
    .layout_key();
    uniforms ^ atlas.rotate_left(17)
}
/// Transparent border around every cell. The sampler clamps at the texture
/// border, not at the cell, and a quad can sample up to half a texel outside
/// its own cell — a rotated icon, or one drawn wider than `MAX_ATLAS_PX`. One
/// texel of nobody's pixels is what keeps such a tap from reading a
/// neighbouring glyph.
///
/// It reads transparent rather than replicating the glyph's own edge, which
/// is what a per-glyph texture's `ClampToEdge` used to give. Replicating was
/// implemented first and then dropped: no frame could be built where it
/// changed a pixel, a full-bleed glyph included, because the rasterizer
/// confines UV to `[0, 1]` and the taps only leave the cell inside the last
/// half texel.
const CELL_GUTTER: u32 = 1;
const INITIAL_VERTICES: usize = 256;

/// Vertex order of one icon quad, shared by [`IconPipeline::prepare`] and
/// [`IconPipeline::repack`] so the two cannot disagree about which corner a
/// vertex holds.
const CORNER_UV: [[f32; 2]; 6] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [0.0, 1.0],
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
];

const ICON_SHADER: &str = concat!(
    include_str!("shader/color.wgsl"),
    r#"
struct Globals {
    transform: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var atlas: texture_2d<f32>;

@group(1) @binding(1)
var atlas_sampler: sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) clip_inv_abcd: vec4<f32>,
    @location(5) clip_inv_ef: vec3<f32>,
    @location(6) cell: vec4<f32>,
}

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_pos: vec2<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) clip_inv_abcd: vec4<f32>,
    @location(5) clip_inv_ef: vec3<f32>,
    // The glyph's cell in the atlas: the taps a shrunk glyph averages stay in
    // it rather than read a neighbour.
    @location(6) @interpolate(flat) cell: vec4<f32>,
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.position = globals.transform * vec4<f32>(input.position, 0.0, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    out.world_pos = input.position;
    out.clip_rect = input.clip_rect;
    out.clip_inv_abcd = input.clip_inv_abcd;
    out.clip_inv_ef = input.clip_inv_ef;
    out.cell = input.cell;
    return out;
}

// One device pixel's worth of the atlas at `uv`, whose screen derivatives are
// `dx`/`dy`. A glyph is rasterized at twice its size for its transform's
// larger scale, which one bilinear tap per pixel filters exactly; shrunk
// further along the other axis, that axis averages a grid of such taps across
// the pixel's footprint. A tap past the glyph's `cell` reads its transparent
// gutter, never a neighbour.
fn icon_sample(uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let texels = vec2<f32>(textureDimensions(atlas));
    let footprint = vec2(length(dx * texels), length(dy * texels));
    let taps = clamp(ceil(footprint * 0.5 - 1.0 / 16.0), vec2(1.0), vec2(8.0));
    if all(taps <= vec2(1.0)) {
        return textureSampleLevel(atlas, atlas_sampler, uv, 0.0);
    }
    let half_texel = 0.5 / texels;
    var sum = vec4(0.0);
    for (var i = 0.0; i < taps.x; i += 1.0) {
        for (var j = 0.0; j < taps.y; j += 1.0) {
            let offset = ((i + 0.5) / taps.x - 0.5) * dx + ((j + 0.5) / taps.y - 0.5) * dy;
            let at = clamp(uv + offset, cell.xy - half_texel, cell.zw + half_texel);
            sum += textureSampleLevel(atlas, atlas_sampler, at, 0.0);
        }
    }
    return sum / (taps.x * taps.y);
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let uv_dx = dpdx(input.uv);
    let uv_dy = dpdy(input.uv);
    let clip_cover = fragment_clip_coverage(
        input.world_pos,
        input.clip_rect,
        input.clip_inv_abcd,
        input.clip_inv_ef.xy,
        input.clip_inv_ef.z,
        0u,
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        1.0,
    );
    if clip_cover <= 0.0 {
        discard;
    }
    let sampled = icon_sample(input.uv, uv_dx, uv_dy, input.cell);
    return vec4<f32>(sampled.rgb * input.color.rgb, sampled.a * input.color.a * clip_cover);
}
"#
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    transform: [f32; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct IconVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    clip_rect: [f32; 4],
    clip_inv_abcd: [f32; 4],
    clip_inv_ef: [f32; 3],
    cell: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct AtlasKey {
    icon: usize,
    px: u32,
}

/// One glyph's placement in the shared atlas.
struct AtlasEntry {
    /// Glyph origin in atlas texels, gutter excluded.
    origin: [u32; 2],
    px: u32,
    /// Kept so a repack can re-rasterize without the caller handing the icon
    /// back. `Icon` geometry is `'static`, so this borrows nothing per frame.
    svg: &'static str,
}

/// A row of equal-sized cells. Glyphs are square, so a shelf that only takes
/// its own cell size never fragments internally.
struct Shelf {
    /// Cell edge including both gutters.
    cell: u32,
    y: u32,
    next_x: u32,
}

/// The one texture every icon draws from. A single bind group is what lets a
/// run of *different* glyphs collapse into one `pass.draw`.
struct Atlas {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    edge: u32,
    shelves: Vec<Shelf>,
    next_y: u32,
}

struct FrameSlot {
    key: AtlasKey,
    first_vertex: u32,
    vertex_count: u32,
}

#[derive(Clone, Copy)]
pub(super) struct PreparedIcon {
    pub(super) index: usize,
}

pub(super) struct IconPipeline {
    pipeline: wgpu::RenderPipeline,
    atlas_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform_bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    vertices: ManagedBuffer,
    vertex_capacity: usize,
    pending_vertices: Vec<IconVertex>,
    uploaded_vertices: Vec<IconVertex>,
    physical_size: [u32; 2],
    uploaded_size: Option<[u32; 2]>,
    frame_slots: Vec<FrameSlot>,
    atlas: Atlas,
    entries: HashMap<AtlasKey, AtlasEntry>,
    frame_keys: HashSet<AtlasKey>,
    /// Set when even a maximal atlas could not hold this frame's glyphs, so
    /// the remaining misses fail straight away instead of repacking per icon.
    exhausted: bool,
}

impl IconPipeline {
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
            label: Some("nana-ui.scene.icon.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(ICON_SHADER)),
        });
        let uniform_layout = if let Some(policy) = policy {
            let table = ResourceTable::new(vec![
                LogicalBinding::new(0, LogicalBindingType::UniformBuffer, &[ShaderStage::Vertex])
                    .min_size(std::mem::size_of::<Uniforms>() as u64),
            ])
            .expect("icon uniform table is valid")
            .for_generation(policy.generation());
            let logical = __framework::logical_layout(device, policy.generation(), &table)
                .expect("icon uniform layout matches the device");
            __framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.icon.uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Uniforms>() as u64
                        ),
                    },
                    count: None,
                }],
            })
        };
        let atlas_layout = if let Some(policy) = policy {
            let table = ResourceTable::new(vec![
                LogicalBinding::new(
                    0,
                    LogicalBindingType::SampledTexture,
                    &[ShaderStage::Fragment],
                ),
                LogicalBinding::new(1, LogicalBindingType::Sampler, &[ShaderStage::Fragment]),
            ])
            .expect("icon atlas table is valid")
            .for_generation(policy.generation());
            let logical = __framework::logical_layout(device, policy.generation(), &table)
                .expect("icon atlas layout matches the device");
            __framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.icon.atlas"),
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
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.icon.uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.icon.uniforms.bind"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nana-ui.scene.icon.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.icon.pipeline"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });
        let create_pipeline = || {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("nana-ui.scene.icon.pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<IconVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array!(
                            0 => Float32x2,
                            1 => Float32x2,
                            2 => Float32x4,
                            3 => Float32x4,
                            4 => Float32x4,
                            5 => Float32x3,
                            6 => Float32x4,
                        ),
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = if let Some(policy) = policy {
            nana_gpu::__framework::render_pipeline_state(
                policy,
                nana_gpu::PipelineKey {
                    generation: policy.generation(),
                    target_format: nana_gpu::__framework::format_from_wgpu(format),
                    sample_count: 1,
                    shader: 0x6963_6f6e_7069_7065,
                    layout: icon_layout_key(),
                    material: 0,
                    primitive: 0,
                    blend: 1,
                    depth: 0,
                    vertex_layout: std::mem::size_of::<IconVertex>() as u64,
                },
                create_pipeline,
            )
            .expect("icon pipeline uses this context generation")
        } else {
            create_pipeline()
        };
        let atlas = new_atlas(device, &atlas_layout, &sampler, ATLAS_START_PX);
        Self {
            pipeline,
            atlas_layout,
            sampler,
            uniform_bind_group,
            uniforms,
            vertices: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.icon.vertices"),
                size: (INITIAL_VERTICES * std::mem::size_of::<IconVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            vertex_capacity: INITIAL_VERTICES,
            pending_vertices: Vec::new(),
            uploaded_vertices: Vec::new(),
            physical_size: [0; 2],
            uploaded_size: None,
            frame_slots: Vec::new(),
            atlas,
            entries: HashMap::new(),
            frame_keys: HashSet::new(),
            exhausted: false,
        }
    }

    pub(super) fn begin_frame(&mut self, physical_size: [u32; 2]) {
        self.pending_vertices.clear();
        self.frame_slots.clear();
        self.frame_keys.clear();
        self.exhausted = false;
        self.physical_size = physical_size;
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        scale: f32,
        icon: Icon,
        color: [f32; 4],
        opacity: f32,
        fragment_clip: FragmentClip,
    ) -> Option<PreparedIcon> {
        self.prepare_with_work(
            device,
            queue,
            bounds,
            affine,
            persp,
            scale,
            icon,
            color,
            opacity,
            fragment_clip,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_with_work(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: LogicalRect,
        affine: [f32; 6],
        persp: [f32; 2],
        scale: f32,
        icon: Icon,
        color: [f32; 4],
        opacity: f32,
        fragment_clip: FragmentClip,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> Option<PreparedIcon> {
        let extent = bounds.width.min(bounds.height);
        if extent <= 0.0 || scale <= 0.0 {
            return None;
        }
        let dest_px = (extent * scale * raster_factor(affine)).round().max(1.0) as u32;
        let px = dest_px.saturating_mul(2).clamp(2, MAX_ATLAS_PX);
        let key = AtlasKey {
            icon: icon.as_ptr() as usize,
            px,
        };
        if !self.entries.contains_key(&key) {
            let rgba = rasterize_icon(icon.svg(), px)?;
            if !self.insert_with_work(device, queue, key, px, icon.svg(), &rgba, work) {
                return None;
            }
        }
        self.frame_keys.insert(key);
        let uv_rect = UvRect::of(self.entries.get(&key)?, self.atlas.edge);
        let color = pack_linear(with_opacity(color, opacity));
        let clip = fragment_clip.for_physical_pixels(scale);
        let first_vertex = self.pending_vertices.len() as u32;
        let [tl, tr, bl, br] = icon_quad(bounds, affine, persp, scale);
        let positions = [tl, tr, bl, tr, br, bl];
        for (position, corner) in positions.into_iter().zip(CORNER_UV) {
            self.pending_vertices.push(IconVertex {
                position,
                uv: uv_rect.at(corner),
                cell: uv_rect.bounds(),
                color,
                clip_rect: clip.rect,
                clip_inv_abcd: clip.inv_abcd,
                clip_inv_ef: [clip.inv_ef[0], clip.inv_ef[1], clip.corner_radius],
            });
        }
        let index = self.frame_slots.len();
        self.frame_slots.push(FrameSlot {
            key,
            first_vertex,
            vertex_count: 6,
        });
        Some(PreparedIcon { index })
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if self.pending_vertices.is_empty() {
            return;
        }
        if self.uploaded_size != Some(self.physical_size) {
            let uniforms = Uniforms {
                transform: orthographic(self.physical_size[0], self.physical_size[1]),
            };
            let uniform_bytes = bytemuck::bytes_of(&uniforms);
            if let Some(work) = work {
                work.write_buffer(queue, &self.uniforms, 0, uniform_bytes);
            } else {
                queue.write_buffer(&self.uniforms, 0, uniform_bytes);
            }
            self.uploaded_size = Some(self.physical_size);
        }
        if self.pending_vertices.len() > self.vertex_capacity {
            self.uploaded_vertices.clear();
            if let Some(work) = work {
                work.record_realloc();
            }
            self.vertex_capacity = self.pending_vertices.len().next_power_of_two();
            let size = (self.vertex_capacity * std::mem::size_of::<IconVertex>()) as u64;
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = work {
                work.replace_buffer(
                    device,
                    &mut self.vertices,
                    size,
                    usage,
                    "nana-ui.scene.icon.vertices",
                );
            } else {
                self.vertices = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana-ui.scene.icon.vertices"),
                    size,
                    usage,
                    mapped_at_creation: false,
                }))
            };
        }
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.vertices,
            bytemuck::cast_slice(&self.uploaded_vertices),
            bytemuck::cast_slice(&self.pending_vertices),
            work,
        );
        self.uploaded_vertices.clone_from(&self.pending_vertices);
    }

    /// Whether `next` can extend a run that starts at `first` and holds
    /// `count` slots. Every glyph lives in the one shared atlas, so the only
    /// requirement left is that the vertices are already adjacent in the
    /// shared buffer — a toolbar of distinct glyphs is one run.
    pub(super) fn can_extend_run(&self, first: usize, count: usize, next: usize) -> bool {
        let (Some(last), Some(candidate)) = (
            self.frame_slots.get(first + count - 1),
            self.frame_slots.get(next),
        ) else {
            return false;
        };
        last.first_vertex + last.vertex_count == candidate.first_vertex
    }

    /// Draw one run of adjacent slots as a single call, whatever glyphs it
    /// holds: they all sample the same atlas and the same bind group.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        first: usize,
        count: usize,
        scissor: PhysicalRect,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let (Some(head), Some(last)) = (
            self.frame_slots.get(first),
            self.frame_slots.get(first + count.max(1) - 1),
        ) else {
            return;
        };
        let end = last.first_vertex + last.vertex_count;
        if end <= head.first_vertex {
            return;
        }
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(head.first_vertex..end, 0..1);
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }

    /// Place one glyph in the shared atlas. Returns `false` only when even a
    /// maximal atlas cannot hold this frame's glyphs, which is also the one
    /// case where the icon does not paint.
    #[cfg_attr(not(test), allow(dead_code))]
    fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: AtlasKey,
        px: u32,
        svg: &'static str,
        rgba: &[u8],
    ) -> bool {
        self.insert_with_work(device, queue, key, px, svg, rgba, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_with_work(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        key: AtlasKey,
        px: u32,
        svg: &'static str,
        rgba: &[u8],
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> bool {
        if self.exhausted {
            return false;
        }
        if let Some(cell) = self.allocate(px + CELL_GUTTER * 2) {
            self.write_cell(queue, cell, px, rgba, work);
            self.entries.insert(
                key,
                AtlasEntry {
                    origin: [cell[0] + CELL_GUTTER, cell[1] + CELL_GUTTER],
                    px,
                    svg,
                },
            );
            return true;
        }
        // Full for this cell size. Rebuilding the atlas around exactly this
        // frame's glyphs both reclaims every idle entry and compacts the
        // shelves, so it is the eviction policy as well as the growth path.
        self.repack(device, queue, (key, svg, rgba), work)
    }

    /// Reserve a `cell`-sized square. Shelves are homogeneous, so a glyph
    /// either extends a shelf of its own cell size or opens a new one.
    fn allocate(&mut self, cell: u32) -> Option<[u32; 2]> {
        let edge = self.atlas.edge;
        if cell > edge {
            return None;
        }
        if let Some(shelf) = self
            .atlas
            .shelves
            .iter_mut()
            .find(|shelf| shelf.cell == cell && shelf.next_x + cell <= edge)
        {
            let origin = [shelf.next_x, shelf.y];
            shelf.next_x += cell;
            return Some(origin);
        }
        if self.atlas.next_y + cell > edge {
            return None;
        }
        let y = self.atlas.next_y;
        self.atlas.next_y += cell;
        self.atlas.shelves.push(Shelf {
            cell,
            y,
            next_x: cell,
        });
        Some([0, y])
    }

    /// Rebuild the atlas holding this frame's glyphs plus `extra`.
    ///
    /// The new texture is sized to leave room for another working set's worth
    /// of glyphs, so a frame that alternates icon sets does not repack every
    /// frame. Every kept glyph is re-rasterized from its own SVG — the atlas
    /// is the only copy of the pixels, and re-rasterizing is bounded by the
    /// frame's glyph count.
    fn repack(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extra: (AtlasKey, &'static str, &[u8]),
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> bool {
        let (extra_key, extra_svg, extra_rgba) = extra;
        // `frame_keys` gains the caller's key only after `prepare` has placed
        // it, so the glyph that triggered this repack arrives as `extra`.
        let mut keep: Vec<(AtlasKey, &'static str)> = self
            .frame_keys
            .iter()
            .filter_map(|key| Some((*key, self.entries.get(key)?.svg)))
            .collect();
        keep.push((extra_key, extra_svg));
        // Biggest cells first: a shelf is only reused by its own size, so
        // placing large cells while the sheet is empty avoids stranding them.
        // The glyph order behind that is `frame_keys` iteration order, which
        // is a hash set's — pin it so one frame packs the same way twice.
        keep.sort_unstable_by(|left, right| {
            right
                .0
                .px
                .cmp(&left.0.px)
                .then(left.0.icon.cmp(&right.0.icon))
        });
        let needed: u64 = keep
            .iter()
            .map(|(key, _)| {
                let cell = (key.px + CELL_GUTTER * 2) as u64;
                cell * cell
            })
            .sum();
        let max_edge = MAX_ATLAS_EDGE.min(device.limits().max_texture_dimension_2d);
        let mut edge = self.atlas.edge.max(ATLAS_START_PX).min(max_edge);
        while (edge as u64) * (edge as u64) < needed * 2 && edge < max_edge {
            edge = (edge * 2).min(max_edge);
        }
        loop {
            self.atlas = new_atlas(device, &self.atlas_layout, &self.sampler, edge);
            self.entries.clear();
            let placed = keep.iter().all(|(key, svg)| {
                let Some(cell) = self.allocate(key.px + CELL_GUTTER * 2) else {
                    return false;
                };
                self.entries.insert(
                    *key,
                    AtlasEntry {
                        origin: [cell[0] + CELL_GUTTER, cell[1] + CELL_GUTTER],
                        px: key.px,
                        svg,
                    },
                );
                true
            });
            if placed {
                break;
            }
            if edge >= max_edge {
                self.entries.clear();
                self.exhausted = true;
                return false;
            }
            edge = (edge * 2).min(max_edge);
        }
        for (key, svg) in &keep {
            let Some(entry) = self.entries.get(key) else {
                continue;
            };
            let cell = [entry.origin[0] - CELL_GUTTER, entry.origin[1] - CELL_GUTTER];
            if *key == extra_key {
                self.write_cell(queue, cell, key.px, extra_rgba, work);
                continue;
            }
            let Some(rgba) = rasterize_icon(svg, key.px) else {
                continue;
            };
            self.write_cell(queue, cell, key.px, &rgba, work);
        }
        self.patch_frame_uvs();
        true
    }

    /// Rewrite the UVs of vertices already emitted this frame. A repack moves
    /// every cell, and those vertices still name the old ones.
    fn patch_frame_uvs(&mut self) {
        let Self {
            frame_slots,
            entries,
            pending_vertices,
            atlas,
            ..
        } = self;
        for slot in frame_slots.iter() {
            let Some(entry) = entries.get(&slot.key) else {
                continue;
            };
            let uv_rect = UvRect::of(entry, atlas.edge);
            let first = slot.first_vertex as usize;
            for (offset, corner) in CORNER_UV.iter().enumerate() {
                if let Some(vertex) = pending_vertices.get_mut(first + offset) {
                    vertex.uv = uv_rect.at(*corner);
                    vertex.cell = uv_rect.bounds();
                }
            }
        }
    }

    /// Upload one glyph into its cell. The gutter is left at the texture's
    /// initial zero, which is all it has to be: no other glyph writes there,
    /// so a tap that leaves a cell reads transparent instead of a neighbour.
    fn write_cell(
        &mut self,
        queue: &wgpu::Queue,
        cell: [u32; 2],
        px: u32,
        rgba: &[u8],
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if let Some(work) = work {
            work.write_texture(
                queue,
                &self.atlas.texture,
                [cell[0] + CELL_GUTTER, cell[1] + CELL_GUTTER, 0],
                rgba,
                px * 4,
                px,
                [px, px, 1],
            );
        } else {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: cell[0] + CELL_GUTTER,
                        y: cell[1] + CELL_GUTTER,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(px * 4),
                    rows_per_image: Some(px),
                },
                wgpu::Extent3d {
                    width: px,
                    height: px,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

/// Normalized UV rect of one atlas cell.
#[derive(Clone, Copy)]
struct UvRect {
    origin: [f32; 2],
    size: [f32; 2],
}

impl UvRect {
    fn of(entry: &AtlasEntry, edge: u32) -> Self {
        let edge = edge as f32;
        Self {
            origin: [entry.origin[0] as f32 / edge, entry.origin[1] as f32 / edge],
            size: [entry.px as f32 / edge, entry.px as f32 / edge],
        }
    }

    fn bounds(self) -> [f32; 4] {
        let [x, y] = self.origin;
        [x, y, x + self.size[0], y + self.size[1]]
    }

    fn at(self, corner: [f32; 2]) -> [f32; 2] {
        [
            self.origin[0] + corner[0] * self.size[0],
            self.origin[1] + corner[1] * self.size[1],
        ]
    }
}

fn new_atlas(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    edge: u32,
) -> Atlas {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui.scene.icon.atlas"),
        size: wgpu::Extent3d {
            width: edge,
            height: edge,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nana-ui.scene.icon.atlas.bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Atlas {
        texture,
        bind_group,
        edge,
        shelves: Vec::new(),
        next_y: 0,
    }
}

fn icon_quad(bounds: LogicalRect, affine: [f32; 6], persp: [f32; 2], scale: f32) -> [[f32; 2]; 4] {
    let extent = bounds.width.min(bounds.height);
    let x = bounds.x + (bounds.width - extent) * 0.5;
    let y = bounds.y + (bounds.height - extent) * 0.5;
    if clip::is_translation_projective(affine, persp) {
        let cx = clip::on_grid(x + extent * 0.5, affine[4], scale);
        let cy = clip::on_grid(y + extent * 0.5, affine[5], scale);
        let (x0, px) = clip::snap_centered_origin(cx, extent, scale);
        let (y0, _) = clip::snap_centered_origin(cy, extent, scale);
        [[x0, y0], [x0 + px, y0], [x0, y0 + px], [x0 + px, y0 + px]]
    } else {
        [
            [x, y],
            [x + extent, y],
            [x, y + extent],
            [x + extent, y + extent],
        ]
        .map(|[px, py]| {
            let [tx, ty] = clip::transform_point_projective(affine, persp, px, py);
            [tx * scale, ty * scale]
        })
    }
}

/// The scale `affine` shows an icon at, in half-octave steps from 1/4 to 4 as
/// text's raster steps: rasterized at about the size it is seen, a glyph's
/// edge stays about one device pixel, where a shrunk bitmap would alias and a
/// magnified one blur. Stepped so a scale animation rasterizes each step once.
fn raster_factor(affine: [f32; 6]) -> f32 {
    let [a, b, c, d, _, _] = affine;
    let scale = (a * a + b * b).max(c * c + d * d).sqrt();
    if !scale.is_finite() || scale <= 0.0 {
        return 1.0;
    }
    2f32.powf((2.0 * scale.log2()).round().clamp(-4.0, 4.0) * 0.5)
}

fn rasterize_icon(svg: &str, pixel_size: u32) -> Option<Vec<u8>> {
    nana_svg_raster::rasterize_white_mask(svg, pixel_size, MAX_ATLAS_PX)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn a_scrolled_icon_moves_by_the_scrolls_whole_pixels() {
        // A 16 px icon at an odd y at 150% is exactly on a tie of the grid.
        // Forty thousand pixels down a list the snapped translation's own
        // rounding error would pick its side frame by frame; the icon must
        // move by the whole pixels its quad does, and no other amount.
        let scale = 1.5;
        let origin = clip::PaintOrigin::new([0.0, 0.0], scale);
        let bounds = LogicalRect::from_xywh(0.0, 40_001.0, 16.0, 16.0);
        let top = |offset: f32| {
            let affine = clip::paint_affine([1.0, 0.0, 0.0, 1.0, 0.0, -offset], origin);
            (icon_quad(bounds, affine, [0.0; 2], scale)[0][1], affine[5])
        };
        let (still, from) = top(39_900.0);
        for step in 1..300 {
            let (moved, to) = top(39_900.0 + step as f32 * 0.37);
            assert_eq!(
                moved,
                still + ((to - from) * scale).round(),
                "scrolled {} px",
                step as f32 * 0.37
            );
        }
    }

    #[test]
    fn icon_svg_rasterizes_coverage() {
        let rgba = rasterize_icon(Icon::Search.svg(), 28).expect("search svg");
        assert_eq!(rgba.len(), 28 * 28 * 4);
        let coverage = rgba.chunks(4).filter(|pixel| pixel[3] > 16).count();
        assert!(
            coverage > 20,
            "search icon should ink the atlas, got {coverage}"
        );
        assert!(
            rgba.chunks(4).any(|pixel| pixel[3] < 16),
            "search icon should keep transparent padding"
        );
    }

    fn ink_bbox(rgba: &[u8], px: u32) -> (u32, u32, u32, u32) {
        let px = px as usize;
        let mut min_x = px;
        let mut min_y = px;
        let mut max_x = 0;
        let mut max_y = 0;
        for y in 0..px {
            for x in 0..px {
                if rgba[(y * px + x) * 4 + 3] > 16 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        (min_x as u32, min_y as u32, max_x as u32, max_y as u32)
    }

    /// Every glyph shares one sheet now, so capacity is area rather than an
    /// entry count. When the sheet fills, the glyphs this frame already
    /// emitted vertices for must survive and the idle ones must go — the
    /// repack is the eviction policy.
    #[test]
    fn a_full_sheet_keeps_this_frames_glyphs_and_drops_the_idle_ones() {
        let (device, queue) = crate::scene_paint::tests::test_device();
        let mut pipeline = IconPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        const PX: u32 = 64;
        let rgba = vec![255u8; (PX * PX * 4) as usize];
        let svg = Icon::Close.svg();
        let key = |icon: usize| AtlasKey { icon, px: PX };

        pipeline.begin_frame([64; 2]);
        for icon in 0..3 {
            assert!(pipeline.insert(&device, &queue, key(icon), PX, svg, &rgba));
            pipeline.frame_keys.insert(key(icon));
        }
        // Then fill the sheet with glyphs nothing is using this frame.
        let mut idle = 100usize;
        let mut before = pipeline.entries.len();
        loop {
            assert!(pipeline.insert(&device, &queue, key(idle), PX, svg, &rgba));
            idle += 1;
            if pipeline.entries.len() < before {
                break;
            }
            before = pipeline.entries.len();
            assert!(
                idle < 400,
                "a {ATLAS_START_PX}² sheet of 66px cells must fill first"
            );
        }
        assert_eq!(
            pipeline.atlas.edge, ATLAS_START_PX,
            "a working set this small must be repacked into the same sheet, not a bigger one"
        );
        for icon in 0..3 {
            assert!(
                pipeline.entries.contains_key(&key(icon)),
                "glyph {icon} is in use this frame and must survive the repack"
            );
        }
        assert_eq!(
            pipeline.entries.len(),
            4,
            "a repack keeps this frame's glyphs plus the one that triggered it"
        );
    }

    /// The other half: when one frame's own glyphs do not fit, the sheet grows
    /// instead of dropping a glyph that still has to paint.
    #[test]
    fn a_frame_larger_than_the_sheet_grows_it_instead_of_dropping_glyphs() {
        let (device, queue) = crate::scene_paint::tests::test_device();
        let mut pipeline = IconPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        const PX: u32 = 128;
        let rgba = vec![255u8; (PX * PX * 4) as usize];
        let svg = Icon::Close.svg();
        let key = |icon: usize| AtlasKey { icon, px: PX };

        pipeline.begin_frame([64; 2]);
        for icon in 0..16 {
            assert!(
                pipeline.insert(&device, &queue, key(icon), PX, svg, &rgba),
                "glyph {icon} must find room"
            );
            pipeline.frame_keys.insert(key(icon));
        }
        assert!(
            pipeline.atlas.edge > ATLAS_START_PX,
            "16 cells of {PX}px cannot fit {ATLAS_START_PX}², so the sheet must grow"
        );
        for icon in 0..16 {
            assert!(
                pipeline.entries.contains_key(&key(icon)),
                "glyph {icon} must still be placed after the growth"
            );
        }
    }

    #[test]
    fn symmetric_icons_are_centered_in_the_atlas() {
        let px = 48;
        for icon in [Icon::Add, Icon::Settings, Icon::Close] {
            let rgba = rasterize_icon(icon.svg(), px).expect("svg");
            let (min_x, min_y, max_x, max_y) = ink_bbox(&rgba, px);
            let cx = (min_x + max_x) as f32 / 2.0;
            let cy = (min_y + max_y) as f32 / 2.0;
            let mid = (px - 1) as f32 / 2.0;
            assert!(
                (cx - mid).abs() < 1.5,
                "{icon:?} horizontal center {cx} vs {mid}"
            );
            assert!(
                (cy - mid).abs() < 1.5,
                "{icon:?} vertical center {cy} vs {mid}"
            );
        }
    }
}

pub(super) struct IconPipelineTarget {
    uniform_bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    vertices: ManagedBuffer,
    vertex_capacity: usize,
    pending_vertices: Vec<IconVertex>,
    uploaded_vertices: Vec<IconVertex>,
    physical_size: [u32; 2],
    uploaded_size: Option<[u32; 2]>,
    frame_slots: Vec<FrameSlot>,
    atlas: Atlas,
    entries: HashMap<AtlasKey, AtlasEntry>,
    frame_keys: HashSet<AtlasKey>,
    exhausted: bool,
}

impl IconPipeline {
    pub(super) fn swap_target(
        &mut self,
        target: &mut Option<IconPipelineTarget>,
        device: &wgpu::Device,
    ) {
        let target = target.get_or_insert_with(|| {
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.icon.uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            IconPipelineTarget {
                uniform_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("nana.target.icon.bind"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
                    }],
                }),
                uniforms,
                vertices: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana.target.icon.vertices"),
                    size: (INITIAL_VERTICES * std::mem::size_of::<IconVertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })),
                vertex_capacity: INITIAL_VERTICES,
                pending_vertices: Vec::new(),
                uploaded_vertices: Vec::new(),
                physical_size: [0; 2],
                uploaded_size: None,
                frame_slots: Vec::new(),
                atlas: new_atlas(device, &self.atlas_layout, &self.sampler, ATLAS_START_PX),
                entries: HashMap::new(),
                frame_keys: HashSet::new(),
                exhausted: false,
            }
        });
        std::mem::swap(&mut self.uniform_bind_group, &mut target.uniform_bind_group);
        std::mem::swap(&mut self.uniforms, &mut target.uniforms);
        std::mem::swap(&mut self.vertices, &mut target.vertices);
        std::mem::swap(&mut self.vertex_capacity, &mut target.vertex_capacity);
        std::mem::swap(&mut self.pending_vertices, &mut target.pending_vertices);
        std::mem::swap(&mut self.uploaded_vertices, &mut target.uploaded_vertices);
        std::mem::swap(&mut self.physical_size, &mut target.physical_size);
        std::mem::swap(&mut self.uploaded_size, &mut target.uploaded_size);
        std::mem::swap(&mut self.frame_slots, &mut target.frame_slots);
        std::mem::swap(&mut self.atlas, &mut target.atlas);
        std::mem::swap(&mut self.entries, &mut target.entries);
        std::mem::swap(&mut self.frame_keys, &mut target.frame_keys);
        std::mem::swap(&mut self.exhausted, &mut target.exhausted);
    }
}
