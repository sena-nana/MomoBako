//! Issue #101 §4 theme / style baseline.
//!
//! Records what a theme or style change makes the retained pipeline do, for
//! the workloads Issue #101 names: static idle, 1 / 100 / 1k / 10k controls,
//! single-node hover and focus, light↔dark, accent-only, density, and a style
//! mutation at the head of a large tree.
//!
//! The gate is the work count, not the wall clock — `elapsed_ms` is a host
//! observation kept for context, like `steady_ms` on the compositor rows.
//! Numbers here are a Phase 0 **baseline**: they describe the pipeline as it
//! is today, including the places Issue #100 will narrow. They are not a
//! target, and nothing here changes what the product paints.

use std::time::{Duration, Instant};

use nana_ui_runtime::{
    AppContext, Button, DocumentId, MutationQueue, NodeKind, NodeStyle, StableNodeId, Stack,
    SystemWork, ThemeWorkCounters, UiWorld, WorkCounters,
};
use serde::Serialize;

const WARMUP: usize = 3;
const ITERATIONS: usize = 12;
const LARGE_WARMUP: usize = 1;
const LARGE_ITERATIONS: usize = 3;
/// Controls in every workload that is not itself a control-scale row.
const FIXTURE_CONTROLS: usize = 1_000;
/// Host ticks used to prove a settled world schedules no further frame.
const IDLE_OBSERVE_TICKS: usize = 8;

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    profile: &'static str,
    catalog_theme: CatalogTheme,
    notes: Vec<&'static str>,
}

#[derive(Serialize)]
struct CatalogTheme {
    cases: Vec<Case>,
}

#[derive(Serialize)]
struct Case {
    id: &'static str,
    workload: &'static str,
    status: &'static str,
    /// Live controls in the fixture this case measured.
    controls: usize,
    /// Retained nodes those controls projected to.
    nodes: usize,
    iterations: usize,
    work: ThemeWork,
    /// The #8 frame counters of the same window, so the theme attribution can
    /// be checked against what the frame actually scheduled.
    frame_work: FrameWork,
    elapsed_ms: Distribution,
}

/// [`ThemeWorkCounters`] as reported. Every field is one the pass measures, so
/// none is optional.
#[derive(Serialize, Clone, Copy, Default)]
struct ThemeWork {
    style_nodes_considered: usize,
    style_nodes_resolved: usize,
    style_nodes_skipped: usize,
    theme_reads: usize,
    style_allocations: usize,
    style_allocated_bytes: usize,
    layout_copies: usize,
    layout_copied_bytes: usize,
    layout_nodes_from_style: usize,
    text_nodes_from_style: usize,
    paint_nodes_from_style: usize,
}

impl From<ThemeWorkCounters> for ThemeWork {
    fn from(counters: ThemeWorkCounters) -> Self {
        Self {
            style_nodes_considered: counters.style_nodes_considered,
            style_nodes_resolved: counters.style_nodes_resolved,
            style_nodes_skipped: counters.style_nodes_skipped,
            theme_reads: counters.theme_reads,
            style_allocations: counters.style_allocations,
            style_allocated_bytes: counters.style_allocated_bytes,
            layout_copies: counters.layout_copies,
            layout_copied_bytes: counters.layout_copied_bytes,
            layout_nodes_from_style: counters.layout_nodes_from_style,
            text_nodes_from_style: counters.text_nodes_from_style,
            paint_nodes_from_style: counters.paint_nodes_from_style,
        }
    }
}

/// The subset of [`WorkCounters`] a theme change can move, taken from the
/// drain inside the measured window ([`SystemWork::counters`]) rather than
/// from `last_work_counters`: an idle frame deliberately does not replace
/// that snapshot, so the idle row would otherwise report the numbers of the
/// last frame that did something. GPU and cache fields are omitted rather
/// than reported as a fake zero — this binary paints nothing.
#[derive(Serialize, Clone, Copy, Default)]
struct FrameWork {
    entities_total: usize,
    style_processed: usize,
    text_shaped: usize,
    layout_nodes: usize,
    render_nodes_changed: usize,
    allocations: usize,
    allocated_bytes: usize,
}

