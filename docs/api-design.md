# Repository Service API

## Transport

- Primary transport: Tauri commands backed by an in-process repository runtime
- Runtime execution: blocking repository work runs on Tauri's blocking task pool

## Repository API

- `GET /repositories`
  - List registered repositories from `MetaHub/repositories.db`
  - Local filesystem repositories return `status: "ready"` when the registered path exists and `status: "missing"` when the local directory cannot be found.
- `POST /repositories`
  - Create a new repository or import an existing folder with `.momo`
- `POST /repositories/{repoId}:relocate`
  - Repair a missing local filesystem repository by pointing the existing `repoId` at a new local folder.
  - Request includes `repoId` and `path`.
  - The selected folder must contain `.momo/repository.json` whose `repoId` matches the request; folders without metadata or with a different `repoId` are rejected.
  - A successful response returns the updated repository summary and preserves existing repository identity, metadata and smart folders.
- `DELETE /repositories/{repoId}`
  - Remove a repository from registry without deleting user files.
  - Also clears application-managed state for that `repoId` when it lives under MomoBako's service storage.
- `POST /repositories/{repoId}:export`
  - Export repository to an archive or upload it to Git
  - Request:
    - `repoId`
    - `target`: `archive` | `git`
    - `archive.format`: `zip` | `7z` | `tar`
    - `archive.outputPath`
    - `archive.compression`: `none` | `fast` | `balanced` | `maximum`
    - `archive.encrypt` and optional `archive.password`
    - `git.remote`, `git.branch`, `git.message`
  - Response includes repository summary, target, output path or Git target, and result message
- `POST /repositories/{repoId}:sync`
  - Trigger incremental scan and emit filesystem events
  - Response keeps the existing `SyncResult` counters: `scannedFiles`, `createdAssets`, `updatedAssets`, `deletedAssets`, `createdEvents`, plus `hardlinkCandidates`
  - Desktop UI may expose local sync progress as phased client state (`scanning`, `writing`, `refreshing`, `complete`) without changing this transport contract
  - Sync updates repository indexes only; thumbnail generation is handled by the thumbnail API after content is visible
  - Newly discovered image assets automatically receive `metadata.color` as the primary `#RRGGBB` color and `metadata.palette` as up to five dominant `#RRGGBB` colors. Palette extraction failures are ignored so imports and syncs can continue.
  - Local filesystem scans store real `sha256:<hex>` content hashes on assets. When a newly discovered file has the same content hash as an existing active asset and is not already in a hardlink group, sync records a pending hardlink candidate instead of auto-linking it.
- File browser requests may include `specialLocation: "trash"` to browse `.momo/trash` without exposing internal repository directories in normal browsing.
- Trash browser entries include `metadata.deletedAt` and `metadata.originalPath` when they were moved by MomoBako.
- `deleteEntry` moves files or recursive directory deletes to `.momo/trash` by default. Use `mode: "permanentDelete"` only for deleting entries already shown from the trash view.
- `mutateTrash` supports `action: "restore" | "restoreAll" | "empty"` to restore a selected trash item, restore all tracked trash items, or clear `.momo/trash`.
- Eagle imports map `isDeleted: true` assets into the same recoverable trash model: the file is written under `.momo/trash`, `.momo/trash.json` stores `originalPath`, `trashPath`, `deletedAt` and `kind`, and the asset row keeps its original repository path with `status: "deleted"`.
- `POST /repositories/{repoId}/files:move`
  - Request body includes repository-relative `sourcePaths` and target `parentPath`
  - Moving into the original parent directory is rejected as a no-op.
  - Directories cannot be moved into themselves or any descendant directory.
- `POST /repositories/{repoId}/files:copy`
  - Request body includes repository-relative `sourcePaths`, optional `parentPath`, and optional `mode`: `hardlinkPreferred` | `copy`
  - Local filesystem repositories use `hardlinkPreferred` by default. File copies try to create hard links first and fall back to ordinary copies if the platform, filesystem, volume boundary, network location, or permissions reject the link.
  - Directory copies create the destination directory tree and apply the same per-file hardlink-preferred behavior recursively.
  - Successful hard links create or reuse `hardlink_groups` by content hash and record `hardlink_members`; fallback copies are recorded with `linkState: "copiedFallback"`.
