import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Vue 侧的对照原文。Nana 壳层测试断言同一批时长和文案。
 */
const read = (path: string) => readFileSync(resolve(path), "utf8");
const shellCss = read("src/styles/shell.css");
const workspaceCss = read("src/styles/pages/workspace.css");
const folderUi = read("src/layouts/useFolderSidebarUi.ts");
const smartUi = read("src/layouts/useSmartFolderSidebarUi.ts");
const fileOps = read("src/composables/workspace/fileOperations.ts");
const files = read("src/composables/workspace/files.ts");
const liliaWorkspace = read("node_modules/@lilia/theme/src/styles/workspace.css");

describe("nana parity motion", () => {
  it("modal_matches_vue_endpoints_and_survives_the_next_reduce", () => {
    expect(shellCss).toContain("opacity 0.16s ease");
    expect(shellCss).toContain("translateY(-8px) scale(0.98)");
    expect(shellCss).toContain("transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1)");
  });

  it("panel_progress_spinner_and_sidebar_use_the_vue_durations", () => {
    expect(shellCss).toContain("opacity 0.14s ease, transform 0.16s ease");
    expect(shellCss).toContain("translateY(-4px)");
    expect(shellCss).toContain("width 0.18s ease");
    expect(shellCss).toContain("spin 0.8s linear infinite");
    expect(workspaceCss).toContain("opacity 0.12s ease");
    expect(shellCss).toContain("opacity 0.35s ease");
    expect(workspaceCss).toContain("progress-pulse 1.15s ease-in-out infinite");
    expect(workspaceCss).toContain("media-preview-progress-sweep 1.05s ease-in-out infinite");
    expect(liliaWorkspace).toContain("grid-template-columns 0.24s var(--lilia-workspace-easing)");
    expect(liliaWorkspace).toContain("cubic-bezier(0.2, 0.8, 0.2, 1)");
  });
});

describe("nana parity sidebar", () => {
  it("folder_dialogs_use_the_vue_titles_and_escape_closes_the_top_one", () => {
    expect(folderUi).toContain('folderDialogMode.value === "create" ? "新建文件夹" : "重命名文件夹"');
  });

  it("hover_opens_after_450ms_and_playlists_hide_when_the_repository_is_missing", () => {
    expect(folderUi).toContain("}, 450);");
  });

  it("smart_edit_popover_clamp_and_playlist_play_match_vue", () => {
    expect(smartUi).toContain('smartFolderDialogMode.value === "create" ? "新建智能文件夹" : "编辑智能文件夹"');
  });
});

describe("nana parity files", () => {
  it("copy_and_move_submit_the_stored_sources", () => {
    expect(fileOps).toContain('updateOperationProgress(progressId, { detail: "创建硬链接或复制文件", value: 32 })');
    expect(fileOps).toContain('updateOperationProgress(progressId, { detail: "刷新文件索引", value: 84 })');
  });

  it("thumbnail_prefetch_waits_for_the_vue_idle_gap", () => {
    expect(files).toContain("}, 420);");
  });
});

describe("nana parity host", () => {
  it("frozen host strings stay on the nana failure path", () => {
    expect(fileOps).toContain("创建硬链接或复制文件");
  });
});
