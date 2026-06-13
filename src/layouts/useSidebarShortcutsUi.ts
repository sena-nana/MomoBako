import { computed, onBeforeUnmount, shallowRef, watch, type Component, type ComputedRef } from "vue";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import {
  Archive,
  Bookmark,
  File,
  FolderTree,
  Tag,
  Trash2,
} from "lucide-vue-next";
import { getWorkspaceParentPath } from "../pages/workspace/dragBehavior";
import { scheduleIdleTask } from "../composables/workspace/scheduler";
import type { RepositoryShortcut, RepositorySnapshot } from "../types/repository";
import type { WorkspacePanelKey } from "../composables/useRepositoryWorkspace";

type PanelKey = Exclude<WorkspacePanelKey, "files" | "search" | "smartFolder" | "actions">;
type ShortcutKey = "all" | "processing" | "untagged" | "deleted";
type ShortcutItem = {
  id: ShortcutKey;
  label: string;
  count: number;
  icon: Component;
};

type SidebarShortcutsUiOptions = {
  activeSnapshot: ComputedRef<RepositorySnapshot | null>;
  isActiveRepositoryMissing: ComputedRef<boolean>;
  loadFileBrowserForDirectory: (directoryPath?: string) => Promise<unknown>;
  route: RouteLocationNormalizedLoadedGeneric;
  router: Router;
  selectSmartFolder: (smartFolderId: string) => Promise<unknown>;
  selectWorkspaceEntry: (path: string) => void;
  setActivePanel: (panel: WorkspacePanelKey) => void;
};

export type { PanelKey, ShortcutKey, ShortcutItem };

export function useSidebarShortcutsUi(options: SidebarShortcutsUiOptions) {
  const shortcutCounts = shallowRef<Record<ShortcutKey, number>>({
    all: 0,
    processing: 0,
    untagged: 0,
    deleted: 0,
  });
  let cancelShortcutCountBuild: (() => void) | null = null;

  const shortcuts = computed<ShortcutItem[]>(() => {
    const counts = shortcutCounts.value;
    return [
      { id: "all", label: "全部", count: counts.all, icon: Archive },
      { id: "processing", label: "处理中", count: counts.processing, icon: FolderTree },
      { id: "untagged", label: "未标签", count: counts.untagged, icon: Tag },
      { id: "deleted", label: "已删除", count: counts.deleted, icon: Trash2 },
    ];
  });
  const quickAccess = computed(() => options.activeSnapshot.value?.quickAccess ?? []);

  watch(
    options.activeSnapshot,
    (snapshot) => {
      cancelShortcutCountBuild?.();
      cancelShortcutCountBuild = scheduleIdleTask(() => {
        const assets = snapshot?.assets ?? [];
        let processing = 0;
        let untagged = 0;
        for (const asset of assets) {
          if (asset.status === "processing") processing += 1;
          if (asset.tags.length === 0) untagged += 1;
        }
        shortcutCounts.value = {
          all: assets.length,
          processing,
          untagged,
          deleted: 0,
        };
      }, 200);
    },
    { immediate: true },
  );

  function selectPanel(next: PanelKey) {
    options.setActivePanel(next);
    if (options.route.path === "/settings") {
      void options.router.push("/");
    }
  }

  function selectShortcut(id: ShortcutKey) {
    if (options.isActiveRepositoryMissing.value) return;
    if (id === "deleted") {
      selectPanel("deleted");
      return;
    }
    options.setActivePanel("files");
    if (options.route.path === "/settings") {
      void options.router.push("/");
    }
  }

  function shortcutIcon(shortcut: RepositoryShortcut) {
    if (shortcut.targetKind === "smartFolder") return Bookmark;
    if (shortcut.targetKind === "file") return File;
    return FolderTree;
  }

  async function openQuickAccess(shortcut: RepositoryShortcut) {
    if (options.isActiveRepositoryMissing.value) return;
    if (options.route.path === "/settings") {
      void options.router.push("/");
    }
    if (shortcut.targetKind === "smartFolder" && shortcut.targetId) {
      await options.selectSmartFolder(shortcut.targetId);
      return;
    }
    if (shortcut.targetKind === "file" && shortcut.targetPath) {
      options.setActivePanel("files");
      await options.loadFileBrowserForDirectory(getWorkspaceParentPath(shortcut.targetPath));
      options.selectWorkspaceEntry(shortcut.targetPath);
      return;
    }
    if (shortcut.targetPath != null) {
      options.setActivePanel("files");
      void options.loadFileBrowserForDirectory(shortcut.targetPath);
    }
  }

  async function openSmartFolder(smartFolderId: string) {
    if (options.route.path === "/settings") {
      await options.router.push("/");
    }
    await options.selectSmartFolder(smartFolderId);
  }

  onBeforeUnmount(() => {
    cancelShortcutCountBuild?.();
  });

  return {
    openQuickAccess,
    openSmartFolder,
    quickAccess,
    selectPanel,
    selectShortcut,
    shortcutIcon,
    shortcuts,
  };
}
