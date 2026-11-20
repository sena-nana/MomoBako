//! Plugin, cache, and API design operations.

use super::*;
use crate::services::mutsuki_host;
use std::path::Path;

mod api;
mod archive;
mod config;
mod data_dirs;
mod manifest;

pub(super) use api::*;
pub(crate) use archive::*;
pub(crate) use config::*;
pub(crate) use data_dirs::*;
pub(crate) use manifest::*;

const PLUGIN_PACKAGE_FORMAT_VERSION: u32 = 2;
const PLUGIN_PACKAGE_MANIFEST: &str = "momobako.package.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginPackageEnvelope {
    format_version: u32,
    plugin_id: String,
    version: String,
    target_triple: String,
    deployment: String,
    product_manifest: String,
    #[serde(default)]
    runtime_manifest: Option<String>,
    #[serde(default)]
    artifacts: Vec<PluginPackageArtifact>,
}

#[derive(Debug, Deserialize)]
struct PluginPackageArtifact {
    role: String,
    path: String,
    sha256: String,
    #[serde(default)]
    executable: bool,
}
