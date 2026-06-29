import type { ComputedRef } from "vue";
import {
  Clipboard,
  Eye,
  FileImage,
  Files,
  FolderOpen,
  ImageOff,
  ImagePlus,
  PencilLine,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "@lucide/vue";
import type { ContextMenuItem } from "../../ui/core";
import type { FileBrowserEntry, RepositorySummary } from "../../types/repository";
import type { EntryActionDialogRequest, EntryActionDialogResultMap } from "../../plugins/sdk";
import { getPluginEntryActions, getPreviewPluginFileActions } from "../../plugins/previewPlugins";

type WorkspaceContextMenuOptions = {
  activeRepoId: ComputedRef<string | null>;
  activeRepository?: ComputedRef<RepositorySummary | null>;
  entryMap: ComputedRef<ReadonlyMap<string, FileBrowserEntry>>;
  hasMultipleSelection: ComputedRef<boolean>;
  isMutatingFiles: ComputedRef<boolean>;
  isSmartFolderPanel: ComputedRef<boolean>;
  isTrashPanel: ComputedRef<boolean>;
  selectedFilePathSet: ComputedRef<ReadonlySet<string>>;
  selectedFilePaths: ComputedRef<string[]>;
  chooseCustomThumbnail: (entry: FileBrowserEntry) => void | Promise<void>;
  clearCustomThumbnail: (entry: FileBrowserEntry) => void | Promise<void>;
  deleteContextSelection: (entry: FileBrowserEntry, contextSelectionPaths: string[]) => void | Promise<void>;
  openEntryActionDialog: <TKind extends keyof EntryActionDialogResultMap>(
    request: Extract<EntryActionDialogRequest, { kind: TKind }>,
  ) => Promise<EntryActionDialogResultMap[TKind]>;
  playlistMenuItems?: (entry: FileBrowserEntry) => ContextMenuItem[];
  refreshRepositoryWorkspace: () => Promise<void>;
  openCopyTargetDialog: (entry: FileBrowserEntry) => void | Promise<void>;
  openDirectory: (path: string) => void | Promise<void>;
  openWorkspaceEntry: (path: string) => void | Promise<void>;
  pasteCustomThumbnail: (entry: FileBrowserEntry) => void | Promise<void>;
  previewEntry: (entry: FileBrowserEntry) => void;
  refreshEntryThumbnail: (entry: FileBrowserEntry) => void | Promise<void>;
  restoreContextSelection: (entry: FileBrowserEntry, contextSelectionPaths: string[]) => void | Promise<void>;
  revealWorkspaceEntry: (path: string) => void | Promise<void>;
  selectWorkspaceEntries: (
    paths: string[],
    options?: { primaryPath?: string | null; anchorPath?: string | null },
  ) => void;
  startRenameEntry: (entry: FileBrowserEntry) => void | Promise<void>;
};

export function useWorkspaceContextMenu(options: WorkspaceContextMenuOptions) {
  function fileEntryContextMenu(entry: FileBrowserEntry): ContextMenuItem[] {
    if (!options.selectedFilePathSet.value.has(entry.path)) {
      options.selectWorkspaceEntries([entry.path], { primaryPath: entry.path, anchorPath: entry.path });
    }
    const contextSelectionPaths = options.selectedFilePathSet.value.has(entry.path)
      ? options.selectedFilePaths.value
      : [entry.path];
    const contextEntries = contextSelectionPaths
      .map((path) => options.entryMap.value.get(path))
      .filter((item): item is FileBrowserEntry => Boolean(item));
    if (options.isSmartFolderPanel.value) {
      return [
        {
          id: "preview",
          label: "预览",
          icon: Eye,
          disabled: entry.kind !== "file",
          onSelect: () => {
            if (entry.kind === "file") {
              options.previewEntry(entry);
            }
          },
        },
        {
          id: "open",
          label: "打开",
          icon: Eye,
          disabled: entry.kind !== "file",
          onSelect: () => options.openWorkspaceEntry(entry.path),
        },
        {
          id: "reveal",
          label: "定位",
          icon: FolderOpen,
          onSelect: () => options.revealWorkspaceEntry(entry.path),
        },
      ];
    }

    const pluginActions = options.activeRepoId.value && entry.kind === "file" && !options.isTrashPanel.value
      ? getPreviewPluginFileActions(options.activeRepoId.value, entry).map<ContextMenuItem>((action) => ({
        id: action.id,
        label: action.label,
        icon: action.icon,
        disabled: action.disabled || options.hasMultipleSelection.value,
        danger: action.danger,
        confirmLabel: action.confirmLabel,
        onSelect: action.onSelect,
      }))
      : [];
    const playlistMenuItems = !options.isSmartFolderPanel.value
      && !options.isTrashPanel.value
      && !options.hasMultipleSelection.value
      ? options.playlistMenuItems?.(entry) ?? []
      : [];
    const pluginEntryActions = options.activeRepoId.value && !options.isTrashPanel.value
      ? getPluginEntryActions({
        repoId: options.activeRepoId.value,
        repository: options.activeRepository?.value ?? null,
        entry,
        entries: contextEntries.length ? contextEntries : [entry],
        refreshRepo: options.refreshRepositoryWorkspace,
        openDialog: options.openEntryActionDialog,
      }).map<ContextMenuItem>((action) => ({
        id: action.id,
        label: action.label,
        icon: action.icon,
        disabled: action.disabled,
        danger: action.danger,
        confirmLabel: action.confirmLabel,
        onSelect: action.onSelect,
      }))
      : [];

    return [
      ...(options.isTrashPanel.value ? [{
        id: "restore",
        label: "还原",
        icon: RotateCcw,
        disabled: options.isMutatingFiles.value,
        onSelect: () => options.restoreContextSelection(entry, contextSelectionPaths),
      }] : []),
      {
        id: "preview",
        label: "预览",
        icon: Eye,
        disabled: entry.kind !== "file" || options.isTrashPanel.value || options.hasMultipleSelection.value,
        onSelect: () => {
          if (entry.kind === "file") {
            options.previewEntry(entry);
          }
        },
      },
      {
        id: "open",
        label: entry.kind === "directory" ? "进入" : "打开",
        icon: Eye,
        disabled: options.isTrashPanel.value || options.hasMultipleSelection.value,
        onSelect: () => {
          if (entry.kind === "directory") {
            options.openDirectory(entry.path);
            return;
          }
          return options.openWorkspaceEntry(entry.path);
        },
      },
      {
        id: "reveal",
        label: "定位",
        icon: FolderOpen,
        disabled: options.isTrashPanel.value,
        onSelect: () => options.revealWorkspaceEntry(entry.path),
      },
      {
        id: "copy-target",
        label: "复制到…",
        icon: Files,
        disabled: options.isTrashPanel.value || options.isMutatingFiles.value,
        onSelect: () => options.openCopyTargetDialog(entry),
      },
      ...(playlistMenuItems.length ? [{
        id: "playlist-membership",
        label: entry.kind === "directory" ? "整个加入播放列表" : "加入播放列表",
        children: playlistMenuItems,
      } satisfies ContextMenuItem] : []),
      ...pluginEntryActions,
      ...pluginActions,
      {
        id: "thumbnail",
        label: "缩略图",
        icon: FileImage,
        disabled: options.isTrashPanel.value,
        children: [
          {
            id: "thumbnail-custom-file",
            label: "自定义缩略图（选择文件）",
            icon: ImagePlus,
            onSelect: () => options.chooseCustomThumbnail(entry),
          },
          {
            id: "thumbnail-custom-clipboard",
            label: "新增自定义缩略图（从剪贴板）",
            icon: Clipboard,
            onSelect: () => options.pasteCustomThumbnail(entry),
          },
          {
            id: "thumbnail-clear-custom",
            label: "取消自定义缩略图",
            icon: ImageOff,
            disabled: !entry.thumbnailCustom,
            onSelect: () => options.clearCustomThumbnail(entry),
          },
          {
            id: "thumbnail-refresh",
            label: "刷新缩略图",
            icon: RefreshCw,
            onSelect: () => options.refreshEntryThumbnail(entry),
          },
        ],
      },
      {
        id: "rename",
        label: "重命名",
        icon: PencilLine,
        disabled: options.isTrashPanel.value || options.hasMultipleSelection.value,
        onSelect: () => options.startRenameEntry(entry),
      },
      {
        id: "delete",
        label: options.isTrashPanel.value ? "彻底删除" : "删除",
        icon: Trash2,
        danger: true,
        disabled: options.isMutatingFiles.value,
        confirmLabel: options.isTrashPanel.value ? "确认彻底删除？再点一次" : undefined,
        onSelect: () => options.deleteContextSelection(entry, contextSelectionPaths),
      },
    ];
  }

  return {
    fileEntryContextMenu,
  };
}
