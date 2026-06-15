import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/vue";
import { afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type {
  FileBrowserEntry,
  PlaylistDetail,
  PlaylistSummary,
  PluginManifest,
  RepositoryAction,
  SearchHit,
  SearchRequest,
  SmartFolder,
  SmartFolderFilter,
  SmartFolderTreeNode,
} from "../src/types/repository";
import {
  altEntries,
  altRepository,
  altSnapshot,
  createMockPlugins,
  defaultRepositoryActions,
  defaultSearchHits,
  initialEntries,
  mockAssetDetail,
  mockSnapshot,
  pluginManifest,
} from "./fixtures/repositoryFixtures";
import type { MockEntry, MockRepository } from "./fixtures/repositoryFixtures";

const missingRepositoryPath = "C:/Mock/MissingAnimeAssets";
const relocatedRepositoryPath = "C:/Mock/RelocatedAnimeAssets";


let mockRepositories: MockRepository[] = [];
let mockSelectedFolder: string | null = null;
let mockSelectedFile: string | null = null;
let mockSavePath: string | null = "C:/Mock/Exports/repository.zip";
let mockDirectoryCreatedOnNextSync: string | null = null;
let mockOpenerFailure: Error | null = null;
let mockInvokeFailure: { command: string; error: Error } | null = null;
let mockInvokeDelay: { command: string; resolve: () => void; promise: Promise<void> } | null = null;
let mockSearchResults: SearchHit[] | null = null;
let mockSmartFolders: SmartFolder[] = [];
let mockRepositoryActions: RepositoryAction[] = [];
let mockPlugins: PluginManifest[] | null = null;
let mockPlaylists: PlaylistSummary[] | null = null;
let mockPlaylistDetails: Record<string, PlaylistDetail> = {};
let mockPluginConfigValues: Record<string, Record<string, unknown>> = {};
const pluginCallCalls: Array<{ pluginId: string; method: string; payload: unknown }> = [];
const pluginCallMockResponses = new Map<string, unknown>();
const invokeCalls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const openerCalls: Array<{ command: "openPath" | "openUrl" | "revealItemInDir"; path: string }> = [];

globalThis.fetch = vi.fn(async (input: string | URL | Request) => {
  const url = typeof input === "string"
    ? input
    : input instanceof URL
      ? input.toString()
      : input.url;
  if (
    url === "asset://C:/Mock/Temp/theme-song.lrc"
    || url === `http://127.0.0.1:49152/preview/${"2".repeat(64)}`
  ) {
    return {
      ok: true,
      status: 200,
      text: async () => "[00:10.00]Mock lyric line 1\n[00:20.00]Mock lyric line 2",
    } as Response;
  }
  return {
    ok: false,
    status: 404,
    text: async () => "",
  } as Response;
}) as typeof fetch;


let mockEntries: MockEntry[] = initialEntries();
let mockTrashEntries: MockEntry[] = [];

function defaultPlaylistSummary(repoId = "repo-main-001"): PlaylistSummary {
  return {
    playlistId: "playlist-mock",
    repoId,
    name: "Mock Playlist",
    playerTypeId: "momobako.playlist.audio-sequence",
    playerPluginId: "momobako.preview.media",
    playerLabel: "音频顺序播放",
    fileClass: "audio",
    itemCount: 1,
    sortOrder: 0,
    createdAt: "2026-06-05T00:18:00Z",
    updatedAt: "2026-06-05T00:18:00Z",
  };
}

function defaultPlaylistDetail(repoId = "repo-main-001", playlistId = "playlist-mock"): PlaylistDetail {
  return {
    playlist: {
      ...defaultPlaylistSummary(repoId),
      playlistId,
    },
    items: [{
      playlistItemId: "playlist-item-mock",
      playlistId,
      assetId: "asset-01",
      path: "asset-01.mp3",
      filename: "asset-01.mp3",
      extension: "mp3",
      thumbnailPath: null,
      status: "ready",
      statusReason: null,
      sortOrder: 0,
      addedAt: "2026-06-05T00:18:00Z",
    }],
  };
}

function recordOpenerCall(command: "openPath" | "openUrl" | "revealItemInDir", path: string) {
  openerCalls.push({ command, path });
  if (mockOpenerFailure) {
    const failure = mockOpenerFailure;
    mockOpenerFailure = null;
    throw failure;
  }
}

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
      isVirtual: false,
      providerId: null,
      providerItemId: null,
      sourcePayload: null,
      localAbsolutePath: null,
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
  if (Array.isArray(value)) return value.map(metadataSearchText).filter(Boolean).join(" ");
  return "";
}

function metadataNumber(value: unknown) {
  if (typeof value === "number") return value;
  if (typeof value === "string" && value.trim()) {
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
  }
  return null;
}

