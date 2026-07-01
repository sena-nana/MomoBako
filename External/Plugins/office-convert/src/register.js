const OFFICE_CONVERT_PLUGIN_ID = "momobako.service.office-convert";

export function register(ctx) {
  ctx.registerSettingsPage({
    label: "Office 转换",
    component: createSettingsPage(ctx),
  });
}

function createSettingsPage(ctx) {
  const { h, onMounted, ref } = ctx.vue;
  return {
    name: "OfficeConvertSettingsPage",
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
            pluginId: OFFICE_CONVERT_PLUGIN_ID,
            method: "officeConvert.getRuntimeStatus",
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
            h("p", { class: "asset-browser__eyebrow" }, "Office Convert"),
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
              row("模式", status.value.converterMode ?? "auto"),
              row("Microsoft Office", statusText(status.value.microsoftOffice)),
              row("系统 LibreOffice", statusText(status.value.libreofficeSystem)),
              row("自带 LibreOffice", statusText(status.value.libreofficeBundle)),
              row("守护进程", status.value.daemon?.running ? "运行中" : "未运行"),
            ])
          : h("p", { class: "repository-add-popover__note" }, "读取当前转换器与守护进程状态。"),
      ]);
    },
  };
}

function statusText(status) {
  if (!status) return "未知";
  if (status.available) return status.path || "可用";
  return status.reason || "不可用";
}
