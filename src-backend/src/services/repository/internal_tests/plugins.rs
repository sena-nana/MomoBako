//! 插件注册表、依赖解析、配置读写、钩子调用与插件包安装卸载的测试。

use super::*;

#[test]
fn plugin_registry_discovers_runtime_manifests() {
    let workspace = TestWorkspace::new("plugin-registry");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let registry = plugin_catalog(&service_root);
    let manifests = registry.list_manifests();

    assert!(manifests.iter().any(|manifest| {
        manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
            && manifest.category == "source"
            && manifest.runtime == "native-dylib"
            && manifest
                .legacy_plugin_ids
                .contains(&LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
    }));
    assert!(manifests.iter().any(|manifest| {
        manifest.plugin_id == "momobako.preview.media"
            && manifest.category == "preview"
            && manifest.runtime == "manifest-only"
            && manifest.hooks.iter().any(|hook| hook.slot == "playlist")
    }));
    assert!(manifests.iter().any(|manifest| {
        manifest.plugin_id == "momobako.library.audio"
            && manifest.category == "library-kind"
            && manifest
                .optional
                .contains(&"momobako.parser.audio".to_string())
    }));
    assert!(manifests.iter().any(|manifest| {
        manifest.plugin_id == "momobako.parser.audio" && manifest.category == "parser"
    }));
    assert!(manifests.iter().any(|manifest| {
        manifest.plugin_id == "momobako.service.network-search" && manifest.category == "service"
    }));
    assert_eq!(
        registry.normalize_plugin_id(LOCAL_FILESYSTEM_PLUGIN_ID),
        LOCAL_FILESYSTEM_PLUGIN_ID
    );
}

#[test]
fn plugin_registry_resolves_dependencies_and_degraded_state() {
    let workspace = TestWorkspace::new("plugin-dependency-state");
    let plugin_root = workspace.path("service/plugins");
    fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("required-provider.momoplug"),
        test_plugin_manifest_json("user.provider", "Provider", serde_json::json!({})),
    );
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("optional-helper.momoplug"),
        test_plugin_manifest_json(
            "user.optional-helper",
            "Optional Helper",
            serde_json::json!({}),
        ),
    );
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("dependent-plugin.momoplug"),
        test_plugin_manifest_json(
            "user.dependent",
            "Dependent Plugin",
            serde_json::json!({
                "permissions": ["readMetadata"],
                "requires": ["user.provider"],
                "optional": ["user.optional-helper"]
            }),
        ),
    );
    let state = RepositoryState::from_root(workspace.path("service"));

    state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: "user.optional-helper".to_string(),
            enabled: false,
        })
        .expect("optional helper should be disabled");
    let plugins = state.list_plugins().expect("plugins should load");
    let dependent = plugins
        .iter()
        .find(|manifest| manifest.plugin_id == "user.dependent")
        .expect("dependent plugin should exist");

    assert_eq!(dependent.status, "ready");
    assert!(dependent.enabled);
    assert!(dependent.degraded);
    assert_eq!(
        dependent.dependency_status.optional[0].plugin_id,
        "user.optional-helper"
    );
    assert_eq!(dependent.dependency_status.optional[0].status, "disabled");
    assert!(dependent
        .degradation_reason
        .as_deref()
        .unwrap_or_default()
        .contains("Optional Helper"));

    fs::remove_file(plugin_root.join("required-provider.momoplug"))
        .expect("required provider archive should be removable");
    let plugins = state.list_plugins().expect("plugins should reload");
    let dependent = plugins
        .iter()
        .find(|manifest| manifest.plugin_id == "user.dependent")
        .expect("dependent plugin should remain listed");

    assert_eq!(dependent.status, "unavailable");
    assert!(!dependent.enabled);
    assert_eq!(dependent.dependency_status.required[0].status, "missing");
    assert!(dependent
        .disable_reason
        .as_deref()
        .unwrap_or_default()
        .contains("user.provider"));
}

