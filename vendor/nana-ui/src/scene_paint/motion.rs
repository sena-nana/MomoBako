//! GPU MotionDescriptor storage + presentation-time uniform.
//!
//! Product present never reads the evaluated sample back. Tests/devtools may
//! use [`MotionGpuResources::evaluate_readback`].

use std::time::Duration;

use nana_ui_core::motion::{
    MOTION_GPU_DESCRIPTOR_SIZE, MOTION_GPU_KEYFRAME_SIZE, MOTION_GPU_TIME_SIZE, MotionGpuTime,
    MotionWorkCounters, motion_gpu_as_bytes,
};
use nana_ui_scene::UiScene;

use super::buffer_upload::upload_changed_with_work;
use crate::gpu_work::ManagedBuffer;

/// The eval pipeline and readback serve only the CPU/GPU parity tests, so
/// they are dead outside test builds.
pub(super) struct MotionGpuResources {
    descriptors: ManagedBuffer,
    keyframes: ManagedBuffer,
    time: wgpu::Buffer,
    /// What `time` holds, so a frame whose clock did not move writes nothing.
    uploaded_time: Option<MotionGpuTime>,
    bind_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    #[cfg_attr(not(test), allow(dead_code))]
    dummy_group: wgpu::BindGroup,
    #[cfg_attr(not(test), allow(dead_code))]
    eval_pipeline: Option<wgpu::RenderPipeline>,
    descriptor_capacity: usize,
    keyframe_capacity: usize,
    uploaded_descriptors: Vec<u8>,
    uploaded_keyframes: Vec<u8>,
    /// Hosts share one painter across windows, and each window document has
    /// its own descriptor store whose epochs count independently.
    last_source: u64,
    last_structure_epoch: u64,
    last_surface_generation: u64,
    last_work: MotionWorkCounters,
}

