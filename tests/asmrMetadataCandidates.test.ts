import { describe, expect, it } from "vitest";
import {
  buildCandidateSummary,
  formatCandidateFieldValue,
  parseCandidateJson,
  readMetadataCandidates,
} from "../External/Plugins/library-asmr/src/asmrCandidates";

describe("asmrMetadataCandidates", () => {
  it("读取 provider-shaped 候选并保护人工和进度字段", () => {
    const candidates = readMetadataCandidates({
      providerCandidates: [
        {
          source: "dlsite",
          confidence: "external-id",
          fields: {
            workTitle: "Rain Voice",
            circle: "Blue Circle",
            voiceActors: ["Aoi", "Momo"],
            rating: 5,
            comment: "manual note",
            listeningProgress: 42,
          },
        },
      ],
    });

    expect(candidates).toHaveLength(1);
    const summary = buildCandidateSummary(candidates[0]);
    expect(summary.patch).toEqual({
      workTitle: "Rain Voice",
      circle: "Blue Circle",
      voiceActors: ["Aoi", "Momo"],
    });
    expect(summary.skipped).toEqual(["rating", "comment", "listeningProgress"]);
  });

  it("支持 metadata 形态候选并格式化数组值", () => {
    const [candidate] = readMetadataCandidates({
      asmrProviderCandidates: [
        {
          source: "asmr-one",
          metadata: {
            scenarioTags: ["耳语", "睡眠"],
            unsupportedNested: { value: true },
          },
        },
      ],
    });

    const summary = buildCandidateSummary(candidate);
    expect(summary.patch).toEqual({ scenarioTags: ["耳语", "睡眠"] });
    expect(summary.skipped).toEqual(["unsupportedNested"]);
    expect(formatCandidateFieldValue(summary.patch.scenarioTags)).toBe("耳语，睡眠");
  });

  it("解析手动粘贴的 provider JSON 候选", () => {
    const result = parseCandidateJson(JSON.stringify({
      source: "dlsite",
      confidence: "manual",
      workTitle: "Rain Voice",
      circle: "Blue Circle",
    }));

    expect(result).toEqual({
      ok: true,
      candidate: {
        source: "dlsite",
        confidence: "manual",
        fields: {
          workTitle: "Rain Voice",
          circle: "Blue Circle",
        },
      },
    });
    expect(parseCandidateJson("{")).toEqual({
      ok: false,
      error: "候选 JSON 格式不正确",
    });
  });
});
