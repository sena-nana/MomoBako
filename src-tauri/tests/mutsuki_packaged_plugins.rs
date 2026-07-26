//! 对真实 `.momoplug` release 产物执行 ABI v2 加载与目标 binding 路由验收。

use mutsuki_tauri_bridge::{FrontendContext, FrontendTaskRequest};
use mutsuki_tauri_host::{MutsukiTauriConfig, PathsConfig, PluginSelection};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const PLUGINS: [(&str, &str, &str); 9] = [
    (
        "local-filesystem-1.0.0.momoplug",
        "momobako.local-filesystem",
        "momobako.filesystem.statEntry",
    ),
    (
        "office-convert-0.1.0.momoplug",
        "momobako.service.office-convert",
        "momobako.officeConvert.getRuntimeStatus",
    ),
    (
        "parser-asmr-folder-0.1.0.momoplug",
        "momobako.parser.asmr-folder",
        "momobako.metadata.defaults.batch",
    ),
    (
        "service-archive-preview-0.1.0.momoplug",
        "momobako.service.archive-preview",
        "momobako.archive.ensurePrepared",
    ),
    (
        "service-downloader-0.1.0.momoplug",
        "momobako.service.downloader",
        "momobako.downloader.getRuntimeStatus",
    ),
    (
        "service-provider-asmr-one-0.1.0.momoplug",
        "momobako.service.provider.asmr-one",
        "momobako.provider.lookupMetadataCandidate",
    ),
    (
        "service-provider-dlsite-0.1.0.momoplug",
        "momobako.service.provider.dlsite",
        "momobako.provider.lookupMetadataCandidate",
    ),
    (
        "source-eagle-library-0.1.0.momoplug",
        "momobako.source.eagle-library",
        "momobako.filesystem.statEntry",
    ),
    (
        "source-netease-cloud-music-0.1.0.momoplug",
        "momobako.source.netease-cloud-music",
        "momobako.filesystem.statEntry",
    ),
];

/// 该验收依赖先执行 `yarn plugins:build && yarn plugins:package`。
#[test]
#[ignore = "requires packaged release plugins"]
fn official_packages_load_and_route_through_abi_v2() {
    let workspace = TestWorkspace::new();
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../External/Plugins/.packages");
    for (archive, _, _) in PLUGINS {
        fs::copy(package_root.join(archive), workspace.plugins.join(archive))
            .unwrap_or_else(|error| panic!("copy {archive}: {error}"));
    }

    let enabled = PLUGINS
        .iter()
        .map(|(_, plugin_id, _)| (*plugin_id).to_string())
        .collect::<BTreeSet<_>>();
    let configs = PLUGINS
        .iter()
        .map(|(_, plugin_id, _)| {
            let data_dir = workspace.root.join("plugin-data").join(plugin_id);
            fs::create_dir_all(&data_dir).expect("plugin data directory");
            (
                (*plugin_id).to_string(),
                json!({
                    "pluginId": plugin_id,
                    "pluginDataDir": data_dir,
                    "serviceRootDir": workspace.root,
                    "pluginConfig": {},
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut config = MutsukiTauriConfig::for_app("MomoBakoPackageTest");
    config.paths = workspace.paths();
    config.plugin_selection = PluginSelection {
        enabled_plugin_ids: Some(enabled),
        configs,
    };
    let host = mutsuki_tauri_host::MutsukiTauriHost::builder()
        .config(config)
        .build()
        .expect("packaged host should start");

    let summaries = host.plugins();
    let packages = host.plugin_packages();
    let package_diagnostics = packages
        .iter()
        .map(|package| format!("{}={:?}", package.plugin_id, package.error))
        .collect::<Vec<_>>();
    for (_, plugin_id, _) in PLUGINS {
        let summary = summaries
            .iter()
            .find(|summary| summary.plugin_id == plugin_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing plugin summary: {plugin_id}; packages={package_diagnostics:#?}"
                )
            });
        assert_eq!(summary.status, "loaded", "{plugin_id}: {:?}", summary.error);
    }

    for (index, (_, plugin_id, protocol_id)) in PLUGINS.iter().enumerate() {
        let result = host
            .call(FrontendTaskRequest {
                protocol_id: (*protocol_id).to_string(),
                payload: json!({}),
                task_id: Some(format!("package-e2e:{index}")),
                trace_id: None,
                correlation_id: None,
                idempotency_key: None,
                target_binding_id: Some(format!("binding:{plugin_id}:{protocol_id}")),
                runner_hint: Some(format!("{plugin_id}.runner")),
                input_refs: Vec::new(),
                priority: 0,
                context: FrontendContext::default(),
            })
            .unwrap_or_else(|error| panic!("{plugin_id} route failed: {error}"));
        assert!(
            result.outcome.is_some(),
            "{plugin_id} did not produce a terminal outcome"
        );
    }

    host.shutdown();
}

struct TestWorkspace {
    root: PathBuf,
    plugins: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "momobako-mutsuki-packages-{}-{nonce}",
            std::process::id()
        ));
        let plugins = root.join("plugins");
        fs::create_dir_all(&plugins).expect("test plugin directory");
        Self { root, plugins }
    }

    fn paths(&self) -> PathsConfig {
        PathsConfig {
            app_data_dir: self.root.clone(),
            config_dir: self.root.join("config"),
            data_dir: self.root.join("data"),
            cache_dir: self.root.join("cache"),
            logs_dir: self.root.join("logs"),
            plugins_dir: self.plugins.clone(),
            resources_dir: self.root.join("resources"),
            runners_dir: self.root.join("runners"),
        }
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
