//! The renderer's own text pipeline, shader and per-target GPU buffers.
//!
//! One program over one atlas bind group. A glyph is always the same 24-byte
//! instance — four vertices, its origin relative to its run, its rectangle in
//! the atlas — and everything about *how* that paragraph reaches the screen
//! lives in two side tables the instances only name:
//!
//! - [`TextRunGpu`]: the run's whole-pixel origin, its color, its opacity and
//!   which presentation it paints under. One per text draw.
//! - [`TextPresentationGpu`]: the transform and the clip. Deduplicated, because
//!   a shell's labels nearly all share one identity transform and one clip.
//!
//! That split is what makes moving, fading or recoloring text a patch of tens
//! of bytes instead of a rebuild of every glyph: [`super::entry`] hands the
//! same instance range back frame after frame and only rewrites a table row.
//!
//! The instances themselves are a storage buffer the draw does not walk. What
//! it walks is the index table: one `u32` per slot, in draw order, naming
//! where that slot's instance sits. That is what lets a paragraph that grew
//! out of its block be placed anywhere in the instance buffer without taking
//! its neighbours' bytes with it — the draw it belongs to follows the table,
//! and the table is rewritten at four bytes a slot where the instances would
//! cost twenty-four.
//!
//! Upright and transformed text are the same program because the difference
//! between them is two bits in the run: whether each corner goes through the
//! homography, and whether the sampler is bilinear. A page a rotated label
//! faulted in is the page an upright one hits.

use std::ops::Range;

use crate::gpu_work::{GpuWorkSink, ManagedBuffer};
use bytemuck::{Pod, Zeroable};

use super::atlas::GlyphAtlasManager;
use super::gamma::TextContrast;
use crate::scene_paint::clip;
use crate::scene_paint::color::orthographic;

/// Content type carried per glyph. Mirrors [`super::atlas::AtlasPageKind`];
/// the shader switches on it to pick which page the glyph came from.
pub(super) const CONTENT_MASK: u32 = 0;
pub(super) const CONTENT_COLOR: u32 = 1;
/// The instance carries its own color instead of inheriting the run's. Set for
/// a rich span, clear for the overwhelming majority of glyphs — which is what
/// lets a plain label's color change be a four-float write.
pub(super) const INSTANCE_OWN_COLOR: u32 = 2;
/// A [`CONTENT_COLOR`] glyph whose texels are subpixel coverage, not color.
pub(super) const INSTANCE_SUBPIXEL: u32 = 4;
/// Bits above these three are the run index.
pub(super) const INSTANCE_RUN_SHIFT: u32 = 3;

/// Bilinear sampling: the quad no longer lands on the texel grid.
pub(super) const RUN_LINEAR: u32 = 1;
/// The run carries a clip the scissor cannot express.
pub(super) const RUN_CLIP: u32 = 2;
/// More than a translation, so each corner goes through the homography.
pub(super) const RUN_PROJECT: u32 = 4;

const INITIAL_INSTANCES: usize = 512;
const INITIAL_RUNS: usize = 64;
const INITIAL_PRESENTATIONS: usize = 8;

