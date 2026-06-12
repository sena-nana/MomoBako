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

export type RepositorySummary = {
  repoId: string;
  name: string;
  path: string;
  backend: RepositoryBackendSummary;
  status: RepositoryStatus;
  assetCount: number;
  updatedAt: string;
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
  version: number;
  tags: string[];
  thumbnailPath?: string | null;
  hardlinkGroupId?: string | null;
  hardlinkState?: HardlinkState | null;
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

export type FolderMetadata = {
  protected: boolean;
  passwordTip?: string | null;
};

export type RepositorySnapshot = {
  repository: RepositorySummary;
  folderLabel: string;
  folders: FolderSummary[];
  assets: AssetSummary[];
  quickAccess?: RepositoryShortcut[];
  tagGroups?: RepositoryTagGroup[];
  metadataFields: string[];
  recentRevisionCount: number;
  overview: {
    totalSizeBytes: number;
    totalSizeLabel: string;
    fileCount: number;
    folderCount: number;
    readmeContent: string | null;
  };
};

export type FileTreeNode = {
  path: string;
  label: string;
  children: FileTreeNode[];
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
};

export type MetadataTagGroup = string;

export type FileBrowserSnapshot = {
  repoId: string;
  rootPath: string;
  backendPluginId: string;
  backendKind: string;
  currentPath: string;
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
};

export type RepositoryFolderRequest = {
  path: string;
};

export type RepositoryRelocateRequest = {
  repoId: string;
  path: string;
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
};

export type FileReadRequest = {
  repoId: string;
  path: string;
};

export type PluginCallRequest = {
  pluginId: string;
  method: string;
  payload?: Record<string, unknown>;
};

export type PluginCallResponse<T = unknown> = {
  pluginId: string;
  method: string;
  payload: T;
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
  mediaType: string;
  sizeBytes: number;
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
  contributes?: Record<string, unknown>;
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
  | "search";

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
  method: string;
  path: string;
  summary: string;
};

export type ApiDesignSnapshot = {
  transport: string;
  endpoints: ApiDefinition[];
};
