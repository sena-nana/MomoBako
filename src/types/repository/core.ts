export type RepositoryBackendSummary = {
  pluginId: string;
  kind: string;
  name: string;
  capabilities: string[];
};

export type RepositoryBackendOption = RepositoryBackendSummary & {
  description: string;
  enabled: boolean;
};

export type RepositoryStatus = "ready" | "missing";

export type RepositoryLocalCacheStatus = {
  required: boolean;
  path?: string | null;
  status: "ready" | "missing" | "unconfigured";
};

export type RepositorySummary = {
  repoId: string;
  name: string;
  path: string;
  backend: RepositoryBackendSummary;
  status: RepositoryStatus;
  assetCount: number;
  updatedAt: string;
  localCache?: RepositoryLocalCacheStatus | null;
};

export type AssetSummary = {
  assetId: string;
  repoId: string;
  path: string;
  filename: string;
  extension: string;
  sizeBytes: number;
  sizeLabel: string;
  status: string;
  modifiedAt: string;
  lastAccessedAt?: string | null;
  version: number;
  tags: string[];
  thumbnailPath?: string | null;
  hardlinkGroupId?: string | null;
  hardlinkState?: HardlinkState | null;
  isVirtual?: boolean;
  providerId?: string | null;
  providerItemId?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  localAbsolutePath?: string | null;
};

export type FolderSummary = {
  path: string;
  label: string;
  assetCount: number;
};

export type RepositoryShortcut = {
  shortcutId: string;
  label: string;
  targetKind: "file" | "folder" | "smartFolder" | string;
  targetPath?: string | null;
  targetId?: string | null;
};

export type RepositoryTagGroup = {
  tagGroupId: string;
  name: string;
  tags: string[];
};

export type PlaylistFileClass = "image" | "audio" | "video" | string;

export type PlaylistItemStatus =
  | "ready"
  | "missing"
  | "deleted"
  | "trashed"
  | "incompatible"
  | "pluginUnavailable";

export type PlaylistPlaybackMode = "listLoop" | "shuffle" | "singleLoop";

export type PlaylistSummary = {
  playlistId: string;
  repoId: string;
  name: string;
  playerTypeId: string;
  playerPluginId: string;
  playerLabel: string;
  fileClass: PlaylistFileClass;
  itemCount: number;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};

export type PlaylistItem = {
  playlistItemId: string;
  playlistId: string;
  assetId: string;
  path: string;
  filename: string;
  extension: string;
  thumbnailPath?: string | null;
  status: PlaylistItemStatus;
  statusReason?: string | null;
  sortOrder: number;
  addedAt: string;
  isVirtual?: boolean;
  providerId?: string | null;
  providerItemId?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  localAbsolutePath?: string | null;
};

export type PlaylistDetail = {
  playlist: PlaylistSummary;
  items: PlaylistItem[];
};

export type FolderMetadata = {
  protected: boolean;
  passwordTip?: string | null;
};

export type RepositorySnapshot = {
  repository: RepositorySummary;
  folderLabel: string;
  folders: FolderSummary[];
  assets: AssetSummary[];
  playlists?: PlaylistSummary[];
  quickAccess?: RepositoryShortcut[];
  tagGroups?: RepositoryTagGroup[];
  metadataFields: string[];
  recentRevisionCount: number;
  overview: {
    totalSizeBytes: number;
    totalSizeLabel: string;
    fileCount: number;
    folderCount: number;
    trashCount: number;
    readmeContent: string | null;
  };
};

export type FileTreeNode = {
  path: string;
  label: string;
  fileCount: number;
  children: FileTreeNode[];
};

export type RepositoryStructureCacheState = "warming" | "ready" | "refreshing";

export type RepositoryStructureUpdatedEvent = {
  repoId: string;
  reason: "watcher" | "cache-miss" | "manual";
  indexedAt: string | null;
};

export type RepositoryTreeSnapshot = {
  repoId: string;
  rootPath: string;
  backendPluginId: string;
  backendKind: string;
  cacheState: RepositoryStructureCacheState;
  indexedAt: string | null;
  tree: FileTreeNode[];
};