/// The text program: `$prelude`, the shared vertex stage and `shade`, then
/// `$fragment`'s entry point.
macro_rules! text_shader {
    ($prelude:literal, $fragment:literal) => {
        concat!(
            $prelude,
            include_str!("../shader/color.wgsl"),
            include_str!("../shader/text_atlas.wgsl"),
            r#"
// One glyph, as `GlyphInstance` lays it out: 24 bytes.
struct GlyphInstance {
    origin: vec2<i32>,
    dim: u32,
    uv: u32,
    color: u32,
    control: u32,
}

@group(0) @binding(3)
var<storage, read> text_instances: array<GlyphInstance>;

struct VsIn {
    @builtin(vertex_index) vertex: u32,
    // The instance's slot, from the draw-order index table.
    @location(0) slot: u32,
}

struct VsOut {
    @invariant @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    // Screen-linear: the fragment's own paint position.
    @location(2) @interpolate(linear) world_pos: vec2<f32>,
    @location(3) @interpolate(flat) content: u32,
    // `presentation << 3 | flags`. Carried rather than re-read: the vertex
    // stage already has the run, and these are the same for every fragment of
    // a paragraph, so reading the row again per fragment would be a dependent
    // load for a value that cannot vary.
    @location(4) @interpolate(flat) run_flags: u32,
    // `color`, sRGB-encoded: what the coverage correction reads, the same
    // for every fragment of a glyph.
    @location(5) @interpolate(flat) encoded: vec3<f32>,
    // The glyph's cell on its page, in normalized coordinates: the taps a
    // shrunk glyph averages stay in it rather than read a neighbour.
    @location(6) @interpolate(flat) cell: vec4<f32>,
}

@vertex
fn vs_main(vertex: VsIn) -> VsOut {
    // `VACANT_INDEX`: no instance behind this slot. Outside the clip volume at
    // every corner, so the quad is culled before rasterizing.
    if vertex.slot == 0xffffffffu {
        var vacant: VsOut;
        vacant.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        return vacant;
    }
    let input = text_instances[vertex.slot];
    let run = text_runs[input.control >> 3u];
    // The page, then whether a color-page texel is subpixel coverage.
    let page = input.control & 1u;
    var content = page;
    if (input.control & 4u) != 0u {
        content = CONTENT_SUBPIXEL;
    }
    let width = input.dim & 0xffffu;
    let height = (input.dim & 0xffff0000u) >> 16u;
    let corner = vec2<u32>(vertex.vertex & 1u, (vertex.vertex >> 1u) & 1u);
    let offset = vec2<u32>(width, height) * corner;
    var local = vec2<f32>(input.origin + vec2<i32>(offset));
    let base = vec2<u32>(input.uv & 0xffffu, (input.uv & 0xffff0000u) >> 16u);
    var texel = vec2<f32>(base + offset);
    if (run.flags & RUN_PROJECT) != 0u {
        // Bitmap texels are raster px, so the quad and its atlas coordinates
        // grow alike.
        let grown = (vec2<f32>(corner) * 2.0 - 1.0) * text_edge_grow(run, local);
        local += grown;
        texel += grown;
    }

    var color = run.color;
    if (input.control & 2u) != 0u {
        color = unpack_srgb(input.color);
    }
    color.a = color.a * run.opacity;

    let world = text_world_position(run, local);
    let clip_w = text_clip_w(run, local);
    var out: VsOut;
    out.position = globals.transform * vec4<f32>(world * clip_w, 0.0, clip_w);
    out.color = color;
    // `atlas_uv` of one texel is the page's texel size.
    out.uv = texel * atlas_uv(vec2<u32>(1u), page);
    out.world_pos = world;
    out.content = content;
    out.run_flags = (run.presentation << 3u) | (run.flags & 7u);
    out.encoded = linear_to_srgb3(color.rgb);
    out.cell = vec4<f32>(atlas_uv(base, page), atlas_uv(base + vec2<u32>(width, height), page));
    return out;
}

// One device pixel's worth of `page` at `uv`, whose screen derivatives are
// `dx`/`dy`. A glyph is rasterized for its transform's larger scale, so an
// anisotropic one is shrunk along the other axis, where one bilinear tap per
// pixel would skip texels: that axis averages a grid of taps across the
// pixel's footprint. A tap past the glyph's `cell` reads its transparent
// gutter, never a neighbour.
fn footprint_sample(page: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, cell: vec4<f32>) -> vec4<f32> {
    let texels = vec2<f32>(textureDimensions(page));
    let footprint = vec2(length(dx * texels), length(dy * texels));
    let taps = clamp(ceil(footprint - 1.0 / 16.0), vec2(1.0), vec2(8.0));
    let half_texel = 0.5 / texels;
    var sum = vec4(0.0);
    for (var i = 0.0; i < taps.x; i += 1.0) {
        for (var j = 0.0; j < taps.y; j += 1.0) {
            let offset = ((i + 0.5) / taps.x - 0.5) * dx + ((j + 0.5) / taps.y - 0.5) * dy;
            let at = clamp(uv + offset, cell.xy - half_texel, cell.zw + half_texel);
            sum += textureSampleLevel(page, atlas_linear, at, 0.0);
        }
    }
    return sum / (taps.x * taps.y);
}

// What a fragment paints: the color, and the per-channel alpha it paints
// with. Equal channels for everything but subpixel coverage.
struct TextShade {
    color: vec3<f32>,
    alpha: vec4<f32>,
}

fn shade(input: VsOut) -> TextShade {
    // Before any branch: derivatives need every fragment of the quad.
    let uv_dx = dpdx(input.uv);
    let uv_dy = dpdy(input.uv);
    let flags = input.run_flags & 7u;
    var clip_cover = 1.0;
    if (flags & RUN_CLIP) != 0u {
        let presentation = text_presentations[input.run_flags >> 3u];
        clip_cover = fragment_clip_coverage(
            input.world_pos,
            presentation.clip_rect,
            presentation.clip_inv_abcd,
            presentation.clip_inv_ef.xy,
            presentation.clip_inv_ef.z,
            presentation.polygon_count,
            presentation.polygon[0],
            presentation.polygon[1],
            presentation.polygon[2],
            presentation.polygon[3],
            1.0,
        );
        if clip_cover <= 0.0 {
            discard;
        }
    }
    let linear = (flags & RUN_LINEAR) != 0u;
    var out: TextShade;
    if input.content == CONTENT_MASK {
        var coverage = 0.0;
        if linear {
            coverage = footprint_sample(mask_atlas, input.uv, uv_dx, uv_dy, input.cell).x;
        } else {
            coverage = textureSampleLevel(mask_atlas, atlas_linear, input.uv, 0.0).x;
        }
        // One coverage against the foreground's luma, as DirectWrite's
        // grayscale blend does.
        let fg = input.encoded;
        let corrected = corrected_coverage(
            vec3<f32>(coverage),
            fg,
            vec3<f32>(dot(fg, vec3<f32>(0.25, 0.5, 0.25))),
            vec3<f32>(dot(fg, vec3<f32>(0.2126, 0.7152, 0.0722))),
            globals.contrast.x,
        ).x;
        out.color = input.color.rgb;
        out.alpha = vec4<f32>(input.color.a * corrected * clip_cover);
        return out;
    }
    var sampled = vec4<f32>(0.0);
    if linear {
        sampled = footprint_sample(color_atlas, input.uv, uv_dx, uv_dy, input.cell);
    } else {
        sampled = textureSampleLevel(color_atlas, atlas_linear, input.uv, 0.0);
    }
    if input.content == CONTENT_SUBPIXEL {
        // Coverage per subpixel, stored encoded so the page's sRGB decode
        // hands back the rasterizer's own values.
        // Each subpixel against its own channel of the foreground, as
        // ClearType's blend does.
        let fg = input.encoded;
        let corrected = corrected_coverage(sampled.rgb, fg, fg, fg, globals.contrast.y);
        out.color = input.color.rgb;
        out.alpha = vec4<f32>(corrected, max(corrected.r, max(corrected.g, corrected.b)))
            * (input.color.a * clip_cover);
        return out;
    }
    // A color bitmap carries its own color; only the run's alpha applies, so a
    // faded or shadowed emoji fades instead of painting at full strength.
    out.color = sampled.rgb;
    out.alpha = vec4<f32>(sampled.a * input.color.a * clip_cover);
    return out;
}
"#,
            $fragment,
        )
    };
}

