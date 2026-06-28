//! Discovered file models and repository identifier helpers.

use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DiscoveredFile {
    pub(super) absolute_path: Option<PathBuf>,
    pub(super) relative_path: String,
    pub(super) filename: String,
    pub(super) extension: String,
    pub(super) size_bytes: i64,
    pub(super) created_at: Option<String>,
    pub(super) modified_at: String,
    #[serde(default)]
    pub(super) is_virtual: bool,
    #[serde(default)]
    pub(super) provider_id: Option<String>,
    #[serde(default)]
    pub(super) provider_item_id: Option<String>,
    #[serde(default)]
    pub(super) source_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) local_absolute_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BackendDiscoveredFile {
    pub(super) absolute_path: Option<PathBuf>,
    pub(super) relative_path: String,
    pub(super) filename: String,
    pub(super) extension: String,
    pub(super) size_bytes: i64,
    pub(super) created_at: Option<String>,
    pub(super) modified_at: String,
    #[serde(default)]
    pub(super) is_virtual: bool,
    #[serde(default)]
    pub(super) provider_id: Option<String>,
    #[serde(default)]
    pub(super) provider_item_id: Option<String>,
    #[serde(default)]
    pub(super) source_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) local_absolute_path: Option<String>,
}

impl BackendDiscoveredFile {
    pub(super) fn into_discovered_file(self, repo_root: &Path) -> Result<DiscoveredFile, String> {
        let relative_path = normalize_entry_path(&self.relative_path)?;
        let absolute_path =
            if self.is_virtual {
                self.absolute_path
            } else {
                Some(self.absolute_path.map(Ok).unwrap_or_else(|| {
                    resolve_repository_relative_path(repo_root, &relative_path)
                })?)
            };
        Ok(DiscoveredFile {
            absolute_path,
            relative_path,
            filename: self.filename,
            extension: self.extension,
            size_bytes: self.size_bytes,
            created_at: self.created_at,
            modified_at: self.modified_at,
            is_virtual: self.is_virtual,
            provider_id: self.provider_id,
            provider_item_id: self.provider_item_id,
            source_payload: self.source_payload,
            local_absolute_path: self.local_absolute_path,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MetadataDefaultsBatchEntry {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) extension: String,
    pub(super) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) metadata: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MetadataDefaultsBatchResponse {
    #[serde(default)]
    pub(super) defaults_by_path: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

pub(super) fn slugify_repo_id(name: &str, path: &str) -> String {
    slugify_ascii_component(&format!("{name}-{path}"))
}

pub(super) fn normalized_netease_account_id(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|item| item.to_string()))
        .or_else(|| value.as_u64().map(|item| item.to_string()))
}

pub(crate) fn asset_id_for_path(repo_id: &str, relative_path: &str) -> String {
    format!(
        "asset-{}",
        sha256_hex(&[repo_id.as_bytes(), relative_path.as_bytes()])
    )
}

pub(super) fn slugify_ascii_component(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}