function filterSearchHits(request: SearchRequest | undefined, hits: SearchHit[]) {
  if (!request) return hits;
  const query = request.query.trim().toLowerCase();
  const tags = request.tags?.map((tag) => tag.toLowerCase()).filter(Boolean) ?? [];
  const formats = request.formats?.map((format) => format.toLowerCase()).filter(Boolean) ?? [];
  const metadataFilters = request.metadataFilters ?? [];
  const excludeTags = request.excludeTags?.map((tag) => tag.toLowerCase()).filter(Boolean) ?? [];
  const excludeFormats = request.excludeFormats?.map((format) => format.toLowerCase()).filter(Boolean) ?? [];
  const excludeMetadataFilters = request.excludeMetadataFilters ?? [];
  const excludeQueryTerms = request.excludeQuery?.toLowerCase().split(/\s+/).filter(Boolean) ?? [];
  const excludePathPrefixes = request.excludePathPrefixes?.map((path) => path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "").toLowerCase()).filter(Boolean) ?? [];
  const excludeNumberFilters = request.excludeNumberFilters ?? [];
  const excludeDateFilters = request.excludeDateFilters ?? [];
  const numberFilters = request.numberFilters ?? [];
  const dateFilters = request.dateFilters ?? [];
  const matchMode = request.matchMode === "or" ? "or" : "and";

  const results = hits.filter((hit) => {
    if (request.repoId && hit.repoId !== request.repoId) return false;
    if (excludePathPrefixes.some((prefix) => {
      const path = hit.path.toLowerCase();
      return path === prefix || path.startsWith(`${prefix}/`);
    })) return false;
    if (excludeQueryTerms.length) {
      const haystack = [
        hit.repoName,
        hit.filename,
        hit.path,
        ...hit.tags,
        ...Object.values(hit.metadata).map(metadataSearchText),
      ].join(" ").toLowerCase();
      if (excludeQueryTerms.some((term) => haystack.includes(term))) return false;
    }
    if (excludeFormats.length && excludeFormats.includes(searchHitFormat(hit))) return false;
    if (excludeTags.length && hit.tags.some((tag) => excludeTags.some((expected) => tag.toLowerCase().includes(expected)))) return false;
    if (excludeMetadataFilters.some((filter) => {
      const actual = metadataSearchText(hit.metadata[filter.key]);
      const expected = filter.value.toLowerCase();
      return actual === expected || actual.includes(expected);
    })) return false;
    if (excludeNumberFilters.some((filter) => {
      const actual = metadataNumber(hit.metadata[filter.key]);
      return actual != null && (filter.min == null || actual >= filter.min) && (filter.max == null || actual <= filter.max);
    })) return false;
    if (excludeDateFilters.some((filter) => {
      const actualText = metadataSearchText(hit.metadata[filter.key]);
      const actualTime = Date.parse(actualText);
      return !Number.isNaN(actualTime)
        && (!filter.from || actualTime >= Date.parse(filter.from))
        && (!filter.to || actualTime <= Date.parse(filter.to));
    })) return false;
    if (numberFilters.some((filter) => {
      const actual = metadataNumber(hit.metadata[filter.key]);
      return actual == null || (filter.min != null && actual < filter.min) || (filter.max != null && actual > filter.max);
    })) return false;
    if (dateFilters.some((filter) => {
      const actualText = metadataSearchText(hit.metadata[filter.key]);
      const actualTime = Date.parse(actualText);
      return Number.isNaN(actualTime)
        || (filter.from && actualTime < Date.parse(filter.from))
        || (filter.to && actualTime > Date.parse(filter.to));
    })) return false;

    const checks: boolean[] = [];
    if (query) {
      const haystack = [
        hit.repoName,
        hit.filename,
        hit.path,
        ...hit.tags,
        ...Object.values(hit.metadata).map(metadataSearchText),
      ].join(" ").toLowerCase();
      checks.push(haystack.includes(query));
    }
    if (formats.length) checks.push(formats.includes(searchHitFormat(hit)));
    if (tags.length) checks.push(hit.tags.some((tag) => tags.some((expected) => tag.toLowerCase().includes(expected))));
    if (request.minRating != null) {
      const rating = typeof hit.metadata.rating === "number" ? hit.metadata.rating : 0;
      checks.push(rating >= request.minRating);
    }
    checks.push(...metadataFilters.map((filter) => {
      const actual = metadataSearchText(hit.metadata[filter.key]);
      const expected = filter.value.toLowerCase();
      return actual === expected || actual.includes(expected);
    }));
    return matchMode === "or" && checks.length ? checks.some(Boolean) : checks.every(Boolean);
  });
  if (request.sort?.field) {
    const direction = request.sort.direction === "desc" ? -1 : 1;
    results.sort((left, right) => {
      const field = request.sort?.field ?? "";
      const leftValue = field === "modifiedAt" ? left.metadata.fileCreatedAt : left.metadata[field.replace(/^metadata\./, "")];
      const rightValue = field === "modifiedAt" ? right.metadata.fileCreatedAt : right.metadata[field.replace(/^metadata\./, "")];
      return metadataSearchText(leftValue).localeCompare(metadataSearchText(rightValue)) * direction;
    });
  }
  return request.limit ? results.slice(0, request.limit) : results;
}

function buildSmartFolderTree(parentId: string | null = null): SmartFolderTreeNode[] {
  return mockSmartFolders
    .filter((folder) => (folder.parentId ?? null) === parentId)
    .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name))
    .map((folder) => ({
      ...folder,
      children: buildSmartFolderTree(folder.smartFolderId),
    }));
}

function deleteSmartFolderTree(smartFolderId: string) {
  const ids = new Set<string>([smartFolderId]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const folder of mockSmartFolders) {
      if (folder.parentId && ids.has(folder.parentId) && !ids.has(folder.smartFolderId)) {
        ids.add(folder.smartFolderId);
        changed = true;
      }
    }
  }
  mockSmartFolders = mockSmartFolders.filter((folder) => !ids.has(folder.smartFolderId));
}

function smartFolderSearchRequest(filter: SmartFolderFilter, repoId: string): SearchRequest {
  return {
    query: filter.query ?? "",
    repoId,
    excludeQuery: filter.excludeQuery,
    tags: filter.tags,
    formats: filter.formats,
    minRating: filter.minRating,
    metadataFilters: [
      ...(filter.metadataFilters ?? []),
      ...(filter.colors ?? []).map((value) => ({ key: "color", value })),
      ...(filter.shapes ?? []).map((value) => ({ key: "shape", value })),
    ],
    excludeTags: filter.excludeTags,
    excludeFormats: filter.excludeFormats,
    excludeMetadataFilters: filter.excludeMetadataFilters,
    excludePathPrefixes: filter.excludePathPrefixes,
    excludeNumberFilters: filter.excludeNumberFilters,
    excludeDateFilters: filter.excludeDateFilters,
    numberFilters: filter.numberFilters,
    dateFilters: filter.dateFilters,
    matchMode: filter.matchMode,
    sort: filter.sort,
    limit: filter.limit,
  };
}

