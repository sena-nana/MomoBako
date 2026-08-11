// 将 Source manifest 的固定动作声明转换为宿主可执行的菜单动作。
import {
  addPlaylistItemsByPaths,
  callPlugin,
  createPlaylist,
  getFileBrowser,
  importEntries,
  listPlaylists,
  syncRepository,
} from "../services/repositoryApi";
import type {
  FileBrowserEntry,
  PluginManifest,
  RepositorySummary,
  SourceEntryActionContribution,
} from "../types/repository";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "../composables/workspace/tasks";
import type { EntryAction, EntryActionContext } from "./sdk";

let manifests: PluginManifest[] = [];

export function syncSourceEntryActionManifests(items: PluginManifest[]) {
  manifests = items.filter((manifest) => (
    manifest.enabled
    && !["disabled", "unavailable", "error"].includes(manifest.status ?? "ready")
    && Boolean(manifest.contributes?.source?.entryActions?.length)
  ));
}

function publicSourcePayload(entry: FileBrowserEntry) {
  return Object.fromEntries(Object.entries(entry.sourcePayload ?? {}).filter(([key]) => {
    const normalized = key.toLowerCase();
    return !normalized.includes("cookie")
      && !normalized.includes("password")
      && !normalized.includes("secret")
      && normalized !== "token";
  }));
}

function sourceNumber(entry: FileBrowserEntry, key: string) {
  const value = entry.sourcePayload?.[key] ?? entry.metadata?.[key] ?? entry.providerItemId;
  const parsed = typeof value === "number" ? value : Number.parseInt(String(value ?? ""), 10);
  return Number.isFinite(parsed) ? parsed : null;
}

function sourceString(entry: FileBrowserEntry, key: string) {
  const value = entry.sourcePayload?.[key] ?? entry.metadata?.[key];
  const normalized = String(value ?? "").trim();
  return normalized || null;
}

function actionMatches(action: SourceEntryActionContribution, entry: FileBrowserEntry) {
  if (action.scope !== entry.kind) return false;
  if (action.providerId && action.providerId !== entry.providerId) return false;
  if (action.entryKind && action.entryKind !== entry.sourcePayload?.entryKind) {
    const legacyMatch = action.entryKind === "track"
      ? entry.kind === "file" && sourceNumber(entry, "songId") !== null
      : action.entryKind === "playlist-folder"
        ? entry.kind === "directory" && sourceNumber(entry, "playlistId") !== null
        : false;
    if (!legacyMatch) return false;
  }
  return true;
}

function actionDisabled(context: EntryActionContext) {
  return context.repository?.authentication?.loginExpired === true
    || context.repository?.authentication?.loggedIn === false
    || (context.repository?.localCache?.required === true
      && context.repository.localCache.status !== "ready")
    || context.entry.sourcePayload?.loginExpired === true
    || context.entry.metadata?.loginExpired === true;
}

function actionMethod(manifest: PluginManifest, action: SourceEntryActionContribution) {
  if (action.method?.trim()) return action.method.trim();
  const media = manifest.contributes?.source?.media;
  if (action.operation === "download-entry") return media?.downloadEntryMethod ?? null;
  if (action.operation === "download-directory") return media?.downloadDirectoryMethod ?? null;
  if (action.operation === "refresh-playback") return media?.preparePlaybackMethod ?? null;
  if (action.operation === "clear-cache") return media?.clearCacheMethod ?? null;
  return null;
}

function downloadTargets(action: SourceEntryActionContribution) {
  if (action.targets?.length) return action.targets;
  return ["local-directory", "writable-repository"] as const;
}

function targetLabel(target: "local-directory" | "writable-repository") {
  return target === "local-directory" ? "本地" : "其他资源库";
}

async function chooseTarget(
  context: EntryActionContext,
  target: "local-directory" | "writable-repository",
) {
  if (target === "local-directory") {
    return context.openDialog({ kind: "directory", title: "选择下载目录" });
  }
  return context.openDialog({
    kind: "repository",
    title: "选择目标资源库",
    requireReady: true,
    requireWritable: true,
  });
}

function destinationPayload(target: string | RepositorySummary, parentPath?: string) {
  if (typeof target === "string") return { kind: "localFolder", path: target };
  return {
    kind: "repository",
    repoId: target.repoId,
    path: target.path,
    ...(parentPath ? { parentPath } : {}),
  };
}

