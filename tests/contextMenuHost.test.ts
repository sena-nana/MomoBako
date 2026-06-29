// Context-menu host motion and reopen behavior tests.
import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";
import {
  closeContextMenu,
  ContextMenuHost,
  finalizeClosedContextMenu,
  installContextMenu,
  openContextMenuAt,
  type ContextMenuItem,
  vContextMenu,
} from "../src/ui/core";
import { SB_MENU_POP_TRANSITION_MS } from "../src/composables/menuMotion";

function renderWithTemplate(template: string, setup: () => Record<string, unknown>) {
  const Wrapper = defineComponent({
    components: { ContextMenuHost },
    template: `${template}<ContextMenuHost />`,
    setup,
  });

  return render(Wrapper, {
    global: {
      directives: {
        contextMenu: vContextMenu,
      },
      stubs: {
        transition: false,
      },
    },
  });
}

async function waitForMenuLeave() {
  await vi.advanceTimersByTimeAsync(SB_MENU_POP_TRANSITION_MS + 50);
}

describe("ContextMenuHost", () => {
  beforeEach(() => {
    closeContextMenu();
    finalizeClosedContextMenu();
    installContextMenu();
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(180);
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(96);
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(1024);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(768);
  });

  afterEach(() => {
    closeContextMenu();
    finalizeClosedContextMenu();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("全局屏蔽浏览器原生右键菜单", () => {
    render(ContextMenuHost);

    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 24,
      clientY: 24,
    });
    document.body.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
  });

  it("provider 返回空数组时继续向祖先查找菜单项", async () => {
    const action = vi.fn();
    const parentMenu: ContextMenuItem[] = [
      { id: "open", label: "打开", onSelect: action },
      { id: "disabled", label: "不可用", disabled: true, onSelect: vi.fn() },
    ];

    renderWithTemplate(
      `
        <div data-testid="parent" v-context-menu="parentMenu">
          <button data-testid="target" v-context-menu="emptyMenu">目标</button>
        </div>
      `,
      () => ({
        parentMenu,
        emptyMenu: () => [],
      }),
    );

    await fireEvent.contextMenu(screen.getByTestId("target"), {
      clientX: 96,
      clientY: 128,
    });

    const menu = await screen.findByRole("menu");
    expect(menu).toHaveStyle({ left: "96px", top: "128px" });
    expect(screen.getByRole("menuitem", { name: "打开" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "不可用" })).toBeDisabled();

    await fireEvent.click(screen.getByRole("menuitem", { name: "打开" }));
    expect(action).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });

  it("菜单会 clamp 在视口内", async () => {
    renderWithTemplate(
      `<button data-testid="target" v-context-menu="items">目标</button>`,
      () => ({
        items: [{ id: "open", label: "打开", onSelect: vi.fn() }],
      }),
    );

    await fireEvent.contextMenu(screen.getByTestId("target"), {
      clientX: 1000,
      clientY: 750,
    });

    expect(await screen.findByRole("menu")).toHaveStyle({
      left: "840px",
      top: "668px",
    });
  });

  it("危险项支持二次确认", async () => {
    const action = vi.fn();
    renderWithTemplate(
      `<button data-testid="target" v-context-menu="items">目标</button>`,
      () => ({
        items: [
          {
            id: "delete",
            label: "删除",
            danger: true,
            confirmLabel: "确认删除？再点一次",
            onSelect: action,
          },
        ],
      }),
    );

    await fireEvent.contextMenu(screen.getByTestId("target"));
    await fireEvent.click(await screen.findByRole("menuitem", { name: "删除" }));

    expect(action).not.toHaveBeenCalled();
    expect(screen.getByRole("menuitem", { name: "确认删除？再点一次" })).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("menuitem", { name: "确认删除？再点一次" }));

    expect(action).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });

  it("Esc 会关闭菜单", async () => {
    vi.useFakeTimers();
    renderWithTemplate(
      `<button data-testid="target" v-context-menu="items">目标</button>`,
      () => ({
        items: [{ id: "open", label: "打开", onSelect: vi.fn() }],
      }),
    );

    await fireEvent.contextMenu(screen.getByTestId("target"));
    expect(await screen.findByRole("menu")).toBeInTheDocument();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("menuitem", { name: "打开" })).toBeInTheDocument();

    await waitForMenuLeave();
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });

  it("可以用坐标程序化打开菜单", async () => {
    render(ContextMenuHost);

    openContextMenuAt(40, 52, [{ id: "open", label: "打开", onSelect: vi.fn() }]);

    expect(await screen.findByRole("menu")).toHaveStyle({
      left: "40px",
      top: "52px",
    });
  });

  it("退场期间重新打开时应显示新的菜单内容", async () => {
    vi.useFakeTimers();
    render(ContextMenuHost, {
      global: {
        stubs: {
          transition: false,
        },
      },
    });

    openContextMenuAt(48, 56, [{ id: "old-item", label: "旧菜单", onSelect: vi.fn() }]);
    expect(await screen.findByRole("menuitem", { name: "旧菜单" })).toBeInTheDocument();

    closeContextMenu();
    openContextMenuAt(92, 88, [{ id: "new-item", label: "新菜单", onSelect: vi.fn() }]);

    expect(await screen.findByRole("menuitem", { name: "新菜单" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "旧菜单" })).not.toBeInTheDocument();

    await waitForMenuLeave();
    expect(screen.getByRole("menuitem", { name: "新菜单" })).toBeInTheDocument();
  });
});