#[test]
fn plugin_data_directory_uses_service_plugin_data_root_and_legacy_ids() {
    let workspace = TestWorkspace::new("plugin-data-directory");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root.clone());

    let response = state
        .get_plugin_data_directory(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
        .expect("plugin data directory should be returned");
    let expected_path = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);

    assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert_eq!(PathBuf::from(response.path), expected_path);
    assert!(expected_path.is_dir());
    assert!(expected_path.starts_with(service_root.join("plugin-data")));
}

#[test]
fn shutdown_runtime_helpers_cleans_office_helper_state_files() {
    let workspace = TestWorkspace::new("shutdown-office-helper-state");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root.clone());

    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.office-convert");
    let helper_dir = plugin_dir.join("helpers").join("libreoffice");
    fs::create_dir_all(&helper_dir).expect("office helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("port.txt"), "23119").expect("port state should be written");
    fs::write(helper_dir.join("session.txt"), "office").expect("session state should be written");
    fs::write(
        helper_dir.join("office-convert-helper.ps1"),
        "Write-Host helper",
    )
    .expect("helper script should be written");

    state
        .shutdown_runtime_helpers()
        .expect("helper shutdown should clean stale office state");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("port.txt").exists());
    assert!(!helper_dir.join("session.txt").exists());
    assert!(!helper_dir.join("office-convert-helper.ps1").exists());
}

#[test]
fn shutdown_runtime_helpers_cleans_aria2_helper_state_files() {
    let workspace = TestWorkspace::new("shutdown-aria2-helper-state");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root.clone());

    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.downloader");
    let helper_dir = plugin_dir.join("helpers").join("aria2");
    fs::create_dir_all(&helper_dir).expect("aria2 helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("session.txt"), "aria2-session")
        .expect("session state should be written");

    state
        .shutdown_runtime_helpers()
        .expect("helper shutdown should clean stale aria2 state");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("session.txt").exists());
}

#[test]
fn plugin_config_api_persists_values_in_plugin_data_dir() {
    let workspace = TestWorkspace::new("plugin-config");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root.clone());

    let updated = state
        .set_plugin_config_value(PluginConfigSetRequest {
            plugin_id: LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            key: "apiKey".to_string(),
            value: serde_json::json!("secret"),
        })
        .expect("plugin config value should be written");

    assert_eq!(updated.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert_eq!(
        updated.values.get("apiKey"),
        Some(&serde_json::json!("secret"))
    );
    let data_dir = plugin_data_dir(&service_root, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert!(data_dir.join("config.json").is_file());

    let loaded = state
        .get_plugin_config(LOCAL_FILESYSTEM_PLUGIN_ID.to_string())
        .expect("plugin config should be loaded");
    assert_eq!(
        loaded.values.get("apiKey"),
        Some(&serde_json::json!("secret"))
    );

    let deleted = state
        .delete_plugin_config_value(PluginConfigDeleteRequest {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            key: "apiKey".to_string(),
        })
        .expect("plugin config value should be deleted");
    assert!(!deleted.values.contains_key("apiKey"));
}

#[test]
fn plugin_config_api_includes_schema_and_rejects_mismatched_values() {
    let workspace = TestWorkspace::new("plugin-config-schema");
    let plugin_root = workspace.path("service/plugins");
    fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("configurable.momoplug"),
        test_plugin_manifest_json(
            "user.configurable",
            "Configurable Plugin",
            serde_json::json!({
                "contributes": {
                    "settings": {
                        "schemaVersion": 1,
                        "fields": [
                            { "key": "enabled", "label": "Enabled", "type": "boolean" },
                            {
                                "key": "mode",
                                "label": "Mode",
                                "type": "select",
                                "options": [
                                    { "label": "Fast", "value": "fast" },
                                    { "label": "Careful", "value": "careful" }
                                ]
                            }
                        ]
                    }
                }
            }),
        ),
    );
    let state = RepositoryState::from_root(workspace.path("service"));

    let snapshot = state
        .get_plugin_config("user.configurable".to_string())
        .expect("plugin config schema should load");
    assert_eq!(
        snapshot.schema["fields"][0]["key"],
        serde_json::json!("enabled")
    );

    let error = state
        .set_plugin_config_value(PluginConfigSetRequest {
            plugin_id: "user.configurable".to_string(),
            key: "enabled".to_string(),
            value: serde_json::json!("yes"),
        })
        .expect_err("schema mismatch should be rejected");
    assert!(error.contains("boolean"));

    state
        .set_plugin_config_value(PluginConfigSetRequest {
            plugin_id: "user.configurable".to_string(),
            key: "mode".to_string(),
            value: serde_json::json!("fast"),
        })
        .expect("schema option should be accepted");
}

