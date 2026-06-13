import { onUnmounted, watch, type ComputedRef } from "vue";
import type { WorkspacePanelKey } from "../../composables/useRepositoryWorkspace";
import {
  cancelWorkspaceComponentPreload,
  queueWorkspaceComponentPreload,
  type WorkspacePreloadHandle,
} from "./lazyComponents";

type WorkspaceComponentPreloadOptions = {
  activePanel: ComputedRef<WorkspacePanelKey>;
  hasRepository: ComputedRef<boolean>;
};

export function useWorkspaceComponentPreload(options: WorkspaceComponentPreloadOptions) {
  let preloadHandle: WorkspacePreloadHandle | null = null;
  let hasQueuedWorkspacePreload = false;

  function queueWorkspacePreload() {
    if (hasQueuedWorkspacePreload) return;
    hasQueuedWorkspacePreload = true;
    preloadHandle = queueWorkspaceComponentPreload(options.activePanel.value, preloadHandle);
  }

  function cancelWorkspacePreload() {
    cancelWorkspaceComponentPreload(preloadHandle);
    preloadHandle = null;
  }

  watch(options.hasRepository, (ready) => {
    if (ready) {
      queueWorkspacePreload();
    }
  }, { immediate: true });

  onUnmounted(cancelWorkspacePreload);
}
