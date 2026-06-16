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
  - `repository_actions`
  - `repository_action_steps`
  - `repository_action_runs`
  - `repository_action_run_steps`
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

## Repository Actions

- Imported Eagle `actions.json` records live in repository-local action tables.
- Import never executes actions. It stores original action JSON, normalized steps, unsupported reasons and enabled state.
- Ready steps can be executed only when the user supplies explicit target asset IDs or paths. Supported metadata/tag steps reuse the normal metadata revision path.
- Unsupported or dangerous steps are preserved for auditability and keep the containing action disabled until a core executor can safely confirm and audit that class of write.

## Search Strategy

- Current implementation performs structured search across:
  - filename
  - path
  - status
  - tags
  - metadata values
- Inclusion filters run first. Exclusion filters then remove matching rows by text query, path prefixes, tags, formats, metadata key/value, numeric ranges, or date ranges.
- Core search and file browsing own common sorting behavior, including `random`; library-kind plugins only surface shortcuts that call these core sort fields.
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
- Plugin manifest includes the existing display fields plus taxonomy, runtime and contribution fields:
  - `pluginId`, `legacyPluginIds`, `name`, `version`, `type`, `kind`, `category`, `description`, `capabilities`, `enabled`
  - `sdk`: `frontend` or `backend`
  - `runtime`: `vue-module`, `native-dylib`, or `manifest-only`
  - `entry`, `source`, `permissions`, `requires`, `optional`, `hooks`, `contributes`, `compat`, `status`
- `category` defines the plugin responsibility layer:
  - `source` provides repository IO such as list/read/write/move/delete/watch. Local filesystem, WebDAV and cloud drive are source plugins.
  - `library-kind` declares content semantics, metadata fields, search facets, default views, organization rules and core host hooks for resource types such as audio, ASMR, anime, manga, fonts and software.
  - `parser` declares file or container metadata extraction outputs by extension/MIME/probe result. Parser plugins only produce normalized candidates.
  - `preview` renders files and thumbnails. Library-kind plugins can prefer preview plugins but do not own preview rendering.
  - `service` provides shared external or background capabilities such as network search, metadata providers, download queues, OCR/ASR, vector search or filesystem watching.
- Core-hosted capabilities such as playlist, PiP, progress, candidate queue, batch organize, download queue, metadata merge, rename/move execution, audit log and unified search are exposed through declarative `hooks`. Plugins contribute data and actions; core owns state, confirmation and dangerous writes.
- Rich library-kind support follows that split: a frontend library plugin registers workspace UI and behavior through `registerLibraryExtension`; parser or support plugins can provide non-destructive sync defaults through `metadata.defaults.batch`; provider plugins expose manual candidate lookup through `provider.lookupMetadataCandidate`. The host owns only these generic extension points and metadata writes still flow through the normal revision path.
- Frontend preview registration is driven by runtime plugin manifests and `.momoplug` bundle loading; preview modules are read from the archive at runtime and do not enter the host frontend bundle.
- Backend plugins use a C ABI boundary with JSON request/response envelopes:
  - `momobako_plugin_manifest`
  - `momobako_plugin_call`
  - `momobako_plugin_free`
- The repository runtime discovers manifests at startup from `.momoplug` archives and routes filesystem backend operations through the plugin registry using canonical `momobako.*` plugin IDs.
- Native backend libraries are extracted from `.momoplug` into a controlled temporary cache before loading; plugin installation itself does not persistently extract archives.
- Plugin-owned persistent files live under `.service-data/plugin-data/<pluginSlug>` (`<serviceRoot>/plugin-data/<pluginSlug>` at runtime). The host creates the directory on demand for frontend plugin settings entry points and before native backend plugin calls, then passes it as `runtime.pluginDataDir` with the current `runtime.pluginConfig` key-value snapshot.
- Plugin configuration uses the same directory and plugin ID normalization path. `contributes.settings` declares optional schema fields and a settings page contribution; the plugin manager opens one settings entry per plugin, renders a registered custom Vue page when available, falls back to the schema form, and stores host-managed key-value config in `config.json`.
- The runtime infers `category` for legacy manifests that only declare `kind`, normalizes legacy IDs such as `builtin.local-filesystem`, and reflects runtime plugin directory changes directly in `GET /plugins`.
- Filesystem backend `listFiles` responses carry both repository-relative paths and absolute local paths so the repository scanner can hash file content after plugin discovery. The runtime still resolves legacy responses that only include `relativePath`.

## Refactor Slices

- Workspace pages follow an MVVM-oriented split: page SFCs keep visual composition, while `src/pages/workspace/*ViewModel.ts` files own state assembly, dialog promises, auth status and component bindings.
- `Home.vue` is the workspace view shell. `useWorkspaceHomeViewModel` composes repository, file, playlist, search, action and dialog state without changing Tauri command names or serialized DTO fields.
- File browser pointer selection, drag intent and selection summaries live in `useFileBrowserPanelViewModel`; `FileBrowserPanel.vue` keeps its props, emits and template surface.
- Repository API transport is grouped under `src/services/repositoryApi/` by domain and re-exported through `src/services/repositoryApi.ts` for existing imports.
- `src-tauri/src/repository_service.rs` remains the public facade for commands. Plugin-facing `RepositoryState` behavior is delegated to `repository_service/plugins.rs` as the first backend domain slice.
