// 用 NanaUI 的 nana-packager 打 MomoBako 的 Windows 发布包：安装版布局和便携版各打一份、各自校验，再压成 zip。
//
// 输入：dist 配置编好的 momobako-nana（`yarn nana:build`）和打包好的插件（`yarn plugins:build && yarn plugins:package`）。
// 输出（默认在 target/nana-package）：
//   installer/app/MomoBako/          安装版布局，数据写进用户目录
//   portable/app/MomoBako/           便携版布局，数据写进 <根>/data
//   MomoBako_<版本>_x64.zip          安装版布局
//   MomoBako_<版本>_x64_portable.zip 便携版
// 便携版的 zip 再解到 check/ 下跑 `validate --run --tamper-suite`：自检只在包里的 data/ 落日志，不碰本机数据。
// nana-packager 按 src-nana/Cargo.toml 里固定的 NanaUI 修订装进 target/nana-packager，也可以用 --packager 指定。
import { spawnSync, type SpawnSyncOptions } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  archiveName,
  readMomoplugEnvelope,
  readTemplateInfo,
  renderPackageConfig,
  type BundledPlugin,
  type PackageTemplateInfo,
  type PackageVariant,
} from "./nana-package-config.ts";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const templatePath = join(repoRoot, "src-nana", "nana-package.toml");
const nanaManifestPath = join(repoRoot, "src-nana", "Cargo.toml");
const pluginPackagesDir = join(repoRoot, "External", "Plugins", ".packages");
const NANAUI_REPOSITORY = "https://github.com/sena-nana/NanaUI.git";
const VARIANTS: readonly PackageVariant[] = ["installer", "portable"];

interface Options {
  exe: string;
  out: string;
  packager?: string;
  buildId?: string;
  expectTag?: string;
}

function log(message: string): void {
  console.log(`[nana-package] ${message}`);
}

/** 解析 `--name value` 形式的参数。 */
function parseOptions(argv: readonly string[]): Options {
  const values = new Map<string, string>();
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!["--exe", "--out", "--packager", "--build-id", "--expect-tag"].includes(flag)) {
      throw new Error(`不认识的参数 ${flag}`);
    }
    if (value === undefined || value.startsWith("--")) throw new Error(`${flag} 需要一个值`);
    values.set(flag, value);
    index += 1;
  }
  // cargo 在仓库根运行，相对的 CARGO_TARGET_DIR 也以仓库根为准。
  const targetDir = resolve(repoRoot, process.env.CARGO_TARGET_DIR?.trim() || "target");
  return {
    exe: resolve(values.get("--exe") ?? join(targetDir, "dist", "momobako-nana.exe")),
    out: resolve(values.get("--out") ?? join(repoRoot, "target", "nana-package")),
    packager: values.get("--packager") ?? (process.env.NANA_PACKAGER?.trim() || undefined),
    buildId: values.get("--build-id"),
    expectTag: values.get("--expect-tag"),
  };
}

/** 跑一个命令，失败时抛错；`capture` 时交回标准输出。 */
function run(command: string, args: readonly string[], options: SpawnSyncOptions & { capture?: boolean } = {}): string {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: options.capture ? ["ignore", "pipe", "inherit"] : "inherit",
    ...options,
  });
  if (result.error) throw new Error(`${command} 起不来：${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} 失败，退出码 ${result.status}`);
  return typeof result.stdout === "string" ? result.stdout : "";
}

function hostTriple(): string {
  const line = run("rustc", ["-vV"], { capture: true }).split(/\r?\n/).find((value) => value.startsWith("host: "));
  if (!line) throw new Error("rustc -vV 没有报出 host 三元组");
  return line.slice("host: ".length).trim();
}

/** src-nana/Cargo.toml 里固定的 NanaUI 修订，nana-packager 也装这一版。 */
function pinnedNanaRevision(): string {
  const manifest = readFileSync(nanaManifestPath, "utf8");
  const revision = manifest.match(/^nana-ui\s*=\s*\{[^}]*\brev\s*=\s*"([0-9a-f]{40})"/m)?.[1];
  if (!revision) throw new Error("src-nana/Cargo.toml 里找不到 nana-ui 的固定修订");
  return revision;
}

/** 用 --packager 给的 nana-packager，否则按固定修订装进 target/nana-packager（装过同一版时 cargo 会跳过）。 */
function ensurePackager(options: Options): string {
  if (options.packager) return resolve(options.packager);
  const root = join(repoRoot, "target", "nana-packager");
  const revision = pinnedNanaRevision();
  log(`安装 nana-packager（NanaUI ${revision}）到 ${root}`);
  run("cargo", ["install", "--git", NANAUI_REPOSITORY, "--rev", revision, "--locked", "nana-packager", "--root", root]);
  return join(root, "bin", process.platform === "win32" ? "nana-packager.exe" : "nana-packager");
}

