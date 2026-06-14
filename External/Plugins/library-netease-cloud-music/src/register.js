const SOURCE_PLUGIN_ID = "momobako.source.netease-cloud-music";
const DOWNLOADER_PLUGIN_ID = "momobako.service.downloader";
const LOCAL_FILESYSTEM_PLUGIN_ID = "momobako.local-filesystem";
let cachedLoginStatus = {
  loggedIn: false,
  loginExpired: false,
};

function isNeteaseEntry(entry) {
  if (!entry) return false;
  if (entry.providerId === "netease-cloud-music") return true;
  return entry.sourcePayload?.provider === "netease-cloud-music"
    || entry.metadata?.provider === "netease-cloud-music";
}

function isPlaylistFolder(entry) {
  return entry.kind === "directory"
    && isNeteaseEntry(entry)
    && entry.sourcePayload?.entryKind === "playlist-folder";
}

function isTrackEntry(entry) {
  return entry.kind === "file" && isNeteaseEntry(entry);
}

function songIdFromEntry(entry) {
  const raw = entry.sourcePayload?.songId ?? entry.metadata?.songId ?? entry.providerItemId;
  const number = typeof raw === "number" ? raw : Number.parseInt(String(raw ?? ""), 10);
  return Number.isFinite(number) ? number : null;
}

function playlistIdFromEntry(entry) {
  const raw = entry.sourcePayload?.playlistId ?? entry.metadata?.playlistId;
  const number = typeof raw === "number" ? raw : Number.parseInt(String(raw ?? ""), 10);
  return Number.isFinite(number) ? number : null;
}

function loginExpired(entry) {
  return entry.sourcePayload?.loginExpired === true
    || entry.metadata?.loginExpired === true;
}

function sourceConfigFromEntry(entry) {
  const payload = entry?.sourcePayload ?? entry?.metadata ?? {};
  return {
    cookie: payload.accountCookie,
    accountId: payload.accountId,
  };
}

async function callSource(ctx, method, payload = {}) {
  const response = await ctx.callPlugin({
    pluginId: SOURCE_PLUGIN_ID,
    method,
    payload,
  });
  return response.payload;
}

async function callDownloader(ctx, method, payload = {}) {
  const response = await ctx.callPlugin({
    pluginId: DOWNLOADER_PLUGIN_ID,
    method,
    payload,
  });
  return response.payload;
}

function rememberLoginStatus(status) {
  cachedLoginStatus = {
    loggedIn: status?.loggedIn === true,
    loginExpired: status?.loginExpired === true || status?.loggedIn === false,
  };
  return cachedLoginStatus;
}

async function refreshLoginStatus(ctx, config = null) {
  const status = await callSource(ctx, "auth.getLoginStatus", config ? { config } : {});
  rememberLoginStatus(status);
  return status;
}

async function ensureLoginReady(ctx, entry) {
  const status = await refreshLoginStatus(ctx, sourceConfigFromEntry(entry));
  if (status?.loginExpired || !status?.loggedIn) {
    throw new Error("登录已失效，请重新登录");
  }
  return status;
}

function buildDownloadDestinationPayload(target) {
  if (typeof target === "string") {
    return {
      kind: "localFolder",
      path: target,
    };
  }
  return {
    kind: "repository",
    repoId: target.repoId,
    path: target.path,
  };
}

function sanitizeFileName(value) {
  const normalized = String(value ?? "")
    .replace(/[<>:"/\\|?*]/g, "_")
    .trim();
  return normalized || "untitled";
}

function joinRelativePath(parent, name) {
  const normalizedParent = String(parent ?? "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  const normalizedName = String(name ?? "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
  if (!normalizedParent) return normalizedName;
  if (!normalizedName) return normalizedParent;
  return `${normalizedParent}/${normalizedName}`;
}

async function syncDownloadedRepository(ctx, repository, context) {
  await ctx.invokeCommand("sync_repository", {
    request: {
      repoId: repository.repoId,
    },
  });
  await context.refreshRepo();
}

async function importDownloadedPaths(ctx, repository, paths = [], parentPath = "") {
  const sourcePaths = Array.from(new Set(
    (paths ?? [])
      .map((value) => String(value ?? "").trim())
      .filter(Boolean),
  ));
  if (!sourcePaths.length) return;
  await ctx.invokeCommand("import_entries", {
    request: {
      repoId: repository.repoId,
      parentPath,
      sourcePaths,
    },
  });
}

function createSettingsPage(ctx) {
  const { h } = ctx.vue;
  return {
    name: "NeteaseCloudMusicSettingsPage",
    props: ["manifest"],
    setup() {
      return () => h("section", { class: "file-metadata-card__source file-metadata-card__library" }, [
        h("div", { class: "file-metadata-card__source-head" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "Netease Cloud Music"),
            h("strong", "使用说明"),
          ]),
        ]),
        h("div", { class: "file-metadata-card__source-grid" }, [
          h("div", { class: "asset-meta__row file-metadata-card__source-row" }, [
            h("span", "登录入口"),
            h("span", { class: "asset-meta__value" }, "添加资源库时扫码登录"),
          ]),
          h("div", { class: "asset-meta__row file-metadata-card__source-row" }, [
            h("span", "多账号"),
            h("span", { class: "asset-meta__value" }, "每个网易云账号一个资源库"),
          ]),
          h("div", { class: "asset-meta__row file-metadata-card__source-row" }, [
            h("span", "重新登录"),
            h("span", { class: "asset-meta__value" }, "在对应资源库中操作，不保存在插件全局配置"),
          ]),
        ]),
        h("p", { class: "repository-add-popover__note" }, "全局插件设置仅保留 API Base URL 等公共配置。账号登录、失效续登和资源库创建都在资源库创建流程中完成。"),
      ]);
    },
  };
}

