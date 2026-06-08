# Repository Service API

## Transport

- Primary transport: local REST-style service boundary exposed through Tauri commands
- Compatibility target: gRPC-ready contracts with stable request/response models

## Repository API

- `GET /repositories`
  - List registered repositories from `MetaHub/repositories.db`
- `POST /repositories`
  - Create a new repository or import an existing folder with `.momo`
- `DELETE /repositories/{repoId}`
  - Remove a repository from registry without deleting user files
- `POST /repositories/{repoId}:export`
  - Export repository descriptor for external tools
- `POST /repositories/{repoId}:sync`
  - Trigger incremental scan and emit filesystem events
  - Response keeps the existing `SyncResult` counters: `scannedFiles`, `createdAssets`, `updatedAssets`, `deletedAssets`, `createdEvents`
  - Desktop UI may expose local sync progress as phased client state (`scanning`, `writing`, `refreshing`, `complete`) without changing this transport contract

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
    - `metadataKey`
    - `metadataValue`
    - `minRating`

## Plugin API

- `GET /plugins`
  - List plugin manifests and capabilities

## Cache API

- `GET /cache`
  - Return cache capacities and recent entries for metadata, thumbnail and query caches