#[test]
fn count_repository_directories_excludes_internal_directories() {
    let workspace = TestWorkspace::new("count-repository-directories");
    let repo_root = workspace.path("repo");
    fs::create_dir_all(repo_root.join("music").join("albums"))
        .expect("nested dir should be created");
    fs::create_dir_all(repo_root.join(".hidden")).expect("hidden dir should be created");
    fs::create_dir_all(repo_root.join(".momo")).expect("meta dir should be created");
    fs::create_dir_all(repo_root.join(".meta")).expect("legacy meta dir should be created");

    let total =
        count_repository_directories(&repo_root).expect("directory counting should succeed");

    assert_eq!(total, 3);
}

#[test]
fn native_dylib_source_plugin_without_library_is_marked_unavailable() {
    let workspace = TestWorkspace::new("native-dylib-source-unavailable");
    let service_root = workspace.path("service");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_local_filesystem_plugin_archive(&plugin_root, serde_json::json!({}));
    disable_local_filesystem_adapter();
    let state = RepositoryState::from_root(service_root);

    let plugin = state
        .list_plugins()
        .expect("plugins should load")
        .into_iter()
        .find(|manifest| manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID)
        .expect("local filesystem plugin should exist");

    assert_eq!(plugin.status, "unavailable");
    assert!(plugin
        .disable_reason
        .as_deref()
        .unwrap_or_default()
        .contains("原生运行时不可用"));
}

#[test]
fn plugin_dependency_resolution_accepts_legacy_ids() {
    let workspace = TestWorkspace::new("plugin-legacy-dependency");
    let plugin_root = workspace.path("service/plugins");
    fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("provider.momoplug"),
        test_plugin_manifest_json(
            "user.provider",
            "Provider",
            serde_json::json!({
                "legacyPluginIds": ["legacy.provider"],
                "compat": {
                    "sdkVersion": "1",
                    "legacyPluginIds": []
                }
            }),
        ),
    );
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("dependent.momoplug"),
        test_plugin_manifest_json(
            "user.dependent",
            "Dependent",
            serde_json::json!({
                "requires": ["legacy.provider"]
            }),
        ),
    );
    let state = RepositoryState::from_root(workspace.path("service"));
    let plugins = state.list_plugins().expect("plugins should load");
    let dependent = plugins
        .iter()
        .find(|manifest| manifest.plugin_id == "user.dependent")
        .expect("dependent plugin should exist");

    assert_eq!(dependent.status, "ready");
    assert_eq!(
        dependent.dependency_status.required[0].plugin_id,
        "user.provider"
    );
    assert!(dependent.dependency_status.missing_required.is_empty());
}

#[test]
fn plugin_call_blocks_missing_required_dependency() {
    let workspace = TestWorkspace::new("plugin-call-required-missing");
    let service_root = workspace.path("service");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_local_filesystem_plugin_archive(
        &plugin_root,
        serde_json::json!({ "requires": ["user.required-provider"] }),
    );
    let state = RepositoryState::from_root(service_root.clone());
    let error = state
        .call_plugin(test_list_files_plugin_call(
            LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
            workspace.path("repo"),
        ))
        .expect_err("missing required dependency should block plugin call");

    assert!(error.contains("plugin call blocked by dependency status"));
    assert!(error.contains(LOCAL_FILESYSTEM_PLUGIN_ID));
    assert!(error.contains("filesystem.listFiles"));
    assert!(error.contains("缺少必需依赖"));
}