async function chooseLocalFolder(context) {
  return context.openDialog({
    kind: "directory",
    title: "选择下载目录",
  });
}

async function chooseRepository(context) {
  return context.openDialog({
    kind: "repository",
    title: "选择目标资源库",
    requireReady: true,
    requireWritable: true,
    backendPluginIds: [LOCAL_FILESYSTEM_PLUGIN_ID],
  });
}

async function loadPlaylistTrackEntries(ctx, repoId, folderPath) {
  const snapshot = await ctx.invokeCommand("get_file_browser", {
    request: {
      repoId,
      directoryPath: folderPath,
      includeTree: false,
    },
  });
  return (snapshot.entries ?? []).filter((entry) => entry.kind === "file" && songIdFromEntry(entry) != null);
}

function playlistTrackDestination(target, playlistName) {
  const folderName = sanitizeFileName(playlistName);
  if (typeof target === "string") {
    return {
      kind: "localFolder",
      path: `${target.replace(/[\\/]+$/, "")}/${folderName}`,
    };
  }
  return {
    kind: "repository",
    repoId: target.repoId,
    path: target.path,
    parentPath: joinRelativePath(target.parentPath, folderName),
  };
}

async function downloadTrackWithProgress(ctx, entry, target, label) {
  const progressId = ctx.startOperationProgress(label, "准备下载歌曲", { initial: 12 });
  try {
    ctx.updateOperationProgress(progressId, {
      detail: `下载 ${entry.name}`,
      value: 48,
      indeterminate: true,
    });
    const result = await callDownloader(ctx, "downloader.downloadTrackPackage", {
      songId: songIdFromEntry(entry),
      level: entry.sourcePayload?.level ?? "standard",
      destination: buildDownloadDestinationPayload(target),
      sourcePayload: entry.sourcePayload ?? {},
    });
    if (typeof target !== "string") {
      await importDownloadedPaths(ctx, target, result?.paths, "");
    }
    ctx.updateOperationProgress(progressId, {
      detail: `已完成 ${entry.name}`,
      value: 96,
      indeterminate: false,
    });
    ctx.finishOperationProgress(progressId);
  } catch (error) {
    ctx.cancelOperationProgress(progressId);
    throw error;
  }
}

async function downloadPlaylistWithProgress(ctx, context, target) {
  const playlistName = context.entry.sourcePayload?.playlistName ?? context.entry.name;
  const tracks = await loadPlaylistTrackEntries(ctx, context.repoId, context.entry.path);
  const total = tracks.length;
  if (!total) return { completed: 0, failed: [] };
  const progressId = ctx.startOperationProgress("下载歌单", `准备下载 ${total} 首歌曲`, { initial: 10 });
  const destination = playlistTrackDestination(target, playlistName);
  try {
    const result = await ctx.downloadPlaylistWithProgress({
      playlistId: playlistIdFromEntry(context.entry),
      playlistName,
      tracks: tracks
        .map((track) => {
          const songId = songIdFromEntry(track);
          if (!Number.isFinite(songId)) return null;
          return {
            songId,
            songName: track.sourcePayload?.songName ?? track.name ?? null,
            sourcePayload: track.sourcePayload ?? {},
          };
        })
        .filter(Boolean),
      destination,
      sourcePayload: context.entry.sourcePayload ?? {},
      level: context.entry.sourcePayload?.level ?? "standard",
    }, (event) => {
      if (event.phase === "start") {
        ctx.updateOperationProgress(progressId, {
          detail: `准备下载 ${event.total} 首歌曲`,
          value: 12,
          indeterminate: false,
        });
        return;
      }
      if (event.phase === "track") {
        const processed = Math.max(0, (event.completed ?? 0) + (event.failed ?? 0));
        ctx.updateOperationProgress(progressId, {
          detail: event.currentSongName
            ? `下载 ${processed}/${event.total}: ${event.currentSongName}`
            : `已处理 ${processed}/${event.total} 首歌曲`,
          value: Math.max(16, Math.round(16 + processed / Math.max(1, event.total) * 72)),
          indeterminate: false,
        });
        return;
      }
      if (event.phase === "complete") {
        ctx.updateOperationProgress(progressId, {
          detail: event.failed
            ? `已完成 ${event.completed}/${event.total} 首，失败 ${event.failed} 首`
            : `已完成 ${event.completed} 首歌曲`,
          value: 96,
          indeterminate: false,
        });
      }
    });
    if (typeof target !== "string") {
      const completed = Array.isArray(result?.completed) ? result.completed : [];
      const importPaths = completed.flatMap((item) => (
        Array.isArray(item?.paths)
          ? item.paths.map((value) => String(value ?? "").trim()).filter(Boolean)
          : []
      ));
      await importDownloadedPaths(ctx, target, importPaths, destination.parentPath ?? "");
    }
    ctx.finishOperationProgress(progressId);
    return result;
  } catch (error) {
    ctx.cancelOperationProgress(progressId);
    throw error;
  }
}

