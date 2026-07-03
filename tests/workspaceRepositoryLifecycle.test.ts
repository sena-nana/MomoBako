/**
 * 验证资源库进入流程会先同步索引，再读取首屏数据。
 */
import { afterEach, describe, expect, it } from "vitest";
import {
  ensureRepositoryWorkspace,
  resetRepositoryWorkspaceForTests,
  selectRepository,
  useWorkspaceProgress,
} from "../src/composables/workspace";
import {
  failNextInvoke,
  getInvokeCalls,
  seedMockRepository,
} from "./setupTests";

afterEach(() => {
  resetRepositoryWorkspaceForTests();
});

describe("workspace repository lifecycle", () => {
  it("启动恢复资源库时先同步再读取快照", async () => {
    seedMockRepository();
    window.localStorage.setItem("momobako.lastActiveRepositoryId", "repo-main-001");

    await ensureRepositoryWorkspace();

    const commands = getInvokeCalls().map((call) => call.command);
    expect(commands.indexOf("sync_repository")).toBeGreaterThan(-1);
    expect(commands.indexOf("get_repository_snapshot")).toBeGreaterThan(-1);
    expect(commands.indexOf("sync_repository")).toBeLessThan(
      commands.indexOf("get_repository_snapshot"),
    );
  });

  it("切换资源库时扫描失败会进入启动错误状态", async () => {
    seedMockRepository();
    failNextInvoke("sync_repository", "扫描失败");

    await selectRepository("repo-main-001");

    const progress = useWorkspaceProgress();
    expect(progress.workspaceStartup.value.status).toBe("error");
    expect(progress.workspaceStartup.value.error).toBe("扫描失败");
  });
});
