import { describe, expect, it } from "vitest";
import {
  clearAsmrPlaylistForRepo,
  createAsmrPlaylistItem,
  hasAsmrPlaylistEntry,
  mergeAsmrPlaylistEntries,
  parseStoredAsmrPlaylist,
  resolveActiveAsmrPlaylist,
} from "../src/pages/workspace/asmrPlaylist";
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
    expect(createAsmrPlaylistItem("repo-1", asmrEntry("Voice/RJ123456/01.mp3", "01 intro"))).toMatchObject({
      repoId: "repo-1",
      path: "Voice/RJ123456/01.mp3",
      title: "01 intro",
      workTitle: "Rain Voice",
      status: "收听中",
    });
    expect(createAsmrPlaylistItem("repo-1", {
      path: "Voice/RJ123456/readme.txt",
      name: "readme.txt",
      kind: "file",
      extension: "txt",
      metadata: { libraryKind: "asmr", asmrEntryKind: "companion" },
    })).toBeNull();
  });

  it("读取新旧 localStorage 队列格式", () => {
    expect(parseStoredAsmrPlaylist(JSON.stringify(["Voice/RJ123456/01.mp3"]))).toEqual([
      {
        repoId: "",
        path: "Voice/RJ123456/01.mp3",
        title: "01.mp3",
        workTitle: "",
        status: "",
      },
    ]);
    expect(parseStoredAsmrPlaylist(JSON.stringify([
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
    const stored = parseStoredAsmrPlaylist(JSON.stringify([
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
    const next = mergeAsmrPlaylistEntries(stored, "repo-1", [
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
    expect(resolveActiveAsmrPlaylist(next, "repo-1", visible)).toEqual(expect.arrayContaining([
      expect.objectContaining({ path: "Voice/RJ123456/01.mp3", title: "01 intro" }),
      expect.objectContaining({ path: "Voice/RJ654321/99.mp3", title: "99 hidden" }),
      expect.objectContaining({ path: "Voice/RJ123456/02.mp3", status: "未收听" }),
    ]));
    expect(hasAsmrPlaylistEntry(next, "repo-1", "Voice/RJ123456/02.mp3")).toBe(true);
    expect(clearAsmrPlaylistForRepo(next, "repo-1")).toEqual([]);
  });
});