export type FileBrowserEntry = {
  path: string;
  name: string;
  kind: "directory" | "file";
  extension?: string | null;
  sizeBytes?: number | null;
  sizeLabel?: string | null;
  modifiedAt?: string | null;
  assetId?: string | null;
  status?: string | null;
  thumbnailPath?: string | null;
  thumbnailCustom?: boolean;
  hardlinkGroupId?: string | null;
  hardlinkState?: HardlinkState | null;
  tags?: string[];
  aliasPaths?: string[];
  folderMetadata?: FolderMetadata | null;
  metadata?: Record<string, unknown>;
  isVirtual?: boolean;
  providerId?: string | null;
  providerItemId?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  localAbsolutePath?: string | null;
};

export type MetadataTagGroup = string;

export type FileBrowserSnapshot = {
  repoId: string;
  rootPath: string;
  backendPluginId: string;
  backendKind: string;
  cacheState: RepositoryStructureCacheState;
  indexedAt: string | null;
  currentPath: string;
  totalEntries: number;
  loadedCount: number;
  nextOffset?: number | null;
  hasMore: boolean;
  specialLocation?: "trash" | null;
  tree?: FileTreeNode[];
  entries: FileBrowserEntry[];
};

export type MetadataEntry = {
  key: string;
  valueType: string;
  value: unknown;
  version: number;
  updatedAt: string;
};

export type RevisionEntry = {
  revisionId: string;
  assetId: string;
  timestamp: string;
  operation: string;
  before: Record<string, unknown>;
  after: Record<string, unknown>;
  source: string;
};

export type AssetDetail = {
  summary: AssetSummary;
  metadata: MetadataEntry[];
  revisions: RevisionEntry[];
};

export type SearchHit = {
  repoId: string;
  repoName: string;
  assetId: string;
  path: string;
  filename: string;
  status: string;
  tags: string[];
  metadata: Record<string, unknown>;
  isVirtual?: boolean;
  providerId?: string | null;
  providerItemId?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  localAbsolutePath?: string | null;
};

export type SearchResponse = {
  query: string;
  results: SearchHit[];
};

export type SearchMetadataFilter = {
  key: string;
  value: string;
};

export type SearchNumberFilter = {
  key: string;
  min?: number;
  max?: number;
};

export type SearchDateFilter = {
  key: string;
  from?: string;
  to?: string;
};

export type SearchSort = {
  field: string;
  direction: "asc" | "desc" | string;
};

export type SearchRequest = {
  query: string;
  repoId?: string;
  excludeQuery?: string;
  metadataKey?: string;
  metadataValue?: string;
  tag?: string;
  tags?: string[];
  metadataFilters?: SearchMetadataFilter[];
  excludeTags?: string[];
  excludeFormats?: string[];
  excludeMetadataFilters?: SearchMetadataFilter[];
  excludePathPrefixes?: string[];
  excludeNumberFilters?: SearchNumberFilter[];
  excludeDateFilters?: SearchDateFilter[];
  numberFilters?: SearchNumberFilter[];
  dateFilters?: SearchDateFilter[];
  formats?: string[];
  minRating?: number;
  matchMode?: "and" | "or" | string;
  sort?: SearchSort;
  limit?: number;
};

export type SmartFolderFilter = {
  query?: string;
  pathPrefix?: string;
  excludeQuery?: string;
  excludePathPrefixes?: string[];
  tags?: string[];
  formats?: string[];
  colors?: string[];
  shapes?: string[];
  metadataFilters?: SearchMetadataFilter[];
  excludeTags?: string[];
  excludeFormats?: string[];
  excludeMetadataFilters?: SearchMetadataFilter[];
  excludeNumberFilters?: SearchNumberFilter[];
  excludeDateFilters?: SearchDateFilter[];
  numberFilters?: SearchNumberFilter[];
  dateFilters?: SearchDateFilter[];
  minRating?: number;
  matchMode?: "and" | "or" | string;
  sort?: SearchSort;
  limit?: number;
};

export type SmartFolder = {
  smartFolderId: string;
  repoId: string;
  parentId?: string | null;
  name: string;
  filter: SmartFolderFilter;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};

export type SmartFolderTreeNode = SmartFolder & {
  children: SmartFolderTreeNode[];
};

export type SmartFolderMutationRequest = {
  repoId: string;
  smartFolderId?: string;
  parentId?: string | null;
  name: string;
  filter: SmartFolderFilter;
};

export type SmartFolderUpdateRequest = SmartFolderMutationRequest & {
  smartFolderId: string;
};

export type SmartFolderMutationResponse = {
  smartFolders: SmartFolderTreeNode[];
  smartFolder?: SmartFolder | null;
};

