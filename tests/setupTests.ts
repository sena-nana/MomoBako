import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/vue";
import { afterEach, vi } from "vitest";
import type { PluginManifest, SearchHit, SearchRequest } from "../src/types/repository";

type MockRepository = {
  repoId: string;
  name: string;
  path: string;
  backend: {
    pluginId: string;
    kind: string;
    name: string;
    capabilities: string[];
  };
  status: string;
  assetCount: number;
  updatedAt: string;
};

type MockEntry = {
  path: string;
  name: string;
  kind: "directory" | "file";
  extension: string | null;
  sizeBytes: number | null;
  sizeLabel: string | null;
  modifiedAt: string | null;
  assetId: string | null;
  status: string | null;
  thumbnailPath?: string | null;
  thumbnailCustom?: boolean;
  metadata?: Record<string, unknown>;
};

let mockRepositories: MockRepository[] = [];
let mockSelectedFolder: string | null = null;
let mockSelectedFile: string | null = null;
let mockSavePath: string | null = "C:/Mock/Exports/repository.zip";
let mockDirectoryCreatedOnNextSync: string | null = null;
let mockOpenerFailure: Error | null = null;
let mockInvokeFailure: { command: string; error: Error } | null = null;
let mockInvokeDelay: { command: string; resolve: () => void; promise: Promise<void> } | null = null;
let mockSearchResults: SearchHit[] | null = null;
let mockPlugins: PluginManifest[] | null = null;
const invokeCalls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const openerCalls: Array<{ command: "openPath" | "revealItemInDir"; path: string }> = [];

const defaultSearchHits = (): SearchHit[] => [
  {
    repoId: "repo-main-001",
    repoName: "主资源库",
    assetId: "asset-01",
    path: "Campaigns/Summer/cover-final.psd",
    filename: "cover-final.psd",
    status: "synced",
    tags: ["封面", "主视觉", "PSD"],
    metadata: {
      note: "最终版封面，保留可编辑图层。",
      color: "红色",
      shape: "方形",
      rating: 5,
    },
  },
  {
    repoId: "repo-main-001",
    repoName: "主资源库",
    assetId: "asset-02",
    path: "Backgrounds/scene-forest-03.png",
    filename: "scene-forest-03.png",
    status: "synced",
    tags: ["背景", "森林", "PNG"],
    metadata: {
      note: "森林场景背景。",
      color: "绿色",
      shape: "横版",
      rating: 3,
    },
  },
];

const initialEntries = (): MockEntry[] => [
  {
    path: "Campaigns",
    name: "Campaigns",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
  },
  {
    path: "Campaigns/Summer",
    name: "Summer",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
  },
  {
    path: "Backgrounds",
    name: "Backgrounds",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
  },
  {
    path: "cover-final.psd",
    name: "cover-final.psd",
    kind: "file",
    extension: "psd",
    sizeBytes: 238950400,
    sizeLabel: "227.9 MB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-01",
    status: "synced",
  },
];

let mockEntries: MockEntry[] = initialEntries();
let mockTrashEntries: MockEntry[] = [];

function getParentPath(path: string) {
  const index = path.lastIndexOf("/");
  return index >= 0 ? path.slice(0, index) : "";
}