- `GET /repositories/{repoId}/hardlinks:candidates`
  - Returns pending same-hash candidates discovered by sync.
- `POST /repositories/{repoId}/hardlinks:confirm`
  - Request body includes `candidateId`
  - Confirms a pending candidate and joins both assets into the same hardlink group only when their stored content hashes and sizes still match.
- `GET /repositories/{repoId}/snapshot`
  - `RepositorySnapshot` returns repository summary, folder summaries, indexed asset summaries, metadata field registry, overview, optional `quickAccess`, and optional `tagGroups`.
  - `quickAccess` entries expose `shortcutId`, `label`, `targetKind`, optional `targetPath`, and optional `targetId`, and can point to files, folders, or smart folders imported from Eagle.
  - `tagGroups` expose repository-level tag grouping metadata for the desktop tag editor; they do not replace per-asset searchable tags.
- `GET /repositories/{repoId}/actions`
  - Lists imported repository actions ordered by `sortOrder` and name.
  - Each action includes `source`, `sourceActionId`, `status`, `enabled`, steps, raw source JSON summary, unsupported reason, and last run.
- `GET /repositories/{repoId}/actions/{actionId}`
  - Reads one action and its steps.
- `PATCH /repositories/{repoId}/actions/{actionId}:enabled`
  - Enables or disables an action. Unsupported actions cannot be enabled.
- `POST /repositories/{repoId}/actions/{actionId}:run`
  - Request must explicitly include `assetIds` or repository-relative `targetPaths`; actions are never run during import.
  - Only ready, enabled actions with supported steps execute. Unsupported or disabled actions, missing targets, and invalid target paths are rejected before mutation.
  - Supported native steps currently update metadata and tag groups through the same revision path as manual metadata edits. Dangerous file operations remain disabled unless a core executor implements confirmation and audit for them.
- `POST /repositories/{repoId}/thumbnails:ensure`
  - Request body includes repository-relative `path`
  - Reuse an existing valid thumbnail cache entry or generate one for supported local image/video files
  - Optional `action`: `ensure`, `refresh`, `save`, `saveGenerated`, `clear`
  - `save` accepts `sourcePath` or `imageBytes` for custom file/folder thumbnails; `saveGenerated` accepts frontend-generated image bytes, used by 3D and text previews
  - Thumbnail cache files live under repository `.momo/thumbnails/` and use sha256 hex filenames
  - File thumbnails also backfill derived metadata such as `thumbnailPalette`
  - Response fields: `repoId`, `path`, `assetId`, `kind`, `thumbnailPath`, `thumbnailCustom`, optional `metadata`
- `POST /repositories/{repoId}/files:preparePreviewSource`
  - Request body includes repository-relative `path`
  - Response returns a session-scoped local preview `sourceUrl` backed by the in-process repository runtime
  - 3D and text previews use this source instead of returning full file bytes through the desktop command bridge
## Desktop Runtime State

- Workspace startup progress is a desktop UI state, not a repository service endpoint
- Startup progress fields: `status`, `stepLabel`, `currentStep`, `totalSteps`, `percent`, `error`
- Sync progress fields reserved for phased UI feedback: `phase`, `label`, `current`, `total`, `percent`

## Asset API

- `GET /repositories/{repoId}/assets/{assetId}`
  - Read asset summary, metadata and revision history
  - File browser and asset detail payloads may include `tags`, `aliasPaths`, `hardlinkGroupId`, `hardlinkState`, and `folderMetadata`.
  - `aliasPaths` lists additional repository-relative locations for Eagle multi-folder aliases; every alias remains a normal asset row and may be linked by hardlink or fallback copy.
  - `folderMetadata` currently carries `protected` and optional `passwordTip` as migration hints only; MomoBako does not store Eagle plaintext passwords and does not block folder access.
- `POST /repositories/{repoId}/assets/{assetId}:undo`
  - Reapply previous metadata snapshot