export type RepositoryActionStep = {
  stepId: string;
  actionId: string;
  repoId: string;
  stepKind: string;
  label: string;
  status: "ready" | "unsupported" | string;
  config: Record<string, unknown> | unknown;
  raw: Record<string, unknown> | unknown;
  unsupportedReason?: string | null;
  sortOrder: number;
};

export type RepositoryActionRun = {
  runId: string;
  actionId: string;
  repoId: string;
  status: "running" | "success" | "failed" | string;
  target: Record<string, unknown> | unknown;
  message?: string | null;
  startedAt: string;
  finishedAt?: string | null;
};

export type RepositoryAction = {
  actionId: string;
  repoId: string;
  source: string;
  sourceActionId?: string | null;
  name: string;
  status: "ready" | "unsupported" | string;
  enabled: boolean;
  raw: Record<string, unknown> | unknown;
  unsupportedReason?: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  steps: RepositoryActionStep[];
  lastRun?: RepositoryActionRun | null;
};

export type RepositoryActionRunRequest = {
  repoId: string;
  actionId: string;
  targetPaths?: string[];
  assetIds?: string[];
};

export type RepositoryActionRunResponse = {
  action: RepositoryAction;
  run: RepositoryActionRun;
};

export type RepositoryActionEnabledRequest = {
  repoId: string;
  actionId: string;
  enabled: boolean;
};

export type RepositoryActionMutationResponse = {
  action: RepositoryAction;
};

export type PlaylistMutationRequest = {
  repoId: string;
  playlistId?: string;
  name: string;
  playerTypeId: string;
};

export type PlaylistUpdateRequest = {
  repoId: string;
  playlistId: string;
  name?: string;
  playerTypeId?: string;
};

export type PlaylistMutationResponse = {
  playlists: PlaylistSummary[];
  playlist?: PlaylistSummary | null;
};

export type PlaylistDetailRequest = {
  repoId: string;
  playlistId: string;
};

export type PlaylistItemsAddRequest = {
  repoId: string;
  playlistId: string;
  assetIds: string[];
};

export type PlaylistItemsByPathsAddRequest = {
  repoId: string;
  playlistId: string;
  paths: string[];
};

export type PlaylistItemsOrderRequest = {
  repoId: string;
  playlistId: string;
  itemIds: string[];
};

export type PlaylistItemRemoveRequest = {
  repoId: string;
  playlistId: string;
  playlistItemId: string;
};

export type PlaylistMembershipRequest = {
  repoId: string;
  assetId: string;
  playlistIds: string[];
};

export type PlaylistMembershipSnapshot = {
  assetId: string;
  playlistIds: string[];
};

export type PlaylistMembershipIndex = {
  memberships: Record<string, string[]>;
};

export type SmartFolderResultSnapshot = {
  repoId: string;
  smartFolder: SmartFolder;
  inheritedFilter: SmartFolderFilter;
  results: FileBrowserEntry[];
};

export type MetadataUpdateRequest = {
  repoId: string;
  assetId: string;
  expectedVersion: number;
  metadata: Record<string, unknown>;
  source?: string;
};

export type MetadataUpdateResponse = {
  outcome: "success" | "conflict" | "merged";
  asset: AssetDetail;
};

export type RepositoryMutationRequest = {
  repoId?: string;
  name: string;
  path: string;
  backendPluginId?: string;
  backendConfig?: Record<string, unknown>;
  skipInitialSync?: boolean;
};

export type RepositoryFolderRequest = {
  path: string;
};

export type RepositoryRelocateRequest = {
  repoId: string;
  path: string;
};

export type RepositoryBackendConfigUpdateRequest = {
  repoId: string;
  backendConfig: Record<string, unknown>;
};

export type NeteaseRepositoryCacheConfigureRequest = {
  repoId: string;
  path: string;
  migrateLegacyCache?: boolean;
};

export type NeteaseRepositoryCacheMigrationSummary = {
  movedStateFiles: number;
  migratedPlaybackCacheFiles: number;
  skippedPlaybackCacheFiles: number;
  failedPlaybackCacheFiles: number;
};

export type NeteaseRepositoryCacheConfigureResponse = {
  repository: RepositorySummary;
  migration: NeteaseRepositoryCacheMigrationSummary;
};

export type RepositoryExportTarget = "archive" | "git";

export type RepositoryArchiveFormat = "zip" | "7z" | "tar";

