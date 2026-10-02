//! Opt-in diagnostics only: reads timestamp queries, never target pixels.
use std::time::Duration;

pub(super) struct TimestampProbe {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    mapped: wgpu::Buffer,
}

impl TimestampProbe {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("nana.benchmark.timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 3,
            }),
            resolved: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.benchmark.resolve"),
                size: 24,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            mapped: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nana.benchmark.readback"),
                size: 24,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
        }
    }
    pub(super) fn stamp(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        encoder.write_timestamp(&self.queries, index);
    }
    pub(super) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.queries, 0..3, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.mapped, 0, 24);
    }
    pub(super) fn read(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> [Duration; 2] {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.mapped
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("timestamp completion");
        rx.recv()
            .expect("timestamp callback")
            .expect("timestamp mapping");
        let mapped = self
            .mapped
            .slice(..)
            .get_mapped_range()
            .expect("timestamp mapped range");
        let words = mapped.as_chunks::<8>().0;
        let ticks = [0, 1, 2].map(|index| u64::from_le_bytes(words[index]));
        let period = queue.get_timestamp_period() as f64;
        let durations = [0, 1].map(|i| {
            assert!(ticks[i + 1] >= ticks[i], "GPU timestamps must be ordered");
            Duration::from_secs_f64((ticks[i + 1] - ticks[i]) as f64 * period / 1_000_000_000.0)
        });
        drop(mapped);
        self.mapped.unmap();
        durations
    }
}
