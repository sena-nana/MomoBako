use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use nana_gpu::{__framework, LogicalBinding, LogicalBindingType, ResourceTable, ShaderStage};
use nana_ui_scene::StrokeCap;

use super::{
    clip::{FragmentClip, LogicalRect},
    color::{orthographic_scaled, pack_linear, with_opacity},
};
use crate::{PhysicalRect, gpu_work::ManagedBuffer};

const INITIAL_INSTANCES: usize = 256;
/// Unique `FragmentClip` slots interned per frame. The storage buffer grows
/// to the next power of two past this; strokes are never dropped.
const INITIAL_CLIPS: usize = 16;
const ROUND_CAP: f32 = 0.0;
const BUTT_CAP: f32 = 1.0;
const MIN_SEGMENT_LEN_SQ: f32 = f32::EPSILON * f32::EPSILON;
const MESH_VERTEX: &[ShaderStage] = &[ShaderStage::Vertex];
const MESH_FRAGMENT: &[ShaderStage] = &[ShaderStage::Fragment];
fn mesh_layout_key() -> u64 {
    ResourceTable::new(vec![
        LogicalBinding::new(0, LogicalBindingType::UniformBuffer, MESH_VERTEX)
            .min_size(std::mem::size_of::<Uniforms>() as u64),
        LogicalBinding::new(
            1,
            LogicalBindingType::StorageBuffer { read_only: true },
            MESH_FRAGMENT,
        ),
        LogicalBinding::new(
            2,
            LogicalBindingType::StorageBuffer { read_only: true },
            MESH_FRAGMENT,
        ),
    ])
    .expect("mesh table")
    .layout_key()
}

pub(super) struct StrokeStyle<'a> {
    pub width: f32,
    pub widths: &'a [f32],
    pub cap: StrokeCap,
    pub dash: &'a [f32],
    pub dash_offset: f32,
    pub colors: &'a [[f32; 4]],
}

/// Per-segment instance. Clip is a palette index, not an inline 88-byte clip.
/// Color stays on the instance so per-point strokes can differ in one draw.
/// Endpoints/radii stay pre-affine; `affine` maps the covering quad and inverts
/// in the fragment SDF so a vanilla disc becomes an ellipse under non-uniform scale.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct MeshInstance {
    color: [f32; 4],
    p0: [f32; 2],
    p1: [f32; 2],
    radii: [f32; 2],
    clip_index: u32,
    packed_caps: f32,
    affine: [f32; 6],
}

const _: () = assert!(std::mem::size_of::<MeshInstance>() == 72);

/// GPU clip record. Matches `GpuClip` in `triangle_solid.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct GpuClip {
    rect: [f32; 4],
    inv_abcd: [f32; 4],
    /// `xyz` = clip inverse e/f + corner radius; `w` = polygon vertex count.
    inv_ef_radius: [f32; 4],
    poly0: [f32; 4],
    poly1: [f32; 4],
    poly2: [f32; 4],
    poly3: [f32; 4],
}

pub(super) const GPU_CLIP_BYTES: usize = std::mem::size_of::<GpuClip>();
const _: () = assert!(GPU_CLIP_BYTES == 112);

/// Bit-exact intern key: every `GpuClip` float as native `to_bits` (112 bytes).
/// `PartialEq` would collapse `-0.0` with `0.0` and treat NaNs as never-equal.
type ClipInternKey = [u32; 28];

fn pack_clip_polygon(polygon: &[[f32; 2]; 8]) -> [[f32; 4]; 4] {
    let mut packed = [[0.0; 4]; 4];
    for (index, point) in polygon.iter().enumerate() {
        let slot = index / 2;
        let component = (index % 2) * 2;
        packed[slot][component] = point[0];
        packed[slot][component + 1] = point[1];
    }
    packed
}

impl GpuClip {
    fn from_fragment(clip: FragmentClip) -> Self {
        let polys = pack_clip_polygon(&clip.polygon);
        Self {
            rect: clip.rect,
            inv_abcd: clip.inv_abcd,
            inv_ef_radius: [
                clip.inv_ef[0],
                clip.inv_ef[1],
                clip.corner_radius,
                f32::from(clip.polygon_count),
            ],
            poly0: polys[0],
            poly1: polys[1],
            poly2: polys[2],
            poly3: polys[3],
        }
    }
}

impl MeshInstance {
    fn segment(
        color: [f32; 4],
        p0: [f32; 2],
        p1: [f32; 2],
        r0: f32,
        r1: f32,
        start_cap: StrokeCap,
        end_cap: StrokeCap,
    ) -> Self {
        Self {
            color,
            p0,
            p1,
            radii: [r0, r1],
            clip_index: 0,
            packed_caps: pack_caps(start_cap, end_cap),
            affine: super::clip::IDENTITY_AFFINE,
        }
    }
}

fn cap_code(cap: StrokeCap) -> f32 {
    match cap {
        StrokeCap::Round => ROUND_CAP,
        StrokeCap::Butt | StrokeCap::Square => BUTT_CAP,
    }
}

fn pack_caps(start: StrokeCap, end: StrokeCap) -> f32 {
    cap_code(start) + 2.0 * cap_code(end)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct Uniforms {
    transform: [f32; 16],
    viewport_scale: f32,
    _pad: [f32; 3],
}

pub(super) struct MeshRange {
    pub first_instance: u32,
    pub instance_count: u32,
}

/// One custom-paint path vertex (Issue #217). Matches `PathVertexInput` in
/// `path.wgsl`. Position and extrusion are already in logical scene px.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct GpuPathVertex {
    position: [f32; 2],
    extrude: [f32; 2],
    coverage: f32,
    clip_index: u32,
    color: [f32; 4],
    /// Gradient-space position; read only when `gradient` is not `NO_GRADIENT`.
    paint_pos: [f32; 2],
    gradient: u32,
}

const _: () = assert!(std::mem::size_of::<GpuPathVertex>() == 52);

const NO_GRADIENT: u32 = u32::MAX;
/// Colour stops a GPU gradient holds. A gradient with more is resampled.
const GRADIENT_STOPS: usize = 16;
const INITIAL_GRADIENTS: usize = 4;

/// One gradient. Matches `GpuGradient` in `path.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct GpuGradient {
    /// kind (0 linear, 1 radial, 2 conic), extend (0 pad, 1 repeat, 2
    /// reflect), stop count, unused.
    header: [u32; 4],
    /// Linear: start.xy, end.xy. Radial: center.xy, radius. Conic: center.xy,
    /// start angle.
    geometry: [f32; 4],
    offsets: [[f32; 4]; GRADIENT_STOPS / 4],
    /// Premultiplied sRGB, the space CSS interpolates stops in.
    colors: [[f32; 4]; GRADIENT_STOPS],
}

impl GpuGradient {
    fn from_resolved(gradient: &nana_ui_runtime::ResolvedGradient) -> Self {
        use nana_ui_runtime::{GradientExtend, GradientShape};
        let (kind, geometry) = match gradient.shape {
            GradientShape::Linear { start, end } => (0, [start[0], start[1], end[0], end[1]]),
            GradientShape::Radial { center, radius } => (1, [center[0], center[1], radius, 0.0]),
            GradientShape::Conic {
                center,
                start_angle,
            } => (2, [center[0], center[1], start_angle, 0.0]),
        };
        let extend = match gradient.extend {
            GradientExtend::Pad => 0,
            GradientExtend::Repeat => 1,
            GradientExtend::Reflect => 2,
        };
        let stops: Vec<(f32, [f32; 4])> = if gradient.stops.len() <= GRADIENT_STOPS {
            gradient.stops.clone()
        } else {
            (0..GRADIENT_STOPS)
                .map(|i| {
                    let t = i as f32 / (GRADIENT_STOPS - 1) as f32;
                    (t, sample_stops(&gradient.stops, t))
                })
                .collect()
        };
        let mut packed = Self::zeroed();
        packed.header = [kind, extend, stops.len() as u32, 0];
        packed.geometry = geometry;
        for (index, (offset, [r, g, b, a])) in stops.iter().enumerate() {
            packed.offsets[index / 4][index % 4] = *offset;
            packed.colors[index] = [r * a, g * a, b * a, *a];
        }
        packed
    }
}

/// Straight sRGB colour of sorted stops at `t`, interpolated premultiplied.
fn sample_stops(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    let premultiplied = |[r, g, b, a]: [f32; 4]| [r * a, g * a, b * a, a];
    let Some(next) = stops.iter().position(|(offset, _)| *offset >= t) else {
        return stops.last().map_or([0.0; 4], |stop| stop.1);
    };
    if next == 0 {
        return stops[0].1;
    }
    let (o0, c0) = stops[next - 1];
    let (o1, c1) = stops[next];
    let k = if o1 > o0 { (t - o0) / (o1 - o0) } else { 1.0 };
    let (p0, p1) = (premultiplied(c0), premultiplied(c1));
    let mixed: [f32; 4] = std::array::from_fn(|i| p0[i] + (p1[i] - p0[i]) * k);
    if mixed[3] <= 0.0 {
        return [0.0; 4];
    }
    [
        mixed[0] / mixed[3],
        mixed[1] / mixed[3],
        mixed[2] / mixed[3],
        mixed[3],
    ]
}

const INITIAL_PATH_VERTICES: usize = 256;
const INITIAL_PATH_INDICES: usize = 768;

pub(super) struct PathRange {
    pub first_index: u32,
    pub index_count: u32,
}

/// Vertex and index storage for custom-paint path triangles. Separate from
/// the stroke instances, but drawn with the same globals and clip palette.
pub(super) struct PathBuffers {
    gradients: ManagedBuffer,
    gradient_capacity: usize,
    pending_gradients: Vec<GpuGradient>,
    uploaded_gradients: Vec<GpuGradient>,
    /// Gradients already queued this frame, by the resolved gradient they
    /// came from: a mesh repainted every frame shares one.
    gradient_intern: HashMap<usize, u32>,
    vertices: ManagedBuffer,
    vertex_capacity: usize,
    indices: ManagedBuffer,
    index_capacity: usize,
    pending_vertices: Vec<GpuPathVertex>,
    pending_indices: Vec<u32>,
    uploaded_vertices: Vec<GpuPathVertex>,
    uploaded_indices: Vec<u32>,
}

