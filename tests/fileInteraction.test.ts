// 验证目录切换使用静默加载，避免覆盖正在显示的任务进度。
import { computed, effectScope } from "vue";
import { describe, expect, it, vi } from "vitest";
import { useFileInteraction } from "../src/pages/workspace/files/useFileInteraction";

describe("useFileInteraction", () => {
  it("打开目录时使用静默加载参数", () => {
    const loadFileBrowserForDirectory = vi.fn();
    const setDragHoverFolderPath = vi.fn();
    const setActiveLibraryCategory = vi.fn();
    const scope = effectScope();

    const api = scope.run(() => useFileInteraction({
      activeAssetId: computed(() => null),
      activeRepoId: computed(() => "repo-main-001"),
      fileBrowser: computed(() => null),
      isFileBrowserPanel: computed(() => true),
      isTrashPanel: computed(() => false),
      loadFileBrowserForDirectory,
      saveAssetMetadata: vi.fn(),
      selectAsset: vi.fn(),
      selectRepository: vi.fn(),
      selectWorkspaceEntry: vi.fn(),
      selectWorkspaceEntries: vi.fn(),
      setActiveLibraryCategory,
      setActivePanel: vi.fn(),
      setActivePreviewPath: vi.fn(),
      setDragHoverFolderPath,
      setPreviewFilePath: vi.fn(),
    }));

    api?.openDirectory("创建的歌单");

    expect(setDragHoverFolderPath).toHaveBeenCalledWith(null);
    expect(setActiveLibraryCategory).toHaveBeenCalledWith("all");
    expect(loadFileBrowserForDirectory).toHaveBeenCalledWith("创建的歌单", { silent: true });

    scope.stop();
  });

  it("回收站目录切换保留静默加载并携带特殊位置", () => {
    const loadFileBrowserForDirectory = vi.fn();
    const setActiveLibraryCategory = vi.fn();
    const scope = effectScope();

    const api = scope.run(() => useFileInteraction({
      activeAssetId: computed(() => null),
      activeRepoId: computed(() => "repo-main-001"),
      fileBrowser: computed(() => null),
      isFileBrowserPanel: computed(() => true),
      isTrashPanel: computed(() => true),
      loadFileBrowserForDirectory,
      saveAssetMetadata: vi.fn(),
      selectAsset: vi.fn(),
      selectRepository: vi.fn(),
      selectWorkspaceEntry: vi.fn(),
      selectWorkspaceEntries: vi.fn(),
      setActiveLibraryCategory,
      setActivePanel: vi.fn(),
      setActivePreviewPath: vi.fn(),
      setDragHoverFolderPath: vi.fn(),
      setPreviewFilePath: vi.fn(),
    }));

    api?.openDirectory("");

    expect(setActiveLibraryCategory).toHaveBeenCalledWith("all");
    expect(loadFileBrowserForDirectory).toHaveBeenCalledWith("", {
      specialLocation: "trash",
      silent: true,
    });

    scope.stop();
  });
});
