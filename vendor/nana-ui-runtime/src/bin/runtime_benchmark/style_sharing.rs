//! Paired diagnostic; construction is outside the measured system stages.
use super::*;

pub(super) fn run(document: DocumentId) {
    let mut cases = serde_json::Map::new();
    for nodes in [5_000, 10_000] {
        let mut samples: BTreeMap<&str, BTreeMap<&str, Vec<Duration>>> = BTreeMap::new();
        let mut paired_delta_ms = Vec::new();
        for iteration in 0..70 {
            let mut totals = [Duration::ZERO; 2];
            let order = if iteration % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            };
            for shared in order {
                let mut world = UiWorld::new();
                world.commit(tree_mutations(nodes, document)).unwrap();
                let work = world.take_system_work();
                let mode = if shared { "shared" } else { "unshared" };
                let total = Instant::now();
                let mut started = total;
                run_systems_with_style_resolver(
                    &mut world,
                    document,
                    &work,
                    |stage| {
                        let elapsed = started.elapsed();
                        if iteration >= 10 {
                            samples
                                .entry(mode)
                                .or_default()
                                .entry(stage)
                                .or_default()
                                .push(elapsed);
                        }
                        started = Instant::now();
                    },
                    |world, ids| {
                        if shared {
                            world.resolve_styles(ids).unwrap();
                        } else {
                            world.benchmark_resolve_styles_unshared(ids).unwrap();
                        }
                    },
                );
                let elapsed = total.elapsed();
                totals[usize::from(shared)] = elapsed;
                if iteration >= 10 {
                    samples
                        .entry(mode)
                        .or_default()
                        .entry("total")
                        .or_default()
                        .push(elapsed);
                }
            }
            if iteration >= 10 {
                paired_delta_ms
                    .push(totals[1].as_secs_f64() * 1000.0 - totals[0].as_secs_f64() * 1000.0);
            }
        }
        let modes = samples
            .into_iter()
            .map(|(mode, stages)| {
                (
                    mode,
                    stages
                        .into_iter()
                        .map(|(stage, values)| (stage, summarize(&values)))
                        .collect::<BTreeMap<_, _>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        cases.insert(nodes.to_string(), serde_json::json!({ "modes": modes, "paired_shared_minus_unshared_ms": paired_delta_ms }));
    }
    let report = serde_json::json!({
        "scope": "same-process alternating style-sharing control; initial systems only; no construction, GPU or presentation",
        "warmup_pairs": 10, "sample_pairs": 60, "cases": cases,
    });
    let json = serde_json::to_string_pretty(&report).unwrap();
    if let Some(path) = std::env::args().skip_while(|arg| arg != "--output").nth(1) {
        std::fs::write(path, json).unwrap();
    } else {
        println!("{json}");
    }
}
