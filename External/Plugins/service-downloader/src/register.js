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

      onMounted(() => {
        void loadStatus();
      });

      async function loadStatus() {
        error.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: DOWNLOADER_PLUGIN_ID,
            method: "downloader.getRuntimeStatus",
            payload: {},
          });
          status.value = response.payload;
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
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
            h("strong", "运行状态"),
          ]),
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            onClick: loadStatus,
          }, "刷新"),
        ]),
        error.value
          ? h("p", { class: "repository-add-popover__note" }, error.value)
          : null,
        status.value
          ? h("div", { class: "file-metadata-card__source-grid" }, [
              row("运行时", status.value.runtime || "native"),
              row("aria2", status.value.aria2?.running ? "运行中" : "未运行"),
              row("下载目录", status.value.downloadsDir || "未初始化"),
              row("任务数", String(status.value.queueSize ?? 0)),
            ])
          : h("p", { class: "repository-add-popover__note" }, "查看 aria2 运行时与下载队列状态。"),
      ]);
    },
  };
}
