// nana-package 的纯函数部分：读打包配置模板、读 .momoplug 的包信封、生成每种发行方式的 nana-package.toml。
import { readFileSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { inflateRawSync } from "node:zlib";

/** nana-packager 认的发行方式。安装版只产出布局，便携版多一个 runtime/manifest/portable 标记。 */
export type PackageVariant = "installer" | "portable";

/** 模板里打包脚本要用到的应用信息。 */
export interface PackageTemplateInfo {
  id: string;
  name: string;
  version: string;
  fileName: string;
  backend: string;
}

/** .momoplug 里的包信封（momobako.package.json）中打包要用的字段。 */
export interface MomoplugEnvelope {
  formatVersion: number;
  pluginId: string;
  version: string;
  targetTriple: string;
}

/** 写进 [[runtime.plugins]] 的一个随附插件。 */
export interface BundledPlugin {
  name: string;
  source: string;
  version: string;
}

/** 随附插件在 NanaUI manifest 里记的 ABI：MomoBako 自己的 .momoplug 第 2 版包格式。 */
export const MOMOPLUG_ABI = "momoplug-v2";

const ENVELOPE_FILE = "momobako.package.json";
const END_OF_CENTRAL_DIRECTORY = 0x06054b50;
const CENTRAL_DIRECTORY_ENTRY = 0x02014b50;
const LOCAL_FILE_HEADER = 0x04034b50;

/** 取 TOML 里某个表下某个键的字符串值；模板由仓库维护，只认 `key = "value"` 这种写法。 */
function tableString(template: string, table: string, key: string): string {
  const lines = template.replace(/\r\n/g, "\n").split("\n");
  let inTable = false;
  for (const line of lines) {
    const header = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/);
    if (header) {
      inTable = header[1].trim() === table;
      continue;
    }
    const value = inTable ? line.match(new RegExp(`^\\s*${key}\\s*=\\s*"([^"]*)"`)) : null;
    if (value) return value[1];
  }
  throw new Error(`nana-package.toml 缺少 [${table}] ${key}`);
}

/** 读模板里的应用身份、可执行文件名和发行方式。 */
export function readTemplateInfo(template: string): PackageTemplateInfo {
  return {
    id: tableString(template, "application", "id"),
    name: tableString(template, "application", "name"),
    version: tableString(template, "application", "version"),
    fileName: tableString(template, "executable", "file_name"),
    backend: tableString(template, "distribution", "backend"),
  };
}

/** 发布包 zip 的文件名，例如 `MomoBako_0.1.0_x64_portable.zip`。 */
export function archiveName(info: PackageTemplateInfo, targetTriple: string, variant: PackageVariant): string {
  const arch = targetTriple.startsWith("x86_64-") ? "x64" : targetTriple.startsWith("aarch64-") ? "arm64" : targetTriple.split("-")[0];
  return `${info.name}_${info.version}_${arch}${variant === "portable" ? "_portable" : ""}.zip`;
}

/** TOML 基本字符串。路径统一用正斜杠，不用转义反斜杠。 */
function tomlString(value: string): string {
  return JSON.stringify(value.replace(/\\/g, "/"));
}

/**
 * 生成一种发行方式的配置：模板里的相对路径改成以模板所在目录为基准的绝对路径（生成的配置不在模板旁边），
 * 发行方式换成这次的，再在末尾为每个随附插件加一条 [[runtime.plugins]]。
 */