function resultPaths(result: unknown) {
  if (!result || typeof result !== "object") return [];
  const value = result as { paths?: unknown; completed?: unknown };
  const direct = Array.isArray(value.paths) ? value.paths : [];
  const completed = Array.isArray(value.completed)
    ? value.completed.flatMap((item) => (
        item && typeof item === "object" && Array.isArray((item as { paths?: unknown }).paths)
          ? (item as { paths: unknown[] }).paths
          : []
      ))
    : [];
  return [...direct, ...completed]
    .map((path) => String(path ?? "").trim())
    .filter(Boolean);
}

async function importAndSyncTarget(
  target: string | RepositorySummary,
  paths: string[],
  parentPath = "",
) {
  if (typeof target === "string" || !paths.length) return;
  await importEntries({ repoId: target.repoId, parentPath, sourcePaths: [...new Set(paths)] });
  await syncRepository({ repoId: target.repoId });
}

async function loadDirectoryFiles(context: EntryActionContext) {
  const files: FileBrowserEntry[] = [];
  let offset = 0;
  do {
    const page = await getFileBrowser({
      repoId: context.repoId,
      directoryPath: context.entry.path,
      includeTree: false,
      offset,
      limit: 500,
    });
    files.push(...page.entries.filter((entry) => entry.kind === "file"));
    if (!page.hasMore || page.nextOffset == null) break;
    offset = page.nextOffset;
  } while (true);
  return files;
}

async function runDownloadAction(
  manifest: PluginManifest,
  action: SourceEntryActionContribution,
  context: EntryActionContext,
  targetKind: "local-directory" | "writable-repository",
) {
  const method = actionMethod(manifest, action);
  if (!method) throw new Error(`Source 动作缺少媒体方法：${action.actionId}`);
  const target = await chooseTarget(context, targetKind);
  if (!target) return;
  const isDirectory = action.operation === "download-directory";
  const playlistName = sourceString(context.entry, "playlistName") ?? context.entry.name;
  const progressId = startOperationProgress(action.label, isDirectory ? "准备下载目录" : `准备下载 ${context.entry.name}`, {
    initial: 10,
  });
  try {
    const tracks = isDirectory ? await loadDirectoryFiles(context) : [];
    updateOperationProgress(progressId, {
      detail: isDirectory ? `正在处理 ${tracks.length} 个文件` : `正在下载 ${context.entry.name}`,
      value: 36,
      indeterminate: true,
    });
    const response = await callPlugin<Record<string, unknown>>({
      pluginId: manifest.pluginId,
      method,
      repositoryId: context.repoId,
      payload: {
        songId: sourceNumber(context.entry, "songId"),
        playlistId: sourceNumber(context.entry, "playlistId"),
        playlistName,
        level: sourceString(context.entry, "level") ?? "standard",
        destination: destinationPayload(target),
        managedCacheRoot: context.repository?.localCache?.path ?? null,
        sourcePayload: publicSourcePayload(context.entry),
        tracks: tracks.map((entry) => ({
          songId: sourceNumber(entry, "songId"),
          songName: sourceString(entry, "songName") ?? entry.name,
          sourcePayload: publicSourcePayload(entry),
        })).filter((entry) => entry.songId !== null),
      },
    });
    const failed = Array.isArray(response.payload.failed) ? response.payload.failed : [];
    if (failed.length) {
      console.error("source directory download partially failed", {
        pluginId: manifest.pluginId,
        actionId: action.actionId,
        repoId: context.repoId,
        failedCount: failed.length,
      });
    }
    const parentPath = isDirectory && typeof target !== "string" ? playlistName : "";
    await importAndSyncTarget(target, resultPaths(response.payload), parentPath);
    await context.refreshRepo();
    updateOperationProgress(progressId, {
      detail: failed.length ? `下载完成，${failed.length} 项失败` : "下载完成",
      value: 100,
      indeterminate: false,
    });
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    const reason = cause instanceof Error ? cause.message : String(cause);
    console.error("source entry download action failed", {
      pluginId: manifest.pluginId,
      actionId: action.actionId,
      repoId: context.repoId,
      reason,
    });
    throw cause;
  }
}

