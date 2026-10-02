//! Headless text-paint bench for Issue #98.
//!
//! What it measures is the frame a shell actually pays for: one that *rebatches*.
//! The painter already skips everything when nothing in the scene moved, so the
//! interesting question is what a text-heavy frame costs when something else on
//! screen did — an animation, a hover, a caret. Each workload below changes
//! exactly one thing and reports what the text path had to redo for it.
//!
//! The numbers that decide whether #98 landed are the work counters, not the
//! milliseconds: `text_instance_rebuilds`, `text_instance_upload_bytes` and
//! `glyph_resolve_requests` say whether a steady frame touched a glyph at all.
//! The timings are here because a structural win that costs more CPU is not a
//! win, not because a public CI runner can resolve 20 µs.
//!
//! ```bash
//! cargo run --release --locked -p nana-ui --features gpu \
//!     --bin nana-text-paint-benchmark -- --output target/performance/issue98/text-paint.json
//! ```
//!
//! `--scale 1.25` paints the same logical viewport onto a display of that many
//! physical px per logical px: where a scroll by a logical pixel is a fraction.

use std::collections::BTreeMap;
use std::sync::Arc;

use nana_gpu::{__framework, GpuContext, GpuRenderTarget};
use nana_ui::runtime::{
    DocumentId, FlexDirection, FlexWrap, LayoutStyle, LayoutViewport, LengthSpec, MutationQueue,
    NodeKind, NodeStyle, RuntimeDocument, ScrollOffset, SemanticColorRole, StableNodeId,
    TextContent,
};
use nana_ui::{NanaTextShaper, RenderTargetId, ScenePaintViewport, SceneWgpuPainter};
use nana_ui_core::{OverflowSpec, PaintTransform, TransformOrigin};
use serde::Serialize;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const WARMUP_FRAMES: usize = 8;
/// Frames of [`Workload::MutateRandom`] churn [`Workload::MutateRandomSettle`]
/// runs before it stops: enough for the draw order to repack with gaps.
const SETTLE_CHURN_FRAMES: usize = 60;
/// One label's box. The viewport is sized from these so every label a cell
/// asks for is actually painted — a column taller than the screen would have
/// the scene cull the rest and every cell would measure the same forty rows.
const LABEL: [f32; 2] = [104.0, 20.0];
/// Distinct strings a repeated-label cell draws from. A table repeats its
/// cells, and a paint benchmark that gave every label its own string would
/// measure the shape cache instead of the paint path.
const DISTINCT_LABELS: usize = 64;

/// Labels inside the panel [`Workload::TransformPanel`] animates.
const PANEL_ROWS: usize = 8;

/// One wrapped paragraph's box, for [`Workload::Paragraphs`]: four or five
/// lines of body text.
const PARAGRAPH: [f32; 2] = [416.0, 96.0];
/// Labels per paragraph when a cell's label count is read as paragraphs. A
/// paragraph holds about as many glyphs as this many labels.
const LABELS_PER_PARAGRAPH: usize = 8;
const PARAGRAPH_TEXT: &str = "Retained text keeps what a paragraph resolved to between \
    frames, so a frame that moves something else on screen does not walk its \
    glyphs again. Wrapping, alignment and ellipsis are decided once, when the \
    text or its box changes, and never because a neighbour animated.";

/// Distinct ideographs [`Workload::AtlasPressure`] draws from, and the sizes
/// it cycles through: together far more glyph area than the atlas budget.
const PRESSURE_POOL: u32 = 20_000;

/// What [`Workload::MutateRandom`] cuts its text from: any start below 16 and
/// any length up to 14 stays inside it.
const HEX_TWICE: &str = "0123456789abcdef0123456789abcdef";
const PRESSURE_SIZES: [f32; 6] = [32.0, 44.0, 56.0, 72.0, 88.0, 104.0];

