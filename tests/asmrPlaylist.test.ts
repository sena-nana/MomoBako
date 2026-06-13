import { describe, expect, it, vi } from "vitest";
import { h, computed, reactive, ref, watch } from "vue";
import { register as registerAsmrLibrary } from "../External/Plugins/library-asmr/src/register";
import {
  clearPlaylistForRepo,
  buildListeningProgressMetadata,
  createPlaylistItem,
  hasPlaylistEntry,
  mergePlaylistEntries,
  parseStoredPlaylist,
  resolveActivePlaylist,
  shouldPersistListeningProgress,
} from "../External/Plugins/library-asmr/src/asmrLibrary";
import type { FileBrowserEntry } from "../src/types/repository";

function asmrEntry(path: string, trackTitle: string, status = "listening"): FileBrowserEntry {
  return {
    path,
    name: path.split("/").pop() ?? path,
    kind: "file",
    extension: "mp3",
    assetId: `asset-${trackTitle}`,
    metadata: {
      libraryKind: "asmr",
      workId: "RJ123456",
      rjCode: "RJ123456",
      workRoot: "Voice/RJ123456 Rain Voice",
      workTitle: "Rain Voice",
      trackPath: path,
      trackTitle,
      asmrEntryKind: "audio",
      listeningStatus: status,
    },
  };
}

describe("asmrPlaylist", () => {
  it("从 ASMR 音轨生成队列项并跳过非音轨", () => {
    expect(createPlaylistItem("repo-1", asmrEntry("Voice/RJ123456/01.mp3", "01 intro"))).toMatchObject({
      repoId: "repo-1",
      path: "Voice/RJ123456/01.mp3",
      title: "01 intro",
      workTitle: "Rain Voice",
      status: "收听中",
    });
    expect(createPlaylistItem("repo-1", {
      path: "Voice/RJ123456/readme.txt",
      name: "readme.txt",
      kind: "file",
      extension: "txt",
      metadata: { libraryKind: "asmr", asmrEntryKind: "companion" },
    })).toBeNull();
  });

  it("读取新旧 localStorage 队列格式", () => {
    expect(parseStoredPlaylist(JSON.stringify(["Voice/RJ123456/01.mp3"]))).toEqual([
      {
        repoId: "",
        path: "Voice/RJ123456/01.mp3",
        title: "01.mp3",
        workTitle: "",
        status: "",
      },
    ]);
    expect(parseStoredPlaylist(JSON.stringify([
      {
        repoId: "repo-1",
        path: "Voice/RJ123456/02.mp3",
        title: "02 rain",
        workTitle: "Rain Voice",
        status: "未收听",
      },
    ]))).toEqual([
      {
        repoId: "repo-1",
        path: "Voice/RJ123456/02.mp3",
        title: "02 rain",
        workTitle: "Rain Voice",
        status: "未收听",
        workRoot: undefined,
        trackPath: undefined,
        assetId: null,
      },
    ]);
  });

  it("合并队列时按仓库和路径去重，并保留不可见项", () => {
    const stored = parseStoredPlaylist(JSON.stringify([
      {
        repoId: "repo-1",
        path: "Voice/RJ123456/01.mp3",
        title: "01 stale",
        workTitle: "Old",
        status: "",
      },
      {
        repoId: "repo-1",
        path: "Voice/RJ654321/99.mp3",
        title: "99 hidden",
        workTitle: "Hidden",
        status: "未收听",
      },
    ]));
    const next = mergePlaylistEntries(stored, "repo-1", [
      asmrEntry("Voice/RJ123456/01.mp3", "01 intro"),
      asmrEntry("Voice/RJ123456/02.mp3", "02 rain", "unlistened"),
    ]);
    const visible = new Map([
      ["Voice/RJ123456/01.mp3", asmrEntry("Voice/RJ123456/01.mp3", "01 intro")],
      ["Voice/RJ123456/02.mp3", asmrEntry("Voice/RJ123456/02.mp3", "02 rain", "unlistened")],
    ]);

    expect(next.map((item) => item.path)).toEqual([
      "Voice/RJ123456/01.mp3",
      "Voice/RJ654321/99.mp3",
      "Voice/RJ123456/02.mp3",
    ]);
    expect(resolveActivePlaylist(next, "repo-1", visible)).toEqual(expect.arrayContaining([
      expect.objectContaining({ path: "Voice/RJ123456/01.mp3", title: "01 intro" }),
      expect.objectContaining({ path: "Voice/RJ654321/99.mp3", title: "99 hidden" }),
      expect.objectContaining({ path: "Voice/RJ123456/02.mp3", status: "未收听" }),
    ]));
    expect(hasPlaylistEntry(next, "repo-1", "Voice/RJ123456/02.mp3")).toBe(true);
    expect(clearPlaylistForRepo(next, "repo-1")).toEqual([]);
  });

  it("由 ASMR 插件根据通用播放事件生成收听进度", () => {
    const entry = asmrEntry("Voice/RJ123456/01.mp3", "01 intro");
    const event = {
      repoId: "repo-1",
      entry,
      state: "timeupdate",
      currentTimeMs: 30_000,
      durationMs: 120_000,
    };

    expect(shouldPersistListeningProgress(event, null, 1_000)).toBe(true);
    expect(shouldPersistListeningProgress(event, { savedAtMs: 500, savedSecond: 29 }, 1_000)).toBe(false);
    expect(buildListeningProgressMetadata(event, new Date("2026-06-13T00:00:00.000Z"))).toEqual({
      listeningProgress: 25,
      listeningStatus: "listening",
      lastListenedAt: "2026-06-13T00:00:00.000Z",
      trackDurationMs: 120_000,
      trackPositionMs: 30_000,
    });
    expect(buildListeningProgressMetadata({
      ...event,
      state: "ended",
      currentTimeMs: 118_000,
    }, new Date("2026-06-13T00:00:00.000Z"))).toMatchObject({
      listeningProgress: 100,
      listeningStatus: "listened",
      trackPositionMs: 120_000,
    });
    expect(shouldPersistListeningProgress({
      ...event,
      entry: { ...entry, metadata: {} },
    }, null, 1_000)).toBe(false);
  });

  it("ASMR 插件订阅通用媒体事件并保存自己的播放进度", async () => {
    const handlers = new Map<string, Array<(payload: unknown) => unknown>>();
    const saveMetadata = vi.fn().mockResolvedValue(null);
    const entry = asmrEntry("Voice/RJ123456/01.mp3", "01 intro");

    registerAsmrLibrary({
      vue: { h, computed, reactive, ref, watch },
      callPlugin: vi.fn(),
      onPluginEvent(eventName: string, handler: (payload: unknown) => unknown) {
        handlers.set(eventName, [...(handlers.get(eventName) ?? []), handler]);
        return vi.fn();
      },
      registerLibraryExtension: vi.fn((definition) => definition),
    });

    await Promise.all((handlers.get("media.playback") ?? []).map((handler) => handler({
      repoId: "repo-1",
      entry,
      state: "pause",
      currentTimeMs: 60_000,
      durationMs: 120_000,
      saveMetadata,
    })));

    expect(saveMetadata).toHaveBeenCalledWith(entry, expect.objectContaining({
      listeningProgress: 50,
      listeningStatus: "listening",
      trackDurationMs: 120_000,
      trackPositionMs: 60_000,
    }));
  });
});
