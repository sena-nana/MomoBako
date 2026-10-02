//! Headless GPU scene bench for Issue #8 `gpu-scene-*`.
//!
//! Loads `perf/scenarios/gpu-scene-*.json`. UiOnly materializes that file's
//! viewport, host-texture slot, and UI nodes, then paints through
//! `SceneWgpuPainter`. No pixel readback. Optional GPU timestamp diagnostics
//! read query results after GPU completion. Missing adapter or Live2D exit 2.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nana_gpu::__framework;
use nana_ui::runtime::{
    Button, DocumentId, Entity, FlexDirection, FlexWrap, FrameProfile, FrameProfiler,
    GpuTextureView, GpuView, GpuViewPalette, GpuWorkObservation, HOST_TEXTURE_RENDERER, IconGlyph,
    LayoutStyle, LayoutViewport, LengthSpec, List, NodeStyle, RuntimeDocument, SemanticColorRole,
    StageStatus, Text,
};
use nana_ui::{
    ButtonKind, GpuContext, GpuStageTimings, HostTexture, HostTextureAlphaMode,
    HostTextureRegistry, Icon, NanaTextShaper, SceneGpuRendererRegistry, ScenePaintViewport,
    SceneWgpuPainter, default_scene_gpu_renderers,
};
use nana_ui_core::{PaintTransform, TransformOrigin};
use nana_ui_scene::ScenePrimitiveKind;
use serde::{Deserialize, Serialize};

#[path = "gpu_scene_benchmark/allocations.rs"]
mod allocations;
#[path = "gpu_scene_benchmark/timestamps.rs"]
mod timestamps;