/// Label counts Issue #98 asks for.
const LABEL_GRID: [usize; 4] = [1, 100, 1_000, 10_000];
/// Frame counts standing in for 60 / 120 / 240 Hz of one second of animation.
const RATE_GRID: [usize; 3] = [60, 120, 240];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Workload {
    /// Nothing about the text changes; something else in the scene does, so the
    /// frame is rebatched. This is the row the whole issue is about.
    Static,
    /// `Static`, but every label has its own string. Beyond the shape cache's
    /// capacity this stops measuring paint and starts measuring reshaping,
    /// which is the point of keeping the row.
    StaticUnique,
    /// Every label's color cycles. No reshape, no raster, no instance.
    Color,
    /// The container fades. Presentation only.
    Opacity,
    /// The container rotates. Presentation only.
    Transform,
    /// A panel of [`PANEL_ROWS`] labels rotates while the rest of the document
    /// stands still — what an animation in a real shell moves. `Transform`
    /// rotates everything, which is the worst case, not the common one.
    TransformPanel,
    /// One label in a hundred gets new text.
    Mutate,
    /// One label in a hundred, chosen at random each frame, gets text of a
    /// random length: the churn a live table or a log view produces, where
    /// paragraphs grow and shrink rather than being swapped like for like.
    MutateRandom,
    /// [`Self::MutateRandom`] for [`SETTLE_CHURN_FRAMES`] warm-up frames, then
    /// [`Self::Static`]: the table that stopped changing, whose draw order
    /// still holds the gaps the churn asked for until it gives them back
    /// (#230).
    MutateRandomSettle,
    /// A text-heavy table: every cell its own string of digits, dates and
    /// names, beside a ticking label.
    Table,
    /// The same table scrolling by whole pixels.
    TableScroll,
    /// The same table scrolled sideways by 0.37 px a frame, the way a trackpad
    /// scrolls: through the Runtime's own scroll offset, which keeps the
    /// fraction, into a painter that draws it on whole device pixels (#223).
    TableScrollX,
    /// Wrapped paragraphs, one per [`LABELS_PER_PARAGRAPH`] labels, beside a
    /// ticking label.
    Paragraphs,
    /// The same one label in a hundred gets four new large ideographs every
    /// frame, from a pool much bigger than the atlas and at a size that
    /// changes every frame: a stream of new glyphs through a working set that
    /// does fit. The atlas fills, evicts the glyphs those labels stopped
    /// showing, and keeps drawing everything else from its entries.
    ///
    /// Not a working set larger than the atlas. A screen that shows more
    /// glyph area than the budget has to evict what it is drawing, every
    /// frame, whatever the renderer does; that is a capacity question, not
    /// a retention one.
    AtlasPressure,
    /// `Static`, painted into two windows that share one device.
    MultiWindow,
    /// The container zooms from 1x to 2x: a scene scale animation, which may
    /// re-rasterize at a few half-octave steps and never once per frame.
    Zoom,
}

impl Workload {
    fn id(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::StaticUnique => "static-unique",
            Self::Color => "color",
            Self::Opacity => "opacity",
            Self::Transform => "transform",
            Self::TransformPanel => "transform-panel",
            Self::Mutate => "mutate-1pct",
            Self::MutateRandom => "mutate-random-1pct",
            Self::MutateRandomSettle => "mutate-random-settle",
            Self::Table => "table",
            Self::TableScroll => "table-scroll",
            Self::TableScrollX => "table-scroll-x",
            Self::Paragraphs => "paragraphs",
            Self::AtlasPressure => "atlas-pressure",
            Self::MultiWindow => "multi-window",
            Self::Zoom => "zoom",
        }
    }

    const ALL: [Self; 16] = [
        Self::Static,
        Self::StaticUnique,
        Self::Color,
        Self::Opacity,
        Self::Transform,
        Self::TransformPanel,
        Self::Mutate,
        Self::MutateRandom,
        Self::MutateRandomSettle,
        Self::Table,
        Self::TableScroll,
        Self::TableScrollX,
        Self::Paragraphs,
        Self::AtlasPressure,
        Self::MultiWindow,
        Self::Zoom,
    ];

    /// Frames before the sampled ones. The churn a settling workload stops
    /// is warm-up, so what it samples is only the frames after it.
    fn warmup(self) -> usize {
        match self {
            Self::MutateRandomSettle => WARMUP_FRAMES + SETTLE_CHURN_FRAMES,
            _ => WARMUP_FRAMES,
        }
    }

    /// Whether the workload is an animation, and therefore worth running at
    /// each of [`RATE_GRID`].
    fn animated(self) -> bool {
        matches!(
            self,
            Self::Color
                | Self::Opacity
                | Self::Transform
                | Self::TransformPanel
                | Self::TableScroll
                | Self::TableScrollX
                | Self::Zoom
        )
    }
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    adapter: String,
    cells: Vec<Cell>,
}

