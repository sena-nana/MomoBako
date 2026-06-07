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

export type RepositorySummary = {
  repoId: string;
  name: string;
  path: string;
  backend: RepositoryBackendSummary;
  status: string;
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
};

export type FolderSummary = {
  path: string;
  label: string;
  assetCount: number;
};

export type RepositorySnapshot = {
  repository: RepositorySummary;
  folderLabel: string;
  folders: FolderSummary[];
  assets: AssetSummary[];
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
};

export type FileBrowserSnapshot = {
  repoId: string;
  rootPath: string;
  backendPluginId: string;
  backendKind: string;
  currentPath: string;
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

export type SearchRequest = {
  query: string;
  repoId?: string;
  metadataKey?: string;
  metadataValue?: string;
  tag?: string;
  minRating?: number;
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

export type FileBrowserRequest = {
  repoId: string;
  directoryPath?: string;
  includeTree?: boolean;
};

export type FileReadRequest = {
  repoId: string;
  path: string;
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

export type FileRenameRequest = {
  repoId: string;
  path: string;
  newName: string;
};

export type FileDeleteMode = "delete" | "moveToParent";

export type FileDeleteRequest = {
  repoId: string;
  path: string;
  mode?: FileDeleteMode;
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
  name: string;
  version: string;
  kind: string;
  description: string;
  capabilities: string[];
  enabled: boolean;
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