/// The program for targets that blend one output. Subpixel glyphs are never
/// resolved for it; were one drawn, it would paint as grayscale.
const TEXT_SHADER: &str = text_shader!(
    "",
    r#"
@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let shaded = shade(input);
    return vec4<f32>(shaded.color, shaded.alpha.a);
}
"#
);

/// The program for subpixel text: the second blend source carries a coverage
/// per color channel, so the blend unit mixes each subpixel on its own.
/// Every other glyph gives the same alpha to all four, which is exactly
/// straight alpha blending.
const TEXT_SHADER_DUAL_SOURCE: &str = text_shader!(
    "enable dual_source_blending;\n",
    r#"
struct DualOut {
    @location(0) @blend_src(0) color: vec4<f32>,
    @location(0) @blend_src(1) alpha: vec4<f32>,
}

@fragment
fn fs_main(input: VsOut) -> DualOut {
    let shaded = shade(input);
    var out: DualOut;
    out.color = vec4<f32>(shaded.color, 1.0);
    out.alpha = shaded.alpha;
    return out;
}
"#
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16],
    /// [`TextContrast::to_gpu`].
    contrast: [f32; 8],
}

/// One glyph: 24 bytes, against the 60 a vertex-per-corner quad would cost.
/// This compactness is the whole point of the instanced path, and keeping
/// presentation out of it is what lets the bytes be retained across frames.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(super) struct GlyphInstance {
    /// Quad top-left in physical pixels, relative to the run's origin.
    origin: [i32; 2],
    /// `width | height << 16`, in physical pixels.
    dim: u32,
    /// Atlas texel `x | y << 16`.
    uv: u32,
    /// sRGB `a << 24 | r << 16 | g << 8 | b`, linearized in the shader. Only
    /// read when [`INSTANCE_OWN_COLOR`] is set.
    color: u32,
    /// `content | own-color | run << 2`.
    control: u32,
}

/// One text draw's presentation. 48 bytes; see the module docs.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(super) struct TextRunGpu {
    pub origin: [f32; 2],
    pub presentation: u32,
    pub flags: u32,
    /// Linear RGB with its own alpha, opacity not yet applied.
    pub color: [f32; 4],
    pub opacity: f32,
    /// Physical px per logical px the instances were resolved at: the device
    /// scale, times the raster step a scaling transform earned the entry.
    /// Only a projected run reads it, to take its corners back to logical
    /// space before the homography.
    pub raster: f32,
    pad: [f32; 2],
}

/// The transform and clip a run paints under. 160 bytes, deduplicated.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(super) struct TextPresentationGpu {
    affine: [f32; 4],
    project: [f32; 4],
    clip_rect: [f32; 4],
    clip_inv_abcd: [f32; 4],
    clip_inv_ef: [f32; 4],
    polygon: [[f32; 4]; 4],
    polygon_count: u32,
    pad: u32,
    /// The whole-pixel part of a translation, in physical px. A translated
    /// run's origin is kept relative to it, so a scroll that lands on whole
    /// pixels rewrites this one shared row instead of every run under it.
    translate: [f32; 2],
}

fn text_layout_key() -> u64 {
    let vertex_fragment = &[
        nana_gpu::ShaderStage::Vertex,
        nana_gpu::ShaderStage::Fragment,
    ];
    let vertex = &[nana_gpu::ShaderStage::Vertex];
    let globals = nana_gpu::ResourceTable::new(vec![
        nana_gpu::LogicalBinding::new(
            0,
            nana_gpu::LogicalBindingType::UniformBuffer,
            vertex_fragment,
        )
        .min_size(std::mem::size_of::<Globals>() as u64),
        nana_gpu::LogicalBinding::new(
            1,
            nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
            vertex_fragment,
        )
        .min_size(std::mem::size_of::<TextRunGpu>() as u64),
        nana_gpu::LogicalBinding::new(
            2,
            nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
            vertex_fragment,
        )
        .min_size(std::mem::size_of::<TextPresentationGpu>() as u64),
        nana_gpu::LogicalBinding::new(
            3,
            nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
            vertex,
        )
        .min_size(INSTANCE_BYTES),
    ])
    .expect("text globals table")
    .layout_key();
    let atlas = nana_gpu::ResourceTable::new(vec![
        nana_gpu::LogicalBinding::new(
            0,
            nana_gpu::LogicalBindingType::SampledTexture,
            vertex_fragment,
        ),
        nana_gpu::LogicalBinding::new(
            1,
            nana_gpu::LogicalBindingType::SampledTexture,
            vertex_fragment,
        ),
        nana_gpu::LogicalBinding::new(2, nana_gpu::LogicalBindingType::Sampler, vertex_fragment),
        nana_gpu::LogicalBinding::new(3, nana_gpu::LogicalBindingType::Sampler, vertex_fragment),
    ])
    .expect("text atlas table")
    .layout_key();
    globals ^ atlas.rotate_left(17)
}

/// One draw: a contiguous span of the index table whose instances sample one
/// pair of pages.
///
/// Spans are split rather than grouped when a page changes, so glyph order
/// inside a run is document order even across a page boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DrawSegment {
    pub mask_page: u32,
    pub color_page: u32,
    pub first: u32,
    pub count: u32,
}

/// GPU state that belongs to one render target: the buffers this frame's
/// glyphs and their presentation land in, and the projection they are placed
/// against.
pub(super) struct TextTargetGpu {
    /// Storage, read through `indices`; its order is the arena's, not the
    /// draw's.
    instances: ManagedBuffer,
    instance_capacity: usize,
    /// The draw-order index table, bound as the per-instance vertex buffer.
    indices: ManagedBuffer,
    index_capacity: usize,
    /// Where a frame's blocks and ranges wait to be copied into place: a ring,
    /// so a target painted twice before one submit gives each paint its own
    /// stretch of it. Only the glyphs: the run and presentation tables are
    /// still written in place, so such a pair of paints shares the second
    /// one's positions and colours.
    staging: ManagedBuffer,
    staging_capacity: u64,
    staging_cursor: u64,
    runs: ManagedBuffer,
    run_capacity: usize,
    presentations: ManagedBuffer,
    presentation_capacity: usize,
    globals: wgpu::Buffer,
    globals_bind_group: Option<wgpu::BindGroup>,
    uploaded_size: Option<[u32; 2]>,
    /// GPU allocations this target could not avoid this frame.
    allocations: usize,
}