function smartFolderResultEntries(filter: SmartFolderFilter, repoId: string): FileBrowserEntry[] {
  return filterSearchHits(smartFolderSearchRequest(filter, repoId), defaultSearchHits())
    .filter((hit) => !filter.pathPrefix || hit.path === filter.pathPrefix || hit.path.startsWith(`${filter.pathPrefix}/`))
    .map((hit) => ({
      path: hit.path,
      name: hit.filename,
      kind: "file",
      extension: searchHitFormat(hit),
      sizeBytes: hit.assetId === "asset-01" ? 238950400 : 15245312,
      sizeLabel: hit.assetId === "asset-01" ? "227.9 MB" : "14.5 MB",
      modifiedAt: "2026-06-05T00:18:00Z",
      assetId: hit.assetId,
      status: hit.status,
      thumbnailPath: null,
      thumbnailCustom: false,
      metadata: hit.metadata,
    }));
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

function pluginCallKey(pluginId: string, method: string) {
  return `${pluginId}:${method}`;
}

function getMockSnapshot(repoId: string) {
  return repoId === altSnapshot.repository.repoId ? altSnapshot : mockSnapshot;
}

function getMockFileBrowser(directoryPath = "", includeTree = true, specialLocation?: "trash", repoId = "repo-main-001") {
  const entries = specialLocation === "trash" ? mockTrashEntries : getMockEntriesForRepository(repoId);
  const snapshotSource = getMockSnapshot(repoId);
  const repository = mockRepositories.find((item) => item.repoId === repoId) ?? snapshotSource.repository;
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
    rootPath: repository.path,
    backendPluginId: repository.backend.pluginId,
    backendKind: repository.backend.kind,
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

function previewPluginModuleSource(pluginId: string) {
  if (pluginId === "momobako.preview.media") {
    return mediaPreviewPluginSourceForTest();
  }
  if (pluginId === "momobako.library.netease-cloud-music") {
    const sourcePath = resolve("External/Plugins/library-netease-cloud-music/src/register.js");
    return readFileSync(sourcePath, "utf-8");
  }
  if (pluginId === "momobako.tool.api-playground") {
    return [
      "export function register(ctx) {",
      "  ctx.registerToolPage({",
      "    toolPageId: 'momobako.tool.api-playground',",
      "    label: 'API Playground',",
      "    description: '调试 /external/v1 后端接口',",
      "    order: 10,",
      "    component: {",
      "      name: 'MockApiPlayground',",
      "      template: '<section class=\"mock-api-playground\">API Playground</section>',",
      "      props: { manifest: { type: Object, default: null } },",
      "    },",
      "  });",
      "}",
      "",
    ].join("\n");
  }
  if (pluginId === "user.settings-page") {
    return [
      "export function register(ctx) {",
      "  ctx.registerSettingsPage({",
      "    label: 'Settings Page',",
      "    description: 'Custom settings surface',",
      "    component: {",
      "      name: 'MockSettingsPage',",
      "      template: '<section class=\"mock-settings-page\">Settings Page</section>',",
      "      props: { manifest: { type: Object, default: null } },",
      "    },",
      "  });",
      "}",
      "",
    ].join("\n");
  }

  const definitionMap: Record<string, { extensions: string[]; thumbnail?: boolean; fileActions?: boolean }> = {
    "momobako.preview.three-model": {
      extensions: ["fbx", "obj", "glb", "gltf", "vrm", "stl", "3mf", "blend"],
    },
    "momobako.preview.text": {
      extensions: ["txt", "md", "markdown", "json", "yaml", "yml", "csv"],
      thumbnail: true,
    },
    "momobako.preview.office": {
      extensions: ["pdf", "doc", "docx", "docm", "xls", "xlsx", "xlsm", "ppt", "pptx", "pptm"],
      thumbnail: true,
    },
  };
  const definition = definitionMap[pluginId];
  if (!definition) {
    return "export function register() { return null; }\n";
  }

  return [
    "export function register(ctx) {",
    "  ctx.registerPreview({",
    `    supportedExtensions: ${JSON.stringify(definition.extensions)},`,
    "    component: {",
    "      name: 'MockPreviewComponent',",
    pluginId === "momobako.preview.media"
      ? "      template: '<section class=\"mock-preview-plugin\"><img v-if=\"previewUrl\" class=\"media-preview__image\" :src=\"previewUrl\" alt=\"\" /></section>',"
      : "      template: '<section class=\"mock-preview-plugin\"></section>',",
    "      props: { entry: { type: Object, default: null }, repoId: { type: String, default: '' } },",
    pluginId === "momobako.preview.media" ? "      data() { return { previewUrl: '' }; }," : "",
    pluginId === "momobako.preview.media"
      ? "      async mounted() { if (this.entry?.path && this.repoId) { const source = await ctx.preparePreviewFileSource({ repoId: this.repoId, path: this.entry.path }); this.previewUrl = source.sourceUrl ?? ''; } },"
      : "",
    "    },",
    definition.thumbnail ? "    generateThumbnail: async () => null," : "",
    definition.fileActions ? "    getFileActions: () => []," : "",
    "  });",
    "}",
    "",
  ].filter(Boolean).join("\n");
}

function mediaPreviewPluginSourceForTest() {
  const sourcePath = resolve("External/Plugins/media-preview/src/register.js");
  return readFileSync(sourcePath, "utf-8")
    .replace(
      new RegExp('import\\s*\\{[\\s\\S]*?\\}\\s*from\\s*"\\./mediaExtensions\\.js";\\s*'),
      [
        "const audioPreviewExtensions = ['mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'opus'];",
        "const imagePreviewExtensions = ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'avif', 'svg'];",
        "const videoPreviewExtensions = ['mp4', 'mov', 'mkv', 'webm', 'avi', 'm4v'];",
        "const isImageExtension = (extension) => imagePreviewExtensions.includes((extension ?? '').toLowerCase());",
        "const isVideoExtension = (extension) => videoPreviewExtensions.includes((extension ?? '').toLowerCase());",
      ].join("\n"),
    );
}

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class MockChannel<T> {
    onmessage: ((message: T) => void) | null = null;
  },
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: async (command: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ command, args });
    if (mockInvokeDelay?.command === command && command !== "prepare_entry_playback_source_with_progress") {
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
      const nextComment = request?.metadata?.comment ?? request?.metadata?.note ?? mockAssetDetail.metadata[1].value;
      return {
        outcome: "success",
        asset: {
          ...mockAssetDetail,
          summary: {
            ...mockAssetDetail.summary,
            version: mockAssetDetail.summary.version + 1,
          },
          metadata: mockAssetDetail.metadata.map((entry) => (
            entry.key === "note" || entry.key === "comment" ? { ...entry, value: nextComment } : entry
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
    if (command === "list_playlists") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      return (mockPlaylists ?? [defaultPlaylistSummary(repoId)]).map((playlist) => ({
        ...playlist,
        repoId,
      }));
    }
    if (command === "list_playlist_memberships") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      const memberships: Record<string, string[]> = {};
      for (const detail of Object.values(mockPlaylistDetails)) {
        if (detail.playlist.repoId !== repoId) continue;
        for (const item of detail.items) {
          memberships[item.assetId] = memberships[item.assetId] ?? [];
          memberships[item.assetId].push(detail.playlist.playlistId);
        }
      }
      return { memberships };
    }
    if (command === "get_playlist_detail") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      const playlistId = typeof args?.playlistId === "string" ? args.playlistId : "playlist-mock";
      return mockPlaylistDetails[playlistId] ?? defaultPlaylistDetail(repoId, playlistId);
    }
    if (command === "create_playlist") {
      const request = args?.request as {
        repoId?: string;
        playlistId?: string;
        name?: string;
        playerTypeId?: string;
      } | undefined;
      const repoId = request?.repoId ?? "repo-main-001";
      const playlistId = request?.playlistId ?? "playlist-created";
      const playlist: PlaylistSummary = {
        playlistId,
        repoId,
        name: request?.name ?? "新播放集",
        playerTypeId: request?.playerTypeId ?? "momobako.playlist.audio-sequence",
        playerPluginId: "momobako.preview.media",
        playerLabel: "音频顺序播放",
        fileClass: "audio",
        itemCount: 0,
        sortOrder: (mockPlaylists ?? []).length,
        createdAt: "2026-06-05T00:18:00Z",
        updatedAt: "2026-06-05T00:18:00Z",
      };
      mockPlaylists = [
        ...(mockPlaylists ?? []).filter((item) => item.playlistId !== playlistId),
        playlist,
      ];
      mockPlaylistDetails[playlistId] = {
        playlist,
        items: [],
      };
      return {
        playlists: mockPlaylists,
        playlist,
      };
    }
    if (command === "update_playlist" || command === "delete_playlist") {
      return {
        playlists: mockPlaylists ?? [],
        playlist: null,
      };
    }
    if (command === "add_playlist_items" || command === "add_playlist_items_by_paths" || command === "reorder_playlist_items" || command === "remove_playlist_item") {
      const request = args?.request as { repoId?: string; playlistId?: string } | undefined;
      return mockPlaylistDetails[request?.playlistId ?? "playlist-mock"] ?? defaultPlaylistDetail(request?.repoId ?? "repo-main-001", request?.playlistId ?? "playlist-mock");
    }
    if (command === "set_playlist_membership") {
      const request = args?.request as { assetId?: string; playlistIds?: string[] } | undefined;
      return {
        assetId: request?.assetId ?? "asset-01",
        playlistIds: request?.playlistIds ?? [],
      };
    }
    if (command === "list_smart_folders") {
      return buildSmartFolderTree();
    }
    if (command === "list_repository_actions") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      return mockRepositoryActions.filter((action) => action.repoId === repoId);
    }
    if (command === "get_repository_action") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      const actionId = typeof args?.actionId === "string" ? args.actionId : "";
      return mockRepositoryActions.find((action) => action.repoId === repoId && action.actionId === actionId) ?? null;
    }
    if (command === "set_repository_action_enabled") {
      const request = args?.request as { repoId?: string; actionId?: string; enabled?: boolean } | undefined;
      let action: RepositoryAction | null = null;
      mockRepositoryActions = mockRepositoryActions.map((item) => {
        if (item.repoId !== request?.repoId || item.actionId !== request?.actionId) return item;
        action = {
          ...item,
          enabled: Boolean(request.enabled),
          updatedAt: "2026-06-05T00:19:00Z",
        };
        return action;
      });
      return { action };
    }
    if (command === "run_repository_action") {
      const request = args?.request as { repoId?: string; actionId?: string; assetIds?: string[]; targetPaths?: string[] } | undefined;
      const run = {
        runId: `run-${request?.actionId ?? "action"}-1`,
        actionId: request?.actionId ?? "",
        repoId: request?.repoId ?? "repo-main-001",
        status: "success",
        target: {
          assetIds: request?.assetIds ?? [],
          targetPaths: request?.targetPaths ?? [],
        },
        message: "已处理目标",
        startedAt: "2026-06-05T00:19:00Z",
        finishedAt: "2026-06-05T00:19:01Z",
      };
      let action: RepositoryAction | null = null;
      mockRepositoryActions = mockRepositoryActions.map((item) => {
        if (item.repoId !== request?.repoId || item.actionId !== request?.actionId) return item;
        action = {
          ...item,
          lastRun: run,
          updatedAt: "2026-06-05T00:19:01Z",
        };
        return action;
      });
      return { action, run };
    }
    if (command === "create_smart_folder") {
      const request = args?.request as {
        repoId?: string;
        parentId?: string | null;
        name?: string;
        filter?: SmartFolderFilter;
      } | undefined;
      const now = "2026-06-05T00:18:00Z";
      const parentId = request?.parentId || null;
      const smartFolder: SmartFolder = {
        smartFolderId: `smart-${mockSmartFolders.length + 1}`,
        repoId: request?.repoId ?? "repo-main-001",
        parentId,
        name: request?.name ?? "智能文件夹",
        filter: request?.filter ?? {},
        sortOrder: mockSmartFolders.filter((item) => (item.parentId ?? null) === parentId).length,
        createdAt: now,
        updatedAt: now,
      };
      mockSmartFolders = [...mockSmartFolders, smartFolder];
      return {
        smartFolders: buildSmartFolderTree(),
        smartFolder,
      };
    }
    if (command === "update_smart_folder") {
      const request = args?.request as {
        repoId?: string;
        smartFolderId?: string;
        parentId?: string | null;
        name?: string;
        filter?: SmartFolderFilter;
      } | undefined;
      let updated: SmartFolder | null = null;
      mockSmartFolders = mockSmartFolders.map((folder) => {
        if (folder.smartFolderId !== request?.smartFolderId) return folder;
        updated = {
          ...folder,
          parentId: request.parentId || null,
          name: request.name ?? folder.name,
          filter: request.filter ?? folder.filter,
          updatedAt: "2026-06-05T00:18:00Z",
        };
        return updated;
      });
      return {
        smartFolders: buildSmartFolderTree(),
        smartFolder: updated,
      };
    }
    if (command === "delete_smart_folder") {
      const smartFolderId = typeof args?.smartFolderId === "string" ? args.smartFolderId : "";
      deleteSmartFolderTree(smartFolderId);
      return {
        smartFolders: buildSmartFolderTree(),
        smartFolder: null,
      };
    }
    if (command === "query_smart_folder") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "repo-main-001";
      const smartFolderId = typeof args?.smartFolderId === "string" ? args.smartFolderId : "";
      const smartFolder = mockSmartFolders.find((folder) => folder.smartFolderId === smartFolderId) ?? mockSmartFolders[0];
      const inheritedFilter = smartFolder?.filter ?? {};
      return {
        repoId,
        smartFolder,
        inheritedFilter,
        results: smartFolder ? smartFolderResultEntries(inheritedFilter, repoId) : [],
      };
    }
    if (command === "read_file") {
      const request = args?.request as { path?: string } | undefined;
      if (request?.path === "Music/theme-song.lrc") {
        return Array.from(new TextEncoder().encode("[00:10.00]Mock lyric line 1\n[00:20.00]Mock lyric line 2"));
      }
      if (request?.path?.endsWith(".lrc")) {
        throw new Error(`file not found: ${request.path}`);
      }
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
            : path.endsWith(".png")
              ? "image/png"
              : path.endsWith(".jpg") || path.endsWith(".jpeg")
                ? "image/jpeg"
                : path.endsWith(".webp")
                  ? "image/webp"
                  : path.endsWith(".gif")
                    ? "image/gif"
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
    if (command === "prepare_entry_playback_source") {
      const request = args?.request as { repoId?: string; path?: string } | undefined;
      const path = request?.path ?? "model.glb";
      const mediaType = path.endsWith(".glb") || path.endsWith(".vrm")
        ? "model/gltf-binary"
        : path.endsWith(".gltf")
          ? "model/gltf+json"
          : path.endsWith(".png")
            ? "image/png"
            : path.endsWith(".jpg") || path.endsWith(".jpeg")
              ? "image/jpeg"
              : path.endsWith(".webp")
                ? "image/webp"
                : path.endsWith(".gif")
                  ? "image/gif"
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
                    : "application/octet-stream";
      return {
        repoId: request?.repoId ?? "repo-main-001",
        path,
        sourceUrl: `http://127.0.0.1:49152/playback/${"1".repeat(64)}`,
        localPath: path.endsWith(".mp3") ? `C:/Mock/Temp/${path.split("/").at(-1)}` : null,
        tempFilePath: path.endsWith(".mp3") ? `C:/Mock/Temp/${path.split("/").at(-1)}` : null,
        lyricPath: path === "Music/theme-song.mp3" ? "C:/Mock/Temp/theme-song.lrc" : null,
        lyricSourceUrl: path === "Music/theme-song.mp3" ? `http://127.0.0.1:49152/preview/${"2".repeat(64)}` : null,
        wordLyricPath: null,
        wordLyricSourceUrl: null,
        mediaType,
        expiresAt: "2026-06-05T01:18:00Z",
        sizeBytes: 1024,
        modifiedAt: "2026-06-05T00:18:00Z",
      };
    }
    if (command === "prepare_entry_playback_source_with_progress") {
      const request = args?.request as { repoId?: string; path?: string } | undefined;
      const progress = args?.progress as { onmessage?: ((payload: unknown) => void) | null } | undefined;
      const path = request?.path ?? "model.glb";
      progress?.onmessage?.({
        phase: "resolve",
        repoId: request?.repoId ?? "repo-main-001",
        path,
        value: 8,
        detail: "解析媒体条目",
        indeterminate: false,
        cached: null,
        error: null,
      });
      progress?.onmessage?.({
        phase: "download",
        repoId: request?.repoId ?? "repo-main-001",
        path,
        value: 42,
        detail: "下载临时音频",
        indeterminate: true,
        cached: null,
        error: null,
      });
      if (mockInvokeDelay?.command === command) {
        await mockInvokeDelay.promise;
        mockInvokeDelay = null;
      }
      progress?.onmessage?.({
        phase: "ready",
        repoId: request?.repoId ?? "repo-main-001",
        path,
        value: 100,
        detail: "播放源已就绪",
        indeterminate: false,
        cached: path.endsWith(".mp3"),
        error: null,
      });
      const mediaType = path.endsWith(".png")
        ? "image/png"
        : path.endsWith(".jpg") || path.endsWith(".jpeg")
          ? "image/jpeg"
          : path.endsWith(".mp4") || path.endsWith(".m4v")
            ? "video/mp4"
            : path.endsWith(".webm")
              ? "video/webm"
              : path.endsWith(".mp3")
                ? "audio/mpeg"
                : "application/octet-stream";
      return {
        repoId: request?.repoId ?? "repo-main-001",
        path,
        sourceUrl: `http://127.0.0.1:49152/playback/${"1".repeat(64)}`,
        localPath: path.endsWith(".mp3") ? `C:/Mock/Temp/${path.split("/").at(-1)}` : null,
        tempFilePath: path.endsWith(".mp3") ? `C:/Mock/Temp/${path.split("/").at(-1)}` : null,
        lyricPath: path === "Music/theme-song.mp3" ? "C:/Mock/Temp/theme-song.lrc" : null,
        lyricSourceUrl: path === "Music/theme-song.mp3" ? `http://127.0.0.1:49152/preview/${"2".repeat(64)}` : null,
        wordLyricPath: null,
        wordLyricSourceUrl: null,
        mediaType,
        expiresAt: "2026-06-05T01:18:00Z",
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
        repoId?: string;
        name?: string;
        path?: string;
        backendPluginId?: string;
      } | undefined;
      const backendPluginId = request?.backendPluginId ?? "momobako.local-filesystem";
      const backendKind = backendPluginId === "momobako.webdav"
        ? "webdav"
        : backendPluginId === "momobako.cloud-drive"
          ? "cloud"
          : backendPluginId === "momobako.source.netease-cloud-music"
            ? "netease-cloud-music"
          : "filesystem";
      const backendName = backendPluginId === "momobako.webdav"
        ? "WebDAV"
        : backendPluginId === "momobako.cloud-drive"
          ? "Cloud Drive"
          : backendPluginId === "momobako.source.netease-cloud-music"
            ? "Netease Cloud Music"
          : "Local Filesystem";
      const created = {
        repoId: request?.repoId ?? "repo-created-001",
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
        localCache: backendPluginId === "momobako.source.netease-cloud-music"
          ? {
            required: true,
            path: request?.path ?? "C:/Mock/NewRepo",
            status: "ready",
          }
          : null,
      };
      mockRepositories = [
        ...mockRepositories.filter((repo) => repo.repoId !== created.repoId),
        {
          ...created,
        },
      ];
      return { repository: created };
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
    if (command === "relocate_repository") {
      const request = args?.request as { repoId?: string; path?: string } | undefined;
      if (request?.path === "C:/Mock/DifferentRepo") {
        throw new Error("selected folder belongs to a different repository");
      }
      if (request?.path === "C:/Mock/NoMetadata") {
        throw new Error("repository metadata not found in selected folder");
      }
      const repoId = request?.repoId ?? "repo-main-001";
      const path = request?.path ?? relocatedRepositoryPath;
      const existing = mockRepositories.find((repo) => repo.repoId === repoId) ?? mockSnapshot.repository;
      const repository = {
        ...existing,
        path,
        status: "ready",
        assetCount: mockSnapshot.repository.assetCount,
      };
      mockRepositories = mockRepositories.map((repo) => (
        repo.repoId === repoId ? repository : repo
      ));
      if (!mockRepositories.some((repo) => repo.repoId === repoId)) {
        mockRepositories = [repository];
      }
      return { repository };
    }
    if (command === "update_repository_backend_config") {
      const request = args?.request as { repoId?: string; backendConfig?: Record<string, unknown> } | undefined;
      const repoId = request?.repoId ?? "";
      const existing = mockRepositories.find((repo) => repo.repoId === repoId) ?? null;
      if (!existing) {
        throw new Error(`repository not found: ${repoId}`);
      }
      return { repository: existing };
    }
    if (command === "configure_netease_repository_cache") {
      const request = args?.request as { repoId?: string; path?: string } | undefined;
      const repoId = request?.repoId ?? "";
      const path = request?.path ?? "C:/Mock/NeteaseCache";
      const existing = mockRepositories.find((repo) => repo.repoId === repoId) ?? null;
      if (!existing) {
        throw new Error(`repository not found: ${repoId}`);
      }
      const repository = {
        ...existing,
        path,
        status: "ready",
        localCache: {
          required: true,
          path,
          status: "ready",
        },
      };
      mockRepositories = mockRepositories.map((repo) => (
        repo.repoId === repoId ? repository : repo
      ));
      return {
        repository,
        migration: {
          movedStateFiles: 0,
          migratedPlaybackCacheFiles: 0,
          skippedPlaybackCacheFiles: 0,
          failedPlaybackCacheFiles: 0,
        },
      };
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
    if (command === "delete_repository") {
      const repoId = typeof args?.repoId === "string" ? args.repoId : "";
      mockRepositories = mockRepositories.filter((repo) => repo.repoId !== repoId);
      return undefined;
    }
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
        sourceUrl?: string;
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
    if (command === "call_plugin") {
      const request = args?.request as { pluginId?: string; method?: string; payload?: { id?: string } } | undefined;
      const pluginId = request?.pluginId ?? "momobako.service.provider.dlsite";
      const method = request?.method ?? "provider.lookupMetadataCandidate";
      const payload = request?.payload ?? {};
      pluginCallCalls.push({ pluginId, method, payload });
      const mockResponse = pluginCallMockResponses.get(pluginCallKey(pluginId, method));
      if (mockResponse !== undefined) {
        return {
          pluginId,
          method,
          payload: typeof mockResponse === "function" ? mockResponse(payload) : mockResponse,
        };
      }
      const id = request?.payload?.id ?? "RJ123456";
      return {
        pluginId,
        method,
        payload: {
          source: pluginId.includes("asmr-one") ? "asmr-one" : "dlsite",
          confidence: "external-id",
          fields: {
            workId: id,
            rjCode: id,
            workTitle: "Fetched Rain Voice",
            circle: "Fetched Circle",
          },
        },
      };
    }
    if (command === "download_playlist_with_progress") {
      const request = args?.request as {
        playlistId?: number;
        playlistName?: string;
        tracks?: Array<{
          songId?: number;
          songName?: string | null;
        }>;
        destination?: Record<string, unknown>;
      } | undefined;
      const progress = args?.progress as { onmessage?: ((payload: unknown) => void) | null } | undefined;
      const tracks = request?.tracks ?? [];
      progress?.onmessage?.({
        phase: "start",
        playlistId: request?.playlistId ?? 0,
        playlistName: request?.playlistName ?? null,
        total: tracks.length,
        completed: 0,
        failed: 0,
      });
      tracks.forEach((track, index) => {
        const songId = track.songId ?? 0;
        progress?.onmessage?.({
          phase: "track",
          playlistId: request?.playlistId ?? 0,
          playlistName: request?.playlistName ?? null,
          total: tracks.length,
          completed: index + 1,
          failed: 0,
          currentSongId: songId,
          currentSongName: track.songName ?? `song-${songId}`,
        });
      });
      progress?.onmessage?.({
        phase: "complete",
        playlistId: request?.playlistId ?? 0,
        playlistName: request?.playlistName ?? null,
        total: tracks.length,
        completed: tracks.length,
        failed: 0,
      });
      return {
        playlistId: request?.playlistId ?? 0,
        playlistName: request?.playlistName ?? null,
        completed: tracks.map((track) => ({
          songId: track.songId ?? 0,
          paths: [
            `C:/Mock/.service-data/plugin-data/momobako-service-downloader/exports/repository-staging/${request?.destination && typeof request.destination === "object" && "repoId" in request.destination ? (request.destination as { repoId?: string }).repoId ?? "repository" : "repository"}/${track.songName ?? `song-${track.songId ?? 0}`}.mp3`,
          ],
        })),
        failed: [],
        summary: {
          total: tracks.length,
          succeeded: tracks.length,
          failed: 0,
        },
      };
    }
    if (command === "read_plugin_archive_text") {
      const request = args?.request as { pluginId?: string; path?: string } | undefined;
      const pluginId = request?.pluginId ?? "";
      return {
        pluginId,
        path: request?.path ?? "dist/register.js",
        text: previewPluginModuleSource(pluginId),
      };
    }
    if (command === "get_plugin_data_directory") {
      const pluginId = args?.pluginId ?? "momobako.preview.media";
      return {
        pluginId,
        path: `C:/MomoBako/.service-data/plugin-data/${pluginId.replace(/[^a-z0-9]+/gi, "-")}`,
      };
    }
    if (command === "get_plugin_config") {
      const pluginId = args?.pluginId ?? "momobako.preview.media";
      return {
        pluginId,
        dataDirectory: `C:/MomoBako/.service-data/plugin-data/${pluginId.replace(/[^a-z0-9]+/gi, "-")}`,
        schema: {},
        values: { ...(mockPluginConfigValues[pluginId] ?? {}) },
      };
    }
    if (command === "set_plugin_config_value") {
      const request = args?.request as { pluginId?: string; key?: string; value?: unknown } | undefined;
      const pluginId = request?.pluginId ?? "momobako.preview.media";
      const nextValues = {
        ...(mockPluginConfigValues[pluginId] ?? {}),
        ...(request?.key ? { [request.key]: request.value } : {}),
      };
      mockPluginConfigValues[pluginId] = nextValues;
      return {
        pluginId,
        dataDirectory: `C:/MomoBako/.service-data/plugin-data/${pluginId.replace(/[^a-z0-9]+/gi, "-")}`,
        schema: {},
        values: nextValues,
      };
    }
    if (command === "delete_plugin_config_value") {
      const request = args?.request as { pluginId?: string } | undefined;
      const pluginId = request?.pluginId ?? "momobako.preview.media";
      mockPluginConfigValues[pluginId] = {};
      return {
        pluginId,
        dataDirectory: `C:/MomoBako/.service-data/plugin-data/${pluginId.replace(/[^a-z0-9]+/gi, "-")}`,
        schema: {},
        values: {},
      };
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
        pluginManifest("user.sample-plugin", [], "Sample Plugin", "0.1.0", "provider-service", "metadata", "从压缩包安装的测试插件。", ["metadata"], true, "backend", "manifest-only", "user"),
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
            group: "External Asset API",
            transport: "external-http",
            method: "GET",
            path: "/external/v1/health",
            summary: "检查外部 API 服务状态。",
            requiresAuth: false,
          },
          {
            group: "Repository API",
            transport: "tauri-command",
            method: "INVOKE",
            path: "list_repositories",
            command: "list_repositories",
            summary: "列出所有仓库。",
            requestTemplate: {},
          },
          {
            group: "Playlist API",
            transport: "tauri-command",
            method: "INVOKE",
            path: "download_playlist_with_progress",
            command: "download_playlist_with_progress",
            summary: "下载歌单并通过进度通道回报逐首处理状态。",
            requestTemplate: {
              request: {
                playlistId: 9001,
                playlistName: "夜跑歌单",
                tracks: [
                  {
                    songId: 2001,
                    songName: "稻香",
                    sourcePayload: {
                      provider: "netease-cloud-music",
                      songId: 2001,
                    },
                  },
                ],
                destination: {
                  kind: "localFolder",
                  path: "C:/Downloads/Playlist",
                },
                sourcePayload: {
                  provider: "netease-cloud-music",
                  playlistId: 9001,
                },
                level: "standard",
              },
              progress: "<Channel<DownloaderPlaylistProgressEvent>>",
            },
          },
          {
            group: "Preview API",
            transport: "tauri-command",
            method: "INVOKE",
            path: "prepare_entry_playback_source_with_progress",
            command: "prepare_entry_playback_source_with_progress",
            summary: "为本地或虚拟条目准备播放源，并通过进度通道回报准备与下载阶段。",
            requestTemplate: {
              request: {
                repoId: "repo-main-001",
                path: "Music/theme-song.mp3",
              },
              progress: "<Channel<EntryPlaybackProgressEvent>>",
            },
          },
          {
            group: "Plugin API / DLsite Provider",
            transport: "plugin-call",
            method: "PLUGIN",
            path: "momobako.service.provider.dlsite:provider.lookupMetadataCandidate",
            summary: "查询 DLsite Provider 元数据候选。",
            pluginId: "momobako.service.provider.dlsite",
            pluginMethod: "provider.lookupMetadataCandidate",
            requestTemplate: {
              id: "RJ123456",
            },
          },
          {
            group: "Plugin API / Local Filesystem",
            transport: "plugin-call",
            method: "PLUGIN",
            path: "momobako.local-filesystem:filesystem.listFiles",
            summary: "递归列出本地仓库文件。",
            pluginId: "momobako.local-filesystem",
            pluginMethod: "filesystem.listFiles",
            requestTemplate: {
              repoRoot: "C:/Mock/AnimeAssets",
              config: {},
            },
          },
        ],
      };
    }
    if (command === "get_external_api_connection_status") {
      return {
        baseUrl: "http://127.0.0.1:31337/external/v1",
        token: "mock-external-token",
        version: "1",
        startedAt: "2026-06-05T00:18:00Z",
        ready: true,
        connectionFilePath: "C:/Mock/.service-data/external-api.json",
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
    recordOpenerCall("openPath", path);
  },
  openUrl: async (path: string) => {
    recordOpenerCall("openUrl", path);
  },
  revealItemInDir: async (path: string) => {
    recordOpenerCall("revealItemInDir", path);
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
  mockSmartFolders = [];
  mockRepositoryActions = [];
  mockPlugins = null;
  mockPlaylists = null;
  mockPlaylistDetails = {};
  mockPluginConfigValues = {};
  mockRepositories = [];
  mockEntries = initialEntries();
  mockTrashEntries = [];
  invokeCalls.length = 0;
  openerCalls.length = 0;
  pluginCallCalls.length = 0;
  pluginCallMockResponses.clear();
});