#[test]
fn plugin_call_blocks_disabled_required_dependency() {
    let workspace = TestWorkspace::new("plugin-call-required-disabled");
    let service_root = workspace.path("service");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("required-provider.momoplug"),
        test_plugin_manifest_json(
            "user.required-provider",
            "Required Provider",
            serde_json::json!({}),
        ),
    );
    write_test_local_filesystem_plugin_archive(
        &plugin_root,
        serde_json::json!({ "requires": ["user.required-provider"] }),
    );
    let state = RepositoryState::from_root(service_root);
    state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: "user.required-provider".to_string(),
            enabled: false,
        })
        .expect("required provider should be disabled");

    let error = state
        .call_plugin(test_list_files_plugin_call(
            LOCAL_FILESYSTEM_PLUGIN_ID,
            workspace.path("repo"),
        ))
        .expect_err("disabled required dependency should block plugin call");

    assert!(error.contains("plugin call blocked by dependency status"));
    assert!(error.contains("必需依赖不可用"));
    assert!(error.contains("Required Provider"));
}

#[test]
fn plugin_call_returns_degraded_runtime_for_disabled_optional_dependency() {
    let workspace = TestWorkspace::new("plugin-call-optional-disabled");
    let service_root = workspace.path("service");
    let repo_root = workspace.path("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    fs::write(repo_root.join("track.mp3"), b"audio").expect("test file should be written");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("optional-helper.momoplug"),
        test_plugin_manifest_json(
            "user.optional-helper",
            "Optional Helper",
            serde_json::json!({}),
        ),
    );
    write_test_local_filesystem_plugin_archive(
        &plugin_root,
        serde_json::json!({ "optional": ["user.optional-helper"] }),
    );
    let state = RepositoryState::from_root(service_root);
    state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: "user.optional-helper".to_string(),
            enabled: false,
        })
        .expect("optional helper should be disabled");

    let response = state
        .call_plugin(test_list_files_plugin_call(
            LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
            repo_root,
        ))
        .expect("optional dependency should not block plugin call");

    assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert!(response.payload.is_array());
    let runtime = response
        .runtime
        .expect("degraded runtime context should be returned");
    assert!(runtime.degraded);
    assert!(runtime
        .degradation_reason
        .as_deref()
        .unwrap_or_default()
        .contains("Optional Helper"));
    assert_eq!(
        runtime.dependency_status.optional[0].plugin_id,
        "user.optional-helper"
    );
    assert_eq!(runtime.dependency_status.optional[0].status, "disabled");
}

