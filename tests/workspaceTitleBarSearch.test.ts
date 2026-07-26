import { fireEvent, render, screen } from "@testing-library/vue";
import { ref } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import WorkspaceTitleBarSearch from "../src/components/WorkspaceTitleBarSearch.vue";

const mocks = vi.hoisted(() => ({
  push: vi.fn(),
  runSearch: vi.fn(),
  setActivePanel: vi.fn(),
  toggleFilterBar: vi.fn(),
}));

vi.mock("vue-router", () => ({
  useRoute: () => ({ path: "/settings" }),
  useRouter: () => ({ push: mocks.push }),
}));

vi.mock("../src/composables/useRepositoryWorkspace", () => ({
  useWorkspaceNavigation: () => ({
    setActivePanel: mocks.setActivePanel,
  }),
  useWorkspaceSearch: () => ({
    activeFilterCount: ref(0),
    hasActiveFilters: ref(false),
    isFilterBarOpen: ref(false),
    searchQuery: ref(""),
    runSearch: mocks.runSearch,
    toggleFilterBar: mocks.toggleFilterBar,
  }),
}));

describe("WorkspaceTitleBarSearch", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("快速输入时立即打开搜索面板，并仅搜索最后一次查询", async () => {
    vi.useFakeTimers();
    render(WorkspaceTitleBarSearch);
    const input = screen.getByRole("searchbox", { name: "全局搜索" });

    await fireEvent.update(input, "c");
    await vi.advanceTimersByTimeAsync(100);
    await fireEvent.update(input, "co");
    await vi.advanceTimersByTimeAsync(100);
    await fireEvent.update(input, "cover");

    expect(mocks.setActivePanel).toHaveBeenCalledTimes(3);
    expect(mocks.setActivePanel).toHaveBeenLastCalledWith("search");
    expect(mocks.push).toHaveBeenCalledTimes(3);
    expect(mocks.push).toHaveBeenLastCalledWith("/");
    expect(mocks.runSearch).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(249);
    expect(mocks.runSearch).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(mocks.runSearch).toHaveBeenCalledTimes(1);
    expect(mocks.runSearch).toHaveBeenCalledWith({ query: "cover" });
  });

  it("卸载时取消尚未执行的搜索", async () => {
    vi.useFakeTimers();
    const { unmount } = render(WorkspaceTitleBarSearch);

    await fireEvent.update(screen.getByRole("searchbox", { name: "全局搜索" }), "pending");
    unmount();
    await vi.advanceTimersByTimeAsync(250);

    expect(mocks.runSearch).not.toHaveBeenCalled();
  });
});
