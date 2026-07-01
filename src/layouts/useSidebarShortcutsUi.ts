import { computed, onBeforeUnmount, shallowRef, watch, type Component, type ComputedRef } from "vue";
import type { RouteLocationNormalizedLoadedGeneric, Router } from "vue-router";
import {
  Archive,
  Bookmark,
  Clock3,
  File,
  FolderTree,
  Tag,
  Trash2,
} from "@lucide/vue";
import { getWorkspaceParentPath } from "../pages/workspace/dragBehavior";
import { scheduleIdleTask } from "../composables/workspace/scheduler";
import type { RepositoryShortcut, RepositorySnapshot } from "../types/repository";
import type { WorkspaceLibraryCategoryKey, WorkspacePanelKey } from "../composables/useRepositoryWorkspace";

type PanelKey = Exclude<WorkspacePanelKey, "files" | "search" | "smartFolder" | "actions">;
type ShortcutKey = WorkspaceLibraryCategoryKey | "trash";
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
  setActiveLibraryCategory: (category: WorkspaceLibraryCategoryKey) => void;
  setActivePanel: (panel: WorkspacePanelKey) => void;
};

export type { PanelKey, ShortcutKey, ShortcutItem };

export function useSidebarShortcutsUi(options: SidebarShortcutsUiOptions) {
  const shortcutCounts = shallowRef<Record<ShortcutKey, number>>({
    all: 0,
    uncategorized: 0,
    untagged: 0,
    recent: 0,
    trash: 0,
  });
  let cancelShortcutCountBuild: (() => void) | null = null;

  const shortcuts = computed<ShortcutItem[]>(() => {
    const counts = shortcutCounts.value;
    return [
      { id: "all", label: "全部", count: counts.all, icon: Archive },
      { id: "uncategorized", label: "未分类", count: counts.uncategorized, icon: FolderTree },
      { id: "untagged", label: "未标签", count: counts.untagged, icon: Tag },
      { id: "recent", label: "最近使用", count: counts.recent, icon: Clock3 },
      { id: "trash", label: "回收站", count: counts.trash, icon: Trash2 },
    ];
  });
  const quickAccess = computed(() => options.activeSnapshot.value?.quickAccess ?? []);

  watch(
    options.activeSnapshot,
    (snapshot) => {
      cancelShortcutCountBuild?.();
      cancelShortcutCountBuild = scheduleIdleTask(() => {
        const assets = (snapshot?.assets ?? []).filter((asset) => asset.status !== "deleted");
        let uncategorized = 0;
        let untagged = 0;
        let recent = 0;
        for (const asset of assets) {
          if (!asset.path.includes("/")) uncategorized += 1;
          if (asset.tags.length === 0) untagged += 1;
          if (asset.lastAccessedAt) recent += 1;
        }
        shortcutCounts.value = {
          all: assets.length,
          uncategorized,
          untagged,
          recent,
          trash: snapshot?.overview.trashCount ?? 0,
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
    if (id === "trash") {
      selectPanel("trash");
      return;
    }
    options.setActiveLibraryCategory(id);
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
      options.setActiveLibraryCategory("all");
      options.setActivePanel("files");
      await options.loadFileBrowserForDirectory(getWorkspaceParentPath(shortcut.targetPath));
      options.selectWorkspaceEntry(shortcut.targetPath);
      return;
    }
    if (shortcut.targetPath != null) {
      options.setActiveLibraryCategory("all");
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
