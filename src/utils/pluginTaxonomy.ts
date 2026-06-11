import type { PluginCategory, PluginManifest } from "../types/repository";

const legacySourceKinds = new Set(["filesystem", "webdav", "cloud"]);

export function pluginCategoryForKind(kind: string): PluginCategory {
  if (legacySourceKinds.has(kind)) return "source";
  if (kind === "preview") return "preview";
  if (kind === "library-kind") return "library-kind";
  if (kind === "parser") return "parser";
  return "service";
}

export function pluginCategory(plugin: Pick<PluginManifest, "category" | "kind">): PluginCategory | string {
  return plugin.category || pluginCategoryForKind(plugin.kind);
}

export function isSourcePlugin(plugin: Pick<PluginManifest, "category" | "kind">) {
  return pluginCategory(plugin) === "source";
}

export function pluginCategoryLabel(category: PluginCategory | string | undefined) {
  if (category === "source") return "库来源";
  if (category === "library-kind") return "库类型";
  if (category === "parser") return "文件解析";
  if (category === "preview") return "预览渲染";
  if (category === "service") return "基础服务";
  return "未分类";
}
