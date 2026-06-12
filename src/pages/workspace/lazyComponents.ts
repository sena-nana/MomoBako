import { defineAsyncComponent } from "vue";
import type { WorkspacePanelKey } from "../../composables/useRepositoryWorkspace";

type IdlePreloadWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
};

type PreloadHandle = {
  kind: "idle" | "timeout";
  id: number;
};

export const workspaceComponentLoaders = {
  CopyTargetDialog: () => import("./CopyTargetDialog.vue"),
  FileBrowserPanel: () => import("./FileBrowserPanel.vue"),
  FilePreviewPane: () => import("./FilePreviewPane.vue"),
  HardlinkCandidateDialog: () => import("./HardlinkCandidateDialog.vue"),
  PluginManagerPanel: () => import("../../components/PluginManagerPanel.vue"),
  RepositoryActionsPanel: () => import("./RepositoryActionsPanel.vue"),
  SearchPanel: () => import("./SearchPanel.vue"),
};

export const CopyTargetDialog = defineAsyncComponent(workspaceComponentLoaders.CopyTargetDialog);
export const FileBrowserPanel = defineAsyncComponent(workspaceComponentLoaders.FileBrowserPanel);
export const FilePreviewPane = defineAsyncComponent(workspaceComponentLoaders.FilePreviewPane);
export const HardlinkCandidateDialog = defineAsyncComponent(workspaceComponentLoaders.HardlinkCandidateDialog);
export const PluginManagerPanel = defineAsyncComponent(workspaceComponentLoaders.PluginManagerPanel);
export const RepositoryActionsPanel = defineAsyncComponent(workspaceComponentLoaders.RepositoryActionsPanel);
export const SearchPanel = defineAsyncComponent(workspaceComponentLoaders.SearchPanel);

function preloadWorkspaceComponents(activePanel: WorkspacePanelKey) {
  const primaryLoaders = activePanel === "search"
    ? [workspaceComponentLoaders.SearchPanel]
    : activePanel === "extensions"
      ? [workspaceComponentLoaders.PluginManagerPanel]
      : activePanel === "actions"
        ? [workspaceComponentLoaders.RepositoryActionsPanel]
        : [workspaceComponentLoaders.FileBrowserPanel];
  const secondaryLoaders = [
    workspaceComponentLoaders.FilePreviewPane,
    workspaceComponentLoaders.SearchPanel,
    workspaceComponentLoaders.PluginManagerPanel,
    workspaceComponentLoaders.CopyTargetDialog,
    workspaceComponentLoaders.HardlinkCandidateDialog,
  ];

  for (const load of new Set([...primaryLoaders, ...secondaryLoaders])) {
    void load().catch(() => undefined);
  }
}

export function queueWorkspaceComponentPreload(
  activePanel: WorkspacePanelKey,
  existingHandle: PreloadHandle | null,
) {
  if (existingHandle) return existingHandle;

  const currentWindow = window as IdlePreloadWindow;
  if (currentWindow.requestIdleCallback) {
    return {
      kind: "idle" as const,
      id: currentWindow.requestIdleCallback(() => preloadWorkspaceComponents(activePanel), { timeout: 1200 }),
    };
  }

  return {
    kind: "timeout" as const,
    id: window.setTimeout(() => preloadWorkspaceComponents(activePanel), 250),
  };
}

export function cancelWorkspaceComponentPreload(handle: PreloadHandle | null) {
  if (!handle) return;

  const currentWindow = window as IdlePreloadWindow;
  if (handle.kind === "idle" && currentWindow.cancelIdleCallback) {
    currentWindow.cancelIdleCallback(handle.id);
  } else {
    window.clearTimeout(handle.id);
  }
}

export type WorkspacePreloadHandle = PreloadHandle;