impl MotionGpuResources {
    #[cfg(test)]
    pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        Self::new_with_policy(device, layout, None, false)
    }

    pub(crate) fn new_with_policy(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        policy: Option<&nana_gpu::GpuDeviceState>,
        gpu_motion_supported: bool,
    ) -> Self {
        let bind_layout = layout.clone();
        let dummy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nana-ui.scene.motion.dummy.layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            }],
        });
        let dummy_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.motion.dummy.uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-ui.scene.motion.dummy.bind"),
            layout: &dummy_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: dummy_uniform.as_entire_binding(),
            }],
        });
        let descriptor_capacity = 1;
        let keyframe_capacity = 1;
        let descriptors = ManagedBuffer::new(storage_buffer(
            device,
            "nana-ui.scene.motion.descriptors",
            descriptor_capacity * MOTION_GPU_DESCRIPTOR_SIZE,
        ));
        let keyframes = ManagedBuffer::new(storage_buffer(
            device,
            "nana-ui.scene.motion.keyframes",
            keyframe_capacity * MOTION_GPU_KEYFRAME_SIZE,
        ));
        let time = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-ui.scene.motion.time"),
            size: MOTION_GPU_TIME_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = motion_bind_group(device, &bind_layout, &descriptors, &keyframes, &time);
        // Rgba32Float is an optional render target. Unsupported adapters keep CPU motion.
        let eval_pipeline = if gpu_motion_supported {
            policy.and_then(|policy| {
                nana_gpu::__framework::render_pipeline_state(
                    policy,
                    nana_gpu::PipelineKey {
                        generation: policy.generation(),
                        target_format: nana_gpu::__framework::format_from_wgpu(wgpu::TextureFormat::Rgba32Float),
                        sample_count: 1,
                        shader: 0x6d6f_7469_6f6e_6576,
                        layout: 5,
                        material: 0,
                        primitive: 0,
                        blend: 0,
                        depth: 0,
                        vertex_layout: 0,
                    },
                    || create_eval_pipeline(device, &dummy_layout, layout)
                        .expect("Rgba32Float motion pipeline creation"),
                ).ok()
            })
        } else {
            None
        };
        Self {
            descriptors,
            keyframes,
            time,
            uploaded_time: None,
            bind_layout,
            bind_group,
            dummy_group,
            eval_pipeline,
            descriptor_capacity,
            keyframe_capacity,
            uploaded_descriptors: Vec::new(),
            uploaded_keyframes: Vec::new(),
            last_source: 0,
            last_structure_epoch: u64::MAX,
            last_surface_generation: 0,
            last_work: MotionWorkCounters::default(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn bind_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_layout
    }

    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn last_work(&self) -> MotionWorkCounters {
        self.last_work
    }

    pub(crate) fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &UiScene) {
        self.sync_with_work(device, queue, scene, None);
    }

    pub(crate) fn sync_with_work(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &UiScene,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        self.last_work = MotionWorkCounters::default();
        let surface = scene.surface_generation();
        let epoch = scene.motion_gpu_structure_epoch();
        let source = scene.motion_gpu_source();
        let descriptors = scene.motion_gpu_descriptors();
        let keyframes = scene.motion_gpu_keyframes();
        let lost = surface != self.last_surface_generation;
        if lost {
            self.last_surface_generation = surface;
            self.last_structure_epoch = u64::MAX;
            self.uploaded_descriptors.clear();
            self.uploaded_keyframes.clear();
        }
        let need_descriptors = lost
            || source != self.last_source
            || epoch != self.last_structure_epoch
            || descriptors.len() > self.descriptor_capacity
            || keyframes.len() > self.keyframe_capacity;
        if need_descriptors {
            self.ensure_capacity(device, descriptors.len(), keyframes.len(), gpu_work);
            let desc_bytes = pad_copy(
                motion_gpu_as_bytes(descriptors),
                self.descriptor_capacity * MOTION_GPU_DESCRIPTOR_SIZE,
            );
            let kf_bytes = pad_copy(
                motion_gpu_as_bytes(keyframes),
                self.keyframe_capacity * MOTION_GPU_KEYFRAME_SIZE,
            );
            let uploaded = if self.uploaded_descriptors.len() == desc_bytes.len() {
                upload_changed_with_work(
                    queue,
                    &self.descriptors,
                    &self.uploaded_descriptors,
                    &desc_bytes,
                    gpu_work,
                )
            } else {
                if let Some(work) = gpu_work {
                    work.write_buffer(queue, &self.descriptors, 0, &desc_bytes);
                } else {
                    queue.write_buffer(&self.descriptors, 0, &desc_bytes);
                }
                desc_bytes.len()
            };
            if self.uploaded_keyframes.len() == kf_bytes.len() {
                let _ = upload_changed_with_work(
                    queue,
                    &self.keyframes,
                    &self.uploaded_keyframes,
                    &kf_bytes,
                    gpu_work,
                );
            } else {
                if let Some(work) = gpu_work {
                    work.write_buffer(queue, &self.keyframes, 0, &kf_bytes);
                } else {
                    queue.write_buffer(&self.keyframes, 0, &kf_bytes);
                }
            }
            self.uploaded_descriptors = desc_bytes;
            self.uploaded_keyframes = kf_bytes;
            self.last_source = source;
            self.last_structure_epoch = epoch;
            self.last_work.record_descriptor_upload(
                descriptors
                    .iter()
                    .filter(|descriptor| descriptor.is_live())
                    .count(),
                uploaded,
            );
        }
        let time = MotionGpuTime::new(scene.motion_gpu_now());
        if self.uploaded_time == Some(time) {
            return;
        }
        let time_bytes = motion_gpu_as_bytes(std::slice::from_ref(&time));
        if let Some(work) = gpu_work {
            work.write_buffer(queue, &self.time, 0, time_bytes);
        } else {
            queue.write_buffer(&self.time, 0, time_bytes);
        }
        self.uploaded_time = Some(time);
    }

    /// Test/devtools only. Product present must not call this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn evaluate_readback(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &UiScene,
        motion_id: u32,
        now: Duration,
    ) -> Option<MotionGpuReadback> {
        self.sync(device, queue, scene);
        let pipeline = self.eval_pipeline.as_ref()?;
        let time = MotionGpuTime::with_eval(now, motion_id);
        queue.write_buffer(
            &self.time,
            0,
            motion_gpu_as_bytes(std::slice::from_ref(&time)),
        );
        self.uploaded_time = Some(time);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-ui.scene.motion.eval.target"),
            size: wgpu::Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui.scene.motion.eval"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-ui.scene.motion.eval.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.dummy_group, &[]);
            pass.set_bind_group(1, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let pixels = readback_rgba32(device, queue, encoder, &texture);
        Some(MotionGpuReadback { pixels })
    }

    fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        descriptors: usize,
        keyframes: usize,
        gpu_work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        let mut grew = false;
        if descriptors > self.descriptor_capacity {
            self.descriptor_capacity = descriptors.next_power_of_two().max(1);
            let size = (self.descriptor_capacity * MOTION_GPU_DESCRIPTOR_SIZE) as u64;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.descriptors,
                    size,
                    usage,
                    "nana-ui.scene.motion.descriptors",
                );
            } else {
                self.descriptors = ManagedBuffer::new(storage_buffer(
                    device,
                    "nana-ui.scene.motion.descriptors",
                    size as usize,
                ));
            }
            self.uploaded_descriptors.clear();
            grew = true;
        }
        if keyframes > self.keyframe_capacity {
            self.keyframe_capacity = keyframes.next_power_of_two().max(1);
            let size = (self.keyframe_capacity * MOTION_GPU_KEYFRAME_SIZE) as u64;
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
            if let Some(work) = gpu_work {
                work.replace_buffer(
                    device,
                    &mut self.keyframes,
                    size,
                    usage,
                    "nana-ui.scene.motion.keyframes",
                );
            } else {
                self.keyframes = ManagedBuffer::new(storage_buffer(
                    device,
                    "nana-ui.scene.motion.keyframes",
                    size as usize,
                ));
            }
            self.uploaded_keyframes.clear();
            grew = true;
        }
        if grew {
            self.bind_group = motion_bind_group(
                device,
                &self.bind_layout,
                &self.descriptors,
                &self.keyframes,
                &self.time,
            );
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct MotionGpuReadback {
    pub pixels: [[f32; 4]; 2],
}

pub(super) fn pack_motion_snap(pixel_snap: u32, transform_id: u32, opacity_id: u32) -> u32 {
    (pixel_snap & 1) | ((transform_id & 0x7fff) << 1) | ((opacity_id & 0xffff) << 16)
}

fn pad_copy(bytes: &[u8], size: usize) -> Vec<u8> {
    let align = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    let size = size.max(bytes.len()).div_ceil(align) * align;
    let mut out = vec![0u8; size];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

fn storage_buffer(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    let align = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    let size = (size.max(4).div_ceil(align) * align) as u64;
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
pub(super) fn motion_bind_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    motion_bind_layout_with_policy(device, None)
}

pub(super) fn motion_bind_layout_with_policy(
    device: &wgpu::Device,
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
                nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                stages,
            ),
            nana_gpu::LogicalBinding::new(
                1,
                nana_gpu::LogicalBindingType::StorageBuffer { read_only: true },
                stages,
            ),
            nana_gpu::LogicalBinding::new(2, nana_gpu::LogicalBindingType::UniformBuffer, stages)
                .min_size(MOTION_GPU_TIME_SIZE as u64),
        ])
        .expect("motion logical table is valid")
        .for_generation(policy.generation());
        let logical = nana_gpu::__framework::logical_layout(device, policy.generation(), &table)
            .expect("motion logical layout matches the device");
        return nana_gpu::__framework::resource_layout(&logical).clone();
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("nana-ui.scene.motion.layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(MOTION_GPU_TIME_SIZE as u64),
                },
                count: None,
            },
        ],
    })
}