impl PathBuffers {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            gradients: ManagedBuffer::new(gradient_buffer(device, INITIAL_GRADIENTS)),
            gradient_capacity: INITIAL_GRADIENTS,
            pending_gradients: Vec::new(),
            uploaded_gradients: Vec::new(),
            gradient_intern: HashMap::new(),
            vertices: ManagedBuffer::new(path_vertex_buffer(device, INITIAL_PATH_VERTICES)),
            vertex_capacity: INITIAL_PATH_VERTICES,
            indices: ManagedBuffer::new(path_index_buffer(device, INITIAL_PATH_INDICES)),
            index_capacity: INITIAL_PATH_INDICES,
            pending_vertices: Vec::new(),
            pending_indices: Vec::new(),
            uploaded_vertices: Vec::new(),
            uploaded_indices: Vec::new(),
        }
    }

    /// Returns whether the gradient buffer was replaced, which invalidates
    /// the bind group that names it.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> bool {
        let mut rebind = false;
        if self.pending_gradients.len() > self.gradient_capacity {
            self.uploaded_gradients.clear();
            self.gradient_capacity = self.pending_gradients.len().next_power_of_two();
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.gradients,
                    (self.gradient_capacity * std::mem::size_of::<GpuGradient>()) as u64,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    "nana-ui.scene.mesh.gradients",
                );
            } else {
                self.gradients =
                    ManagedBuffer::new(gradient_buffer(device, self.gradient_capacity));
            }
            rebind = true;
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        if self.pending_gradients != self.uploaded_gradients {
            super::buffer_upload::upload_changed_with_work(
                queue,
                &self.gradients,
                bytemuck::cast_slice(&self.uploaded_gradients),
                bytemuck::cast_slice(&self.pending_gradients),
                gpu_work,
            );
            self.uploaded_gradients.clone_from(&self.pending_gradients);
        }
        if self.pending_indices.is_empty() {
            self.uploaded_vertices.clear();
            self.uploaded_indices.clear();
            return rebind;
        }
        if self.pending_vertices == self.uploaded_vertices
            && self.pending_indices == self.uploaded_indices
        {
            return rebind;
        }
        if self.pending_vertices.len() > self.vertex_capacity {
            self.uploaded_vertices.clear();
            self.vertex_capacity = self.pending_vertices.len().next_power_of_two();
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.vertices,
                    (self.vertex_capacity * std::mem::size_of::<GpuPathVertex>()) as u64,
                    wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    "nana-ui.scene.mesh.vertices",
                );
            } else {
                self.vertices =
                    ManagedBuffer::new(path_vertex_buffer(device, self.vertex_capacity));
            }
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        if self.pending_indices.len() > self.index_capacity {
            self.uploaded_indices.clear();
            self.index_capacity = self.pending_indices.len().next_power_of_two();
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.indices,
                    (self.index_capacity * std::mem::size_of::<u32>()) as u64,
                    wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    "nana-ui.scene.mesh.indices",
                );
            } else {
                self.indices = ManagedBuffer::new(path_index_buffer(device, self.index_capacity));
            }
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.vertices,
            bytemuck::cast_slice(&self.uploaded_vertices),
            bytemuck::cast_slice(&self.pending_vertices),
            gpu_work,
        );
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.indices,
            bytemuck::cast_slice(&self.uploaded_indices),
            bytemuck::cast_slice(&self.pending_indices),
            gpu_work,
        );
        self.uploaded_vertices.clone_from(&self.pending_vertices);
        self.uploaded_indices.clone_from(&self.pending_indices);
        if let Some(work) = gpu_work {
            work.record_batch_rebuild();
        }
        rebind
    }
}

fn gradient_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.path.gradients"),
        size: (capacity.max(1) * std::mem::size_of::<GpuGradient>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn path_vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.path.vertices"),
        size: (capacity * std::mem::size_of::<GpuPathVertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn path_index_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.path.indices"),
        size: (capacity * std::mem::size_of::<u32>()) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A mesh vertex's AA extrude, built in node space to push its edges one
/// pixel out, carried through the linear part `[a, b, c, d]` of the draw
/// transform so the edges still move one pixel on screen.
///
/// The extrude is a miter: along the bisector of the two edge normals, as
/// long as `1 / cos` of half the angle between them (a straight edge's is
/// its unit normal). That gives both normals back. Each maps by the inverse
/// transpose — not the matrix itself: under `scaleX(0.01)` a vertical edge's
/// normal stays one pixel long rather than shrinking to a hundredth — and
/// the miter is rebuilt on screen from the two. A degenerate transform has
/// no screen normal and gets no fringe.
fn screen_extrude([a, b, c, d]: [f32; 4], extrude: [f32; 2]) -> [f32; 2] {
    const MITER_LIMIT: f32 = 4.0;
    let det = a * d - b * c;
    let length = (extrude[0] * extrude[0] + extrude[1] * extrude[1]).sqrt();
    if !det.is_finite() || det.abs() < 1e-12 || length.is_nan() || length <= 1e-12 {
        return [0.0, 0.0];
    }
    if a == d && b == -c {
        // A rotation and uniform scale, as nearly every node is under: the
        // miter just turns with it.
        let k = (a * a + b * b).sqrt();
        return [
            (a * extrude[0] + c * extrude[1]) / k,
            (b * extrude[0] + d * extrude[1]) / k,
        ];
    }
    let bisector = [extrude[0] / length, extrude[1] / length];
    let cos = (1.0 / length).min(1.0);
    let sin = (1.0 - cos * cos).max(0.0).sqrt();
    let normal = |sin: f32| {
        let n = [
            bisector[0] * cos - bisector[1] * sin,
            bisector[0] * sin + bisector[1] * cos,
        ];
        let mapped = [(d * n[0] - b * n[1]) / det, (-c * n[0] + a * n[1]) / det];
        let len = (mapped[0] * mapped[0] + mapped[1] * mapped[1]).sqrt();
        if len.is_nan() || len <= 1e-12 {
            None
        } else {
            Some([mapped[0] / len, mapped[1] / len])
        }
    };
    let (Some(n1), Some(n2)) = (normal(sin), normal(-sin)) else {
        return [0.0, 0.0];
    };
    let denominator = 1.0 + n1[0] * n2[0] + n1[1] * n2[1];
    let miter = if denominator > 1e-3 {
        [(n1[0] + n2[0]) / denominator, (n1[1] + n2[1]) / denominator]
    } else {
        n1
    };
    let reach = (miter[0] * miter[0] + miter[1] * miter[1]).sqrt();
    if reach > MITER_LIMIT {
        [
            miter[0] * MITER_LIMIT / reach,
            miter[1] * MITER_LIMIT / reach,
        ]
    } else {
        miter
    }
}

pub(super) struct MeshPipeline {
    pipeline: wgpu::RenderPipeline,
    pipeline_msaa: wgpu::RenderPipeline,
    path_pipeline: wgpu::RenderPipeline,
    path_pipeline_msaa: wgpu::RenderPipeline,
    /// Layer masks (Issue #217). Groups are only ever single-sampled.
    erase_pipeline: wgpu::RenderPipeline,
    tint_pipeline: wgpu::RenderPipeline,
    paths: PathBuffers,
    bind_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    /// What `uniforms` holds, so a frame of the same size writes nothing.
    uploaded_uniforms: Option<Uniforms>,
    clips: ManagedBuffer,
    clip_capacity: usize,
    instances: ManagedBuffer,
    instance_capacity: usize,
    pending_instances: Vec<MeshInstance>,
    uploaded_instances: Vec<MeshInstance>,
    pending_clips: Vec<GpuClip>,
    clip_intern: HashMap<ClipInternKey, u32>,
    uploaded_clips: Vec<GpuClip>,
}

impl MeshPipeline {
    #[cfg(test)]
    pub(super) fn cached_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }
    pub(super) fn new_with_policy(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-ui.scene.triangle.solid.shader"),
            source: wgpu::ShaderSource::Wgsl(super::corner_shape::specialize(concat!(
                include_str!("shader/triangle.wgsl"),
                "\n",
                include_str!("shader/triangle_solid.wgsl"),
                "\n",
                include_str!("shader/path.wgsl"),
                "\n",
                include_str!("shader/color.wgsl"),
            ), super::corner_shape::corner_exponent())),
        });
        let bind_layout = if let Some(policy) = policy {
            let table = ResourceTable::new(vec![
                LogicalBinding::new(0, LogicalBindingType::UniformBuffer, MESH_VERTEX)
                    .min_size(std::mem::size_of::<Uniforms>() as u64),
                LogicalBinding::new(
                    1,
                    LogicalBindingType::StorageBuffer { read_only: true },
                    MESH_FRAGMENT,
                ),
                LogicalBinding::new(
                    2,
                    LogicalBindingType::StorageBuffer { read_only: true },
                    MESH_FRAGMENT,
                ),
            ])
            .expect("mesh logical table is valid")
            .for_generation(policy.generation());
            let logical = __framework::logical_layout(device, policy.generation(), &table)
                .expect("mesh logical layout matches the device");
            __framework::resource_layout(&logical).clone()
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nana-ui.scene.triangle.uniforms"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
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
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.triangle.uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let clips = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.triangle.clips"),
            size: (INITIAL_CLIPS * std::mem::size_of::<GpuClip>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        let paths = PathBuffers::new(device);
        let bind_group = mesh_bind_group(device, &bind_layout, &uniforms, &clips, &paths.gradients);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-ui.scene.triangle.solid.pipeline"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = cached_mesh_pipeline(policy, device, &shader, &layout, format, 1, 1);
        let pipeline_msaa = cached_mesh_pipeline(policy, device, &shader, &layout, format, 4, 2);
        let path_pipeline = cached_path_pipeline(policy, device, &shader, &layout, format, 1, 3);
        let path_pipeline_msaa =
            cached_path_pipeline(policy, device, &shader, &layout, format, 4, 4);
        // dst · (1 − coverage)
        let erase = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        // src · dst alpha: the mesh's paint where the layer already is.
        let tint = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::DstAlpha,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        };
        let erase_pipeline = create_path_pipeline(
            device,
            &shader,
            &layout,
            format,
            1,
            wgpu::BlendState {
                color: erase,
                alpha: erase,
            },
        );
        let tint_pipeline = create_path_pipeline(
            device,
            &shader,
            &layout,
            format,
            1,
            wgpu::BlendState {
                color: tint,
                alpha: tint,
            },
        );
        Self {
            pipeline,
            pipeline_msaa,
            path_pipeline,
            path_pipeline_msaa,
            erase_pipeline,
            tint_pipeline,
            paths,
            bind_layout,
            bind_group,
            uniforms,
            uploaded_uniforms: None,
            clips,
            clip_capacity: INITIAL_CLIPS,
            instances: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana-ui.scene.triangle.instances"),
                size: (INITIAL_INSTANCES * std::mem::size_of::<MeshInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })),
            instance_capacity: INITIAL_INSTANCES,
            pending_instances: Vec::new(),
            uploaded_instances: Vec::new(),
            pending_clips: Vec::new(),
            clip_intern: HashMap::new(),
            uploaded_clips: Vec::new(),
        }
    }

    pub(super) fn begin_frame(&mut self) {
        self.pending_instances.clear();
        self.pending_clips.clear();
        self.clip_intern.clear();
        self.paths.pending_vertices.clear();
        self.paths.pending_indices.clear();
        self.paths.pending_gradients.clear();
        self.paths.gradient_intern.clear();
    }

    /// Queue a custom-paint path mesh (Issue #217). `origin` places the
    /// node-local vertices in layout space, then `affine` maps them to the
    /// scene. The fringe extrusion is carried through the same linear map and
    /// normalised so it stays one physical pixel under scale.
    pub(super) fn push_path(
        &mut self,
        mesh: &nana_ui_scene::PathMesh,
        origin: [f32; 2],
        affine: [f32; 6],
        opacity: f32,
        fragment_clip: FragmentClip,
    ) -> Option<PathRange> {
        if mesh.indices.is_empty() || opacity <= 0.0 {
            return None;
        }
        let clip_index = intern_clip(
            &mut self.pending_clips,
            &mut self.clip_intern,
            fragment_clip,
        );
        let [a, b, c, d, e, f] = affine;
        let gradient = match mesh.gradient.as_ref() {
            None => NO_GRADIENT,
            Some(resolved) => {
                let paths = &mut self.paths;
                *paths
                    .gradient_intern
                    .entry(Arc::as_ptr(resolved) as usize)
                    .or_insert_with(|| {
                        paths
                            .pending_gradients
                            .push(GpuGradient::from_resolved(resolved));
                        paths.pending_gradients.len() as u32 - 1
                    })
            }
        };
        let base = self.paths.pending_vertices.len() as u32;
        let mut last_color = ([f32::NAN; 4], [0.0f32; 4]);
        self.paths
            .pending_vertices
            .extend(mesh.vertices.iter().map(|vertex| {
                // A mesh is mostly one colour: convert it to linear once,
                // not three `powf`s a vertex.
                if vertex.color != last_color.0 {
                    last_color = (
                        vertex.color,
                        pack_linear(with_opacity(vertex.color, opacity)),
                    );
                }
                let x = vertex.position[0] + origin[0];
                let y = vertex.position[1] + origin[1];
                let [ex, ey] = vertex.extrude;
                GpuPathVertex {
                    position: [a * x + c * y + e, b * x + d * y + f],
                    extrude: screen_extrude([a, b, c, d], [ex, ey]),
                    coverage: vertex.coverage,
                    clip_index,
                    color: last_color.1,
                    paint_pos: vertex.paint_pos,
                    gradient,
                }
            }));
        let first_index = self.paths.pending_indices.len() as u32;
        self.paths
            .pending_indices
            .extend(mesh.indices.iter().map(|index| base + index));
        Some(PathRange {
            first_index,
            index_count: mesh.indices.len() as u32,
        })
    }

    /// Apply a painter layer's mask to the open group.
    pub(super) fn draw_mask(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        range: &PathRange,
        scissor: PhysicalRect,
        mode: nana_ui_scene::LayerMaskMode,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if range.index_count == 0 {
            return;
        }
        pass.set_pipeline(match mode {
            nana_ui_scene::LayerMaskMode::Erase => &self.erase_pipeline,
            nana_ui_scene::LayerMaskMode::Tint => &self.tint_pipeline,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.paths.vertices.slice(..));
        pass.set_index_buffer(self.paths.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        pass.draw_indexed(
            range.first_index..range.first_index + range.index_count,
            0,
            0..1,
        );
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }

    pub(super) fn draw_path(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        range: &PathRange,
        scissor: PhysicalRect,
        sample_count: u32,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if range.index_count == 0 {
            return;
        }
        pass.set_pipeline(if sample_count > 1 {
            &self.path_pipeline_msaa
        } else {
            &self.path_pipeline
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.paths.vertices.slice(..));
        pass.set_index_buffer(self.paths.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        pass.draw_indexed(
            range.first_index..range.first_index + range.index_count,
            0,
            0..1,
        );
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit fields of the host or GPU projection contract"
    )]
    pub(super) fn push_stroke_with_path_length(
        &mut self,
        points: &[[f32; 2]],
        style: StrokeStyle<'_>,
        affine: [f32; 6],
        color: [f32; 4],
        opacity: f32,
        fragment_clip: FragmentClip,
        path_length: f32,
    ) -> Option<MeshRange> {
        if points.len() < 2 || style.width <= 0.0 {
            return None;
        }
        let start = self.pending_instances.len() as u32;
        let packed_color = pack_linear(with_opacity(color, opacity));
        let packed_colors: Vec<[f32; 4]> = if style.colors.len() == points.len() {
            style
                .colors
                .iter()
                .map(|item| pack_linear(with_opacity(*item, opacity)))
                .collect()
        } else {
            Vec::new()
        };
        emit_stroke_instances(
            &mut self.pending_instances,
            points,
            StrokeStyle {
                colors: &packed_colors,
                ..style
            },
            packed_color,
            path_length,
        );
        apply_affine_to_instances(&mut self.pending_instances, start as usize, affine);
        stamp_fragment_clip(
            &mut self.pending_instances,
            &mut self.pending_clips,
            &mut self.clip_intern,
            start as usize,
            fragment_clip,
        );
        let instance_count = self.pending_instances.len() as u32 - start;
        (instance_count > 0).then_some(MeshRange {
            first_instance: start,
            instance_count,
        })
    }

    pub(super) fn push_spinner(
        &mut self,
        bounds: LogicalRect,
        affine: [f32; 6],
        phase: u8,
        color: [f32; 4],
        opacity: f32,
        fragment_clip: FragmentClip,
    ) -> Option<MeshRange> {
        let scale = bounds.width.min(bounds.height) / 24.0;
        if scale <= 0.0 {
            return None;
        }
        let color = with_opacity(color, opacity);
        let center = [
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        ];
        let start = self.pending_instances.len() as u32;
        let radius = 2.2 * scale * 0.5;
        for index in 0..8_u8 {
            let angle = f32::from(index) * std::f32::consts::FRAC_PI_4;
            let from = [
                center[0] + angle.cos() * 6.0 * scale,
                center[1] + angle.sin() * 6.0 * scale,
            ];
            let to = [
                center[0] + angle.cos() * 10.0 * scale,
                center[1] + angle.sin() * 10.0 * scale,
            ];
            let distance = (index + 8 - phase % 8) % 8;
            let alpha = 1.0 - f32::from(distance) * 0.105;
            let mut tick_color = color;
            tick_color[3] *= alpha;
            push_segment(
                &mut self.pending_instances,
                from,
                to,
                radius,
                radius,
                StrokeCap::Round,
                StrokeCap::Round,
                pack_linear(tick_color),
            );
        }
        apply_affine_to_instances(&mut self.pending_instances, start as usize, affine);
        stamp_fragment_clip(
            &mut self.pending_instances,
            &mut self.pending_clips,
            &mut self.clip_intern,
            start as usize,
            fragment_clip,
        );
        let instance_count = self.pending_instances.len() as u32 - start;
        (instance_count > 0).then_some(MeshRange {
            first_instance: start,
            instance_count,
        })
    }

    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        physical_size: [u32; 2],
        scale_factor: f32,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let viewport_scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let uniforms = Uniforms {
            transform: orthographic_scaled(physical_size[0], physical_size[1], viewport_scale),
            viewport_scale,
            _pad: [0.0; 3],
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
        if self.paths.upload(device, queue, gpu_work) {
            self.bind_group = mesh_bind_group(
                device,
                &self.bind_layout,
                &self.uniforms,
                &self.clips,
                &self.paths.gradients,
            );
        }
        if self.pending_instances.is_empty() && self.paths.pending_indices.is_empty() {
            self.uploaded_instances.clear();
            self.uploaded_clips.clear();
            return;
        }
        if self.pending_instances == self.uploaded_instances
            && clips_bit_eq(&self.pending_clips, &self.uploaded_clips)
        {
            return;
        }
        if self.pending_instances.len() > self.instance_capacity {
            self.uploaded_instances.clear();
            self.instance_capacity = self.pending_instances.len().next_power_of_two();
            let size = (self.instance_capacity * std::mem::size_of::<MeshInstance>()) as u64;
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.instances,
                    size,
                    usage,
                    "nana-ui.scene.triangle.instances",
                );
            } else {
                self.instances =
                    ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("nana-ui.scene.triangle.instances"),
                        size,
                        usage,
                        mapped_at_creation: false,
                    }));
            }
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        if self.pending_clips.len() > self.clip_capacity {
            self.uploaded_clips.clear();
            self.clip_capacity = self.pending_clips.len().next_power_of_two();
            let size = (self.clip_capacity * std::mem::size_of::<GpuClip>()) as u64;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.clips,
                    size,
                    usage,
                    "nana-ui.scene.triangle.clips",
                );
            } else {
                self.clips = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana-ui.scene.triangle.clips"),
                    size,
                    usage,
                    mapped_at_creation: false,
                }));
            }
            self.bind_group = mesh_bind_group(
                device,
                &self.bind_layout,
                &self.uniforms,
                &self.clips,
                &self.paths.gradients,
            );
            if let Some(work) = gpu_work {
                work.record_realloc();
            }
        }
        let instance_bytes = bytemuck::cast_slice(&self.pending_instances);
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.instances,
            bytemuck::cast_slice(&self.uploaded_instances),
            instance_bytes,
            gpu_work,
        );
        let clip_bytes = bytemuck::cast_slice(&self.pending_clips);
        super::buffer_upload::upload_changed_with_work(
            queue,
            &self.clips,
            bytemuck::cast_slice(&self.uploaded_clips),
            clip_bytes,
            gpu_work,
        );
        self.uploaded_instances.clone_from(&self.pending_instances);
        self.uploaded_clips.clone_from(&self.pending_clips);
        if let Some(work) = gpu_work {
            work.record_batch_rebuild();
        }
    }

    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        range: &MeshRange,
        scissor: PhysicalRect,
        sample_count: u32,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if range.instance_count == 0 {
            return;
        }
        pass.set_pipeline(if sample_count > 1 {
            &self.pipeline_msaa
        } else {
            &self.pipeline
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        pass.draw(
            0..6,
            range.first_instance..range.first_instance + range.instance_count,
        );
        if let Some(work) = gpu_work {
            work.record_draw_batch();
            work.record_draw_call();
        }
    }
}