#[test]
fn plugin_call_accepts_legacy_required_dependency_id() {
    let workspace = TestWorkspace::new("plugin-call-legacy-required");
    let service_root = workspace.path("service");
    let repo_root = workspace.path("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_plugin_archive_with_manifest(
        &plugin_root.join("provider.momoplug"),
        test_plugin_manifest_json(
            "user.provider",
            "Provider",
            serde_json::json!({
                "legacyPluginIds": ["legacy.provider"],
                "compat": {
                    "sdkVersion": "1",
                    "legacyPluginIds": []
                }
            }),
        ),
    );
    write_test_local_filesystem_plugin_archive(
        &plugin_root,
        serde_json::json!({ "requires": ["legacy.provider"] }),
    );
    let state = RepositoryState::from_root(service_root);

    let response = state
        .call_plugin(test_list_files_plugin_call(
            LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
            repo_root,
        ))
        .expect("legacy dependency id should resolve before plugin call");

    assert_eq!(response.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert!(response.runtime.is_none());
}

#[test]
fn plugin_hook_execution_records_declared_hook_calls() {
    let workspace = TestWorkspace::new("plugin-hook-execution-records");
    let service_root = workspace.path("service");
    let repo_root = workspace.path("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    fs::write(repo_root.join("note.txt"), b"note").expect("test file should be written");
    let plugin_root = runtime_plugins_dir(&service_root);
    write_test_local_filesystem_plugin_archive(
        &plugin_root,
        serde_json::json!({
            "hooks": [
                {
                    "slot": "auditLog",
                    "action": "filesystem.listFiles",
                    "label": "记录文件列表"
                }
            ]
        }),
    );
    let state = RepositoryState::from_root(service_root);

    state
        .call_plugin(test_list_files_plugin_call(
            LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID,
            repo_root,
        ))
        .expect("declared hook plugin call should succeed");
    let all_records = state
        .list_plugin_hook_executions(PluginHookExecutionListRequest::default())
        .expect("hook execution records should load");

    assert_eq!(all_records.records.len(), 1);
    let record = &all_records.records[0];
    assert_eq!(record.plugin_id, LOCAL_FILESYSTEM_PLUGIN_ID);
    assert_eq!(record.hook_slot, "auditLog");
    assert_eq!(record.hook_action, "filesystem.listFiles");
    assert_eq!(record.hook_label.as_deref(), Some("记录文件列表"));
    assert_eq!(record.status, "success");
    assert_eq!(record.target.get("repoRoot"), None);
    assert_eq!(record.target.get("config"), None);

    state
        .call_plugin(PluginCallRequest {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            method: "filesystem.listTree".to_string(),
            repository_id: None,
            payload: serde_json::json!({
                "repoRoot": workspace.path("repo"),
                "path": "note.txt"
            }),
        })
        .expect("non-hook plugin call should succeed");
    let filtered = state
        .list_plugin_hook_executions(PluginHookExecutionListRequest {
            plugin_id: Some(LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID.to_string()),
            limit: Some(1),
        })
        .expect("filtered hook execution records should load");

    assert_eq!(filtered.records.len(), 1);
    assert_eq!(filtered.records[0].hook_action, "filesystem.listFiles");
}

#[test]
fn release_plugin_manifest_loading_returns_empty_when_runtime_dir_is_empty() {
    let workspace = TestWorkspace::new("runtime-plugin-empty");
    let manifests = load_plugin_manifests_from_runtime(workspace.path("plugins"));

    assert!(manifests.is_empty());
}

#[test]
fn runtime_manifest_scan_reflects_deleted_plugin_archives() {
    let workspace = TestWorkspace::new("runtime-plugin-scan");
    let plugin_root = workspace.path("plugins");
    fs::create_dir_all(&plugin_root).expect("runtime plugin dir should be created");
    let plugin_archive = plugin_root.join("sample-plugin.momoplug");
    write_test_plugin_archive(&plugin_archive, "user.sample-runtime");

    let manifests = load_plugin_manifests_from_runtime(plugin_root.clone());
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].manifest.plugin_id, "user.sample-runtime");

    fs::remove_file(plugin_archive).expect("runtime plugin archive should be removable");
    let manifests = load_plugin_manifests_from_runtime(plugin_root);
    assert!(manifests.is_empty());
}

#[test]
fn set_plugin_enabled_persists_plugin_state() {
    let workspace = TestWorkspace::new("plugin-enabled-state");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root.clone());

    let response = state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: "momobako.preview.media".to_string(),
            enabled: false,
        })
        .expect("plugin should be disabled");
    let disabled = response
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "momobako.preview.media")
        .expect("media plugin should be listed");
    assert!(!disabled.enabled);
    assert_eq!(disabled.status, "disabled");

    let reloaded_state = RepositoryState::from_root(service_root);
    let plugins = reloaded_state
        .list_plugins()
        .expect("plugins should reload from persisted state");
    let disabled = plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "momobako.preview.media")
        .expect("media plugin should be listed after reload");
    assert!(!disabled.enabled);
}

#[test]
fn delete_plugin_rejects_builtin_plugins() {
    let workspace = TestWorkspace::new("builtin-plugin-delete");
    let service_root = workspace.path("service");
    seed_standard_test_plugins(&service_root);
    let state = RepositoryState::from_root(service_root);

    let error = state
        .delete_plugin("momobako.preview.media".to_string())
        .expect_err("built-in plugins should not be deleted");

    assert!(error.contains("built-in plugins cannot be deleted"));
}

