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

- Runtime plugins live under `plugins/builtin/*` in development. `yarn plugins:build` stages the runtime subset under `src-tauri/resources/plugins/builtin/*`, and release builds bundle it to `$RESOURCE/plugins/builtin/*`. The Tauri resource map intentionally avoids source-relative `../` paths so release packages do not expose `_up_/plugins/builtin`. When Tauri provides a resource directory, the runtime only scans `$RESOURCE/plugins/builtin` and does not fall back to cwd/source directories.
- Plugin manifest includes the existing display fields plus runtime fields:
  - `pluginId`, `legacyPluginIds`, `name`, `version`, `kind`, `description`, `capabilities`, `enabled`
  - `sdk`: `frontend` or `backend`
  - `runtime`: `vue-module`, `native-dylib`, or `manifest-only`
  - `entry`, `source`, `permissions`, `compat`, `status`
- Frontend plugins use `src/plugins/sdk.ts` and register Vue preview components with `definePreviewPlugin()` and `registerPreviewPlugin()`.
- Backend plugins use a C ABI boundary with JSON request/response envelopes:
  - `momobako_plugin_manifest`
  - `momobako_plugin_call`
  - `momobako_plugin_free`
- The repository runtime discovers manifests at startup from the runtime plugin directory, normalizes legacy IDs such as `builtin.local-filesystem`, and routes filesystem backend operations through the plugin registry. If the runtime plugin directory is removed, `GET /plugins` reflects that removal instead of silently rebuilding the list from compiled manifests.
- Built-in local filesystem is available as a trusted runtime backend loaded from its plugin directory. WebDAV, cloud drive, watcher, metadata provider, and vector index are separate manifest-only built-ins until their runtime implementations are added.
- Filesystem backend `listFiles` responses carry both repository-relative paths and absolute local paths so the repository scanner can hash file content after plugin discovery. The runtime still resolves legacy responses that only include `relativePath`.
