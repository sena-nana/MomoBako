//! Observed GPU work for Issue #8 counters.
//!
//! Values are recorded only on a real encode/submit path. CPU-only Runtime
//! drains never construct this sink, so WorkCounters GPU fields stay `None`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLockReadGuard};
use std::time::Duration;

use nana_gpu::{GpuContext, GpuDeviceState, TransientResourceKey};
use nana_ui_core::{FrameStage, GpuWorkObservation};
use nana_ui_runtime::FrameProfiler;

pub(crate) type TransientBufferRegistry = Arc<Mutex<Vec<(TransientResourceKey, wgpu::Buffer)>>>;

/// What a sink needs from the frame it records into.
pub(crate) struct FrameLink {
    pub(crate) transient_registry: TransientBufferRegistry,
    pub(crate) uploads: nana_gpu::__framework::FrameUploadHandle,
}

impl FrameLink {
    pub(crate) fn of(frame: &nana_gpu::FrameContext) -> Self {
        Self {
            transient_registry: nana_gpu::__framework::transient_registry(frame),
            uploads: nana_gpu::__framework::frame_uploads(frame),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ManagedBuffer(Option<wgpu::Buffer>);

impl ManagedBuffer {
    pub(crate) fn new(buffer: wgpu::Buffer) -> Self {
        Self(Some(buffer))
    }

    fn take(&mut self) -> wgpu::Buffer {
        self.0.take().expect("managed buffer slot is populated")
    }

    fn put(&mut self, buffer: wgpu::Buffer) {
        debug_assert!(self.0.is_none());
        self.0 = Some(buffer);
    }
}

impl Deref for ManagedBuffer {
    type Target = wgpu::Buffer;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("managed buffer slot is populated")
    }
}

impl DerefMut for ManagedBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("managed buffer slot is populated")
    }
}

/// Shared accumulator for one Scene/WGPU frame.
#[derive(Debug, Default)]
pub struct GpuWorkSink {
    work: RefCell<GpuWorkObservation>,
    policy: Option<GpuDeviceState>,
    gpu: Option<GpuContext>,
    transient_registry: Option<TransientBufferRegistry>,
    /// The frame's upload recorder. Without one (tests, offscreen encoders)
    /// writes go straight to the queue.
    uploads: Option<nana_gpu::__framework::FrameUploadHandle>,
}

