import { describe, expect, it, vi } from "vitest";

describe("useTheme", () => {
  it("从 localStorage 恢复主题并写入 html data-theme", async () => {
    localStorage.setItem("tauri-template.theme", "light");
    vi.resetModules();

    const { useTheme } = await import("../src/composables/useTheme");
    const { theme } = useTheme();

    expect(theme.value).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("setTheme 会同步更新 data-theme 和 localStorage", async () => {
    vi.resetModules();
    const { useTheme } = await import("../src/composables/useTheme");
    const { theme, setTheme } = useTheme();

    setTheme("dark");

    expect(theme.value).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("tauri-template.theme")).toBe("dark");
  });
});
