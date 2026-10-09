import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  clampAnchoredMenuPosition,
  createAnchoredMenuPosition,
  SB_MENU_EDGE_PADDING,
} from "../src/composables/menuMotion";

/**
 * Vue 界面与 Nana 原生壳层共用的时长、阈值和文案对照。
 *
 * 每一项先确认 Vue 侧的参照值还在，再确认 Nana 侧同名用途的命名常量取同一个值。
 * 只对照命名常量，不读 Rust 测试里的断言原文：行为由 Rust 单测负责，这里只防两边的数各改各的。
 */
const read = (path: string) => readFileSync(resolve(path), "utf8");
const shellCss = read("src/styles/shell.css");
const workspaceCss = read("src/styles/pages/workspace.css");
const folderUi = read("src/layouts/useFolderSidebarUi.ts");
const smartUi = read("src/layouts/useSmartFolderSidebarUi.ts");
const files = read("src/composables/workspace/files.ts");
const liliaWorkspace = read("node_modules/@lilia/theme/src/styles/workspace.css");
const motion = read("src-nana/src/shell/motion.rs");

/** 取 Rust 源码里 `const NAME: 类型 = 值;` 的值。找不到时返回 undefined，让断言写清是哪一个。 */
const rustConst = (source: string, name: string) =>
  new RegExp(String.raw`const ${name}: [A-Za-z0-9]+ = (-?[0-9.]+);`).exec(source)?.[1];

/** 取 Rust 源码里 `const NAME: &str = "文字";` 的文字。 */
const rustStr = (source: string, name: string) =>
  new RegExp(String.raw`const ${name}: &str = "([^"]*)";`).exec(source)?.[1];

describe("nana parity motion", () => {
  it("modal_uses_the_vue_timings_and_endpoints", () => {
    expect(shellCss).toContain("opacity 0.16s ease");
    expect(shellCss).toContain("transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1)");
    expect(shellCss).toContain("translateY(-8px) scale(0.98)");
    expect(rustConst(motion, "MODAL_OVERLAY_MS")).toBe("160");
    expect(rustConst(motion, "MODAL_CARD_MS")).toBe("180");
    expect(rustConst(motion, "MODAL_CARD_SHIFT")).toBe("-8.0");
    expect(rustConst(motion, "MODAL_CARD_SCALE")).toBe("0.98");
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
    expect(rustConst(motion, "PANEL_OPACITY_MS")).toBe("140");
    expect(rustConst(motion, "PANEL_RISE_MS")).toBe("160");
    expect(rustConst(motion, "PANEL_SHIFT")).toBe("-4.0");
    expect(rustConst(motion, "PROGRESS_WIDTH_MS")).toBe("180");
    expect(rustConst(motion, "SPINNER_MS")).toBe("800");
    expect(rustConst(motion, "SIDEBAR_TOOL_FADE_MS")).toBe("120");
    expect(rustConst(motion, "FOOTER_FADE_MS")).toBe("350");
    expect(rustConst(motion, "PULSE_MS")).toBe("1150");
    expect(rustConst(motion, "SWEEP_MS")).toBe("1050");
    expect(rustConst(motion, "SIDEBAR_COLLAPSE_MS")).toBe("240");
  });
});

describe("nana parity sidebar", () => {
  it("folder_and_smart_folder_dialogs_use_the_vue_titles", () => {
    expect(folderUi).toContain('folderDialogMode.value === "create" ? "新建文件夹" : "重命名文件夹"');
    expect(smartUi).toContain('smartFolderDialogMode.value === "create" ? "新建智能文件夹" : "编辑智能文件夹"');
    const gap = read("src-nana/src/shell/sidebar_gap.rs");
    expect(rustStr(gap, "FOLDER_CREATE_TITLE")).toBe("新建文件夹");
    expect(rustStr(gap, "FOLDER_RENAME_TITLE")).toBe("重命名文件夹");
    expect(rustStr(gap, "SMART_EDIT_TITLE")).toBe("编辑智能文件夹");
    expect(gap).toContain('"新建智能文件夹"');
  });

  it("folder_hover_opens_after_450ms", () => {
    expect(folderUi).toContain("}, 450);");
    expect(rustConst(motion, "FOLDER_HOVER_MS")).toBe("450");
  });

  it("popover_clamps_to_the_viewport_with_the_vue_edge_padding", () => {
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
    expect(rustConst(read("src-nana/src/shell/sidebar_gap.rs"), "POPOVER_PADDING")).toBe("4.0");
  });
});

describe("nana parity pointer", () => {
  it("entry_drag_and_box_select_follow_the_vue_thresholds", () => {
    const panel = read("src/pages/workspace/files/useFileBrowserPanelViewModel.ts");
    expect(panel).toContain("const dragStartThreshold = 7");
    expect(panel).toContain("Math.abs(selection.currentX - selection.startX) > 3");
    const gesture = read("src-nana/src/shell/pointer_gesture.rs");
    expect(rustConst(gesture, "ENTRY_DRAG_PX")).toBe("7.0");
    expect(rustConst(gesture, "BOX_DRAG_PX")).toBe("3.0");
  });
});

describe("nana parity files", () => {
  it("thumbnail_prefetch_waits_for_the_vue_idle_gap", () => {
    expect(files).toContain("}, 420);");
    expect(rustConst(motion, "PREFETCH_IDLE_MS")).toBe("420");
  });
});
