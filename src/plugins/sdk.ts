import type { Component, DefineComponent } from "vue";
import {
  computed,
  defineAsyncComponent,
  h,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import type {
  FileBrowserEntry,
  PlaylistItem,
  PlaylistPlayerContribution,
  PluginManifest,
  SearchSort,
} from "../types/repository";
import {
  callPlugin,
  ensureThumbnail,
  preparePreviewFileSource,
  readFile,
  readPluginArchiveText,
  writeBinaryFile,
} from "../services/repositoryApi";

export type PreviewPluginFileAction = {
  id: string;
  label: string;
  icon?: Component;
  disabled?: boolean;
  danger?: boolean;
  confirmLabel?: string;
  onSelect: () => Promise<void> | void;
};

export type FilePreviewPlugin = {
  pluginId: string;
  name: string;
  kind: "preview";
  supportedExtensions: string[];
  component: Component;
  generateThumbnail?: (context: {
    repoId: string;
    entry: FileBrowserEntry;
  }) => Promise<{ bytes: number[]; mediaType: string } | null>;
  getFileActions?: (context: {
    repoId: string;
    entry: FileBrowserEntry;
  }) => PreviewPluginFileAction[];
  manifest?: PluginManifest;
};

export type PlaylistPlayerRuntimeEvent =
  | { type: "state"; canPlay?: boolean; isPlaying?: boolean }
  | { type: "time"; currentTimeMs: number; durationMs?: number }
  | { type: "ended" }
  | { type: "error"; message: string };

export type PlaylistPlayerObjectFit = "contain" | "cover";

export type PlaylistPlayerRuntimeSettings = {
  imageDurationMs?: number;
  objectFit?: PlaylistPlayerObjectFit;
};

export type PlaylistPlayerRuntimeApi = {
  load: (item: PlaylistItem) => Promise<void> | void;
  play: () => Promise<void> | void;
  pause: () => Promise<void> | void;
  configure?: (settings: PlaylistPlayerRuntimeSettings) => Promise<void> | void;
  seek?: (timeMs: number) => Promise<void> | void;
  setVolume?: (value: number) => Promise<void> | void;
  dispose?: () => Promise<void> | void;
};

export type PlaylistPlayerController = {
  mountTarget: HTMLElement;
  repoId: string;
  onEvent: (event: PlaylistPlayerRuntimeEvent) => void;
};

export type RegisteredPlaylistPlayer = PlaylistPlayerContribution & {
  pluginId: string;
  pluginName: string;
  manifest?: PluginManifest;
  createRuntime: (controller: PlaylistPlayerController) => Promise<PlaylistPlayerRuntimeApi> | PlaylistPlayerRuntimeApi;
};

export type LibrarySearchShortcut = {
  id: string;
  label: string;
  metadataFilters: string;
  sort?: SearchSort;
};

export type LibraryFileSummary = {
  inline?: string;
  rows?: Array<{ label: string; value: string }>;
};

export type LibraryExtensionContext = {
  repoId: string;
  entry: FileBrowserEntry;
  entries: FileBrowserEntry[];
  saveMetadata: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
  saveCoverThumbnail?: (path: string, sourceUrl: string) => Promise<unknown>;
  previewEntry?: (entry: FileBrowserEntry) => void;
};

export type LibraryExtensionComponentProps = LibraryExtensionContext;

export type LibraryExtensionDefinition = {
  libraryKind: string;
  label: string;
  matchEntry: (entry: FileBrowserEntry) => boolean;
  searchShortcuts?: LibrarySearchShortcut[];
  fileSummary?: (entry: FileBrowserEntry) => LibraryFileSummary | null;
  metadataPanel?: Component;
  previewPanel?: Component;
};

export type RegisteredLibraryExtension = LibraryExtensionDefinition & {
  pluginId: string;
  pluginName: string;
  manifest?: PluginManifest;
};

export type PluginEventHandler<T = unknown> = (payload: T) => void | Promise<void>;

export type MediaPlaybackEvent = {
  repoId: string;
  entry: FileBrowserEntry;
  state: "metadata" | "timeupdate" | "pause" | "ended";
  currentTimeMs: number;
  durationMs: number;
  saveMetadata?: (entry: FileBrowserEntry, metadata: Record<string, unknown>) => Promise<unknown>;
};

export type PreviewPluginDefinition = {
  manifest: PluginManifest;
  supportedExtensions: string[];
  component: Component;
  generateThumbnail?: FilePreviewPlugin["generateThumbnail"];
  getFileActions?: FilePreviewPlugin["getFileActions"];
};

export type FrontendPluginContext = {
  manifest: PluginManifest;
  registerPreview: (definition: Omit<PreviewPluginDefinition, "manifest">) => FilePreviewPlugin;
  registerPlaylistPlayer: (definition: PlaylistPlayerContribution & {
    createRuntime: RegisteredPlaylistPlayer["createRuntime"];
  }) => RegisteredPlaylistPlayer;
  registerLibraryExtension: (definition: LibraryExtensionDefinition) => RegisteredLibraryExtension;
  defineLazyComponent: <T extends Component | DefineComponent>(
    loader: () => Promise<T | { default: T }>,
  ) => Component;
  loadModule: <T = unknown>(path: string) => Promise<T>;
  callPlugin: typeof callPlugin;
  preparePreviewFileSource: typeof preparePreviewFileSource;
  readFile: typeof readFile;
  ensureThumbnail: typeof ensureThumbnail;
  saveGeneratedThumbnail: (request: {
    repoId: string;
    path: string;
    imageBytes: number[];
    mediaType?: string;
  }) => Promise<Awaited<ReturnType<typeof ensureThumbnail>>>;
  writeBinaryFile: typeof writeBinaryFile;
  saveFileDialog: typeof saveDialog;
  fileSrc: typeof convertFileSrc;
  emitPluginEvent: <T = unknown>(eventName: string, payload: T) => void;
  onPluginEvent: <T = unknown>(eventName: string, handler: PluginEventHandler<T>) => () => void;
  vue: {
    h: typeof h;
    ref: typeof ref;
    computed: typeof computed;
    watch: typeof watch;
    onMounted: typeof onMounted;
    onBeforeUnmount: typeof onBeforeUnmount;
    nextTick: typeof nextTick;
  };
};

const previewPluginRegistry = new Map<string, FilePreviewPlugin>();
const playlistPlayerRegistry = new Map<string, RegisteredPlaylistPlayer>();
const libraryExtensionRegistry = new Map<string, RegisteredLibraryExtension>();
const pluginEventHandlers = new Map<string, Set<PluginEventHandler>>();
const loadedPluginModules = new Map<string, Promise<void>>();
const pluginModuleUrls = new Map<string, string>();

function normalizeExtensions(extensions: string[]) {
  return [...new Set(
    extensions
      .map((extension) => extension.trim().toLowerCase())
      .filter(Boolean),
  )];
}

export function definePreviewPlugin(definition: PreviewPluginDefinition) {
  return {
    pluginId: definition.manifest.pluginId,
    name: definition.manifest.name,
    kind: "preview" as const,
    supportedExtensions: normalizeExtensions(definition.supportedExtensions),
    component: definition.component,
    generateThumbnail: definition.generateThumbnail,
    getFileActions: definition.getFileActions,
    manifest: definition.manifest,
  };
}

export function registerPreviewPlugin(plugin: FilePreviewPlugin) {
  previewPluginRegistry.set(plugin.pluginId, plugin);
  return plugin;
}

export function registerPlaylistPlayer(player: RegisteredPlaylistPlayer) {
  playlistPlayerRegistry.set(player.playerTypeId, player);
  return player;
}

export function registerLibraryExtension(extension: RegisteredLibraryExtension) {
  libraryExtensionRegistry.set(extension.libraryKind, extension);
  return extension;
}

export function listRegisteredPreviewPlugins() {
  return [...previewPluginRegistry.values()];
}

export function listRegisteredPlaylistPlayers() {
  return [...playlistPlayerRegistry.values()].filter((player) => player.manifest?.enabled ?? true);
}

export function getRegisteredPlaylistPlayerByType(playerTypeId: string) {
  const player = playlistPlayerRegistry.get(playerTypeId);
  if (!player) return null;
  return (player.manifest?.enabled ?? true) ? player : null;
}

export function listRegisteredLibraryExtensions() {
  return [...libraryExtensionRegistry.values()].filter((extension) => extension.manifest?.enabled ?? true);
}

export function getRegisteredLibraryExtensionsForEntry(entry: FileBrowserEntry | null) {
  if (!entry) return [];
  return listRegisteredLibraryExtensions().filter((extension) => extension.matchEntry(entry));
}

export function emitPluginEvent<T = unknown>(eventName: string, payload: T) {
  for (const handler of pluginEventHandlers.get(eventName) ?? []) {
    void Promise.resolve(handler(payload));
  }
}

export function onPluginEvent<T = unknown>(eventName: string, handler: PluginEventHandler<T>) {
  const handlers = pluginEventHandlers.get(eventName) ?? new Set<PluginEventHandler>();
  handlers.add(handler as PluginEventHandler);
  pluginEventHandlers.set(eventName, handlers);
  return () => {
    handlers.delete(handler as PluginEventHandler);
    if (!handlers.size) pluginEventHandlers.delete(eventName);
  };
}

function pluginBlobUrlCacheKey(pluginId: string, path: string) {
  return `${pluginId}:${path}`;
}

async function loadPluginModule<T = unknown>(pluginId: string, path: string): Promise<T> {
  const cacheKey = pluginBlobUrlCacheKey(pluginId, path);
  let url = pluginModuleUrls.get(cacheKey);
  if (!url) {
    const response = await readPluginArchiveText({ pluginId, path });
    url = `data:text/javascript;charset=utf-8,${encodeURIComponent(response.text)}`;
    pluginModuleUrls.set(cacheKey, url);
  }
  return import(/* @vite-ignore */ url) as Promise<T>;
}

function createFrontendPluginContext(manifest: PluginManifest): FrontendPluginContext {
  return {
    manifest,
    registerPreview(definition) {
      const plugin = definePreviewPlugin({
        manifest,
        ...definition,
      });
      registerPreviewPlugin(plugin);
      return plugin;
    },
    registerPlaylistPlayer(definition) {
      const player: RegisteredPlaylistPlayer = {
        ...definition,
        pluginId: manifest.pluginId,
        pluginName: manifest.name,
        supportedExtensions: normalizeExtensions(definition.supportedExtensions),
        manifest,
      };
      registerPlaylistPlayer(player);
      return player;
    },
    registerLibraryExtension(definition) {
      const extension: RegisteredLibraryExtension = {
        ...definition,
        pluginId: manifest.pluginId,
        pluginName: manifest.name,
        manifest,
      };
      registerLibraryExtension(extension);
      return extension;
    },
    defineLazyComponent(loader) {
      return defineAsyncComponent(loader);
    },
    loadModule<T = unknown>(path: string) {
      return loadPluginModule<T>(manifest.pluginId, path);
    },
    callPlugin,
    preparePreviewFileSource,
    readFile,
    ensureThumbnail,
    saveGeneratedThumbnail(request) {
      return ensureThumbnail({
        repoId: request.repoId,
        path: request.path,
        action: "saveGenerated",
        imageBytes: request.imageBytes,
        mediaType: request.mediaType,
      });
    },
    writeBinaryFile,
    saveFileDialog: saveDialog,
    fileSrc: convertFileSrc,
    emitPluginEvent,
    onPluginEvent,
    vue: {
      h,
      ref,
      computed,
      watch,
      onMounted,
      onBeforeUnmount,
      nextTick,
    },
  };
}

async function registerFrontendPluginManifest(manifest: PluginManifest) {
  const modulePath = manifest.entry?.frontend?.module?.trim();
  if (!modulePath) return;
  const moduleExport = manifest.entry?.frontend?.export?.trim() || "register";
  const module = await loadPluginModule<Record<string, unknown>>(manifest.pluginId, modulePath);
  const register = module[moduleExport];
  if (typeof register !== "function") {
    throw new Error(`plugin register export not found: ${manifest.pluginId}:${moduleExport}`);
  }
  await Promise.resolve(register(createFrontendPluginContext(manifest)));
}

export async function syncRegisteredPreviewPluginManifests(manifests: PluginManifest[]) {
  const manifestMap = new Map(manifests.map((manifest) => [manifest.pluginId, manifest]));
  for (const [pluginId, plugin] of previewPluginRegistry) {
    const manifest = manifestMap.get(pluginId);
    if (!manifest) {
      previewPluginRegistry.delete(pluginId);
      loadedPluginModules.delete(pluginId);
      continue;
    }
    plugin.manifest = manifest;
  }
  for (const player of playlistPlayerRegistry.values()) {
    const manifest = manifestMap.get(player.pluginId);
    if (manifest) player.manifest = manifest;
  }
  for (const [libraryKind, extension] of libraryExtensionRegistry) {
    const manifest = manifestMap.get(extension.pluginId);
    if (!manifest) {
      libraryExtensionRegistry.delete(libraryKind);
      loadedPluginModules.delete(extension.pluginId);
      continue;
    }
    extension.manifest = manifest;
  }

  for (const manifest of manifests) {
    if (manifest.sdk !== "frontend" || manifest.runtime !== "vue-module") continue;
    if (!manifest.entry?.frontend?.module) continue;
    if (
      previewPluginRegistry.has(manifest.pluginId)
      || [...playlistPlayerRegistry.values()].some((player) => player.pluginId === manifest.pluginId)
      || [...libraryExtensionRegistry.values()].some((extension) => extension.pluginId === manifest.pluginId)
    ) {
      const plugin = previewPluginRegistry.get(manifest.pluginId);
      if (plugin) plugin.manifest = manifest;
      for (const player of playlistPlayerRegistry.values()) {
        if (player.pluginId === manifest.pluginId) {
          player.manifest = manifest;
        }
      }
      for (const extension of libraryExtensionRegistry.values()) {
        if (extension.pluginId === manifest.pluginId) {
          extension.manifest = manifest;
        }
      }
      continue;
    }
    if (!loadedPluginModules.has(manifest.pluginId)) {
      loadedPluginModules.set(
        manifest.pluginId,
        registerFrontendPluginManifest(manifest).catch((error) => {
          loadedPluginModules.delete(manifest.pluginId);
          throw error;
        }),
      );
    }
    await loadedPluginModules.get(manifest.pluginId);
    const plugin = previewPluginRegistry.get(manifest.pluginId);
    if (plugin) plugin.manifest = manifest;
  }
}

export function getRegisteredPreviewPluginForEntry(entry: FileBrowserEntry | null) {
  const extension = entry?.extension?.toLowerCase();
  if (!extension) return null;
  return listRegisteredPreviewPlugins()
    .filter((plugin) => plugin.manifest?.enabled ?? true)
    .find((plugin) => plugin.supportedExtensions.includes(extension)) ?? null;
}

export function clearPreviewPluginRegistry() {
  previewPluginRegistry.clear();
  playlistPlayerRegistry.clear();
  libraryExtensionRegistry.clear();
  pluginEventHandlers.clear();
  loadedPluginModules.clear();
  pluginModuleUrls.clear();
}