#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const WARMUP: usize = 3;
const FRAMES: usize = 20;
/// Logical edge of one `gpu-view` node. Small on purpose: the scale scenarios
/// pack many of them into one viewport.
const GPU_VIEW_EXTENT: u32 = 24;

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsupported_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario_id: Option<String>,
    composition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    materialization: Option<Materialization>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_graph: Option<FrameGraphReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frames: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu_work: Option<GpuWorkSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu_policy: Option<GpuPolicySnapshot>,
    /// Per sampled frame, so a gate reads "what one frame redid" rather than
    /// a running total. Only emitted when the scenario keeps the batch moving.
    #[serde(skip_serializing_if = "Option::is_none")]
    text_counters: Option<BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_stages: Option<BTreeMap<String, StageStatusReport>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stages: Option<StageReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sampling: Option<SamplingReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gpu_timestamps: Option<TimestampReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    framework_thread_allocations: Option<allocations::Report>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct HostTextureParams {
    slot: String,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct Materialization {
    viewport: [u32; 2],
    host_texture: HostTextureParams,
    ui_nodes: Vec<String>,
    node_repeat: BTreeMap<String, usize>,
    shared_gpu_view_slot: bool,
    /// Echoed so a runner cannot quietly measure a frame the painter answered
    /// from its prepared batch and call it a retained-text gate.
    text_ticker: bool,
    /// Echoed for the same reason: a paint-only or compositor-only gate that
    /// ran a still scene would pass by construction.
    #[serde(skip_serializing_if = "Option::is_none")]
    text_animation: Option<TextAnimation>,
    ui_entity_count: usize,
    host_texture_resources: usize,
    scene_primitive_kinds: Vec<String>,
}

#[derive(Serialize, Clone, Copy)]
struct GpuWorkSnapshot {
    batch_rebuilds: usize,
    draw_batches: usize,
    draw_calls: usize,
    gpu_upload_bytes: usize,
    gpu_buffer_reallocations: usize,
}

#[derive(Serialize, Clone, Copy)]
struct GpuPolicySnapshot {
    upload_bytes: u64,
    buffer_reallocations: u64,
    transient_pool_hits: u64,
    transient_pool_misses: u64,
    pipeline_registry_hits: u64,
    pipeline_registry_misses: u64,
    frame_slot_stalls: u64,
    frame_slot_waits: u64,
    retired_resources: u64,
    upload_writes: u64,
    upload_copies: u64,
    upload_flushes: u64,
    upload_ring_allocations: u64,
    upload_ring_waits: u64,
}

impl From<nana_gpu::GpuPolicyStats> for GpuPolicySnapshot {
    fn from(stats: nana_gpu::GpuPolicyStats) -> Self {
        Self {
            upload_bytes: stats.upload_bytes,
            buffer_reallocations: stats.buffer_reallocations,
            transient_pool_hits: stats.transient_pool_hits,
            transient_pool_misses: stats.transient_pool_misses,
            pipeline_registry_hits: stats.pipeline_registry_hits,
            pipeline_registry_misses: stats.pipeline_registry_misses,
            frame_slot_stalls: stats.frame_slot_stalls,
            frame_slot_waits: stats.frame_slot_waits,
            retired_resources: stats.retired_resources,
            upload_writes: stats.upload_writes,
            upload_copies: stats.upload_copies,
            upload_flushes: stats.upload_flushes,
            upload_ring_allocations: stats.upload_ring_allocations,
            upload_ring_waits: stats.upload_ring_waits,
        }
    }
}

impl From<GpuWorkObservation> for GpuWorkSnapshot {
    fn from(observed: GpuWorkObservation) -> Self {
        Self {
            batch_rebuilds: observed.batch_rebuilds,
            draw_batches: observed.draw_batches,
            draw_calls: observed.draw_calls,
            gpu_upload_bytes: observed.gpu_upload_bytes,
            gpu_buffer_reallocations: observed.gpu_buffer_reallocations,
        }
    }
}

#[derive(Serialize)]
struct StageStatusReport {
    status: &'static str,
}

#[derive(Serialize)]
struct StageReport {
    batch_ms: Distribution,
    gpu_upload_ms: Distribution,
    encode_ms: Distribution,
    submit_ms: Distribution,
}

/// Structural cost of the render graph for this scene. `frame_plan()` memoizes
/// per structure, so this is what one add/remove of a custom node pays, not a
/// per-frame cost.
#[derive(Serialize, Clone, Copy)]
struct FrameGraphReport {
    build_ms: f64,
    pass_count: usize,
    resource_count: usize,
}

#[derive(Serialize)]
struct SamplingReport {
    elapsed_seconds: f64,
    warmup_seconds: f64,
    mode: &'static str,
    surface_present_measured: bool,
    framework_cpu_prepare_ms: Distribution,
    runtime_passes: usize,
    structure_plan_rebuilds: usize,
    maximum_gpu_work_per_frame: GpuWorkSnapshot,
}

#[derive(Serialize)]
struct TimestampReport {
    producer_ms: Distribution,
    ui_composition_ms: Distribution,
}

#[derive(Serialize)]
struct Distribution {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

#[derive(Deserialize)]
struct ScenarioFile {
    id: String,
    kind: String,
    params: ScenarioParams,
}

#[derive(Deserialize)]
struct ScenarioParams {
    composition: String,
    #[serde(default)]
    independent_textures: bool,
    /// Per-kind child count. Scale rides here so `ui_nodes` stays readable and
    /// the runner can echo-compare both.
    #[serde(default)]
    node_repeat: BTreeMap<String, usize>,
    /// `false` gives every `gpu-view` node its own `slot_id`, so the scene
    /// carries N distinct `CustomRenderNode::resource` strings and the render
    /// graph builds N external resources. `true` shares one slot.
    #[serde(default)]
    shared_gpu_view_slot: bool,
    viewport: [u32; 2],
    host_texture: HostTextureParams,
    ui_nodes: Vec<String>,
    /// Change one label's text every frame. Without it the painter answers
    /// from its prepared batch and the frame measures nothing at all — which
    /// is right for a draw-call scale row and useless for a text row, because
    /// what a shell actually pays for is the frame *beside* an animation.
    #[serde(default)]
    text_ticker: bool,
    /// Animate something about the text other than what it says, every
    /// frame: its color, or its container's opacity or transform, or a
    /// fraction of a pixel's slide. These are the #98 paint-only and
    /// compositor-only gates and #223's sub-pixel one — the text never changes,
    /// so every frame must be answered without shaping, laying out,
    /// rasterizing or resolving a glyph. `resize` is the #99 constraint-only
    /// gate: the labels' width changes, so every one is laid out again, from
    /// the runs it already shaped.
    #[serde(default)]
    text_animation: Option<TextAnimation>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
enum TextAnimation {
    /// Every label alternates between two foreground roles.
    Color,
    /// The list holding the labels fades.
    Opacity,
    /// The list holding the labels turns.
    Transform,
    /// The list holding the labels slides sideways by 0.37 px a frame: a
    /// trackpad's horizontal scroll, as the painter sees it (#223).
    Slide,
    /// Every label alternates between two widths it wraps inside.
    Resize,
}

impl ScenarioParams {
    fn texture_slot(&self, index: usize) -> String {
        if self.independent_textures {
            format!("{}:{index}", self.host_texture.slot)
        } else {
            self.host_texture.slot.clone()
        }
    }

    fn repeat(&self, kind: &str) -> usize {
        self.node_repeat.get(kind).copied().unwrap_or(1).max(1)
    }

    fn node_count(&self, kind: &str) -> usize {
        self.ui_nodes
            .iter()
            .filter(|node| node.as_str() == kind)
            .map(|_| self.repeat(kind))
            .sum()
    }
}

fn main() {
    let args = Args::parse();
    let report = match load_scenario(args.scenario.as_ref()) {
        Ok(scenario) => run_scenario(scenario, &args),
        Err(error) => unsupported(error.scenario_id, &error.composition, error.reason),
    };
    write_report(&args.output, &report);
    if report.status != "ok" {
        std::process::exit(2);
    }
}

struct LoadError {
    scenario_id: Option<String>,
    composition: String,
    reason: String,
}

fn load_err(scenario_id: Option<String>, composition: &str, reason: String) -> LoadError {
    LoadError {
        scenario_id,
        composition: composition.to_string(),
        reason,
    }
}

struct Args {
    output: Option<PathBuf>,
    scenario: Option<PathBuf>,
    sample_seconds: Option<f64>,
    gpu_timestamps: bool,
    allocation_counts: bool,
}

impl Args {
    fn parse() -> Self {
        let mut output = None;
        let mut scenario = None;
        let mut sample_seconds = None;
        let mut gpu_timestamps = false;
        let mut allocation_counts = false;
        let mut argv = std::env::args().skip(1);
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--output" => output = argv.next().map(PathBuf::from),
                "--scenario" => scenario = argv.next().map(PathBuf::from),
                "--sample-seconds" => {
                    let seconds: f64 = argv
                        .next()
                        .expect("--sample-seconds needs a value")
                        .parse()
                        .expect("invalid sample duration");
                    assert!(
                        seconds.is_finite() && seconds > 0.0,
                        "sample duration must be finite and positive"
                    );
                    sample_seconds = Some(seconds);
                }
                "--gpu-timestamps" => gpu_timestamps = true,
                "--allocation-counts" => allocation_counts = true,
                _ => {}
            }
        }
        Self {
            output,
            scenario,
            sample_seconds,
            gpu_timestamps,
            allocation_counts,
        }
    }
}

fn load_scenario(path: Option<&PathBuf>) -> Result<ScenarioFile, LoadError> {
    let Some(path) = path else {
        return Err(load_err(
            None,
            "unknown",
            "gpu-scene-* must load perf/scenarios/gpu-scene-*.json via --scenario. \
Do not invent a private hosted-gpu-demo tree."
                .into(),
        ));
    };
    let text = fs::read_to_string(path).map_err(|error| {
        load_err(
            None,
            "unknown",
            format!("failed to read scenario JSON {}: {error}", path.display()),
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        load_err(
            None,
            "unknown",
            format!("invalid scenario JSON {}: {error}", path.display()),
        )
    })?;
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let composition = value
        .pointer("/params/composition")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    if composition != "UiOnly" {
        return Err(load_err(
            Some(id),
            &composition,
            live2d_reason(&composition),
        ));
    }
    serde_json::from_value(value).map_err(|error| {
        load_err(
            Some(id),
            &composition,
            format!(
                "gpu-scene-ui UiOnly requires viewport, host_texture, and ui_nodes from the scenario JSON: {error}"
            ),
        )
    })
}

fn live2d_reason(composition: &str) -> String {
    format!(
        "GpuScene composition {composition} needs a real Live2D Scene pass. \
HostTexture evidence from a UiOnly encode is not Live2D. Required by #8 / not implemented."
    )
}

/// What one sampled frame asked the text path to redo.
///
/// Deltas, not totals: a gate that read a running total would pass or fail on
/// how long the bench ran.
fn text_counters_per_frame(
    warm: nana_ui::TextGlyphCounters,
    end: nana_ui::TextGlyphCounters,
    shaping: TextShapingWork,
    frames: usize,
) -> BTreeMap<String, f64> {
    let per_frame = frames as f64;
    let mut out = BTreeMap::new();
    // The #98 gates name these "shape runs created" and "layouts created".
    // Both sides are counted: the Runtime measures text, and the painter lays
    // out what arrives without a Runtime layout it can draw from.
    out.insert(
        "text_nodes_shaped".to_string(),
        shaping.nodes_shaped as f64 / per_frame,
    );
    out.insert(
        "text_layouts_created".to_string(),
        shaping.layouts_created as f64 / per_frame,
    );
    out.insert(
        "paint_shape_cache_misses".to_string(),
        shaping.paint_misses as f64 / per_frame,
    );
    // #99 constraint-only. A node asks the engine's layout cache only when it
    // needs a layout other than the one it holds, so the lookups are the nodes
    // a width change reached; a new layout either reused the shaped runs it
    // had (a width change) or had to shape again (new text or style).
    out.insert(
        "text_layout_lookups".to_string(),
        shaping.layout_lookups as f64 / per_frame,
    );
    out.insert(
        "text_constraint_only_relayouts".to_string(),
        shaping.constraint_only_relayouts as f64 / per_frame,
    );
    out.insert(
        "text_layouts_reshaped".to_string(),
        shaping
            .layouts_created
            .saturating_sub(shaping.constraint_only_relayouts) as f64
            / per_frame,
    );
    let mut delta = |name: &str, after: u64, before: u64| {
        out.insert(
            name.to_string(),
            after.saturating_sub(before) as f64 / per_frame,
        );
    };
    delta(
        "glyph_resolve_requests",
        end.glyph_resolve_requests,
        warm.glyph_resolve_requests,
    );
    delta(
        "glyph_rasterized",
        end.glyph_rasterized,
        warm.glyph_rasterized,
    );
    delta(
        "glyph_upload_bytes",
        end.glyph_upload_bytes,
        warm.glyph_upload_bytes,
    );
    delta(
        "text_instance_rebuilds",
        end.text_instance_rebuilds,
        warm.text_instance_rebuilds,
    );
    delta(
        "text_instance_upload_bytes",
        end.text_instance_upload_bytes,
        warm.text_instance_upload_bytes,
    );
    // #224: what reordering cost, apart from what the changed paragraph wrote.
    delta(
        "text_index_upload_bytes",
        end.text_index_upload_bytes,
        warm.text_index_upload_bytes,
    );
    delta(
        "text_prepare_nodes_considered",
        end.text_prepare_nodes_considered,
        warm.text_prepare_nodes_considered,
    );
    delta(
        "text_prepare_nodes_skipped",
        end.text_prepare_nodes_skipped,
        warm.text_prepare_nodes_skipped,
    );
    // Without this the three prepare counters do not add up and a reader is
    // left guessing whether the difference was rebuilt or never drawn.
    delta(
        "text_prepare_nodes_culled",
        end.text_prepare_nodes_culled,
        warm.text_prepare_nodes_culled,
    );
    out.insert(
        "text_gpu_entries_active".to_string(),
        end.text_gpu_entries_active as f64,
    );
    out
}

fn unsupported(scenario_id: Option<String>, composition: &str, reason: String) -> Report {
    Report {
        schema_version: 1,
        status: "unsupported",
        unsupported_reason: Some(reason),
        scenario_id,
        composition: composition.to_string(),
        materialization: None,
        frame_graph: None,
        adapter: None,
        frames: None,
        gpu_work: None,
        gpu_policy: None,
        text_counters: None,
        frame_stages: None,
        stages: None,
        sampling: None,
        gpu_timestamps: None,
        framework_thread_allocations: None,
    }
}

fn run_scenario(scenario: ScenarioFile, args: &Args) -> Report {
    if scenario.kind != "GpuScene" || scenario.params.composition != "UiOnly" {
        return unsupported(
            Some(scenario.id),
            &scenario.params.composition,
            live2d_reason(&scenario.params.composition),
        );
    }
    run_ui_only(scenario, args)
}

/// Shaping and layout the sampled frames asked for, Runtime and painter.
#[derive(Default, Clone, Copy)]
struct TextShapingWork {
    nodes_shaped: usize,
    layouts_created: usize,
    constraint_only_relayouts: usize,
    layout_lookups: usize,
    paint_misses: usize,
}

fn run_ui_only(scenario: ScenarioFile, args: &Args) -> Report {
    let params = &scenario.params;
    let Some((gpu, adapter)) = request_device(args.gpu_timestamps) else {
        return unsupported(
            Some(scenario.id),
            "UiOnly",
            "No WGPU adapter/device supporting the requested features (GPU timestamps need TIMESTAMP_QUERY and TIMESTAMP_QUERY_INSIDE_ENCODERS)."
                .into(),
        );
    };
    let device = __framework::device(&gpu).clone();
    let queue = __framework::queue(&gpu).clone();
    let slot = params.host_texture.slot.as_str();
    // Every gpu-texture-view child claims its own slot when textures are
    // independent, so this must follow node_repeat, not the ui_nodes length.
    let resource_count = if params.independent_textures {
        params.node_count("gpu-texture-view")
    } else {
        1
    };
    let textures = HostTextureRegistry::new();
    let previews = (0..resource_count)
        .map(|index| {
            let preview = HostSlotContent::new(
                &gpu,
                (params.host_texture.width, params.host_texture.height),
            );
            textures.register(
                params.texture_slot(index),
                preview.texture(),
                params.host_texture.width,
                params.host_texture.height,
                HostTextureAlphaMode::Premultiplied,
            );
            preview
        })
        .collect::<Vec<_>>();

    let (mut document, handles) = match ui_document(params) {
        Ok(built) => built,
        Err(reason) => return unsupported(Some(scenario.id), "UiOnly", reason),
    };
    let ticker = params
        .text_ticker
        .then(|| handles.texts.first().copied())
        .flatten();
    let mut shaper = NanaTextShaper::default();
    let viewport = LayoutViewport::new(params.viewport[0] as f32, params.viewport[1] as f32);
    document
        .flush(viewport, &mut shaper)
        .expect("gpu-scene-ui flush");

    let materialization = Materialization {
        viewport: params.viewport,
        host_texture: params.host_texture.clone(),
        ui_nodes: params.ui_nodes.clone(),
        node_repeat: params.node_repeat.clone(),
        shared_gpu_view_slot: params.shared_gpu_view_slot,
        text_ticker: params.text_ticker,
        text_animation: params.text_animation,
        host_texture_resources: resource_count,
        ui_entity_count: document
            .context()
            .world()
            .last_work_counters()
            .entities_total,
        scene_primitive_kinds: scene_primitive_kinds(document.scene(), slot),
    };
    let graph = measure_frame_graph(document.scene());

    // A `gpu-view` node fails scene validation without a registered "gpu-view"
    // renderer, and that rejects the whole frame. Measure the product reference
    // painter rather than a private one.
    let renderers: Option<SceneGpuRendererRegistry> =
        (params.node_count("gpu-view") > 0).then(default_scene_gpu_renderers);

    let mut painter = SceneWgpuPainter::new(&gpu, __framework::format_from_wgpu(FORMAT));
    let target = __framework::render_target(
        &gpu,
        color_target(&device, params.viewport[0], params.viewport[1]),
        FORMAT,
        params.viewport,
    );
    let paint_viewport = ScenePaintViewport {
        logical_size: [params.viewport[0] as f32, params.viewport[1] as f32],
        physical_size: params.viewport,
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.08, 0.08, 0.09, 1.0],
        clear: true,
    };

    let mut batch = Vec::with_capacity(FRAMES);
    let mut upload = Vec::with_capacity(FRAMES);
    let mut encode = Vec::with_capacity(FRAMES);
    let mut submit = Vec::with_capacity(FRAMES);
    let mut prepare = Vec::new();
    let mut producer_gpu = Vec::new();
    let mut ui_gpu = Vec::new();
    let mut queries = args
        .gpu_timestamps
        .then(|| timestamps::TimestampProbe::new(&device));
    let mut sampled_at = None;
    let mut warm_text = None;
    let mut shaping = TextShapingWork::default();
    let warmup_started = Instant::now();
    let warmup = Duration::from_secs(if args.sample_seconds.is_some() { 2 } else { 0 });
    let mut frame = 0usize;
    let mut runtime_passes = 0;
    let mut structure_plan_rebuilds = 0;
    let mut plan = document.scene().frame_plan().expect("valid frame plan");
    let mut maximum_gpu_work = GpuWorkObservation::default();
    let mut allocation_report = allocations::Report::default();
    let (work, last_stages) = loop {
        let sampling = frame >= WARMUP && warmup_started.elapsed() >= warmup;
        if sampling && sampled_at.is_none() {
            sampled_at = Some(Instant::now());
            warm_text = Some((
                painter.text_glyph_counters(),
                painter.text_shape_cache_stats().1,
            ));
        }
        if let Some(ticker) = ticker {
            // Fixed width. Going from `tick 9` to `tick 10` widens the label,
            // and every label after it in the row moves by a fraction of a
            // pixel — which is a real re-resolve (the glyphs' sub-pixel phase
            // changed), but it is the cost of a reflow, not of retention, and
            // whether a sample window happened to contain one would decide
            // whether the gate passed.
            document
                .context_mut()
                .set_component(ticker, Text::new(format!("tick {:04}", frame % 10_000)))
                .expect("ticker text");
        }
        if let Some(animation) = params.text_animation {
            animate_text(&mut document, &handles, animation, frame);
        }
        let runtime_started = Instant::now();
        let (update, runtime_allocations) = allocations::measure(args.allocation_counts, || {
            document
                .flush(viewport, &mut shaper)
                .expect("settled GPU document flush")
        });
        let runtime_elapsed = runtime_started.elapsed();
        for (index, preview) in previews.iter().enumerate() {
            preview.write_uniform(&queue, frame.wrapping_add(index) as u32);
        }
        let mut recording = gpu.begin_frame("nana-gpu-scene-benchmark");
        let encoder = __framework::encoder(&mut recording);
        if let Some(probe) = &queries {
            probe.stamp(encoder, 0);
        }
        for preview in &previews {
            preview.encode(encoder);
        }
        if let Some(probe) = &queries {
            probe.stamp(encoder, 1);
        }
        let prepare_started = Instant::now();
        let (_, paint_allocations) = allocations::measure(args.allocation_counts, || {
            painter
                .paint(
                    document.scene(),
                    &mut recording,
                    &target,
                    paint_viewport,
                    Some(&textures),
                    renderers.as_ref(),
                )
                .expect("gpu-scene-ui paint")
        });
        let prepare_elapsed = prepare_started.elapsed() + runtime_elapsed;
        if let Some(probe) = &queries {
            let encoder = __framework::encoder(&mut recording);
            probe.stamp(encoder, 2);
            probe.resolve(encoder);
        }
        let submission = recording.submit();
        painter.record_submit(&submission);
        let timings = painter
            .last_gpu_timings()
            .expect("encoded GPU scene frame must time stages");
        let work = painter
            .last_gpu_work()
            .expect("encoded GPU scene frame must record counters");
        let frame_stages = host_frame_stages(
            document.context().last_frame_profile(),
            timings,
            update.is_idle(),
        );
        let gpu_sample = queries.as_mut().map(|probe| probe.read(&device, &queue));
        if sampling {
            let runtime_text = document.context().world().last_text_work_counters();
            shaping.nodes_shaped += runtime_text.text_nodes_shaped;
            shaping.layouts_created += runtime_text.layouts_created;
            shaping.constraint_only_relayouts += runtime_text.constraint_only_relayouts;
            shaping.layout_lookups += runtime_text.layout_cache_lookups;
            if args.allocation_counts {
                allocation_report.observe(runtime_allocations, paint_allocations);
            }
            runtime_passes += update.passes;
            let next_plan = document.scene().frame_plan().expect("valid frame plan");
            structure_plan_rebuilds += usize::from(!Arc::ptr_eq(&plan, &next_plan));
            plan = next_plan;
            maximum_gpu_work.batch_rebuilds =
                maximum_gpu_work.batch_rebuilds.max(work.batch_rebuilds);
            maximum_gpu_work.gpu_upload_bytes =
                maximum_gpu_work.gpu_upload_bytes.max(work.gpu_upload_bytes);
            maximum_gpu_work.gpu_buffer_reallocations = maximum_gpu_work
                .gpu_buffer_reallocations
                .max(work.gpu_buffer_reallocations);
            maximum_gpu_work.draw_batches = maximum_gpu_work.draw_batches.max(work.draw_batches);
            maximum_gpu_work.draw_calls = maximum_gpu_work.draw_calls.max(work.draw_calls);
            prepare.push(prepare_elapsed);
            if let Some([producer, ui]) = gpu_sample {
                producer_gpu.push(producer);
                ui_gpu.push(ui);
            }
            batch.push(timings.batch);
            upload.push(timings.gpu_upload);
            encode.push(timings.encode);
            submit.push(timings.submit);
            let done = match args.sample_seconds {
                Some(seconds) => sampled_at.unwrap().elapsed().as_secs_f64() >= seconds,
                None => batch.len() >= FRAMES,
            };
            if done {
                break (work, frame_stages);
            }
        }
        frame += 1;
    };
    let text = warm_text.map(|(warm, warm_misses)| {
        shaping.paint_misses = painter
            .text_shape_cache_stats()
            .1
            .saturating_sub(warm_misses);
        text_counters_per_frame(
            warm,
            painter.text_glyph_counters(),
            shaping,
            batch.len().max(1),
        )
    });
    Report {
        schema_version: 1,
        status: "ok",
        unsupported_reason: None,
        scenario_id: Some(scenario.id),
        composition: "UiOnly".into(),
        materialization: Some(materialization),
        frame_graph: graph,
        adapter: Some(adapter),
        frames: Some(batch.len()),
        gpu_work: Some(GpuWorkSnapshot::from(work)),
        gpu_policy: Some(GpuPolicySnapshot::from(gpu.policy().stats())),
        text_counters: text,
        frame_stages: Some(last_stages),
        sampling: Some(SamplingReport {
            elapsed_seconds: sampled_at.unwrap().elapsed().as_secs_f64(),
            warmup_seconds: sampled_at
                .unwrap()
                .duration_since(warmup_started)
                .as_secs_f64(),
            mode: if args.gpu_timestamps {
                "offscreen-gpu-completion-serialized"
            } else {
                "offscreen-submit"
            },
            surface_present_measured: false,
            framework_cpu_prepare_ms: summarize(&prepare),
            runtime_passes,
            structure_plan_rebuilds,
            maximum_gpu_work_per_frame: GpuWorkSnapshot::from(maximum_gpu_work),
        }),
        gpu_timestamps: args.gpu_timestamps.then(|| TimestampReport {
            producer_ms: summarize(&producer_gpu),
            ui_composition_ms: summarize(&ui_gpu),
        }),
        framework_thread_allocations: args.allocation_counts.then_some(allocation_report),
        stages: Some(StageReport {
            batch_ms: summarize(&batch),
            gpu_upload_ms: summarize(&upload),
            encode_ms: summarize(&encode),
            submit_ms: summarize(&submit),
        }),
    }
}

/// The nodes a scenario animates.
struct DocumentHandles {
    root: Entity<List>,
    texts: Vec<Entity<Text>>,
}

/// Change the one thing `animation` names about the labels, for this frame.
fn animate_text(
    document: &mut RuntimeDocument,
    handles: &DocumentHandles,
    animation: TextAnimation,
    frame: usize,
) {
    match animation {
        TextAnimation::Color => {
            let role = if frame.is_multiple_of(2) {
                SemanticColorRole::Text
            } else {
                SemanticColorRole::Muted
            };
            for text in &handles.texts {
                document
                    .context_mut()
                    .set_component(*text, Text::new(UI_ONLY_TEXT).color(role))
                    .expect("recolor text");
            }
        }
        TextAnimation::Opacity => {
            let mut style = root_style();
            Arc::make_mut(&mut style.layout).opacity =
                Some(0.35 + 0.6 * ((frame % 32) as f32 / 32.0));
            document
                .context_mut()
                .set_component(handles.root, List::new().label(ROOT_LABEL).style(style))
                .expect("fade list");
        }
        TextAnimation::Transform => {
            // -1.5°, upright, +1.5°: the list passes through the identity
            // every third frame, which is the switch between a translated and
            // a projected run. The cycle fits inside the warm-up, so a label
            // that only turns into view at one end of it is built there
            // rather than counted as a rebuild.
            let angle = ((frame % 3) as f32 - 1.0) * 1.5f32.to_radians();
            let (sin, cos) = angle.sin_cos();
            let mut style = root_style();
            let layout = Arc::make_mut(&mut style.layout);
            layout.transform = Some(PaintTransform {
                a: cos,
                b: sin,
                c: -sin,
                d: cos,
                ..PaintTransform::default()
            });
            layout.transform_origin = Some(TransformOrigin::default());
            document
                .context_mut()
                .set_component(handles.root, List::new().label(ROOT_LABEL).style(style))
                .expect("turn list");
        }
        TextAnimation::Slide => {
            // A pure translation that is almost never a whole pixel. The
            // painter snaps it, which is what keeps every label's glyphs; the
            // Runtime's scroll offset reaches it the same way, as a
            // translation of everything under the scroller. Eight pixels of
            // travel keeps every label as much on screen as it started.
            let mut style = root_style();
            Arc::make_mut(&mut style.layout).transform = Some(PaintTransform {
                e: -(frame as f32 * 0.37 % 8.0),
                ..PaintTransform::default()
            });
            document
                .context_mut()
                .set_component(handles.root, List::new().label(ROOT_LABEL).style(style))
                .expect("slide list");
        }
        TextAnimation::Resize => {
            let width = if frame.is_multiple_of(2) {
                RESIZE_WIDTHS[0]
            } else {
                RESIZE_WIDTHS[1]
            };
            for text in &handles.texts {
                document
                    .context_mut()
                    .set_component(*text, Text::new(RESIZE_TEXT).style(resize_style(width)))
                    .expect("resize text");
            }
        }
    }
}

/// Both widths wrap [`RESIZE_TEXT`] onto more than one line, and the labels
/// start at neither, so the first sampled frame is already a change.
const RESIZE_WIDTHS: [f32; 2] = [72.0, 60.0];
/// Words to wrap between: a width change moves line breaks, not just the box.
const RESIZE_TEXT: &str = "UiOnly wraps here";

fn resize_style(width: f32) -> NodeStyle {
    let mut style = NodeStyle::default();
    Arc::make_mut(&mut style.layout).width = Some(LengthSpec::Px(width));
    style
}

const UI_ONLY_TEXT: &str = "UiOnly";
const ROOT_LABEL: &str = "gpu-scene-ui";

fn ui_document(params: &ScenarioParams) -> Result<(RuntimeDocument, DocumentHandles), String> {
    if !params.ui_nodes.iter().any(|node| node == "list") {
        return Err("UiOnly ui_nodes must include list as the document root".into());
    }
    if !params
        .ui_nodes
        .iter()
        .any(|node| node == "gpu-texture-view" || node == "gpu-view")
    {
        return Err(
            "UiOnly ui_nodes must include gpu-texture-view or gpu-view for the GPU content slot"
                .into(),
        );
    }
    let document_id = DocumentId::new(1).expect("gpu-scene document");
    let mut document = RuntimeDocument::new(document_id);
    let root = document
        .context_mut()
        .create_component(
            document_id,
            List::new().label(ROOT_LABEL).style(root_style()),
        )
        .expect("list");
    let mut texture_index = 0;
    let mut gpu_view_index = 0u64;
    let mut texts = Vec::new();
    for kind in &params.ui_nodes {
        for _ in 0..params.repeat(kind) {
            match kind.as_str() {
                "list" => {}
                "text" => {
                    let child = document
                        .context_mut()
                        .create_component(
                            document_id,
                            if params.text_animation == Some(TextAnimation::Resize) {
                                Text::new(RESIZE_TEXT)
                            } else {
                                Text::new(UI_ONLY_TEXT)
                            },
                        )
                        .expect("text");
                    document
                        .context_mut()
                        .append_child(root, child)
                        .expect("text child");
                    texts.push(child);
                }
                "icon" => {
                    let child = document
                        .context_mut()
                        .create_component(document_id, IconGlyph::new(Icon::File))
                        .expect("icon");
                    document
                        .context_mut()
                        .append_child(root, child)
                        .expect("icon child");
                }
                "gpu-texture-view" => {
                    let slot = params.texture_slot(texture_index);
                    texture_index += 1;
                    let child = document
                        .context_mut()
                        .create_component(
                            document_id,
                            GpuTextureView::new(slot.as_str()).style(slot_style(
                                params.host_texture.width,
                                params.host_texture.height,
                            )),
                        )
                        .expect("gpu-texture-view");
                    document
                        .context_mut()
                        .append_child(root, child)
                        .expect("slot child");
                }
                "gpu-view" => {
                    // A shared slot keeps one external resource for the whole run;
                    // distinct slots are the worst case the render graph must build.
                    let slot_id = if params.shared_gpu_view_slot {
                        0
                    } else {
                        gpu_view_index
                    };
                    gpu_view_index += 1;
                    let child = document
                        .context_mut()
                        .create_component(
                            document_id,
                            GpuView::new(slot_id)
                                .palette(GpuViewPalette {
                                    background: [0.05, 0.06, 0.09, 1.0],
                                    accent: [0.35, 0.72, 0.98, 1.0],
                                })
                                .seed(slot_id as f32 * 0.125)
                                .style(slot_style(GPU_VIEW_EXTENT, GPU_VIEW_EXTENT)),
                        )
                        .expect("gpu-view");
                    document
                        .context_mut()
                        .append_child(root, child)
                        .expect("gpu-view child");
                }
                "button" => {
                    let child = document
                        .context_mut()
                        .create_component(
                            document_id,
                            Button::new("HostTexture").kind(ButtonKind::Primary),
                        )
                        .expect("button");
                    document
                        .context_mut()
                        .append_child(root, child)
                        .expect("button child");
                }
                other => {
                    return Err(format!(
                        "UiOnly ui_nodes contains unknown node {other}; catalog allows \
                         list/text/icon/gpu-texture-view/gpu-view/button"
                    ));
                }
            }
        }
    }
    Ok((document, DocumentHandles { root, texts }))
}

/// Wrapping row. A column would push every repeated node past the viewport, and
/// viewport culling would then measure an empty frame instead of the workload.
fn root_style() -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    *layout = LayoutStyle {
        direction: Some(FlexDirection::Row),
        flex_wrap: FlexWrap::Wrap,
        ..LayoutStyle::default()
    };
    style
}