#[derive(Serialize)]
struct Cell {
    workload: String,
    labels: usize,
    /// Wrapper elements between the document and the labels.
    depth: usize,
    /// Physical px per logical px the frame is painted at.
    scale: f32,
    /// In physical px.
    viewport: [u32; 2],
    frames: usize,
    /// Glyphs the visible labels resolve to, so a per-glyph reading is possible.
    live_glyphs: u64,
    /// Per sampled frame.
    counters: BTreeMap<String, f64>,
    /// `RuntimeDocument::flush`: extraction, layout and the scene delta. The
    /// painter's `batch_ms` does not include it, and a container style change
    /// spends most of a frame here rather than there.
    flush_ms: Percentiles,
    batch_ms: Percentiles,
    gpu_upload_ms: Percentiles,
    /// Submit to completion, waited on every frame: the GPU's time for it,
    /// plus the submit itself.
    gpu_ms: Percentiles,
    /// Recording the passes: where each extra draw's CPU cost lands.
    encode_ms: Percentiles,
}

#[derive(Serialize)]
struct Percentiles {
    p50: f64,
    p95: f64,
    max: f64,
}

impl Percentiles {
    fn of(mut samples: Vec<f64>) -> Self {
        if samples.is_empty() {
            return Self {
                p50: 0.0,
                p95: 0.0,
                max: 0.0,
            };
        }
        samples.sort_by(f64::total_cmp);
        let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
        Self {
            p50: at(0.5),
            p95: at(0.95),
            max: samples[samples.len() - 1],
        }
    }
}

fn main() {
    let mut output = None;
    // Filters, so one cell can be re-measured on an otherwise idle machine
    // rather than inside a fifteen-minute sweep that shares the GPU with
    // whatever else is running.
    let mut only_labels: Vec<usize> = Vec::new();
    let mut only_workload: Vec<String> = Vec::new();
    let mut frame_override: Option<usize> = None;
    let mut depth = 0usize;
    let mut scale = 1.0f32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => output = args.next(),
            "--labels" => {
                let Some(value) = args.next().and_then(|raw| raw.parse().ok()) else {
                    eprintln!("--labels needs a count");
                    std::process::exit(2);
                };
                only_labels.push(value);
            }
            "--frames" => {
                let Some(value) = args.next().and_then(|raw| raw.parse().ok()) else {
                    eprintln!("--frames needs a count");
                    std::process::exit(2);
                };
                frame_override = Some(value);
            }
            "--depth" => {
                let Some(value) = args.next().and_then(|raw| raw.parse().ok()) else {
                    eprintln!("--depth needs a count");
                    std::process::exit(2);
                };
                depth = value;
            }
            "--scale" => {
                let Some(value) = args
                    .next()
                    .and_then(|raw| raw.parse::<f32>().ok())
                    .filter(|value| value.is_finite() && *value > 0.0)
                else {
                    eprintln!("--scale needs a positive factor");
                    std::process::exit(2);
                };
                scale = value;
            }
            "--workload" => {
                let Some(value) = args.next() else {
                    eprintln!("--workload needs an id");
                    std::process::exit(2);
                };
                only_workload.push(value);
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some((gpu, adapter)) = gpu() else {
        eprintln!("no wgpu adapter; nothing to measure");
        std::process::exit(2);
    };
    let mut cells = Vec::new();
    for labels in LABEL_GRID {
        if !only_labels.is_empty() && !only_labels.contains(&labels) {
            continue;
        }
        for workload in Workload::ALL {
            if !only_workload.is_empty() && !only_workload.iter().any(|id| id == workload.id()) {
                continue;
            }
            let rates: &[usize] = if workload.animated() {
                &RATE_GRID
            } else if workload == Workload::MutateRandomSettle {
                // Long enough that the frame the order gives its gaps back
                // falls well inside the window, and most of what it samples
                // is after it.
                &[RATE_GRID[2]]
            } else {
                &[RATE_GRID[0]]
            };
            for frames in rates {
                let frames = frame_override.unwrap_or(*frames);
                cells.push(run(&gpu, workload, labels, frames, depth, scale));
            }
        }
    }
    print_table(&cells);
    let report = Report {
        schema_version: 1,
        adapter,
        cells,
    };
    if let Some(path) = output {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&report).expect("serializable report"),
        )
        .expect("write report");
        println!("\nwrote {path}");
    }
}