fn mesh_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    clips: &wgpu::Buffer,
    gradients: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nana-ui.scene.triangle.uniforms.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: clips.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: gradients.as_entire_binding(),
            },
        ],
    })
}

fn cached_mesh_pipeline(
    policy: Option<&nana_gpu::GpuDeviceState>,
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    material: u64,
) -> wgpu::RenderPipeline {
    let create = || mesh_pipeline(device, shader, layout, format, sample_count);
    if let Some(policy) = policy {
        nana_gpu::__framework::render_pipeline_state(
            policy,
            nana_gpu::PipelineKey {
                generation: policy.generation(),
                target_format: nana_gpu::__framework::format_from_wgpu(format),
                sample_count,
                shader: 0x6d65_7368_736f_6c69 ^ super::corner_shape::cache_salt(),
                layout: mesh_layout_key(),
                material,
                primitive: 0,
                blend: 1,
                depth: 0,
                vertex_layout: std::mem::size_of::<MeshInstance>() as u64,
            },
            create,
        )
        .expect("mesh pipeline uses this context generation")
    } else {
        create()
    }
}

fn cached_path_pipeline(
    policy: Option<&nana_gpu::GpuDeviceState>,
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    material: u64,
) -> wgpu::RenderPipeline {
    let create = || {
        create_path_pipeline(
            device,
            shader,
            layout,
            format,
            sample_count,
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        )
    };
    if let Some(policy) = policy {
        nana_gpu::__framework::render_pipeline_state(
            policy,
            nana_gpu::PipelineKey {
                generation: policy.generation(),
                target_format: nana_gpu::__framework::format_from_wgpu(format),
                sample_count,
                shader: 0x6d65_7368_7061_7468 ^ super::corner_shape::cache_salt(),
                layout: mesh_layout_key(),
                material,
                primitive: 0,
                blend: 2,
                depth: 0,
                vertex_layout: std::mem::size_of::<GpuPathVertex>() as u64,
            },
            create,
        )
        .expect("path pipeline uses this context generation")
    } else {
        create()
    }
}

fn create_path_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("nana-ui.scene.path.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("path_vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuPathVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array!(
                    0 => Float32x2,
                    1 => Float32x2,
                    2 => Float32,
                    3 => Uint32,
                    4 => Float32x4,
                    5 => Float32x2,
                    6 => Uint32,
                ),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("path_fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("PATH_SAMPLES", f64::from(sample_count))],
                ..Default::default()
            },
        }),
        // Tessellated triangles come in either winding.
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
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