- `POST /repositories/{repoId}/assets/{assetId}:redo`
  - Reapply latest metadata snapshot

## Metadata API

- `PATCH /repositories/{repoId}/assets/{assetId}/metadata`
  - Request body includes `expectedVersion`
  - Generic file metadata keys include `rating`, `addedToLibraryAt`, `fileCreatedAt`, `fileModifiedAt`, `comment`, `link`, `width`, `height`, `originalSizeBytes`, `thumbnailPalette`, and `tagGroups`
  - Eagle imports preserve original source and timing fields in generic metadata: `url` becomes `link`, `annotation` becomes `comment`, import/create/modified timestamps become `addedToLibraryAt`, `fileCreatedAt`, and `fileModifiedAt`, and dimensions/size become `width`, `height`, and `originalSizeBytes`
  - Reads remain backward compatible with legacy `note`; when `comment` is empty, desktop clients and repository migration may fall back to `note`.
  - When `metadata.tagGroups` is supplied, the backend also synchronizes the flattened values into the searchable `tags` table.
  - Assets that belong to the same Eagle alias group propagate metadata updates together so duplicate paths stay behaviorally aligned.
  - Outcomes: `success`, `conflict`, `merged`

## Search API

- `POST /search`
  - Supports:
    - free text query
    - `repoId`
    - `tag`
    - `tags`
    - `metadataKey`
    - `metadataValue`
    - `metadataFilters`
    - `excludeQuery`
    - `excludePathPrefixes`
    - `excludeTags`
    - `excludeFormats`
    - `excludeMetadataFilters`
    - `excludeNumberFilters`
    - `excludeDateFilters`
    - `numberFilters`
    - `dateFilters`
    - `matchMode`
    - `sort`
    - `limit`
    - `formats`
    - `minRating`
  - `tags` and `formats` match with OR semantics inside each field; different filter fields combine with AND semantics.
  - `metadataFilters` accepts key/value pairs such as `color` and `shape`; values are matched against metadata text.
  - `exclude*` filters remove matches after inclusion filters are applied. `excludeQuery` matches against the same text haystack as `query`; `excludePathPrefixes` removes matching repository-relative path prefixes.
  - `numberFilters` support numeric ranges such as `width=1024..4096` or `originalSizeBytes=..10485760`.
  - `dateFilters` support ISO timestamp ranges such as `fileCreatedAt=2024-01-01T00:00:00Z..2024-12-31T23:59:59Z`.
  - `matchMode: "or"` allows smart-folder-style any-match logic across populated include filters; the default remains AND semantics.
  - `sort.field` accepts built-in fields such as `filename`, `path`, `rating`, `sizeBytes`, `modifiedAt`, and metadata fields such as `metadata.width`, `metadata.fileCreatedAt`, and `metadata.addedToLibraryAt`.
  - `limit` truncates the result set after sorting.
  - Desktop resource filtering sends the current `repoId` and may search with an empty free text query.

## Smart Folder API

- Smart folders are repository-scoped virtual filter templates stored under `.momo/metadata.db`; they never create or mutate real directories.
- `GET /repositories/{repoId}/smart-folders`
  - Returns nested `SmartFolderTreeNode[]` ordered by parent and `sortOrder`.
- `POST /repositories/{repoId}/smart-folders`
  - Creates a smart folder with `name`, optional `parentId`, and `filter`.
- `PATCH /repositories/{repoId}/smart-folders/{smartFolderId}`
  - Updates name, parent and filter.
- `DELETE /repositories/{repoId}/smart-folders/{smartFolderId}`
  - Deletes the selected template and child templates only; repository files are untouched.
- `POST /repositories/{repoId}/smart-folders/{smartFolderId}:query`
  - Returns file-list entries for the selected smart folder.
  - Child smart folders inherit parent filters with AND semantics.
  - Supported filters: `query`, `pathPrefix`, `excludeQuery`, `excludePathPrefixes`, `tags`, `formats`, `colors`, `shapes`, `metadataFilters`, `excludeTags`, `excludeFormats`, `excludeMetadataFilters`, `excludeNumberFilters`, `excludeDateFilters`, `numberFilters`, `dateFilters`, `matchMode`, `sort`, `limit`, and `minRating`.
  - Eagle smart folders now map OR logic, exclusion rules, date ranges, numeric ranges, sort order, and result limits without being silently skipped.

