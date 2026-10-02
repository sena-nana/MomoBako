use nana_ui::{FrameDemand, RuntimeProgramContext};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nana_window::{MaterialEffect, MaterialFallback, MaterialOutcome};

pub struct StartupProbe {
    started_at: Instant,
    measure_first_frame: bool,
    recorded: bool,
    measure_seconds: Option<f64>,
    output: Option<PathBuf>,
    first_present: Option<Instant>,
    sample_started: Option<Instant>,
    previous_present: Option<Instant>,
    intervals: Vec<f64>,
    destroy_device: bool,
    destroyed_generation: Option<u64>,
    recovered_generation: Option<u64>,
    presented: usize,
}

impl StartupProbe {
    pub fn new(started_at: Instant) -> Self {
        let args = std::env::args().collect::<Vec<_>>();
        let value = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .map(|index| args.get(index + 1).expect("probe option needs a value"))
        };
        let measure_seconds = value("--measure-present-seconds").map(|value| {
            let seconds: f64 = value.parse().expect("invalid present sample duration");
            assert!(seconds.is_finite() && seconds > 0.0);
            seconds
        });
        Self {
            started_at,
            measure_first_frame: std::env::args_os()
                .any(|argument| argument == "--measure-first-frame"),
            recorded: false,
            measure_seconds,
            output: value("--performance-output").map(PathBuf::from),
            first_present: None,
            sample_started: None,
            previous_present: None,
            intervals: Vec::new(),
            destroy_device: args.iter().any(|arg| arg == "--probe-device-loss"),
            destroyed_generation: None,
            recovered_generation: None,
            presented: 0,
        }
    }

    pub fn demand(&self) -> FrameDemand {
        if self.measure_seconds.is_some() || self.destroy_device {
            FrameDemand::Continuous(std::num::NonZeroU32::new(120).unwrap())
        } else {
            FrameDemand::OnDemand
        }
    }

    pub fn continuous_preview(&self) -> bool {
        self.measure_seconds.is_some() || self.destroy_device
    }

    pub fn record_frame<Message: Send + 'static>(
        &mut self,
        context: &RuntimeProgramContext<Message>,
        exchange: Option<nana_ui::FrameExchangeStats>,
    ) -> bool {
        if self.record_first_frame(context.material()) {
            return true;
        }
        self.presented += 1;
        let now = Instant::now();
        let first = *self.first_present.get_or_insert(now);
        let generation = context.gpu().generation().get();
        if self.destroy_device && self.destroyed_generation.is_none() && self.presented >= 3 {
            self.destroyed_generation = Some(generation);
            context.gpu().wgpu().device().destroy();
            return false;
        }
        if self
            .destroyed_generation
            .is_some_and(|old| generation != old)
        {
            self.recovered_generation = Some(generation);
        }
        if let Some(seconds) = self.measure_seconds {
            if now.duration_since(first) < Duration::from_secs(2) {
                return false;
            }
            let started = *self.sample_started.get_or_insert(now);
            if let Some(previous) = self.previous_present.replace(now) {
                self.intervals
                    .push(now.duration_since(previous).as_secs_f64() * 1000.0);
            }
            if now.duration_since(started).as_secs_f64() < seconds {
                return false;
            }
        } else if !self.destroy_device || self.recovered_generation.is_none() {
            return false;
        }
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f64::total_cmp);
        let percentile = |q: f64| {
            sorted
                .get(((sorted.len().saturating_sub(1)) as f64 * q).round() as usize)
                .copied()
        };
        let sample_seconds = self
            .sample_started
            .map(|start| now.duration_since(start).as_secs_f64())
            .unwrap_or(0.0);
        let mean = (!sorted.is_empty()).then(|| sorted.iter().sum::<f64>() / sorted.len() as f64);
        let p50 = percentile(0.5);
        let p95 = percentile(0.95);
        let p99 = percentile(0.99);
        let budget = 1000.0 / 120.0;
        let overdue = sorted.iter().filter(|interval| **interval > budget).count();
        let ratio = (!sorted.is_empty()).then(|| overdue as f64 / sorted.len() as f64);
        let cadence_hz = if sample_seconds > 0.0 {
            sorted.len() as f64 / sample_seconds
        } else {
            0.0
        };
        let mean_v = mean.unwrap_or(0.0);
        let p50_v = p50.unwrap_or(0.0);
        let p95_v = p95.unwrap_or(0.0);
        // NanaLive DisplayClock SMALL_LATE_TOLERANCE is 2ms; p95 vs budget, not
        // a count of samples 0.01ms over the period. Faster cadence is not a miss.
        let interval_gate_passed = p95_v > 0.0
            && p95_v <= budget + 2.0
            && mean_v > 0.0
            && mean_v <= budget * 1.02
            && cadence_hz >= 120.0 * 0.98
            && p50_v > 0.0
            && p50_v < budget * 1.5;
        let report = serde_json::json!({
            "kind": "hosted-gpu-demo-present-probe", "requested_hz": 120,
            "adapter": context.gpu().capabilities().adapter_name(),
            "backend": format!("{:?}", context.gpu().capabilities().backend()),
            "physical_size": [context.geometry().physical_size.0, context.geometry().physical_size.1],
            "surface_alpha": format!("{:?}", context.surface_alpha_mode()),
            "sample_seconds": sample_seconds,
            "interval_samples": sorted.len(),
            "interval_ms": {
                "mean": mean,
                "p50": p50,
                "p95": p95,
                "p99": p99,
                "max": sorted.last(),
            },
            "over_8_33ms_ratio": ratio,
            "interval_gate_passed": interval_gate_passed,
            "destroyed_generation": self.destroyed_generation,
            "recovered_generation": self.recovered_generation,
            "frame_exchange": exchange.map(|stats| serde_json::json!({
                "submitted": stats.submitted,
                "published": stats.published,
                "superseded": stats.superseded,
                "pool_full": stats.pool_full,
                "stale_epoch": stats.stale_epoch,
                "occupied": stats.occupied,
                "occupied_high_water": stats.occupied_high_water,
            })),
            "note": "Surface present callback intervals; no display scanout feedback, no pixel readback. Device probe explicitly destroys only this application's Device. Gate: P95 <= 8.33+2ms (NanaLive small-late), mean not slower than period+2%, cadence >= 98% of 120Hz, P50 not a 60Hz lock."
        });
        let json = serde_json::to_string_pretty(&report).unwrap();
        if let Some(path) = &self.output {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, &json).unwrap();
        }
        println!("{json}");
        true
    }

    pub fn record_first_frame(&mut self, material: MaterialOutcome) -> bool {
        if !self.measure_first_frame || self.recorded {
            return false;
        }
        self.recorded = true;
        let elapsed_ms = self.started_at.elapsed().as_secs_f64() * 1_000.0;
        match material.fallback {
            Some(fallback) => println!(
                "{{\"first_frame_ms\":{elapsed_ms:.3},\"material\":\"{}\",\"fallback\":\"{}\"}}",
                material_name(material.effect),
                fallback_name(fallback),
            ),
            None => println!(
                "{{\"first_frame_ms\":{elapsed_ms:.3},\"material\":\"{}\"}}",
                material_name(material.effect),
            ),
        }
        true
    }
}

const fn material_name(material: MaterialEffect) -> &'static str {
    match material {
        MaterialEffect::Solid => "solid",
        MaterialEffect::Transparent => "transparent",
        MaterialEffect::Vibrancy => "vibrancy",
        MaterialEffect::Mica => "mica",
        MaterialEffect::Acrylic => "acrylic",
    }
}

const fn fallback_name(fallback: MaterialFallback) -> &'static str {
    match fallback {
        MaterialFallback::NativeMaterialUnavailable => "native_unavailable",
        MaterialFallback::PlatformDoesNotProvideNativeMaterial => "unsupported",
    }
}
