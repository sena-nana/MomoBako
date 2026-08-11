//! Source 的安全边界与稳定契约测试。

use std::collections::BTreeMap;

use momobako_mutsuki_plugin_sdk::PluginRuntimeContext;

use crate::{
    actions, client,
    models::{RuntimeContext, StoredSession},
};

#[test]
fn normalized_domain_removes_api_suffixes() {
    assert_eq!(
        client::normalize_ncm_domain(Some("https://music.163.com/weapi")),
        "https://music.163.com"
    );
    assert_eq!(
        client::normalize_ncm_domain(Some("https://interface.music.163.com/eapi/song/lyric")),
        "https://interface.music.163.com"
    );
}

#[test]
fn stored_session_serialization_never_contains_cookie() {
    let value = serde_json::to_value(StoredSession {
        credential_ref: "keyring:momobako.netease.source:42".to_string(),
        account_id: 42,
        user_name: Some("user".to_string()),
        nickname: Some("nickname".to_string()),
        avatar_url: None,
        fetched_at: "2026-08-11T00:00:00Z".to_string(),
    })
    .expect("session should serialize");
    assert!(value.get("cookie").is_none());
    assert!(value.get("credentialRef").is_some());
}

#[test]
fn action_contract_is_declarative() {
    let value = actions::describe();
    let actions = value.as_array().expect("actions should be an array");
    assert!(actions.iter().all(|action| action.get("script").is_none()));
    assert!(actions
        .iter()
        .any(|action| action["operation"] == "playlist-from-directory"));
    assert!(actions
        .iter()
        .any(|action| action["method"] == "media.prepareTrackPlayback"));
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../manifest.json"))
        .expect("plugin manifest should be valid json");
    assert_eq!(value, manifest["contributes"]["source"]["entryActions"]);
}

#[test]
fn runtime_prefers_call_settings_without_exposing_legacy_cookie() {
    let runtime = PluginRuntimeContext {
        plugin_id: "momobako.netease.source".to_string(),
        plugin_data_dir: ".".to_string(),
        service_root_dir: ".".to_string(),
        plugin_runtime_dir: ".".to_string(),
        plugin_config: BTreeMap::new(),
    };
    let config = serde_json::json!({
        "defaultLevel": "lossless",
        "credentialRef": "keyring:momobako.netease.source:42",
        "accountId": "42"
    });
    let context = RuntimeContext::new(runtime, std::path::PathBuf::from("."), Some(&config));
    assert_eq!(context.default_level, "lossless");
    assert_eq!(context.repo_backend_config.account_id, Some(42));
    assert!(context.repo_backend_config.legacy_cookie.is_none());
}
