import type {
  PluginManifest,
  RepositoryAction,
  RepositoryLocalCacheStatus,
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
  localCache?: RepositoryLocalCacheStatus | null;
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
  isVirtual?: boolean;
  providerId?: string | null;
  providerItemId?: string | null;
  sourcePayload?: Record<string, unknown> | null;
  localAbsolutePath?: string | null;
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
    path: "Backgrounds/scene-forest-03.png",
    name: "scene-forest-03.png",
    kind: "file",
    extension: "png",
    sizeBytes: 15245312,
    sizeLabel: "14.5 MB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: "asset-02",
    status: "synced",
    tags: ["背景", "森林", "PNG"],
    metadata: {
      note: "森林场景背景。",
      fileCreatedAt: "2026-06-04T00:00:00Z",
      width: 1280,
      height: 720,
    },
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
  {
    path: "loose-note.txt",
    name: "loose-note.txt",
    kind: "file",
    extension: "txt",
    sizeBytes: 2048,
    sizeLabel: "2 KB",
    modifiedAt: "2026-06-06T08:00:00Z",
    assetId: "asset-03",
    status: "synced",
    tags: [],
    metadata: {
      note: "根目录待整理文件。",
      fileCreatedAt: "2026-06-06T08:00:00Z",
    },
  },
];