pub(super) struct TextGpu {
    pipeline: wgpu::RenderPipeline,
    /// The dual-source program, while subpixel text is on.
    dual_source: Option<wgpu::RenderPipeline>,
    layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    globals_layout: wgpu::BindGroupLayout,
    /// The platform's text parameters, read once when the painter is made.
    contrast: TextContrast,
    policy: Option<nana_gpu::GpuDeviceState>,
    /// The most glyph slots one target's instance buffer may hold on this
    /// device. It is bound whole as a storage buffer, so the binding limit
    /// applies as well as the buffer one — 128 MiB, about 5.6 million slots,
    /// under WebGPU's defaults.
    instance_slots: u32,
}

impl TextGpu {
    pub(super) fn new_with_policy(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas: &GlyphAtlasManager,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Self {
        Self::new_inner(device, format, atlas, policy)
    }

    fn new_inner(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas: &GlyphAtlasManager,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Self {
        let globals_layout = if let Some(policy) = policy {
            let vertex_fragment = &[
                nana_gpu::ShaderStage::Vertex,
                nana_gpu::ShaderStage::Fragment,
            ];
            let vertex = &[nana_gpu::ShaderStage::Vertex];
            let table = nana_gpu::ResourceTable::new(vec![
                nana_gpu::LogicalBinding::new(
                    0,
                    nana_gpu::LogicalBindingType::UniformBuffer,
                    vertex_fragment,
                )
                .min_size(std::mem::size_of::<Globals>() as u64),
                nana_gpu::LogicalBinding::new(
                    1,
                    nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                    vertex_fragment,
                )
                .min_size(std::mem::size_of::<TextRunGpu>() as u64),
                nana_gpu::LogicalBinding::new(
                    2,
                    nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                    vertex_fragment,
                )
                .min_size(std::mem::size_of::<TextPresentationGpu>() as u64),
                nana_gpu::LogicalBinding::new(
                    3,
                    nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                    vertex,
                )
                .min_size(INSTANCE_BYTES),
            ])
            .expect("text globals logical table is valid")
            .for_generation(policy.generation());
            let logical =
                nana_gpu::__framework::logical_layout(device, policy.generation(), &table)
                    .expect("text globals logical layout matches the device");
            nana_gpu::__framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.text.globals"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<Globals>() as u64
                            ),
                        },
                        count: None,
                    },
                    storage_entry(
                        1,
                        wgpu::ShaderStages::VERTEX_FRAGMENT,
                        std::mem::size_of::<TextRunGpu>() as u64,
                    ),
                    storage_entry(
                        2,
                        wgpu::ShaderStages::VERTEX_FRAGMENT,
                        std::mem::size_of::<TextPresentationGpu>() as u64,
                    ),
                    // Only the vertex stage reads a glyph: every fragment of it
                    // gets what it needs through the varyings.
                    storage_entry(3, wgpu::ShaderStages::VERTEX, INSTANCE_BYTES),
                ],
            })
        };
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.text.pipeline"),
            bind_group_layouts: &[Some(&globals_layout), Some(atlas.layout())],
            immediate_size: 0,
        });
        let create = || {
            build_pipeline(
                device,
                &layout,
                format,
                TEXT_SHADER,
                wgpu::BlendState::ALPHA_BLENDING,
            )
        };
        let pipeline = if let Some(policy) = policy {
            nana_gpu::__framework::render_pipeline_state(
                policy,
                nana_gpu::PipelineKey {
                    generation: policy.generation(),
                    target_format: nana_gpu::__framework::format_from_wgpu(format),
                    sample_count: 1,
                    shader: 0x7465_7874_6d61_696e,
                    layout: text_layout_key(),
                    material: 0,
                    primitive: 1,
                    blend: 1,
                    depth: 0,
                    vertex_layout: INDEX_BYTES,
                },
                create,
            )
            .expect("text pipeline uses this context generation")
        } else {
            create()
        };
        let limits = device.limits();
        let instance_bytes = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        Self {
            pipeline,
            dual_source: None,
            layout,
            format,
            globals_layout,
            contrast: TextContrast::system(),
            policy: policy.cloned(),
            instance_slots: u32::try_from(instance_bytes / INSTANCE_BYTES).unwrap_or(u32::MAX),
        }
    }

    pub(super) fn instance_slots(&self) -> u32 {
        self.instance_slots
    }

    /// Stand in for a device with a smaller binding limit.
    #[cfg(test)]
    pub(super) fn set_instance_slots(&mut self, slots: u32) {
        self.instance_slots = slots;
    }

    /// Whether subpixel glyphs can be drawn: the device blends two sources.
    pub(super) fn supports_subpixel(device: &wgpu::Device) -> bool {
        device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    }

    /// Switch to the program that can draw subpixel glyphs, or back. Every
    /// glyph draws through the one that is current, so the switch is a whole
    /// frame's worth of text at once. Only for a device that
    /// [`supports_subpixel`](Self::supports_subpixel).
    pub(super) fn set_subpixel(&mut self, device: &wgpu::Device, enabled: bool) {
        // `src0 * src1 + dst * (1 - src1)`, channel by channel.
        let per_channel = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Src1,
            dst_factor: wgpu::BlendFactor::OneMinusSrc1,
            operation: wgpu::BlendOperation::Add,
        };
        self.dual_source = enabled.then(|| {
            let create = || {
                build_pipeline(
                    device,
                    &self.layout,
                    self.format,
                    TEXT_SHADER_DUAL_SOURCE,
                    wgpu::BlendState {
                        color: per_channel,
                        alpha: per_channel,
                    },
                )
            };
            if let Some(policy) = &self.policy {
                nana_gpu::__framework::render_pipeline_state(
                    policy,
                    nana_gpu::PipelineKey {
                        generation: policy.generation(),
                        target_format: nana_gpu::__framework::format_from_wgpu(self.format),
                        sample_count: 1,
                        shader: 0x7465_7874_6475_616c,
                        layout: text_layout_key(),
                        material: 1,
                        primitive: 1,
                        blend: 3,
                        depth: 0,
                        vertex_layout: INDEX_BYTES,
                    },
                    create,
                )
                .expect("dual-source text pipeline uses this context generation")
            } else {
                create()
            }
        });
    }

    pub(super) fn new_target(&self, device: &wgpu::Device) -> TextTargetGpu {
        TextTargetGpu::new(device, &self.globals_layout)
    }

    /// Write this frame's projection, arena blocks, index ranges and
    /// presentation tables.
    ///
    /// Only what changed: the arena and the index table are written through
    /// the coalesced ranges [`super::TextPipeline::flush_runs`] staged, and the
    /// two presentation tables through a block diff against what this target
    /// already holds. A repaint of unchanged text costs no queue traffic at
    /// all.
    pub(super) fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &mut TextTargetGpu,
        frame: &FrameUpload<'_>,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> TextUploadBytes {
        let mut rebind = false;
        let physical_size = frame.physical_size;
        if target.uploaded_size != Some(physical_size) {
            let globals = Globals {
                transform: orthographic(physical_size[0], physical_size[1]),
                contrast: self.contrast.to_gpu(),
            };
            let global_bytes = bytemuck::bytes_of(&globals);
            if let Some(work) = work {
                work.write_buffer(queue, &target.globals, 0, global_bytes);
            } else {
                queue.write_buffer(&target.globals, 0, global_bytes);
            }
            target.uploaded_size = Some(physical_size);
        }
        let mut bytes = TextUploadBytes::default();
        // Both ways: the arena and the order only change capacity inside a
        // repack, which writes every block it places, so the replacement
        // never has to carry bytes over from the buffer it replaces.
        // Shrinking is what keeps a list that was once ten thousand rows long
        // from holding that much GPU memory for the rest of the session.
        if frame.arena_capacity != 0 && frame.arena_capacity != target.instance_capacity as u32 {
            target.instance_capacity = frame.arena_capacity as usize;
            let size = target.instance_capacity as u64 * INSTANCE_BYTES;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | READ_BACK;
            if let Some(work) = work {
                work.replace_buffer(
                    device,
                    &mut target.instances,
                    size,
                    usage,
                    "nana-ui.scene.text.instances",
                );
            } else {
                target.instances = ManagedBuffer::new(instance_buffer(device, size));
            }
            target.allocations += 1;
            rebind = true;
        }
        if frame.order_capacity != 0 && frame.order_capacity != target.index_capacity as u32 {
            target.index_capacity = frame.order_capacity as usize;
            let size = target.index_capacity as u64 * INDEX_BYTES;
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | READ_BACK;
            if let Some(work) = work {
                work.replace_buffer(
                    device,
                    &mut target.indices,
                    size,
                    usage,
                    "nana-ui.scene.text.indices",
                );
            } else {
                target.indices = ManagedBuffer::new(index_buffer(device, size));
            }
            target.allocations += 1;
        }
        // Every block and range this frame moved, in one write to a staging
        // ring and one copy each. A `queue.write_buffer` per write costs a
        // staging allocation of its own — about fifty thousand instructions on
        // Metal, far more than the bytes — and a frame that rebuilds a hundred
        // labels would make a hundred of them.
        //
        // Pending queue writes land at the next submit, ahead of its command
        // buffers and after everything submitted before, so the copies of the
        // last frame have read their part of the ring by the time this frame's
        // bytes arrive. The copies are recorded in `encoder`, ahead of the
        // passes that read the buffers.
        let instance_bytes: &[u8] = bytemuck::cast_slice(frame.staging);
        let index_bytes: &[u8] = bytemuck::cast_slice(frame.index_staging);
        let staged = (instance_bytes.len() + index_bytes.len()) as u64;
        if staged != 0 {
            // Keep surface reconfiguration out of the mapped queue write and
            // the encoder copies that consume it.
            let _submission = work.and_then(GpuWorkSink::lock_submission);
            // `None` from wgpu is a staging allocation it could not make — a
            // lost device, or out of memory — already reported through the
            // device's error sink. The frame then tries buffers of its own.
            let ring = target.stage(device, work, staged).and_then(|at| {
                let view = queue.write_buffer_with(
                    &target.staging,
                    at,
                    wgpu::BufferSize::new(staged).expect("nonzero"),
                )?;
                Some((at, view))
            });
            let [instances, indices] = match ring {
                Some((at, mut view)) => {
                    // One call: each has the fixed cost, however few bytes.
                    view.slice(..instance_bytes.len())
                        .copy_from_slice(instance_bytes);
                    view.slice(instance_bytes.len()..)
                        .copy_from_slice(index_bytes);
                    [
                        Some((target.staging.clone(), at)),
                        Some((target.staging.clone(), at + instance_bytes.len() as u64)),
                    ]
                }
                // A frame too large for the ring — the first one of a long
                // list, an arena repack — gets buffers of its own, freed once
                // their copies have run, so the ring never grows to hold it.
                // One per destination: a frame writes each slot at most once,
                // so neither is larger than the buffer it fills, which the
                // device already allowed — the two together can be past
                // `max_buffer_size`.
                None => [instance_bytes, index_bytes].map(|part| {
                    let source = one_off_staging(device, part, &mut bytes.lost)?;
                    target.allocations += 1;
                    Some((source, 0))
                }),
            };
            if let Some((source, at)) = instances {
                bytes.instances += copy_writes(
                    encoder,
                    &source,
                    at,
                    frame.writes,
                    INSTANCE_BYTES,
                    &target.instances,
                );
            }
            if let Some((source, at)) = indices {
                bytes.indices += copy_writes(
                    encoder,
                    &source,
                    at,
                    frame.index_writes,
                    INDEX_BYTES,
                    &target.indices,
                );
            }
        }
        if !frame.runs.is_empty() {
            let mut dirty = frame.run_dirty.clone();
            if frame.runs.len() > target.run_capacity {
                target.run_capacity = frame.runs.len().next_power_of_two();
                let size = (target.run_capacity * std::mem::size_of::<TextRunGpu>()) as u64;
                let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
                if let Some(work) = work {
                    work.replace_buffer(
                        device,
                        &mut target.runs,
                        size,
                        usage,
                        "nana-ui.scene.text.runs",
                    );
                } else {
                    target.runs =
                        ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("nana-ui.scene.text.runs"),
                            size,
                            usage,
                            mapped_at_creation: false,
                        }));
                }
                target.allocations += 1;
                // The new buffer holds nothing, so every row is owed.
                dirty = Some(0..frame.runs.len() as u32);
                rebind = true;
            }
            if let Some(dirty) = dirty {
                let end = (dirty.end as usize).min(frame.runs.len());
                let start = (dirty.start as usize).min(end);
                let data: &[u8] = bytemuck::cast_slice(&frame.runs[start..end]);
                if !data.is_empty() {
                    let offset = (start * std::mem::size_of::<TextRunGpu>()) as u64;
                    if let Some(work) = work {
                        work.write_buffer(queue, &target.runs, offset, data);
                    } else {
                        queue.write_buffer(&target.runs, offset, data);
                    }
                    bytes.presentation += data.len();
                }
            }
        }
        if !frame.presentations.is_empty() {
            let mut previous: &[TextPresentationGpu] = frame.uploaded_presentations;
            if frame.presentations.len() > target.presentation_capacity {
                target.presentation_capacity = frame.presentations.len().next_power_of_two();
                let size = (target.presentation_capacity
                    * std::mem::size_of::<TextPresentationGpu>()) as u64;
                let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
                if let Some(work) = work {
                    work.replace_buffer(
                        device,
                        &mut target.presentations,
                        size,
                        usage,
                        "nana-ui.scene.text.presentations",
                    );
                } else {
                    target.presentations =
                        ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("nana-ui.scene.text.presentations"),
                            size,
                            usage,
                            mapped_at_creation: false,
                        }));
                }
                target.allocations += 1;
                previous = &[];
                rebind = true;
            }
            bytes.presentation += crate::scene_paint::buffer_upload::upload_changed_with_work(
                queue,
                &target.presentations,
                bytemuck::cast_slice(previous),
                bytemuck::cast_slice(frame.presentations),
                work,
            );
        }
        if rebind {
            target.globals_bind_group = Some(target.bind(device, &self.globals_layout));
        }
        if let Some(work) = work {
            // Runs and presentations went through the sink, which counted
            // them; the ring's copies did not.
            work.record_upload(bytes.instances + bytes.indices);
        }
        bytes
    }

    pub(super) fn draw_segment(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        target: &TextTargetGpu,
        segment: &DrawSegment,
        bind_group: &wgpu::BindGroup,
    ) {
        if segment.count == 0 {
            return;
        }
        pass.set_pipeline(self.dual_source.as_ref().unwrap_or(&self.pipeline));
        let Some(globals) = target.globals_bind_group.as_ref() else {
            return;
        };
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, bind_group, &[]);
        pass.set_vertex_buffer(0, target.indices.slice(..));
        pass.draw(0..4, segment.first..segment.first + segment.count);
    }
}