export function getInvokeCalls(command?: string) {
  return command ? invokeCalls.filter((call) => call.command === command) : [...invokeCalls];
}

export function seedMockRepository() {
  mockRepositories = [mockSnapshot.repository];
  mockRepositoryActions = defaultRepositoryActions();
  mockPlaylists = [defaultPlaylistSummary()];
  mockPlaylistDetails = {
    "playlist-mock": defaultPlaylistDetail(),
  };
}

export function seedLargeMockDirectory(entryCount = 1200) {
  seedMockRepository();
  mockEntries = Array.from({ length: entryCount }, (_, index): MockEntry => {
    const isDirectory = index % 5 === 0;
    const padded = String(index).padStart(4, "0");
    const name = isDirectory ? `Folder-${padded}` : `asset-${padded}.png`;
    return {
      path: name,
      name,
      kind: isDirectory ? "directory" : "file",
      extension: isDirectory ? null : "png",
      sizeBytes: isDirectory ? null : 1024 + index,
      sizeLabel: isDirectory ? null : "1 KB",
      modifiedAt: "2026-06-05T00:18:00Z",
      assetId: isDirectory ? null : `asset-large-${padded}`,
      status: isDirectory ? null : "synced",
      thumbnailPath: isDirectory ? undefined : null,
      thumbnailCustom: false,
      tags: isDirectory ? undefined : ["large"],
      metadata: isDirectory ? undefined : { color: index % 2 ? "红色" : "蓝色" },
    };
  });
}