async function runClearCacheAction(
  manifest: PluginManifest,
  action: SourceEntryActionContribution,
  context: EntryActionContext,
) {
  const method = actionMethod(manifest, action);
  if (!method) throw new Error(`Source 动作缺少缓存方法：${action.actionId}`);
  await callPlugin({
    pluginId: manifest.pluginId,
    method,
    repositoryId: context.repoId,
    payload: {
      songId: sourceNumber(context.entry, "songId"),
      level: sourceString(context.entry, "level") ?? "standard",
      managedCacheRoot: context.repository?.localCache?.path ?? null,
      sourcePayload: publicSourcePayload(context.entry),
    },
  });
  await context.refreshRepo();
}

async function runRefreshPlaybackAction(
  manifest: PluginManifest,
  action: SourceEntryActionContribution,
  context: EntryActionContext,
) {
  const method = actionMethod(manifest, action);
  if (!method) throw new Error(`Source 动作缺少播放源方法：${action.actionId}`);
  await callPlugin({
    pluginId: manifest.pluginId,
    method,
    repositoryId: context.repoId,
    payload: {
      songId: sourceNumber(context.entry, "songId"),
      level: sourceString(context.entry, "level") ?? "standard",
      forceRefresh: true,
      managedCacheRoot: context.repository?.localCache?.path ?? null,
    },
  });
  await context.refreshRepo();
}

async function runPlaylistAction(action: SourceEntryActionContribution, context: EntryActionContext) {
  const providerId = context.entry.providerId ?? "source";
  const providerItemId = sourceNumber(context.entry, "playlistId") ?? context.entry.providerItemId;
  if (!providerItemId) throw new Error("来源目录缺少播放列表标识");
  const playlistId = `${providerId}-${providerItemId}`;
  const name = sourceString(context.entry, "playlistName") ?? context.entry.name;
  const existing = await listPlaylists(context.repoId);
  if (!existing.some((playlist) => playlist.playlistId === playlistId)) {
    await createPlaylist({
      repoId: context.repoId,
      playlistId,
      name,
      playerTypeId: action.playerTypeId ?? "momobako.playlist.audio-sequence",
    });
  }
  await addPlaylistItemsByPaths({
    repoId: context.repoId,
    playlistId,
    paths: [context.entry.path],
  });
  await context.refreshRepo();
}

function actionsForDeclaration(
  manifest: PluginManifest,
  action: SourceEntryActionContribution,
  context: EntryActionContext,
): EntryAction[] {
  const disabled = actionDisabled(context);
  if (action.operation === "download-entry" || action.operation === "download-directory") {
    const targets = downloadTargets(action);
    return targets.map((target) => ({
      id: `${manifest.pluginId}:${action.actionId}:${target}`,
      label: targets.length > 1 ? `${action.label}到${targetLabel(target)}` : action.label,
      disabled,
      confirmLabel: disabled ? "来源认证或本地缓存不可用" : undefined,
      onSelect: () => runDownloadAction(manifest, action, context, target),
    }));
  }
  if (action.operation === "clear-cache") {
    return [{
      id: `${manifest.pluginId}:${action.actionId}`,
      label: action.label,
      disabled,
      confirmLabel: disabled ? "来源认证或本地缓存不可用" : undefined,
      onSelect: () => runClearCacheAction(manifest, action, context),
    }];
  }
  if (action.operation === "refresh-playback") {
    return [{
      id: `${manifest.pluginId}:${action.actionId}`,
      label: action.label,
      disabled,
      confirmLabel: disabled ? "来源认证或本地缓存不可用" : undefined,
      onSelect: () => runRefreshPlaybackAction(manifest, action, context),
    }];
  }
  return [{
    id: `${manifest.pluginId}:${action.actionId}`,
    label: action.label,
    disabled,
    confirmLabel: disabled ? "来源认证或本地缓存不可用" : undefined,
    onSelect: () => runPlaylistAction(action, context),
  }];
}

export function getManifestSourceEntryActions(context: EntryActionContext): EntryAction[] {
  return manifests.flatMap((manifest) => (
    manifest.contributes?.source?.entryActions
      ?.filter((action) => actionMatches(action, context.entry))
      .flatMap((action) => actionsForDeclaration(manifest, action, context))
    ?? []
  ));
}
