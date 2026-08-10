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
const loadedPluginModules = new Map<string, { packageHash: string; promise: Promise<void> }>();
const pluginModuleUrls = new Map<string, string>();
const pluginRegistrationDisposers = new Map<string, Set<() => void | Promise<void>>>();
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

function frontendPackageHash(manifest: PluginManifest) {
  return manifest.packageHash ?? `legacy:${manifest.version}`;
}

function pluginBlobUrlCacheKey(manifest: PluginManifest, path: string) {
  return `${manifest.pluginId}:${frontendPackageHash(manifest)}:${path}`;
}

async function loadPluginModule<T = unknown>(manifest: PluginManifest, path: string): Promise<T> {
  const cacheKey = pluginBlobUrlCacheKey(manifest, path);
  let url = pluginModuleUrls.get(cacheKey);
  if (!url) {
    const response = await readPluginArchiveText({ pluginId: manifest.pluginId, path });
    url = typeof navigator !== "undefined" && navigator.userAgent.toLowerCase().includes("jsdom")
      ? `data:text/javascript;charset=utf-8,${encodeURIComponent(response.text)}`
      : URL.createObjectURL(new Blob([response.text], { type: "text/javascript;charset=utf-8" }));
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
      return loadPluginModule<T>(manifest, path);
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
    onPluginEvent<T = unknown>(eventName: string, handler: PluginEventHandler<T>) {
      const dispose = onPluginEvent(eventName, handler);
      const disposers = pluginRegistrationDisposers.get(manifest.pluginId) ?? new Set();
      disposers.add(dispose);
      pluginRegistrationDisposers.set(manifest.pluginId, disposers);
      return () => {
        dispose();
        disposers.delete(dispose);
      };
    },
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
  const module = await loadPluginModule<Record<string, unknown>>(manifest, modulePath);
  const register = module[moduleExport];
  if (typeof register !== "function") {
    throw new Error(`plugin register export not found: ${manifest.pluginId}:${moduleExport}`);
  }
  const registration = await Promise.resolve(register(createFrontendPluginContext(manifest, modulePath)));
  const dispose = typeof registration === "function"
    ? registration as () => void | Promise<void>
    : typeof registration === "object" && registration !== null && "dispose" in registration
      && typeof registration.dispose === "function"
      ? () => Promise.resolve(registration.dispose.call(registration)).then(() => undefined)
      : null;
  if (dispose) {
    const disposers = pluginRegistrationDisposers.get(manifest.pluginId) ?? new Set();
    disposers.add(dispose);
    pluginRegistrationDisposers.set(manifest.pluginId, disposers);
  }
}

function updateFrontendPluginManifest(manifest: PluginManifest) {
  const preview = previewPluginRegistry.get(manifest.pluginId);
  if (preview) preview.manifest = manifest;
  for (const player of playlistPlayerRegistry.values()) {
    if (player.pluginId === manifest.pluginId) player.manifest = manifest;
  }
  for (const extension of libraryExtensionRegistry.values()) {
    if (extension.pluginId === manifest.pluginId) extension.manifest = manifest;
  }
  for (const page of toolPageRegistry.values()) {
    if (page.pluginId === manifest.pluginId) page.manifest = manifest;
  }
  const settingsPage = settingsPageRegistry.get(manifest.pluginId);
  if (settingsPage) settingsPage.manifest = manifest;
  for (const provider of entryActionProviderRegistry.values()) {
    if (provider.pluginId === manifest.pluginId) provider.manifest = manifest;
  }
}

async function unloadFrontendPlugin(pluginId: string) {
  for (const dispose of pluginRegistrationDisposers.get(pluginId) ?? []) {
    try {
      await dispose();
    } catch (error) {
      console.error(`[frontend-plugin] dispose failed: ${pluginId}`, error);
    }
  }
  pluginRegistrationDisposers.delete(pluginId);
  previewPluginRegistry.delete(pluginId);
  for (const [key, value] of playlistPlayerRegistry) {
    if (value.pluginId === pluginId) playlistPlayerRegistry.delete(key);
  }
  for (const [key, value] of libraryExtensionRegistry) {
    if (value.pluginId === pluginId) libraryExtensionRegistry.delete(key);
  }
  for (const [key, value] of toolPageRegistry) {
    if (value.pluginId === pluginId) toolPageRegistry.delete(key);
  }
  settingsPageRegistry.delete(pluginId);
  for (const [key, value] of entryActionProviderRegistry) {
    if (value.pluginId === pluginId) entryActionProviderRegistry.delete(key);
  }
  loadedPluginModules.delete(pluginId);
  for (const [key, url] of pluginModuleUrls) {
    if (!key.startsWith(`${pluginId}:`)) continue;
    if (url.startsWith("blob:")) URL.revokeObjectURL(url);
    pluginModuleUrls.delete(key);
  }
}

export async function syncRegisteredPreviewPluginManifests(manifests: PluginManifest[]) {
  const frontendManifests = new Map(
    manifests
      .filter((manifest) => (
        manifest.sdk === "frontend"
        && manifest.runtime === "vue-module"
        && Boolean(manifest.entry?.frontend?.module)
        && manifest.enabled !== false
        && !["disabled", "unavailable", "error"].includes(manifest.status ?? "ready")
      ))
      .map((manifest) => [manifest.pluginId, manifest]),
  );

  for (const [pluginId, loaded] of [...loadedPluginModules]) {
    const manifest = frontendManifests.get(pluginId);
    if (!manifest || loaded.packageHash !== frontendPackageHash(manifest)) {
      await unloadFrontendPlugin(pluginId);
    }
  }

  for (const manifest of frontendManifests.values()) {
    if (!loadedPluginModules.has(manifest.pluginId)) {
      const packageHash = frontendPackageHash(manifest);
      const promise = registerFrontendPluginManifest(manifest).catch(async (error) => {
        await unloadFrontendPlugin(manifest.pluginId);
        throw error;
      });
      loadedPluginModules.set(manifest.pluginId, { packageHash, promise });
    }
    await loadedPluginModules.get(manifest.pluginId)?.promise;
    updateFrontendPluginManifest(manifest);
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
  for (const pluginId of loadedPluginModules.keys()) void unloadFrontendPlugin(pluginId);
  for (const url of pluginModuleUrls.values()) {
    if (url.startsWith("blob:")) URL.revokeObjectURL(url);
  }
  previewPluginRegistry.clear();
  playlistPlayerRegistry.clear();
  libraryExtensionRegistry.clear();
  toolPageRegistry.clear();
  settingsPageRegistry.clear();
  entryActionProviderRegistry.clear();
  pluginEventHandlers.clear();
  pluginRegistrationDisposers.clear();
  loadedPluginModules.clear();
  pluginModuleUrls.clear();
  bumpFrontendPluginRegistry();
}

export const syncRegisteredFrontendPluginManifests = syncRegisteredPreviewPluginManifests;