impl From<WorkCounters> for FrameWork {
    fn from(counters: WorkCounters) -> Self {
        Self {
            entities_total: counters.entities_total,
            style_processed: counters.style_processed,
            text_shaped: counters.text_shaped,
            layout_nodes: counters.layout_nodes,
            render_nodes_changed: counters.render_nodes_changed,
            allocations: counters.allocations,
            allocated_bytes: counters.allocated_bytes,
        }
    }
}

#[derive(Serialize, Clone, Copy, Default)]
struct Distribution {
    p50: f64,
    p95: f64,
    max: f64,
}

fn main() {
    let cases = vec![
        control_scale_case("theme-controls-1", 1),
        control_scale_case("theme-controls-100", 100),
        control_scale_case("theme-controls-1k", 1_000),
        control_scale_case("theme-controls-10k", 10_000),
        static_idle_case(),
        hover_case(),
        focus_case(),
        palette_switch_case(),
        accent_only_case(),
        density_case(),
        head_mutation_case(),
    ];
    let report = Report {
        schema_version: 1,
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        catalog_theme: CatalogTheme { cases },
        notes: vec![
            "Generated by: cargo run --release --locked -p nana-ui-runtime --features benchmark --bin nana-theme-benchmark -- --output target/performance/issue101/theme.json",
            "Issue #101 §4 baseline. work counts are the contract; elapsed_ms is a host observation.",
            "Controls are product Buttons through AppContext, not bare divs: a Button is what carries SemanticPaint and InteractionStyle.",
        ],
    };
    write_report(&report);
}