function createMissingMockRepository() {
  return {
    ...mockSnapshot.repository,
    path: missingRepositoryPath,
    status: "missing",
    assetCount: 0,
  };
}

export function seedMissingMockRepository() {
  mockRepositories = [createMissingMockRepository()];
  mockPlaylists = [defaultPlaylistSummary()];
  mockPlaylistDetails = {
    "playlist-mock": defaultPlaylistDetail(),
  };
}

export function seedMixedMockRepositories() {
  mockRepositories = [altRepository, createMissingMockRepository()];
  mockRepositoryActions = defaultRepositoryActions();
  mockPlaylists = [defaultPlaylistSummary()];
  mockPlaylistDetails = {
    "playlist-mock": defaultPlaylistDetail(),
  };
}

export function seedMockRepositoryActions(actions: RepositoryAction[] = defaultRepositoryActions()) {
  mockRepositoryActions = actions;
}

export function seedMockPlaylists(playlists: PlaylistSummary[], details: Record<string, PlaylistDetail>) {
  mockPlaylists = playlists;
  mockPlaylistDetails = details;
}

export function seedMockPlugins(plugins: PluginManifest[]) {
  mockPlugins = plugins;
}

export function seedMockEntries(entries: MockEntry[]) {
  mockEntries = entries;
}

export function seedMockRepositories(repositories: MockRepository[]) {
  mockRepositories = repositories;
}

export function seedMockPluginConfig(pluginId: string, values: Record<string, unknown>) {
  mockPluginConfigValues[pluginId] = { ...values };
}

export function mockPluginCallResponse(pluginId: string, method: string, payload: unknown) {
  pluginCallMockResponses.set(pluginCallKey(pluginId, method), payload);
}

export function getPluginCallCalls(pluginId?: string, method?: string) {
  return pluginCallCalls.filter((call) => (
    (pluginId ? call.pluginId === pluginId : true)
    && (method ? call.method === method : true)
  ));
}

export function getRelocatedRepositoryPath() {
  return relocatedRepositoryPath;
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

export function getOpenerCalls(command?: "openPath" | "openUrl" | "revealItemInDir") {
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
