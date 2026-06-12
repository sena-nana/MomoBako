import { afterEach, describe, expect, it } from "vitest";
import { buildSearchRequest, resetSearchState, updateFilters } from "../src/composables/workspace/search";
import { activeRepoId } from "../src/composables/workspace/state";

describe("workspace search filters", () => {
  afterEach(() => {
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
});