export type RepositoryCompressionLevel = "none" | "fast" | "balanced" | "maximum";

export type RepositoryExportRequest = {
  repoId: string;
  target: RepositoryExportTarget;
  archive?: {
    format: RepositoryArchiveFormat;
    outputPath: string;
    compression: RepositoryCompressionLevel;
    encrypt: boolean;
    password?: string;
  };
  git?: {
    remote?: string;
    branch?: string;
    message?: string;
  };
};

export type RepositoryExportResponse = {
  repository: RepositorySummary;
  target: RepositoryExportTarget;
  outputPath?: string;
  format?: RepositoryArchiveFormat;
  encrypted?: boolean;
  remote?: string;
  branch?: string;
  message: string;
};

export type FileBrowserRequest = {
  repoId: string;
  directoryPath?: string;
  includeTree?: boolean;
  specialLocation?: "trash";
  offset?: number;
  limit?: number;
};

export type FileReadRequest = {
  repoId: string;
  path: string;
};

export type EntryAccessRecordRequest = {
  repoId: string;
  path: string;
};

export type EntryAccessRecordResponse = {
  repoId: string;
  path: string;
  recordedAt: string;
};

export type RecentAccessHistoryClearRequest = {
  repoId: string;
};

export type RecentAccessHistoryClearResponse = {
  repoId: string;
  clearedCount: number;
};

export type PluginCallRequest = {
  pluginId: string;
  method: string;
  payload?: Record<string, unknown>;
};

export type DownloaderPlaylistProgressEvent = {
  phase: "start" | "track" | "complete";
  playlistId: number;
  playlistName?: string | null;
  total: number;
  completed: number;
  failed: number;
  currentSongId?: number | null;
  currentSongName?: string | null;
  error?: string | null;
};

export type DownloaderPlaylistTrackRequest = {
  songId: number;
  songName?: string | null;
  sourcePayload?: Record<string, unknown> | null;
};

export type DownloaderPlaylistRequest = {
  playlistId: number;
  playlistName?: string;
  tracks: DownloaderPlaylistTrackRequest[];
  destination: {
    kind: string;
    path?: string | null;
    repoId?: string | null;
    parentPath?: string | null;
  };
  managedCacheRoot?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  level?: string | null;
};

export type PluginCallResponse<T = unknown> = {
  pluginId: string;
  method: string;
  payload: T;
  runtime?: PluginCallRuntime;
};

export type PluginCallRuntime = {
  degraded: boolean;
  degradationReason?: string | null;
  dependencyStatus: PluginDependencyStatus;
};

export type PluginHookExecutionRecord = {
  executionId: string;
  pluginId: string;
  hookSlot: string;
  hookAction: string;
  hookLabel?: string | null;
  status: "success" | "failed" | "blocked" | string;
  message: string;
  target: Record<string, unknown>;
  startedAt: string;
  finishedAt: string;
  runtime?: PluginCallRuntime | null;
};

export type PluginHookExecutionListRequest = {
  pluginId?: string;
  limit?: number;
};

export type PluginHookExecutionListResponse = {
  records: PluginHookExecutionRecord[];
};

export type PluginArchiveReadRequest = {
  pluginId: string;
  path: string;
};

export type PluginArchiveTextResponse = {
  pluginId: string;
  path: string;
  text: string;
};

export type PluginDataDirectoryResponse = {
  pluginId: string;
  path: string;
};

export type PluginDataFilePreviewSourceRequest = {
  pluginId: string;
  path: string;
  mediaType: string;
};

export type RepositoryCacheFilePreviewSourceRequest = {
  repoId: string;
  path: string;
  mediaType: string;
};

export type PluginDataFilePreviewSourceResponse = {
  pluginId: string;
  path: string;
  token: string;
  sourceUrl?: string | null;
  mediaType: string;
  sizeBytes: number;
  modifiedAt?: string | null;
};

export type RepositoryCacheFilePreviewSourceResponse = {
  repoId: string;
  path: string;
  token: string;
  sourceUrl?: string | null;
  mediaType: string;
  sizeBytes: number;
  modifiedAt?: string | null;
};

export type DownloaderEnsureRuntimeRequest = Record<string, never>;

export type DownloaderEnqueueRequest = {
  url: string;
  destinationPath: string;
  metadata?: Record<string, unknown> | null;
};

export type DownloaderAwaitRequest = {
  taskId: string;
};