#[test]
fn install_plugin_from_archive_loads_and_deletes_user_plugin() {
    let workspace = TestWorkspace::new("plugin-archive-install");
    let service_root = workspace.path("service");
    let archive_path = workspace.path("sample-plugin.momoplug");
    write_test_v2_plugin_archive(&archive_path, "user.sample-metadata", "builtin");
    let state = RepositoryState::from_root(service_root.clone());

    let response = state
        .install_plugin_from_archive(PluginInstallRequest {
            package_path: archive_path.to_string_lossy().to_string(),
        })
        .expect("plugin archive should install");
    let installed = response
        .plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "user.sample-metadata")
        .expect("installed plugin should be listed");
    assert_eq!(installed.source, "user");
    assert_eq!(installed.provenance.as_deref(), Some("user-installed"));
    assert_eq!(installed.trust_level.as_deref(), Some("untrusted"));
    assert_eq!(installed.package_format_version, Some(2));
    assert!(installed.enabled);
    assert!(runtime_plugins_dir(&service_root)
        .join("user")
        .join("user-sample-metadata-0.1.0.momoplug")
        .is_file());

    let response = state
        .delete_plugin("user.sample-metadata".to_string())
        .expect("user plugin should be deleted");
    assert!(!response
        .plugins
        .iter()
        .any(|plugin| plugin.plugin_id == "user.sample-metadata"));
    assert!(!runtime_plugins_dir(&service_root)
        .join("user")
        .join("user-sample-metadata-0.1.0.momoplug")
        .exists());
}

#[test]
fn frontend_plugin_packages_are_listed_as_unreadable() {
    let workspace = TestWorkspace::new("plugin-frontend-rejected");
    let service_root = workspace.path("service");
    let runtime_root = runtime_plugins_dir(&service_root);
    fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive_with_options(
        &runtime_root.join("example-text-preview-0.1.0.momoplug"),
        TestPluginArchiveOptions {
            plugin_id: "momobako.example.text-preview",
            name: "Example Text Preview",
            source: "user",
            runtime: "vue-module",
            sdk: "frontend",
            kind: "preview",
            plugin_type_layer: "library-kind",
            plugin_type_kind: "preview",
            entry: serde_json::json!({
                "frontend": {
                    "module": "dist/register.js",
                    "export": "register"
                }
            }),
            extra_files: vec![(
                "dist/register.js".to_string(),
                "export function register(){ return 'ok'; }".to_string(),
            )],
        },
    );
    write_test_plugin_archive(
        &runtime_root.join("good-plugin.momoplug"),
        "user.good-plugin",
    );

    let state = RepositoryState::from_root(service_root);
    let plugins = state.list_plugins().expect("plugins should load");

    assert!(!plugins
        .iter()
        .any(|plugin| plugin.plugin_id == "momobako.example.text-preview"));
    let rejected = plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "broken.example-text-preview-0-1-0")
        .expect("frontend plugin package should be listed as unreadable");
    assert!(!rejected.enabled);
    assert!(matches!(rejected.status.as_str(), "error" | "disabled"));
    assert!(rejected.description.contains("unsupported plugin form"));
    assert!(plugins
        .iter()
        .any(|plugin| plugin.plugin_id == "user.good-plugin" && plugin.status == "ready"));
}