## Eagle Import Notes

- Eagle multi-folder ownership is imported as one primary asset plus additional alias asset rows. Alias files attempt hard-link creation first and fall back to normal copies when the filesystem rejects linking.
- Alias rows are tracked in `asset_alias_groups` / `asset_alias_members`; hardlink or copy state continues to use the existing hardlink tables.
- Eagle `quickAccess` is imported into repository shortcuts, and Eagle `tagsGroups` becomes repository-level tag groups.
- Eagle `actions.json` is imported into repository actions and steps. Recognized metadata/tag steps become MomoBako native steps; unknown or dangerous steps are preserved as `unsupported`, keep their raw payload, and disable the containing action by default.
- Eagle folder passwords are explicitly out of scope. MomoBako does not read, store, hash, export, or enforce Eagle plaintext `password`; it only stores `protected=true` and optional `passwordTip` for display as migration hints.

## Plugin API

- `GET /plugins`
  - List runtime-discovered plugin manifests and capabilities
  - Runtime discovery scans `<serviceRoot>/plugins/*.momoplug`; missing or deleted archive files are reflected directly in the response and are not replaced by compiled defaults
  - Manifest fields include `pluginId`, `legacyPluginIds`, `name`, `version`, `type`, `kind`, `category`, `description`, `capabilities`, `enabled`, `sdk`, `entry`, `source`, `runtime`, `permissions`, `requires`, `optional`, `hooks`, `contributes`, `compat`, and `status`
  - `category` is one of `source`, `library-kind`, `parser`, `preview`, or `service`; legacy manifests without `category` are inferred from `kind`.
  - `source` plugins are attachable repository IO backends. Existing `filesystem`, `webdav`, and `cloud` kinds remain accepted as source plugins for compatibility.
  - `library-kind` plugins declare content fields, facets, view presets, organization rules and declarative core-host hooks for content types. Official manifest-only library kinds include audio, ASMR, video, anime, manga, ebook, image, design, 3D model, font, game, software, archive and project.
  - `parser` plugins declare extraction targets and normalized candidate outputs for concrete file/container types; parser output enters the candidate queue rather than directly writing metadata.
  - `preview` plugins render file previews and thumbnails independently of library-kind semantics.
  - `service` plugins expose shared capabilities such as metadata providers, network search, download queues, filesystem watching and vector search. External/network services are manual-trigger and candidate-only unless a future runtime implementation changes the contract.
  - `hooks` declare how plugins attach to core-hosted capabilities such as playlist, PiP, progress, candidate queue, batch organize, download queue, metadata merge, rename/move execution, audit log and unified search.
  - Backend plugin IDs are normalized to the `momobako.*` namespace; legacy `builtin.*` IDs remain accepted when reading existing repositories
  - Disabled or manifest-only source backends are displayed but not offered as usable repository backends until enabled with an available runtime
  - Filesystem backend `listFiles` responses include `absolutePath`, `relativePath`, `filename`, `extension`, `sizeBytes`, and `modifiedAt`; the runtime tolerates legacy responses without `absolutePath` by resolving `relativePath` under `repoRoot`
- `POST /plugins:install`
  - Request body includes `packagePath`
  - Only `.momoplug` files are accepted
  - Install copies the archive to `<serviceRoot>/plugins` and refreshes discovery without persistent extraction
- `POST /plugins:call`
  - Request body includes `pluginId`, `method`, and arbitrary JSON `payload`
  - Used by frontend preview or codec plugins to invoke native plugin capabilities without adding file-format-specific commands to the core runtime
- `POST /files:writeBinary`
  - Request body includes absolute `path` and raw `bytes`
  - Used by plugins for export flows such as writing decoded media chosen through a save dialog

## Cache API

- `GET /cache`
  - Return cache capacities and recent entries for metadata, thumbnail and query caches
