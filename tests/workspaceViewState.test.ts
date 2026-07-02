/**
 * 验证预览页状态在异步预览链路中保持稳定。
 */
import { computed, nextTick, ref } from "vue";
import { describe, expect, it } from "vitest";
import { useWorkspaceViewState } from "../src/pages/workspace/useWorkspaceViewState";
import type {
  FileBrowserEntry,
  FileBrowserSnapshot,
  PlaylistDetail,
  RepositorySnapshot,
  SearchHit,
  SmartFolderResultSnapshot,
} from "../src/types/repository";

function createFileEntry(path: string, extension = "docx"): FileBrowserEntry {
  return {
    path,
    name: path.split("/").pop() ?? path,
    kind: "file",
    extension,
    sizeBytes: 4096,
    sizeLabel: "4 KB",
    modifiedAt: "2026-07-02T10:00:00Z",
    assetId: `asset:${path}`,
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
    localAbsolutePath: `C:/Mock/Repo/${path}`,
  };
}

function createWorkspaceViewStateHarness() {
  const entry = createFileEntry("Docs/demo.docx");
  const entryMap = computed<ReadonlyMap<string, FileBrowserEntry>>(() => new Map([[entry.path, entry]]));
  const selectedFilePath = ref<string | null>(entry.path);
  const activePreviewPath = ref<string | null>(null);
  const selectedEntries = computed<FileBrowserEntry[]>(() => (
    selectedFilePath.value === entry.path ? [entry] : []
  ));

  return {
    entry,
    selectedFilePath,
    activePreviewPath,
    state: useWorkspaceViewState({
      activeAssetDetail: computed(() => null),
      activeLibraryCategory: computed(() => "all"),
      activeLibraryCategoryLabel: computed(() => "全部"),
      activePanel: computed(() => "files"),
      activePlaylistDetail: computed<PlaylistDetail | null>(() => null),
      activePreviewPath: computed(() => activePreviewPath.value),
      activeRepositoryStatus: computed(() => "ready"),
      activeSnapshot: computed<RepositorySnapshot | null>(() => null),
      directoryEntries: computed(() => []),
      fileBrowser: computed<FileBrowserSnapshot | null>(() => null),
      fileBrowserEntryMap: entryMap,
      fileEntries: computed(() => [entry]),
      hasMultipleSelection: computed(() => selectedEntries.value.length > 1),
      hasSplitFileGroups: computed(() => false),
      playlistPreviewEntryMap: computed<ReadonlyMap<string, FileBrowserEntry>>(() => new Map()),
      searchResults: computed<SearchHit[]>(() => []),
      selectedEntries,
      selectedEntry: computed(() => selectedEntries.value[0] ?? null),
      selectedFilePath: computed(() => selectedFilePath.value),
      smartFolderResult: computed<SmartFolderResultSnapshot | null>(() => null),
      isLibraryCategoryVirtualView: computed(() => false),
      isLoadingFileBrowser: computed(() => false),
      isLoadingSmartFolder: computed(() => false),
      libraryCategorySummary: computed(() => ""),
    }),
  };
}

describe("workspace view state", () => {
  it("office 预览打开后遇到临时选中项抖动时保持预览页", async () => {
    const harness = createWorkspaceViewStateHarness();
    harness.activePreviewPath.value = harness.entry.path;
    await nextTick();

    expect(harness.state.previewFileEntry.value?.path).toBe(harness.entry.path);

    harness.selectedFilePath.value = null;
    await nextTick();
    expect(harness.state.previewFileEntry.value?.path).toBe(harness.entry.path);

    harness.selectedFilePath.value = "Docs/other.docx";
    await nextTick();
    expect(harness.state.previewFileEntry.value?.path).toBe(harness.entry.path);

    harness.activePreviewPath.value = null;
    await nextTick();
    expect(harness.state.previewFileEntry.value).toBeNull();
  });
});