fn gpu() -> Option<(GpuContext, String)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let info = adapter.get_info();
    // The adapter's own limits: a ten-thousand-label viewport at `--scale 2`
    // is past the 8192 px texture side the defaults allow.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("nana-text-paint-benchmark"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some((
        __framework::adopt(adapter, device, queue),
        format!("{} ({:?})", info.name, info.backend),
    ))
}

/// A viewport that holds `count` boxes of `cell`, roughly square.
fn viewport_for(count: usize, cell: [f32; 2]) -> [u32; 2] {
    let area = count as f32 * cell[0] * cell[1];
    let width = (area.sqrt().max(cell[0]) / cell[0]).ceil() * cell[0];
    let columns = (width / cell[0]).max(1.0);
    let rows = (count as f32 / columns).ceil().max(1.0);
    [
        (width as u32).clamp(320, 8192),
        ((rows * cell[1]) as u32).clamp(240, 8192),
    ]
}

fn run(
    context: &GpuContext,
    workload: Workload,
    labels: usize,
    frames: usize,
    depth: usize,
    scale: f32,
) -> Cell {
    let (count, cell) = if workload == Workload::Paragraphs {
        ((labels / LABELS_PER_PARAGRAPH).max(1), PARAGRAPH)
    } else {
        (labels, LABEL)
    };
    let device = __framework::device(context);
    let logical = viewport_for(count, cell);
    let physical = logical.map(|side| (side as f32 * scale).round() as u32);
    let largest = device.limits().max_texture_dimension_2d;
    if physical.iter().any(|side| *side > largest) {
        eprintln!(
            "{} with {labels} labels at {scale}x needs a {}x{} target; this device allows {largest}",
            workload.id(),
            physical[0],
            physical[1]
        );
        std::process::exit(2);
    }
    let document_id = DocumentId::new(1).expect("document");
    let mut document = RuntimeDocument::new(document_id);
    let root = StableNodeId::new(1).expect("root");
    let column = StableNodeId::new(2).expect("column");
    let ticker = StableNodeId::new(3).expect("ticker");
    let mut build = MutationQueue::new();
    build.create(root, document_id, NodeKind::Document);
    // Wrappers between the document and the labels. A real shell's text sits
    // ten to twenty elements deep, and everything the painter asks per
    // primitive walks that chain.
    let mut parent = root;
    for level in 0..depth {
        let wrapper = StableNodeId::new(1_000_000 + level as u64).expect("wrapper");
        build.create(
            wrapper,
            document_id,
            NodeKind::Element { tag: "div".into() },
        );
        build.insert(parent, wrapper, None);
        build.set_style(wrapper, column_style(None, None));
        parent = wrapper;
    }
    build.create(column, document_id, NodeKind::Element { tag: "div".into() });
    build.insert(parent, column, None);
    build.set_style(column, column_style(None, None));
    build.create(ticker, document_id, NodeKind::Text);
    build.insert(column, ticker, None);
    build.set_text(ticker, TextContent { value: ".".into() });
    build.set_style(ticker, label_style(LABEL));
    // The panel `TransformPanel` animates. Only that workload builds it, so
    // every other row keeps the tree it has always been measured on.
    let panel = StableNodeId::new(2_000_000).expect("panel");
    if workload == Workload::TransformPanel {
        build.create(panel, document_id, NodeKind::Element { tag: "div".into() });
        build.insert(column, panel, None);
        build.set_style(panel, panel_style(None));
    }
    // What `TableScrollX` scrolls: the column, over a sheet of labels a
    // little wider than it. The sheet holds as many columns of labels as the
    // viewport does, so the extra width is empty and scrolling across it
    // brings no label into view or out of it — every frame measures a scroll,
    // not a label being built for the first time.
    let sheet = StableNodeId::new(3_000_000).expect("sheet");
    if workload == Workload::TableScrollX {
        build.set_style(column, scrolling_column_style());
        build.create(sheet, document_id, NodeKind::Element { tag: "div".into() });
        build.insert(column, sheet, None);
        build.set_style(sheet, sheet_style(logical[0] as f32 + SHEET_OVERHANG));
    }
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        let label = StableNodeId::new(4 + index as u64).expect("label");
        let parent = if workload == Workload::TransformPanel && index < PANEL_ROWS {
            panel
        } else if workload == Workload::TableScrollX {
            sheet
        } else {
            column
        };
        build.create(label, document_id, NodeKind::Text);
        build.insert(parent, label, None);
        build.set_text(
            label,
            TextContent {
                value: label_text(workload, index).into(),
            },
        );
        build.set_style(label, label_style(cell));
        rows.push(label);
    }
    document
        .context_mut()
        .commit_mutations(build)
        .expect("build document");

    let viewport = LayoutViewport::new(logical[0] as f32, logical[1] as f32);
    let mut shaper = NanaTextShaper::default();
    let mut painter = SceneWgpuPainter::new(context, __framework::format_from_wgpu(FORMAT));
    let target = color_target(context, physical);
    // The second window of `MultiWindow`: its own surface, the same device,
    // the same painter — so the same atlas and the same shaped paragraphs.
    let second = (workload == Workload::MultiWindow).then(|| color_target(context, physical));
    let paint_viewport = ScenePaintViewport {
        logical_size: [logical[0] as f32, logical[1] as f32],
        physical_size: physical,
        scale_factor: scale,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.08, 0.08, 0.09, 1.0],
        clear: true,
    };

    let mut batch = Vec::new();
    let mut flush = Vec::new();
    let mut upload = Vec::new();
    let mut gpu = Vec::new();
    let mut encode = Vec::new();
    let mut warm = None;
    let mut warm_glyph = None;
    let mut warm_shape = None;
    let warmup = workload.warmup();
    for frame in 0..warmup + frames {
        mutate(
            &mut document,
            workload,
            if workload == Workload::TransformPanel {
                panel
            } else {
                column
            },
            ticker,
            &rows,
            frame,
        );
        let flush_started = std::time::Instant::now();
        document.flush(viewport, &mut shaper).expect("flush");
        let flush_elapsed = flush_started.elapsed();
        let mut recording = context.begin_frame("nana-text-paint-benchmark");

        let mut frame_batch = std::time::Duration::ZERO;
        let mut frame_upload = std::time::Duration::ZERO;
        let mut frame_encode = std::time::Duration::ZERO;
        for (window, view) in std::iter::once(&target).chain(second.as_ref()).enumerate() {
            // One window paints the way a single-window host does, so these
            // cells stay comparable with every earlier report.
            if second.is_none() {
                painter.paint(
                    document.scene(),
                    &mut recording,
                    view,
                    paint_viewport,
                    None,
                    None,
                )
            } else {
                painter.paint_target(
                    RenderTargetId(window as u64),
                    document.scene(),
                    &mut recording,
                    view,
                    paint_viewport,
                    None,
                    None,
                )
            }
            .expect("paint");
            let timings = painter.last_gpu_timings().expect("timed frame");
            frame_batch += timings.batch;
            frame_upload += timings.gpu_upload;
            frame_encode += timings.encode;
        }
        let submitted = std::time::Instant::now();
        recording.submit();
        // Waited on every frame so this is the GPU's own time for the frame,
        // not a queue that is running behind. What a vertex stage change costs
        // shows up here and nowhere else in this report.
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the frame completes");
        let gpu_elapsed = submitted.elapsed();
        if frame + 1 == warmup {
            warm = Some(painter.text_glyph_counters());
            warm_glyph = Some(painter.text_glyph_counters().text_gpu_entry_glyphs);
            warm_shape = Some(painter.text_shape_cache_stats());
        }
        if frame >= warmup {
            flush.push(flush_elapsed.as_secs_f64() * 1000.0);
            batch.push(frame_batch.as_secs_f64() * 1000.0);
            upload.push(frame_upload.as_secs_f64() * 1000.0);
            gpu.push(gpu_elapsed.as_secs_f64() * 1000.0);
            encode.push(frame_encode.as_secs_f64() * 1000.0);
        }
    }
    let warm = warm.expect("warm counters");
    let end = painter.text_glyph_counters();
    let per_frame = frames.max(1) as f64;
    let mut counters = BTreeMap::new();
    let mut delta = |name: &str, after: u64, before: u64| {
        counters.insert(
            name.to_string(),
            (after.saturating_sub(before)) as f64 / per_frame,
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
        "text_instance_patches",
        end.text_instance_patches,
        warm.text_instance_patches,
    );
    delta(
        "text_instance_upload_bytes",
        end.text_instance_upload_bytes,
        warm.text_instance_upload_bytes,
    );
    // #224: the draw-order index table. Separate from the instances, so a
    // paragraph that moved in the arena shows up as its own bytes there and
    // whatever reordering it caused shows up here.
    delta(
        "text_index_upload_bytes",
        end.text_index_upload_bytes,
        warm.text_index_upload_bytes,
    );
    delta(
        "text_pipeline_draws",
        end.text_pipeline_draws,
        warm.text_pipeline_draws,
    );
    // #230: the quads the vertex stage runs, gap and slack slots included.
    delta(
        "text_index_slots_drawn",
        end.text_index_slots_drawn,
        warm.text_index_slots_drawn,
    );
    delta(
        "text_presentation_upload_bytes",
        end.text_presentation_upload_bytes,
        warm.text_presentation_upload_bytes,
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
    delta(
        "text_prepare_nodes_culled",
        end.text_prepare_nodes_culled,
        warm.text_prepare_nodes_culled,
    );
    // #99: paragraphs drawn from the layout Runtime measured with, rather than
    // laid out a second time here. Next to `shape_cache_misses` it says which
    // of the two paths a workload is actually on.
    delta(
        "text_retained_layouts_drawn",
        end.text_retained_layouts_drawn,
        warm.text_retained_layouts_drawn,
    );
    delta(
        "text_gpu_entries_created",
        end.text_gpu_entries_created,
        warm.text_gpu_entries_created,
    );
    delta(
        "glyph_atlas_evict",
        end.glyph_atlas_evict,
        warm.glyph_atlas_evict,
    );
    delta(
        "atlas_relocations",
        end.atlas_relocations,
        warm.atlas_relocations,
    );
    delta(
        "atlas_stale_handle_rejects",
        end.atlas_stale_handle_rejects,
        warm.atlas_stale_handle_rejects,
    );
    delta(
        "glyph_raster_cache_hit",
        end.glyph_raster_cache_hit,
        warm.glyph_raster_cache_hit,
    );
    counters.insert(
        "glyph_atlas_pages".to_string(),
        f64::from(end.glyph_atlas_pages),
    );
    counters.insert(
        "glyph_atlas_bytes".to_string(),
        end.glyph_atlas_bytes as f64,
    );
    counters.insert(
        "text_gpu_entries_active".to_string(),
        end.text_gpu_entries_active as f64,
    );
    let (_, warm_misses, warm_evictions) = warm_shape.unwrap_or_default();
    let (_, misses, evictions) = painter.text_shape_cache_stats();
    counters.insert(
        "shape_cache_misses".to_string(),
        (misses - warm_misses) as f64 / per_frame,
    );
    counters.insert(
        "shape_cache_evictions".to_string(),
        (evictions - warm_evictions) as f64 / per_frame,
    );
    Cell {
        workload: workload.id().to_string(),
        labels,
        depth,
        scale,
        viewport: physical,
        frames,
        live_glyphs: warm_glyph.unwrap_or_default(),
        counters,
        flush_ms: Percentiles::of(flush),
        batch_ms: Percentiles::of(batch),
        gpu_upload_ms: Percentiles::of(upload),
        gpu_ms: Percentiles::of(gpu),
        encode_ms: Percentiles::of(encode),
    }
}

/// Change exactly one thing, so the cell's counters name that one thing.
fn mutate(
    document: &mut RuntimeDocument,
    workload: Workload,
    column: StableNodeId,
    ticker: StableNodeId,
    rows: &[StableNodeId],
    frame: usize,
) {
    let mut queue = MutationQueue::new();
    let workload = match workload {
        Workload::MutateRandomSettle if frame < workload.warmup() => Workload::MutateRandom,
        Workload::MutateRandomSettle => Workload::Static,
        other => other,
    };
    match workload {
        Workload::Static
        | Workload::StaticUnique
        | Workload::Table
        | Workload::Paragraphs
        | Workload::MultiWindow
        | Workload::MutateRandomSettle => {
            // Not a text change *to the labels*: the label beside an animation
            // is what a shell spends its frames on.
            queue.set_text(
                ticker,
                TextContent {
                    value: ".".repeat(1 + frame % 3).into(),
                },
            );
        }
        Workload::Color => {
            let role = if frame.is_multiple_of(2) {
                SemanticColorRole::Text
            } else {
                SemanticColorRole::Muted
            };
            for row in rows {
                queue.set_style(
                    *row,
                    NodeStyle {
                        foreground: Some(role),
                        ..NodeStyle::default()
                    },
                );
            }
        }
        Workload::Opacity => {
            let opacity = 0.35 + 0.6 * ((frame % 32) as f32 / 32.0);
            queue.set_style(column, column_style(Some(opacity), None));
        }
        Workload::Transform | Workload::TransformPanel => {
            let angle = (frame % 360) as f32 * std::f32::consts::PI / 180.0;
            let (sin, cos) = angle.sin_cos();
            let turned = Some(PaintTransform {
                a: cos,
                b: sin,
                c: -sin,
                d: cos,
                ..PaintTransform::default()
            });
            queue.set_style(
                column,
                if workload == Workload::TransformPanel {
                    panel_style(turned)
                } else {
                    column_style(None, turned)
                },
            );
        }
        Workload::Mutate => {
            // One row in a hundred, not one in `len / 100`: the first reading
            // of that is "every row" as soon as the list is short.
            for row in rows.iter().step_by(100) {
                queue.set_text(
                    *row,
                    TextContent {
                        value: format!("Tick {frame}").into(),
                    },
                );
            }
        }
        Workload::MutateRandom => {
            let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ (frame as u64).wrapping_mul(0x2545_f491);
            for _ in 0..rows.len().div_ceil(100) {
                let pick = xorshift(&mut state);
                let row = rows[(pick % rows.len() as u64) as usize];
                let length = 1 + (xorshift(&mut state) % 14) as usize;
                let start = pick as usize % 16;
                queue.set_text(
                    row,
                    TextContent {
                        value: format!("Item {}", &HEX_TWICE[start..start + length]).into(),
                    },
                );
            }
        }
        Workload::AtlasPressure => {
            let size = PRESSURE_SIZES[frame % PRESSURE_SIZES.len()];
            let stride = 100;
            for (turn, row) in rows.iter().step_by(stride).enumerate() {
                let first = ((frame * 131 + turn * 7) as u32 * 4) % PRESSURE_POOL;
                let value = (0..4)
                    .filter_map(|offset| char::from_u32(0x4e00 + (first + offset) % PRESSURE_POOL))
                    .collect::<String>();
                queue.set_text(
                    *row,
                    TextContent {
                        value: value.into(),
                    },
                );
                let mut style = label_style(LABEL);
                Arc::make_mut(&mut style.layout).font_size = Some(size);
                queue.set_style(*row, style);
            }
        }
        Workload::TableScroll => {
            // Whole pixels, the way a scroll lands on a 1x display.
            let offset = Some(PaintTransform {
                f: -((frame % 40) as f32),
                ..PaintTransform::default()
            });
            queue.set_style(column, column_style(None, offset));
        }
        Workload::TableScrollX => {
            // Never a whole pixel for long, at any scale.
            let x = frame as f32 * 0.37 % SHEET_OVERHANG;
            queue.set_scroll_offset(column, ScrollOffset { x, y: 0.0 });
        }
        Workload::Zoom => {
            let factor = 1.0 + (frame % 60) as f32 / 60.0;
            let zoomed = Some(PaintTransform {
                a: factor,
                d: factor,
                ..PaintTransform::default()
            });
            queue.set_style(column, column_style(None, zoomed));
        }
    }
    document
        .context_mut()
        .commit_mutations(queue)
        .expect("frame mutation");
}

/// What one label says. Repeated by default; see [`Workload::StaticUnique`].
fn label_text(workload: Workload, index: usize) -> String {
    match workload {
        Workload::StaticUnique => format!("Row {index}"),
        Workload::Table | Workload::TableScroll | Workload::TableScrollX => table_cell(index),
        Workload::Paragraphs => PARAGRAPH_TEXT.to_string(),
        _ => format!("Row {}", index % DISTINCT_LABELS),
    }
}

/// One cell of a text-heavy table: the column decides what kind of text, the
/// row makes it unique.
fn table_cell(index: usize) -> String {
    let row = index / 5;
    match index % 5 {
        0 => format!("#{row:05}"),
        1 => format!("Account {}", row * 37 % 9973),
        2 => format!("{}.{:02}", row * 7919 % 100_000, row % 100),
        3 => format!("2026-{:02}-{:02}", 1 + row % 12, 1 + row % 28),
        _ => ["Active", "Pending", "Closed", "Review"][row % 4].to_string(),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn label_style(cell: [f32; 2]) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(cell[0])),
            height: Some(LengthSpec::Px(cell[1])),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

/// The panel [`Workload::TransformPanel`] animates.
///
/// Sized to the labels it holds rather than filling the column: a `Fill` box
/// here would take the whole viewport and push every other label off screen,
/// and the cell would measure a document nobody paints.
fn panel_style(transform: Option<PaintTransform>) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(LABEL[0] * PANEL_ROWS as f32)),
            height: Some(LengthSpec::Px(LABEL[1])),
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            transform,
            transform_origin: transform.is_some().then(TransformOrigin::default),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

/// How much wider than the viewport [`Workload::TableScrollX`]'s sheet is: the
/// distance it scrolls. Less than a label, so the sheet holds no more columns
/// of them than the viewport.
const SHEET_OVERHANG: f32 = 40.0;

/// The column, scrolling sideways over the sheet.
fn scrolling_column_style() -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            width: Some(LengthSpec::Fill),
            height: Some(LengthSpec::Fill),
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            overflow_x: OverflowSpec::Scroll,
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

/// The labels of [`Workload::TableScrollX`], wrapped at `width`.
fn sheet_style(width: f32) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(width)),
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

