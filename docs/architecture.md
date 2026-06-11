# Repository Architecture

## Registry

- Global registry lives in `MetaHub/repositories.db`
- Tracks `repoId`, `name`, `path`, `status`, timestamps

## Repository Layout

- User files remain untouched
- System data lives under `.momo/`
- Current implementation stores:
  - `repository.json`
  - `metadata.db`
  - `cache/`
  - `thumbnails/`
  - `logs/`
  - `indexes/`

## Storage Model

- SQLite with WAL
- Core tables:
  - `repositories`
  - `assets`
  - `metadata`
  - `tags`
  - `revisions`
  - `events`
  - `schema_version`

## Sync Model

- Full scan walks repository files excluding `.momo`
- Existing asset paths are reconciled against disk
- Missing files are marked `deleted`
- New files create:
  - asset row
  - default metadata rows
  - filesystem event rows

## Revision Model

- Metadata writes produce a new `revisions` row
- Undo/Redo are implemented by replaying `before` / `after` metadata snapshots
- Asset version is incremented on every applied state transition

## Search Strategy

- Current implementation performs structured search across:
  - filename
  - path
  - status
  - tags
  - metadata values
- Indexed columns exist for:
  - `assets(repo_id, path)`
  - `assets(filename)`
  - `metadata(key)`
  - `tags(normalized_tag)`

## Cache Strategy

- LRU-style capacities are configured for:
  - metadata cache
  - thumbnail cache
  - query cache
- Current UI surfaces capacities and recent entries
- Thumbnail cache files use sha256 hex filenames; 3D and text preview thumbnails are rendered on the frontend and persisted through the thumbnail API.
- Large local preview files are exposed to preview plugins through session-scoped local HTTP URLs served by the in-process repository runtime, so the UI does not marshal full file bytes through the command bridge. Text previews fetch bounded byte ranges from the same source and fall back to direct file reads when needed.

## Plugin Architecture

- Plugin source projects live under `External/Plugins/*` and are built independently from the main app.
- Runtime plugins live under `<serviceRoot>/plugins/*.momoplug`.
- The desktop runtime scans only `.momoplug` files in that directory, reads `manifest.json` from the archive directly, and does not fall back to compiled manifests or source-relative plugin folders.
- Plugin manifest includes the existing display fields plus runtime fields:
  - `pluginId`, `legacyPluginIds`, `name`, `version`, `type`, `kind`, `description`, `capabilities`, `enabled`
  - `sdk`: `frontend` or `backend`
  - `runtime`: `vue-module`, `native-dylib`, or `manifest-only`
  - `entry`, `contributes`, `source`, `permissions`, `compat`, `status`
- Frontend preview registration is driven by runtime plugin manifests and `.momoplug` bundle loading; preview modules are read from the archive at runtime and do not enter the host frontend bundle.
- Backend plugins use a C ABI boundary with JSON request/response envelopes:
  - `momobako_plugin_manifest`
  - `momobako_plugin_call`
  - `momobako_plugin_free`
- The repository runtime discovers manifests at startup from `.momoplug` archives and routes filesystem backend operations through the plugin registry using canonical `momobako.*` plugin IDs.
- Native backend libraries are extracted from `.momoplug` into a controlled temporary cache before loading; plugin installation itself does not persistently extract archives.
- Filesystem backend `listFiles` responses carry both repository-relative paths and absolute local paths so the repository scanner can hash file content after plugin discovery. The runtime still resolves legacy responses that only include `relativePath`.
