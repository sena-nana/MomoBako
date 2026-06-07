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

## Plugin Architecture

- Plugin manifest includes:
  - `pluginId`
  - `name`
  - `version`
  - `kind`
  - `description`
  - `capabilities`
  - `enabled`
- Extension points:
  - filesystem watcher
  - metadata provider
  - semantic/vector search
  - OCR / AI tagging / sync adapters
