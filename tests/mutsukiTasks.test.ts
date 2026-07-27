import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriMocks.listen,
}));

import { runMutsukiTask } from "../src/services/mutsukiTasks";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function cancelledResult() {
  return {
    outcome: {
      status: "cancelled",
      reason: "frontend aborted",
    },
  };
}

describe("runMutsukiTask cancellation", () => {
  beforeEach(() => {
    tauriMocks.invoke.mockReset();
    tauriMocks.listen.mockReset();
    tauriMocks.listen.mockResolvedValue(vi.fn());
  });

  it("does not listen or submit when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(runMutsukiTask("test.protocol", {}, undefined, controller.signal))
      .rejects.toMatchObject({ name: "AbortError" });

    expect(tauriMocks.listen).not.toHaveBeenCalled();
    expect(tauriMocks.invoke).not.toHaveBeenCalled();
  });

  it("does not submit when cancellation happens while event listening is pending", async () => {
    const listening = deferred<() => void>();
    const unlisten = vi.fn();
    tauriMocks.listen.mockReturnValue(listening.promise);
    const controller = new AbortController();

    const task = runMutsukiTask("test.protocol", {}, undefined, controller.signal);
    const rejection = expect(task).rejects.toMatchObject({ name: "AbortError" });
    await vi.waitFor(() => expect(tauriMocks.listen).toHaveBeenCalledOnce());
    controller.abort();
    listening.resolve(unlisten);

    await rejection;
    expect(tauriMocks.invoke).not.toHaveBeenCalled();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("cancels after start registers the task and waits for its terminal result", async () => {
    const started = deferred<{ task_id: string }>();
    const cancellation = deferred<boolean>();
    tauriMocks.invoke.mockImplementation((command: string) => {
      if (command === "mutsuki_start_task") return started.promise;
      if (command === "mutsuki_cancel_task") return cancellation.promise;
      if (command === "mutsuki_task_result") return Promise.resolve(cancelledResult());
      throw new Error(`unexpected command: ${command}`);
    });
    const controller = new AbortController();

    const task = runMutsukiTask("test.protocol", {}, undefined, controller.signal);
    const rejection = expect(task).rejects.toMatchObject({ name: "AbortError" });
    await vi.waitFor(() => {
      expect(tauriMocks.invoke).toHaveBeenCalledWith("mutsuki_start_task", expect.anything());
    });
    controller.abort();
    expect(tauriMocks.invoke).not.toHaveBeenCalledWith("mutsuki_cancel_task", expect.anything());

    started.resolve({ task_id: "registered-task" });
    await vi.waitFor(() => {
      expect(tauriMocks.invoke).toHaveBeenCalledWith("mutsuki_cancel_task", {
        request: { task_id: "registered-task", reason: "frontend aborted" },
      });
    });
    expect(tauriMocks.invoke).not.toHaveBeenCalledWith("mutsuki_task_result", expect.anything());

    cancellation.resolve(true);
    await rejection;
    expect(tauriMocks.invoke).toHaveBeenCalledWith("mutsuki_task_result", {
      request: { task_id: "registered-task" },
    });
    expect(tauriMocks.invoke.mock.calls.filter(([command]) => command === "mutsuki_cancel_task"))
      .toHaveLength(1);
  });
});
