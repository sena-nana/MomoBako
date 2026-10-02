//! Synthetic semantic projection diagnostic; excludes Runtime layout and native UIA.
use nana_ui::AccessTreeProjector;
use nana_ui_runtime::{
    AccessibilityDelta, AccessibilityNode, AccessibilityRole, LayoutBox, StableNodeId,
};
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn node(value: u64) -> AccessibilityNode {
    AccessibilityNode {
        id: StableNodeId::new(value).unwrap(),
        parent: (value != 1).then(|| StableNodeId::new(1).unwrap()),
        children: Vec::new(),
        role: AccessibilityRole::Generic,
        label: None,
        value: None,
        description: None,
        disabled: false,
        checked: None,
        mixed: false,
        orientation: None,
        selected: None,
        multiline: false,
        editable: false,
        selection: None,
        modal: false,
        busy: false,
        invalid: false,
        numeric_minimum: None,
        numeric_maximum: None,
        numeric_step: None,
        numeric_value: None,
        focused: false,
        bounds: LayoutBox::default(),
        writing: Default::default(),
    }
}

fn main() {
    const WARMUP: usize = 100;
    let samples = std::env::args()
        .nth(1)
        .map_or(2_000, |value| value.parse::<usize>().expect("sample count"));
    assert!(samples > 0);
    let mut rows = Vec::new();
    for retained in [10_000_u64, 50_000, 100_000] {
        let mut nodes = (1..=retained).map(node).collect::<Vec<_>>();
        nodes[0].children = (2..=retained)
            .map(|id| StableNodeId::new(id).unwrap())
            .collect();
        // Updating the last sibling detects accidental linear parent-child searches.
        let editor = nodes.last_mut().unwrap();
        editor.role = AccessibilityRole::TextInput;
        editor.editable = true;
        let mut editor = editor.clone();
        let mut projector = AccessTreeProjector::new(nodes, true, 1.0);
        let mut times = Vec::with_capacity(samples);
        for iteration in 0..WARMUP + samples {
            editor.value = Some(if iteration % 2 == 0 { "Even" } else { "Odd" }.into());
            editor.focused = iteration % 2 == 0;
            let delta = AccessibilityDelta {
                generation: iteration as u64 + 1,
                updated: vec![editor.clone()],
                removed: Vec::new(),
            };
            let start = Instant::now();
            let update = projector.apply_delta(delta).unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1_000_000.0;
            assert_eq!(update.nodes.len(), 2);
            assert!(update.tree.is_none());
            black_box(&update);
            if iteration >= WARMUP {
                times.push(elapsed);
            }
        }
        times.sort_by(f64::total_cmp);
        let percentile = |percent: usize| times[(times.len() * percent).div_ceil(100) - 1];
        rows.push(json!({
            "retained_semantic_nodes": retained, "emitted_nodes_per_delta": 2,
            "p50_us": percentile(50), "p95_us": percentile(95), "p99_us": percentile(99),
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope": "AccessKit semantic delta only; not a frame or presentation benchmark",
            "warmup": WARMUP, "samples_per_size": samples, "rows": rows,
        }))
        .unwrap()
    );
}
