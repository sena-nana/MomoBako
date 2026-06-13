const protectedCandidateFields = new Set([
  "comment",
  "rating",
  "listeningProgress",
  "listeningStatus",
  "lastListenedAt",
  "trackPositionMs",
  "trackDurationMs",
]);

const providerCandidateFields = new Set([
  "workId",
  "rjCode",
  "title",
  "workTitle",
  "circle",
  "creator",
  "voiceActors",
  "characters",
  "series",
  "scenarioTags",
  "audioTraits",
  "nsfw",
  "ageRating",
  "language",
  "releaseDate",
  "price",
  "dlCount",
  "sales",
  "reviewCount",
  "rateCount",
  "rateAverage",
  "rateCountDetail",
  "rank",
  "cover",
  "coverUrl",
  "coverSourceWorkId",
  "sourceUrl",
  "purchaseSource",
]);

function isPlainRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeProviderName(value) {
  if (typeof value !== "string") return "";
  const normalized = value.trim();
  return normalized || "";
}

function normalizeCandidateFields(candidate) {
  const rawFields = isPlainRecord(candidate.fields)
    ? candidate.fields
    : isPlainRecord(candidate.metadata)
      ? candidate.metadata
      : candidate;
  const fields = {};
  for (const [key, value] of Object.entries(rawFields)) {
    if (key === "source" || key === "confidence" || key === "fields" || key === "metadata") continue;
    fields[key] = value;
  }
  return fields;
}

export function readMetadataCandidates(metadata) {
  const rawCandidates = metadata?.providerCandidates ?? metadata?.asmrProviderCandidates;
  if (!Array.isArray(rawCandidates)) return [];
  return rawCandidates
    .filter(isPlainRecord)
    .map((candidate) => ({
      source: normalizeProviderName(candidate.source) || "provider",
      confidence: normalizeProviderName(candidate.confidence) || undefined,
      fields: normalizeCandidateFields(candidate),
    }))
    .filter((candidate) => Object.keys(candidate.fields).length);
}

export function parseCandidateJson(raw) {
  const trimmed = raw.trim();
  if (!trimmed) return { ok: false, error: "候选 JSON 为空" };
  let parsed;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { ok: false, error: "候选 JSON 格式不正确" };
  }
  if (!isPlainRecord(parsed)) {
    return { ok: false, error: "候选 JSON 必须是对象" };
  }
  const candidate = {
    source: normalizeProviderName(parsed.source) || "manual",
    confidence: normalizeProviderName(parsed.confidence) || "manual",
    fields: normalizeCandidateFields(parsed),
  };
  if (!Object.keys(candidate.fields).length) {
    return { ok: false, error: "候选 JSON 没有可读取字段" };
  }
  return { ok: true, candidate };
}

export function appendMetadataCandidate(metadata, candidate) {
  const current = readMetadataCandidates(metadata);
  return [
    ...current,
    {
      source: candidate.source,
      confidence: candidate.confidence,
      fields: candidate.fields,
    },
  ];
}

export function buildCandidateSummary(candidate) {
  const patch = {};
  const skipped = [];
  for (const [key, value] of Object.entries(candidate.fields)) {
    if (protectedCandidateFields.has(key) || !providerCandidateFields.has(key)) {
      skipped.push(key);
      continue;
    }
    patch[key] = value;
  }
  return {
    source: candidate.source,
    confidence: candidate.confidence ?? "候选",
    patch,
    skipped,
  };
}

export function formatCandidateFieldValue(value) {
  if (Array.isArray(value)) {
    return value
      .filter((item) => typeof item === "string" || typeof item === "number" || typeof item === "boolean")
      .map(String)
      .filter(Boolean)
      .join("，");
  }
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value == null) return "";
  return JSON.stringify(value) ?? "";
}