/// A settled fixture of `controls` Buttons and the style pass that built it.
///
/// This is the cost of resolving a document from nothing, which is the
/// scale row Issue #101 asks for at 1 / 100 / 1k / 10k.
fn control_scale_case(id: &'static str, controls: usize) -> Case {
    let (warmup, iterations) = iteration_budget(controls);
    let mut samples = Vec::with_capacity(iterations);
    let mut work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    let mut nodes = 0;
    for iteration in 0..(warmup + iterations) {
        let Fixture {
            mut context,
            document,
            ..
        } = control_fixture(controls);
        context.compat_world_mut().begin_frame_counters();
        let started = Instant::now();
        let scheduled = context.take_system_work();
        run_systems(&mut context, document, &scheduled);
        let elapsed = started.elapsed();
        context.compat_world_mut().end_frame_counters();
        if iteration >= warmup {
            samples.push(elapsed);
            work = context.world().last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
            nodes = context.world().len();
        }
    }
    Case {
        id,
        workload: "controls",
        status: "ok",
        controls,
        nodes,
        iterations,
        work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

/// A settled document that nothing touched. Issue #100 §15: the steady frame
/// must not walk the theme.
fn static_idle_case() -> Case {
    let Fixture {
        mut context,
        document,
        ..
    } = settled_fixture(FIXTURE_CONTROLS);
    let nodes = context.world().len();
    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    for iteration in 0..(WARMUP + ITERATIONS) {
        context.compat_world_mut().begin_frame_counters();
        let started = Instant::now();
        let scheduled = context.take_system_work();
        assert!(
            scheduled.is_empty(),
            "a settled document must schedule no work"
        );
        run_systems(&mut context, document, &scheduled);
        let elapsed = started.elapsed();
        context.compat_world_mut().end_frame_counters();
        if iteration >= WARMUP {
            samples.push(elapsed);
            work = context.world().last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
        }
    }
    assert_eq!(
        context
            .compat_world_mut()
            .scheduled_ui_frames(IDLE_OBSERVE_TICKS),
        0,
        "a settled document must not keep asking for frames"
    );
    Case {
        id: "theme-static-idle",
        workload: "idle",
        status: "ok",
        controls: FIXTURE_CONTROLS,
        nodes,
        iterations: ITERATIONS,
        work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

/// Hovering one control in a 1k-control document. Issue #100 §15: this must
/// not become a document-wide theme resolve.
fn hover_case() -> Case {
    let fixture = settled_fixture(FIXTURE_CONTROLS);
    let target = fixture.target();
    let Fixture {
        mut context,
        document,
        ..
    } = fixture;
    let nodes = context.world().len();
    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    for iteration in 0..(WARMUP + ITERATIONS) {
        // Leave the node before measuring, so every measured pass is the same
        // enter and not an already-hovered no-op.
        context.set_pointer_hover(document, 1, None).unwrap();
        let _ = context.take_system_work();
        context.compat_world_mut().begin_frame_counters();
        let started = Instant::now();
        context
            .set_pointer_hover(document, 1, Some(target))
            .unwrap();
        let scheduled = context.take_system_work();
        run_systems(&mut context, document, &scheduled);
        let elapsed = started.elapsed();
        context.compat_world_mut().end_frame_counters();
        if iteration >= WARMUP {
            samples.push(elapsed);
            work = context.world().last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
        }
    }
    Case {
        id: "theme-hover-one",
        workload: "hover",
        status: "ok",
        controls: FIXTURE_CONTROLS,
        nodes,
        iterations: ITERATIONS,
        work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

/// Focusing one control in a 1k-control document.
fn focus_case() -> Case {
    let fixture = settled_fixture(FIXTURE_CONTROLS);
    let target = fixture.target();
    let Fixture {
        mut context,
        document,
        ..
    } = fixture;
    let nodes = context.world().len();
    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    for iteration in 0..(WARMUP + ITERATIONS) {
        context.clear_focus(document).unwrap();
        let _ = context.take_system_work();
        context.compat_world_mut().begin_frame_counters();
        let started = Instant::now();
        context.focus_node(document, target).unwrap();
        let scheduled = context.take_system_work();
        run_systems(&mut context, document, &scheduled);
        let elapsed = started.elapsed();
        context.compat_world_mut().end_frame_counters();
        if iteration >= WARMUP {
            samples.push(elapsed);
            work = context.world().last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
        }
    }
    Case {
        id: "theme-focus-one",
        workload: "focus",
        status: "ok",
        controls: FIXTURE_CONTROLS,
        nodes,
        iterations: ITERATIONS,
        work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

/// Light↔dark. Only the palette moves, so Layout and Text must stay at zero.
fn palette_switch_case() -> Case {
    theme_install_case(
        "theme-palette-switch",
        "palette-switch",
        |context, index| {
            let mode = if index.is_multiple_of(2) {
                nana_ui_core::ThemeMode::Light
            } else {
                nana_ui_core::ThemeMode::Dark
            };
            assert!(context.set_theme(mode).unwrap(), "the mode must change");
        },
    )
}

/// One accent role, nothing else. The document-wide invalidation this records
/// is the Phase 0 number Issue #100 §7 narrows.
fn accent_only_case() -> Case {
    theme_install_case("theme-accent-only", "accent-only", |context, index| {
        let mut palette = nana_ui_core::SemanticPalette::dark();
        // The even branch must differ from the settled fixture's own dark
        // accent, or the first install would be a no-op and the row would
        // measure nothing.
        if index.is_multiple_of(2) {
            palette.accent = nana_ui_core::SemanticColor::rgb8(240, 145, 123);
        }
        assert!(
            context
                .set_style_tokens(
                    nana_ui_core::ThemeMode::Dark,
                    nana_ui_core::UI_METRICS,
                    palette,
                    palette.surface,
                )
                .unwrap(),
            "the accent must change"
        );
    })
}

/// Control metrics only. This one is expected to reach Layout.
fn density_case() -> Case {
    theme_install_case("theme-density", "density", |context, index| {
        let mut metrics = nana_ui_core::UI_METRICS;
        if index.is_multiple_of(2) {
            metrics.control_height += 4.0;
            metrics.compact_control_height += 4.0;
            metrics.selection_height += 4.0;
        }
        assert!(
            context
                .set_style_tokens(
                    nana_ui_core::ThemeMode::Dark,
                    metrics,
                    nana_ui_core::SemanticPalette::dark(),
                    nana_ui_core::SemanticPalette::dark().surface,
                )
                .unwrap(),
            "the metrics must change"
        );
    })
}

fn theme_install_case(
    id: &'static str,
    workload: &'static str,
    mut install: impl FnMut(&mut AppContext, usize),
) -> Case {
    let Fixture {
        mut context,
        document,
        ..
    } = settled_fixture(FIXTURE_CONTROLS);
    let nodes = context.world().len();
    let mut samples = Vec::with_capacity(ITERATIONS);
    let mut work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    for iteration in 0..(WARMUP + ITERATIONS) {
        context.compat_world_mut().begin_frame_counters();
        let started = Instant::now();
        install(&mut context, iteration);
        let scheduled = context.take_system_work();
        run_systems(&mut context, document, &scheduled);
        let elapsed = started.elapsed();
        context.compat_world_mut().end_frame_counters();
        if iteration >= WARMUP {
            samples.push(elapsed);
            work = context.world().last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
        }
    }
    Case {
        id,
        workload,
        status: "ok",
        controls: FIXTURE_CONTROLS,
        nodes,
        iterations: ITERATIONS,
        work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

/// A style change at the head of a large tree: the subtree scope Issue #101
/// asks for, before `ThemeScope` exists to express it. Style resolution
/// inherits downward, so this is what a scoped patch costs today.
fn head_mutation_case() -> Case {
    let document = DocumentId::new(7).unwrap();
    let mut world = UiWorld::new();
    world.commit(head_tree(10_000, document)).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let _ = world.extract_nodes(&work.render_extraction);
    let nodes = world.len();
    let mut samples = Vec::with_capacity(LARGE_ITERATIONS);
    let mut theme_work = ThemeWork::default();
    let mut frame_work = FrameWork::default();
    for iteration in 0..(LARGE_WARMUP + LARGE_ITERATIONS) {
        world.begin_frame_counters();
        let started = Instant::now();
        let mut queue = MutationQueue::new();
        queue.set_style(
            StableNodeId::new(1).unwrap(),
            head_style(iteration.is_multiple_of(2)),
        );
        world.commit(queue).unwrap();
        let scheduled = world.take_system_work();
        world.resolve_styles(&scheduled.style).unwrap();
        let _ = world.extract_nodes(&scheduled.render_extraction);
        let elapsed = started.elapsed();
        world.end_frame_counters();
        if iteration >= LARGE_WARMUP {
            samples.push(elapsed);
            theme_work = world.last_theme_work_counters().into();
            frame_work = scheduled.counters().into();
        }
    }
    Case {
        id: "theme-head-style-mutation",
        workload: "head-scope",
        status: "ok",
        controls: 0,
        nodes,
        iterations: LARGE_ITERATIONS,
        work: theme_work,
        frame_work,
        elapsed_ms: summarize(&samples),
    }
}

fn iteration_budget(controls: usize) -> (usize, usize) {
    if controls >= 10_000 {
        (LARGE_WARMUP, LARGE_ITERATIONS)
    } else {
        (WARMUP, ITERATIONS)
    }
}

/// `controls` product Buttons under one column, not yet resolved. The last
/// control's id comes back with it: the hover and focus rows drive the node
/// furthest from the root, so neither pass can look cheap by sitting on it.
fn control_fixture(controls: usize) -> Fixture {
    let document = DocumentId::new(1).unwrap();
    let mut context = AppContext::new();
    let root = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let mut last = None;
    for index in 0..controls {
        let child = context
            .create_component(document, Button::new(format!("Action {index}")))
            .unwrap();
        context.append_child(root, child).unwrap();
        last = Some(child.stable_id());
    }
    Fixture {
        context,
        document,
        last_control: last,
    }
}

/// The same fixture with its first style, layout and extract pass already run.
fn settled_fixture(controls: usize) -> Fixture {
    let mut fixture = control_fixture(controls);
    // Components settle over more than one drain (mount, then projection).
    // Keep draining until the document is quiet, or the "idle" row would be
    // measuring the tail of construction.
    for _ in 0..16 {
        let work = fixture.context.take_system_work();
        if work.is_empty() {
            break;
        }
        run_systems(&mut fixture.context, fixture.document, &work);
    }
    fixture
}

struct Fixture {
    context: AppContext,
    document: DocumentId,
    last_control: Option<StableNodeId>,
}

impl Fixture {
    fn target(&self) -> StableNodeId {
        self.last_control.expect("the fixture has a control")
    }
}

fn head_tree(nodes: usize, document: DocumentId) -> MutationQueue {
    let mut queue = MutationQueue::new();
    for index in 1..=nodes {
        let id = StableNodeId::new(index as u64).unwrap();
        queue.create(id, document, NodeKind::Element { tag: "div".into() });
        if index > 1 {
            queue.insert(StableNodeId::new((index / 2) as u64).unwrap(), id, None);
        }
    }
    queue
}

/// An inherited paint change at the head: every descendant re-resolves.
fn head_style(alternate: bool) -> NodeStyle {
    NodeStyle {
        foreground: Some(if alternate {
            nana_ui_core::SemanticColorRole::Accent
        } else {
            nana_ui_core::SemanticColorRole::Muted
        }),
        ..NodeStyle::default()
    }
}

fn run_systems(context: &mut AppContext, document: DocumentId, work: &SystemWork) {
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context.compat_world_mut().reconcile_focus(&work.focus_ime);
    let _ = context
        .compat_world_mut()
        .project_accessibility_nodes(&work.accessibility);
    let _ = context
        .compat_world_mut()
        .layout_inputs(&work.layout)
        .unwrap();
    if !work.input_hit_test.is_empty()
        && !context
            .compat_world_mut()
            .rebuild_hit_test_scoped(document, &work.input_hit_test)
    {
        context.compat_world_mut().rebuild_hit_test(document);
    }
    let _ = context
        .compat_world_mut()
        .extract_nodes(&work.render_extraction)
        .len();
}

fn summarize(samples: &[Duration]) -> Distribution {
    if samples.is_empty() {
        return Distribution::default();
    }
    let mut millis = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    millis.sort_by(f64::total_cmp);
    let at = |q: f64| {
        let index = ((millis.len() - 1) as f64 * q).round() as usize;
        millis[index]
    };
    Distribution {
        p50: at(0.50),
        p95: at(0.95),
        max: *millis.last().expect("non-empty"),
    }
}

fn write_report(report: &Report) {
    let json = serde_json::to_string_pretty(report).expect("benchmark report must serialize");
    let mut arguments = std::env::args_os().skip(1);
    match arguments.next() {
        None => println!("{json}"),
        Some(flag) if flag == "--output" => {
            let path = std::path::PathBuf::from(
                arguments
                    .next()
                    .expect("--output requires a destination path"),
            );
            assert!(
                arguments.next().is_none(),
                "unexpected arguments after --output destination"
            );
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .expect("benchmark output directory must be writable");
            }
            std::fs::write(&path, format!("{json}\n"))
                .expect("benchmark report destination must be writable");
            println!("{}", path.display());
        }
        Some(argument) => panic!(
            "unsupported argument `{}`; expected --output <path>",
            argument.to_string_lossy()
        ),
    }
}