/// What one frame hands the GPU. Grouped because the arrays travel together
/// and their previous contents are the only thing that decides whether a byte
/// moves at all.
pub(super) struct FrameUpload<'a> {
    /// The target's size in physical pixels, which the projection is built for.
    pub physical_size: [u32; 2],
    /// Instance slots the arena must hold; a different count replaces the
    /// buffer.
    pub arena_capacity: u32,
    pub writes: &'a [ArenaWrite],
    pub staging: &'a [GlyphInstance],
    /// Index slots the draw order must hold, likewise.
    pub order_capacity: u32,
    pub index_writes: &'a [ArenaWrite],
    pub index_staging: &'a [u32],
    pub runs: &'a [TextRunGpu],
    /// Rows that changed, as one span.
    pub run_dirty: Option<Range<u32>>,
    pub presentations: &'a [TextPresentationGpu],
    pub uploaded_presentations: &'a [TextPresentationGpu],
}

/// One coalesced run of arena or index slots to write, staged contiguously.
#[derive(Clone, Debug)]
pub(super) struct ArenaWrite {
    pub offset: u32,
    pub staged: Range<u32>,
}

/// Bytes written, split the way [`super::TextGlyphCounters`] reports them:
/// glyph geometry, the draw order over it, and the presentation tables that
/// only say where it goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TextUploadBytes {
    pub instances: usize,
    pub indices: usize,
    pub presentation: usize,
    /// The frame's blocks and ranges could not be staged, so the instance and
    /// index buffers do not hold what the arena and the order say they do.
    pub lost: bool,
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    source: &'static str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("nana-ui.scene.text.shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(source)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("nana-ui.scene.text.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: INDEX_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array!(0 => Uint32),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

