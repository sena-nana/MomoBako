import { ref, shallowRef } from "vue";
import type {
  ApiDesignSnapshot,
  AssetDetail,
  CacheSnapshot,
  FileBrowserSnapshot,
  FileTreeNode,
  HardlinkCandidate,
  PlaylistDetail,
  PlaylistSummary,
  PluginManifest,
  RepositoryAction,
  RepositorySnapshot,
  RepositorySummary,
  SearchHit,
  SmartFolderResultSnapshot,
  SmartFolderTreeNode,
  SyncResult,
  WorkspaceStartupState,
} from "../../types/repository";

export type WorkspacePanelKey = "files" | "deleted" | "search" | "smartFolder" | "playlist" | "actions" | "extensions";

export type WorkspaceFilterState = {
  tags: string[];
  formats: string[];
  colors: string[];
  shapes: string[];
  excludeTags: string[];
  excludeFormats: string[];
  excludeQuery: string;
  excludePathPrefixes: string;
  excludeMetadataFilters: string;
  excludeNumberFilters: string;
  excludeDateFilters: string;
  numberFilters: string;
  dateFilters: string;
  matchMode: "and" | "or";
  sortField: string;
  sortDirection: "asc" | "desc";
  limit: number | null;
  minRating: number | null;
};

export type WorkspaceRequestToken = number;

export const STARTUP_TOTAL_STEPS = 3;

export function createInitialWorkspaceStartup(): WorkspaceStartupState {
  return {
    status: "idle",
    stepLabel: "准备加载仓库",
    currentStep: 0,
    totalSteps: STARTUP_TOTAL_STEPS,
    percent: 0,
    error: null,
  };
}

export function createInitialFilters(): WorkspaceFilterState {
  return {
    tags: [],
    formats: [],
    colors: [],
    shapes: [],
    excludeTags: [],
    excludeFormats: [],
    excludeQuery: "",
    excludePathPrefixes: "",
    excludeMetadataFilters: "",
    excludeNumberFilters: "",
    excludeDateFilters: "",
    numberFilters: "",
    dateFilters: "",
    matchMode: "and",
    sortField: "",
    sortDirection: "asc",
    limit: null,
    minRating: null,
  };
}

export const repositories = shallowRef<RepositorySummary[]>([]);
export const activeRepoId = ref<string | null>(null);
export const activeSnapshot = shallowRef<RepositorySnapshot | null>(null);
export const activeAssetId = ref<string | null>(null);
export const activeAssetDetail = shallowRef<AssetDetail | null>(null);
export const activePreviewPath = ref<string | null>(null);
export const activePanel = ref<WorkspacePanelKey>("files");
export const currentDirectoryPath = ref("");
export const fileBrowser = shallowRef<FileBrowserSnapshot | null>(null);
export const fileTree = shallowRef<FileTreeNode[]>([]);
export const selectedFilePath = ref<string | null>(null);
export const selectedFilePaths = ref<string[]>([]);
export const selectionAnchorPath = ref<string | null>(null);
export const searchQuery = ref("");
export const searchResults = shallowRef<SearchHit[]>([]);
export const smartFolders = shallowRef<SmartFolderTreeNode[]>([]);
export const repositoryActions = shallowRef<RepositoryAction[]>([]);
export const playlists = shallowRef<PlaylistSummary[]>([]);
export const playlistMemberships = shallowRef<Record<string, string[]>>({});
export const activePlaylistId = ref<string | null>(null);
export const activePlaylistDetail = shallowRef<PlaylistDetail | null>(null);
export const activeRepositoryActionId = ref<string | null>(null);
export const activeSmartFolderId = ref<string | null>(null);
export const smartFolderResult = shallowRef<SmartFolderResultSnapshot | null>(null);
export const isFilterBarOpen = ref(false);
export const filters = ref<WorkspaceFilterState>(createInitialFilters());
export const hardlinkCandidates = shallowRef<HardlinkCandidate[]>([]);
export const lastSyncResult = shallowRef<SyncResult | null>(null);
export const plugins = shallowRef<PluginManifest[]>([]);
export const cacheSnapshot = shallowRef<CacheSnapshot | null>(null);
export const apiDesign = shallowRef<ApiDesignSnapshot | null>(null);
export const workspaceStartup = ref<WorkspaceStartupState>(createInitialWorkspaceStartup());
export const isLoadingRepositories = ref(false);
export const isLoadingSnapshot = ref(false);
export const isLoadingAssetDetail = ref(false);
export const isLoadingFileBrowser = ref(false);
export const isSearching = ref(false);
export const isLoadingSmartFolder = ref(false);
export const isLoadingRepositoryActions = ref(false);
export const isRunningRepositoryAction = ref(false);
export const isMutatingSmartFolder = ref(false);
export const isSavingMetadata = ref(false);
export const isSyncing = ref(false);
export const isMutatingFiles = ref(false);
export const isLoadingSettingsData = ref(false);
export const isManagingPlugins = ref(false);
export const isExternalDragActive = ref(false);
export const isInternalDragActive = ref(false);
export const draggedWorkspacePaths = ref<string[]>([]);
export const dragHoverFolderPath = ref<string | null>(null);
export const error = ref<string | null>(null);
