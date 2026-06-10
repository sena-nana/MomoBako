import { afterEach, describe, expect, it, vi } from "vitest";
import {
  getWorkspaceParentPath,
  internalWorkspaceDragDistance,
  normalizeWorkspaceMovePaths,
  resolveWorkspaceDropTarget,
  shouldDelegateToExternalDrag,
} from "../src/pages/workspace/dragBehavior";

describe("dragBehavior", () => {
  const elementFromPoint = vi.fn<(x: number, y: number) => Element | null>();

  afterEach(() => {
    document.body.innerHTML = "";
    elementFromPoint.mockReset();
    vi.restoreAllMocks();
  });

  Object.defineProperty(document, "elementFromPoint", {
    value: elementFromPoint,
    configurable: true,
  });

  it("calculates drag distance from the latest pointer position", () => {
    expect(internalWorkspaceDragDistance({
      startX: 10,
      startY: 10,
      lastX: 58,
      lastY: 46,
    })).toBeCloseTo(60);
  });

  it("delegates to external drag only after leaving the window and passing the threshold", () => {
    const session = {
      startX: 40,
      startY: 40,
      lastX: 130,
      lastY: 40,
    };

    expect(shouldDelegateToExternalDrag(
      session,
      130,
      40,
      { width: 120, height: 120 },
      72,
    )).toBe(true);

    expect(shouldDelegateToExternalDrag(
      session,
      119,
      40,
      { width: 120, height: 120 },
      72,
    )).toBe(false);

    expect(shouldDelegateToExternalDrag(
      { ...session, lastX: 80, lastY: 40 },
      130,
      40,
      { width: 120, height: 120 },
      72,
    )).toBe(false);
  });

  it("resolves folder targets before falling back to the current directory", () => {
    document.body.innerHTML = `
      <div class="files-browser">
        <div class="files-list__item" data-folder-path="Archive">
          <span class="child">Archive</span>
        </div>
      </div>
    `;
    const folderChild = document.querySelector(".child") as HTMLElement;
    elementFromPoint.mockReturnValue(folderChild);

    expect(resolveWorkspaceDropTarget(document, 30, 30, "Current")).toBe("Archive");

    const browser = document.querySelector(".files-browser") as HTMLElement;
    elementFromPoint.mockReturnValue(browser);
    expect(resolveWorkspaceDropTarget(document, 12, 12, "Current")).toBe("Current");

    elementFromPoint.mockReturnValue(null);
    expect(resolveWorkspaceDropTarget(document, 999, 999, "Current")).toBeNull();
  });

  it("filters out same-folder and self-targeted internal moves", () => {
    expect(getWorkspaceParentPath("Scenes/Act1/shot.txt")).toBe("Scenes/Act1");
    expect(normalizeWorkspaceMovePaths([
      "Scenes/Act1/shot.txt",
      "Scenes",
      "Loose.txt",
    ], "Scenes/Act1")).toEqual(["Scenes", "Loose.txt"]);
  });
});