fn column_style(opacity: Option<f32>, transform: Option<PaintTransform>) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(LayoutStyle {
            width: Some(LengthSpec::Fill),
            height: Some(LengthSpec::Fill),
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            opacity,
            transform,
            transform_origin: transform.is_some().then(TransformOrigin::default),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    }
}

fn color_target(gpu: &GpuContext, physical: [u32; 2]) -> GpuRenderTarget {
    let view = __framework::device(gpu)
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("nana-text-paint-benchmark target"),
            size: wgpu::Extent3d {
                width: physical[0],
                height: physical[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default());
    __framework::render_target(gpu, view, FORMAT, physical)
}

fn print_table(cells: &[Cell]) {
    println!(
        "{:<18} {:>7} {:>6} {:>8} {:>10} {:>10} {:>12} {:>10} {:>7} {:>10} {:>10} {:>10}",
        "workload",
        "labels",
        "Hz",
        "glyphs",
        "resolve/f",
        "rebuild/f",
        "inst B/f",
        "index B/f",
        "draws/f",
        "reshape/f",
        "flush p50",
        "batch p50"
    );
    for cell in cells {
        let get = |name: &str| cell.counters.get(name).copied().unwrap_or_default();
        println!(
            "{:<18} {:>7} {:>6} {:>8} {:>10.1} {:>10.1} {:>12.0} {:>10.0} {:>7.1} {:>10.0} {:>9.3}m {:>9.3}m",
            cell.workload,
            cell.labels,
            cell.frames,
            cell.live_glyphs,
            get("glyph_resolve_requests"),
            get("text_instance_rebuilds"),
            get("text_instance_upload_bytes"),
            get("text_index_upload_bytes"),
            get("text_pipeline_draws"),
            get("shape_cache_misses"),
            cell.flush_ms.p50,
            cell.batch_ms.p50,
        );
    }
}