fn slot_style(width: u32, height: u32) -> NodeStyle {
    let mut style = NodeStyle::default();
    let layout = Arc::make_mut(&mut style.layout);
    *layout = LayoutStyle {
        width: Some(LengthSpec::Px(width as f32)),
        height: Some(LengthSpec::Px(height as f32)),
        ..LayoutStyle::default()
    };
    style
}

fn measure_frame_graph(scene: &nana_ui_scene::UiScene) -> Option<FrameGraphReport> {
    let started = Instant::now();
    let graph = scene.frame_graph(nana_ui_scene::ResourceId(1)).ok()?;
    let build_ms = started.elapsed().as_secs_f64() * 1_000.0;
    Some(FrameGraphReport {
        build_ms,
        pass_count: graph.passes.len(),
        resource_count: graph.resources.len(),
    })
}

fn scene_primitive_kinds(scene: &nana_ui_scene::UiScene, slot: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    for primitive in scene.primitives() {
        let name = primitive_kind_name(&primitive.kind, slot);
        if !kinds.iter().any(|existing| existing == name) {
            kinds.push(name.to_string());
        }
    }
    kinds.sort();
    kinds
}

fn primitive_kind_name(kind: &ScenePrimitiveKind, _slot: &str) -> &'static str {
    match kind {
        ScenePrimitiveKind::Quad { .. }
        | ScenePrimitiveKind::QuadBatch { .. }
        | ScenePrimitiveKind::QuadColorBatch { .. } => "quad",
        ScenePrimitiveKind::Text { .. } => "text",
        ScenePrimitiveKind::Icon { .. } | ScenePrimitiveKind::IconBatch { .. } => "icon",
        ScenePrimitiveKind::Spinner { .. } => "spinner",
        ScenePrimitiveKind::Stroke { .. } => "stroke",
        ScenePrimitiveKind::Path { .. } => "path",
        ScenePrimitiveKind::LayerBegin { .. } | ScenePrimitiveKind::LayerEnd { .. } => "layer",
        ScenePrimitiveKind::Custom { node: custom, .. }
            if custom.renderer.as_ref() == HOST_TEXTURE_RENDERER =>
        {
            "host-texture"
        }
        ScenePrimitiveKind::Custom { .. } => "custom",
    }
}