impl GpuWorkSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_policy(policy: &GpuDeviceState) -> Self {
        Self {
            work: RefCell::new(GpuWorkObservation::default()),
            policy: Some(policy.clone()),
            gpu: None,
            transient_registry: None,
            uploads: None,
        }
    }

    pub fn with_gpu(gpu: &GpuContext) -> Self {
        Self {
            work: RefCell::new(GpuWorkObservation::default()),
            policy: Some(gpu.policy().clone()),
            gpu: Some(gpu.clone()),
            transient_registry: None,
            uploads: None,
        }
    }

    /// A sink bound to one frame: pooled buffers are held until its
    /// submission completes and writes go through its upload recorder.
    pub(crate) fn with_frame(gpu: &GpuContext, frame: Option<FrameLink>) -> Self {
        let (transient_registry, uploads) = frame.map_or((None, None), |frame| {
            (Some(frame.transient_registry), Some(frame.uploads))
        });
        Self {
            work: RefCell::new(GpuWorkObservation::default()),
            policy: Some(gpu.policy().clone()),
            gpu: Some(gpu.clone()),
            transient_registry,
            uploads,
        }
    }

    /// Hold the host submission lock while a renderer maps a queue staging
    /// view and records the copy that consumes it. Surface reconfiguration
    /// takes the write side of the same lock.
    pub(crate) fn lock_submission(&self) -> Option<RwLockReadGuard<'_, ()>> {
        self.gpu
            .as_ref()
            .map(nana_gpu::__framework::lock_submission)
    }

    /// Replace a persistent renderer buffer through the policy pool. The old
    /// buffer is held by the current frame and returned only after submission
    /// completion (or immediately when the frame is discarded).
    pub(crate) fn replace_buffer(
        &self,
        device: &wgpu::Device,
        slot: &mut ManagedBuffer,
        size: u64,
        usage: wgpu::BufferUsages,
        label: &'static str,
    ) {
        // The slot owns the buffer through Option, so a failed allocation or
        // unwind can never leave an uninitialized WGPU object behind.
        let old = slot.take();
        let Some(gpu) = &self.gpu else {
            let next = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            });
            slot.put(next);
            drop(old);
            return;
        };
        let Some(registry) = &self.transient_registry else {
            // A sink not bound to a FrameContext cannot prove queue
            // completion, so it must not acquire or recycle a pooled buffer.
            let next = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            });
            slot.put(next);
            drop(old);
            return;
        };
        let key = TransientResourceKey::buffer(gpu.generation(), usage.bits(), size);
        let next = nana_gpu::__framework::acquire_transient_buffer(
            gpu.policy(),
            &key,
            device,
            label,
            usage,
        );
        let next = if let Ok(next) = next {
            registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((key, old));
            next
        } else {
            let next = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            });
            drop(old);
            next
        };
        slot.put(next);
    }

    /// Write `bytes` into `target` at `offset`, as `queue.write_buffer`
    /// would: in a frame, through its upload recorder, which stages every
    /// write of the frame in one chunk and copies them ahead of it.
    pub(crate) fn write_buffer(
        &self,
        queue: &wgpu::Queue,
        target: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) {
        if let Some(uploads) = &self.uploads {
            uploads.write_buffer(target, offset, bytes);
        } else if bytes
            .len()
            .is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT as usize)
        {
            queue.write_buffer(target, offset, bytes);
        } else {
            let mut padded = bytes.to_vec();
            padded.resize(bytes.len().next_multiple_of(4), 0);
            queue.write_buffer(target, offset, &padded);
        }
        self.count_upload(bytes.len());
    }

    /// Write a region of mip 0 of `texture`, as `queue.write_texture` would.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_texture(
        &self,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        origin: [u32; 3],
        bytes: &[u8],
        bytes_per_row: u32,
        rows_per_image: u32,
        extent: [u32; 3],
    ) {
        self.write_texture_region(
            queue,
            texture,
            0,
            origin,
            bytes,
            bytes_per_row,
            rows_per_image,
            extent,
        );
    }

    /// Write the whole of mip `mip_level` of `texture`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_texture_level(
        &self,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        mip_level: u32,
        bytes: &[u8],
        bytes_per_row: u32,
        rows_per_image: u32,
        extent: [u32; 3],
    ) {
        self.write_texture_region(
            queue,
            texture,
            mip_level,
            [0, 0, 0],
            bytes,
            bytes_per_row,
            rows_per_image,
            extent,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_texture_region(
        &self,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        mip_level: u32,
        origin: [u32; 3],
        bytes: &[u8],
        bytes_per_row: u32,
        rows_per_image: u32,
        extent: [u32; 3],
    ) {
        let origin = wgpu::Origin3d {
            x: origin[0],
            y: origin[1],
            z: origin[2],
        };
        let extent = wgpu::Extent3d {
            width: extent[0],
            height: extent[1],
            depth_or_array_layers: extent[2],
        };
        if let Some(uploads) = &self.uploads {
            let texel = texture.format().block_copy_size(None).unwrap_or(4);
            uploads.write_texture(
                texture,
                mip_level,
                origin,
                extent,
                bytes,
                bytes_per_row,
                rows_per_image,
                extent.width * texel,
            );
        } else {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(rows_per_image),
                },
                extent,
            );
        }
        self.count_upload(bytes.len());
    }

    fn count_upload(&self, bytes: usize) {
        self.work.borrow_mut().record_upload(bytes);
        if let Some(policy) = &self.policy {
            policy.record_upload(bytes as u64);
        }
    }

    /// Count bytes a renderer uploaded without going through this sink.
    pub fn record_upload(&self, bytes: usize) {
        self.count_upload(bytes);
    }

    pub fn record_realloc(&self) {
        self.work.borrow_mut().record_realloc();
        if let Some(policy) = &self.policy {
            policy.record_reallocation();
        }
    }

    pub fn record_batch_rebuild(&self) {
        self.work.borrow_mut().record_batch_rebuild();
    }

    pub fn record_draw_batch(&self) {
        self.work.borrow_mut().record_draw_batch();
    }

    pub fn record_draw_call(&self) {
        self.work.borrow_mut().record_draw_call();
    }

    pub fn snapshot(&self) -> GpuWorkObservation {
        *self.work.borrow()
    }
}

/// Stage timings a GPU host measured while encoding/submitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GpuStageTimings {
    pub batch: Duration,
    pub gpu_upload: Duration,
    pub encode: Duration,
    pub submit: Duration,
}

impl GpuStageTimings {
    /// Fold host-measured GPU stages onto a FrameProfiler.
    ///
    /// Runtime-only hosts must call [`FrameProfiler::mark_runtime_unsupported`]
    /// instead and never invent these durations.
    pub fn record_on(self, profiler: &mut FrameProfiler) {
        profiler.record(FrameStage::Batch, self.batch);
        profiler.record(FrameStage::GpuUpload, self.gpu_upload);
        profiler.record(FrameStage::Encode, self.encode);
        profiler.record(FrameStage::Submit, self.submit);
    }
}