const INSTANCE_BYTES: u64 = std::mem::size_of::<GlyphInstance>() as u64;
const INDEX_BYTES: u64 = std::mem::size_of::<u32>() as u64;

fn instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.text.instances"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | READ_BACK,
        mapped_at_creation: false,
    })
}

/// Tests read the instance and index buffers back to check the copies that
/// fill them; nothing else ever reads them on the CPU.
const READ_BACK: wgpu::BufferUsages = if cfg!(test) {
    wgpu::BufferUsages::COPY_SRC
} else {
    wgpu::BufferUsages::empty()
};

/// A buffer holding `bytes` to copy from; `None` when there are none, or
/// when the device would not map one (lost, or out of memory), which also
/// sets `lost`.
fn one_off_staging(device: &wgpu::Device, bytes: &[u8], lost: &mut bool) -> Option<wgpu::Buffer> {
    if bytes.is_empty() {
        return None;
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.text.staging.frame"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    let Ok(mut view) = buffer.slice(..).get_mapped_range_mut() else {
        *lost = true;
        return None;
    };
    view.copy_from_slice(bytes);
    drop(view);
    buffer.unmap();
    Some(buffer)
}

/// Copy each staged run from `source`, whose staging starts at `base`, to its
/// place in `destination`, `stride` bytes a slot. Returns the bytes copied.
fn copy_writes(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Buffer,
    base: u64,
    writes: &[ArenaWrite],
    stride: u64,
    destination: &wgpu::Buffer,
) -> usize {
    let mut copied = 0;
    for write in writes {
        let len = u64::from(write.staged.end - write.staged.start) * stride;
        encoder.copy_buffer_to_buffer(
            source,
            base + u64::from(write.staged.start) * stride,
            destination,
            u64::from(write.offset) * stride,
            len,
        );
        copied += len as usize;
    }
    copied
}

