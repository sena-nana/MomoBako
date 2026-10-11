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
- Thumbnail cache files use sha256 hex filenames. The backend generates thumbnails for images, videos and audio covers; custom thumbnails (local file, remote URL or image bytes) are saved through the thumbnail API.
- The in-process repository runtime still serves session-scoped loopback URLs (`sourceUrl`) for prepared preview and playback sources. The Nana host reads file bytes in process through the query ViewModel and does not use these URLs.

## Plugin Architecture

- Plugin source projects live under `External/Plugins/*`. All Rust plugins are members of the repository root Cargo workspace and share one `Cargo.lock`, dependency graph and `target/`; the build script submits selected native packages to Cargo in one invocation.
- Bundled runtime plugins live under `<serviceRoot>/plugins/builtin/*.momoplug`; installed user plugins live under `<serviceRoot>/plugins/user/*.momoplug`. Root-level archives are read only as legacy packages.
- The Nana release package (`nana-packager`, see [Nana 发布包](./nana-packaging.md)) carries the packaged plugins (`External/Plugins/.packages/*.momoplug`) in NanaUI's `runtime/plugins`. On startup `src-nana/src/plugin_host.rs` syncs the plugins listed in the package manifest into `plugins/builtin` through `momobako_backend::services::runtime::bundled_plugins` (in development it reads `External/Plugins/.packages`) and removes builtin packages that are no longer bundled, then loads them and installs the plugin host. `cargo check` and `yarn verify` never require a plugin build or package.
- New packages use `momobako.package.json` format v2. The envelope binds `pluginId`, `version`, `targetTriple`, deployment, product/runtime manifests and every executable artifact hash. The host computes the whole-package hash and owns `provenance`, `trustLevel` and `source` instead of trusting the product manifest.
- Plugin manifest includes the existing display fields plus taxonomy, runtime and contribution fields:
  - `pluginId`, `legacyPluginIds`, `name`, `version`, `type`, `kind`, `category`, `description`, `capabilities`, `enabled`
  - `sdk`: always `backend`
  - `runtime`: `native-dylib`, `process`, or `manifest-only`
  - `entry`, `source`, `permissions`, `requires`, `optional`, `hooks`, `contributes`, `compat`, `status`
- Plugins never ship UI code. The removed frontend plugins (`sdk: "frontend"`, `runtime: "vue-module"`) are rejected when a package is read; the host draws previews, players, tool pages, settings forms and library panels natively from the manifest declarations (see [API 设计](./api-design.md) Plugin API).
- `category` defines the plugin responsibility layer:
  - `source` provides repository IO such as list/read/write/move/delete/watch. Local filesystem, WebDAV and cloud drive are source plugins.
  - `library-kind` declares content semantics, metadata fields, search facets, default views, organization rules and core host hooks for resource types such as audio, ASMR, anime, manga, fonts and software.
  - `parser` declares file or container metadata extraction outputs by extension/MIME/probe result. Parser plugins only produce normalized candidates.
  - `preview` declares preview and playback capabilities, such as the playlist types the host plays. The host renders previews natively by file extension; library-kind plugins do not own preview rendering.
  - `service` provides shared external or background capabilities such as network search, metadata providers, download queues, OCR/ASR, vector search or filesystem watching.
- Core-hosted capabilities such as playlist, PiP, progress, candidate queue, batch organize, download queue, metadata merge, rename/move execution, audit log and unified search are exposed through declarative `hooks`. Plugins contribute data and actions; core owns state, confirmation and dangerous writes.
- Rich library-kind support follows that split: library-kind plugins declare search shortcuts through `contributes.libraryExtension` and the host draws the library panels (the ASMR work queue and candidate lookup are host features); parser or support plugins can provide non-destructive sync defaults through `metadata.defaults.batch`; source plugins can declare `contributes.source.metadataMirrorKeys` for system metadata mirroring; provider plugins expose manual candidate lookup through `provider.lookupMetadataCandidate`. The host owns only these generic extension points and metadata writes still flow through the normal revision path.
- Executable backend packages carry both the Momo-facing `manifest.json` and a native Mutsuki `plugin.toml`; their plugin IDs and versions must match.
- Trusted bundled backend plugins may export Mutsuki ABI v2 runners. User-installed native code must use `process` deployment; the first process pilot is `momobako.service.archive-preview`, using a JSONL request/response bridge with crash and hard-timeout process termination.

### Cooperative cancellation for long tasks

- MomoBako's built-in long-task runner uses the Host-owned `NativeRunner` cancellation probe. Repository code only sees a read-only domain cancellation interface; no cancellation handle or registry is added to the wire schema.
- "Cancellation requested" means the probe is set. The worker may still be waiting for a repository write lock, finishing the current atomic batch item, synchronizing the watcher/index, or returning from an external plugin call.
- "Worker finalizing" is non-terminal. Core publishes the cancelled terminal state only after the worker has stopped and discards any result or progress events produced after cancellation was requested.
- Batch operations use one item as the atomic boundary. Completed items remain on disk and in the index, while unstarted items have no side effects. File copies and archive extraction write to temporary siblings and publish with a rename only after the complete file is written.
- Native filesystem loops check cancellation between items and I/O chunks. External plugin calls cannot be interrupted in-process; cancellation is observed as soon as the call returns, after any required repository consistency cleanup.
- Hosts request cancellation through `MutsukiTaskViewModel::cancel` and wait for the real terminal outcome. The Nana task popover has no cancel entry yet.

- The repository runtime caches a catalog snapshot and invalidates it from plugin directory/config metadata, so normal plugin calls no longer reopen every ZIP. It maps `callPlugin` methods to declared `momobako.*` protocols and target bindings.
- Executable artifacts are staged atomically into a whole-package-hash-isolated runtime cache. Candidate generations are fully loaded before swap; any load failure keeps the current generation, while a successful swap drains calls before disposing ABI sessions or terminating process runners. Companion artifacts such as the Office helper are declared in `plugin.toml` instead of extracted by product-specific host code.
- Plugin-owned persistent files live under `.service-data/plugin-data/<pluginSlug>` (`<serviceRoot>/plugin-data/<pluginSlug>` at runtime). The host creates the directory on demand when the plugin manager opens it and before native backend plugin calls, then passes it as `runtime.pluginDataDir` with the current `runtime.pluginConfig` key-value snapshot.
- Plugin configuration uses the same directory and plugin ID normalization path. `contributes.settings` declares optional schema fields and settings page metadata; the plugin manager opens one settings entry per plugin, renders the schema form, and stores host-managed key-value config in `config.json`.
- The runtime infers `category` for legacy manifests that only declare `kind`, normalizes legacy IDs such as `builtin.local-filesystem`, and reflects runtime plugin directory changes directly in `GET /plugins`.
- Filesystem backend `listFiles` responses carry both repository-relative paths and absolute local paths so the repository scanner can hash file content after plugin discovery. The runtime still resolves legacy responses that only include `relativePath`.

## UI Structure

- The desktop UI is the NanaUI shell in `src-nana` (crate `momobako-nana`). `ShellViewModel` is the single state source; messages are reduced, side effects go to the domain ViewModels of `momobako-backend` through `NativeServices`, and `ShellView::sync` writes the results into resident views. See [Nana 壳层绑定式视图](./nana-shell-view.md) and [Nana 迁移边界](./nana-migration.md).
- The former Vue UI (page SFCs with `*ViewModel.ts` composables) and the Tauri command facade were removed; the Vue/Tauri sources last appear in commit `7bc847c`.