export type DownloaderRemoveRequest = {
  taskId: string;
};

export type DownloaderTaskStatus =
  | "queued"
  | "active"
  | "completed"
  | "failed"
  | "removed"
  | string;

export type DownloaderTaskRecord = {
  taskId: string;
  gid: string;
  url: string;
  destinationPath: string;
  metadata?: Record<string, unknown> | null;
  status: DownloaderTaskStatus;
  createdAt: string;
  finishedAt?: string | null;
  totalLength?: number | null;
  completedLength?: number | null;
  error?: string | null;
};

export type DownloaderAria2Status = {
  running: boolean;
  pid?: number | null;
  executablePath?: string | null;
  version?: string | null;
  rpcUrl?: string | null;
  secret?: string | null;
  source?: string | null;
  updatedAt?: string | null;
  error?: string | null;
  downloadUrl: string;
  bundledArchivePath?: string | null;
};

export type DownloaderRuntimeStatus = {
  runtime: string;
  aria2: DownloaderAria2Status;
  queueSize: number;
  downloadsDir: string;
  downloadUrl: string;
  helperDir?: string;
};

export type DownloaderEnsureRuntimeResponse = {
  runtime: string;
  downloadsDir: string;
  helperDir: string;
  downloadUrl: string;
  aria2: DownloaderAria2Status;
  queueSize: number;
};

export type DownloaderEnqueueResponse = {
  taskId: string;
  gid: string;
  status: DownloaderTaskStatus;
  destinationPath: string;
};

export type DownloaderAwaitResponse = DownloaderTaskRecord;

export type DownloaderRemoveResponse = {
  taskId: string;
  removed: true;
};

export type OfficeConverterStatus = {
  available: boolean;
  path?: string | null;
  version?: string | null;
  reason?: string | null;
};

export type OfficeConvertPreviewResult = {
  pdfPath: string;
  cached: boolean;
  converter: string;
  cacheKey: string;
  mediaType: "application/pdf";
  sizeBytes: number;
  modifiedAt?: string | null;
};

export type OfficeConvertEnsurePreviewPdfRequest = {
  repoId: string;
  entryPath: string;
  extension: string;
  sourcePath?: string | null;
  sourceModifiedAt?: string | null;
  sourceSizeBytes?: number | null;
};

export type OfficeConvertGetRuntimeStatusRequest = Record<string, never>;

export type OfficeConvertClearPreviewCacheRequest = {
  repoId: string;
};

export type OfficeConvertRunRuntimeSelfCheckRequest = Record<string, never>;

export type OfficeConvertShutdownDaemonRequest = Record<string, never>;

export type OfficeConvertDaemonControl = {
  health?: string | null;
  convert?: string | null;
  shutdown?: string | null;
};

export type OfficeConvertDaemonLastConvert = {
  phase?: string | null;
  sourcePath?: string | null;
  pdfPath?: string | null;
  updatedAt?: string | null;
  conversionMode?: string | null;
  error?: string | null;
};

export type OfficeConvertDaemonLastSelfCheck = {
  startedAt?: string | null;
  completedAt?: string | null;
  durationMs?: number | null;
  ok?: boolean | null;
  converter?: string | null;
  converterPath?: string | null;
  converterVersion?: string | null;
  conversionMode?: string | null;
  samplePath?: string | null;
  pdfPath?: string | null;
  pdfSizeBytes?: number | null;
  error?: string | null;
};

export type OfficeConvertDaemonStatus = {
  running: boolean;
  healthy?: boolean | null;
  helperType?: string | null;
  port?: number | null;
  baseUrl?: string | null;
  pid?: number | null;
  sofficeReady?: boolean | null;
  sofficePid?: number | null;
  unoAvailable?: boolean | null;
  pythonValid?: boolean | null;
  pythonPath?: string | null;
  path?: string | null;
  updatedAt?: string | null;
  error?: string | null;
  control?: OfficeConvertDaemonControl | null;
  lastConvert?: OfficeConvertDaemonLastConvert | null;
  lastSelfCheck?: OfficeConvertDaemonLastSelfCheck | null;
};

export type OfficeConvertRuntimeStatus = {
  converterMode: "auto" | "microsoft-office" | "libreoffice" | string;
  microsoftOffice: OfficeConverterStatus;
  libreofficeSystem: OfficeConverterStatus;
  libreofficeBundle: OfficeConverterStatus;
  daemon: OfficeConvertDaemonStatus;
  autoDownloadLibreOffice: boolean;
  bundledDownloadUrl: string;
};

