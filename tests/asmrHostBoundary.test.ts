import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const forbiddenPatterns = [
  /\basmr\b/i,
  /\bRJ\d{6,8}\b/,
  /\brjCode\b/,
  /\bDLsite\b/i,
  /\basmr-one\b/i,
  /\bASMR One\b/i,
  /lookup_asmr/i,
];

describe("ASMR host boundary", () => {
  it("keeps ASMR-specific implementation out of host source trees", () => {
    const files = [
      ...scanSourceFiles(resolve("src"), [".ts", ".vue"]),
      ...scanSourceFiles(resolve("src-tauri/src"), [".rs"]),
    ];
    const violations = files.flatMap((file) => {
      const text = readFileSync(file, "utf-8");
      return forbiddenPatterns
        .filter((pattern) => pattern.test(text))
        .map((pattern) => `${relative(resolve("."), file)}: ${pattern}`);
    });

    expect(violations).toEqual([]);
  });
});

function scanSourceFiles(root: string, extensions: string[]): string[] {
  if (!statSync(root, { throwIfNoEntry: false })?.isDirectory()) return [];
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) return scanSourceFiles(path, extensions);
    return extensions.some((extension) => entry.name.endsWith(extension)) ? [path] : [];
  });
}