function register(ctx) {
  ctx.registerSettingsPage({
    component: createSettingsPage(ctx),
  });

  ctx.registerLibraryExtension({
    libraryKind: "netease-cloud-music",
    label: "网易云音乐",
    matchEntry: isNeteaseEntry,
    fileSummary(entry) {
      if (entry.kind !== "file") return null;
      const artists = Array.isArray(entry.sourcePayload?.artists) ? entry.sourcePayload.artists.join("，") : "";
      const album = entry.sourcePayload?.albumName;
      return {
        inline: artists || "网易云音乐虚拟歌曲",
        rows: [
          artists ? { label: "艺术家", value: artists } : null,
          album ? { label: "专辑", value: String(album) } : null,
        ].filter(Boolean),
      };
    },
  });

  ctx.registerEntryActionProvider({
    matchEntry: isNeteaseEntry,
    getEntryActions(context) {
      const disabled = loginExpired(context.entry);
      const disabledLabel = "登录已失效，请重新登录";
      if (isTrackEntry(context.entry)) {
        return [
          {
            id: "netease-download-local",
            label: "下载到本地",
            disabled,
            confirmLabel: disabled ? disabledLabel : undefined,
            onSelect: async () => {
              if (disabled) return;
              await ensureLoginReady(ctx, context.entry);
              const folder = await chooseLocalFolder(context);
              if (!folder) return;
              await downloadTrackWithProgress(ctx, context.entry, folder, "下载歌曲到本地");
            },
          },
          {
            id: "netease-download-repository",
            label: "下载到其他资源库",
            disabled,
            confirmLabel: disabled ? disabledLabel : undefined,
            onSelect: async () => {
              if (disabled) return;
              await ensureLoginReady(ctx, context.entry);
              const repo = await chooseRepository(context);
              if (!repo) return;
              await downloadTrackWithProgress(ctx, context.entry, repo, "下载歌曲到资源库");
              await syncDownloadedRepository(ctx, repo, context);
            },
          },
          {
            id: "netease-clear-playback-cache",
            label: "重新获取播放资源",
            disabled,
            confirmLabel: disabled ? disabledLabel : undefined,
            onSelect: async () => {
              if (disabled) return;
              await callDownloader(ctx, "downloader.clearTrackCache", {
                songId: songIdFromEntry(context.entry),
                level: context.entry.sourcePayload?.level ?? "standard",
                sourcePayload: context.entry.sourcePayload ?? {},
              });
            },
          },
        ];
      }

      if (isPlaylistFolder(context.entry)) {
        return [
          {
            id: "netease-playlist-download-local",
            label: "下载歌单到本地",
            disabled,
            confirmLabel: disabled ? disabledLabel : undefined,
            onSelect: async () => {
              if (disabled) return;
              await ensureLoginReady(ctx, context.entry);
              const folder = await chooseLocalFolder(context);
              if (!folder) return;
              await downloadPlaylistWithProgress(ctx, context, folder);
            },
          },
          {
            id: "netease-playlist-download-repository",
            label: "下载歌单到其他资源库",
            disabled,
            confirmLabel: disabled ? disabledLabel : undefined,
            onSelect: async () => {
              if (disabled) return;
              await ensureLoginReady(ctx, context.entry);
              const repo = await chooseRepository(context);
              if (!repo) return;
              await downloadPlaylistWithProgress(ctx, context, repo);
              await syncDownloadedRepository(ctx, repo, context);
            },
          },
        ];
      }

      return [];
    },
  });
}

export { register };
