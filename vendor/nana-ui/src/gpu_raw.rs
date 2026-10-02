//! The framework's own access to the WGPU objects behind the GPU contract.
//!
//! Crate-private on purpose: consumers reach these objects only through the
//! `wgpu-interop` escape hatch.

use nana_gpu::{__framework, GpuContext};

pub(crate) trait GpuRaw {
    fn raw_device(&self) -> &wgpu::Device;
    fn raw_queue(&self) -> &wgpu::Queue;
}

impl GpuRaw for GpuContext {
    fn raw_device(&self) -> &wgpu::Device {
        __framework::device(self)
    }

    fn raw_queue(&self) -> &wgpu::Queue {
        __framework::queue(self)
    }
}