function getEntryName(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function addMockEntry(path: string, kind: "directory" | "file") {
  const name = getEntryName(path);
  mockEntries = [
    ...mockEntries,
    {
      path,
      name,
      kind,
      extension: kind === "file" && name.includes(".") ? name.split(".").at(-1) ?? "txt" : null,
      sizeBytes: kind === "file" ? 0 : null,
      sizeLabel: kind === "file" ? "0 B" : null,
      modifiedAt: "2026-06-05T00:18:00Z",
      assetId: null,
      status: kind === "file" ? "synced" : null,
    },
  ];
}

function searchHitFormat(hit: SearchHit) {
  const index = hit.filename.lastIndexOf(".");
  return index >= 0 ? hit.filename.slice(index + 1).toLowerCase() : "";
}

function metadataSearchText(value: unknown) {
  if (typeof value === "string") return value.toLowerCase();
  if (typeof value === "number" || typeof value === "boolean") return String(value).toLowerCase();
  return "";
}

function filterSearchHits(request: SearchRequest | undefined, hits: SearchHit[]) {
  if (!request) return hits;
  const query = request.query.trim().toLowerCase();
  const tags = request.tags?.map((tag) => tag.toLowerCase()).filter(Boolean) ?? [];
  const formats = request.formats?.map((format) => format.toLowerCase()).filter(Boolean) ?? [];
  const metadataFilters = request.metadataFilters ?? [];

  return hits.filter((hit) => {
    if (request.repoId && hit.repoId !== request.repoId) return false;
    if (query) {
      const haystack = [
        hit.repoName,
        hit.filename,
        hit.path,
        ...hit.tags,
        ...Object.values(hit.metadata).map(metadataSearchText),
      ].join(" ").toLowerCase();
      if (!haystack.includes(query)) return false;
    }
    if (formats.length && !formats.includes(searchHitFormat(hit))) return false;
    if (tags.length && !hit.tags.some((tag) => tags.some((expected) => tag.toLowerCase().includes(expected)))) return false;
    if (request.minRating != null) {
      const rating = typeof hit.metadata.rating === "number" ? hit.metadata.rating : 0;
      if (rating < request.minRating) return false;
    }
    return metadataFilters.every((filter) => {
      const actual = metadataSearchText(hit.metadata[filter.key]);
      const expected = filter.value.toLowerCase();
      return actual === expected || actual.includes(expected);
    });
  });
}

function moveEntryTreeToTrash(targetPath: string) {
  const targetEntry = mockEntries.find((entry) => entry.path === targetPath);
  if (!targetEntry) return null;

  const deletedAt = new Date().toISOString();
  const trashRootPath = targetPath;
  const movingEntries = mockEntries
    .filter((entry) => entry.path === targetPath || entry.path.startsWith(`${targetPath}/`))
    .map((entry) => ({
      ...entry,
      path: entry.path === targetPath ? trashRootPath : `${trashRootPath}${entry.path.slice(targetPath.length)}`,
      name: getEntryName(entry.path === targetPath ? trashRootPath : `${trashRootPath}${entry.path.slice(targetPath.length)}`),
      status: entry.kind === "file" ? "deleted" : entry.status,
      metadata: {
        deletedAt,
        originalPath: entry.path,
      },
    }));

  mockEntries = mockEntries.filter((entry) => (
    entry.path !== targetPath && !entry.path.startsWith(`${targetPath}/`)
  ));
  mockTrashEntries = [
    ...mockTrashEntries.filter((entry) => (
      entry.path !== trashRootPath && !entry.path.startsWith(`${trashRootPath}/`)
    )),
    ...movingEntries,
  ];
  return targetEntry;
}

function restoreTrashTree(targetPath: string) {
  const selectedEntries = mockTrashEntries.filter((entry) => (
    entry.path === targetPath || entry.path.startsWith(`${targetPath}/`)
  ));
  if (!selectedEntries.length) return;

  const restoredEntries = selectedEntries.map((entry) => {
    const originalPath = typeof entry.metadata?.originalPath === "string"
      ? entry.metadata.originalPath
      : entry.path;
    return {
      ...entry,
      path: originalPath,
      name: getEntryName(originalPath),
      status: entry.kind === "file" ? "synced" : entry.status,
      metadata: undefined,
    };
  });
  mockEntries = [
    ...mockEntries.filter((entry) => (
      !restoredEntries.some((restored) => restored.path === entry.path)
    )),
    ...restoredEntries,
  ];
  mockTrashEntries = mockTrashEntries.filter((entry) => (
    entry.path !== targetPath && !entry.path.startsWith(`${targetPath}/`)
  ));
}

function buildTree(entries = mockEntries) {
  type TreeNode = { path: string; label: string; children: TreeNode[] };
  const roots: TreeNode[] = [];
  const nodeMap = new Map<string, TreeNode>();
  const directoryEntries = entries
    .filter((entry) => entry.kind === "directory")
    .sort((left, right) => left.path.localeCompare(right.path));

  for (const entry of directoryEntries) {
    const node: TreeNode = {
      path: entry.path,
      label: entry.name,
      children: [],
    };
    nodeMap.set(entry.path, node);
    const parentNode = nodeMap.get(getParentPath(entry.path));
    if (parentNode) {
      parentNode.children.push(node);
    } else {
      roots.push(node);
    }
  }

  const sortNodes = (nodes: TreeNode[]) => {
    nodes.sort((left, right) => left.label.localeCompare(right.label));
    nodes.forEach((node) => sortNodes(node.children));
  };
  sortNodes(roots);
  return roots;
}

function getEntriesForDirectory(entries: MockEntry[], directoryPath: string) {
  return entries
    .filter((entry) => getParentPath(entry.path) === directoryPath)
    .sort((left, right) => {
      if (left.kind !== right.kind) {
        return left.kind === "directory" ? -1 : 1;
      }
      return left.path.localeCompare(right.path);
    });
}

function getMockEntriesForRepository(repoId: string) {
  return repoId === altSnapshot.repository.repoId ? altEntries : mockEntries;
}

function getMockSnapshot(repoId: string) {
  return repoId === altSnapshot.repository.repoId ? altSnapshot : mockSnapshot;
}

function getMockFileBrowser(directoryPath = "", includeTree = true, specialLocation?: "trash", repoId = "repo-main-001") {
  const entries = specialLocation === "trash" ? mockTrashEntries : getMockEntriesForRepository(repoId);
  const snapshotSource = getMockSnapshot(repoId);
  const snapshot: {
    repoId: string;
    rootPath: string;
    backendPluginId: string;
    backendKind: string;
    currentPath: string;
    specialLocation?: "trash";
    tree?: ReturnType<typeof buildTree>;
    entries: ReturnType<typeof getEntriesForDirectory>;
  } = {
    repoId,
    rootPath: snapshotSource.repository.path,
    backendPluginId: snapshotSource.repository.backend.pluginId,
    backendKind: snapshotSource.repository.backend.kind,
    currentPath: directoryPath,
    entries: getEntriesForDirectory(entries, directoryPath),
  };
  if (specialLocation) {
    snapshot.specialLocation = specialLocation;
  } else if (includeTree) {
    snapshot.tree = buildTree(entries);
  }
  return snapshot;
}

const mockSnapshot = {
  repository: {
    repoId: "repo-main-001",
    name: "主资源库",
    path: "C:/Mock/AnimeAssets",
    backend: {
      pluginId: "momobako.local-filesystem",
      kind: "filesystem",
      name: "Local Filesystem",
      capabilities: ["browse", "read", "write", "watch", "sync"],
    },
    status: "ready",
    assetCount: 6,
    updatedAt: "2026-06-05T00:18:00Z",
  },
  folderLabel: "Campaigns",
  folders: [
    { path: "Campaigns", label: "Campaigns", assetCount: 1 },
    { path: "Backgrounds", label: "Backgrounds", assetCount: 1 },
    { path: "Characters", label: "Characters", assetCount: 1 },
  ],
  assets: [
    {
      assetId: "asset-01",
      repoId: "repo-main-001",
      path: "Campaigns/Summer/cover-final.psd",
      filename: "cover-final.psd",
      extension: "psd",
      sizeBytes: 238950400,
      sizeLabel: "227.9 MB",
      status: "synced",
      modifiedAt: "2026-06-05T00:18:00Z",
      version: 1,
      tags: ["封面", "主视觉", "PSD"],
    },
    {
      assetId: "asset-02",
      repoId: "repo-main-001",
      path: "Backgrounds/scene-forest-03.png",
      filename: "scene-forest-03.png",
      extension: "png",
      sizeBytes: 15245312,
      sizeLabel: "14.5 MB",
      status: "synced",
      modifiedAt: "2026-06-04T22:04:00Z",
      version: 1,
      tags: ["背景", "森林", "PNG"],
    },
  ],
  metadataFields: ["favorite", "note", "rating", "title"],
  recentRevisionCount: 6,
  overview: {
    totalSizeBytes: 254195712,
    totalSizeLabel: "242.4 MB",
    fileCount: 2,
    folderCount: 3,
    readmeContent: "# 主资源库\n用于存放项目主视觉、背景和衍生素材。\n",
  },
};

const altRepository = {
  repoId: "repo-alt-001",
  name: "参考资源库",
  path: "C:/Mock/ReferenceAssets",
  backend: {
    pluginId: "momobako.local-filesystem",
    kind: "filesystem",
    name: "Local Filesystem",
    capabilities: ["browse", "read", "write", "watch", "sync"],
  },
  status: "ready",
  assetCount: 1,
  updatedAt: "2026-06-05T00:18:00Z",
};

const altEntries: MockEntry[] = [
  {
    path: "Reference",
    name: "Reference",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
  },
  {
    path: "Reference/Paint",
    name: "Paint",
    kind: "directory",
    extension: null,
    sizeBytes: null,
    sizeLabel: null,
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: null,
    status: null,
  },
  {
    path: "Reference/Paint/target-preview.png",
    name: "target-preview.png",
    kind: "file",
    extension: "png",
    sizeBytes: 4096,
    sizeLabel: "4 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-alt-01",
    status: "synced",
  },
];

const altSnapshot = {
  repository: altRepository,
  folderLabel: "Reference",
  folders: [
    { path: "Reference", label: "Reference", assetCount: 1 },
    { path: "Reference/Paint", label: "Paint", assetCount: 1 },
  ],
  assets: [
    {
      assetId: "asset-alt-01",
      repoId: "repo-alt-001",
      path: "Reference/Paint/target-preview.png",
      filename: "target-preview.png",
      extension: "png",
      sizeBytes: 4096,
      sizeLabel: "4 KB",
      status: "synced",
      modifiedAt: "2026-06-05T00:18:00Z",
      version: 1,
      tags: ["参考", "PNG"],
    },
  ],
  metadataFields: ["note"],
  recentRevisionCount: 1,
  overview: {
    totalSizeBytes: 4096,
    totalSizeLabel: "4 KB",
    fileCount: 1,
    folderCount: 2,
    readmeContent: null,
  },
};

const mockAssetDetail = {
  summary: mockSnapshot.assets[0],
  metadata: [
    { key: "favorite", valueType: "boolean", value: true, version: 1, updatedAt: "2026-06-05T00:18:00Z" },
    { key: "note", valueType: "string", value: "最终版封面，保留可编辑图层。", version: 1, updatedAt: "2026-06-05T00:18:00Z" },
    { key: "rating", valueType: "number", value: 5, version: 1, updatedAt: "2026-06-05T00:18:00Z" },
    { key: "title", valueType: "string", value: "Summer Launch Cover", version: 1, updatedAt: "2026-06-05T00:18:00Z" },
  ],
  revisions: [
    {
      revisionId: "rev-asset-01",
      assetId: "asset-01",
      timestamp: "2026-06-05T00:18:00Z",
      operation: "metadata.seeded",
      before: {},
      after: { note: "最终版封面，保留可编辑图层。", rating: 5 },
      source: "seed",
    },
  ],
};

function pluginManifest(
  pluginId: string,
  legacyPluginIds: string[],
  name: string,
  version: string,
  kind: string,
  description: string,
  capabilities: string[],
  enabled: boolean,
  sdk: "frontend" | "backend",
  runtime: "vue-module" | "native-dylib" | "manifest-only",
  source: "builtin" | "user" | "system" = "builtin",
): PluginManifest {
  return {
    pluginId,
    legacyPluginIds,
    name,
    version,
    kind,
    description,
    capabilities,
    enabled,
    sdk,
    entry: {},
    source,
    runtime,
    permissions: [],
    compat: { sdkVersion: "1", legacyPluginIds },
    status: enabled ? "ready" : "disabled",
  };
}

function createMockPlugins() {
  return [
    pluginManifest("momobako.local-filesystem", ["builtin.local-filesystem"], "Local Filesystem", "1.0.0", "filesystem", "使用本地目录作为仓库文件管理后端。", ["browse", "read", "write", "watch", "sync"], true, "backend", "native-dylib"),
    pluginManifest("momobako.webdav", ["builtin.webdav"], "WebDAV", "0.1.0", "webdav", "通过 WebDAV 适配远程文件管理服务。", ["browse", "read", "write", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.cloud-drive", ["builtin.cloud-drive"], "Cloud Drive", "0.1.0", "cloud", "预留云盘文件系统接入点，如对象存储或网盘。", ["browse", "read", "write", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.preview.three-model", ["builtin.three-model-preview"], "3D Model Preview", "1.0.0", "preview", "为 FBX、OBJ、GLB、glTF 与 VRM 模型提供可旋转缩放的 3D 文件预览。", ["preview", "3d-model", "fbx", "obj", "gltf", "vrm"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.media", ["builtin.media-preview"], "Media Preview", "1.0.0", "preview", "为常见视频与音频文件提供内联播放预览。", ["preview", "media", "video", "audio"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.text", ["builtin.text-preview"], "Text Preview", "1.0.0", "preview", "为常见文本与 Markdown 文件提供阅读预览，并生成文本缩略图。", ["preview", "text", "markdown", "thumbnail"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.office", ["builtin.office-preview"], "Office & PDF Preview", "1.0.0", "preview", "为 Microsoft Office 文档与 PDF 文件提供预览，并生成文档缩略图。", ["preview", "thumbnail", "pdf", "office", "word", "excel", "powerpoint"], true, "frontend", "vue-module"),
    pluginManifest("momobako.filesystem-watcher", ["builtin.filesystem-watcher"], "Filesystem Watcher", "1.0.0", "watcher", "监听仓库目录，记录新增、删除、修改与重命名事件。", ["watch", "events", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.metadata-provider", ["builtin.metadata-provider"], "Metadata Provider", "1.0.0", "metadata", "提供可扩展的元数据生成与写入能力。", ["metadata", "tags", "ocr"], false, "backend", "manifest-only"),
    pluginManifest("momobako.vector-index", ["builtin.vector-index"], "Vector Index", "0.1.0", "search", "预留向量检索与 AI 语义搜索扩展点。", ["semantic-search", "embedding"], false, "backend", "manifest-only"),
  ];
}

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class MockChannel<T> {
    onmessage: ((message: T) => void) | null = null;
  },
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ command, args });
    if (mockInvokeDelay?.command === command) {
      await mockInvokeDelay.promise;
      mockInvokeDelay = null;
    }
    if (mockInvokeFailure?.command === command) {
      const failure = mockInvokeFailure.error;
      mockInvokeFailure = null;
      throw failure;
    }
    if (command === "list_repositories") return mockRepositories;
    if (command === "get_repository_snapshot") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : mockSnapshot.repository.repoId;
      const snapshot = getMockSnapshot(repoId);
      return {
        ...snapshot,
        repository: mockRepositories.find((item) => item.repoId === repoId) ?? snapshot.repository,
      };
    }
    if (command === "get_asset_detail") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : mockSnapshot.repository.repoId;
      const assetId = typeof args?.assetId === "string" ? args.assetId : "asset-01";
      const snapshot = getMockSnapshot(repoId);
      const summary = snapshot.assets.find((item) => item.assetId === assetId) ?? snapshot.assets[0];
      return {
        ...mockAssetDetail,
        summary,
      };
    }
    if (command === "search_assets") {
      const request = args?.request as SearchRequest | undefined;
      const results = filterSearchHits(request, mockSearchResults ?? defaultSearchHits());
      return {
        query: typeof request?.query === "string" ? request.query : "",
        results,
      };
    }
    if (command === "update_asset_metadata") {
      const request = args?.request as { metadata?: Record<string, unknown> } | undefined;
      const nextNote = request?.metadata?.note ?? mockAssetDetail.metadata[1].value;
      return {
        outcome: "success",
        asset: {
          ...mockAssetDetail,
          summary: {
            ...mockAssetDetail.summary,
            version: mockAssetDetail.summary.version + 1,
          },
          metadata: mockAssetDetail.metadata.map((entry) => (
            entry.key === "note" ? { ...entry, value: nextNote } : entry
          )),
        },
      };
    }
    if (command === "get_file_browser") {
      const request = args?.request as { repoId?: string; directoryPath?: string; includeTree?: boolean; specialLocation?: "trash" } | undefined;
      return getMockFileBrowser(
        request?.directoryPath ?? "",
        request?.includeTree ?? true,
        request?.specialLocation,
        request?.repoId ?? "repo-main-001",
      );
    }
    if (command === "read_file") {
      return [35, 32, 77, 111, 99, 107, 32, 102, 105, 108, 101];
    }
    if (command === "prepare_preview_file_source") {
      const request = args?.request as { repoId?: string; path?: string } | undefined;
      const path = request?.path ?? "model.glb";
      return {
        repoId: request?.repoId ?? "repo-main-001",
        path,
        token: "0".repeat(64),
        sourceUrl: `http://127.0.0.1:49152/preview/${"0".repeat(64)}`,
        mediaType: path.endsWith(".glb") || path.endsWith(".vrm")
          ? "model/gltf-binary"
          : path.endsWith(".gltf")
            ? "model/gltf+json"
            : path.endsWith(".pdf")
              ? "application/pdf"
              : path.endsWith(".docx")
                ? "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                : path.endsWith(".docm")
                  ? "application/vnd.ms-word.document.macroenabled.12"
                : path.endsWith(".xlsx")
                  ? "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                  : path.endsWith(".xlsm")
                    ? "application/vnd.ms-excel.sheet.macroenabled.12"
                    : path.endsWith(".pptx")
                      ? "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                      : path.endsWith(".pptm")
                        ? "application/vnd.ms-powerpoint.presentation.macroenabled.12"
            : path.endsWith(".mp4") || path.endsWith(".m4v")
              ? "video/mp4"
              : path.endsWith(".webm")
                ? "video/webm"
                : path.endsWith(".mp3")
                  ? "audio/mpeg"
                  : path.endsWith(".wav")
                    ? "audio/wav"
                    : "application/octet-stream",
        sizeBytes: 1024,
        modifiedAt: "2026-06-05T00:18:00Z",
      };
    }
    if (command === "create_directory") {
      const request = args?.request as { name?: string; parentPath?: string } | undefined;
      const name = request?.name ?? "NewFolder";
      const parentPath = request?.parentPath ?? "";
      const path = parentPath ? `${parentPath}/${name}` : name;
      addMockEntry(path, "directory");
      return getMockFileBrowser(parentPath, true);
    }
    if (command === "create_file") {
      const request = args?.request as { name?: string; parentPath?: string } | undefined;
      const name = request?.name ?? "note.txt";
      const parentPath = request?.parentPath ?? "";
      const path = parentPath ? `${parentPath}/${name}` : name;
      addMockEntry(path, "file");
      return getMockFileBrowser(parentPath, false);
    }
    if (command === "import_entries" || command === "copy_entries") {
      const request = args?.request as { parentPath?: string; sourcePaths?: string[] } | undefined;
      const parentPath = request?.parentPath ?? "";
      let importedDirectory = false;
      for (const sourcePath of request?.sourcePaths ?? []) {
        const normalizedSourcePath = sourcePath.replace(/\\/g, "/").replace(/\/+$/, "");
        const name = getEntryName(normalizedSourcePath);
        const kind = name.includes(".") ? "file" : "directory";
        const path = parentPath ? `${parentPath}/${name}` : name;
        addMockEntry(path, kind);
        importedDirectory ||= kind === "directory";
      }
      return getMockFileBrowser(parentPath, importedDirectory);
    }
    if (command === "list_hardlink_candidates") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      return { repoId, candidates: [] };
    }
    if (command === "confirm_hardlink_candidate") {
      const request = args?.request as { repoId?: string; candidateId?: string } | undefined;
      return {
        repoId: request?.repoId ?? "repo-main-001",
        candidate: {
          candidateId: request?.candidateId ?? "candidate-01",
          repoId: request?.repoId ?? "repo-main-001",
          newAssetId: "asset-new",
          newPath: "copy.psd",
          existingAssetId: "asset-01",
          existingPath: "cover-final.psd",
          contentHash: "sha256:mock",
          sizeBytes: 1,
          sizeLabel: "1 B",
          createdAt: "2026-06-05T00:18:00Z",
        },
        state: "linked",
      };
    }
    if (command === "rename_entry") {
      const request = args?.request as { path?: string; newName?: string } | undefined;
      const sourcePath = request?.path ?? "";
      const newName = request?.newName ?? "renamed.txt";
      const parentPath = getParentPath(sourcePath);
      const targetPath = parentPath ? `${parentPath}/${newName}` : newName;
      mockEntries = mockEntries.map((entry) => {
        if (entry.path === sourcePath) {
          return { ...entry, path: targetPath, name: newName };
        }
        if (entry.path.startsWith(`${sourcePath}/`)) {
          const nextPath = `${targetPath}${entry.path.slice(sourcePath.length)}`;
          return { ...entry, path: nextPath, name: getEntryName(nextPath) };
        }
        return entry;
      });
      return getMockFileBrowser(parentPath, mockEntries.some((entry) => entry.path === targetPath && entry.kind === "directory"));
    }
    if (command === "delete_entry") {
      const request = args?.request as { path?: string; mode?: "delete" | "moveToParent" | "permanentDelete" } | undefined;
      const targetPath = request?.path ?? "";
      const mode = request?.mode ?? "delete";
      const entries = mode === "permanentDelete" ? mockTrashEntries : mockEntries;
      const targetEntry = entries.find((entry) => entry.path === targetPath);
      const parentPath = getParentPath(targetPath);
      if (!targetEntry) {
        return getMockFileBrowser(parentPath, false, mode === "permanentDelete" ? "trash" : undefined);
      }

      if (targetEntry.kind === "directory" && mode === "moveToParent") {
        mockEntries = mockEntries
          .filter((entry) => entry.path !== targetPath)
          .map((entry) => {
            if (!entry.path.startsWith(`${targetPath}/`)) {
              return entry;
            }
            const relativePath = entry.path.slice(targetPath.length + 1);
            const nextPath = parentPath ? `${parentPath}/${relativePath}` : relativePath;
            return {
              ...entry,
              path: nextPath,
              name: getEntryName(nextPath),
            };
          });
        return getMockFileBrowser(parentPath, true);
      }

      if (mode === "permanentDelete") {
        mockTrashEntries = mockTrashEntries.filter((entry) => (
          entry.path !== targetPath && !entry.path.startsWith(`${targetPath}/`)
        ));
        return getMockFileBrowser(parentPath, false, "trash");
      }

      moveEntryTreeToTrash(targetPath);
      return getMockFileBrowser(parentPath, targetEntry.kind === "directory");
    }
    if (command === "mutate_trash") {
      const request = args?.request as { action?: "restore" | "restoreAll" | "empty"; path?: string } | undefined;
      if (request?.action === "restore" && request.path) {
        restoreTrashTree(request.path);
      } else if (request?.action === "restoreAll") {
        for (const entry of [...mockTrashEntries].filter((item) => !getParentPath(item.path))) {
          restoreTrashTree(entry.path);
        }
      } else if (request?.action === "empty") {
        mockTrashEntries = [];
      }
      return getMockFileBrowser("", false, "trash");
    }
    if (command === "create_repository" || command === "import_repository") {
      const request = args?.request as {
        name?: string;
        path?: string;
        backendPluginId?: string;
      } | undefined;
      const backendPluginId = request?.backendPluginId ?? "momobako.local-filesystem";
      const backendKind = backendPluginId === "momobako.webdav"
        ? "webdav"
        : backendPluginId === "momobako.cloud-drive"
          ? "cloud"
          : "filesystem";
      const backendName = backendPluginId === "momobako.webdav"
        ? "WebDAV"
        : backendPluginId === "momobako.cloud-drive"
          ? "Cloud Drive"
          : "Local Filesystem";
      mockRepositories = [
        {
          repoId: "repo-created-001",
          name: request?.name ?? "新资源库",
          path: request?.path ?? "C:/Mock/NewRepo",
          backend: {
            pluginId: backendPluginId,
            kind: backendKind,
            name: backendName,
            capabilities: ["browse", "read", "write", "sync"],
          },
          status: "ready",
          assetCount: 0,
          updatedAt: "2026-06-05T00:18:00Z",
        },
      ];
      return { repository: mockRepositories[0] };
    }
    if (command === "attach_repository_folder") {
      const request = args?.request as { path?: string } | undefined;
      const path = request?.path ?? "C:/Mock/NewRepo";
      mockRepositories = [
        {
          repoId: "repo-created-001",
          name: path.split("/").filter(Boolean).at(-1) ?? "NewRepo",
          path,
          backend: {
            pluginId: "momobako.local-filesystem",
            kind: "filesystem",
            name: "Local Filesystem",
            capabilities: ["browse", "read", "write", "watch", "sync"],
          },
          status: "ready",
          assetCount: 0,
          updatedAt: "2026-06-05T00:18:00Z",
        },
      ];
      return { repository: mockRepositories[0] };
    }
    if (command === "export_repository") {
      const request = args?.request as { target?: string; archive?: { outputPath?: string; format?: string; encrypt?: boolean }; git?: { remote?: string; branch?: string } } | undefined;
      return {
        repository: mockRepositories[0],
        target: request?.target ?? "archive",
        outputPath: request?.archive?.outputPath,
        format: request?.archive?.format,
        encrypted: request?.archive?.encrypt,
        remote: request?.git?.remote,
        branch: request?.git?.branch,
        message: request?.target === "git" ? "资源库已上传到 Git" : "资源库压缩包已导出",
      };
    }
    if (command === "delete_repository") return undefined;
    if (command === "sync_repository") {
      if (mockDirectoryCreatedOnNextSync) {
        addMockEntry(mockDirectoryCreatedOnNextSync, "directory");
        mockDirectoryCreatedOnNextSync = null;
      }
      return {
        repoId: "repo-main-001",
        scannedFiles: 6,
        createdAssets: 1,
        updatedAssets: 5,
        deletedAssets: 0,
        createdEvents: 6,
        hardlinkCandidates: 0,
      };
    }
    if (command === "ensure_thumbnail") {
      const request = args?.request as {
        repoId?: string;
        path?: string;
        action?: "ensure" | "refresh" | "save" | "saveGenerated" | "clear";
        sourcePath?: string;
        imageBytes?: number[];
      } | undefined;
      const path = request?.path ?? "";
      const entries = mockEntries.find((item) => item.path === path)
        ? mockEntries
        : mockTrashEntries;
      const entry = entries.find((item) => item.path === path);
      const action = request?.action ?? "ensure";
      if (entry && (action === "save" || action === "saveGenerated")) {
        entry.thumbnailPath = `C:/Mock/Thumbs/${path.replace(/[\\/]/g, "__")}.jpg`;
        entry.thumbnailCustom = action === "save";
      } else if (entry && action === "clear") {
        entry.thumbnailPath = null;
        entry.thumbnailCustom = false;
      } else if (entry && action === "refresh") {
        entry.thumbnailPath = entry.kind === "file" ? `C:/Mock/Thumbs/${path.replace(/[\\/]/g, "__")}.jpg` : null;
        entry.thumbnailCustom = false;
      } else if (entry && !entry.thumbnailPath && entry.kind === "file") {
        entry.thumbnailPath = `C:/Mock/Thumbs/${path.replace(/[\\/]/g, "__")}.jpg`;
      }
      return {
        repoId: request?.repoId ?? "repo-main-001",
        path,
        assetId: entry?.assetId ?? `asset-${path.replace(/[^a-z0-9]/gi, "-")}`,
        kind: entry?.kind ?? "file",
        thumbnailPath: entry?.thumbnailPath ?? null,
        thumbnailCustom: entry?.thumbnailCustom ?? false,
      };
    }
    if (command === "undo_last_revision" || command === "redo_last_revision") {
      return {
        outcome: "success",
        asset: mockAssetDetail,
      };
    }
    if (command === "list_plugins") {
      mockPlugins ??= createMockPlugins();
      return mockPlugins;
    }
    if (command === "set_plugin_enabled") {
      mockPlugins ??= createMockPlugins();
      const request = args?.request as { pluginId?: string; enabled?: boolean } | undefined;
      mockPlugins = mockPlugins.map((plugin) => (
        plugin.pluginId === request?.pluginId
          ? {
              ...plugin,
              enabled: Boolean(request.enabled),
              status: request.enabled ? "ready" : "disabled",
            }
          : plugin
      ));
      return { plugins: mockPlugins };
    }
    if (command === "delete_plugin") {
      mockPlugins ??= createMockPlugins();
      const pluginId = args?.pluginId;
      mockPlugins = mockPlugins.filter((plugin) => plugin.pluginId !== pluginId);
      return { plugins: mockPlugins };
    }
    if (command === "install_plugin_from_archive") {
      mockPlugins ??= createMockPlugins();
      mockPlugins = [
        ...mockPlugins,
        pluginManifest("user.sample-plugin", [], "Sample Plugin", "0.1.0", "metadata", "从压缩包安装的测试插件。", ["metadata"], true, "backend", "manifest-only", "user"),
      ];
      return { plugins: mockPlugins };
    }
    if (command === "get_cache_snapshot") {
      return {
        config: {
          metadataCapacity: 2048,
          thumbnailCapacity: 512,
          queryCapacity: 128,
        },
        entries: [
          {
            cacheType: "metadata",
            key: "repo-main-001:asset-01",
            lastAccessedAt: "2026-06-05T00:18:00Z",
          },
        ],
      };
    }
    if (command === "get_api_design_snapshot") {
      return {
        transport: "REST over local repository service, gRPC-ready contract design",
        endpoints: [
          {
            group: "Repository API",
            method: "GET",
            path: "/repositories",
            summary: "列出所有仓库。",
          },
        ],
      };
    }
    return null;
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: async (options?: { filters?: unknown[] }) => {
    if (options?.filters) return mockSelectedFile;
    return mockSelectedFolder;
  },
  save: async () => mockSavePath,
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: async (path: string) => {
    openerCalls.push({ command: "openPath", path });
    if (mockOpenerFailure) {
      const failure = mockOpenerFailure;
      mockOpenerFailure = null;
      throw failure;
    }
  },
  revealItemInDir: async (path: string) => {
    openerCalls.push({ command: "revealItemInDir", path });
    if (mockOpenerFailure) {
      const failure = mockOpenerFailure;
      mockOpenerFailure = null;
      throw failure;
    }
  },
}));

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  mockSelectedFolder = null;
  mockSelectedFile = null;
  mockSavePath = "C:/Mock/Exports/repository.zip";
  mockDirectoryCreatedOnNextSync = null;
  mockOpenerFailure = null;
  mockInvokeFailure = null;
  mockInvokeDelay = null;
  mockSearchResults = null;
  mockPlugins = null;
  mockRepositories = [];
  mockEntries = initialEntries();
  mockTrashEntries = [];
  invokeCalls.length = 0;
  openerCalls.length = 0;
});

