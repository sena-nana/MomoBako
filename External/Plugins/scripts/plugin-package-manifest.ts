// 统一解析、刷新并校验 .momoplug 的 Momo 与 Mutsuki 双清单。
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { isAbsolute, relative, resolve } from "node:path";
import { parse, stringify } from "smol-toml";

interface MomoPluginManifest {
  pluginId: string;
  version: string;
  runtime?: string;
  entry?: {
    backend?: unknown;
  };
}

interface MutsukiCompanionArtifact {
  path: string;
  sha256: string;
  executable?: boolean;
  role?: string;
}

interface MutsukiPluginArtifact {
  path: string;
  sha256: string;
  companion_artifacts?: MutsukiCompanionArtifact[];
}

interface MutsukiPluginManifest {
  plugin_id: string;
  version: string;
  artifact: MutsukiPluginArtifact;
}

interface MutsukiPluginToml {
  manifest: MutsukiPluginManifest;
  [key: string]: unknown;
}

export interface ValidatedPluginPackage {
  manifest: MomoPluginManifest;
  pluginToml?: MutsukiPluginToml;
}

/** 判断 Momo 清单是否声明了需要 Mutsuki 承载的执行型后端。 */
function isExecutableBackend(manifest: MomoPluginManifest): boolean {
  return manifest.runtime === "native-dylib" || manifest.entry?.backend !== undefined;
}

/** 将未知 JSON 值收窄为打包所需的 Momo 清单字段。 */
export function readMomoPluginManifest(path: string): MomoPluginManifest {
  const value: unknown = JSON.parse(readFileSync(path, "utf-8"));
  if (
    typeof value !== "object"
    || value === null
    || !("pluginId" in value)
    || typeof value.pluginId !== "string"
    || value.pluginId.trim() === ""
    || !("version" in value)
    || typeof value.version !== "string"
    || value.version.trim() === ""
  ) {
    throw new Error(`invalid Momo plugin manifest: ${path}`);
  }
  return value as MomoPluginManifest;
}

/** 将未知 TOML 值收窄为 Mutsuki PluginToml 核心结构。 */
function parseMutsukiManifest(path: string): MutsukiPluginToml {
  const value: unknown = parse(readFileSync(path, "utf-8"));
  if (
    typeof value !== "object"
    || value === null
    || !("manifest" in value)
    || typeof value.manifest !== "object"
    || value.manifest === null
  ) {
    throw new Error(`invalid Mutsuki plugin manifest: ${path}`);
  }
  const manifest = value.manifest as Record<string, unknown>;
  if (
    typeof manifest.plugin_id !== "string"
    || manifest.plugin_id.trim() === ""
    || typeof manifest.version !== "string"
    || manifest.version.trim() === ""
    || typeof manifest.artifact !== "object"
    || manifest.artifact === null
  ) {
    throw new Error(`invalid Mutsuki plugin manifest: ${path}`);
  }
  return value as MutsukiPluginToml;
}

/** 解析严格的包内相对路径，并阻止跨目录或平台相关的绝对路径。 */
export function resolvePluginPackagePath(
  pluginDir: string,
  archivePath: string,
  label: string,
): string {
  if (
    archivePath.trim() === ""
    || archivePath.includes("\\")
    || archivePath.startsWith("/")
    || /^[A-Za-z]:/.test(archivePath)
    || isAbsolute(archivePath)
  ) {
    throw new Error(`${label} path must be a package-relative forward-slash path: ${archivePath}`);
  }
  const segments = archivePath.split("/");
  if (segments.some((segment) => segment === "" || segment === "." || segment === "..")) {
    throw new Error(`${label} path must not contain empty, dot, or parent segments: ${archivePath}`);
  }

  const packageRoot = resolve(pluginDir);
  const absolutePath = resolve(packageRoot, ...segments);
  const relativePath = relative(packageRoot, absolutePath);
  if (relativePath === "" || relativePath.startsWith("..") || isAbsolute(relativePath)) {
    throw new Error(`${label} path escapes the plugin package: ${archivePath}`);
  }
  return absolutePath;
}

/** 计算 Mutsuki 清单使用的规范小写 SHA-256。 */
function sha256File(path: string): string {
  return `sha256:${createHash("sha256").update(readFileSync(path)).digest("hex")}`;
}

/** 校验单个 artifact 的相对路径、文件存在性与内容哈希。 */
function validateArtifact(
  pluginDir: string,
  artifact: MutsukiPluginArtifact | MutsukiCompanionArtifact,
  label: string,
): void {
  if (typeof artifact.path !== "string" || typeof artifact.sha256 !== "string") {
    throw new Error(`${label} must declare string path and sha256 fields`);
  }
  const absolutePath = resolvePluginPackagePath(pluginDir, artifact.path, label);
  if (!existsSync(absolutePath) || !statSync(absolutePath).isFile()) {
    throw new Error(`${label} file is missing: ${artifact.path}`);
  }
  if (!/^sha256:[0-9a-f]{64}$/.test(artifact.sha256)) {
    throw new Error(`${label} sha256 must use sha256:<64 lowercase hex>: ${artifact.sha256}`);
  }
  const actual = sha256File(absolutePath);
  if (actual !== artifact.sha256) {
    throw new Error(`${label} sha256 mismatch: expected ${artifact.sha256}, got ${actual}`);
  }
}

