//! Repository persistence constants and SQL schema.

use super::*;

pub(super) const REGISTRY_FILE_NAME: &str = "repositories.db";
pub(super) const REPO_META_DIR: &str = ".momo";
pub(super) const LEGACY_REPO_META_DIR: &str = ".meta";
pub(super) const REPO_TRASH_DIR: &str = "trash";
pub(super) const REPO_TRASH_MANIFEST_FILE_NAME: &str = "trash.json";
pub(super) const REPO_METADATA_FILE_NAME: &str = "repository.json";
pub(super) const REPO_DB_FILE_NAME: &str = "metadata.db";
pub(super) const REPO_SCHEMA_VERSION: i64 = 4;
pub(super) const THUMBNAIL_SIZE: u32 = 256;
pub(super) const MAX_REMOTE_THUMBNAIL_BYTES: u64 = 10 * 1024 * 1024;
pub(super) const PLUGIN_HOOK_EXECUTIONS_FILE_NAME: &str = "plugin-hook-executions.jsonl";

pub(super) static FFMPEG_READY: OnceLock<Result<(), String>> = OnceLock::new();
pub(super) const REGISTRY_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  backend_plugin_id TEXT NOT NULL,
  backend_config_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
  component TEXT PRIMARY KEY,
  version INTEGER NOT NULL
);

INSERT INTO schema_version(component, version)
VALUES ('registry', 1)
ON CONFLICT(component) DO UPDATE SET version = excluded.version;
"#;

pub(super) const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";
pub(super) const LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID: &str = "builtin.local-filesystem";
pub(super) const NETEASE_CLOUD_MUSIC_PLUGIN_ID: &str = "momobako.netease.source";
pub(super) const LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID: &str = "momobako.source.netease-cloud-music";
pub(super) const AUDIO_PLAYER_PLUGIN_ID: &str = "momobako.player.audio";
pub(super) const LEGACY_AUDIO_PLAYER_PLUGIN_ID: &str = "momobako.preview.media";
pub(super) const NETEASE_CLOUD_MUSIC_PROVIDER_ID: &str = "netease-cloud-music";
pub(super) const PLUGIN_SDK_VERSION: &str = "2";
pub(super) const MAX_PARALLEL_IMPORTS: usize = 4;

