/**
 * 通用下载服务设置页。
 *
 * 展示 aria2 运行状态、任务队列和当前运行目录，
 * 帮助定位音乐下载与运行时下载问题。
 */
const DOWNLOADER_PLUGIN_ID = "momobako.service.downloader";

export function register(ctx) {
  ctx.registerSettingsPage({
    label: "下载服务",
    component: createSettingsPage(ctx),
  });
}

function createSettingsPage(ctx) {
  const { h, onMounted, ref } = ctx.vue;
  return {
    name: "DownloaderSettingsPage",
    props: ["manifest"],
    setup() {
      const status = ref(null);
      const error = ref("");
      const loading = ref(false);

      onMounted(() => {
        void loadStatus();
      });

      async function loadStatus() {
        loading.value = true;
        error.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: DOWNLOADER_PLUGIN_ID,
            method: "downloader.getRuntimeStatus",
            payload: {},
          });
          status.value = response.payload ?? null;
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          loading.value = false;
        }
      }

      function row(label, value) {
        return h("div", { class: "asset-meta__row file-metadata-card__source-row" }, [
          h("span", label),
          h("span", { class: "asset-meta__value" }, value),
        ]);
      }

      return () => h("section", { class: "file-metadata-card__source file-metadata-card__library" }, [
        h("div", { class: "file-metadata-card__source-head" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "Downloader"),
            h("strong", "aria2 运行状态"),
          ]),
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            onClick: loadStatus,
            disabled: loading.value,
          }, loading.value ? "刷新中" : "刷新"),
        ]),
        error.value
          ? h("p", { class: "repository-add-popover__note" }, error.value)
          : null,
        status.value
          ? h("div", { class: "file-metadata-card__source-grid" }, [
              row("运行时", status.value.runtime || "aria2"),
              row("aria2 状态", aria2RunningText(status.value.aria2)),
              row("aria2 版本", status.value.aria2?.version || "未知"),
              row("RPC 地址", status.value.aria2?.rpcUrl || "未启动"),
              row("任务数", String(status.value.queueSize ?? 0)),
              row("下载目录", status.value.downloadsDir || "未初始化"),
              row("下载源", status.value.downloadUrl || "未配置"),
            ])
          : h("p", { class: "repository-add-popover__note" }, "查看 aria2 运行时、任务队列和下载目录状态。"),
        h("p", { class: "repository-add-popover__note" }, "音乐下载、歌词导出与 LibreOffice 运行时下载统一复用这套 aria2 任务层。"),
      ]);
    },
  };
}

function aria2RunningText(status) {
  if (!status) return "未知";
  if (status.running) {
    const parts = ["运行中"];
    if (status.pid) parts.push(`PID ${status.pid}`);
    if (status.source) parts.push(status.source);
    return parts.join(" | ");
  }
  return status.error || "未运行";
}