fn motion_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    descriptors: &wgpu::Buffer,
    keyframes: &wgpu::Buffer,
    time: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nana-ui.scene.motion.bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: descriptors.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: keyframes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: time.as_entire_binding(),
            },
        ],
    })
}

fn create_eval_pipeline(
    device: &wgpu::Device,
    dummy: &wgpu::BindGroupLayout,
    motion: &wgpu::BindGroupLayout,
) -> Option<wgpu::RenderPipeline> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("nana-ui.scene.motion.eval.shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
            "shader/motion.wgsl"
        ))),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("nana-ui.scene.motion.eval.pipeline"),
        bind_group_layouts: &[Some(dummy), Some(motion)],
        immediate_size: 0,
    });
    Some(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nana-ui.scene.motion.eval"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("motion_eval_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("motion_eval_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba32Float,
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
        }),
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn readback_rgba32(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> [[f32; 4]; 2] {
    let bytes_per_row = 256u32;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nana-ui.scene.motion.eval.readback"),
        size: u64::from(bytes_per_row),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let index = queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(index),
        timeout: None,
    });
    let data = slice
        .get_mapped_range()
        .expect("motion eval readback must be mapped");
    let mut pixels = [[0.0f32; 4]; 2];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let offset = i * 16;
        let mut words = [0u8; 16];
        words.copy_from_slice(&data[offset..offset + 16]);
        *pixel = bytemuck::cast(words);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nana_ui_core::{
        LayoutStyle, PaintMat4, PaintTransform,
        motion::{
            AnimatableProperty, DecayParams, Easing, Keyframe, MotionCurve, MotionHandle,
            MotionSample, MotionTo, MotionTrack, MotionValue, SpringParams, StepJump,
            evaluate_track,
        },
    };
    use nana_ui_runtime::{
        AnimationId, AnimationSpec, ComputedStyle, DocumentId, ExtractedNode, LayoutBox,
        MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
    };
    use nana_ui_scene::{AffineTransform, UiScene};

    use super::super::{
        ScenePaintViewport, SceneWgpuPainter,
        tests::{paint_scene_rgba, test_device},
    };
    use super::*;

    /// Same absolute tolerance as the Linear harness. Bezier bisection and
    /// closed-form spring/decay share the CPU f32 formulas in WGSL.
    const GPU_CPU_TOL: f32 = 1e-3;

    fn id(value: u64) -> StableNodeId {
        StableNodeId::new(value).unwrap()
    }

    fn projective_motion_fixture_with_target(
        now: Duration,
        target: PaintTransform,
    ) -> (UiWorld, UiScene, UiScene, MotionHandle, MotionValue) {
        let projective = PaintMat4::perspective(800.0)
            .expect("perspective")
            .then(PaintMat4::rotate_y(30_f32.to_radians()));
        projective_motion_fixture_with_target_and_base(now, target, projective)
    }

    fn projective_motion_fixture_with_target_and_base(
        now: Duration,
        target: PaintTransform,
        projective: PaintMat4,
    ) -> (UiWorld, UiScene, UiScene, MotionHandle, MotionValue) {
        projective_motion_fixture_with_range_and_base(
            now,
            PaintTransform::default(),
            target,
            projective,
        )
    }

    fn projective_motion_fixture_with_range_and_base(
        now: Duration,
        from: PaintTransform,
        target: PaintTransform,
        projective: PaintMat4,
    ) -> (UiWorld, UiScene, UiScene, MotionHandle, MotionValue) {
        let parent = id(1);
        let node = id(2);
        let spec = AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            node,
            Duration::ZERO,
            Duration::from_millis(400),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Transform)
        .with_range(
            MotionValue::Transform(from),
            MotionTo::Value(MotionValue::Transform(target)),
        );
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(parent, DocumentId::new(1).unwrap(), NodeKind::Document);
        queue.create(
            node,
            DocumentId::new(1).unwrap(),
            NodeKind::Element { tag: "div".into() },
        );
        queue.insert(parent, node, None);
        queue.write_layout(
            parent,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 128.0,
                height: 96.0,
            },
        );
        queue.write_layout(
            node,
            LayoutBox {
                x: 20.0,
                y: 20.0,
                width: 48.0,
                height: 32.0,
            },
        );
        queue.set_style(
            parent,
            NodeStyle {
                layout: std::sync::Arc::new(LayoutStyle {
                    transform_3d: Some(projective),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.set_style(
            node,
            NodeStyle {
                layout: std::sync::Arc::new(LayoutStyle {
                    background: Some([1.0, 0.0, 0.0, 1.0]),
                    transform_origin: Some(nana_ui_core::TransformOrigin {
                        x: nana_ui_core::LengthSpec::Percent(25.0),
                        y: nana_ui_core::LengthSpec::Percent(75.0),
                    }),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.start_animation(spec);
        world.commit(queue).unwrap();
        world.advance_animations(Duration::ZERO);
        let mut extracted = world.extract_nodes(&[parent, node]);
        for extracted_node in &mut extracted {
            extracted_node.style = std::sync::Arc::new(ComputedStyle {
                background: (extracted_node.id == node).then_some([1.0, 0.0, 0.0, 1.0]),
                ..ComputedStyle::default()
            });
        }
        let mut scene = UiScene::new();
        scene.apply_delta(extracted.clone(), []);
        scene.apply_presentation(
            world.presentation_store(),
            now,
            Some(world.motion_descriptors()),
        );
        let handle = world
            .motion_descriptors()
            .handle_for(nana_ui_core::motion::MotionTrackId::new(1).unwrap())
            .expect("transform descriptor");
        let value = world
            .motion_descriptors()
            .evaluate(handle, now)
            .expect("transform sample")
            .value;
        let MotionValue::Transform(value) = value else {
            panic!("expected transform sample")
        };

        let mut reference_nodes = extracted;
        let reference = reference_nodes
            .iter_mut()
            .find(|extracted_node| extracted_node.id == node)
            .expect("animated node");
        std::sync::Arc::make_mut(&mut reference.source_style.layout).transform = Some(value);
        let mut reference_scene = UiScene::new();
        reference_scene.apply_delta(reference_nodes, []);
        (
            world,
            scene,
            reference_scene,
            handle,
            MotionValue::Transform(value),
        )
    }

    fn projective_motion_fixture(
        now: Duration,
    ) -> (UiWorld, UiScene, UiScene, MotionHandle, MotionValue) {
        projective_motion_fixture_with_target(
            now,
            PaintTransform {
                a: 1.4,
                b: 0.08,
                c: -0.12,
                d: 0.82,
                e: 12.0,
                f: 5.0,
            },
        )
    }

    fn opacity_spec(curve: MotionCurve, to: MotionTo) -> AnimationSpec {
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            id(1),
            Duration::ZERO,
            Duration::from_millis(400),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Opacity)
        .with_curve(curve)
        .with_range(MotionValue::Scalar(0.0), to)
    }

    fn compositor_motion_scene(
        spec: AnimationSpec,
        present_at: Duration,
    ) -> (UiWorld, UiScene, MotionHandle, MotionTrack) {
        let track = spec.to_motion_track().expect("valid compositor track");
        let mut world = UiWorld::new();
        let node = spec.target;
        let mut queue = MutationQueue::new();
        queue.create(node, DocumentId::new(1).unwrap(), NodeKind::Document);
        queue.start_animation(spec);
        world.commit(queue).unwrap();
        world.advance_animations(Duration::ZERO);
        let mut extracted: Vec<ExtractedNode> = world.extract_nodes(&[node]);
        extracted[0].layout = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
        };
        extracted[0].source_style = NodeStyle {
            layout: std::sync::Arc::new(LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                ..LayoutStyle::default()
            }),
            ..extracted[0].source_style.clone()
        };
        extracted[0].style = std::sync::Arc::new(ComputedStyle {
            background: Some([1.0, 0.0, 0.0, 1.0]),
            ..ComputedStyle::default()
        });
        let mut scene = UiScene::new();
        scene.apply_delta(extracted, []);
        scene.apply_presentation(
            world.presentation_store(),
            present_at,
            Some(world.motion_descriptors()),
        );
        let layer = scene
            .compositor_layer(node)
            .expect("promoted compositor layer");
        let handle = layer.bindings[0].handle();
        (world, scene, handle, track)
    }

    fn animated_opacity_scene(now: Duration) -> (UiWorld, UiScene, MotionHandle) {
        let (world, scene, handle, _) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Easing(Easing::Linear),
                MotionTo::Value(MotionValue::Scalar(1.0)),
            ),
            now,
        );
        (world, scene, handle)
    }

    fn assert_cpu_samples_match(via_desc: &MotionSample, via_track: &MotionSample, label: &str) {
        assert_eq!(
            via_desc.applies, via_track.applies,
            "{label}: evaluate_descriptor applies != evaluate_track"
        );
        assert!(
            (via_desc.progress - via_track.progress).abs() < GPU_CPU_TOL,
            "{label}: evaluate_descriptor progress {} != evaluate_track {}",
            via_desc.progress,
            via_track.progress
        );
        match (via_desc.value, via_track.value) {
            (MotionValue::Scalar(left), MotionValue::Scalar(right)) => {
                assert!(
                    (left - right).abs() < GPU_CPU_TOL,
                    "{label}: evaluate_descriptor {left} != evaluate_track {right}"
                );
            }
            (MotionValue::Transform(left), MotionValue::Transform(right)) => {
                for (name, l, r) in [
                    ("a", left.a, right.a),
                    ("b", left.b, right.b),
                    ("c", left.c, right.c),
                    ("d", left.d, right.d),
                    ("e", left.e, right.e),
                    ("f", left.f, right.f),
                ] {
                    assert!(
                        (l - r).abs() < GPU_CPU_TOL,
                        "{label}: evaluate_descriptor {name} {l} != evaluate_track {r}"
                    );
                }
            }
            (left, right) => assert_eq!(left, right, "{label}: CPU value kinds"),
        }
    }

    fn assert_gpu_matches_cpu(cpu: &MotionSample, readback: &MotionGpuReadback, label: &str) {
        match cpu.value {
            MotionValue::Scalar(expected) => {
                assert!(
                    (readback.pixels[0][0] - expected).abs() < GPU_CPU_TOL,
                    "{label}: cpu scalar {expected} gpu {}",
                    readback.pixels[0][0]
                );
            }
            MotionValue::Transform(expected) => {
                let gpu = [
                    readback.pixels[0][0],
                    readback.pixels[0][1],
                    readback.pixels[0][2],
                    readback.pixels[0][3],
                    readback.pixels[1][0],
                    readback.pixels[1][1],
                ];
                let cpu_channels = [
                    expected.a, expected.b, expected.c, expected.d, expected.e, expected.f,
                ];
                for (name, cpu_v, gpu_v) in ["a", "b", "c", "d", "e", "f"]
                    .into_iter()
                    .zip(cpu_channels)
                    .zip(gpu)
                    .map(|((name, cpu_v), gpu_v)| (name, cpu_v, gpu_v))
                {
                    assert!(
                        (cpu_v - gpu_v).abs() < GPU_CPU_TOL,
                        "{label}: cpu {name} {cpu_v} gpu {gpu_v}"
                    );
                }
            }
            other => panic!("{label}: unsupported CPU value {other:?}"),
        }
        assert!(
            (readback.pixels[1][2] - cpu.progress).abs() < GPU_CPU_TOL,
            "{label}: cpu progress {} gpu {}",
            cpu.progress,
            readback.pixels[1][2]
        );
        assert_eq!(
            readback.pixels[1][3],
            if cpu.applies { 1.0 } else { 0.0 },
            "{label}: applies"
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "parity fixture: GPU harness plus one track and its timestamps"
    )]
    fn assert_cpu_gpu_eval_parity(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        world: &UiWorld,
        scene: &UiScene,
        handle: MotionHandle,
        track: &MotionTrack,
        timestamps: &[Duration],
        label: &str,
    ) {
        let mut gpu = MotionGpuResources::new(device, &motion_bind_layout(device));
        for now in timestamps {
            let via_desc = world
                .motion_descriptors()
                .evaluate(handle, *now)
                .expect("evaluate_descriptor");
            let via_track = evaluate_track(track, *now);
            let stamp = format!("{label} @{}ms", now.as_millis());
            assert_cpu_samples_match(&via_desc, &via_track, &stamp);
            let readback = gpu
                .evaluate_readback(device, queue, scene, handle.index().saturating_add(1), *now)
                .expect("evaluate_readback");
            assert_gpu_matches_cpu(&via_desc, &readback, &stamp);
        }
    }

    fn assert_scalar_not_linear(cpu: &MotionSample, linear: f32, label: &str) {
        let MotionValue::Scalar(value) = cpu.value else {
            panic!("{label}: expected scalar CPU sample");
        };
        assert!(
            (value - linear).abs() > 0.04,
            "{label}: CPU {value} is too close to linear {linear}; this timestamp does not exercise the curve"
        );
    }

    fn paint_once(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        painter: &mut SceneWgpuPainter,
        scene: &UiScene,
    ) {
        let viewport = ScenePaintViewport {
            logical_size: [64.0, 64.0],
            physical_size: [64, 64],
            scale_factor: 1.0,
            scene_origin: [0.0, 0.0],
            target_origin: [0.0, 0.0],
            clear_color: [0.0, 0.0, 0.0, 1.0],
            clear: true,
        };
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("nana-ui.motion.paint"),
                size: wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nana-ui.motion.paint.encoder"),
        });
        painter
            .paint_encoder(scene, &mut encoder, &view, viewport, None, None)
            .unwrap();
        queue.submit(Some(encoder.finish()));
    }

    #[test]
    fn gpu_evaluate_matches_cpu_descriptor() {
        let (device, queue) = test_device();
        let now = Duration::from_millis(200);
        let (world, scene, handle) = animated_opacity_scene(Duration::from_millis(16));
        let cpu = world
            .motion_descriptors()
            .evaluate(handle, now)
            .expect("cpu evaluate");
        let mut gpu = MotionGpuResources::new(&device, &motion_bind_layout(&device));
        let readback = gpu
            .evaluate_readback(
                &device,
                &queue,
                &scene,
                handle.index().saturating_add(1),
                now,
            )
            .expect("gpu evaluate");
        match cpu.value {
            MotionValue::Scalar(expected) => {
                assert!(
                    (readback.pixels[0][0] - expected).abs() < 1e-3,
                    "cpu {expected} gpu {}",
                    readback.pixels[0][0]
                );
            }
            other => panic!("expected scalar, got {other:?}"),
        }
        assert!((readback.pixels[1][2] - cpu.progress).abs() < 1e-3);
        assert_eq!(readback.pixels[1][3], if cpu.applies { 1.0 } else { 0.0 });
    }

    #[test]
    fn gpu_evaluate_matches_cpu_cubic_bezier() {
        let (device, queue) = test_device();
        let (world, scene, handle, track) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Easing(Easing::CubicBezier([0.2, 0.8, 0.2, 1.0])),
                MotionTo::Value(MotionValue::Scalar(1.0)),
            ),
            Duration::from_millis(16),
        );
        let mid = Duration::from_millis(200);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, mid)
            .expect("evaluate_descriptor");
        assert_scalar_not_linear(&via_desc, 0.5, "cubic-bezier @200ms");
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[Duration::from_millis(100), mid],
            "cubic-bezier",
        );
    }

    #[test]
    fn gpu_evaluate_matches_cpu_steps() {
        let (device, queue) = test_device();
        let (world, scene, handle, track) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Steps {
                    count: 4,
                    jump: StepJump::End,
                },
                MotionTo::Value(MotionValue::Scalar(1.0)),
            ),
            Duration::from_millis(16),
        );
        let mid = Duration::from_millis(150);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, mid)
            .expect("evaluate_descriptor");
        assert_scalar_not_linear(&via_desc, 0.375, "steps @150ms");
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[Duration::from_millis(50), mid],
            "steps",
        );
    }

    #[test]
    fn gpu_evaluate_matches_cpu_spring() {
        let (device, queue) = test_device();
        // Underdamped so WGSL hits the sin/cos branch (zeta ≈ 0.45).
        let (world, scene, handle, track) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Spring(SpringParams::new(180.0, 12.0, 1.0)),
                MotionTo::Value(MotionValue::Scalar(1.0)),
            ),
            Duration::from_millis(16),
        );
        let mid = Duration::from_millis(80);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, mid)
            .expect("evaluate_descriptor");
        assert_scalar_not_linear(&via_desc, 0.2, "spring @80ms");
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[mid, Duration::from_millis(120)],
            "spring",
        );
    }

    #[test]
    fn gpu_evaluate_matches_cpu_decay() {
        let (device, queue) = test_device();
        let mut spec = opacity_spec(
            MotionCurve::Decay(DecayParams::new(0.2)),
            MotionTo::Value(MotionValue::Scalar(1.0)),
        );
        spec.velocity = MotionValue::Scalar(5.0);
        let (world, scene, handle, track) =
            compositor_motion_scene(spec, Duration::from_millis(16));
        let mid = Duration::from_millis(60);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, mid)
            .expect("evaluate_descriptor");
        assert_scalar_not_linear(&via_desc, 0.15, "decay @60ms");
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[Duration::from_millis(40), mid],
            "decay",
        );
    }

    #[test]
    fn gpu_evaluate_matches_cpu_keyframes() {
        let (device, queue) = test_device();
        let (world, scene, handle, track) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Easing(Easing::Linear),
                MotionTo::Keyframes(vec![
                    Keyframe {
                        offset: 0.0,
                        value: MotionValue::Scalar(0.0),
                        easing: None,
                    },
                    Keyframe {
                        offset: 0.4,
                        value: MotionValue::Scalar(0.8),
                        easing: Some(Easing::EaseOutCubic),
                    },
                    Keyframe {
                        offset: 1.0,
                        value: MotionValue::Scalar(0.2),
                        easing: Some(Easing::CubicBezier([0.2, 0.8, 0.2, 1.0])),
                    },
                ]),
            ),
            Duration::from_millis(16),
        );
        let mid = Duration::from_millis(160);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, mid)
            .expect("evaluate_descriptor");
        assert_scalar_not_linear(&via_desc, 0.4, "keyframes @160ms");
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[Duration::from_millis(80), mid],
            "keyframes",
        );
    }

    #[test]
    fn gpu_evaluate_matches_cpu_transform_bezier() {
        let (device, queue) = test_device();
        let from = PaintTransform::default();
        let to = PaintTransform {
            e: 40.0,
            ..PaintTransform::default()
        };
        let spec = AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            id(1),
            Duration::ZERO,
            Duration::from_millis(400),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Transform)
        .with_curve(MotionCurve::Easing(Easing::CubicBezier([
            0.2, 0.8, 0.2, 1.0,
        ])))
        .with_range(
            MotionValue::Transform(from),
            MotionTo::Value(MotionValue::Transform(to)),
        );
        let (world, scene, handle, track) =
            compositor_motion_scene(spec, Duration::from_millis(16));
        let now = Duration::from_millis(200);
        let via_desc = world
            .motion_descriptors()
            .evaluate(handle, now)
            .expect("evaluate_descriptor");
        match via_desc.value {
            MotionValue::Transform(value) => {
                assert!(
                    (value.e - 20.0).abs() > 0.04,
                    "transform bezier @200ms e={} is too close to linear 20",
                    value.e
                );
            }
            other => panic!("expected transform, got {other:?}"),
        }
        assert_cpu_gpu_eval_parity(
            &device,
            &queue,
            &world,
            &scene,
            handle,
            &track,
            &[Duration::from_millis(100), now],
            "transform cubic-bezier",
        );
    }

    #[test]
    fn gpu_transform_overlay_scales_around_transform_origin() {
        let node = id(1);
        let spec = AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            node,
            Duration::ZERO,
            Duration::from_millis(400),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Transform)
        .with_range(
            MotionValue::Transform(PaintTransform {
                a: 0.0,
                ..PaintTransform::default()
            }),
            MotionTo::Value(MotionValue::Transform(PaintTransform::default())),
        );
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(node, DocumentId::new(1).unwrap(), NodeKind::Document);
        queue.start_animation(spec);
        world.commit(queue).unwrap();
        world.advance_animations(Duration::ZERO);
        let mut extracted: Vec<ExtractedNode> = world.extract_nodes(&[node]);
        // The right half of the target, pivoting on its right edge.
        extracted[0].layout = LayoutBox {
            x: 32.0,
            y: 0.0,
            width: 32.0,
            height: 64.0,
        };
        extracted[0].source_style = NodeStyle {
            layout: std::sync::Arc::new(LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                transform_origin: Some(nana_ui_core::TransformOrigin {
                    x: nana_ui_core::LengthSpec::Percent(100.0),
                    y: nana_ui_core::LengthSpec::Percent(50.0),
                }),
                ..LayoutStyle::default()
            }),
            ..extracted[0].source_style.clone()
        };
        extracted[0].style = std::sync::Arc::new(ComputedStyle {
            background: Some([1.0, 0.0, 0.0, 1.0]),
            ..ComputedStyle::default()
        });
        let mut scene = UiScene::new();
        scene.apply_delta(extracted, []);
        scene.apply_presentation(
            world.presentation_store(),
            Duration::from_millis(200),
            Some(world.motion_descriptors()),
        );
        assert_ne!(
            scene.compositor_gpu_motion_ids(node).0,
            0,
            "the quad must take the GPU transform path"
        );

        let (device, queue) = test_device();
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut painter,
            &scene,
            [64.0, 64.0],
            [64, 64],
            1.0,
        );
        let red_at = |x: usize| pixels[(32 * 64 + x) * 4] > 128;
        // Half scale around the right edge covers x in [48, 64); around the
        // scene origin it would cover [16, 32).
        assert!(red_at(56), "the collapsed side must grow from the pivot");
        assert!(!red_at(40), "no paint left of the half-scaled box");
        assert!(
            !red_at(24),
            "the overlay must not pivot on the scene origin"
        );
    }

    #[test]
    fn gpu_projective_motion_matches_cpu_homography() {
        let now = Duration::from_millis(200);
        let (world, scene, reference_scene, _handle, MotionValue::Transform(motion)) =
            projective_motion_fixture(now)
        else {
            panic!("expected transform fixture")
        };
        let node = id(2);
        let actual_draw = scene
            .primitives()
            .find(|primitive| {
                primitive.node == node
                    && matches!(
                        primitive.kind,
                        nana_ui_scene::ScenePrimitiveKind::Quad { .. }
                    )
            })
            .and_then(|primitive| scene.draw_primitive(primitive.id))
            .expect("animated quad");
        let reference_draw = reference_scene
            .primitives()
            .find(|primitive| {
                primitive.node == node
                    && matches!(
                        primitive.kind,
                        nana_ui_scene::ScenePrimitiveKind::Quad { .. }
                    )
            })
            .and_then(|primitive| reference_scene.draw_primitive(primitive.id))
            .expect("reference quad");
        let encode = scene.compositor_paint_encode(
            actual_draw.node,
            &actual_draw.kind,
            actual_draw.transform,
            actual_draw.paint_opacity,
        );
        assert_ne!(
            encode.motion_ids.0, 0,
            "the animated quad must use GPU motion"
        );
        assert!(encode.transform.is_projective(), "base must be projective");
        let origin = encode.transform_origin;
        let local_motion = AffineTransform::from_matrix(motion.around_origin(
            20.0,
            20.0,
            origin[0] - 20.0,
            origin[1] - 20.0,
        ));
        let expected = encode.transform.then(local_motion);
        assert_eq!(expected, reference_draw.transform);
        assert_eq!(expected.1, reference_draw.transform.1);

        let (device, queue) = test_device();
        let actual_pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm),
            &scene,
            [128.0, 96.0],
            [128, 96],
            1.0,
        );
        let reference_pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm),
            &reference_scene,
            [128.0, 96.0],
            [128, 96],
            1.0,
        );
        let differing = actual_pixels
            .iter()
            .zip(reference_pixels.iter())
            .filter(|(actual, reference)| actual.abs_diff(**reference) > 3)
            .count();
        assert!(
            differing < 512,
            "GPU projective motion diverged from CPU reference at {differing} bytes"
        );
        assert_eq!(world.presentation_values_cpu_sampled(), 0);
    }

    #[test]
    fn projective_motion_singular_denominator_fails_closed_in_cpu_contract() {
        let base = AffineTransform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [0.01, 0.0]);
        let translation = AffineTransform::from_matrix([1.0, 0.0, 0.0, 1.0, -100.0, 0.0]);
        assert_eq!(base.then(translation), AffineTransform::IDENTITY);
    }

    #[test]
    fn gpu_projective_motion_singular_denominator_matches_fail_closed_reference() {
        let mut projective = PaintMat4::IDENTITY;
        projective.m[3] = 0.0052083335;
        let (_, persp) = LayoutStyle {
            transform_3d: Some(projective),
            ..LayoutStyle::default()
        }
        .world_scene_transform(0.0, 0.0, 128.0, 96.0)
        .expect("projective base");
        assert_eq!(persp, [0.007812501, 0.0]);
        let now = Duration::from_millis(200);
        // Both keyframes are singular, so the midpoint keeps k exactly zero
        // in both CPU and WGSL f32 math instead of depending on interpolation
        // rounding at a large translation.
        let singular = PaintTransform {
            e: -128.0,
            ..PaintTransform::default()
        };
        let (_world, scene, reference_scene, _handle, MotionValue::Transform(motion)) =
            projective_motion_fixture_with_range_and_base(now, singular, singular, projective)
        else {
            panic!("expected transform fixture")
        };
        let node = id(2);
        let actual_draw = scene
            .primitives()
            .find(|primitive| primitive.node == node)
            .and_then(|primitive| scene.draw_primitive(primitive.id))
            .expect("animated quad");
        let reference_draw = reference_scene
            .primitives()
            .find(|primitive| primitive.node == node)
            .and_then(|primitive| reference_scene.draw_primitive(primitive.id))
            .expect("reference quad");
        let encode = scene.compositor_paint_encode(
            actual_draw.node,
            &actual_draw.kind,
            actual_draw.transform,
            actual_draw.paint_opacity,
        );
        let origin = encode.transform_origin;
        let local_motion = AffineTransform::from_matrix(motion.around_origin(
            20.0,
            20.0,
            origin[0] - 20.0,
            origin[1] - 20.0,
        ));
        assert_eq!(
            encode.transform.then(local_motion),
            AffineTransform::IDENTITY
        );
        assert_eq!(reference_draw.transform, AffineTransform::IDENTITY);

        // The singular branch intentionally returns the fail-closed identity
        // instead of attempting to rasterize an undefined projective map.
        let (device, queue) = test_device();
        let actual_pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm),
            &scene,
            [128.0, 96.0],
            [128, 96],
            1.0,
        );
        let reference_pixels = paint_scene_rgba(
            &device,
            &queue,
            &mut SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm),
            &reference_scene,
            [128.0, 96.0],
            [128, 96],
            1.0,
        );
        assert_eq!(actual_pixels, reference_pixels);
    }

    #[test]
    fn steady_frames_upload_descriptors_once() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut painter = SceneWgpuPainter::for_test(format);
        let (mut world, mut scene, _) = animated_opacity_scene(Duration::from_millis(16));
        paint_once(&device, &queue, &mut painter, &scene);
        let first = painter.last_motion_work();
        assert!(
            first.motion_descriptors_uploaded > 0,
            "start must upload live descriptors"
        );
        world.advance_animations(Duration::from_millis(32));
        scene.apply_presentation(
            world.presentation_store(),
            world.animation_now(),
            Some(world.motion_descriptors()),
        );
        paint_once(&device, &queue, &mut painter, &scene);
        let second = painter.last_motion_work();
        assert_eq!(
            second.motion_descriptors_uploaded, 0,
            "steady timestamp must not reupload the descriptor table"
        );
        assert_eq!(second.motion_descriptor_bytes_uploaded, 0);
    }

    /// Scene identity is refreshed by any node change. Keyed on that rather
    /// than on the descriptor source, every scroll, keystroke or hover would
    /// re-pack and re-diff the whole table and report work `last_motion_work`
    /// promises is only there when the table actually changed.
    #[test]
    fn a_scene_mutation_alone_does_not_reupload_the_descriptor_table() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut painter = SceneWgpuPainter::for_test(format);
        let spec = opacity_spec(
            MotionCurve::Easing(Easing::Linear),
            MotionTo::Value(MotionValue::Scalar(1.0)),
        );
        let node = spec.target;
        let (world, mut scene, _, _) = compositor_motion_scene(spec, Duration::from_millis(16));
        paint_once(&device, &queue, &mut painter, &scene);
        assert!(painter.last_motion_work().motion_descriptors_uploaded > 0);

        let instance = scene.instance_id();
        let epoch = scene.motion_gpu_structure_epoch();
        let mut extracted = world.extract_nodes(&[node]);
        extracted[0].layout = LayoutBox {
            x: 0.0,
            y: 8.0,
            width: 64.0,
            height: 64.0,
        };
        scene.apply_delta(extracted, []);
        assert_ne!(scene.instance_id(), instance, "the node change must land");
        assert_eq!(
            scene.motion_gpu_structure_epoch(),
            epoch,
            "moving a box does not restructure motion"
        );

        paint_once(&device, &queue, &mut painter, &scene);
        let work = painter.last_motion_work();
        assert_eq!(work.motion_descriptors_uploaded, 0);
        assert_eq!(work.motion_descriptor_bytes_uploaded, 0);
    }

    #[test]
    fn a_painter_shared_by_windows_uploads_each_documents_descriptors() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut painter = SceneWgpuPainter::for_test(format);
        let now = Duration::from_millis(200);
        // Two window documents with identical structure epochs but different
        // motion: one fades to opaque, the other stays transparent.
        let (world_a, scene_a, handle_a, _) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Easing(Easing::Linear),
                MotionTo::Value(MotionValue::Scalar(1.0)),
            ),
            Duration::from_millis(16),
        );
        let (world_b, scene_b, handle_b, _) = compositor_motion_scene(
            opacity_spec(
                MotionCurve::Easing(Easing::Linear),
                MotionTo::Value(MotionValue::Scalar(0.0)),
            ),
            Duration::from_millis(16),
        );
        assert_eq!(
            scene_a.motion_gpu_structure_epoch(),
            scene_b.motion_gpu_structure_epoch()
        );
        assert_eq!(handle_a, handle_b);
        paint_once(&device, &queue, &mut painter, &scene_a);
        paint_once(&device, &queue, &mut painter, &scene_b);
        assert!(
            painter.last_motion_work().motion_descriptors_uploaded > 0,
            "another document's table must replace the shared upload"
        );
        for (world, scene, handle) in [
            (&world_a, &scene_a, handle_a),
            (&world_b, &scene_b, handle_b),
        ] {
            let cpu = world
                .motion_descriptors()
                .evaluate(handle, now)
                .expect("cpu evaluate");
            let readback = painter
                .motion
                .evaluate_readback(&device, &queue, scene, handle.index() + 1, now)
                .expect("gpu evaluate");
            let MotionValue::Scalar(expected) = cpu.value else {
                panic!("expected scalar");
            };
            assert!(
                (readback.pixels[0][0] - expected).abs() < GPU_CPU_TOL,
                "cpu {expected} gpu {}",
                readback.pixels[0][0]
            );
        }
    }

    #[test]
    fn surface_generation_rebuilds_descriptor_upload() {
        let (device, queue) = test_device();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut painter = SceneWgpuPainter::for_test(format);
        let (world, mut scene, _) = animated_opacity_scene(Duration::from_millis(16));
        paint_once(&device, &queue, &mut painter, &scene);
        scene.set_surface_generation(scene.surface_generation().wrapping_add(1));
        scene.apply_presentation(
            world.presentation_store(),
            Duration::from_millis(16),
            Some(world.motion_descriptors()),
        );
        paint_once(&device, &queue, &mut painter, &scene);
        assert!(
            painter.last_motion_work().motion_descriptors_uploaded > 0,
            "device/surface generation change must reupload descriptors"
        );
    }
}
