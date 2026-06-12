import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function readManifest<T = Record<string, unknown>>(pluginDir: string): T {
  return JSON.parse(readFileSync(resolve("External/Plugins", pluginDir, "manifest.json"), "utf-8")) as T;
}

describe("ASMR plugin contracts", () => {
  it("declares Kikoeru-equivalent ASMR library fields, views and hooks", () => {
    const manifest = readManifest<{
      pluginId: string;
      version: string;
      optional: string[];
      hooks: Array<{ slot: string; action: string }>;
      contributes: {
        libraryKind: {
          fields: string[];
          facets: string[];
          sortFields: string[];
          views: string[];
          recognition: Record<string, unknown>;
          progress: Record<string, unknown>;
        };
      };
    }>("library-asmr");

    expect(manifest.pluginId).toBe("momobako.library.asmr");
    expect(manifest.version).toBe("0.2.0");
    expect(manifest.optional).toEqual(expect.arrayContaining([
      "momobako.parser.asmr-folder",
      "momobako.service.provider.dlsite",
      "momobako.service.provider.asmr-one",
      "momobako.preview.media",
    ]));
    expect(manifest.hooks.map((hook) => hook.slot)).toEqual(expect.arrayContaining([
      "playlist",
      "progress",
      "search",
      "metadataMerge",
      "batchOrganize",
    ]));
    expect(manifest.contributes.libraryKind.recognition).toMatchObject({
      folderPattern: "(?i)RJ\\d{6,8}",
      duplicatePolicy: "candidate-warning",
      nonMatchingFolderPolicy: "recurse",
    });
    expect(manifest.contributes.libraryKind.fields).toEqual(expect.arrayContaining([
      "workId",
      "rjCode",
      "circle",
      "voiceActors",
      "lyricStatus",
      "listeningStatus",
      "lastListenedAt",
    ]));
    expect(manifest.contributes.libraryKind.views).toEqual(expect.arrayContaining([
      "works",
      "tracks",
      "withLyrics",
      "continueListening",
      "missingMetadata",
    ]));
    expect(manifest.contributes.libraryKind.sortFields).toEqual(expect.arrayContaining([
      "releaseDate",
      "dlCount",
      "price",
      "rateAverage",
      "reviewCount",
    ]));
    expect(manifest.contributes.libraryKind.sortFields).not.toContain("random");
    expect(manifest.contributes.libraryKind.progress).toMatchObject({
      statusField: "listeningStatus",
      positionField: "listeningProgress",
      lastOpenedField: "lastListenedAt",
    });
  });

  it("declares ASMR folder parser and network providers as candidate-only plugins", () => {
    const parser = readManifest<{
      pluginId: string;
      enabled: boolean;
      contributes: { parser: { candidateOnly: boolean; trackExtensions: string[]; lyricExtensions: string[] } };
    }>("parser-asmr-folder");
    const dlsite = readManifest<{
      pluginId: string;
      enabled: boolean;
      requires: string[];
      contributes: { provider: { candidateOnly: boolean; manualTrigger: boolean; fields: string[]; settings: Record<string, unknown> } };
    }>("service-provider-dlsite");
    const asmrOne = readManifest<{
      pluginId: string;
      enabled: boolean;
      requires: string[];
      contributes: { provider: { candidateOnly: boolean; manualTrigger: boolean; fields: string[] } };
    }>("service-provider-asmr-one");

    expect(parser.pluginId).toBe("momobako.parser.asmr-folder");
    expect(parser.enabled).toBe(false);
    expect(parser.contributes.parser.candidateOnly).toBe(true);
    expect(parser.contributes.parser.trackExtensions).toEqual(expect.arrayContaining(["mp3", "flac", "m4a", "mka"]));
    expect(parser.contributes.parser.lyricExtensions).toEqual(expect.arrayContaining(["lrc", "srt", "ass", "vtt"]));

    for (const provider of [dlsite, asmrOne]) {
      expect(provider.enabled).toBe(false);
      expect(provider.requires).toEqual(["momobako.service.network-search"]);
      expect(provider.contributes.provider.candidateOnly).toBe(true);
      expect(provider.contributes.provider.manualTrigger).toBe(true);
      expect(provider.contributes.provider.fields).toEqual(expect.arrayContaining(["workId", "rjCode", "title", "circle", "voiceActors"]));
    }
    expect(dlsite.contributes.provider.fields).toEqual(expect.arrayContaining(["dlCount", "price", "coverSourceWorkId"]));
    expect(dlsite.contributes.provider.settings).toHaveProperty("proxy");
  });
});