/// Bytes a target's staging ring starts at, and the most it grows to.
const MIN_STAGING: u64 = 64 * 1024;
const MAX_STAGING: u64 = 1024 * 1024;

fn staging_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.text.staging"),
        size,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn index_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.text.indices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | READ_BACK,
        mapped_at_creation: false,
    })
}

fn storage_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    min: u64,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(min),
        },
        count: None,
    }
}

impl TextTargetGpu {
    /// Room for `len` bytes in the staging ring, and where it starts. The
    /// ring is kept at least four frames of this size, so the stretch a
    /// second paint before the same submit takes does not wrap onto the
    /// first one's. `None` when that would take more than [`MAX_STAGING`]:
    /// the ring stays sized for the frames a steady shell makes.
    fn stage(
        &mut self,
        device: &wgpu::Device,
        work: Option<&GpuWorkSink>,
        len: u64,
    ) -> Option<u64> {
        let wanted = len.saturating_mul(4);
        if wanted > MAX_STAGING {
            return None;
        }
        if wanted > self.staging_capacity {
            self.staging_capacity = wanted.next_power_of_two().max(MIN_STAGING);
            if let Some(work) = work {
                work.replace_buffer(
                    device,
                    &mut self.staging,
                    self.staging_capacity,
                    wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    "nana-ui.scene.text.staging",
                );
            } else {
                self.staging = ManagedBuffer::new(staging_buffer(device, self.staging_capacity));
            }
            self.staging_cursor = 0;
            self.allocations += 1;
        }
        if self.staging_cursor + len > self.staging_capacity {
            self.staging_cursor = 0;
        }
        let at = self.staging_cursor;
        self.staging_cursor = at + len;
        Some(at)
    }

    /// What the instance and index buffers hold, once the queue is idle.
    #[cfg(test)]
    pub(super) fn read_back(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (Vec<GlyphInstance>, Vec<u32>) {
        let read = |source: &wgpu::Buffer| {
            let size = source.size();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.text.read_back"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nana-ui.scene.text.read_back"),
            });
            encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, size);
            queue.submit([encoder.finish()]);
            buffer.slice(..).map_async(wgpu::MapMode::Read, |result| {
                result.expect("read back");
            });
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the copy completes");
            buffer
                .slice(..)
                .get_mapped_range()
                .expect("mapped for reading")
                .to_vec()
        };
        (
            bytemuck::pod_collect_to_vec(&read(&self.instances)),
            bytemuck::pod_collect_to_vec(&read(&self.indices)),
        )
    }

    pub(super) fn take_allocations(&mut self) -> usize {
        std::mem::take(&mut self.allocations)
    }

    fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout) -> Self {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.text.globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut target = Self {
            instances: ManagedBuffer::new(instance_buffer(
                device,
                INITIAL_INSTANCES as u64 * INSTANCE_BYTES,
            )),
            instance_capacity: INITIAL_INSTANCES,
            indices: ManagedBuffer::new(index_buffer(
                device,
                INITIAL_INSTANCES as u64 * INDEX_BYTES,
            )),
            index_capacity: INITIAL_INSTANCES,
            staging: ManagedBuffer::new(staging_buffer(device, MIN_STAGING)),
            staging_capacity: MIN_STAGING,
            staging_cursor: 0,
            runs: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.text.runs"),
                size: (INITIAL_RUNS * std::mem::size_of::<TextRunGpu>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            run_capacity: INITIAL_RUNS,
            presentations: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.text.presentations"),
                size: (INITIAL_PRESENTATIONS * std::mem::size_of::<TextPresentationGpu>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            presentation_capacity: INITIAL_PRESENTATIONS,
            // Replaced below, once the buffers the real layout needs exist.
            globals_bind_group: None,
            globals,
            uploaded_size: None,
            allocations: 0,
        };
        target.globals_bind_group = Some(target.bind(device, globals_layout));
        target
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.text.globals.bind"),
            layout: globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.runs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.presentations.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.instances.as_entire_binding(),
                },
            ],
        })
    }
}

