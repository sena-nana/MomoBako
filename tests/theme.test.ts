import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

async function loadTheme() {
  vi.resetModules();
  return import("../src/ui/core");
}

afterEach(() => {
  vi.resetModules();
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("useTheme", () => {
  it("reads the MomoBako theme key", async () => {
    localStorage.setItem("momobako.theme", "light");

    const { useTheme } = await loadTheme();

    expect(useTheme().theme.value).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("falls back to the LiliaUI default theme", async () => {
    const { useTheme } = await loadTheme();

    expect(useTheme().theme.value).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("persists updates to the MomoBako theme key", async () => {
    const { useTheme } = await loadTheme();

    useTheme().setTheme("light");
    await nextTick();

    expect(localStorage.getItem("momobako.theme")).toBe("light");
  });
});