#[test]
fn runtime_builtin_plugins_keep_manifest_source_value() {
    let workspace = TestWorkspace::new("runtime-builtin-source");
    let service_root = workspace.path("service");
    let runtime_root = runtime_plugins_dir(&service_root);
    fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive_with_options(
        &runtime_root.join("media-preview-1.0.0.momoplug"),
        TestPluginArchiveOptions {
            plugin_id: "momobako.preview.media",
            name: "Media Preview",
            source: "builtin",
            runtime: "manifest-only",
            sdk: "backend",
            kind: "preview",
            plugin_type_layer: "library-kind",
            plugin_type_kind: "preview",
            entry: serde_json::json!({}),
            extra_files: Vec::new(),
        },
    );

    let state = RepositoryState::from_root(service_root);
    let plugins = state.list_plugins().expect("plugins should load");
    let plugin = plugins
        .iter()
        .find(|item| item.plugin_id == "momobako.preview.media")
        .expect("media plugin should be listed");

    assert_eq!(plugin.source, "builtin");
}

#[test]
fn install_plugin_from_archive_rejects_zip_extension() {
    let workspace = TestWorkspace::new("plugin-archive-zip");
    let service_root = workspace.path("service");
    let archive_path = workspace.path("sample-plugin.zip");
    write_test_plugin_archive(&archive_path, "user.sample-metadata");
    let state = RepositoryState::from_root(service_root);

    let error = state
        .install_plugin_from_archive(PluginInstallRequest {
            package_path: archive_path.to_string_lossy().to_string(),
        })
        .expect_err("zip extension should be rejected");

    assert!(error.contains(".momoplug extension"));
}

#[test]
fn install_plugin_from_archive_rejects_root_level_manifest_packages() {
    let workspace = TestWorkspace::new("plugin-archive-root-manifest");
    let service_root = workspace.path("service");
    let archive_path = workspace.path("sample-plugin.momoplug");
    write_test_plugin_archive_without_root_dir(&archive_path, "user.sample-rootless");
    let state = RepositoryState::from_root(service_root);

    let error = state
        .install_plugin_from_archive(PluginInstallRequest {
            package_path: archive_path.to_string_lossy().to_string(),
        })
        .expect_err("root-level manifest package should be rejected");

    assert!(error.contains("exactly one root directory with manifest.json"));
}

#[test]
fn install_plugin_from_archive_rejects_duplicate_plugin_id() {
    let workspace = TestWorkspace::new("plugin-archive-duplicate-id");
    let service_root = workspace.path("service");
    let first_archive = workspace.path("sample-plugin-a.momoplug");
    let second_archive = workspace.path("sample-plugin-b.momoplug");
    write_test_plugin_archive(&first_archive, "user.sample-duplicate");
    write_test_plugin_archive(&second_archive, "user.sample-duplicate");
    let state = RepositoryState::from_root(service_root);

    state
        .install_plugin_from_archive(PluginInstallRequest {
            package_path: first_archive.to_string_lossy().to_string(),
        })
        .expect("first plugin archive should install");

    let error = state
        .install_plugin_from_archive(PluginInstallRequest {
            package_path: second_archive.to_string_lossy().to_string(),
        })
        .expect_err("duplicate plugin id should be rejected");

    assert!(error.contains("plugin already exists: user.sample-duplicate"));
}

#[test]
fn broken_plugin_archives_do_not_hide_other_runtime_plugins() {
    let workspace = TestWorkspace::new("broken-plugin-archive");
    let service_root = workspace.path("service");
    let runtime_root = runtime_plugins_dir(&service_root);
    fs::create_dir_all(&runtime_root).expect("runtime plugin dir should be created");
    write_test_plugin_archive(
        &runtime_root.join("good-plugin.momoplug"),
        "user.good-plugin",
    );
    fs::write(runtime_root.join("broken-plugin.momoplug"), b"not-a-zip")
        .expect("broken plugin archive should be written");

    let state = RepositoryState::from_root(service_root);
    let plugins = state.list_plugins().expect("plugins should load");

    let good = plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "user.good-plugin")
        .expect("good plugin should still be listed");
    assert_eq!(good.status, "ready");

    let broken = plugins
        .iter()
        .find(|plugin| plugin.plugin_id == "broken.broken-plugin")
        .expect("broken plugin placeholder should be listed");
    assert!(!broken.enabled);
    assert!(matches!(broken.status.as_str(), "error" | "disabled"));
    assert!(broken.description.contains("Failed to read plugin archive"));
}
