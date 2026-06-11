import type {
  PluginManifest,
  RepositoryAction,
  SearchHit,
} from "../../src/types/repository";

export type MockRepository = {
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

export type MockEntry = {
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
  tags?: string[];
  aliasPaths?: string[];
  folderMetadata?: {
    protected: boolean;
    passwordTip?: string | null;
  } | null;
  metadata?: Record<string, unknown>;
};

export const defaultSearchHits = (): SearchHit[] => [
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
      color: "#336699",
      palette: ["#336699", "#88AACC"],
      shape: "方形",
      rating: 5,
      width: 1920,
      originalSizeBytes: 238950400,
      fileCreatedAt: "2026-06-01T00:00:00Z",
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
      width: 1280,
      originalSizeBytes: 15245312,
      fileCreatedAt: "2026-06-04T00:00:00Z",
    },
  },
];

export const initialEntries = (): MockEntry[] => [
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
    folderMetadata: {
      protected: true,
      passwordTip: "项目归档密码提示",
    },
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
    path: "Campaigns/Summer/cover-final.psd",
    name: "cover-final.psd",
    kind: "file",
    extension: "psd",
    sizeBytes: 238950400,
    sizeLabel: "227.9 MB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-01",
    status: "synced",
    tags: ["封面", "主视觉", "PSD"],
    aliasPaths: [
      "cover-final.psd",
      "Campaigns/Summer/cover-final.psd",
    ],
    metadata: {
      note: "最终版封面，保留可编辑图层。",
      link: "https://example.test/source/cover",
      addedToLibraryAt: "2026-06-01T00:18:00Z",
      fileCreatedAt: "2026-06-02T00:18:00Z",
      fileModifiedAt: "2026-06-05T00:18:00Z",
      width: 1920,
      height: 1080,
      originalSizeBytes: 238950400,
      palette: ["#336699", "#88AACC"],
      tagGroups: ["封面", "主视觉"],
    },
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
    tags: ["封面", "主视觉", "PSD"],
    aliasPaths: [
      "cover-final.psd",
      "Campaigns/Summer/cover-final.psd",
    ],
    metadata: {
      note: "最终版封面，保留可编辑图层。",
      link: "https://example.test/source/cover",
      addedToLibraryAt: "2026-06-01T00:18:00Z",
      fileCreatedAt: "2026-06-02T00:18:00Z",
      fileModifiedAt: "2026-06-05T00:18:00Z",
      width: 1920,
      height: 1080,
      originalSizeBytes: 238950400,
      palette: ["#336699", "#88AACC"],
      tagGroups: ["封面", "主视觉"],
    },
  },
];

export const mockSnapshot = {
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
  quickAccess: [
    {
      shortcutId: "shortcut-folder-campaigns",
      repoId: "repo-main-001",
      label: "快捷 Campaigns",
      targetKind: "folder",
      targetPath: "Campaigns",
      targetId: null,
      sortOrder: 0,
      createdAt: "2026-06-05T00:18:00Z",
    },
    {
      shortcutId: "shortcut-file-cover",
      repoId: "repo-main-001",
      label: "封面文件",
      targetKind: "file",
      targetPath: "Campaigns/Summer/cover-final.psd",
      targetId: null,
      sortOrder: 1,
      createdAt: "2026-06-05T00:18:00Z",
    },
  ],
  tagGroups: [
    {
      tagGroupId: "tag-group-project",
      repoId: "repo-main-001",
      name: "项目标签",
      tags: ["封面", "主视觉", "背景"],
      sortOrder: 0,
      createdAt: "2026-06-05T00:18:00Z",
      updatedAt: "2026-06-05T00:18:00Z",
    },
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
  metadataFields: ["comment", "favorite", "note", "rating", "title"],
  recentRevisionCount: 6,
  overview: {
    totalSizeBytes: 254195712,
    totalSizeLabel: "242.4 MB",
    fileCount: 2,
    folderCount: 3,
    readmeContent: "# 主资源库\n用于存放项目主视觉、背景和衍生素材。\n",
  },
};

export const altRepository = {
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

export const altEntries: MockEntry[] = [
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

export const altSnapshot = {
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

export const mockAssetDetail = {
  summary: mockSnapshot.assets[0],
  metadata: [
    { key: "favorite", valueType: "boolean", value: true, version: 1, updatedAt: "2026-06-05T00:18:00Z" },
    { key: "comment", valueType: "string", value: "最终版封面，保留可编辑图层。", version: 1, updatedAt: "2026-06-05T00:18:00Z" },
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
      after: { comment: "最终版封面，保留可编辑图层。", note: "最终版封面，保留可编辑图层。", rating: 5 },
      source: "seed",
    },
  ],
};

export function pluginManifest(
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

export function createMockPlugins() {
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

export function defaultRepositoryActions(): RepositoryAction[] {
  return [
    {
      actionId: "action-ready",
      repoId: "repo-main-001",
      source: "eagle-importer",
      sourceActionId: "eagle-action-ready",
      name: "标记精选",
      status: "ready",
      enabled: true,
      raw: { id: "eagle-action-ready", name: "标记精选" },
      unsupportedReason: null,
      sortOrder: 0,
      createdAt: "2026-06-05T00:18:00Z",
      updatedAt: "2026-06-05T00:18:00Z",
      steps: [
        {
          stepId: "action-ready-step-1",
          actionId: "action-ready",
          repoId: "repo-main-001",
          stepKind: "metadata.update",
          label: "更新元数据",
          status: "ready",
          config: { metadata: { rating: 5 } },
          raw: { type: "rating", rating: 5 },
          unsupportedReason: null,
          sortOrder: 0,
        },
      ],
      lastRun: null,
    },
    {
      actionId: "action-unsupported",
      repoId: "repo-main-001",
      source: "eagle-importer",
      sourceActionId: "eagle-action-unsupported",
      name: "外部导出",
      status: "unsupported",
      enabled: false,
      raw: { id: "eagle-action-unsupported", name: "外部导出" },
      unsupportedReason: "unsupported action step: shell",
      sortOrder: 1,
      createdAt: "2026-06-05T00:18:00Z",
      updatedAt: "2026-06-05T00:18:00Z",
      steps: [
        {
          stepId: "action-unsupported-step-1",
          actionId: "action-unsupported",
          repoId: "repo-main-001",
          stepKind: "unsupported",
          label: "未支持步骤 1",
          status: "unsupported",
          config: {},
          raw: { type: "shell", command: "open-external-app" },
          unsupportedReason: "unsupported action step: shell",
          sortOrder: 0,
        },
      ],
      lastRun: null,
    },
  ];
}