impl GlyphInstance {
    pub(super) fn new(
        origin: [i32; 2],
        size: [u32; 2],
        texel: [u32; 2],
        color: u32,
        control: u32,
    ) -> Self {
        Self {
            origin,
            dim: (size[0] & 0xffff) | ((size[1] & 0xffff) << 16),
            uv: (texel[0] & 0xffff) | ((texel[1] & 0xffff) << 16),
            color,
            control,
        }
    }

    /// Rebind this glyph to `run` without touching the geometry it resolved
    /// to. What a retained entry does when it is drawn a second time under a
    /// different presentation.
    pub(super) fn with_run(self, run: u32) -> Self {
        Self {
            control: (self.control & ((1 << INSTANCE_RUN_SHIFT) - 1)) | (run << INSTANCE_RUN_SHIFT),
            ..self
        }
    }

    /// Re-point this glyph at the rectangle its atlas handle now names.
    pub(super) fn with_placement(self, origin: [u32; 2], size: [u32; 2]) -> Self {
        Self {
            dim: (size[0] & 0xffff) | ((size[1] & 0xffff) << 16),
            uv: (origin[0] & 0xffff) | ((origin[1] & 0xffff) << 16),
            ..self
        }
    }

    /// The atlas rectangle this glyph samples, or `None` for a glyph that
    /// covers nothing.
    #[cfg(test)]
    pub(super) fn placement(&self) -> Option<([u32; 2], [u32; 2])> {
        (self.dim != 0).then_some((
            [self.uv & 0xffff, self.uv >> 16],
            [self.dim & 0xffff, self.dim >> 16],
        ))
    }

    /// Keep the slot but cover nothing: the placement this glyph named is gone
    /// and sampling whatever now owns it would draw a different letter.
    pub(super) fn vacated(self) -> Self {
        Self { dim: 0, ..self }
    }

    /// A glyph that covers nothing. Fills the slack a size class leaves after
    /// an entry's glyphs, which the entry's index range still names, so
    /// neighbouring entries batch into one draw across it.
    pub(super) const VACANT: Self = Self {
        origin: [0, 0],
        dim: 0,
        uv: 0,
        color: 0,
        control: 0,
    };
}

impl TextRunGpu {
    /// A row nothing names: a slot whose entry has retired, or one the table
    /// grew past. Fully transparent, so a stray instance could only draw
    /// nothing.
    pub(super) const VACANT: Self = Self {
        origin: [0.0; 2],
        presentation: 0,
        flags: 0,
        color: [0.0; 4],
        opacity: 0.0,
        raster: 1.0,
        pad: [0.0; 2],
    };

    pub(super) fn new(
        origin: [f32; 2],
        presentation: u32,
        flags: u32,
        color: [f32; 4],
        opacity: f32,
        raster: f32,
    ) -> Self {
        Self {
            origin,
            presentation,
            flags,
            color,
            opacity,
            raster,
            pad: [0.0; 2],
        }
    }
}

impl TextPresentationGpu {
    pub(super) fn new(
        affine: [f32; 6],
        persp: [f32; 2],
        clip: &clip::FragmentClip,
        scale: f32,
    ) -> Self {
        let mut polygon = [[0.0f32; 4]; 4];
        for (index, slot) in polygon.iter_mut().enumerate() {
            let [ax, ay] = clip.polygon[index * 2];
            let [bx, by] = clip.polygon[index * 2 + 1];
            *slot = [ax, ay, bx, by];
        }
        Self {
            affine: [affine[0], affine[1], affine[2], affine[3]],
            project: [affine[4], affine[5], persp[0], persp[1]],
            clip_rect: clip.rect,
            clip_inv_abcd: clip.inv_abcd,
            clip_inv_ef: [clip.inv_ef[0], clip.inv_ef[1], clip.corner_radius, scale],
            polygon,
            polygon_count: u32::from(clip.polygon_count),
            pad: 0,
            translate: whole_translation(affine, scale),
        }
    }

    /// Bit pattern, so two runs that present identically share one row.
    pub(super) fn to_bits(self) -> [u32; 40] {
        let mut bits = [0u32; 40];
        let floats: [f32; 36] = bytemuck::cast([
            self.affine,
            self.project,
            self.clip_rect,
            self.clip_inv_abcd,
            self.clip_inv_ef,
            self.polygon[0],
            self.polygon[1],
            self.polygon[2],
            self.polygon[3],
        ]);
        for (slot, value) in bits.iter_mut().zip(floats.iter()) {
            *slot = value.to_bits();
        }
        bits[36] = self.polygon_count;
        bits[37] = self.translate[0].to_bits();
        bits[38] = self.translate[1].to_bits();
        bits
    }
}

/// The whole physical pixels of `affine`'s translation at `scale`.
///
/// What a translated run's origin is relative to: the CPU subtracts it when it
/// writes the run row, the vertex stage adds it back from the presentation
/// row, and both read this one value, so the two cannot round it apart.
///
/// Nearest, not `floor`: the painter snaps a translation to whole device
/// pixels in logical px, and at 110%, 120%, 175% and other scales
/// `k / scale * scale` comes back as `k - ulp` for some `k`. Floored, the
/// whole part would flip between `k - 1` and `k` and a label's phase between
/// 0.9999999 and 0 — a re-resolve on every frame the float happened to land
/// low.
pub(super) fn whole_translation(affine: [f32; 6], scale: f32) -> [f32; 2] {
    [(affine[4] * scale).round(), (affine[5] * scale).round()]
}

/// sRGB `a<<24 | r<<16 | g<<8 | b`, the form the instance and the retained
/// placement both carry.
pub(super) fn pack_srgb(color: [f32; 4]) -> u32 {
    let [r, g, b, a] = crate::scene_paint::color::to_rgba8(color);
    (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}
