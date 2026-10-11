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

/**
 * ASMR 的识别、元数据和查询都在插件里（parser-asmr-folder、provider-dlsite、provider-asmr-one、library-asmr），
 * 领域服务不认识这个库类型。Nana 宿主自己画 ASMR 作品面板（`src-nana/src/shell/inspect_asmr.rs` 等），
 * 不在这条边界里。
 */
describe("ASMR host boundary", () => {
  it("keeps ASMR-specific implementation out of the domain backend", () => {
    const files = [
      ...scanSourceFiles(resolve("src-backend/src"), [".rs"]),
      ...scanSourceFiles(resolve("src-tauri/src"), [".rs"]),
    ];
    const violations = files.flatMap((file) => {
      const text = readFileSync(file, "utf-8");
      return forbiddenPatterns
        .filter((pattern) => pattern.test(text))
        .map((pattern) => `${relative(resolve("."), file)}: ${pattern}`);
    });

    expect(files.length).toBeGreaterThan(0);
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
