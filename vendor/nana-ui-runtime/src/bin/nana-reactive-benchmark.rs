//! Declarative view layer against the raw imperative path: nodes made one
//! at a time with `create_detached_component` / `append_child` (one commit
//! each) and patched with `update_component`, and keyed `mount` for lists.
//!
//! Every scenario runs its variants interleaved, alternating which goes
//! first each round, and reports the minimum and median per operation. The
//! numbers cover the authoring layer and its commits, not layout or paint.
//!
//! `cargo run --release -p nana-ui-runtime --features benchmark --bin nana-reactive-benchmark`

use std::hint::black_box;
use std::time::{Duration, Instant};

use nana_ui_runtime::view::{Signal, button, column, each, row, signal, text};
use nana_ui_runtime::{AppContext, DocumentId, Entity, Stack, Text};

const ROUNDS: usize = 15;

fn document() -> DocumentId {
    DocumentId::new(1).unwrap()
}

struct Series {
    name: &'static str,
    samples: Vec<Duration>,
}

impl Series {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            samples: Vec::new(),
        }
    }

    fn report(&mut self, scenario: &str) {
        self.samples.sort();
        let min = self.samples[0];
        let median = self.samples[self.samples.len() / 2];
        println!(
            "{scenario:<34} {:<22} min {:>10.2} µs   median {:>10.2} µs",
            self.name,
            min.as_secs_f64() * 1e6,
            median.as_secs_f64() * 1e6
        );
    }
}

/// Run `variants` interleaved for [`ROUNDS`], rotating the first one.
fn interleave(scenario: &str, variants: &mut [(Series, &mut dyn FnMut() -> Duration)]) {
    for round in 0..ROUNDS {
        let count = variants.len();
        for offset in 0..count {
            let (series, run) = &mut variants[(round + offset) % count];
            let sample = run();
            series.samples.push(sample);
        }
    }
    for (series, _) in variants.iter_mut() {
        series.report(scenario);
    }
}

fn timed(f: impl FnOnce()) -> Duration {
    let started = Instant::now();
    f();
    started.elapsed()
}

