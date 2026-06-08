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