export function renderPackageConfig(
  template: string,
  templateDir: string,
  variant: PackageVariant,
  plugins: readonly BundledPlugin[],
): string {
  if (/^\s*\[\[?runtime\.plugins\]?\]/m.test(template) || /^\s*plugins\s*=/m.test(template)) {
    throw new Error("nana-package.toml 不要写 runtime.plugins：随附插件由打包脚本按 .momoplug 生成");
  }
  const rebased = template
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((line) => line.trimStart().startsWith("#") ? line : line.replace(
      /\b(source|root|icon|info_plist_extra)(\s*=\s*)"([^"\\]*)"/g,
      (_match, key: string, separator: string, value: string) =>
        `${key}${separator}${tomlString(isAbsolute(value) ? value : resolve(templateDir, value))}`,
    ))
    .join("\n");
  let replaced = false;
  const switched = rebased.replace(/^(\s*backend\s*=\s*)"[^"]*"/m, (_match, prefix: string) => {
    replaced = true;
    return `${prefix}"${variant}"`;
  });
  if (!replaced) throw new Error("nana-package.toml 缺少 [distribution] backend");
  const entries = plugins.map((plugin) => [
    "",
    "[[runtime.plugins]]",
    `name = ${tomlString(plugin.name)}`,
    `source = ${tomlString(plugin.source)}`,
    `abi = ${tomlString(MOMOPLUG_ABI)}`,
    `version = ${tomlString(plugin.version)}`,
  ].join("\n"));
  return `${switched.trimEnd()}\n${entries.join("\n")}\n`;
}

/** 在 zip 的中央目录里找一个条目，返回解开后的内容；只支持存储和 deflate，.momoplug 都是这两种。 */
function readZipEntry(archive: Buffer, matches: (name: string) => boolean, label: string): Buffer {
  const searchStart = Math.max(0, archive.length - 0xffff - 22);
  let end = -1;
  for (let offset = archive.length - 22; offset >= searchStart; offset -= 1) {
    if (archive.readUInt32LE(offset) === END_OF_CENTRAL_DIRECTORY) {
      end = offset;
      break;
    }
  }
  if (end < 0) throw new Error(`${label} 不是 zip`);
  const count = archive.readUInt16LE(end + 10);
  let cursor = archive.readUInt32LE(end + 16);
  const found: Buffer[] = [];
  for (let index = 0; index < count; index += 1) {
    if (archive.readUInt32LE(cursor) !== CENTRAL_DIRECTORY_ENTRY) throw new Error(`${label} 的 zip 目录损坏`);
    const method = archive.readUInt16LE(cursor + 10);
    const compressedSize = archive.readUInt32LE(cursor + 20);
    const nameLength = archive.readUInt16LE(cursor + 28);
    const extraLength = archive.readUInt16LE(cursor + 30);
    const commentLength = archive.readUInt16LE(cursor + 32);
    const localOffset = archive.readUInt32LE(cursor + 42);
    const name = archive.toString("utf8", cursor + 46, cursor + 46 + nameLength);
    cursor += 46 + nameLength + extraLength + commentLength;
    if (!matches(name)) continue;
    if (archive.readUInt32LE(localOffset) !== LOCAL_FILE_HEADER) throw new Error(`${label} 的 ${name} 本地头损坏`);
    const dataStart = localOffset + 30 + archive.readUInt16LE(localOffset + 26) + archive.readUInt16LE(localOffset + 28);
    const data = archive.subarray(dataStart, dataStart + compressedSize);
    if (method === 0) found.push(data);
    else if (method === 8) found.push(inflateRawSync(data));
    else throw new Error(`${label} 的 ${name} 用了不支持的压缩方式 ${method}`);
  }
  if (found.length !== 1) throw new Error(`${label} 里应当正好有一个 ${ENVELOPE_FILE}，实际 ${found.length} 个`);
  return found[0];
}

/** 读 .momoplug 的包信封，校验是第 2 版格式、字段齐全。 */
export function readMomoplugEnvelope(path: string): MomoplugEnvelope {
  const raw = readZipEntry(
    readFileSync(path),
    (name) => name === ENVELOPE_FILE || name.endsWith(`/${ENVELOPE_FILE}`),
    path,
  );
  const value: unknown = JSON.parse(raw.toString("utf8"));
  if (typeof value !== "object" || value === null) throw new Error(`${path} 的包信封不是对象`);
  const envelope = value as Record<string, unknown>;
  const { formatVersion, pluginId, version, targetTriple } = envelope;
  if (
    formatVersion !== 2
    || typeof pluginId !== "string" || pluginId.trim() === ""
    || typeof version !== "string" || version.trim() === ""
    || typeof targetTriple !== "string" || targetTriple.trim() === ""
  ) {
    throw new Error(`${path} 的包信封不是第 2 版或缺字段`);
  }
  return { formatVersion, pluginId, version, targetTriple };
}