/** 插件打包输出里的随附插件，按文件名排序；三元组不对、格式不对或同一插件出现两次时报错。 */
function collectPlugins(targetTriple: string): BundledPlugin[] {
  if (!existsSync(pluginPackagesDir)) {
    throw new Error(`没有插件打包输出 ${pluginPackagesDir}，先运行 yarn plugins:build 和 yarn plugins:package`);
  }
  const files = readdirSync(pluginPackagesDir).filter((name) => name.endsWith(".momoplug")).sort();
  if (files.length === 0) throw new Error(`${pluginPackagesDir} 里没有 .momoplug`);
  const seen = new Set<string>();
  return files.map((file) => {
    const source = join(pluginPackagesDir, file);
    const envelope = readMomoplugEnvelope(source);
    if (envelope.targetTriple !== "any" && envelope.targetTriple !== targetTriple) {
      throw new Error(`${file} 是给 ${envelope.targetTriple} 编的，这次打的是 ${targetTriple}`);
    }
    if (seen.has(envelope.pluginId)) throw new Error(`插件 ${envelope.pluginId} 有不止一个包`);
    seen.add(envelope.pluginId);
    return { name: envelope.pluginId, source, version: envelope.version };
  });
}

/** 默认的构建号是当前提交的短哈希；不在 git 工作树里时不写构建号。 */
function defaultBuildId(): string | undefined {
  try {
    return run("git", ["rev-parse", "--short=12", "HEAD"], { capture: true }).trim() || undefined;
  } catch (error) {
    log(`取不到当前提交，manifest 里不写构建号：${error instanceof Error ? error.message : String(error)}`);
    return undefined;
  }
}

/** 用 Windows 自带的 tar（bsdtar）压 zip、解 zip；zip 里的顶层目录是应用根目录名。 */
function windowsTar(): string {
  if (process.platform !== "win32") throw new Error("发布包目前只打 Windows 版，需要在 Windows 上运行");
  return join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
}

function zipDirectory(parent: string, name: string, archive: string): void {
  rmSync(archive, { force: true });
  run(windowsTar(), ["-a", "-c", "-f", archive, "-C", parent, name]);
}

function unzip(archive: string, destination: string): void {
  rmSync(destination, { recursive: true, force: true });
  mkdirSync(destination, { recursive: true });
  run(windowsTar(), ["-x", "-f", archive, "-C", destination]);
}

/** 打一种发行方式：生成配置、打包、静态校验、压 zip。交回 zip 路径。 */
function packageVariant(
  packager: string,
  info: PackageTemplateInfo,
  template: string,
  plugins: readonly BundledPlugin[],
  variant: PackageVariant,
  context: { options: Options; targetTriple: string; buildId?: string },
): string {
  const { options, targetTriple, buildId } = context;
  const configDir = join(options.out, "config");
  mkdirSync(configDir, { recursive: true });
  const configPath = join(configDir, `${variant}.toml`);
  writeFileSync(configPath, renderPackageConfig(template, dirname(templatePath), variant, plugins), "utf8");
  const variantOut = join(options.out, variant);
  log(`打包 ${variant}：${plugins.length} 个随附插件 → ${variantOut}`);
  run(packager, [
    "package",
    "--config", configPath,
    "--exe", options.exe,
    "--out", variantOut,
    "--target", targetTriple,
    "--profile", "dist",
    ...(buildId ? ["--build-id", buildId] : []),
  ]);
  const appParent = join(variantOut, "app");
  log(`校验 ${variant}`);
  run(packager, ["validate", join(appParent, info.fileName)]);
  const archive = join(options.out, archiveName(info, targetTriple, variant));
  zipDirectory(appParent, info.fileName, archive);
  log(`${variant} → ${archive}（${(statSync(archive).size / 1024 / 1024).toFixed(1)} MiB）`);
  return archive;
}

function main(): void {
  const options = parseOptions(process.argv.slice(2));
  const template = readFileSync(templatePath, "utf8");
  const info = readTemplateInfo(template);
  if (info.backend !== "installer") throw new Error("src-nana/nana-package.toml 的 backend 应当是 installer，便携版由脚本换");
  if (options.expectTag && options.expectTag !== `v${info.version}`) {
    throw new Error(`tag ${options.expectTag} 和应用版本 ${info.version} 对不上`);
  }
  if (!existsSync(options.exe)) throw new Error(`没有可执行文件 ${options.exe}，先运行 yarn nana:build`);
  const targetTriple = hostTriple();
  const plugins = collectPlugins(targetTriple);
  const packager = ensurePackager(options);
  const buildId = options.buildId ?? defaultBuildId();
  const context = { options, targetTriple, buildId };
  mkdirSync(options.out, { recursive: true });
  for (const stale of readdirSync(options.out).filter((name) => name.endsWith(".zip"))) {
    rmSync(join(options.out, stale), { force: true });
  }
  const archives = VARIANTS.map((variant) => packageVariant(packager, info, template, plugins, variant, context));

  const checkDir = join(options.out, "check");
  const portableArchive = archives[VARIANTS.indexOf("portable")];
  unzip(portableArchive, checkDir);
  log(`在解开的便携版上跑自检：${checkDir}`);
  run(packager, ["validate", join(checkDir, info.fileName), "--run", "--tamper-suite"]);
  log(`完成：${archives.join("、")}`);
}

try {
  main();
} catch (error) {
  console.error(`[nana-package] ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
