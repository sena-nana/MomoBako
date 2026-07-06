import type { Component, DefineComponent } from "vue";
import {
  computed,
  defineAsyncComponent,
  h,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import type {
  FileBrowserEntry,
  EntryPlaybackProgressEvent,
  PlaylistItem,
  PlaylistPlayerContribution,
  PluginConfigSnapshot,
  PluginManifest,
  PluginSettingsPageContribution,
  RepositorySummary,
  SearchSort,
  ToolPageContribution,
} from "../types/repository";
import {
  callPlugin,
  deletePluginConfigValue,
  downloadPlaylistWithProgress,
  ensureThumbnail,
  getApiDesignSnapshot,
  getExternalApiConnectionStatus,
  getPluginConfig,
  getPluginDataDirectory,
  prepareEntryPlaybackSource,
  prepareEntryPlaybackSourceWithProgress,
  preparePluginDataFilePreviewSource,
  prepareRepositoryCacheFilePreviewSource,
  preparePreviewFileSource,
  readFile,
  readPluginArchiveText,
  setPluginConfigValue,
  writeBinaryFile,
} from "../services/repositoryApi";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "../composables/workspace/tasks";
import { emitSystemLog } from "../services/systemLog";

export type PreviewPluginFileAction = {
  id: string;
  label: string;
  icon?: Component;
  disabled?: boolean;
  danger?: boolean;
  confirmLabel?: string;
  onSelect: () => Promise<void> | void;
};

export type EntryActionDialogRequest =
  | {
      kind: "directory";
      title?: string;
      defaultPath?: string | null;
    }
  | {
      kind: "repository";
      title?: string;
      requireReady?: boolean;
      requireWritable?: boolean;
      backendPluginIds?: string[];
      backendKinds?: string[];
    };

export type EntryActionDialogResultMap = {
  directory: string | null;
  repository: RepositorySummary | null;
};

export type EntryAction = {
  id: string;
  label: string;
  icon?: Component;
  disabled?: boolean;
  danger?: boolean;
  confirmLabel?: string;
  onSelect: () => Promise<void> | void;
};

export type EntryActionContext = {
  repoId: string;
  repository?: RepositorySummary | null;
  entry: FileBrowserEntry;
  entries: FileBrowserEntry[];
  refreshRepo: () => Promise<void>;
  openDialog: <TKind extends keyof EntryActionDialogResultMap>(
    request: Extract<EntryActionDialogRequest, { kind: TKind }>,
  ) => Promise<EntryActionDialogResultMap[TKind]>;
};

export type EntryActionProviderDefinition = {
  matchEntry?: (entry: FileBrowserEntry) => boolean;
  getEntryActions: (context: EntryActionContext) => EntryAction[];
};

export type RegisteredEntryActionProvider = EntryActionProviderDefinition & {
  pluginId: string;
  pluginName: string;
  manifest?: PluginManifest;
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
  | {
      type: "state";
      canPlay?: boolean;
      isPlaying?: boolean;
      loading?: boolean;
      progress?: EntryPlaybackProgressEvent;
    }
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

export type ToolPageContext = {
  manifest: PluginManifest;
  activeRepoId: string | null;
  activeRepository: RepositorySummary | null;
  currentDirectoryPath: string;
  isRepositoryWritable: boolean;
  isTrashPanel: boolean;
  isVirtualView: boolean;
};

export type ToolPageComponentProps = ToolPageContext;

export type RegisteredToolPage = ToolPageContribution & {
  pluginId: string;
  pluginName: string;
  manifest?: PluginManifest;
  component: Component;
};

export type PluginSettingsPageContext = {
  manifest: PluginManifest;
};

export type PluginSettingsPageComponentProps = PluginSettingsPageContext;

export type RegisteredPluginSettingsPage = PluginSettingsPageContribution & {
  pluginId: string;
  pluginName: string;
  manifest?: PluginManifest;
  component: Component;
};

export type PluginEventHandler<T = unknown> = (payload: T) => void | Promise<void>;

export type PluginLoggerOptions = {
  category?: string;
  action: string;
  context?: Record<string, unknown>;
  repoId?: string | null;
  location?: {
    modulePath?: string | null;
    file?: string | null;
    line?: number | null;
  } | null;
};

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
  registerToolPage: (definition: ToolPageContribution & {
    component: Component;
  }) => RegisteredToolPage;
  registerSettingsPage: (definition: PluginSettingsPageContribution & {
    component: Component;
  }) => RegisteredPluginSettingsPage;
  registerEntryActionProvider: (
    definition: EntryActionProviderDefinition,
  ) => RegisteredEntryActionProvider;
  defineLazyComponent: <T extends Component | DefineComponent>(
    loader: () => Promise<T | { default: T }>,
  ) => Component;
  loadModule: <T = unknown>(path: string) => Promise<T>;
  getApiDesignSnapshot: typeof getApiDesignSnapshot;
  getExternalApiConnectionStatus: typeof getExternalApiConnectionStatus;
  getPluginDataDirectory: () => ReturnType<typeof getPluginDataDirectory>;
  getPluginConfig: () => Promise<PluginConfigSnapshot>;
  setPluginConfigValue: (key: string, value: unknown) => Promise<PluginConfigSnapshot>;
  deletePluginConfigValue: (key: string) => Promise<PluginConfigSnapshot>;
  invokeCommand: typeof invoke;
  callPlugin: typeof callPlugin;
  downloadPlaylistWithProgress: typeof downloadPlaylistWithProgress;
  preparePluginDataFilePreviewSource: typeof preparePluginDataFilePreviewSource;
  prepareRepositoryCacheFilePreviewSource: typeof prepareRepositoryCacheFilePreviewSource;
  prepareEntryPlaybackSource: typeof prepareEntryPlaybackSource;
  prepareEntryPlaybackSourceWithProgress: typeof prepareEntryPlaybackSourceWithProgress;
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
  openDialog: typeof openDialog;
  saveFileDialog: typeof saveDialog;
  fileSrc: typeof convertFileSrc;
  startOperationProgress: typeof startOperationProgress;
  updateOperationProgress: typeof updateOperationProgress;
  finishOperationProgress: typeof finishOperationProgress;
  cancelOperationProgress: typeof cancelOperationProgress;
  logger: {
    debug: (message: string, options: PluginLoggerOptions) => Promise<void>;
    info: (message: string, options: PluginLoggerOptions) => Promise<void>;
    warn: (message: string, options: PluginLoggerOptions) => Promise<void>;
    error: (message: string, options: PluginLoggerOptions) => Promise<void>;
  };
  emitPluginEvent: <T = unknown>(eventName: string, payload: T) => void;
  onPluginEvent: <T = unknown>(eventName: string, handler: PluginEventHandler<T>) => () => void;
  vue: {
    h: typeof h;
    ref: typeof ref;
    shallowRef: typeof shallowRef;
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
const toolPageRegistry = new Map<string, RegisteredToolPage>();
const settingsPageRegistry = new Map<string, RegisteredPluginSettingsPage>();
const entryActionProviderRegistry = new Map<string, RegisteredEntryActionProvider>();
const pluginEventHandlers = new Map<string, Set<PluginEventHandler>>();
const loadedPluginModules = new Map<string, Promise<void>>();
const pluginModuleUrls = new Map<string, string>();
export const frontendPluginRegistryVersion = ref(0);

function bumpFrontendPluginRegistry() {
  frontendPluginRegistryVersion.value += 1;
}

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
  bumpFrontendPluginRegistry();
  return plugin;
}

function createPluginLogger(manifest: PluginManifest, modulePath: string) {
  async function write(
    level: "debug" | "info" | "warn" | "error",
    message: string,
    options: PluginLoggerOptions,
  ) {
    await emitSystemLog(level, {
      category: options.category ?? "plugin.frontend",
      action: options.action,
      message,
      context: options.context,
      repoId: options.repoId,
      pluginId: manifest.pluginId,
      sourceKind: "frontend-plugin",
      sourceLabel: manifest.name,
      location: {
        modulePath: options.location?.modulePath ?? modulePath,
        file: options.location?.file ?? modulePath,
        line: options.location?.line ?? null,
      },
      stackOffset: 4,
    });
  }

  return {
    debug: (message: string, options: PluginLoggerOptions) => write("debug", message, options),
    info: (message: string, options: PluginLoggerOptions) => write("info", message, options),
    warn: (message: string, options: PluginLoggerOptions) => write("warn", message, options),
    error: (message: string, options: PluginLoggerOptions) => write("error", message, options),
  };
}

export function registerPlaylistPlayer(player: RegisteredPlaylistPlayer) {
  playlistPlayerRegistry.set(player.playerTypeId, player);
  bumpFrontendPluginRegistry();
  return player;
}

export function registerLibraryExtension(extension: RegisteredLibraryExtension) {
  libraryExtensionRegistry.set(extension.libraryKind, extension);
  bumpFrontendPluginRegistry();
  return extension;
}

export function registerToolPage(page: RegisteredToolPage) {
  toolPageRegistry.set(page.toolPageId, page);
  bumpFrontendPluginRegistry();
  return page;
}

export function registerPluginSettingsPage(page: RegisteredPluginSettingsPage) {
  settingsPageRegistry.set(page.pluginId, page);
  bumpFrontendPluginRegistry();
  return page;
}

export function registerEntryActionProvider(provider: RegisteredEntryActionProvider) {
  entryActionProviderRegistry.set(`${provider.pluginId}:${entryActionProviderRegistry.size}`, provider);
  bumpFrontendPluginRegistry();
  return provider;
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

export function listRegisteredToolPages() {
  return [...toolPageRegistry.values()]
    .filter((page) => page.manifest?.enabled ?? true)
    .sort((left, right) => (
      (left.order ?? 100) - (right.order ?? 100)
      || left.label.localeCompare(right.label)
      || left.toolPageId.localeCompare(right.toolPageId)
    ));
}

export function getRegisteredToolPage(pageId: string | null | undefined) {
  if (!pageId) return null;
  const page = toolPageRegistry.get(pageId);
  if (!page) return null;
  return (page.manifest?.enabled ?? true) ? page : null;
}

export function listRegisteredPluginSettingsPages() {
  return [...settingsPageRegistry.values()]
    .filter((page) => page.manifest?.enabled ?? true)
    .sort((left, right) => (
      (left.order ?? 100) - (right.order ?? 100)
      || (left.label ?? left.pluginName).localeCompare(right.label ?? right.pluginName)
      || left.pluginId.localeCompare(right.pluginId)
    ));
}

export function getRegisteredPluginSettingsPage(pluginId: string | null | undefined) {
  if (!pluginId) return null;
  const page = settingsPageRegistry.get(pluginId);
  if (!page) return null;
  return (page.manifest?.enabled ?? true) ? page : null;
}

export function listRegisteredEntryActionProviders() {
  return [...entryActionProviderRegistry.values()]
    .filter((provider) => provider.manifest?.enabled ?? true);
}

export function getRegisteredEntryActions(context: EntryActionContext) {
  return listRegisteredEntryActionProviders()
    .filter((provider) => provider.matchEntry?.(context.entry) ?? true)
    .flatMap((provider) => provider.getEntryActions(context));
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

function createFrontendPluginContext(manifest: PluginManifest, modulePath: string): FrontendPluginContext {
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
    registerToolPage(definition) {
      const page: RegisteredToolPage = {
        ...definition,
        pluginId: manifest.pluginId,
        pluginName: manifest.name,
        manifest,
      };
      registerToolPage(page);
      return page;
    },
    registerSettingsPage(definition) {
      const page: RegisteredPluginSettingsPage = {
        ...definition,
        label: definition.label ?? manifest.contributes?.settings?.settingsPage?.label ?? "设置",
        description: definition.description ?? manifest.contributes?.settings?.settingsPage?.description,
        order: definition.order ?? manifest.contributes?.settings?.settingsPage?.order,
        pluginId: manifest.pluginId,
        pluginName: manifest.name,
        manifest,
      };
      registerPluginSettingsPage(page);
      return page;
    },
    registerEntryActionProvider(definition) {
      const provider: RegisteredEntryActionProvider = {
        ...definition,
        pluginId: manifest.pluginId,
        pluginName: manifest.name,
        manifest,
      };
      registerEntryActionProvider(provider);
      return provider;
    },
    defineLazyComponent(loader) {
      return defineAsyncComponent(loader);
    },
    loadModule<T = unknown>(path: string) {
      return loadPluginModule<T>(manifest.pluginId, path);
    },
    getApiDesignSnapshot,
    getExternalApiConnectionStatus,
    getPluginDataDirectory() {
      return getPluginDataDirectory(manifest.pluginId);
    },
    getPluginConfig() {
      return getPluginConfig(manifest.pluginId);
    },
    setPluginConfigValue(key, value) {
      return setPluginConfigValue({ pluginId: manifest.pluginId, key, value });
    },
    deletePluginConfigValue(key) {
      return deletePluginConfigValue({ pluginId: manifest.pluginId, key });
    },
    invokeCommand: invoke,
    callPlugin,
    downloadPlaylistWithProgress,
    preparePluginDataFilePreviewSource,
    prepareRepositoryCacheFilePreviewSource,
    prepareEntryPlaybackSource,
    prepareEntryPlaybackSourceWithProgress,
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
    openDialog,
    saveFileDialog: saveDialog,
    fileSrc: convertFileSrc,
    startOperationProgress,
    updateOperationProgress,
    finishOperationProgress,
    cancelOperationProgress,
    logger: createPluginLogger(manifest, modulePath),
    emitPluginEvent,
    onPluginEvent,
    vue: {
      h,
      ref,
      shallowRef,
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
  await Promise.resolve(register(createFrontendPluginContext(manifest, modulePath)));
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
  for (const [playerTypeId, player] of playlistPlayerRegistry) {
    const manifest = manifestMap.get(player.pluginId);
    if (!manifest) {
      playlistPlayerRegistry.delete(playerTypeId);
      loadedPluginModules.delete(player.pluginId);
      continue;
    }
    player.manifest = manifest;
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
  for (const [toolPageId, page] of toolPageRegistry) {
    const manifest = manifestMap.get(page.pluginId);
    if (!manifest) {
      toolPageRegistry.delete(toolPageId);
      loadedPluginModules.delete(page.pluginId);
      continue;
    }
    page.manifest = manifest;
  }
  for (const [pluginId, page] of settingsPageRegistry) {
    const manifest = manifestMap.get(pluginId);
    if (!manifest) {
      settingsPageRegistry.delete(pluginId);
      loadedPluginModules.delete(pluginId);
      continue;
    }
    page.manifest = manifest;
  }
  for (const [providerId, provider] of entryActionProviderRegistry) {
    const manifest = manifestMap.get(provider.pluginId);
    if (!manifest) {
      entryActionProviderRegistry.delete(providerId);
      loadedPluginModules.delete(provider.pluginId);
      continue;
    }
    provider.manifest = manifest;
  }

  for (const manifest of manifests) {
    if (manifest.sdk !== "frontend" || manifest.runtime !== "vue-module") continue;
    if (!manifest.entry?.frontend?.module) continue;
    if (
      previewPluginRegistry.has(manifest.pluginId)
      || [...playlistPlayerRegistry.values()].some((player) => player.pluginId === manifest.pluginId)
      || [...libraryExtensionRegistry.values()].some((extension) => extension.pluginId === manifest.pluginId)
      || [...toolPageRegistry.values()].some((page) => page.pluginId === manifest.pluginId)
      || settingsPageRegistry.has(manifest.pluginId)
      || [...entryActionProviderRegistry.values()].some((provider) => provider.pluginId === manifest.pluginId)
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
      for (const page of toolPageRegistry.values()) {
        if (page.pluginId === manifest.pluginId) {
          page.manifest = manifest;
        }
      }
      const settingsPage = settingsPageRegistry.get(manifest.pluginId);
      if (settingsPage) settingsPage.manifest = manifest;
      for (const provider of entryActionProviderRegistry.values()) {
        if (provider.pluginId === manifest.pluginId) {
          provider.manifest = manifest;
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
  bumpFrontendPluginRegistry();
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
  toolPageRegistry.clear();
  settingsPageRegistry.clear();
  entryActionProviderRegistry.clear();
  pluginEventHandlers.clear();
  loadedPluginModules.clear();
  pluginModuleUrls.clear();
  bumpFrontendPluginRegistry();
}

export const syncRegisteredFrontendPluginManifests = syncRegisteredPreviewPluginManifests;
