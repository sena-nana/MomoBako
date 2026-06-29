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
  it("loads the legacy theme key for existing users", async () => {
    localStorage.setItem("tauri-template.theme", "light");

    const { useTheme } = await loadTheme();

    expect(useTheme().theme.value).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("momobako.theme")).toBe("light");
  });

  it("prefers the MomoBako theme key over the legacy key", async () => {
    localStorage.setItem("tauri-template.theme", "light");
    localStorage.setItem("momobako.theme", "dark");

    const { useTheme } = await loadTheme();

    expect(useTheme().theme.value).toBe("dark");
  });

  it("persists updates to the MomoBako theme key", async () => {
    const { useTheme } = await loadTheme();

    useTheme().setTheme("light");
    await nextTick();

    expect(localStorage.getItem("momobako.theme")).toBe("light");
  });
});
