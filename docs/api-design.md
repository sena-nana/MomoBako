# Repository Service API

## Transport

- Primary transport: Tauri commands backed by an in-process repository runtime
- Runtime execution: blocking repository work runs on Tauri's blocking task pool

## Repository API

- `GET /repositories`
  - List registered repositories from `MetaHub/repositories.db`
- `POST /repositories`
  - Create a new repository or import an existing folder with `.momo`
- `DELETE /repositories/{repoId}`
  - Remove a repository from registry without deleting user files
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
  - Local filesystem scans store real `sha256:<hex>` content hashes on assets. When a newly discovered file has the same content hash as an existing active asset and is not already in a hardlink group, sync records a pending hardlink candidate instead of auto-linking it.
- File browser requests may include `specialLocation: "trash"` to browse `.momo/trash` without exposing internal repository directories in normal browsing.
- Trash browser entries include `metadata.deletedAt` and `metadata.originalPath` when they were moved by MomoBako.
- `deleteEntry` moves files or recursive directory deletes to `.momo/trash` by default. Use `mode: "permanentDelete"` only for deleting entries already shown from the trash view.
- `mutateTrash` supports `action: "restore" | "restoreAll" | "empty"` to restore a selected trash item, restore all tracked trash items, or clear `.momo/trash`.
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
- `POST /repositories/{repoId}/thumbnails:ensure`
  - Request body includes repository-relative `path`
  - Reuse an existing valid thumbnail cache entry or generate one for supported local image/video files
  - Optional `action`: `ensure`, `refresh`, `save`, `saveGenerated`, `clear`
  - `save` accepts `sourcePath` or `imageBytes` for custom file/folder thumbnails; `saveGenerated` accepts frontend-generated image bytes, used by 3D and text previews
  - Thumbnail cache files live under repository `.momo/thumbnails/` and use sha256 hex filenames
  - Response fields: `repoId`, `path`, `assetId`, `kind`, `thumbnailPath`, `thumbnailCustom`
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
- `POST /repositories/{repoId}/assets/{assetId}:undo`
  - Reapply previous metadata snapshot
- `POST /repositories/{repoId}/assets/{assetId}:redo`
  - Reapply latest metadata snapshot

## Metadata API

- `PATCH /repositories/{repoId}/assets/{assetId}/metadata`
  - Request body includes `expectedVersion`
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
    - `formats`
    - `minRating`
  - `tags` and `formats` match with OR semantics inside each field; different filter fields combine with AND semantics.
  - `metadataFilters` accepts key/value pairs such as `color` and `shape`; values are matched against metadata text.
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
  - Supported filters: `query`, `pathPrefix`, `tags`, `formats`, `colors`, `shapes`, `metadataFilters`, and `minRating`.

## Plugin API

- `GET /plugins`
  - List runtime-discovered plugin manifests and capabilities
  - Release builds discover built-in plugins from `$RESOURCE/plugins/builtin`; missing or deleted plugin directories are reflected in the response instead of being replaced by compiled defaults or cwd/source fallbacks
  - Manifest fields include `pluginId`, `legacyPluginIds`, `name`, `version`, `kind`, `description`, `capabilities`, `enabled`, `sdk`, `entry`, `source`, `runtime`, `permissions`, `compat`, and `status`
  - Backend plugin IDs are normalized to the `momobako.*` namespace; legacy `builtin.*` IDs remain accepted when reading existing repositories
  - Disabled or manifest-only filesystem backends are displayed but not offered as attachable repository backends
  - Filesystem backend `listFiles` responses include `absolutePath`, `relativePath`, `filename`, `extension`, `sizeBytes`, and `modifiedAt`; the runtime tolerates legacy responses without `absolutePath` by resolving `relativePath` under `repoRoot`

## Cache API

- `GET /cache`
  - Return cache capacities and recent entries for metadata, thumbnail and query caches
