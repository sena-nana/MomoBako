import { afterEach, describe, expect, it, vi } from "vitest";
import * as repositoryApi from "../src/services/repositoryApi";
import type { SearchHit, SearchResponse } from "../src/types/repository";
import {
  buildSearchRequest,
  resetSearchState,
  runSearch,
  updateFilters,
} from "../src/composables/workspace/search";
import {
  activeRepoId,
  isSearching,
  searchResults,
} from "../src/composables/workspace/state";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function searchHit(filename: string): SearchHit {
  return {
    repoId: "repo-main-001",
    repoName: "主资源库",
    assetId: `asset-${filename}`,
    path: filename,
    filename,
    status: "synced",
    tags: [],
    metadata: {},
  };
}

describe("workspace search filters", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    resetSearchState();
    activeRepoId.value = null;
  });

  it("builds positive metadata filters for ASMR shortcuts and generic metadata queries", () => {
    activeRepoId.value = "repo-main-001";
    updateFilters({
      metadataFilters: "libraryKind=asmr\nlyricStatus=local",
      sortField: "random",
      sortDirection: "asc",
    });

    expect(buildSearchRequest("")).toMatchObject({
      repoId: "repo-main-001",
      metadataFilters: [
        { key: "libraryKind", value: "asmr" },
        { key: "lyricStatus", value: "local" },
      ],
      sort: {
        field: "random",
        direction: "asc",
      },
    });
  });

  it("旧请求后完成时不会覆盖最新搜索结果", async () => {
    const oldResponse = deferred<SearchResponse>();
    const latestResponse = deferred<SearchResponse>();
    vi.spyOn(repositoryApi, "searchAssets").mockImplementation((request) => (
      request.query === "old" ? oldResponse.promise : latestResponse.promise
    ));

    const oldSearch = runSearch({ query: "old" });
    const latestSearch = runSearch({ query: "latest" });
    latestResponse.resolve({
      query: "latest",
      results: [searchHit("latest.png")],
    });
    await latestSearch;

    expect(searchResults.value.map((result) => result.filename)).toEqual(["latest.png"]);

    oldResponse.resolve({
      query: "old",
      results: [searchHit("old.png")],
    });
    await oldSearch;

    expect(searchResults.value.map((result) => result.filename)).toEqual(["latest.png"]);
  });

  it("旧请求先完成时不会提交结果或提前结束最新请求", async () => {
    const oldResponse = deferred<SearchResponse>();
    const latestResponse = deferred<SearchResponse>();
    vi.spyOn(repositoryApi, "searchAssets").mockImplementation((request) => (
      request.query === "old" ? oldResponse.promise : latestResponse.promise
    ));

    const oldSearch = runSearch({ query: "old" });
    const latestSearch = runSearch({ query: "latest" });
    oldResponse.resolve({
      query: "old",
      results: [searchHit("old.png")],
    });
    await oldSearch;

    expect(searchResults.value).toEqual([]);
    expect(isSearching.value).toBe(true);

    latestResponse.resolve({
      query: "latest",
      results: [searchHit("latest.png")],
    });
    await latestSearch;

    expect(searchResults.value.map((result) => result.filename)).toEqual(["latest.png"]);
    expect(isSearching.value).toBe(false);
  });

  it("清空查询时立即使仍在执行的旧请求失效", async () => {
    const oldResponse = deferred<SearchResponse>();
    vi.spyOn(repositoryApi, "searchAssets").mockReturnValue(oldResponse.promise);

    const oldSearch = runSearch({ query: "old" });
    await runSearch({ query: "" });

    expect(searchResults.value).toEqual([]);
    expect(isSearching.value).toBe(false);

    oldResponse.resolve({
      query: "old",
      results: [searchHit("old.png")],
    });
    await oldSearch;

    expect(searchResults.value).toEqual([]);
    expect(isSearching.value).toBe(false);
  });
});