fn host_frame_stages(
    cpu: &FrameProfile,
    timings: GpuStageTimings,
    runtime_idle: bool,
) -> BTreeMap<String, StageStatusReport> {
    let mut profiler = FrameProfiler::new();
    for timing in &cpu.stages {
        if timing.stage.gpu_host_owned() {
            continue;
        }
        if runtime_idle {
            profiler.skip(timing.stage);
            continue;
        }
        match timing.status {
            StageStatus::Ran => profiler.record(timing.stage, timing.duration),
            StageStatus::Skipped => profiler.skip(timing.stage),
            StageStatus::Unsupported => profiler.unsupported(timing.stage),
        }
    }
    timings.record_on(&mut profiler);
    profiler
        .finish()
        .stages
        .into_iter()
        .map(|timing| {
            (
                format!("{:?}", timing.stage),
                StageStatusReport {
                    status: stage_status_name(timing.status),
                },
            )
        })
        .collect()
}

fn stage_status_name(status: StageStatus) -> &'static str {
    match status {
        StageStatus::Ran => "ran",
        StageStatus::Skipped => "skipped",
        StageStatus::Unsupported => "unsupported",
    }
}

fn request_device(gpu_timestamps: bool) -> Option<(GpuContext, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env_or_default(
        &instance, None,
    ))
    .ok()?;
    let required_features = if gpu_timestamps {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
    } else {
        wgpu::Features::empty()
    };
    if !adapter.features().contains(required_features) {
        return None;
    }
    let info = adapter.get_info();
    let label = format!("{} ({:?})", info.name, info.backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("nana-gpu-scene-benchmark"),
        required_features,
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .ok()?;
    Some((__framework::adopt(adapter, device, queue), label))
}