pub(super) const REPOSITORY_SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS repositories (
  repo_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assets (
  asset_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  filename TEXT NOT NULL,
  extension TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL,
  hash TEXT,
  status TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  last_accessed_at TEXT,
  thumbnail_path TEXT,
  is_virtual INTEGER NOT NULL DEFAULT 0,
  provider_id TEXT,
  provider_item_id TEXT,
  source_payload_json TEXT,
  local_absolute_path TEXT,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_assets_repo_path ON assets(repo_id, path);
CREATE INDEX IF NOT EXISTS idx_assets_repo_filename ON assets(repo_id, filename);
CREATE INDEX IF NOT EXISTS idx_assets_repo_status ON assets(repo_id, status);
CREATE INDEX IF NOT EXISTS idx_assets_repo_hash ON assets(repo_id, hash);

CREATE TABLE IF NOT EXISTS hardlink_groups (
  group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_groups_repo_hash_size
ON hardlink_groups(repo_id, content_hash, size_bytes);

CREATE TABLE IF NOT EXISTS hardlink_members (
  group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  path TEXT NOT NULL,
  link_state TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  verified_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, asset_id),
  FOREIGN KEY(group_id) REFERENCES hardlink_groups(group_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_hardlink_members_repo_path
ON hardlink_members(repo_id, path);

CREATE TABLE IF NOT EXISTS hardlink_candidates (
  candidate_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  new_asset_id TEXT NOT NULL,
  new_path TEXT NOT NULL,
  existing_asset_id TEXT NOT NULL,
  existing_path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_candidates_unique
ON hardlink_candidates(repo_id, new_asset_id, existing_asset_id);

CREATE TABLE IF NOT EXISTS asset_alias_groups (
  alias_group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  source TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS asset_alias_members (
  alias_group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  path TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, asset_id),
  FOREIGN KEY(alias_group_id) REFERENCES asset_alias_groups(alias_group_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_asset_alias_members_group
ON asset_alias_members(repo_id, alias_group_id, path);

CREATE TABLE IF NOT EXISTS repository_shortcuts (
  shortcut_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  label TEXT NOT NULL,
  target_kind TEXT NOT NULL,
  target_path TEXT,
  target_id TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_shortcuts_repo_order
ON repository_shortcuts(repo_id, sort_order, label);

CREATE TABLE IF NOT EXISTS tag_groups (
  tag_group_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS tag_group_members (
  tag_group_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  normalized_tag TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(tag_group_id, normalized_tag),
  FOREIGN KEY(tag_group_id) REFERENCES tag_groups(tag_group_id)
);

CREATE INDEX IF NOT EXISTS idx_tag_group_members_repo_tag
ON tag_group_members(repo_id, normalized_tag);

CREATE TABLE IF NOT EXISTS folder_metadata (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  protected INTEGER NOT NULL DEFAULT 0,
  password_tip TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS directories (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  parent_path TEXT NOT NULL,
  name TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_directories_repo_parent
ON directories(repo_id, parent_path, name);

CREATE TABLE IF NOT EXISTS entry_thumbnails (
  repo_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  thumbnail_path TEXT NOT NULL,
  custom INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, path, kind)
);

CREATE TABLE IF NOT EXISTS source_trash_entries (
  repo_id TEXT NOT NULL,
  trash_path TEXT NOT NULL,
  original_path TEXT NOT NULL,
  kind TEXT NOT NULL,
  deleted_at TEXT NOT NULL,
  shared_asset_id TEXT,
  PRIMARY KEY(repo_id, trash_path),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS netease_directory_cache (
  repo_id TEXT NOT NULL,
  directory_path TEXT NOT NULL,
  total_entries INTEGER NOT NULL,
  refreshed_at TEXT NOT NULL,
  PRIMARY KEY(repo_id, directory_path),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS netease_directory_entries (
  repo_id TEXT NOT NULL,
  directory_path TEXT NOT NULL,
  order_index INTEGER NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  extension TEXT,
  size_bytes INTEGER,
  modified_at TEXT,
  is_virtual INTEGER NOT NULL DEFAULT 0,
  provider_id TEXT,
  provider_item_id TEXT,
  source_payload_json TEXT,
  local_absolute_path TEXT,
  PRIMARY KEY(repo_id, directory_path, order_index),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_netease_directory_entries_repo_path
ON netease_directory_entries(repo_id, path);

CREATE TABLE IF NOT EXISTS smart_folders (
  smart_folder_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  parent_id TEXT,
  name TEXT NOT NULL,
  filter_json TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(parent_id) REFERENCES smart_folders(smart_folder_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_smart_folders_repo_parent
ON smart_folders(repo_id, parent_id, sort_order, name);

CREATE TABLE IF NOT EXISTS repository_actions (
  action_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  source TEXT NOT NULL,
  source_action_id TEXT,
  name TEXT NOT NULL,
  status TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL,
  unsupported_reason TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_actions_repo_order
ON repository_actions(repo_id, sort_order, name);

CREATE TABLE IF NOT EXISTS playlists (
  playlist_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  name TEXT NOT NULL,
  player_type_id TEXT NOT NULL,
  player_plugin_id TEXT NOT NULL,
  player_label TEXT NOT NULL,
  file_class TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_playlists_repo_order
ON playlists(repo_id, sort_order, updated_at DESC);

CREATE TABLE IF NOT EXISTS playlist_items (
  playlist_item_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  playlist_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(playlist_id) REFERENCES playlists(playlist_id) ON DELETE CASCADE,
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_playlist_items_unique_asset
ON playlist_items(playlist_id, asset_id);

CREATE INDEX IF NOT EXISTS idx_playlist_items_repo_order
ON playlist_items(repo_id, playlist_id, sort_order, added_at);

CREATE TABLE IF NOT EXISTS repository_action_steps (
  step_id TEXT PRIMARY KEY,
  action_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  step_kind TEXT NOT NULL,
  label TEXT NOT NULL,
  status TEXT NOT NULL,
  config_json TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  unsupported_reason TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(action_id) REFERENCES repository_actions(action_id) ON DELETE CASCADE,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_action_steps_action_order
ON repository_action_steps(action_id, sort_order);

CREATE TABLE IF NOT EXISTS repository_action_runs (
  run_id TEXT PRIMARY KEY,
  action_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  status TEXT NOT NULL,
  target_json TEXT NOT NULL,
  message TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  FOREIGN KEY(action_id) REFERENCES repository_actions(action_id),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_repository_action_runs_action_time
ON repository_action_runs(action_id, started_at DESC);

CREATE TABLE IF NOT EXISTS repository_action_run_steps (
  run_step_id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  repo_id TEXT NOT NULL,
  status TEXT NOT NULL,
  message TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  FOREIGN KEY(run_id) REFERENCES repository_action_runs(run_id) ON DELETE CASCADE,
  FOREIGN KEY(step_id) REFERENCES repository_action_steps(step_id),
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE TABLE IF NOT EXISTS metadata (
  asset_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value_type TEXT NOT NULL,
  value_json TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(asset_id, key),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_metadata_key ON metadata(key);

CREATE TABLE IF NOT EXISTS tags (
  asset_id TEXT NOT NULL,
  tag TEXT NOT NULL,
  normalized_tag TEXT NOT NULL,
  PRIMARY KEY(asset_id, normalized_tag),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(normalized_tag);

CREATE TABLE IF NOT EXISTS revisions (
  revision_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  source TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
  FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
);

CREATE INDEX IF NOT EXISTS idx_revisions_asset_time ON revisions(asset_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS events (
  event_id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL,
  asset_id TEXT,
  event_type TEXT NOT NULL,
  path TEXT NOT NULL,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
);

CREATE INDEX IF NOT EXISTS idx_events_repo_time ON events(repo_id, created_at DESC);

CREATE TABLE IF NOT EXISTS schema_version (
  component TEXT PRIMARY KEY,
  version INTEGER NOT NULL
);

INSERT INTO schema_version(component, version)
VALUES ('repository', 3)
ON CONFLICT(component) DO UPDATE SET version = excluded.version;
"#;
