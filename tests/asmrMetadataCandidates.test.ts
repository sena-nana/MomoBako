import { describe, expect, it } from "vitest";
import {
  buildAsmrMetadataCandidateSummary,
  formatCandidateFieldValue,
  parseAsmrMetadataCandidateJson,
  readAsmrMetadataCandidates,
} from "../src/pages/workspace/asmrMetadataCandidates";

describe("asmrMetadataCandidates", () => {
  it("读取 provider-shaped 候选并保护人工和进度字段", () => {
    const candidates = readAsmrMetadataCandidates({
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
    const summary = buildAsmrMetadataCandidateSummary(candidates[0]);
    expect(summary.patch).toEqual({
      workTitle: "Rain Voice",
      circle: "Blue Circle",
      voiceActors: ["Aoi", "Momo"],
    });
    expect(summary.skipped).toEqual(["rating", "comment", "listeningProgress"]);
  });

  it("支持 metadata 形态候选并格式化数组值", () => {
    const [candidate] = readAsmrMetadataCandidates({
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

    const summary = buildAsmrMetadataCandidateSummary(candidate);
    expect(summary.patch).toEqual({ scenarioTags: ["耳语", "睡眠"] });
    expect(summary.skipped).toEqual(["unsupportedNested"]);
    expect(formatCandidateFieldValue(summary.patch.scenarioTags)).toBe("耳语，睡眠");
  });

  it("解析手动粘贴的 provider JSON 候选", () => {
    const result = parseAsmrMetadataCandidateJson(JSON.stringify({
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
    expect(parseAsmrMetadataCandidateJson("{")).toEqual({
      ok: false,
      error: "候选 JSON 格式不正确",
    });
  });
});