fn color_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-gpu-scene-benchmark target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

struct HostSlotContent {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    host: HostTexture,
}

impl HostSlotContent {
    fn new(gpu: &GpuContext, size: (u32, u32)) -> Self {
        let device = __framework::device(gpu);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nana-gpu-scene-benchmark slot"),
            source: wgpu::ShaderSource::Wgsl(SLOT_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nana-gpu-scene-benchmark slot layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nana-gpu-scene-benchmark slot pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nana-gpu-scene-benchmark slot pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nana-gpu-scene-benchmark slot uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nana-gpu-scene-benchmark slot bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-gpu-scene-benchmark host texture"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let host = HostTexture::new(1, 1, &__framework::texture_from_wgpu(gpu, texture.clone()));
        Self {
            pipeline,
            bind_group,
            uniform,
            _texture: texture,
            view,
            host,
        }
    }

    fn texture(&self) -> HostTexture {
        self.host.clone()
    }

    fn write_uniform(&self, queue: &wgpu::Queue, frame: u32) {
        let seed = frame as f32 * 0.17;
        let parameters = [seed, 0.0, 0.0, 0.0];
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&parameters));
        self.host.invalidate();
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-gpu-scene-benchmark host slot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

const SLOT_SHADER: &str = r#"
struct SceneUniform {
    parameters: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fragment_main() -> @location(0) vec4<f32> {
    let seed = scene.parameters.x;
    return vec4<f32>(0.12 + seed * 0.01, 0.28, 0.46, 1.0);
}
"#;

fn summarize(samples: &[Duration]) -> Distribution {
    let mut values = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.partial_cmp(right).unwrap());
    Distribution {
        p50: percentile(&values, 0.50),
        p95: percentile(&values, 0.95),
        p99: percentile(&values, 0.99),
        max: values.last().copied().unwrap_or(0.0),
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn write_report(path: &Option<PathBuf>, report: &Report) {
    let json = serde_json::to_string_pretty(report).expect("serialize gpu-scene report") + "\n";
    if let Some(path) = path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).expect("write gpu-scene report directory");
        }
        fs::write(path, json).expect("write gpu-scene report");
    } else {
        print!("{json}");
    }
}
