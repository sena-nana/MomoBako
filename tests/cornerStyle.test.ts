import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

async function loadCornerStyle() {
  vi.resetModules();
  return import("../src/ui/core");
}

afterEach(() => {
  vi.resetModules();
  localStorage.clear();
  delete document.documentElement.dataset.corners;
  document.documentElement.style.removeProperty("--app-corner-radius");
});

describe("useCornerStyle", () => {
  it("reads the MomoBako corner preferences", async () => {
    localStorage.setItem("momobako.corners", "round");
    localStorage.setItem("momobako.cornerRadius", "12");

    const { useCornerStyle } = await loadCornerStyle();

    const state = useCornerStyle();
    expect(state.cornerStyle.value).toBe("round");
    expect(state.cornerRadius.value).toBe(12);
    expect(document.documentElement.dataset.corners).toBe("round");
    expect(document.documentElement.style.getPropertyValue("--app-corner-radius")).toBe("12px");
  });

  it("keeps the stored MomoBako radius", async () => {
    localStorage.setItem("momobako.corners", "smooth");
    localStorage.setItem("momobako.cornerRadius", "6");

    const { useCornerStyle } = await loadCornerStyle();

    const state = useCornerStyle();
    expect(state.cornerStyle.value).toBe("smooth");
    expect(state.cornerRadius.value).toBe(6);
  });

  it("keeps the default corner radius at 8px", async () => {
    const { useCornerStyle } = await loadCornerStyle();

    const state = useCornerStyle();
    await nextTick();

    expect(state.cornerRadius.value).toBe(8);
    expect(localStorage.getItem("momobako.cornerRadius")).toBe("8");
    expect(document.documentElement.style.getPropertyValue("--app-corner-radius")).toBe("8px");
  });
});
