//! Repository domain service facade and module map.

use rusqlite::{params, types::Type, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{CStr, CString, OsString},
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::c_char,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

mod action;
mod api_design;
mod asset_mutation;
mod browser;
mod contracts;
mod discovery;
mod export;
mod external_assets;
mod facade;
mod file_transfer;
mod filesystem_backend;
#[cfg(test)]
mod internal_tests;
mod local_index;
mod management;
mod pathing;
mod playback;
mod playlist;
mod plugin;
mod plugin_runtime;
mod query;
mod read_model;
mod records;
mod registry_store;
mod schema;
mod search_engine;
mod smart_folder;
mod state;
mod sync_engine;
pub(crate) mod test_support;
mod thumbnail;
mod trash;
mod utils;

use self::api_design::*;
use self::asset_mutation::*;
pub use self::contracts::*;
use self::discovery::*;
use self::export::*;
use self::file_transfer::*;
use self::filesystem_backend::*;
use self::local_index::*;
use self::pathing::*;
pub(crate) use self::playback::download_playlist_with_progress;
use self::plugin::{
    apply_plugin_settings, broken_plugin_manifest, embedded_local_filesystem_fallback_enabled,
    ensure_plugin_data_dir, ensure_repository_backend_runtime_available, is_source_plugin,
    load_native_plugin, load_plugin_config_values, load_plugin_settings,
    parse_plugin_manifest_with_source, plugin_data_dir, plugin_legacy_ids,
    read_plugin_manifest_from_archive, resolve_plugin_manifest_dependencies, runtime_plugins_dir,
};
#[cfg(test)]
use self::plugin::{is_repository_backend_plugin, parse_plugin_manifest};
use self::plugin_runtime::*;
#[cfg(test)]
pub(crate) use self::plugin_runtime::{
    install_local_filesystem_test_plugin_archive, set_test_downloader_playback_hook,
    set_test_downloader_track_package_hook,
};
use self::read_model::*;
use self::records::*;
use self::registry_store::*;
use self::schema::*;
use self::search_engine::*;
pub use self::state::RepositoryState;
pub(crate) use self::state::RepositoryStructureRefreshRequest;
use self::sync_engine::*;
use self::thumbnail::*;
use self::trash::*;
use self::utils::*;