export const initialTrashEntries = (): MockEntry[] => [
  {
    path: "deleted-draft.png",
    name: "deleted-draft.png",
    kind: "file",
    extension: "png",
    sizeBytes: 6144,
    sizeLabel: "6 KB",
    modifiedAt: "2026-06-03T12:00:00Z",
    assetId: "asset-04",
    status: "deleted",
    tags: [],
    metadata: {
      deletedAt: "2026-06-30T12:00:00Z",
      originalPath: "Drafts/deleted-draft.png",
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
    assetCount: 4,
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
      lastAccessedAt: "2026-06-12T09:00:00Z",
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
      lastAccessedAt: "2026-06-10T09:00:00Z",
      version: 1,
      tags: ["背景", "森林", "PNG"],
    },
    {
      assetId: "asset-03",
      repoId: "repo-main-001",
      path: "loose-note.txt",
      filename: "loose-note.txt",
      extension: "txt",
      sizeBytes: 2048,
      sizeLabel: "2 KB",
      status: "synced",
      modifiedAt: "2026-06-06T08:00:00Z",
      lastAccessedAt: null,
      version: 1,
      tags: [],
    },
    {
      assetId: "asset-04",
      repoId: "repo-main-001",
      path: "Drafts/deleted-draft.png",
      filename: "deleted-draft.png",
      extension: "png",
      sizeBytes: 6144,
      sizeLabel: "6 KB",
      status: "deleted",
      modifiedAt: "2026-06-03T12:00:00Z",
      lastAccessedAt: null,
      version: 1,
      tags: [],
    },
  ],
  metadataFields: ["comment", "favorite", "note", "rating", "title"],
  recentRevisionCount: 6,
  overview: {
    totalSizeBytes: 254197760,
    totalSizeLabel: "242.4 MB",
    fileCount: 4,
    folderCount: 3,
    trashCount: 1,
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
      lastAccessedAt: null,
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
    trashCount: 0,
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
  layer: "source" | "library-kind" | "extractor-parser" | "provider-service" | "integration-capability-hook",
  kind: string,
  description: string,
  capabilities: string[],
  enabled: boolean,
  sdk: "frontend" | "backend",
  runtime: "vue-module" | "native-dylib" | "manifest-only",
  source: "builtin" | "user" | "system" = "builtin",
): PluginManifest {
  const category =
    layer === "extractor-parser" ? "parser"
    : layer === "provider-service" || layer === "integration-capability-hook" ? "service"
    : layer;
  const previewExtensions =
    pluginId === "momobako.preview.three-model" ? ["fbx", "obj", "glb", "gltf", "vrm", "stl", "3mf", "blend"]
    : pluginId === "momobako.preview.media" ? ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg", "mp4", "mov", "mkv", "webm", "avi", "m4v", "mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"]
    : pluginId === "momobako.preview.text" ? ["txt", "md", "markdown", "json", "yaml", "yml", "csv"]
    : pluginId === "momobako.preview.office"
      ? [
          "pdf",
          "doc", "docx", "docm", "dotx", "dotm", "dot",
          "xls", "xlsx", "xlsm", "xlsb", "xltx", "xltm", "xlt",
          "ppt", "pptx", "pptm", "ppsx", "ppsm", "pps", "potx", "potm", "pot",
        ]
    : pluginId === "momobako.preview.archive" ? ["zip", "cbz", "7z", "rar", "cbr"]
    : [];
  const hooks =
    pluginId === "momobako.preview.media"
      ? [
          { slot: "playlist", action: "preview.media.enqueue", label: "加入播放列表" },
          { slot: "pip", action: "preview.media.openPip", label: "画中画" },
          { slot: "progress", action: "preview.media.reportProgress", label: "更新播放进度" },
        ]
      : pluginId === "momobako.preview.office"
        ? [{ slot: "progress", action: "preview.office.reportReadPosition", label: "更新阅读进度" }]
      : pluginId === "momobako.filesystem-watcher"
        ? [{ slot: "auditLog", action: "service.watcher.recordEvents", label: "记录文件事件" }]
      : pluginId === "momobako.vector-index"
        ? [{ slot: "search", action: "service.vector.search", label: "语义搜索" }]
      : pluginId === "momobako.tool.api-playground"
        ? [{ slot: "toolPage", action: "tool.apiPlayground.open", label: "打开 API Playground" }]
      : [];
  const requires =
    pluginId === "momobako.preview.archive" ? ["momobako.service.archive-preview"]
    : [];
  const optional =
    pluginId === "momobako.local-filesystem" ? ["momobako.filesystem-watcher"]
    : pluginId === "momobako.preview.media" ? ["momobako.parser.image", "momobako.parser.audio", "momobako.parser.video"]
    : pluginId === "momobako.preview.office" ? ["momobako.parser.ebook"]
    : pluginId === "momobako.metadata-provider" ? ["momobako.service.network-search"]
    : [];
  const permissions =
    pluginId === "momobako.local-filesystem" ? ["filesystem:read", "filesystem:write"]
    : pluginId === "momobako.webdav" || pluginId === "momobako.cloud-drive" ? ["network", "filesystem:read", "filesystem:write"]
    : pluginId.startsWith("momobako.preview.") ? ["preview:read"]
    : pluginId === "momobako.preview.office" ? ["preview:read", "thumbnail:write"]
    : pluginId === "momobako.tool.api-playground" ? ["network:localhost", "external-api:read", "external-api:write"]
    : pluginId === "momobako.filesystem-watcher" || pluginId === "momobako.vector-index" ? ["filesystem:read"]
    : [];
  const contributes =
    previewExtensions.length > 0 ? {
      preview: {
        extensions: previewExtensions,
        thumbnail: pluginId === "momobako.preview.text" || pluginId === "momobako.preview.office",
      },
      ...(pluginId === "momobako.preview.media"
        ? {
            playlistPlayers: [
              {
                playerTypeId: "momobako.playlist.image-slideshow",
                label: "图片幻灯片",
                fileClass: "image",
                supportedExtensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"],
                supportsSeek: false,
                supportsVolume: false,
                supportsPreviewNavigation: true,
                description: "按顺序展示图片并交由宿主处理队列模式。",
              },
              {
                playerTypeId: "momobako.playlist.audio-sequence",
                label: "音频顺序播放",
                fileClass: "audio",
                supportedExtensions: ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"],
                supportsSeek: true,
                supportsVolume: true,
                supportsPreviewNavigation: true,
                description: "复用媒体能力播放音频队列。",
              },
              {
                playerTypeId: "momobako.playlist.video-sequence",
                label: "视频顺序播放",
                fileClass: "video",
                supportedExtensions: ["mp4", "mov", "mkv", "webm", "avi", "m4v"],
                supportsSeek: true,
                supportsVolume: true,
                supportsPreviewNavigation: true,
                description: "复用媒体能力播放视频队列。",
              },
            ],
          }
        : {}),
    }
    : category === "source" ? {
      source: {
        operations: ["list", "read", "write", "move", "delete", "watch"],
        dangerousOperations: ["write", "move", "delete"],
      },
      ...(pluginId === "momobako.local-filesystem"
        ? {
            settings: {
              schemaVersion: 1,
              settingsPage: {
                label: "本地文件系统",
                description: "配置本地资源库的文件检索方式。",
                order: 10,
              },
              fields: [
                {
                  key: "fileSearchMode",
                  label: "文件检索方式",
                  type: "select",
                  description: "NTFS 与 Everything 不可用时会自动回退到现有扫描。",
                  default: "recursive",
                  options: [
                    { label: "现有扫描", value: "recursive" },
                    { label: "NTFS 索引", value: "ntfs" },
                    { label: "Everything 索引", value: "everything" },
                  ],
                },
              ],
            },
          }
        : {}),
    }
    : pluginId === "momobako.filesystem-watcher" ? {
      service: {
        type: "watcher",
      },
    }
    : pluginId === "momobako.vector-index" ? {
      service: {
        type: "semantic-search",
        candidateOnly: true,
      },
    }
    : pluginId === "momobako.tool.api-playground" ? {
      toolPages: [
        {
          toolPageId: "momobako.tool.api-playground",
          label: "API Playground",
          description: "调试 /external/v1 后端接口",
          order: 10,
        },
      ],
    }
    : {};
  return {
    pluginId,
    legacyPluginIds,
    name,
    version,
    type: {
      layer,
      kind,
    },
    kind,
    category,
    description,
    capabilities,
    enabled,
    sdk,
    entry: sdk === "frontend"
      ? {
          frontend: {
            module: "dist/register.js",
            export: "register",
          },
        }
      : {},
    source,
    runtime,
    permissions,
    requires,
    optional,
    hooks,
    contributes,
    compat: { sdkVersion: "1", legacyPluginIds },
    status: enabled ? "ready" : "disabled",
    dependencyStatus: {
      required: [],
      optional: [],
      missingRequired: [],
      missingOptional: [],
      disabledRequired: [],
      disabledOptional: [],
    },
    disableReason: enabled ? null : "插件已被禁用。",
    degraded: false,
    degradationReason: null,
  };
}

export function createMockPlugins() {
  return [
    pluginManifest("momobako.service.archive-preview", [], "Archive Preview Service", "0.1.0", "provider-service", "archive-preview", "Provides read-only archive extraction and internal file preview support.", ["archive", "preview", "read", "readonly"], true, "backend", "native-dylib"),
    pluginManifest("momobako.preview.archive", [], "Archive Preview", "0.1.0", "library-kind", "preview", "Previews ZIP, CBZ, 7Z, RAR and CBR archives as read-only containers.", ["preview", "archive", "readonly"], true, "frontend", "vue-module"),
    pluginManifest("momobako.local-filesystem", ["builtin.local-filesystem"], "Local Filesystem", "1.0.0", "source", "filesystem", "使用本地目录作为仓库文件管理后端。", ["browse", "read", "write", "watch", "sync", "localRootPath"], true, "backend", "native-dylib"),
    pluginManifest("momobako.webdav", ["builtin.webdav"], "WebDAV", "0.1.0", "source", "webdav", "通过 WebDAV 适配远程文件管理服务。", ["browse", "read", "write", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.cloud-drive", ["builtin.cloud-drive"], "Cloud Drive", "0.1.0", "source", "cloud", "预留云盘文件系统接入点，如对象存储或网盘。", ["browse", "read", "write", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.preview.three-model", ["builtin.three-model-preview"], "3D Model Preview", "1.0.0", "library-kind", "preview", "为 FBX、OBJ、GLB、glTF 与 VRM 模型提供可旋转缩放的 3D 文件预览。", ["preview", "3d-model", "fbx", "obj", "gltf", "vrm"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.media", ["builtin.media-preview"], "Media Preview", "1.0.0", "library-kind", "preview", "为常见图片、视频与音频文件提供内联预览和播放列表播放能力。", ["preview", "playlist", "media", "image", "video", "audio"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.text", ["builtin.text-preview"], "Text Preview", "1.0.0", "library-kind", "preview", "为常见文本与 Markdown 文件提供阅读预览，并生成文本缩略图。", ["preview", "text", "markdown", "thumbnail"], true, "frontend", "vue-module"),
    pluginManifest("momobako.preview.office", ["builtin.office-preview"], "Office & PDF Preview", "1.0.0", "library-kind", "preview", "为 Microsoft Office 文档与 PDF 文件提供预览，并生成文档缩略图。", ["preview", "thumbnail", "pdf", "office", "word", "excel", "powerpoint"], true, "frontend", "vue-module"),
    pluginManifest("momobako.tool.api-playground", [], "API Playground", "0.1.0", "integration-capability-hook", "api-playground", "在 MomoBako 内调试本机外部后端 API。", ["tool-page", "api-playground", "external-api"], true, "frontend", "vue-module"),
    pluginManifest("momobako.filesystem-watcher", ["builtin.filesystem-watcher"], "Filesystem Watcher", "1.0.0", "integration-capability-hook", "watcher", "监听仓库目录，记录新增、删除、修改与重命名事件。", ["watch", "events", "sync"], false, "backend", "manifest-only"),
    pluginManifest("momobako.metadata-provider", ["builtin.metadata-provider"], "Metadata Provider", "1.0.0", "provider-service", "metadata", "提供可扩展的元数据生成与写入能力。", ["metadata", "tags", "ocr"], false, "backend", "manifest-only"),
    pluginManifest("momobako.vector-index", ["builtin.vector-index"], "Vector Index", "0.1.0", "provider-service", "search", "预留向量检索与 AI 语义搜索扩展点。", ["semantic-search", "embedding"], false, "backend", "manifest-only"),
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