fn mesh_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("nana-ui.scene.triangle.solid.pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("solid_vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<MeshInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array!(
                    0 => Float32x4,
                    1 => Float32x2,
                    2 => Float32x2,
                    3 => Float32x2,
                    4 => Uint32,
                    5 => Float32,
                    6 => Float32x4,
                    7 => Float32x2,
                ),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            // Both sample counts share the analytic-coverage fragment: MSAA
            // samples geometry, not the SDF, so it cannot own the edge.
            entry_point: Some("solid_fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // Premultiplied alpha, not BlendState::MAX (MAX would also
                // combine against dest quads in this pass).
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

/// Graph / TimeSeries (`pattern: None`: empty dash, empty colors, cap != Square)
/// skip dash and Square expansion. Other attributes share the solid drawable walk.
fn emit_stroke_instances(
    instances: &mut Vec<MeshInstance>,
    points: &[[f32; 2]],
    style: StrokeStyle<'_>,
    color: [f32; 4],
    path_length: f32,
) {
    let colors = if style.colors.len() == points.len() {
        style.colors
    } else {
        &[]
    };
    if style.dash.is_empty() && colors.is_empty() && style.cap != StrokeCap::Square {
        append_stroke_instances(
            instances,
            points,
            style.width,
            style.widths,
            style.cap,
            color,
        );
        return;
    }
    match normalize_dash(style.dash) {
        DashPattern::Empty => {}
        DashPattern::Solid => append_solid_polyline(
            instances,
            points,
            style.width,
            style.widths,
            style.cap,
            color,
            colors,
        ),
        DashPattern::Cycle(pattern) => append_dashed_stroke_instances(
            instances,
            points,
            style.width,
            style.widths,
            style.cap,
            color,
            colors,
            &pattern,
            style.dash_offset,
            path_length,
        ),
    }
}

fn append_stroke_instances(
    instances: &mut Vec<MeshInstance>,
    points: &[[f32; 2]],
    width: f32,
    widths: &[f32],
    cap: StrokeCap,
    color: [f32; 4],
) {
    append_solid_polyline(instances, points, width, widths, cap, color, &[]);
}

/// One pass over drawable segments. Interior Round joins share a disc (Butt
/// start on the next segment). A closed polyline (first==last, ≥2 drawable
/// segments) is an interior join at the close. Dash reuses this walk.
fn append_solid_polyline(
    instances: &mut Vec<MeshInstance>,
    points: &[[f32; 2]],
    width: f32,
    widths: &[f32],
    cap: StrokeCap,
    color: [f32; 4],
    colors: &[[f32; 4]],
) {
    let per_point = widths.len() == points.len();
    let per_color = colors.len() == points.len();
    let expand_square = cap == StrokeCap::Square;
    let closed = polyline_is_closed(points, width, widths, per_point);
    let mut current = next_drawable_segment(points, width, widths, per_point, 0);
    let mut first = true;
    while let Some((index, r0, r1)) = current {
        let next = next_drawable_segment(points, width, widths, per_point, index + 1);
        let start_cap = if first && !closed {
            cap
        } else {
            StrokeCap::Butt
        };
        let end_cap = if next.is_none() {
            if closed && cap == StrokeCap::Square {
                StrokeCap::Butt
            } else {
                cap
            }
        } else if cap == StrokeCap::Butt {
            cap
        } else {
            StrokeCap::Round
        };
        let segment_color = if per_color { colors[index] } else { color };
        push_polyline_segment(
            instances,
            points[index],
            points[index + 1],
            r0.max(0.0),
            r1.max(0.0),
            start_cap,
            end_cap,
            segment_color,
            expand_square,
        );
        first = false;
        current = next;
    }
}

fn next_drawable_segment(
    points: &[[f32; 2]],
    width: f32,
    widths: &[f32],
    per_point: bool,
    start: usize,
) -> Option<(usize, f32, f32)> {
    let last = points.len().saturating_sub(1);
    for index in start..last {
        let (r0, r1) = if per_point {
            (widths[index] * 0.5, widths[index + 1] * 0.5)
        } else {
            let radius = width * 0.5;
            (radius, radius)
        };
        if segment_is_drawable(points[index], points[index + 1], r0, r1) {
            return Some((index, r0, r1));
        }
    }
    None
}

fn points_coincide(a: [f32; 2], b: [f32; 2]) -> bool {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length_sq = dx * dx + dy * dy;
    length_sq.is_finite() && length_sq < MIN_SEGMENT_LEN_SQ
}

/// SVG/CSS close: `points[0]` coincides with `points[last]` and the loop still
/// has two drawable segments after skipping zero-length windows.
fn polyline_is_closed(points: &[[f32; 2]], width: f32, widths: &[f32], per_point: bool) -> bool {
    match (points.first(), points.last()) {
        (Some(&first), Some(&last)) if points_coincide(first, last) => {
            let Some((index, _, _)) = next_drawable_segment(points, width, widths, per_point, 0)
            else {
                return false;
            };
            next_drawable_segment(points, width, widths, per_point, index + 1).is_some()
        }
        _ => false,
    }
}

fn drawable_polyline_length(
    points: &[[f32; 2]],
    width: f32,
    widths: &[f32],
    per_point: bool,
) -> f32 {
    let mut total = 0.0;
    let mut start = 0;
    while let Some((index, _, _)) = next_drawable_segment(points, width, widths, per_point, start) {
        let p0 = points[index];
        let p1 = points[index + 1];
        total += (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        start = index + 1;
    }
    total
}

/// Same drawable walk as `append_solid_polyline`. Closed paths wrap phase
/// (`s = 0 ≡ s = L`); `wrap_join` butts both ends when dash is ON on each side.
/// `path_length` scales geometric `s` into pathLength units before phase.
#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
fn append_dashed_stroke_instances(
    instances: &mut Vec<MeshInstance>,
    points: &[[f32; 2]],
    width: f32,
    widths: &[f32],
    cap: StrokeCap,
    color: [f32; 4],
    colors: &[[f32; 4]],
    pattern: &[f32],
    dash_offset: f32,
    path_length: f32,
) {
    let cycle: f32 = pattern.iter().copied().sum();
    if cycle <= f32::EPSILON {
        return;
    }
    let per_point = widths.len() == points.len();
    let per_color = colors.len() == points.len();
    let expand_square = cap == StrokeCap::Square;
    let closed = polyline_is_closed(points, width, widths, per_point);
    let geometric_length = drawable_polyline_length(points, width, widths, per_point);
    let scale = dash_path_scale(path_length, geometric_length);
    let wrap_join = closed && {
        geometric_length > 1e-4
            && dash_phase(dash_offset + 1e-4, pattern, cycle).0
            && dash_phase(
                geometric_length * scale + dash_offset - 1e-4,
                pattern,
                cycle,
            )
            .0
    };
    let mut path_s = 0.0;
    let mut prev_ended_on_at_vertex = wrap_join;
    let mut current = next_drawable_segment(points, width, widths, per_point, 0);
    while let Some((index, r0, r1)) = current {
        let next = next_drawable_segment(points, width, widths, per_point, index + 1);
        let p0 = points[index];
        let p1 = points[index + 1];
        let r0 = r0.max(0.0);
        let r1 = r1.max(0.0);
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let len = dx.hypot(dy);
        let c0 = if per_color { colors[index] } else { color };
        let c1 = if per_color { colors[index + 1] } else { color };
        let mut local = 0.0;
        while local < len {
            let (on, remaining) =
                dash_phase((path_s + local) * scale + dash_offset, pattern, cycle);
            let remaining_geo = remaining / scale;
            if remaining_geo <= 1e-5 {
                local += 1e-4;
                continue;
            }
            let take = remaining_geo.min(len - local);
            if on && take > f32::EPSILON {
                let t0 = local / len;
                let t1 = (local + take) / len;
                let starts_at_vertex = local <= 1e-5;
                let ends_at_vertex = (local + take) >= len - 1e-5;
                let at_path_start = path_s + local <= 1e-4;
                let next_on = if !ends_at_vertex {
                    false
                } else if next.is_some() {
                    dash_phase((path_s + len) * scale + dash_offset + 1e-4, pattern, cycle).0
                } else {
                    wrap_join
                };
                let start_cap =
                    if wrap_join && at_path_start || starts_at_vertex && prev_ended_on_at_vertex {
                        StrokeCap::Butt
                    } else {
                        cap
                    };
                let end_cap = if ends_at_vertex && next_on {
                    StrokeCap::Butt
                } else {
                    cap
                };
                push_polyline_segment(
                    instances,
                    lerp2(p0, p1, t0),
                    lerp2(p0, p1, t1),
                    (r0 + (r1 - r0) * t0).max(0.0),
                    (r0 + (r1 - r0) * t1).max(0.0),
                    start_cap,
                    end_cap,
                    lerp4(c0, c1, t0),
                    expand_square,
                );
                prev_ended_on_at_vertex = ends_at_vertex;
            } else {
                prev_ended_on_at_vertex = false;
            }
            local += take;
        }
        path_s += len;
        current = next;
    }
}

fn dash_path_scale(path_length: f32, geometric_length: f32) -> f32 {
    if path_length > 0.0
        && path_length.is_finite()
        && geometric_length > 0.0
        && geometric_length.is_finite()
    {
        path_length / geometric_length
    } else {
        1.0
    }
}

enum DashPattern {
    Solid,
    Empty,
    Cycle(Vec<f32>),
}

fn normalize_dash(dash: &[f32]) -> DashPattern {
    if dash.is_empty() {
        return DashPattern::Solid;
    }
    if dash.iter().any(|value| !value.is_finite() || *value < 0.0) {
        return DashPattern::Solid;
    }
    if dash.iter().all(|value| *value == 0.0) {
        return DashPattern::Empty;
    }
    let mut pattern = dash.to_vec();
    if pattern.len() % 2 == 1 {
        pattern.extend_from_slice(dash);
    }
    DashPattern::Cycle(pattern)
}

fn dash_phase(distance: f32, pattern: &[f32], cycle: f32) -> (bool, f32) {
    let mut d = distance % cycle;
    if d < 0.0 {
        d += cycle;
    }
    let mut on = true;
    for &len in pattern {
        if d < len {
            return (on, (len - d).max(0.0));
        }
        d -= len;
        on = !on;
    }
    (false, 0.0)
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
fn push_polyline_segment(
    instances: &mut Vec<MeshInstance>,
    p0: [f32; 2],
    p1: [f32; 2],
    r0: f32,
    r1: f32,
    start_cap: StrokeCap,
    end_cap: StrokeCap,
    color: [f32; 4],
    expand_square: bool,
) {
    if expand_square {
        push_capped_segment(instances, p0, p1, r0, r1, start_cap, end_cap, color);
    } else {
        push_segment(instances, p0, p1, r0, r1, start_cap, end_cap, color);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
fn push_capped_segment(
    instances: &mut Vec<MeshInstance>,
    p0: [f32; 2],
    p1: [f32; 2],
    r0: f32,
    r1: f32,
    start_cap: StrokeCap,
    end_cap: StrokeCap,
    color: [f32; 4],
) {
    let origin = p0;
    let (p0, start_cap) = square_end(p0, p1, r0, start_cap, true);
    let (p1, end_cap) = square_end(origin, p1, r1, end_cap, false);
    push_segment(instances, p0, p1, r0, r1, start_cap, end_cap, color);
}

fn square_end(
    p0: [f32; 2],
    p1: [f32; 2],
    radius: f32,
    cap: StrokeCap,
    start: bool,
) -> ([f32; 2], StrokeCap) {
    if cap != StrokeCap::Square {
        return (if start { p0 } else { p1 }, cap);
    }
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let len_sq = dx * dx + dy * dy;
    if !len_sq.is_finite() || len_sq < MIN_SEGMENT_LEN_SQ {
        return (if start { p0 } else { p1 }, StrokeCap::Butt);
    }
    let inv_len = len_sq.sqrt().recip();
    let tx = dx * inv_len * radius;
    let ty = dy * inv_len * radius;
    if start {
        ([p0[0] - tx, p0[1] - ty], StrokeCap::Butt)
    } else {
        ([p1[0] + tx, p1[1] + ty], StrokeCap::Butt)
    }
}

/// One articulated-line segment. Interior joins keep a single endpoint disc:
/// the previous segment draws it, this segment uses a butt start.
#[expect(
    clippy::too_many_arguments,
    reason = "Explicit fields of the host or GPU projection contract"
)]
fn push_segment(
    instances: &mut Vec<MeshInstance>,
    p0: [f32; 2],
    p1: [f32; 2],
    r0: f32,
    r1: f32,
    start_cap: StrokeCap,
    end_cap: StrokeCap,
    color: [f32; 4],
) {
    if !segment_is_drawable(p0, p1, r0, r1) {
        return;
    }
    instances.push(MeshInstance::segment(
        color, p0, p1, r0, r1, start_cap, end_cap,
    ));
}

fn segment_is_drawable(p0: [f32; 2], p1: [f32; 2], r0: f32, r1: f32) -> bool {
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let length_sq = dx * dx + dy * dy;
    length_sq.is_finite()
        && length_sq >= MIN_SEGMENT_LEN_SQ
        && r0.is_finite()
        && r1.is_finite()
        && (r0 > 0.0 || r1 > 0.0)
}

/// Stamp the CSS/Canvas 2×3. Endpoints stay pre-affine; identity is already the
/// instance default.
fn apply_affine_to_instances(instances: &mut [MeshInstance], start: usize, affine: [f32; 6]) {
    if affine == super::clip::IDENTITY_AFFINE {
        return;
    }
    for instance in &mut instances[start..] {
        instance.affine = affine;
    }
}

fn clips_bit_eq(a: &[GpuClip], b: &[GpuClip]) -> bool {
    a.len() == b.len()
        && bytemuck::cast_slice::<GpuClip, u8>(a) == bytemuck::cast_slice::<GpuClip, u8>(b)
}

fn intern_clip(
    clips: &mut Vec<GpuClip>,
    intern: &mut HashMap<ClipInternKey, u32>,
    clip: FragmentClip,
) -> u32 {
    let packed = GpuClip::from_fragment(clip);
    match intern.entry(bytemuck::cast(packed)) {
        std::collections::hash_map::Entry::Occupied(slot) => *slot.get(),
        std::collections::hash_map::Entry::Vacant(slot) => {
            let index = clips.len() as u32;
            clips.push(packed);
            *slot.insert(index)
        }
    }
}

fn stamp_fragment_clip(
    instances: &mut [MeshInstance],
    clips: &mut Vec<GpuClip>,
    intern: &mut HashMap<ClipInternKey, u32>,
    start: usize,
    clip: FragmentClip,
) {
    if start >= instances.len() {
        return;
    }
    let index = intern_clip(clips, intern, clip);
    for instance in &mut instances[start..] {
        instance.clip_index = index;
    }
}

/// CPU covering-fringe / SDF. Same formulas as `triangle_solid.wgsl`
/// (`STROKE_AA_FRINGE`, `hull_half_width`, `covering_corner`, `local_aa_fringe`,
/// `sd_variable_capsule`).
#[cfg(test)]
const STROKE_AA_FRINGE: f32 = 1.0;

#[cfg(test)]
fn hull_half_width(along: f32, pad0: f32, pad1: f32, seg_len: f32) -> f32 {
    let dr = pad0 - pad1;
    if dr.abs() >= seg_len {
        return pad0.max(pad1);
    }
    let sin_a = dr / seg_len;
    let cos_a = (1.0 - sin_a * sin_a).max(0.0).sqrt();
    (pad0 - sin_a * along) / cos_a.max(1e-8)
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "Independent stroke geometry fixture inputs"
)]
fn covering_with_fringe(
    p0: [f32; 2],
    p1: [f32; 2],
    r0: f32,
    r1: f32,
    start: StrokeCap,
    end: StrokeCap,
    fringe: f32,
    corner: [f32; 2],
) -> Option<[f32; 2]> {
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let seg_len = dx.hypot(dy);
    if !seg_len.is_finite() || seg_len < f32::EPSILON {
        return None;
    }
    let tx = dx / seg_len;
    let ty = dy / seg_len;
    let nx = -ty;
    let ny = tx;
    let pad0 = r0 + fringe;
    let pad1 = r1 + fringe;
    let mut end0 = match start {
        StrokeCap::Round | StrokeCap::Square => pad0,
        StrokeCap::Butt => fringe,
    };
    let mut end1 = match end {
        StrokeCap::Round | StrokeCap::Square => pad1,
        StrokeCap::Butt => fringe,
    };
    if pad0 >= pad1 + seg_len && end != StrokeCap::Butt {
        end1 = end1.max(pad0 - seg_len);
    }
    if pad1 >= pad0 + seg_len && start != StrokeCap::Butt {
        end0 = end0.max(pad1 - seg_len);
    }
    let along = if corner[0] > 0.0 {
        seg_len + end1
    } else {
        -end0
    };
    let side = hull_half_width(along, pad0, pad1, seg_len);
    Some([
        p0[0] + tx * along + nx * (corner[1] * side),
        p0[1] + ty * along + ny * (corner[1] * side),
    ])
}

#[cfg(test)]
fn covering_corner(
    p0: [f32; 2],
    p1: [f32; 2],
    r0: f32,
    r1: f32,
    start: StrokeCap,
    end: StrokeCap,
    corner: [f32; 2],
) -> Option<[f32; 2]> {
    covering_with_fringe(p0, p1, r0, r1, start, end, STROKE_AA_FRINGE, corner)
}

#[cfg(test)]
fn local_aa_fringe(abcd: [f32; 4], viewport_scale: f32) -> f32 {
    let sigma = if abcd == [1.0, 0.0, 0.0, 1.0] {
        1.0
    } else {
        let [a, b, c, d] = abcd;
        let det = a * d - b * c;
        let fro2 = a * a + b * b + c * c + d * d;
        let disc = (fro2 * fro2 - 4.0 * det * det).max(0.0);
        ((fro2 - disc.sqrt()) * 0.5).max(0.0).sqrt()
    };
    STROKE_AA_FRINGE / (sigma * viewport_scale.max(1e-4)).max(1e-4)
}

#[cfg(test)]
fn sd_variable_capsule(point: [f32; 2], a: [f32; 2], b: [f32; 2], r0: f32, r1: f32) -> f32 {
    let ba = [b[0] - a[0], b[1] - a[1]];
    let l = ba[0].hypot(ba[1]);
    if l <= 1e-8 {
        return (point[0] - a[0]).hypot(point[1] - a[1]) - r0.max(r1);
    }
    let tx = ba[0] / l;
    let ty = ba[1] / l;
    let nx = -ty;
    let ny = tx;
    let pa0 = point[0] - a[0];
    let pa1 = point[1] - a[1];
    let along = pa0 * tx + pa1 * ty;
    let perp = (pa0 * nx + pa1 * ny).abs();
    let dr = r0 - r1;
    if dr.abs() >= l {
        return if r0 > r1 {
            (point[0] - a[0]).hypot(point[1] - a[1]) - r0
        } else {
            (point[0] - b[0]).hypot(point[1] - b[1]) - r1
        };
    }
    let sin_a = dr / l;
    let cos_a = (1.0 - sin_a * sin_a).max(0.0).sqrt();
    let k = along * cos_a - perp * sin_a;
    if k < 0.0 {
        return (point[0] - a[0]).hypot(point[1] - a[1]) - r0;
    }
    if k > cos_a * l {
        return (point[0] - b[0]).hypot(point[1] - b[1]) - r1;
    }
    along * sin_a + perp * cos_a - r0
}

#[cfg(test)]
fn sd_stroke(
    point: [f32; 2],
    a: [f32; 2],
    b: [f32; 2],
    r0: f32,
    r1: f32,
    start: StrokeCap,
    end: StrokeCap,
) -> f32 {
    let (a, start) = square_end(a, b, r0, start, true);
    let (b, end) = square_end(a, b, r1, end, false);
    let mut distance = sd_variable_capsule(point, a, b, r0, r1);
    if start == StrokeCap::Butt || end == StrokeCap::Butt {
        let ba = [b[0] - a[0], b[1] - a[1]];
        let seg_len = ba[0].hypot(ba[1]).max(1e-8);
        let local_x = (point[0] - a[0]) * (ba[0] / seg_len) + (point[1] - a[1]) * (ba[1] / seg_len);
        if start == StrokeCap::Butt {
            distance = distance.max(-local_x);
        }
        if end == StrokeCap::Butt {
            distance = distance.max(local_x - seg_len);
        }
    }
    distance
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::super::clip::{IDENTITY_AFFINE, invert_affine, transform_point};
    use super::*;

    fn capsule_distance(point: [f32; 2], p0: [f32; 2], p1: [f32; 2]) -> f32 {
        let pa = [point[0] - p0[0], point[1] - p0[1]];
        let ba = [p1[0] - p0[0], p1[1] - p0[1]];
        let denom = ba[0] * ba[0] + ba[1] * ba[1];
        let t = if denom <= f32::EPSILON {
            0.0
        } else {
            ((pa[0] * ba[0] + pa[1] * ba[1]) / denom).clamp(0.0, 1.0)
        };
        let closest = [p0[0] + ba[0] * t, p0[1] + ba[1] * t];
        (point[0] - closest[0]).hypot(point[1] - closest[1])
    }

    #[test]
    fn one_segment_emits_one_instance() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [10.0, 0.0]],
            4.0,
            &[],
            StrokeCap::Round,
            [1.0, 0.0, 0.0, 1.0],
        );
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert_eq!(instances[0].p1, [10.0, 0.0]);
        assert_eq!(instances[0].radii, [2.0, 2.0]);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Round)
        );
        assert_eq!(instances[0].affine, IDENTITY_AFFINE);
        assert_eq!(std::mem::size_of::<MeshInstance>(), 72);
        let pad = 2.0 + STROKE_AA_FRINGE;
        let corner = covering_corner(
            instances[0].p0,
            instances[0].p1,
            2.0,
            2.0,
            StrokeCap::Round,
            StrokeCap::Round,
            [-1.0, -1.0],
        )
        .expect("quad");
        assert!(corner[0] <= -pad + 1e-4);
        assert!(corner[1] <= -pad + 1e-4);
    }

    #[test]
    fn zero_length_and_empty_strokes_emit_nothing() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[3.0, 3.0], [3.0, 3.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(instances.is_empty());
        append_stroke_instances(
            &mut instances,
            &[[1.0, 1.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert!(instances.is_empty());
    }

    #[test]
    fn two_segments_share_an_endpoint_disc() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Round)
        );
        assert_eq!(
            instances[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        let join = [8.0, 0.0];
        let first = capsule_distance(join, instances[0].p0, instances[0].p1);
        assert!(first <= instances[0].radii[1] + 1e-5);
        assert!(
            sd_stroke(
                join,
                instances[1].p0,
                instances[1].p1,
                instances[1].radii[0],
                instances[1].radii[1],
                StrokeCap::Butt,
                StrokeCap::Round,
            ) <= 1e-5,
            "join stays on the first segment's end disc, not a second start disc"
        );
    }

    #[test]
    fn capsule_covers_midline_and_rejects_far_normal() {
        let p0 = [0.0, 0.0];
        let p1 = [20.0, 0.0];
        let radius = 1.5;
        assert!(capsule_distance([10.0, 0.0], p0, p1) < radius);
        assert!(capsule_distance([0.0, 0.0], p0, p1) < radius);
        assert!(capsule_distance([-0.5, 0.0], p0, p1) < radius);
        assert!(capsule_distance([10.0, 4.0], p0, p1) > radius);
        let outside_corner = covering_corner(
            p0,
            p1,
            radius,
            radius,
            StrokeCap::Round,
            StrokeCap::Round,
            [-1.0, -1.0],
        )
        .expect("quad");
        assert!(
            capsule_distance(outside_corner, p0, p1) > radius,
            "covering-quad corner must be discarded by the capsule, got {}",
            capsule_distance(outside_corner, p0, p1)
        );
    }

    #[test]
    fn affine_scales_radius_and_moves_endpoints() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [4.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0, 1.0, 1.0, 1.0],
        );
        let affine = [2.0, 0.0, 0.0, 2.0, 5.0, 7.0];
        apply_affine_to_instances(&mut instances, 0, affine);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert_eq!(instances[0].p1, [4.0, 0.0]);
        assert_eq!(instances[0].radii, [1.0, 1.0]);
        assert_eq!(instances[0].affine, affine);
        let world0 = transform_point(affine, instances[0].p0[0], instances[0].p0[1]);
        let world1 = transform_point(affine, instances[0].p1[0], instances[0].p1[1]);
        assert!((world0[0] - 5.0).abs() < 1e-5);
        assert!((world0[1] - 7.0).abs() < 1e-5);
        assert!((world1[0] - 13.0).abs() < 1e-5);
        let inv = invert_affine(affine).expect("invert");
        let back = transform_point(inv, world0[0], world0[1]);
        assert!((back[0]).abs() < 1e-5);
        assert!((back[1]).abs() < 1e-5);
        let world_edge = transform_point(affine, 2.0, 1.0);
        assert!(
            (world_edge[1] - 9.0).abs() < 1e-5,
            "uniform scale 2 maps local radius 1 to world radius 2, got {world_edge:?}"
        );
    }

    #[test]
    fn affine_non_uniform_scale_covers_the_stretched_ellipse() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [4.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0, 1.0, 1.0, 1.0],
        );
        // Local disc r=1; affine maps it to ellipse 2 × 0.5, not a min-axis disc.
        let affine = [2.0, 0.0, 0.0, 0.5, 5.0, 7.0];
        apply_affine_to_instances(&mut instances, 0, affine);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert_eq!(instances[0].p1, [4.0, 0.0]);
        assert_eq!(instances[0].radii, [1.0, 1.0]);
        assert_eq!(instances[0].affine, affine);
        let inv = invert_affine(affine).expect("invert");
        // Local (−0.5, 0) is inside the start disc; world (4, 7) is outside a
        // baked min-axis disc of 0.5.
        let world_stretched = transform_point(affine, -0.5, 0.0);
        assert!((world_stretched[0] - 4.0).abs() < 1e-5);
        assert!((world_stretched[1] - 7.0).abs() < 1e-5);
        let local = transform_point(inv, world_stretched[0], world_stretched[1]);
        assert!(
            sd_stroke(
                local,
                instances[0].p0,
                instances[0].p1,
                instances[0].radii[0],
                instances[0].radii[1],
                StrokeCap::Round,
                StrokeCap::Round,
            ) < 0.0,
            "stretched-axis sample must stay inside the local disc SDF"
        );
        let local_end = covering_with_fringe(
            instances[0].p0,
            instances[0].p1,
            instances[0].radii[0],
            instances[0].radii[1],
            StrokeCap::Round,
            StrokeCap::Round,
            local_aa_fringe([2.0, 0.0, 0.0, 0.5], 1.0),
            [-1.0, 0.0],
        )
        .expect("quad");
        let world_cover = transform_point(affine, local_end[0], local_end[1]);
        let p0_world = transform_point(affine, 0.0, 0.0);
        let ellipse_along_x = instances[0].radii[0] * 2.0;
        assert!(
            world_cover[0] <= p0_world[0] - ellipse_along_x + 1e-4,
            "covering quad must enclose the stretched ellipse, cover={} p0={} extent={}",
            world_cover[0],
            p0_world[0],
            ellipse_along_x
        );
        let fringe = local_aa_fringe([2.0, 0.0, 0.0, 0.5], 1.0);
        assert!(
            world_cover[0] <= p0_world[0] - ellipse_along_x - fringe * 2.0 + 1e-3,
            "1/σ_min local pad must survive the stretch, cover={} expected<={}",
            world_cover[0],
            p0_world[0] - ellipse_along_x - fringe * 2.0
        );
    }

    #[test]
    fn affine_rotation_does_not_change_radius() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [4.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0, 1.0, 1.0, 1.0],
        );
        // 90° CCW: (x, y) -> (-y, x). Rotation lives in the affine; local
        // radii stay 1 so isotropic scale of the disc is unchanged.
        let affine = [0.0, 1.0, -1.0, 0.0, 0.0, 0.0];
        apply_affine_to_instances(&mut instances, 0, affine);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert_eq!(instances[0].p1, [4.0, 0.0]);
        assert_eq!(instances[0].radii, [1.0, 1.0]);
        assert_eq!(instances[0].affine, affine);
        let world1 = transform_point(affine, instances[0].p1[0], instances[0].p1[1]);
        assert!((world1[0]).abs() < 1e-5);
        assert!((world1[1] - 4.0).abs() < 1e-5);
    }

    #[test]
    fn per_point_widths_emit_tapered_radii() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [20.0, 0.0], [20.0, 10.0]],
            2.0,
            &[8.0, 2.0, 4.0],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].radii, [4.0, 1.0]);
        assert_eq!(instances[1].radii, [1.0, 2.0]);
    }

    #[test]
    fn mismatched_widths_fall_back_to_uniform_width() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [4.0, 0.0]],
            6.0,
            &[1.0],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(instances[0].radii, [3.0, 3.0]);
    }

    #[test]
    fn variable_capsule_matches_constant_radius() {
        let p0 = [0.0, 0.0];
        let p1 = [16.0, 0.0];
        let radius = 2.0;
        for point in [[8.0, 0.0], [0.0, 0.0], [16.0, 1.0], [8.0, 3.0], [-1.0, 0.0]] {
            let constant = capsule_distance(point, p0, p1) - radius;
            let variable = sd_variable_capsule(point, p0, p1, radius, radius);
            assert!(
                (constant - variable).abs() < 2e-4,
                "point {point:?}: constant {constant} vs variable {variable}"
            );
        }
    }

    #[test]
    fn variable_capsule_keeps_large_end_and_thins() {
        let p0 = [0.0, 0.0];
        let p1 = [20.0, 0.0];
        assert!(sd_variable_capsule([0.0, 3.0], p0, p1, 4.0, 1.0) < 0.0);
        assert!(sd_variable_capsule([20.0, 0.4], p0, p1, 4.0, 1.0) < 0.0);
        assert!(sd_variable_capsule([20.0, 2.0], p0, p1, 4.0, 1.0) > 0.0);
        assert!(sd_variable_capsule([10.0, 0.0], p0, p1, 4.0, 1.0) < 0.0);
    }

    #[test]
    fn variable_capsule_uses_external_tangents() {
        let p0 = [0.0, 0.0];
        let p1 = [10.0, 0.0];
        // Naive centerline lerp at x=5 is mix(4,1,0.5)=2.5. The hull's
        // external tangent sits farther out (~2.62), so this point is inside.
        assert!(sd_variable_capsule([5.0, 2.55], p0, p1, 4.0, 1.0) < 0.0);
        assert!(sd_variable_capsule([5.0, 3.5], p0, p1, 4.0, 1.0) > 0.0);
    }

    #[test]
    fn variable_capsule_larger_disc_contains_smaller() {
        let p0 = [0.0, 0.0];
        let p1 = [5.0, 0.0];
        assert!(sd_variable_capsule([5.0, 6.0], p0, p1, 10.0, 1.0) < 0.0);
        assert!(sd_variable_capsule([0.0, 9.5], p0, p1, 10.0, 1.0) < 0.0);
        assert!(sd_variable_capsule([0.0, 11.0], p0, p1, 10.0, 1.0) > 0.0);
        assert!(sd_variable_capsule([8.0, 0.0], p0, p1, 10.0, 1.0) < 0.0);
    }

    #[test]
    fn butt_cap_cuts_the_endpoint_disc() {
        let p0 = [0.0, 0.0];
        let p1 = [20.0, 0.0];
        let radius = 2.0;
        assert!(
            sd_stroke(
                [10.0, 0.0],
                p0,
                p1,
                radius,
                radius,
                StrokeCap::Butt,
                StrokeCap::Butt
            ) < 0.0
        );
        assert!(
            sd_stroke(
                [0.0, 0.0],
                p0,
                p1,
                radius,
                radius,
                StrokeCap::Butt,
                StrokeCap::Butt
            ) <= 1e-5
        );
        assert!(
            sd_stroke(
                [-0.5, 0.0],
                p0,
                p1,
                radius,
                radius,
                StrokeCap::Butt,
                StrokeCap::Butt
            ) > 0.0,
            "butt must reject the round-cap disc past the start"
        );
        assert!(capsule_distance([-0.5, 0.0], p0, p1) < radius);
        let butt_corner = covering_corner(
            p0,
            p1,
            radius,
            radius,
            StrokeCap::Butt,
            StrokeCap::Butt,
            [-1.0, 0.0],
        )
        .expect("quad");
        assert!(
            butt_corner[0] > -radius,
            "butt covering quad must not extend a full radius past the start, got {}",
            butt_corner[0]
        );
    }

    #[test]
    fn tapered_covering_quad_contains_external_tangents() {
        let p0 = [0.0, 0.0];
        let p1 = [10.0, 0.0];
        let r0 = 4.0;
        let r1 = 1.0;
        let thick = covering_corner(
            p0,
            p1,
            r0,
            r1,
            StrokeCap::Round,
            StrokeCap::Round,
            [-1.0, 1.0],
        )
        .expect("quad");
        let thin = covering_corner(
            p0,
            p1,
            r0,
            r1,
            StrokeCap::Round,
            StrokeCap::Round,
            [1.0, 1.0],
        )
        .expect("quad");
        assert!(
            thick[1] >= r0 + STROKE_AA_FRINGE - 1e-4,
            "start corner must cover the large disc, got {}",
            thick[1]
        );
        let span = thin[0] - thick[0];
        for x in [0.0, 5.0, 10.0] {
            let t = (x - thick[0]) / span;
            let cover_y = thick[1] + t * (thin[1] - thick[1]);
            let distance = sd_variable_capsule([x, cover_y], p0, p1, r0, r1);
            assert!(
                distance >= -1e-3,
                "covering edge at x={x} must not sit inside the hull, sd={distance}"
            );
        }
        let t0 = (0.0 - thick[0]) / span;
        let cover_at_start = thick[1] + t0 * (thin[1] - thick[1]);
        assert!(
            cover_at_start >= r0 + STROKE_AA_FRINGE - 1e-3,
            "expanded start disc must stay inside the trapezoid, got {cover_at_start}"
        );
        let t1 = (10.0 - thick[0]) / span;
        let cover_at_end = thick[1] + t1 * (thin[1] - thick[1]);
        assert!(
            cover_at_end >= r1 + STROKE_AA_FRINGE - 1e-3,
            "expanded end disc must stay inside the trapezoid, got {cover_at_end}"
        );

        let uniform0 = covering_corner(
            p0,
            p1,
            2.0,
            2.0,
            StrokeCap::Round,
            StrokeCap::Round,
            [-1.0, 1.0],
        )
        .expect("quad");
        let uniform1 = covering_corner(
            p0,
            p1,
            2.0,
            2.0,
            StrokeCap::Round,
            StrokeCap::Round,
            [1.0, 1.0],
        )
        .expect("quad");
        assert!((uniform0[1] - (2.0 + STROKE_AA_FRINGE)).abs() < 1e-4);
        assert!((uniform1[1] - (2.0 + STROKE_AA_FRINGE)).abs() < 1e-4);
    }

    #[test]
    fn covering_quad_contains_larger_disc_when_nested() {
        let far = covering_corner(
            [0.0, 0.0],
            [5.0, 0.0],
            10.0,
            1.0,
            StrokeCap::Round,
            StrokeCap::Round,
            [1.0, 1.0],
        )
        .expect("quad");
        assert!(
            far[0] >= 10.0 + STROKE_AA_FRINGE - 1e-3,
            "nested larger disc must cover past the small end, got {}",
            far[0]
        );
        assert!(
            far[1] >= 10.0 + STROKE_AA_FRINGE - 1e-3,
            "nested larger disc must keep its radius, got {}",
            far[1]
        );
    }

    #[test]
    fn interior_round_join_omits_the_second_start_disc() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(instances.len(), 3);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Round)
        );
        assert_eq!(
            instances[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_eq!(
            instances[2].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
    }

    #[test]
    fn closed_round_polyline_shares_one_disc_at_the_close() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(instances.len(), 3);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_eq!(
            instances[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_eq!(
            instances[2].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        let close = [0.0, 0.0];
        assert!(
            sd_stroke(
                close,
                instances[2].p0,
                instances[2].p1,
                instances[2].radii[0],
                instances[2].radii[1],
                StrokeCap::Butt,
                StrokeCap::Round,
            ) <= 1e-5,
            "last end disc covers the close vertex"
        );
        assert!(
            sd_stroke(
                [-0.5, 0.0],
                instances[0].p0,
                instances[0].p1,
                instances[0].radii[0],
                instances[0].radii[1],
                StrokeCap::Butt,
                StrokeCap::Round,
            ) > 0.0,
            "first start is Butt: no second disc past the close"
        );
    }

    #[test]
    fn closed_butt_polyline_keeps_both_close_ends_butt() {
        let mut instances = Vec::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Butt,
            [1.0; 4],
        );
        assert_eq!(instances.len(), 3);
        let butt = pack_caps(StrokeCap::Butt, StrokeCap::Butt);
        assert!(
            instances
                .iter()
                .all(|instance| instance.packed_caps == butt)
        );
        assert!(
            sd_stroke(
                [-0.5, 0.0],
                instances[0].p0,
                instances[0].p1,
                1.0,
                1.0,
                StrokeCap::Butt,
                StrokeCap::Butt,
            ) > 0.0
        );
        assert!(
            sd_stroke(
                [-0.5, 0.0],
                instances[2].p0,
                instances[2].p1,
                1.0,
                1.0,
                StrokeCap::Butt,
                StrokeCap::Butt,
            ) > 0.0
        );
    }

    #[test]
    fn open_polyline_keeps_two_end_caps() {
        let mut closed = Vec::new();
        append_stroke_instances(
            &mut closed,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        let mut open = Vec::new();
        append_stroke_instances(
            &mut open,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(open.len(), 2);
        assert_eq!(
            open[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Round)
        );
        assert_eq!(
            open[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_ne!(open[0].packed_caps, closed[0].packed_caps);
    }

    #[test]
    fn duplicate_close_vertex_is_skipped_and_still_closed() {
        let mut closed = Vec::new();
        append_stroke_instances(
            &mut closed,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        let mut duplicate = Vec::new();
        append_stroke_instances(
            &mut duplicate,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0], [0.0, 0.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        assert_eq!(duplicate, closed);
        assert_eq!(duplicate.len(), 3);
        assert_eq!(
            duplicate[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_eq!(
            duplicate[2].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
    }

    #[test]
    fn closed_square_polyline_butts_the_close_instead_of_double_expand() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 0.0]],
            solid_style(4.0, &[], StrokeCap::Square),
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 3);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert_eq!(instances[2].p1, [0.0, 0.0]);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        assert_eq!(
            instances[2].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Butt)
        );
    }

    fn solid_style<'a>(width: f32, widths: &'a [f32], cap: StrokeCap) -> StrokeStyle<'a> {
        StrokeStyle {
            width,
            widths,
            cap,
            dash: &[],
            dash_offset: 0.0,
            colors: &[],
        }
    }

    #[test]
    fn unused_attributes_match_legacy_solid_emit() {
        let points = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0]];
        let color = [1.0, 0.0, 0.0, 1.0];
        let mut legacy = Vec::new();
        append_stroke_instances(&mut legacy, &points, 2.0, &[], StrokeCap::Round, color);
        let mut via_style = Vec::new();
        emit_stroke_instances(
            &mut via_style,
            &points,
            solid_style(2.0, &[], StrokeCap::Round),
            color,
            0.0,
        );
        assert_eq!(
            via_style, legacy,
            "empty dash/colors and Round must reuse the solid instance loop"
        );
        emit_stroke_instances(
            &mut via_style,
            &points,
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[-4.0, 2.0],
                dash_offset: 3.0,
                colors: &[[0.0, 1.0, 0.0, 1.0]],
            },
            color,
            0.0,
        );
        assert_eq!(
            via_style[legacy.len()..],
            legacy,
            "invalid dash and mismatched colors must stay on the solid emit"
        );
    }

    #[test]
    fn square_cap_expands_endpoints_onto_the_butt_gpu_path() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [10.0, 0.0]],
            solid_style(4.0, &[], StrokeCap::Square),
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 1);
        assert!((instances[0].p0[0] + 2.0).abs() < 1e-5);
        assert!((instances[0].p1[0] - 12.0).abs() < 1e-5);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Butt)
        );
        assert!(
            sd_stroke(
                [-1.5, 1.5],
                [0.0, 0.0],
                [10.0, 0.0],
                2.0,
                2.0,
                StrokeCap::Square,
                StrokeCap::Square,
            ) < 0.0
        );
        assert!(
            sd_stroke(
                [-1.5, 1.5],
                [0.0, 0.0],
                [10.0, 0.0],
                2.0,
                2.0,
                StrokeCap::Round,
                StrokeCap::Round,
            ) > 0.0
        );
        assert!(
            sd_stroke(
                [-2.5, 0.0],
                [0.0, 0.0],
                [10.0, 0.0],
                2.0,
                2.0,
                StrokeCap::Square,
                StrokeCap::Square,
            ) > 0.0
        );
    }

    #[test]
    fn dash_emits_only_on_segments() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [40.0, 0.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Butt,
                dash: &[10.0, 10.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].p0, [0.0, 0.0]);
        assert!((instances[0].p1[0] - 10.0).abs() < 1e-4);
        assert!((instances[1].p0[0] - 20.0).abs() < 1e-4);
        assert!((instances[1].p1[0] - 30.0).abs() < 1e-4);
    }

    fn dashed_line_style() -> StrokeStyle<'static> {
        StrokeStyle {
            width: 2.0,
            widths: &[],
            cap: StrokeCap::Butt,
            dash: &[10.0, 10.0],
            dash_offset: 0.0,
            colors: &[],
        }
    }

    #[test]
    fn path_length_twice_geometry_halves_dash_period() {
        // 40px line, dash 10,10 → two on-segments. path_length = 80 maps
        // geometric s → 2s (dashes in pathLength units), so on-dashes are
        // 5px and four on-segments cover the line.
        let points = [[0.0, 0.0], [40.0, 0.0]];
        let mut geometric = Vec::new();
        emit_stroke_instances(&mut geometric, &points, dashed_line_style(), [1.0; 4], 0.0);
        assert_eq!(geometric.len(), 2);
        let mut scaled = Vec::new();
        emit_stroke_instances(&mut scaled, &points, dashed_line_style(), [1.0; 4], 80.0);
        assert_eq!(scaled.len(), 4);
        assert_eq!(scaled[0].p0, [0.0, 0.0]);
        assert!((scaled[0].p1[0] - 5.0).abs() < 1e-4);
        assert!((scaled[1].p0[0] - 10.0).abs() < 1e-4);
        assert!((scaled[1].p1[0] - 15.0).abs() < 1e-4);
        assert!((scaled[2].p0[0] - 20.0).abs() < 1e-4);
        assert!((scaled[2].p1[0] - 25.0).abs() < 1e-4);
        assert!((scaled[3].p0[0] - 30.0).abs() < 1e-4);
        assert!((scaled[3].p1[0] - 35.0).abs() < 1e-4);
    }

    #[test]
    fn unset_path_length_keeps_geometric_dash() {
        let points = [[0.0, 0.0], [40.0, 0.0]];
        let mut baseline = Vec::new();
        emit_stroke_instances(&mut baseline, &points, dashed_line_style(), [1.0; 4], 0.0);
        for path_length in [0.0, -8.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut instances = Vec::new();
            emit_stroke_instances(
                &mut instances,
                &points,
                dashed_line_style(),
                [1.0; 4],
                path_length,
            );
            assert_eq!(
                instances, baseline,
                "path_length {path_length} must match geometric dash"
            );
        }
    }

    #[test]
    fn solid_stroke_ignores_path_length() {
        let points = [[0.0, 0.0], [40.0, 0.0]];
        let color = [1.0; 4];
        let mut geometric = Vec::new();
        emit_stroke_instances(
            &mut geometric,
            &points,
            solid_style(2.0, &[], StrokeCap::Round),
            color,
            0.0,
        );
        let mut scaled = Vec::new();
        emit_stroke_instances(
            &mut scaled,
            &points,
            solid_style(2.0, &[], StrokeCap::Round),
            color,
            80.0,
        );
        assert_eq!(scaled, geometric);
    }

    #[test]
    fn odd_dash_repeats_and_zero_pattern_emits_nothing() {
        let mut odd = Vec::new();
        emit_stroke_instances(
            &mut odd,
            &[[0.0, 0.0], [40.0, 0.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Butt,
                dash: &[10.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(odd.len(), 2);
        let mut empty = Vec::new();
        emit_stroke_instances(
            &mut empty,
            &[[0.0, 0.0], [40.0, 0.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[0.0, 0.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn continuous_dash_keeps_join_once_across_vertices() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[100.0, 4.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Butt)
        );
        assert_eq!(
            instances[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
    }

    #[test]
    fn dash_mid_segment_uses_stroke_cap() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [40.0, 0.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[10.0, 10.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        let round = pack_caps(StrokeCap::Round, StrokeCap::Round);
        assert_eq!(instances[0].packed_caps, round);
        assert_eq!(instances[1].packed_caps, round);
        assert!((instances[1].p0[0] - 20.0).abs() < 1e-4);
        assert!((instances[1].p1[0] - 30.0).abs() < 1e-4);
    }

    #[test]
    fn continuous_dash_skips_zero_length_like_solid() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 0.0], [8.0, 8.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[100.0, 4.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Butt)
        );
        assert_eq!(
            instances[1].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
    }

    #[test]
    fn dash_square_cap_expands_open_ends_onto_butt() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [40.0, 0.0]],
            StrokeStyle {
                width: 4.0,
                widths: &[],
                cap: StrokeCap::Square,
                dash: &[10.0, 10.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        assert!((instances[0].p0[0] + 2.0).abs() < 1e-5);
        assert!((instances[0].p1[0] - 12.0).abs() < 1e-5);
        assert!((instances[1].p0[0] - 18.0).abs() < 1e-5);
        assert!((instances[1].p1[0] - 32.0).abs() < 1e-5);
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Butt)
        );
    }

    #[test]
    fn closed_dash_butts_once_when_on_across_the_close() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 0.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[100.0, 4.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert_eq!(instances.len(), 3);
        let butt = pack_caps(StrokeCap::Butt, StrokeCap::Butt);
        assert!(
            instances
                .iter()
                .all(|instance| instance.packed_caps == butt),
            "ON dash wrapping the close Butt-joins like other vertices"
        );
    }

    #[test]
    fn closed_dash_first_start_is_butt_when_wrap_on() {
        // Square length 40. Pattern [9, 7] cycle 16: ON at s=0 and at s=40−
        // (40 % 16 = 8, still in the 9 ON). First slice is 0..9 on the first
        // edge, so start Butt is wrap, not an interior vertex join.
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
            ],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[9.0, 7.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert!(
            !instances.is_empty(),
            "wrap-ON closed dash still emits slices"
        );
        assert_eq!(
            instances[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round),
            "first drawable start is Butt so the close shares one disc"
        );
        assert!((instances[0].p0[0]).abs() < 1e-4 && (instances[0].p0[1]).abs() < 1e-4);
        assert!((instances[0].p1[0] - 9.0).abs() < 1e-4);
        let last = instances.last().expect("last slice");
        assert_eq!(
            last.packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Butt),
            "last drawable end is Butt when wrap ON"
        );
        assert!((last.p1[0]).abs() < 1e-4 && (last.p1[1]).abs() < 1e-4);
    }

    #[test]
    fn closed_dash_gap_at_close_uses_stroke_cap() {
        // Cycle 10 on length 40: s=0 is ON, s=40− is OFF (last interval).
        // Open dash ends at the close use Round, same as a mid-segment cut.
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
            ],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[6.0, 4.0],
                dash_offset: 0.0,
                colors: &[],
            },
            [1.0; 4],
            0.0,
        );
        assert!(!instances.is_empty(), "gap at close still emits ON slices");
        let round = pack_caps(StrokeCap::Round, StrokeCap::Round);
        assert_eq!(
            instances[0].packed_caps, round,
            "first start is the stroke cap when the close is a gap"
        );
        assert_eq!(
            instances.last().expect("last slice").packed_caps,
            round,
            "last ON end is the stroke cap when it does not wrap"
        );
    }

    #[test]
    fn closed_dash_offset_shifts_wrap_join() {
        let points = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let style = |dash_offset: f32| StrokeStyle {
            width: 2.0,
            widths: &[],
            cap: StrokeCap::Round,
            dash: &[6.0, 4.0],
            dash_offset,
            colors: &[],
        };
        let mut no_wrap = Vec::new();
        emit_stroke_instances(&mut no_wrap, &points, style(0.0), [1.0; 4], 0.0);
        assert_eq!(
            no_wrap[0].packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Round)
        );

        let mut wrap = Vec::new();
        emit_stroke_instances(&mut wrap, &points, style(2.0), [1.0; 4], 0.0);
        assert_eq!(
            wrap[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round),
            "dash_offset 2 puts both close sides in ON"
        );
        assert_eq!(
            wrap.last().expect("last slice").packed_caps,
            pack_caps(StrokeCap::Round, StrokeCap::Butt)
        );
    }

    #[test]
    fn path_length_scales_closed_wrap_join() {
        // Square length 40, dash [9, 7] cycle 16: wrap ON without pathLength.
        // path_length = 80 samples the close at 80− (OFF), so the close is a gap.
        let points = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        let style = || StrokeStyle {
            width: 2.0,
            widths: &[],
            cap: StrokeCap::Round,
            dash: &[9.0, 7.0],
            dash_offset: 0.0,
            colors: &[],
        };
        let mut wrap = Vec::new();
        emit_stroke_instances(&mut wrap, &points, style(), [1.0; 4], 0.0);
        assert_eq!(
            wrap[0].packed_caps,
            pack_caps(StrokeCap::Butt, StrokeCap::Round)
        );
        let mut scaled = Vec::new();
        emit_stroke_instances(&mut scaled, &points, style(), [1.0; 4], 80.0);
        let round = pack_caps(StrokeCap::Round, StrokeCap::Round);
        assert_eq!(
            scaled[0].packed_caps, round,
            "scaled close is OFF, so first start uses the stroke cap"
        );
        assert_eq!(
            scaled.last().expect("last slice").packed_caps,
            round,
            "scaled close is OFF, so last ON end uses the stroke cap"
        );
    }

    #[test]
    fn per_point_colors_use_the_segment_start() {
        let mut instances = Vec::new();
        emit_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [10.0, 0.0], [10.0, 8.0]],
            StrokeStyle {
                width: 2.0,
                widths: &[],
                cap: StrokeCap::Round,
                dash: &[],
                dash_offset: 0.0,
                colors: &[
                    [1.0, 0.0, 0.0, 1.0],
                    [0.0, 1.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0, 1.0],
                ],
            },
            [0.0, 0.0, 1.0, 1.0],
            0.0,
        );
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(instances[1].color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn stamp_interns_identical_clips_once() {
        let mut instances = Vec::new();
        let mut clips = Vec::new();
        let mut intern = HashMap::new();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 0.0], [8.0, 0.0], [8.0, 8.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        stamp_fragment_clip(
            &mut instances,
            &mut clips,
            &mut intern,
            0,
            FragmentClip::PASS,
        );
        assert_eq!(clips.len(), 1);
        assert!(instances.iter().all(|instance| instance.clip_index == 0));
        let start = instances.len();
        append_stroke_instances(
            &mut instances,
            &[[0.0, 8.0], [4.0, 8.0]],
            2.0,
            &[],
            StrokeCap::Round,
            [1.0; 4],
        );
        stamp_fragment_clip(
            &mut instances,
            &mut clips,
            &mut intern,
            start,
            FragmentClip::PASS,
        );
        assert_eq!(clips.len(), 1);
        stamp_fragment_clip(
            &mut instances,
            &mut clips,
            &mut intern,
            start,
            FragmentClip::REJECT,
        );
        assert_eq!(clips.len(), 2);
        assert_eq!(instances[start].clip_index, 1);
        assert_eq!(instances[0].clip_index, 0);
    }

    #[test]
    fn intern_clip_keeps_polygon_vertices() {
        let mut clip = FragmentClip::PASS;
        clip.polygon_count = 3;
        clip.polygon[0] = [0.0, 0.0];
        clip.polygon[1] = [64.0, 0.0];
        clip.polygon[2] = [32.0, 64.0];
        let packed = GpuClip::from_fragment(clip);
        assert_eq!(packed.inv_ef_radius[3], 3.0);
        assert_eq!(packed.poly0, [0.0, 0.0, 64.0, 0.0]);
        assert_eq!(packed.poly1[0], 32.0);
        assert_eq!(packed.poly1[1], 64.0);
        assert_eq!(std::mem::size_of::<GpuClip>(), 112);
    }

    #[test]
    fn a_path_fringe_stays_one_pixel_on_screen_under_any_scale() {
        let length = |v: [f32; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
        // A vertical edge's outward normal under scaleX(0.01) and scale(3):
        // still one pixel, still along x.
        for m in [[0.01, 0.0, 0.0, 1.0], [3.0, 0.0, 0.0, 3.0]] {
            let e = screen_extrude(m, [1.0, 0.0]);
            assert!(
                (length(e) - 1.0).abs() < 1e-5 && e[1].abs() < 1e-6,
                "{m:?}: {e:?}"
            );
        }
        // A mirror keeps it pointing out of the mirrored shape.
        assert!(screen_extrude([-1.0, 0.0, 0.0, 1.0], [1.0, 0.0])[0] < 0.0);
        // A skew keeps it perpendicular to the edge on screen: the edge
        // (0,1) maps to (0.5, 1).
        let e = screen_extrude([1.0, 0.0, 0.5, 1.0], [1.0, 0.0]);
        assert!((e[0] * 0.5 + e[1]).abs() < 1e-5, "{e:?}");
        // A square corner's miter under scaleX(0.01) still pushes both of
        // its edges one pixel: (1, 1) on screen, not (1.41, 0.01).
        let corner = screen_extrude([0.01, 0.0, 0.0, 1.0], [1.0, 1.0]);
        assert!(
            (corner[0] - 1.0).abs() < 1e-4 && (corner[1] - 1.0).abs() < 1e-4,
            "{corner:?}"
        );
        // Collapsed to a line: no fringe rather than an infinite one.
        assert_eq!(screen_extrude([0.0, 0.0, 0.0, 1.0], [1.0, 0.0]), [0.0, 0.0]);
    }

    #[test]
    fn local_aa_fringe_matches_shader_one_over_sigma_min() {
        assert!((local_aa_fringe([1.0, 0.0, 0.0, 1.0], 1.0) - 1.0).abs() < 1e-5);
        let squashed = local_aa_fringe([2.0, 0.0, 0.0, 0.5], 1.0);
        assert!(
            (squashed - 2.0).abs() < 1e-4,
            "σ_min of scale(2, 0.5) is 0.5 so fringe is 2, got {squashed}"
        );
        let zoomed_out = local_aa_fringe([1.0, 0.0, 0.0, 1.0], 0.5);
        assert!(
            (zoomed_out - 2.0).abs() < 1e-5,
            "viewport 0.5 must lift the 1px physical pad into 2 local px, got {zoomed_out}"
        );
        let zoomed_in = local_aa_fringe([1.0, 0.0, 0.0, 1.0], 2.0);
        assert!(
            (zoomed_in - 0.5).abs() < 1e-5,
            "viewport 2 must keep a 1 physical px pad, got {zoomed_in}"
        );
        let combined = local_aa_fringe([2.0, 0.0, 0.0, 0.5], 0.5);
        assert!(
            (combined - 4.0).abs() < 1e-4,
            "σ_min 0.5 and viewport 0.5 must compose, got {combined}"
        );
    }

    #[test]
    fn intern_keeps_negative_zero_and_nan_payloads_distinct() {
        let mut clips = Vec::new();
        let mut intern = HashMap::new();
        let mut plus_zero = FragmentClip::PASS;
        plus_zero.rect[0] = 0.0;
        let mut minus_zero = FragmentClip::PASS;
        minus_zero.rect[0] = -0.0;
        assert_eq!(intern_clip(&mut clips, &mut intern, plus_zero), 0);
        assert_eq!(intern_clip(&mut clips, &mut intern, minus_zero), 1);
        assert_eq!(intern_clip(&mut clips, &mut intern, plus_zero), 0);
        let mut nan_a = FragmentClip::PASS;
        nan_a.corner_radius = f32::from_bits(0x7fc0_0001);
        let mut nan_b = FragmentClip::PASS;
        nan_b.corner_radius = f32::from_bits(0x7fc0_0002);
        assert_eq!(intern_clip(&mut clips, &mut intern, nan_a), 2);
        assert_eq!(intern_clip(&mut clips, &mut intern, nan_a), 2);
        assert_eq!(intern_clip(&mut clips, &mut intern, nan_b), 3);
        assert_eq!(clips.len(), 4);
    }
}

pub(super) struct MeshPipelineTarget {
    paths: PathBuffers,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    uploaded_uniforms: Option<Uniforms>,
    clips: ManagedBuffer,
    clip_capacity: usize,
    instances: ManagedBuffer,
    instance_capacity: usize,
    pending_instances: Vec<MeshInstance>,
    uploaded_instances: Vec<MeshInstance>,
    pending_clips: Vec<GpuClip>,
    clip_intern: HashMap<ClipInternKey, u32>,
    uploaded_clips: Vec<GpuClip>,
}

impl MeshPipeline {
    pub(super) fn swap_target(
        &mut self,
        target: &mut Option<MeshPipelineTarget>,
        device: &wgpu::Device,
    ) {
        let target = target.get_or_insert_with(|| {
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.mesh.uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let clips = ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.target.mesh.clips"),
                size: (INITIAL_CLIPS * std::mem::size_of::<GpuClip>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            let paths = PathBuffers::new(device);
            MeshPipelineTarget {
                bind_group: mesh_bind_group(
                    device,
                    &self.bind_layout,
                    &uniforms,
                    &clips,
                    &paths.gradients,
                ),
                paths,
                uniforms,
                uploaded_uniforms: None,
                clips,
                clip_capacity: INITIAL_CLIPS,
                instances: ManagedBuffer::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("nana.target.mesh.instances"),
                    size: (INITIAL_INSTANCES * std::mem::size_of::<MeshInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })),
                instance_capacity: INITIAL_INSTANCES,
                pending_instances: Vec::new(),
                uploaded_instances: Vec::new(),
                pending_clips: Vec::new(),
                uploaded_clips: Vec::new(),
                clip_intern: HashMap::new(),
            }
        });
        std::mem::swap(&mut self.paths, &mut target.paths);
        std::mem::swap(&mut self.bind_group, &mut target.bind_group);
        std::mem::swap(&mut self.uniforms, &mut target.uniforms);
        std::mem::swap(&mut self.uploaded_uniforms, &mut target.uploaded_uniforms);
        std::mem::swap(&mut self.clips, &mut target.clips);
        std::mem::swap(&mut self.clip_capacity, &mut target.clip_capacity);
        std::mem::swap(&mut self.instances, &mut target.instances);
        std::mem::swap(&mut self.instance_capacity, &mut target.instance_capacity);
        std::mem::swap(&mut self.pending_instances, &mut target.pending_instances);
        std::mem::swap(&mut self.uploaded_instances, &mut target.uploaded_instances);
        std::mem::swap(&mut self.pending_clips, &mut target.pending_clips);
        std::mem::swap(&mut self.clip_intern, &mut target.clip_intern);
        std::mem::swap(&mut self.uploaded_clips, &mut target.uploaded_clips);
    }
}