export type OfficeConvertClearPreviewCacheResponse = {
  repoId: string;
  removed: number;
};

export type OfficeConvertRunRuntimeSelfCheckResponse = {
  ok: boolean;
  converter: string;
  converterPath: string;
  converterVersion?: string | null;
  conversionMode?: string | null;
  samplePath: string;
  pdfPath?: string | null;
  pdfSizeBytes?: number | null;
  durationMs: number;
  error?: string | null;
};

export type OfficeConvertShutdownDaemonResponse = {
  stopped: boolean;
  pid?: number | null;
  reason?: string | null;
};

export type PluginConfigValue = unknown;

export type PluginConfigSnapshot = {
  pluginId: string;
  dataDirectory: string;
  schema?: PluginSettingsContribution | Record<string, unknown> | null;
  values: Record<string, PluginConfigValue>;
};

export type PluginConfigSetRequest = {
  pluginId: string;
  key: string;
  value: PluginConfigValue;
};

export type PluginConfigDeleteRequest = {
  pluginId: string;
  key: string;
};

export type BinaryFileWriteRequest = {
  path: string;
  bytes: number[];
};

export type BinaryFileWriteResponse = {
  path: string;
  sizeBytes: number;
};

export type FilePreviewSourceResponse = {
  repoId: string;
  path: string;
  token: string;
  sourceUrl?: string | null;
  localPath?: string | null;
  mediaType: string;
  sizeBytes: number;
  modifiedAt?: string | null;
};

export type EntryPlaybackRequest = {
  repoId: string;
  path: string;
};

export type EntryPlaybackProgressEvent = {
  phase: "resolve" | "download" | "preview" | "ready" | "error";
  repoId: string;
  path: string;
  value: number;
  detail: string;
  indeterminate: boolean;
  cached?: boolean | null;
  error?: string | null;
};

export type EntryPlaybackSourceResponse = {
  repoId: string;
  path: string;
  mediaType: string;
  sourceUrl?: string | null;
  localPath?: string | null;
  tempFilePath?: string | null;
  lyricPath?: string | null;
  lyricSourceUrl?: string | null;
  wordLyricPath?: string | null;
  wordLyricSourceUrl?: string | null;
  expiresAt?: string | null;
  sizeBytes?: number | null;
  modifiedAt?: string | null;
};

export type FileCreateRequest = {
  repoId: string;
  parentPath?: string;
  name: string;
};

export type FileImportRequest = {
  repoId: string;
  parentPath?: string;
  sourcePaths: string[];
};

export type FileArchiveImportRequest = {
  repoId: string;
  parentPath?: string;
  archivePath: string;
};

export type EagleImportMode = "copy" | "move";

export type EagleLibraryImportRequest = {
  repoId: string;
  parentPath?: string;
  libraryPath: string;
  mode: EagleImportMode;
};

export type EagleLibraryImportSummary = {
  importedFiles: number;
  importedDirectories: number;
  importedTrashEntries: number;
  importedShortcuts: number;
  importedSmartFolders: number;
  importedRepositoryActions: number;
  importedTagGroups: number;
  importedAliasGroups: number;
  importedHardlinkGroups: number;
};

export type EagleLibraryImportWarning = {
  type: string;
  assetId?: string | null;
  field?: string | null;
  folderId?: string | null;
  targetRelativePath?: string | null;
  source?: string | null;
  sourceId?: string | null;
  name?: string | null;
  reason?: string | null;
  index?: number | null;
  conditions?: unknown;
  details?: Record<string, unknown> | unknown;
};

export type EagleLibraryImportResponse = {
  snapshot: FileBrowserSnapshot;
  summary: EagleLibraryImportSummary;
  warnings: EagleLibraryImportWarning[];
};

export type ExternalAddAssetClient = {
  id?: string;
  name?: string;
  version?: string;
};

export type ExternalAddAssetItem = {
  kind: "remoteUrl" | string;
  url?: string;
  filename?: string;
  headers?: Record<string, string>;
  metadata?: Record<string, unknown>;
};

export type ExternalAddAssetRequest = {
  repoId: string;
  parentPath?: string;
  client?: ExternalAddAssetClient;
  items: ExternalAddAssetItem[];
};

