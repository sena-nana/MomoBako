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
    frontend?: {
      module?: string;
    };
  };
}

export const PLUGIN_PACKAGE_FORMAT_VERSION = 2;
export const PLUGIN_PACKAGE_MANIFEST = "momobako.package.json";

export type PluginDeployment = "manifest" | "frontend" | "abi" | "process";

export interface PluginPackageArtifact {
  role: string;
  path: string;
  sha256: string;
  executable?: boolean;
}

export interface PluginPackageEnvelope {
  formatVersion: 2;
  pluginId: string;
  version: string;
  targetTriple: string;
  deployment: PluginDeployment;
  productManifest: "manifest.json";
  runtimeManifest?: "plugin.toml";
  artifacts: PluginPackageArtifact[];
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

interface MutsukiHandlerBinding {
  binding_id: string;
  plugin_id: string;
  protocol_id: string;
  target_runner_hint?: string;
}

interface MutsukiPluginManifest {
  plugin_id: string;
  version: string;
  artifact: MutsukiPluginArtifact;
  provides?: {
    handler_bindings?: MutsukiHandlerBinding[];
  };
}

interface MutsukiPluginToml extends MutsukiPluginManifest {
  [key: string]: unknown;
}

export interface ValidatedPluginPackage {
  manifest: MomoPluginManifest;
  pluginToml?: MutsukiPluginToml;
  packageEnvelope: PluginPackageEnvelope;
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
  ) {
    throw new Error(`invalid Mutsuki plugin manifest: ${path}`);
  }
  const manifest = value as Record<string, unknown>;
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
export function sha256File(path: string): string {
  return `sha256:${createHash("sha256").update(readFileSync(path)).digest("hex")}`;
}

/** 根据产品清单选择宿主部署模型，避免安装来源伪装成执行信任级别。 */
function deploymentForManifest(manifest: MomoPluginManifest): PluginDeployment {
  if (manifest.runtime === "native-dylib") return "abi";
  if (manifest.runtime === "process") return "process";
  if (manifest.runtime === "vue-module" || manifest.entry?.frontend?.module) return "frontend";
  return "manifest";
}

/** 读取并收窄 v2 包信封。 */
function readPluginPackageEnvelope(path: string): PluginPackageEnvelope {
  const value: unknown = JSON.parse(readFileSync(path, "utf-8"));
  if (typeof value !== "object" || value === null) {
    throw new Error(`invalid plugin package envelope: ${path}`);
  }
  const envelope = value as Partial<PluginPackageEnvelope>;
  if (
    envelope.formatVersion !== PLUGIN_PACKAGE_FORMAT_VERSION
    || typeof envelope.pluginId !== "string"
    || typeof envelope.version !== "string"
    || typeof envelope.targetTriple !== "string"
    || !["manifest", "frontend", "abi", "process"].includes(envelope.deployment ?? "")
    || envelope.productManifest !== "manifest.json"
    || !Array.isArray(envelope.artifacts)
  ) {
    throw new Error(`unsupported or invalid plugin package envelope: ${path}`);
  }
  return envelope as PluginPackageEnvelope;
}

/** 从已构建目录生成宿主可验证的 v2 包信封。 */
export function writePluginPackageEnvelope(pluginDir: string, targetTriple: string): void {
  const manifest = readMomoPluginManifest(resolve(pluginDir, "manifest.json"));
  const pluginTomlPath = resolve(pluginDir, "plugin.toml");
  const pluginToml = existsSync(pluginTomlPath) ? parseMutsukiManifest(pluginTomlPath) : undefined;
  const deployment = deploymentForManifest(manifest);
  const artifacts: PluginPackageArtifact[] = [];

  if (pluginToml) {
    artifacts.push({
      role: deployment === "process" ? "runner" : "plugin",
      path: pluginToml.artifact.path,
      sha256: pluginToml.artifact.sha256,
      executable: deployment === "process" || undefined,
    });
    for (const companion of companionArtifacts(pluginToml.artifact)) {
      artifacts.push({
        role: companion.role ?? "companion",
        path: companion.path,
        sha256: companion.sha256,
        executable: companion.executable,
      });
    }
  } else if (manifest.entry?.frontend?.module) {
    const path = manifest.entry.frontend.module.replace(/\\/g, "/");
    const absolutePath = resolvePluginPackagePath(pluginDir, path, "frontend artifact");
    artifacts.push({ role: "frontend", path, sha256: sha256File(absolutePath) });
  }

  const envelope: PluginPackageEnvelope = {
    formatVersion: PLUGIN_PACKAGE_FORMAT_VERSION,
    pluginId: manifest.pluginId,
    version: manifest.version,
    targetTriple: deployment === "abi" || deployment === "process" ? targetTriple : "any",
    deployment,
    productManifest: "manifest.json",
    runtimeManifest: pluginToml ? "plugin.toml" : undefined,
    artifacts,
  };
  writeFileSync(
    resolve(pluginDir, PLUGIN_PACKAGE_MANIFEST),
    `${JSON.stringify(envelope, null, 2)}\n`,
    "utf-8",
  );
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

/** 强制 binding ID 按插件隔离，使同一 protocol 可由多个仓库后端安全提供。 */
function validateHandlerBindings(manifest: MutsukiPluginManifest): void {
  const bindings = manifest.provides?.handler_bindings;
  if (!Array.isArray(bindings) || bindings.length === 0) {
    throw new Error(`executable plugin must declare handler bindings: ${manifest.plugin_id}`);
  }
  const bindingIds = new Set<string>();
  for (const binding of bindings) {
    if (
      typeof binding !== "object"
      || binding === null
      || typeof binding.binding_id !== "string"
      || typeof binding.plugin_id !== "string"
      || typeof binding.protocol_id !== "string"
    ) {
      throw new Error(`invalid handler binding: ${manifest.plugin_id}`);
    }
    if (binding.plugin_id !== manifest.plugin_id) {
      throw new Error(
        `handler binding plugin mismatch: expected ${manifest.plugin_id}, got ${binding.plugin_id}`,
      );
    }
    const expectedBindingId =
      `binding:${manifest.plugin_id}:${binding.protocol_id}`;
    if (binding.binding_id !== expectedBindingId) {
      throw new Error(
        `handler binding id must be plugin-scoped: expected ${expectedBindingId}, got ${binding.binding_id}`,
      );
    }
    if (bindingIds.has(binding.binding_id)) {
      throw new Error(`duplicate handler binding id: ${binding.binding_id}`);
    }
    bindingIds.add(binding.binding_id);
  }
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
    companionArtifacts(pluginToml.artifact).map((companion) => companion.path),
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
  const packageEnvelope = readPluginPackageEnvelope(resolve(pluginDir, PLUGIN_PACKAGE_MANIFEST));
  if (packageEnvelope.pluginId !== manifest.pluginId || packageEnvelope.version !== manifest.version) {
    throw new Error(`package envelope identity mismatch: ${manifest.pluginId}@${manifest.version}`);
  }
  if (packageEnvelope.deployment !== deploymentForManifest(manifest)) {
    throw new Error(`package envelope deployment mismatch: ${manifest.pluginId}`);
  }
  const artifactPaths = new Set<string>();
  for (const [index, artifact] of packageEnvelope.artifacts.entries()) {
    if (typeof artifact.role !== "string" || artifact.role.trim() === "") {
      throw new Error(`package artifact ${index} requires a role`);
    }
    if (artifactPaths.has(artifact.path)) {
      throw new Error(`duplicate package artifact path: ${artifact.path}`);
    }
    artifactPaths.add(artifact.path);
    validateArtifact(pluginDir, artifact, `package artifact ${index}`);
  }
  if (!existsSync(pluginTomlPath)) {
    if (isExecutableBackend(manifest)) {
      throw new Error(`executable backend plugin requires plugin.toml: ${manifest.pluginId}`);
    }
    return { manifest, packageEnvelope };
  }

  const pluginToml = parseMutsukiManifest(pluginTomlPath);
  const mutsukiManifest = pluginToml;
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

  validateHandlerBindings(mutsukiManifest);
  validateArtifact(pluginDir, mutsukiManifest.artifact, "plugin artifact");
  for (const [index, companion] of companionArtifacts(mutsukiManifest.artifact).entries()) {
    validateArtifact(pluginDir, companion, `companion artifact ${index}`);
  }
  const runtimeArtifactPaths = new Set([
    pluginToml.artifact.path,
    ...companionArtifacts(pluginToml.artifact).map((artifact) => artifact.path),
  ]);
  if (
    runtimeArtifactPaths.size !== artifactPaths.size
    || [...runtimeArtifactPaths].some((path) => !artifactPaths.has(path))
  ) {
    throw new Error(`package envelope artifacts differ from plugin.toml: ${manifest.pluginId}`);
  }
  return { manifest, pluginToml, packageEnvelope };
}

/** 构建完成后只更新清单已声明文件的哈希，不推断或注入产物。 */
export function refreshPluginArtifactHashes(pluginDir: string): void {
  const pluginTomlPath = resolve(pluginDir, "plugin.toml");
  if (!existsSync(pluginTomlPath)) {
    return;
  }
  const pluginToml = parseMutsukiManifest(pluginTomlPath);
  const artifact = pluginToml.artifact;
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
