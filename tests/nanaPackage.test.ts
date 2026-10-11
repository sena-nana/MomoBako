// Nana 发布包脚本的纯函数：配置模板和二进制身份一致、按发行方式生成配置、读 .momoplug 的包信封。
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateRawSync } from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";

import {
  archiveName,
  MOMOPLUG_ABI,
  readMomoplugEnvelope,
  readTemplateInfo,
  renderPackageConfig,
} from "../scripts/nana-package-config.ts";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path: string) => readFileSync(join(repoRoot, path), "utf-8");

/** 拼一个最小的 zip：本地头、数据、中央目录和目录尾，CRC 不校验，填 0。 */
function makeZip(entries: { name: string; data: string; deflate: boolean }[]): Buffer {
  const locals: Buffer[] = [];
  const centrals: Buffer[] = [];
  let offset = 0;
  for (const entry of entries) {
    const name = Buffer.from(entry.name, "utf8");
    const raw = Buffer.from(entry.data, "utf8");
    const data = entry.deflate ? deflateRawSync(raw) : raw;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(entry.deflate ? 8 : 0, 8);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(name.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(entry.deflate ? 8 : 0, 10);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(raw.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, name, data);
    centrals.push(central, name);
    offset += local.length + name.length + data.length;
  }
  const directory = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

const temporaryDirs: string[] = [];
afterEach(() => {
  for (const dir of temporaryDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function writeTemporary(name: string, contents: Buffer): string {
  const dir = mkdtempSync(join(tmpdir(), "momobako-nana-package-"));
  temporaryDirs.push(dir);
  const path = join(dir, name);
  writeFileSync(path, contents);
  return path;
}

describe("Nana 发布包配置", () => {
  it("配置模板和二进制里的身份、crate 版本一致", () => {
    const info = readTemplateInfo(read("src-nana/nana-package.toml"));
    const buildScript = read("src-nana/build.rs");
    const constant = (name: string) => buildScript.match(new RegExp(`const ${name}: &str = "([^"]+)";`))?.[1];
    const crateVersion = read("src-nana/Cargo.toml").match(/^version\s*=\s*"([^"]+)"/m)?.[1];

    expect(info).toEqual({
      id: constant("APP_ID"),
      name: constant("APP_NAME"),
      version: crateVersion,
      fileName: "MomoBako",
      backend: "installer",
    });
    expect(constant("EXECUTABLE")).toBe(`${info.fileName}.exe`);
    expect(read("src-nana/nana-package.toml")).toMatch(new RegExp(`^vendor = "${constant("APP_VENDOR")}"$`, "m"));
  });

  it("便携版换发行方式，随附插件逐个写进 runtime.plugins，相对路径换成绝对路径", () => {
    const template = [
      "schema_version = 1",
      "[distribution]",
      "backend = \"installer\"",
      "# tools = [{ source = \"in-comment\" }]",
      "[runtime]",
      "tools = [{ source = \"vendor/ffmpeg.exe\" }]",
    ].join("\r\n");
    const config = renderPackageConfig(template, "D:/repo/src-nana", "portable", [
      { name: "momobako.local-filesystem", source: "D:\\repo\\External\\Plugins\\.packages\\local.momoplug", version: "1.0.0" },
    ]);

    expect(config).toContain("backend = \"portable\"");
    expect(config).not.toContain("installer");
    expect(config).toContain("tools = [{ source = \"D:/repo/src-nana/vendor/ffmpeg.exe\" }]");
    expect(config).toContain("# tools = [{ source = \"in-comment\" }]");
    expect(config).toContain([
      "[[runtime.plugins]]",
      "name = \"momobako.local-filesystem\"",
      "source = \"D:/repo/External/Plugins/.packages/local.momoplug\"",
      `abi = "${MOMOPLUG_ABI}"`,
      "version = \"1.0.0\"",
    ].join("\n"));
    expect(() => renderPackageConfig(`${template}\n[[runtime.plugins]]\nname = "x"`, "D:/repo", "installer", []))
      .toThrow("runtime.plugins");
  });

  it("发布包文件名带版本、架构和发行方式", () => {
    const info = { id: "com.momobako.desktop", name: "MomoBako", version: "0.1.0", fileName: "MomoBako", backend: "installer" };
    expect(archiveName(info, "x86_64-pc-windows-msvc", "installer")).toBe("MomoBako_0.1.0_x64.zip");
    expect(archiveName(info, "aarch64-pc-windows-msvc", "portable")).toBe("MomoBako_0.1.0_arm64_portable.zip");
  });

  it("从 .momoplug 里读出包信封，不认第 1 版和缺字段的", () => {
    const envelope = {
      formatVersion: 2,
      pluginId: "momobako.local-filesystem",
      version: "1.0.0",
      targetTriple: "x86_64-pc-windows-msvc",
      deployment: "abi",
    };
    const plugin = writeTemporary("local-filesystem.momoplug", makeZip([
      { name: "local-filesystem/manifest.json", data: "{}", deflate: false },
      { name: "local-filesystem/momobako.package.json", data: JSON.stringify(envelope), deflate: true },
    ]));
    expect(readMomoplugEnvelope(plugin)).toEqual({
      formatVersion: 2,
      pluginId: "momobako.local-filesystem",
      version: "1.0.0",
      targetTriple: "x86_64-pc-windows-msvc",
    });

    const legacy = writeTemporary("legacy.momoplug", makeZip([
      { name: "legacy/momobako.package.json", data: JSON.stringify({ ...envelope, formatVersion: 1 }), deflate: false },
    ]));
    expect(() => readMomoplugEnvelope(legacy)).toThrow("第 2 版");
    const missing = writeTemporary("missing.momoplug", makeZip([{ name: "missing/manifest.json", data: "{}", deflate: false }]));
    expect(() => readMomoplugEnvelope(missing)).toThrow("momobako.package.json");
  });
});
