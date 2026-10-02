//! `NanaApplication::builder` publishes paths and starts diagnostics, and a
//! real document flush reaches the session log through the framework's own
//! instrumentation (Issue #227). One test: the diagnostics runtime and the
//! published paths are process-wide.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use nana_ui::diagnostics::nlog::{self, Entry, MetricSample};
use nana_ui::diagnostics::{DiagnosticsConfig, PersistMode, Severity};
use nana_ui::runtime::{
    DocumentId, LayoutViewport, MeasureTextShaper, RuntimeDocument, Stack, Text,
};
use nana_ui::{ApplicationIdentity, ApplicationPaths, NanaApplication};
use nana_ui_platform::{PathEnvironment, PathPlatform, RuntimeLayout};

#[test]
fn builder_session_records_framework_metrics_and_flushes_on_drop() {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("nana-app-diag-{nanos}"));
    let env = PathEnvironment {
        platform: PathPlatform::Linux,
        executable: home.join("opt/app/app"),
        vars: BTreeMap::from([("HOME".to_owned(), OsString::from(&home))]),
        android_files_dir: None,
        android_cache_dir: None,
        portable_marker: false,
    };
    let identity = ApplicationIdentity::new("dev.nanaui.diagtest", "Diag Test", "9.9.9");
    let paths = ApplicationPaths::resolve_with(&identity, &env).unwrap();
    assert_eq!(paths.layout(), RuntimeLayout::Installed);
    let logs: PathBuf = paths.logs().to_path_buf();

    let session = NanaApplication::builder(identity)
        .paths(paths)
        .diagnostics(DiagnosticsConfig {
            persist: PersistMode::All,
            min_severity: Severity::Debug,
            poll_interval: Duration::from_millis(5),
            panic_hook: false,
            ..DiagnosticsConfig::default()
        })
        .start();
    assert_eq!(
        NanaApplication::paths().map(|p| p.logs()),
        Some(logs.as_path())
    );
    assert!(session.diagnostics().is_some());

    let id = DocumentId::new(1).unwrap();
    let mut document = RuntimeDocument::new(id);
    let root = document
        .context_mut()
        .create_component(id, Stack::column(4.0))
        .unwrap();
    for label in ["one", "two", "three"] {
        let text = document
            .context_mut()
            .create_detached_component(id, Text::new(label))
            .unwrap();
        document.context_mut().append_child(root, text).unwrap();
    }
    let mut shaper = MeasureTextShaper;
    document
        .flush(LayoutViewport::new(320.0, 240.0), &mut shaper)
        .unwrap();
    drop(session);

    let log = std::fs::read_dir(&logs)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "nlog"))
        .expect("session log");
    let file = nlog::read_file(&log).unwrap();
    assert_eq!(file.header.app_id, "dev.nanaui.diagtest");
    assert_eq!(file.header.framework_version, env!("CARGO_PKG_VERSION"));
    assert!(
        file.header
            .extra
            .contains(&("layout".into(), "installed".into()))
    );
    let metric_names: Vec<&str> = file
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Metrics { samples, .. } => Some(samples.iter()),
            _ => None,
        })
        .flatten()
        .filter_map(|s: &MetricSample| file.metric_name(s.key()))
        .collect();
    for expected in [
        "runtime.flushes",
        "runtime.flush.cpu",
        "runtime.stage.layout",
        "layout.pass",
        "layout.invocations",
    ] {
        assert!(
            metric_names.contains(&expected),
            "{expected} missing from {metric_names:?}"
        );
    }
    assert!(
        matches!(file.entries.last(), Some(Entry::Marker { text, .. }) if text == "shutdown"),
        "the session drop did not flush a final marker"
    );
    let _ = std::fs::remove_dir_all(home);
}
