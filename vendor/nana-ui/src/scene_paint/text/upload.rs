//! Glyph atlas uploads: one region per newly placed or relocated glyph.
//!
//! Never the whole atlas. A frame that adds one glyph to a full page writes
//! that glyph's rectangle and nothing else, which is the difference between a
//! few hundred bytes and a megabyte of texture traffic per frame.
//!
//! In-flight safety is `wgpu::Queue::write_texture`'s: it copies the bytes
//! into queue-owned staging at call time and orders the transfer ahead of the
//! next submission, so nothing here can overwrite a region the GPU is still
//! reading. That is the reason this queue stages through the `Queue` rather
//! than keeping a hand-rolled ring — a ring would have to track fences that
//! the painter, which owns neither the submit nor the surface, cannot see.

use std::sync::Arc;

use super::atlas::{AtlasPageKind, GlyphAtlasManager};
use super::raster::GlyphImage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GlyphUploadCounters {
    pub regions: u64,
    pub bytes: u64,
}

struct PendingUpload {
    kind: AtlasPageKind,
    page: u32,
    /// Destination of the *padded* cell, gutter included.
    origin: [u32; 2],
    cell: [u32; 2],
    image: Arc<GlyphImage>,
    padding: u32,
}

#[derive(Default)]
pub(super) struct GlyphUploadQueue {
    pending: Vec<PendingUpload>,
    /// Scratch for the padded cell, reused across regions and frames so a
    /// frame that faults in a page of CJK does not allocate per glyph.
    scratch: Vec<u8>,
    counters: GlyphUploadCounters,
}

impl GlyphUploadQueue {
    pub(super) fn counters(&self) -> GlyphUploadCounters {
        self.counters
    }

    /// Queue one glyph's cell. Holding the `Arc` is what lets the raster cache
    /// evict the entry that produced it before this ever reaches the GPU.
    pub(super) fn push(
        &mut self,
        kind: AtlasPageKind,
        page: u32,
        origin: [u32; 2],
        cell: [u32; 2],
        image: Arc<GlyphImage>,
        padding: u32,
    ) {
        self.pending.push(PendingUpload {
            kind,
            page,
            origin,
            cell,
            image,
            padding,
        });
    }

    /// Write every queued region. Called before the pass that samples them.
    pub(super) fn flush(
        &mut self,
        queue: &wgpu::Queue,
        atlas: &GlyphAtlasManager,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) {
        if self.pending.is_empty() {
            return;
        }
        for upload in self.pending.drain(..) {
            let Some(texture) = atlas.page_texture(upload.page) else {
                continue;
            };
            debug_assert_eq!(
                upload.kind.bytes_per_texel(),
                upload.image.format.bytes_per_pixel(),
                "a region's page kind and its bitmap's format must agree, or \
                 the row stride below reads the wrong number of bytes"
            );
            let texel = upload.kind.bytes_per_texel();
            let row = upload.cell[0] as usize * texel;
            let total = row * upload.cell[1] as usize;
            self.scratch.clear();
            self.scratch.resize(total, 0);
            let pad = upload.padding as usize;
            let glyph_row = upload.image.width as usize * texel;
            for y in 0..upload.image.height as usize {
                let src = y * glyph_row;
                let dest = (y + pad) * row + pad * texel;
                self.scratch[dest..dest + glyph_row]
                    .copy_from_slice(&upload.image.data[src..src + glyph_row]);
            }
            if let Some(work) = work {
                work.write_texture(
                    queue,
                    texture,
                    [upload.origin[0], upload.origin[1], 0],
                    &self.scratch,
                    row as u32,
                    upload.cell[1],
                    [upload.cell[0], upload.cell[1], 1],
                );
            } else {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: upload.origin[0],
                            y: upload.origin[1],
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &self.scratch,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row as u32),
                        rows_per_image: Some(upload.cell[1]),
                    },
                    wgpu::Extent3d {
                        width: upload.cell[0],
                        height: upload.cell[1],
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.counters.regions += 1;
            self.counters.bytes += total as u64;
        }
    }
}
