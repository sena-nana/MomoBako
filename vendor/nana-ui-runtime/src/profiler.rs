//! Independent per-stage frame timing for the Performance Contract (Issue #8 §4).
//!
//! Hosts can time Style / Text Shape / Layout / Hit Test / Accessibility /
//! Extract even when those calls currently sit inside one frame loop. Stages
//! the Runtime does not own report `unsupported` with zero duration.

use std::time::{Duration, Instant};

use nana_ui_core::FrameStage;

/// Whether a named stage ran, was skipped this frame, or is not implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Ran,
    Skipped,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageTiming {
    pub stage: FrameStage,
    pub duration: Duration,
    pub status: StageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameProfile {
    pub stages: Vec<StageTiming>,
    pub cpu_total: Duration,
}

impl Default for FrameProfile {
    fn default() -> Self {
        let mut profiler = FrameProfiler::new();
        profiler.mark_runtime_unsupported();
        profiler.finish()
    }
}

impl FrameProfile {
    pub fn stage(&self, stage: FrameStage) -> Option<StageTiming> {
        self.stages
            .iter()
            .copied()
            .find(|timing| timing.stage == stage)
    }

    pub fn any_stage_ran(&self) -> bool {
        self.stages
            .iter()
            .any(|timing| timing.status == StageStatus::Ran)
    }
}

/// Times named frame stages independently of how the host groups system calls.
pub struct FrameProfiler {
    timings: [StageTiming; 13],
}

impl Default for FrameProfiler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameProfiler {
    /// Every stage starts as skipped with zero duration. Call
    /// [`Self::mark_runtime_unsupported`] for GPU/submit stages the Runtime
    /// cannot measure.
    pub fn new() -> Self {
        Self {
            timings: FrameStage::ALL.map(|stage| StageTiming {
                stage,
                duration: Duration::ZERO,
                status: StageStatus::Skipped,
            }),
        }
    }

    /// Mark Batch / GPU Upload / Encode / Submit unsupported. Other stages stay
    /// skipped until timed.
    pub fn mark_runtime_unsupported(&mut self) {
        for stage in FrameStage::ALL {
            if stage.runtime_unsupported() {
                self.unsupported(stage);
            }
        }
    }

    pub fn time<R>(&mut self, stage: FrameStage, work: impl FnOnce() -> R) -> R {
        let started = Instant::now();
        let result = work();
        self.record(stage, started.elapsed());
        result
    }

    /// Mark a stage as ran and add `duration`. Multi-pass frames accumulate.
    pub fn record(&mut self, stage: FrameStage, duration: Duration) {
        let index = Self::index(stage);
        let current = &mut self.timings[index];
        current.duration = current.duration.saturating_add(duration);
        current.status = StageStatus::Ran;
    }

    pub fn skip(&mut self, stage: FrameStage) {
        self.set(
            stage,
            StageTiming {
                stage,
                duration: Duration::ZERO,
                status: StageStatus::Skipped,
            },
        );
    }

    pub fn unsupported(&mut self, stage: FrameStage) {
        self.set(
            stage,
            StageTiming {
                stage,
                duration: Duration::ZERO,
                status: StageStatus::Unsupported,
            },
        );
    }

    pub fn finish(self) -> FrameProfile {
        let cpu_total = self
            .timings
            .iter()
            .filter(|timing| timing.status == StageStatus::Ran)
            .map(|timing| timing.duration)
            .fold(Duration::ZERO, |acc, duration| acc.saturating_add(duration));
        FrameProfile {
            stages: self.timings.to_vec(),
            cpu_total,
        }
    }

    fn set(&mut self, stage: FrameStage, timing: StageTiming) {
        self.timings[Self::index(stage)] = timing;
    }

    fn index(stage: FrameStage) -> usize {
        FrameStage::ALL
            .iter()
            .position(|candidate| *candidate == stage)
            .expect("FrameStage::ALL lists every stage")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiler_times_owned_stages_and_leaves_gpu_stages_unsupported() {
        let mut profiler = FrameProfiler::new();
        profiler.mark_runtime_unsupported();
        profiler.time(FrameStage::Style, || 1 + 1);
        profiler.skip(FrameStage::Layout);

        let profile = profiler.finish();
        let style = profile.stage(FrameStage::Style).unwrap();
        assert_eq!(style.status, StageStatus::Ran);
        let layout = profile.stage(FrameStage::Layout).unwrap();
        assert_eq!(layout.status, StageStatus::Skipped);
        assert_eq!(layout.duration, Duration::ZERO);
        let gpu = profile.stage(FrameStage::GpuUpload).unwrap();
        assert_eq!(gpu.status, StageStatus::Unsupported);
        assert_eq!(gpu.duration, Duration::ZERO);
        assert_eq!(
            profile.stage(FrameStage::Submit).unwrap().status,
            StageStatus::Unsupported
        );
        assert_eq!(profile.cpu_total, style.duration);
    }

    #[test]
    fn unused_profiler_finish_has_no_ran_stages() {
        let mut profiler = FrameProfiler::new();
        profiler.mark_runtime_unsupported();
        let profile = profiler.finish();
        assert!(!profile.any_stage_ran());
        assert_eq!(profile.cpu_total, Duration::ZERO);
    }
}