export type ExternalApiConnectionStatus = {
  baseUrl: string;
  token: string;
  version: string;
  startedAt: string;
  ready: boolean;
  connectionFilePath: string;
};

export type ExternalImportedAsset = {
  itemIndex: number;
  assetId?: string | null;
  path: string;
};

export type ExternalAddAssetFailure = {
  itemIndex: number;
  code:
    | "unauthorized"
    | "notReady"
    | "repoNotFound"
    | "repoUnavailable"
    | "unsupportedRepositoryBackend"
    | "invalidTargetPath"
    | "invalidInput"
    | "downloadFailed"
    | "duplicateTarget"
    | "importRejected"
    | "internalError"
    | string;
  message: string;
  retryable: boolean;
  details?: Record<string, unknown>;
};

export type ExternalAddAssetResponse = {
  requestId: string;
  status: "success" | "partial" | "failed";
  imported: ExternalImportedAsset[];
  failed: ExternalAddAssetFailure[];
  summary: {
    total: number;
    imported: number;
    failed: number;
  };
};

export type FileCopyMode = "hardlinkPreferred" | "copy";

export type FileCopyRequest = {
  repoId: string;
  sourcePaths: string[];
  parentPath?: string;
  mode?: FileCopyMode;
};

export type FileMoveRequest = {
  repoId: string;
  sourcePaths: string[];
  parentPath: string;
};

export type HardlinkState = "primary" | "linked" | "copied" | "copiedFallback" | "broken" | "missing";

export type HardlinkCandidate = {
  candidateId: string;
  repoId: string;
  newAssetId: string;
  newPath: string;
  existingAssetId: string;
  existingPath: string;
  contentHash: string;
  sizeBytes: number;
  sizeLabel: string;
  createdAt: string;
};

export type HardlinkCandidateResponse = {
  repoId: string;
  candidates: HardlinkCandidate[];
};

export type HardlinkConfirmRequest = {
  repoId: string;
  candidateId: string;
};

export type HardlinkConfirmResponse = {
  repoId: string;
  candidate: HardlinkCandidate;
  state: HardlinkState;
};

export type FileRenameRequest = {
  repoId: string;
  path: string;
  newName: string;
};

export type FileDeleteMode = "delete" | "moveToParent" | "permanentDelete";

export type FileDeleteRequest = {
  repoId: string;
  path: string;
  mode?: FileDeleteMode;
};

export type TrashMutationAction = "restore" | "restoreAll" | "empty";

export type TrashMutationRequest = {
  repoId: string;
  action: TrashMutationAction;
  path?: string;
};

export type RepositoryMutationResponse = {
  repository: RepositorySummary;
};

export type SyncRequest = {
  repoId: string;
};

export type SyncResult = {
  repoId: string;
  scannedFiles: number;
  createdAssets: number;
  updatedAssets: number;
  deletedAssets: number;
  createdEvents: number;
  hardlinkCandidates: number;
};

export type ThumbnailAction = "ensure" | "refresh" | "save" | "saveGenerated" | "clear";

export type ThumbnailRequest = {
  repoId: string;
  path: string;
  action?: ThumbnailAction;
  sourcePath?: string;
  sourceUrl?: string;
  imageBytes?: number[];
  mediaType?: string;
};

export type ThumbnailResponse = {
  repoId: string;
  path: string;
  assetId: string;
  kind: "directory" | "file";
  thumbnailPath?: string | null;
  thumbnailCustom: boolean;
  metadata?: Record<string, unknown> | null;
};

export type WorkspaceStartupStatus = "idle" | "loading" | "ready" | "error";

export type WorkspaceStartupState = {
  status: WorkspaceStartupStatus;
  stepLabel: string;
  currentStep: number;
  totalSteps: number;
  percent: number;
  error: string | null;
};

export type RepositorySyncProgress = {
  phase: "idle" | "scanning" | "writing" | "refreshing" | "complete" | "error";
  label: string;
  current: number;
  total: number;
  percent: number;
};

export type RevisionActionRequest = {
  repoId: string;
  assetId: string;
};

export type RevisionActionResponse = {
  outcome: "success" | "conflict" | "merged";
  asset: AssetDetail;
};

export type CacheConfig = {
  metadataCapacity: number;
  thumbnailCapacity: number;
  queryCapacity: number;
};

export type CacheEntry = {
  cacheType: string;
  key: string;
  lastAccessedAt: string;
};

