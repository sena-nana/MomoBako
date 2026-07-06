import { defineAsyncComponent } from "vue";
import type { WorkspacePanelKey } from "../../composables/useRepositoryWorkspace";
import WorkspacePlaylistPage from "./playlists/WorkspacePlaylistPage.vue";

type IdlePreloadWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
};

type PreloadHandle = {
  kind: "idle" | "timeout";
  id: number;
};
type WorkspacePreloadHandle = {
  primary: PreloadHandle | null;
  secondary: PreloadHandle | null;
};

export const workspaceComponentLoaders = {
  CopyTargetDialog: () => import("./CopyTargetDialog.vue"),
  ExtensionsPanel: () => import("./ExtensionsPanel.vue"),
  FileBrowserPanel: () => import("./files/FileBrowserPanel.vue"),
  FilePreviewPane: () => import("./preview/FilePreviewPane.vue"),
  HardlinkCandidateDialog: () => import("./HardlinkCandidateDialog.vue"),
  RepositoryActionsPanel: () => import("./repository/RepositoryActionsPanel.vue"),
  SearchPanel: () => import("./SearchPanel.vue"),
  WorkspaceLogsPanel: () => import("./WorkspaceLogsPanel.vue"),
  WorkspaceFilterBar: () => import("./search/WorkspaceFilterBar.vue"),
  WorkspaceFilesSurface: () => import("./WorkspaceFilesSurface.vue"),
  WorkspacePlaylistPage: () => import("./playlists/WorkspacePlaylistPage.vue"),
};

export const CopyTargetDialog = defineAsyncComponent(workspaceComponentLoaders.CopyTargetDialog);
export const ExtensionsPanel = defineAsyncComponent(workspaceComponentLoaders.ExtensionsPanel);
export const FileBrowserPanel = defineAsyncComponent(workspaceComponentLoaders.FileBrowserPanel);
export const FilePreviewPane = defineAsyncComponent(workspaceComponentLoaders.FilePreviewPane);
export const HardlinkCandidateDialog = defineAsyncComponent(workspaceComponentLoaders.HardlinkCandidateDialog);
export const RepositoryActionsPanel = defineAsyncComponent(workspaceComponentLoaders.RepositoryActionsPanel);
export const SearchPanel = defineAsyncComponent(workspaceComponentLoaders.SearchPanel);
export const WorkspaceLogsPanel = defineAsyncComponent(workspaceComponentLoaders.WorkspaceLogsPanel);
export const WorkspaceFilterBar = defineAsyncComponent(workspaceComponentLoaders.WorkspaceFilterBar);
export const WorkspaceFilesSurface = defineAsyncComponent(workspaceComponentLoaders.WorkspaceFilesSurface);
export { WorkspacePlaylistPage };

function currentPanelLoaders(activePanel: WorkspacePanelKey) {
  return activePanel === "search"
    ? [workspaceComponentLoaders.SearchPanel]
    : activePanel === "logs"
      ? [workspaceComponentLoaders.WorkspaceLogsPanel]
    : activePanel === "extensions"
      ? [workspaceComponentLoaders.ExtensionsPanel]
      : activePanel === "actions"
        ? [workspaceComponentLoaders.RepositoryActionsPanel]
        : activePanel === "playlist"
          ? [workspaceComponentLoaders.WorkspacePlaylistPage]
          : [workspaceComponentLoaders.WorkspaceFilesSurface, workspaceComponentLoaders.FileBrowserPanel];
}

function secondaryWorkspaceLoaders(activePanel: WorkspacePanelKey) {
  return [
    ...currentPanelLoaders(activePanel),
    workspaceComponentLoaders.FilePreviewPane,
    workspaceComponentLoaders.WorkspaceFilterBar,
    workspaceComponentLoaders.SearchPanel,
    workspaceComponentLoaders.WorkspaceLogsPanel,
    workspaceComponentLoaders.ExtensionsPanel,
    workspaceComponentLoaders.CopyTargetDialog,
    workspaceComponentLoaders.HardlinkCandidateDialog,
    workspaceComponentLoaders.WorkspacePlaylistPage,
  ];
}

function preloadWorkspaceComponents(loaders: Array<() => Promise<unknown>>) {
  for (const load of new Set(loaders)) {
    void load().catch(() => undefined);
  }
}

function schedulePreload(callback: () => void, timeout: number, fallbackDelay: number): PreloadHandle {
  const currentWindow = window as IdlePreloadWindow;
  if (currentWindow.requestIdleCallback) {
    return {
      kind: "idle",
      id: currentWindow.requestIdleCallback(callback, { timeout }),
    };
  }

  return {
    kind: "timeout",
    id: window.setTimeout(callback, fallbackDelay),
  };
}

export function queueWorkspaceComponentPreload(
  activePanel: WorkspacePanelKey,
  existingHandle: WorkspacePreloadHandle | null,
) {
  if (existingHandle) return existingHandle;
  preloadWorkspaceComponents([
    ...currentPanelLoaders(activePanel),
    workspaceComponentLoaders.WorkspacePlaylistPage,
  ]);

  return {
    primary: schedulePreload(() => preloadWorkspaceComponents(currentPanelLoaders(activePanel)), 800, 120),
    secondary: schedulePreload(() => preloadWorkspaceComponents(secondaryWorkspaceLoaders(activePanel)), 2400, 900),
  };
}

function cancelPreloadHandle(handle: PreloadHandle | null) {
  if (!handle) return;

  const currentWindow = window as IdlePreloadWindow;
  if (handle.kind === "idle" && currentWindow.cancelIdleCallback) {
    currentWindow.cancelIdleCallback(handle.id);
  } else {
    window.clearTimeout(handle.id);
  }
}

export function cancelWorkspaceComponentPreload(handle: WorkspacePreloadHandle | null) {
  if (!handle) return;
  cancelPreloadHandle(handle.primary);
  cancelPreloadHandle(handle.secondary);
}

export type { WorkspacePreloadHandle };