/** 返回并校验 companion 列表，拒绝重复路径和无效可执行标记。 */
function companionArtifacts(artifact: MutsukiPluginArtifact): MutsukiCompanionArtifact[] {
  const companions = artifact.companion_artifacts ?? [];
  if (!Array.isArray(companions)) {
    throw new Error("manifest.artifact.companion_artifacts must be an array");
  }
  const paths = new Set<string>([artifact.path]);
  for (const companion of companions) {
    if (typeof companion !== "object" || companion === null) {
      throw new Error("manifest.artifact.companion_artifacts entries must be tables");
    }
    if (companion.executable !== undefined && typeof companion.executable !== "boolean") {
      throw new Error(`companion artifact executable must be boolean: ${companion.path}`);
    }
    if (companion.role !== undefined && typeof companion.role !== "string") {
      throw new Error(`companion artifact role must be a string: ${companion.path}`);
    }
    if (paths.has(companion.path)) {
      throw new Error(`duplicate companion artifact path: ${companion.path}`);
    }
    paths.add(companion.path);
  }
  return companions;
}

/** 确认需要本地编译的 companion 已由 Mutsuki 清单显式声明。 */
export function assertDeclaredCompanionArtifacts(pluginDir: string, paths: string[]): void {
  if (paths.length === 0) {
    return;
  }
  const pluginTomlPath = resolve(pluginDir, "plugin.toml");
  if (!existsSync(pluginTomlPath)) {
    throw new Error("companion artifact builds require plugin.toml declarations");
  }
  const pluginToml = parseMutsukiManifest(pluginTomlPath);
  const declared = new Set(
    companionArtifacts(pluginToml.manifest.artifact).map((companion) => companion.path),
  );
  const requested = new Set<string>();
  for (const path of paths) {
    resolvePluginPackagePath(pluginDir, path, "companion build artifact");
    if (requested.has(path)) {
      throw new Error(`duplicate companion build artifact path: ${path}`);
    }
    requested.add(path);
    if (!declared.has(path)) {
      throw new Error(`companion build artifact is not declared in plugin.toml: ${path}`);
    }
  }
}

/** 校验待归档目录的双清单一致性及所有执行产物。 */
export function validatePluginPackage(pluginDir: string): ValidatedPluginPackage {
  const momoManifestPath = resolve(pluginDir, "manifest.json");
  const pluginTomlPath = resolve(pluginDir, "plugin.toml");
  const manifest = readMomoPluginManifest(momoManifestPath);
  if (!existsSync(pluginTomlPath)) {
    if (isExecutableBackend(manifest)) {
      throw new Error(`executable backend plugin requires plugin.toml: ${manifest.pluginId}`);
    }
    return { manifest };
  }

  const pluginToml = parseMutsukiManifest(pluginTomlPath);
  const mutsukiManifest = pluginToml.manifest;
  if (mutsukiManifest.plugin_id !== manifest.pluginId) {
    throw new Error(
      `plugin id mismatch: manifest.json=${manifest.pluginId}, plugin.toml=${mutsukiManifest.plugin_id}`,
    );
  }
  if (mutsukiManifest.version !== manifest.version) {
    throw new Error(
      `plugin version mismatch: manifest.json=${manifest.version}, plugin.toml=${mutsukiManifest.version}`,
    );
  }

  validateArtifact(pluginDir, mutsukiManifest.artifact, "plugin artifact");
  for (const [index, companion] of companionArtifacts(mutsukiManifest.artifact).entries()) {
    validateArtifact(pluginDir, companion, `companion artifact ${index}`);
  }
  return { manifest, pluginToml };
}

/** 构建完成后只更新清单已声明文件的哈希，不推断或注入产物。 */
export function refreshPluginArtifactHashes(pluginDir: string): void {
  const pluginTomlPath = resolve(pluginDir, "plugin.toml");
  if (!existsSync(pluginTomlPath)) {
    return;
  }
  const pluginToml = parseMutsukiManifest(pluginTomlPath);
  const artifact = pluginToml.manifest.artifact;
  const declared = [artifact, ...companionArtifacts(artifact)];
  for (const [index, item] of declared.entries()) {
    const label = index === 0 ? "plugin artifact" : `companion artifact ${index - 1}`;
    const absolutePath = resolvePluginPackagePath(pluginDir, item.path, label);
    if (!existsSync(absolutePath) || !statSync(absolutePath).isFile()) {
      throw new Error(`${label} file is missing: ${item.path}`);
    }
    item.sha256 = sha256File(absolutePath);
  }
  writeFileSync(pluginTomlPath, stringify(pluginToml), "utf-8");
}