export type CacheSnapshot = {
  config: CacheConfig;
  entries: CacheEntry[];
};

export type PlaylistPlayerContribution = {
  playerTypeId: string;
  label: string;
  fileClass: PlaylistFileClass;
  supportedExtensions: string[];
  supportsSeek: boolean;
  supportsVolume: boolean;
  supportsPreviewNavigation: boolean;
  description?: string;
};

export type ToolPageContribution = {
  toolPageId: string;
  label: string;
  description?: string;
  order?: number;
};

export type PluginConfigFieldOption = {
  label: string;
  value: string | number | boolean;
};

export type PluginConfigField = {
  key: string;
  label: string;
  type: "string" | "number" | "boolean" | "select" | "json" | string;
  description?: string;
  required?: boolean;
  default?: PluginConfigValue;
  placeholder?: string;
  options?: PluginConfigFieldOption[];
  min?: number;
  max?: number;
};

export type PluginSettingsPageContribution = {
  label?: string;
  description?: string;
  order?: number;
};

export type PluginSettingsContribution = {
  schemaVersion?: number;
  fields?: PluginConfigField[];
  settingsPage?: PluginSettingsPageContribution;
};

export type PluginApiTestContribution = {
  method: string;
  summary?: string;
  payload?: unknown;
  requestTemplate?: unknown;
};

export type PluginManifest = {
  pluginId: string;
  legacyPluginIds?: string[];
  name: string;
  version: string;
  type?: {
    layer:
      | "source"
      | "library-kind"
      | "extractor-parser"
      | "provider-service"
      | "integration-capability-hook";
    kind: string;
  };
  kind: string;
  category?: PluginCategory | string;
  description: string;
  capabilities: string[];
  enabled: boolean;
  sdk?: "frontend" | "backend";
  entry?: {
    frontend?: {
      module: string;
      export?: string;
    };
    backend?: {
      library: string;
      path?: string;
    };
    manifestOnly?: boolean;
    [key: string]: unknown;
  };
  source?: "builtin" | "user" | "system";
  runtime?: "vue-module" | "native-dylib" | "manifest-only";
  permissions?: string[];
  requires?: string[];
  optional?: string[];
  hooks?: PluginHook[];
  contributes?: {
    apiTests?: PluginApiTestContribution[];
    playlistPlayers?: PlaylistPlayerContribution[];
    settings?: PluginSettingsContribution;
    toolPages?: ToolPageContribution[];
    [key: string]: unknown;
  };
  compat?: {
    sdkVersion?: string;
    legacyPluginIds?: string[];
  };
  status?: "ready" | "disabled" | "unavailable" | "error";
  dependencyStatus?: PluginDependencyStatus;
  disableReason?: string | null;
  degraded?: boolean;
  degradationReason?: string | null;
  archivePath?: string;
};

export type PluginDependencyStatus = {
  required: PluginDependencyState[];
  optional: PluginDependencyState[];
  missingRequired: string[];
  missingOptional: string[];
  disabledRequired: string[];
  disabledOptional: string[];
};

export type PluginDependencyState = {
  pluginId: string;
  name?: string | null;
  status: "ready" | "disabled" | "missing" | "unavailable" | "error" | string;
  enabled: boolean;
  available: boolean;
};

export type PluginCategory = "source" | "library-kind" | "parser" | "preview" | "service";

export type PluginHook = {
  slot: CoreHostCapability | string;
  action: string;
  label?: string;
  requires?: string[];
};

export type CoreHostCapability =
  | "playlist"
  | "pip"
  | "progress"
  | "candidateQueue"
  | "batchOrganize"
  | "downloadQueue"
  | "metadataMerge"
  | "renameMove"
  | "auditLog"
  | "search"
  | "toolPage";

export type PluginEnabledRequest = {
  pluginId: string;
  enabled: boolean;
};

export type PluginInstallRequest = {
  packagePath: string;
};

export type PluginMutationResponse = {
  plugins: PluginManifest[];
};

export type ApiDefinition = {
  group: string;
  transport?: "external-http" | "tauri-command" | "plugin-call" | string;
  method: string;
  path: string;
  summary: string;
  command?: string;
  pluginId?: string;
  pluginMethod?: string;
  requiresAuth?: boolean;
  requestTemplate?: unknown;
};

export type ApiDesignSnapshot = {
  transport: string;
  endpoints: ApiDefinition[];
};
