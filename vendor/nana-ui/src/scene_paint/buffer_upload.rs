//! Upload changed aligned blocks while retaining the existing GPU allocation.
//! Callers pass an empty previous slice after replacing the GPU buffer.
use std::ops::Range;

/// Granularity of the comparison: a change anywhere in a block sends the block.
const BLOCK: usize = 64;

/// Unchanged bytes one write re-sends rather than split into two writes: a
/// `queue.write_buffer` costs about 32 000 instructions on Metal, a byte about
/// one, so a gap this size is still cheaper than another call.
const MERGE_GAP: usize = 16 * 1024;

pub(super) fn upload_changed_with_work(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    previous: &[u8],
    next: &[u8],
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> usize {
    debug_assert_eq!(next.len() % wgpu::COPY_BUFFER_ALIGNMENT as usize, 0);
    let mut uploaded = 0;
    changed_ranges(previous, next, |range| {
        if let Some(work) = work {
            work.write_buffer(queue, buffer, range.start as u64, &next[range.clone()]);
        } else {
            queue.write_buffer(buffer, range.start as u64, &next[range.clone()]);
        }
        uploaded += range.len();
    });
    uploaded
}

/// The ranges of `next` to write so the buffer holding `previous` holds
/// `next`: changed blocks, joined across gaps of at most [`MERGE_GAP`].
fn changed_ranges(previous: &[u8], next: &[u8], mut write: impl FnMut(Range<usize>)) {
    let mut run: Option<Range<usize>> = None;
    for offset in (0..next.len()).step_by(BLOCK) {
        let end = (offset + BLOCK).min(next.len());
        if previous.get(offset..end) == Some(&next[offset..end]) {
            continue;
        }
        match &mut run {
            Some(run) if offset - run.end <= MERGE_GAP => run.end = end,
            _ => {
                if let Some(done) = run.replace(offset..end) {
                    write(done);
                }
            }
        }
    }
    if let Some(done) = run {
        write(done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(previous: &[u8], next: &[u8]) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        changed_ranges(previous, next, |range| out.push(range));
        out
    }

    #[test]
    fn unchanged_bytes_write_nothing() {
        let bytes = vec![7u8; 4096];
        assert!(ranges(&bytes, &bytes).is_empty());
    }

    #[test]
    fn nearby_changes_share_one_write() {
        let previous = vec![0u8; 8192];
        let mut next = previous.clone();
        next[10] = 1;
        next[700] = 1;
        next[4100] = 1;
        assert_eq!(ranges(&previous, &next), vec![0..4160]);
    }

    #[test]
    fn changes_farther_apart_than_the_gap_stay_separate() {
        let previous = vec![0u8; 4 * MERGE_GAP];
        let mut next = previous.clone();
        next[0] = 1;
        // Gap of exactly MERGE_GAP after the first block: still joined.
        next[BLOCK + MERGE_GAP] = 1;
        // One block past the gap: a write of its own.
        let far = 2 * BLOCK + 2 * MERGE_GAP + BLOCK;
        next[far] = 1;
        assert_eq!(
            ranges(&previous, &next),
            vec![0..2 * BLOCK + MERGE_GAP, far..far + BLOCK]
        );
    }
}
