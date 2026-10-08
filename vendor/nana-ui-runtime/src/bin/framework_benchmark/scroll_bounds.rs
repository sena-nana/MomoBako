//! CPU-only diagnostic for the canonical layout content index, not a Surface
//! or end-to-end UI benchmark. Sleeping between samples bounds sample storage.
use nana_ui_runtime::{
    DocumentId, LayoutBox, MutationQueue, NodeKind, ScrollOffset, StableNodeId, UiWorld,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn id(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

fn distribution(samples: &mut [f64]) -> Value {
    samples.sort_unstable_by(f64::total_cmp);
    let percentile = |p: f64| {
        samples[((samples.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(samples.len() - 1)]
    };
    json!({"p50": percentile(0.5), "p95": percentile(0.95), "p99": percentile(0.99), "max": samples.last()})
}

fn measure(nodes: u64, duration: Duration) -> Value {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let mut mutations = MutationQueue::new();
    mutations.create(id(1), document, NodeKind::Document);
    for value in 2..=nodes {
        mutations.create(id(value), document, NodeKind::Text);
        mutations.insert(id(1), id(value), None);
        mutations.write_layout(
            id(value),
            LayoutBox {
                x: 0.0,
                y: (value - 2) as f32 * 10.0,
                width: 10.0,
                height: 10.0,
            },
        );
    }
    world.commit(mutations).unwrap();
    world.take_system_work();
    let started = Instant::now();
    let (_, initial_work) = world.benchmark_scroll_content_extent(id(1));
    let initial_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut query = [Vec::new(), Vec::new()];
    let mut commit = [Vec::new(), Vec::new()];
    let mut max_work = [(0, 0); 2];
    let mut iteration = 0usize;
    let mut large = false;
    let mut measured_elapsed = Duration::ZERO;
    for (record, window) in [(false, Duration::from_secs(2)), (true, duration)] {
        let started = Instant::now();
        while started.elapsed() < window {
            let kind = iteration % 2;
            let mut mutations = MutationQueue::new();
            if kind == 0 {
                large = !large;
                mutations.write_layout(
                    id(nodes),
                    LayoutBox {
                        x: 0.0,
                        y: if large {
                            (nodes - 2) as f32 * 10.0
                        } else {
                            0.0
                        },
                        width: 10.0,
                        height: 10.0,
                    },
                );
            } else {
                mutations.set_scroll_offset(
                    id(1),
                    ScrollOffset {
                        x: 0.0,
                        y: iteration as f32,
                    },
                );
            }
            let begin_commit = Instant::now();
            world.commit(mutations).unwrap();
            let commit_ms = begin_commit.elapsed().as_secs_f64() * 1000.0;
            let begin_query = Instant::now();
            let ((_, bottom), work) = world.benchmark_scroll_content_extent(id(1));
            let query_ms = begin_query.elapsed().as_secs_f64() * 1000.0;
            let expected = if large { nodes - 1 } else { nodes - 2 } as f32 * 10.0;
            assert_eq!(bottom, expected);
            assert!(work.0 <= if kind == 0 { 2 } else { 0 });
            if record {
                query[kind].push(query_ms);
                commit[kind].push(commit_ms);
                max_work[kind].0 = max_work[kind].0.max(work.0);
                max_work[kind].1 = max_work[kind].1.max(work.1);
            }
            world.take_system_work();
            world.take_scroll_hit_updates();
            iteration += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        if record {
            measured_elapsed = started.elapsed();
        }
    }
    let samples = [query[0].len(), query[1].len()];
    json!({
        "retained_nodes": nodes, "sample_seconds": measured_elapsed.as_secs_f64(),
        "initial_index_ms": initial_ms, "initial_refreshed_nodes": initial_work.0,
        "geometry": {"samples": samples[0], "commit_ms": distribution(&mut commit[0]), "query_ms": distribution(&mut query[0]), "max_refreshed_nodes": max_work[0].0, "max_range_updates": max_work[0].1},
        "scroll_only": {"samples": samples[1], "commit_ms": distribution(&mut commit[1]), "query_ms": distribution(&mut query[1]), "max_refreshed_nodes": max_work[1].0, "max_range_updates": max_work[1].1}
    })
}

pub fn run() {
    let mut seconds = 60.0f64;
    let mut nodes = vec![10_000, 50_000, 100_000];
    let mut output = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--profile-scroll-bounds" => {}
            "--seconds" => {
                seconds = arguments
                    .next()
                    .expect("seconds value")
                    .parse()
                    .expect("valid seconds")
            }
            "--nodes" => {
                nodes = vec![
                    arguments
                        .next()
                        .expect("nodes value")
                        .parse()
                        .expect("valid nodes"),
                ]
            }
            "--output" => output = Some(arguments.next().expect("output path")),
            _ => panic!("unknown argument: {argument}"),
        }
    }
    assert!(seconds.is_finite() && seconds >= 0.1);
    assert!(nodes.iter().all(|count| *count >= 3));
    let cases = nodes
        .into_iter()
        .map(|nodes| {
            let result = measure(nodes, Duration::from_secs_f64(seconds));
            eprintln!("scroll bounds: {nodes} nodes complete");
            result
        })
        .collect::<Vec<_>>();
    let report = json!({"scope": "CPU layout content index; no shaping, Scene, GPU or presentation", "warmup_seconds_per_case": 2, "sample_pause_ms": 1, "work_counters_enabled": true, "drains_scroll_hit_updates": true, "cases": cases});
    let json = serde_json::to_string_pretty(&report).unwrap();
    if let Some(path) = output {
        std::fs::write(path, json).unwrap();
    } else {
        println!("{json}");
    }
}
