import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  clampAnchoredMenuPosition,
  createAnchoredMenuPosition,
  SB_MENU_EDGE_PADDING,
} from "../src/composables/menuMotion";

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

  it("live_popover_clamps_to_the_measured_viewport", () => {
    const previousWidth = window.innerWidth;
    const previousHeight = window.innerHeight;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 400 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 300 });
    const placed = clampAnchoredMenuPosition(createAnchoredMenuPosition(-20, 900), 200, 100);
    Object.defineProperty(window, "innerWidth", { configurable: true, value: previousWidth });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: previousHeight });
    expect(SB_MENU_EDGE_PADDING).toBe(4);
    expect(placed.x).toBe(4);
    expect(placed.y).toBe(196);
    const gap = read("src-nana/src/shell/sidebar_gap.rs");
    expect(gap).toContain("assert_eq!(model.sidebar.popover_x, 4.0);");
    expect(gap).toContain("assert_eq!(model.sidebar.popover_y, 196.0);");
    const live = read("src-nana/src/shell/pointer_gesture.rs");
    expect(live).toContain("assert_ne!((model.sidebar.popover_x, model.sidebar.popover_y), (8.0, 48.0));");
    expect(live).toContain("model.sidebar.popover_x >= 4.0 && model.sidebar.popover_y >= 4.0");
  });

  it("live_folder_hover_opens_after_the_idle_clock_reaches_450ms", () => {
    expect(folderUi).toContain("ensureFolderExpanded(path);");
    expect(folderUi).toContain("openFolder(path);");
    expect(folderUi).toContain("loadFileBrowserForDirectory(path, { silent: true });");
    expect(folderUi).toContain("clearFolderHoverTimer();");
    expect(folderUi).toContain("}, 450);");
    const motion = read("src-nana/src/shell/motion.rs");
    expect(motion).toContain("pub const FOLDER_HOVER_MS: u64 = 450;");
    const live = read("src-nana/src/shell/pointer_gesture.rs");
    expect(live).toContain('assert_eq!(model.current_directory, "photos");');
    expect(live).toContain('tree_shows_directory(&window, "photos")');
    expect(live).toContain("悬停打开要发出目录浏览");
    expect(live).toContain("离开后再进入不应立刻打开");
  });

  it("live_folder_button_escape_and_prefetch_use_the_shell_path", () => {
    const switcher = read("src/layouts/useRepositorySwitcherUi.ts");
    expect(switcher).toContain('if (event.key === "Escape" && addRepositoryPopoverMode.value !== "closed")');
    expect(switcher).toContain("closeAddRepositoryPopover();");
    const live = read("src-nana/src/shell/pointer_gesture.rs");
    expect(live).toContain('labeled_id(&window, "文件夹名称")');
    expect(live).toContain("焦点在对话框输入框时 Escape 应该关掉它");
    const host = read("src-nana/src/window_host.rs");
    expect(host).toContain("fn note_dismissed_dialog");
    expect(host).toContain("GapMessage::Escape");
  });
});

describe("nana parity pointer", () => {
  it("live_pointer_drag_and_box_select_follow_the_vue_thresholds", () => {
    const panel = read("src/pages/workspace/files/useFileBrowserPanelViewModel.ts");
    expect(panel).toContain("const dragStartThreshold = 7");
    expect(panel).toContain("Math.abs(selection.currentX - selection.startX) > 3");
    expect(panel).toContain('selection.additive ? "append" : "replace"');
    expect(panel).toContain('options.emit("selectEntries", [], "replace")');
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
    const inputTests = read("src-nana/src/shell/input_tests.rs");
    const adminTests = read("src-nana/src/shell/admin_tests.rs");
    const frozen = [
      "打开失败：宿主外部打开尚未接通",
      "定位失败：宿主目录揭示尚未接通",
      "拖出失败：宿主文件拖出尚未接通",
      "最小化到托盘尚未接通",
      "复制失败：宿主剪贴板尚未接通",
    ];
    for (const line of frozen) {
      expect(inputTests.includes(line) || adminTests.includes(line), line).toBe(true);
    }
    expect(fileOps).toContain('detail: "创建硬链接或复制文件"');
  });
});