export function getInvokeCalls(command?: string) {
  return command ? invokeCalls.filter((call) => call.command === command) : [...invokeCalls];
}

export function seedMockRepository() {
  mockRepositories = [mockSnapshot.repository];
}

export function seedCrossRepositorySearchHit() {
  mockRepositories = [mockSnapshot.repository, altRepository];
  const asset = altSnapshot.assets[0];
  mockSearchResults = [
    {
      repoId: altRepository.repoId,
      repoName: altRepository.name,
      assetId: asset.assetId,
      path: asset.path,
      filename: asset.filename,
      status: asset.status,
      tags: asset.tags,
      metadata: { note: "跨仓库命中" },
    },
  ];
}

export function setMockSavePath(path: string | null) {
  mockSavePath = path;
}

export function seedMockRepositoryPath(path: string) {
  mockRepositories = [
    {
      ...mockSnapshot.repository,
      path,
    },
  ];
}

export function selectMockFolder(path: string) {
  mockSelectedFolder = path;
}

export function selectMockFile(path: string | null) {
  mockSelectedFile = path;
}

export function createDirectoryOnNextSync(path: string) {
  mockDirectoryCreatedOnNextSync = path;
}

export function getOpenerCalls(command?: "openPath" | "revealItemInDir") {
  return command ? openerCalls.filter((call) => call.command === command) : [...openerCalls];
}

export function failNextOpenerCall(message: string) {
  mockOpenerFailure = new Error(message);
}

export function failNextInvoke(command: string, message: string) {
  mockInvokeFailure = { command, error: new Error(message) };
}

export function delayNextInvoke(command: string) {
  let resolveDelay = () => {};
  const promise = new Promise<void>((resolve) => {
    resolveDelay = resolve;
  });
  mockInvokeDelay = {
    command,
    resolve: resolveDelay,
    promise,
  };
  return {
    resolve: resolveDelay,
  };
}