fn mount(n: usize) {
    let scenario = format!("mount {n} text nodes");
    let mut raw = || {
        let mut cx = AppContext::new();
        timed(|| {
            black_box(raw_texts(&mut cx, n));
        })
    };
    let mut constant = || {
        let mut cx = AppContext::new();
        timed(|| {
            black_box(
                cx.mount_view_root(document(), || {
                    column().children((0..n).map(|i| text(format!("行 {i}"))).collect::<Vec<_>>())
                })
                .unwrap(),
            );
        })
    };
    let mut bound = || {
        let mut cx = AppContext::new();
        timed(|| {
            black_box(
                cx.mount_view_root(document(), || {
                    column().children(
                        (0..n)
                            .map(|i| text(signal(format!("行 {i}"))))
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap(),
            );
        })
    };
    interleave(
        &scenario,
        &mut [
            (Series::new("create + append"), &mut raw),
            (Series::new("mount_view constant"), &mut constant),
            (Series::new("mount_view bound"), &mut bound),
        ],
    );
}

/// Styled controls: every row holds its own layouts, unlike plain texts.
fn mount_controls(n: usize) {
    let scenario = format!("mount {n} button rows");
    let mut raw = || {
        let mut cx = AppContext::new();
        timed(|| {
            let root = cx.create_component(document(), Stack::column(0.0)).unwrap();
            for i in 0..n {
                let strip = cx
                    .create_detached_component(document(), Stack::row(8.0))
                    .unwrap();
                cx.append_child(root, strip).unwrap();
                let button = cx
                    .create_detached_component(
                        document(),
                        nana_ui_runtime::Button::new(format!("按钮 {i}")),
                    )
                    .unwrap();
                cx.append_child(root, button).unwrap();
            }
            black_box(&cx);
        })
    };
    let mut new = || {
        let mut cx = AppContext::new();
        timed(|| {
            black_box(
                cx.mount_view_root(document(), || {
                    column().children(
                        (0..n)
                            .map(|i| (row().gap(8.0).children(()), button(format!("按钮 {i}"))))
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap(),
            );
        })
    };
    interleave(
        &scenario,
        &mut [
            (Series::new("create + append"), &mut raw),
            (Series::new("mount_view"), &mut new),
        ],
    );
}

/// A column of `n` texts, one node and one commit at a time.
fn raw_texts(cx: &mut AppContext, n: usize) -> Vec<Entity<Text>> {
    let root = cx.create_component(document(), Stack::column(0.0)).unwrap();
    (0..n)
        .map(|i| {
            let text = cx
                .create_detached_component(document(), Text::new(format!("行 {i}")))
                .unwrap();
            cx.append_child(root, text).unwrap();
            text
        })
        .collect()
}

/// `n` texts made imperatively, for the `update_component` variant.
fn old_tree(n: usize) -> (AppContext, Vec<Entity<Text>>) {
    let mut cx = AppContext::new();
    let texts = raw_texts(&mut cx, n);
    (cx, texts)
}

/// `n` bound texts: each reads its own signal, or all read one.
fn new_tree(n: usize, shared: bool) -> (AppContext, Vec<Signal<String>>) {
    let mut cx = AppContext::new();
    let mut signals = Vec::new();
    cx.mount_view_root(document(), || {
        let one = signal(String::from("行"));
        column().children(
            (0..n)
                .map(|i| {
                    let source = if shared {
                        one
                    } else {
                        signal(format!("行 {i}"))
                    };
                    signals.push(source);
                    text(source)
                })
                .collect::<Vec<_>>(),
        )
    })
    .unwrap();
    (cx, signals)
}

fn updates(n: usize, per_op: usize) {
    const OPS: usize = 200;
    let scenario = format!("update {per_op} of {n} nodes");
    let (mut old_cx, old_texts) = old_tree(n);
    let (mut each_cx, each_signals) = new_tree(n, false);
    let mut tick = 0u64;
    let mut run_old = || {
        timed(|| {
            for _ in 0..OPS {
                tick += 1;
                for entity in &old_texts[..per_op] {
                    let value = format!("v{tick}");
                    old_cx
                        .update_component(*entity, |text, _| text.value = value)
                        .unwrap();
                }
            }
        }) / OPS as u32
    };
    let mut tick_each = 0u64;
    let mut run_each = || {
        timed(|| {
            for _ in 0..OPS {
                tick_each += 1;
                for source in &each_signals[..per_op] {
                    source.set(format!("v{tick_each}"));
                }
                each_cx.flush_reactive().unwrap();
            }
        }) / OPS as u32
    };
    if per_op == 1 {
        interleave(
            &scenario,
            &mut [
                (Series::new("update_component"), &mut run_old),
                (Series::new("set + flush"), &mut run_each),
            ],
        );
    } else {
        // Only the first `per_op` nodes change: in `new_each` through their
        // own signals, in `subset_cx` through one signal they all read.
        let (mut subset_cx, subset_signals) = new_tree(per_op, true);
        let mut tick_subset = 0u64;
        let mut run_subset = || {
            timed(|| {
                for _ in 0..OPS {
                    tick_subset += 1;
                    subset_signals[0].set(format!("v{tick_subset}"));
                    subset_cx.flush_reactive().unwrap();
                }
            }) / OPS as u32
        };
        interleave(
            &scenario,
            &mut [
                (Series::new("update_component"), &mut run_old),
                (Series::new("N signals, 1 flush"), &mut run_each),
                (Series::new("1 signal, N nodes"), &mut run_subset),
            ],
        );
    }
}

#[derive(Clone)]
struct Row {
    id: u32,
    title: Signal<String>,
}

fn list(n: usize) {
    const OPS: usize = 20;
    // Imperative: the list is a keyed `mount` rewritten on every change.
    let mut old_cx = AppContext::new();
    let old_container = old_cx
        .create_component(document(), Stack::column(0.0))
        .unwrap();
    let mut old_ids: Vec<u32> = (0..n as u32).collect();
    let remount = |cx: &mut AppContext, ids: &[u32], changed: Option<(u32, &str)>| {
        cx.mount(old_container, |scope| {
            for id in ids {
                let title = match changed {
                    Some((target, title)) if target == *id => title.to_owned(),
                    _ => format!("任务 {id}"),
                };
                scope.child(format!("r{id}"), Text::new(title))?;
            }
            Ok(())
        })
        .unwrap();
    };
    remount(&mut old_cx, &old_ids, None);

    let mut new_cx = AppContext::new();
    let rows = std::cell::Cell::new(None);
    let view = new_cx
        .mount_view_root(document(), || {
            let list = signal(
                (0..n as u32)
                    .map(|id| Row {
                        id,
                        title: signal(format!("任务 {id}")),
                    })
                    .collect::<Vec<_>>(),
            );
            rows.set(Some(list));
            each(list, |row| row.id, |row| text(row.title))
        })
        .unwrap();
    black_box(view.roots());
    let rows = rows.get().unwrap();
    let mut next = n as u32;
    let old_extra = u32::MAX;

    let scenario = format!("insert+remove 1 row of {n}");
    let mut run_old = || {
        timed(|| {
            for _ in 0..OPS {
                old_ids.insert(0, old_extra);
                remount(&mut old_cx, &old_ids, None);
                old_ids.remove(0);
                remount(&mut old_cx, &old_ids, None);
            }
        }) / (2 * OPS) as u32
    };
    let mut run_new = || {
        timed(|| {
            for _ in 0..OPS {
                next += 1;
                let title = signal(format!("任务 {next}"));
                rows.update(|list| list.insert(0, Row { id: next, title }));
                new_cx.flush_reactive().unwrap();
                rows.update(|list| {
                    list.remove(0);
                });
                new_cx.flush_reactive().unwrap();
            }
        }) / (2 * OPS) as u32
    };
    interleave(
        &scenario,
        &mut [
            (Series::new("mount rewrite"), &mut run_old),
            (Series::new("each"), &mut run_new),
        ],
    );

    let scenario = format!("edit 1 row field of {n}");
    let old_row = Entity::<Text>::from_stable_id(
        old_cx
            .assembled_child(old_container.stable_id(), "r57")
            .unwrap(),
    );
    let mut tick = 0u64;
    let mut run_targeted = || {
        timed(|| {
            for _ in 0..OPS {
                tick += 1;
                let value = format!("改 {tick}");
                old_cx
                    .update_component(old_row, |text, _| text.value = value)
                    .unwrap();
            }
        }) / OPS as u32
    };
    let mut tick_remount = 0u64;
    let mut run_remount = || {
        let mut local = AppContext::new();
        let container = local
            .create_component(document(), Stack::column(0.0))
            .unwrap();
        let ids: Vec<u32> = (0..n as u32).collect();
        let remount_local = |cx: &mut AppContext, changed: &str| {
            cx.mount(container, |scope| {
                for id in &ids {
                    let title = if *id == 57 {
                        changed.to_owned()
                    } else {
                        format!("任务 {id}")
                    };
                    scope.child(format!("r{id}"), Text::new(title))?;
                }
                Ok(())
            })
            .unwrap();
        };
        remount_local(&mut local, "起");
        timed(|| {
            for _ in 0..OPS {
                tick_remount += 1;
                remount_local(&mut local, &format!("改 {tick_remount}"));
            }
        }) / OPS as u32
    };
    let row = rows.with_untracked(|list| list[57].title);
    let mut tick_new = 0u64;
    let mut run_row_signal = || {
        timed(|| {
            for _ in 0..OPS {
                tick_new += 1;
                row.set(format!("改 {tick_new}"));
                new_cx.flush_reactive().unwrap();
            }
        }) / OPS as u32
    };
    interleave(
        &scenario,
        &mut [
            (Series::new("update_component"), &mut run_targeted),
            (Series::new("mount rewrite"), &mut run_remount),
            (Series::new("row signal"), &mut run_row_signal),
        ],
    );
}

/// Bindings that re-run and produce the value the node already has: typing
/// into a draft that stays non-empty, re-setting an equal label.
fn rerun_unchanged(n: usize) {
    const OPS: usize = 200;
    let mut cx = AppContext::new();
    let sources = std::cell::Cell::new(None);
    cx.mount_view_root(document(), || {
        let draft = signal(String::from("x"));
        let label = signal(String::from("保存"));
        sources.set(Some((draft, label)));
        column().children(
            (0..n)
                .map(|_| {
                    button(label)
                        .disabled(move || draft.with(|d| d.is_empty()))
                        .loading(false)
                })
                .collect::<Vec<_>>(),
        )
    })
    .unwrap();
    let (draft, label) = sources.get().unwrap();
    let mut run = || {
        timed(|| {
            for _ in 0..OPS {
                draft.update(|d| d.push('y'));
                label.set(String::from("保存"));
                cx.flush_reactive().unwrap();
            }
        }) / OPS as u32
    };
    interleave(
        &format!("rerun {n} bound buttons, no change"),
        &mut [(Series::new("set + flush"), &mut run)],
    );
}

/// Styled controls whose bound field really changes: the path that copies
/// and projects the whole component.
fn button_labels(n: usize) {
    const OPS: usize = 200;
    let scenario = format!("relabel {n} buttons");
    let mut raw_cx = AppContext::new();
    let root = raw_cx
        .create_component(document(), Stack::column(0.0))
        .unwrap();
    let buttons = (0..n)
        .map(|i| {
            let button = raw_cx
                .create_detached_component(
                    document(),
                    nana_ui_runtime::Button::new(format!("按钮 {i}")),
                )
                .unwrap();
            raw_cx.append_child(root, button).unwrap();
            button
        })
        .collect::<Vec<_>>();
    let mut tick = 0u64;
    let mut run_raw = || {
        timed(|| {
            for _ in 0..OPS {
                tick += 1;
                for button in &buttons {
                    let label = format!("v{tick}");
                    raw_cx
                        .update_component(*button, |button, _| button.label = label)
                        .unwrap();
                }
            }
        }) / OPS as u32
    };
    let mut view_cx = AppContext::new();
    let mut labels = Vec::new();
    view_cx
        .mount_view_root(document(), || {
            column().children(
                (0..n)
                    .map(|i| {
                        let label = signal(format!("按钮 {i}"));
                        labels.push(label);
                        button(label)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap();
    let mut tick_view = 0u64;
    let mut run_view = || {
        timed(|| {
            for _ in 0..OPS {
                tick_view += 1;
                for label in &labels {
                    label.set(format!("v{tick_view}"));
                }
                view_cx.flush_reactive().unwrap();
            }
        }) / OPS as u32
    };
    interleave(
        &scenario,
        &mut [
            (Series::new("update_component"), &mut run_raw),
            (Series::new("set + flush"), &mut run_view),
        ],
    );
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn main() {
    // `NANA_BENCH=relabel` runs one scenario, for a profiler to sample.
    // `NANA_BENCH=updates` runs only the update scenarios, for A/B runs.
    match std::env::var("NANA_BENCH").as_deref() {
        Ok("relabel") => loop {
            button_labels(100);
        },
        Ok("updates") => {
            updates(5_000, 1);
            updates(5_000, 100);
            button_labels(100);
            return;
        }
        _ => {}
    }
    println!("load before: {}", load());
    mount(1_000);
    mount(5_000);
    mount_controls(1_000);
    updates(5_000, 1);
    updates(5_000, 100);
    list(2_000);
    rerun_unchanged(100);
    button_labels(100);
    println!("load after:  {}", load());
}
