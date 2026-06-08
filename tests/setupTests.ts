import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/vue";
import { afterEach, vi } from "vitest";

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
};

let mockRepositories: MockRepository[] = [];
let mockSelectedFolder: string | null = null;
let mockDirectoryCreatedOnNextSync: string | null = null;
let mockOpenerFailure: Error | null = null;
let mockInvokeFailure: { command: string; error: Error } | null = null;
let mockInvokeDelay: { command: string; resolve: () => void; promise: Promise<void> } | null = null;
const invokeCalls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const openerCalls: Array<{ command: "openPath" | "revealItemInDir"; path: string }> = [];

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

let mockEntries = initialEntries();

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

function buildTree() {
  type TreeNode = { path: string; label: string; children: TreeNode[] };
  const roots: TreeNode[] = [];
  const nodeMap = new Map<string, TreeNode>();
  const directoryEntries = mockEntries
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

function getEntriesForDirectory(directoryPath: string) {
  return mockEntries
    .filter((entry) => getParentPath(entry.path) === directoryPath)
    .sort((left, right) => {
      if (left.kind !== right.kind) {
        return left.kind === "directory" ? -1 : 1;
      }
      return left.path.localeCompare(right.path);
    });
}

function getMockFileBrowser(directoryPath = "", includeTree = true) {
  const snapshot: {
    repoId: string;
    rootPath: string;
    backendPluginId: string;
    backendKind: string;
    currentPath: string;
    tree?: ReturnType<typeof buildTree>;
    entries: ReturnType<typeof getEntriesForDirectory>;
  } = {
    repoId: "repo-main-001",
    rootPath: "C:/Mock/AnimeAssets",
    backendPluginId: "builtin.local-filesystem",
    backendKind: "filesystem",
    currentPath: directoryPath,
    entries: getEntriesForDirectory(directoryPath),
  };
  if (includeTree) {
    snapshot.tree = buildTree();
  }
  return snapshot;
}

const mockSnapshot = {
  repository: {
    repoId: "repo-main-001",
    name: "主资源库",
    path: "C:/Mock/AnimeAssets",
    backend: {
      pluginId: "builtin.local-filesystem",
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

vi.mock("@tauri-apps/api/core", () => ({
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
      return {
        ...mockSnapshot,
        repository: mockRepositories.find((item) => item.repoId === repoId) ?? mockSnapshot.repository,
      };
    }
    if (command === "get_asset_detail") {
      const assetId = typeof args?.assetId === "string" ? args.assetId : "asset-01";
      const summary = mockSnapshot.assets.find((item) => item.assetId === assetId) ?? mockSnapshot.assets[0];
      return {
        ...mockAssetDetail,
        summary,
      };
    }
    if (command === "search_assets") {
      const request = args?.request as { query?: string } | undefined;
      return {
        query: typeof request?.query === "string" ? request.query : "",
        results: [
          {
            repoId: "repo-main-001",
            repoName: "主资源库",
            assetId: "asset-01",
            path: "Campaigns/Summer/cover-final.psd",
            filename: "cover-final.psd",
            status: "synced",
            tags: ["封面", "主视觉", "PSD"],
            metadata: { note: "最终版封面，保留可编辑图层。" },
          },
        ],
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
      const request = args?.request as { directoryPath?: string; includeTree?: boolean } | undefined;
      return getMockFileBrowser(request?.directoryPath ?? "", request?.includeTree ?? true);
    }
    if (command === "read_file") {
      return [35, 32, 77, 111, 99, 107, 32, 102, 105, 108, 101];
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
    if (command === "import_entries") {
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
      const request = args?.request as { path?: string; mode?: "delete" | "moveToParent" } | undefined;
      const targetPath = request?.path ?? "";
      const mode = request?.mode ?? "delete";
      const targetEntry = mockEntries.find((entry) => entry.path === targetPath);
      const parentPath = getParentPath(targetPath);
      if (!targetEntry) {
        return getMockFileBrowser(parentPath);
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

      mockEntries = mockEntries.filter((entry) => (
        entry.path !== targetPath && !entry.path.startsWith(`${targetPath}/`)
      ));
      return getMockFileBrowser(parentPath, targetEntry.kind === "directory");
    }
    if (command === "create_repository" || command === "import_repository") {
      const request = args?.request as {
        name?: string;
        path?: string;
        backendPluginId?: string;
      } | undefined;
      const backendPluginId = request?.backendPluginId ?? "builtin.local-filesystem";
      const backendKind = backendPluginId === "builtin.webdav"
        ? "webdav"
        : backendPluginId === "builtin.cloud-drive"
          ? "cloud"
          : "filesystem";
      const backendName = backendPluginId === "builtin.webdav"
        ? "WebDAV"
        : backendPluginId === "builtin.cloud-drive"
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
            pluginId: "builtin.local-filesystem",
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
      return { repository: mockRepositories[0] };
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
      };
    }
    if (command === "undo_last_revision" || command === "redo_last_revision") {
      return {
        outcome: "success",
        asset: mockAssetDetail,
      };
    }
    if (command === "list_plugins") {
      return [
        {
          pluginId: "builtin.local-filesystem",
          name: "Local Filesystem",
          version: "1.0.0",
          kind: "filesystem",
          description: "使用本地目录作为仓库文件管理后端。",
          capabilities: ["browse", "read", "write", "watch", "sync"],
          enabled: true,
        },
        {
          pluginId: "builtin.webdav",
          name: "WebDAV",
          version: "0.1.0",
          kind: "webdav",
          description: "通过 WebDAV 适配远程文件管理服务。",
          capabilities: ["browse", "read", "write", "sync"],
          enabled: false,
        },
        {
          pluginId: "builtin.cloud-drive",
          name: "Cloud Drive",
          version: "0.1.0",
          kind: "cloud",
          description: "预留云盘文件系统接入点，如对象存储或网盘。",
          capabilities: ["browse", "read", "write", "sync"],
          enabled: false,
        },
        {
          pluginId: "builtin.three-model-preview",
          name: "3D Model Preview",
          version: "1.0.0",
          kind: "preview",
          description: "为 FBX、OBJ、GLB 与 glTF 模型提供可旋转缩放的 3D 文件预览。",
          capabilities: ["preview", "3d-model", "fbx", "obj", "gltf"],
          enabled: true,
        },
        {
          pluginId: "builtin.filesystem-watcher",
          name: "Filesystem Watcher",
          version: "1.0.0",
          kind: "watcher",
          description: "监听仓库目录，记录新增、删除、修改与重命名事件。",
          capabilities: ["watch", "events", "sync"],
          enabled: true,
        },
      ];
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
  open: async () => mockSelectedFolder,
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
  mockDirectoryCreatedOnNextSync = null;
  mockOpenerFailure = null;
  mockInvokeFailure = null;
  mockInvokeDelay = null;
  mockRepositories = [];
  mockEntries = initialEntries();
  invokeCalls.length = 0;
  openerCalls.length = 0;
});

export function getInvokeCalls(command?: string) {
  return command ? invokeCalls.filter((call) => call.command === command) : [...invokeCalls];
}

export function seedMockRepository() {
  mockRepositories = [mockSnapshot.repository];
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
