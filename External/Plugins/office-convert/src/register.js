/**
 * Office Convert 设置页。
 *
 * 展示转换器探测、自带 LibreOffice 下载状态、守护进程状态，
 * 并提供按资源库清理预览缓存的操作。
 */
const OFFICE_CONVERT_PLUGIN_ID = "momobako.service.office-convert";

export function register(ctx) {
  ctx.registerSettingsPage({
    label: "Office 转换",
    component: createSettingsPage(ctx),
  });
}

function createSettingsPage(ctx) {
  const { computed, h, onMounted, ref } = ctx.vue;
  return {
    name: "OfficeConvertSettingsPage",
    props: ["manifest"],
    setup() {
      const status = ref(null);
      const repositories = ref([]);
      const selectedRepoId = ref("");
      const clearing = ref(false);
      const loading = ref(false);
      const message = ref("");
      const error = ref("");

      const selectedRepository = computed(() =>
        repositories.value.find((item) => item.repoId === selectedRepoId.value) || null,
      );

      onMounted(() => {
        void loadAll();
      });

      async function loadAll() {
        loading.value = true;
        error.value = "";
        message.value = "";
        try {
          const [runtimeResponse, repoList] = await Promise.all([
            ctx.callPlugin({
              pluginId: OFFICE_CONVERT_PLUGIN_ID,
              method: "officeConvert.getRuntimeStatus",
              payload: {},
            }),
            ctx.invokeCommand("list_repositories"),
          ]);
          status.value = runtimeResponse.payload ?? null;
          repositories.value = Array.isArray(repoList) ? repoList : [];
          if (!selectedRepoId.value && repositories.value.length > 0) {
            selectedRepoId.value = repositories.value[0].repoId;
          }
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          loading.value = false;
        }
      }

      async function clearCache() {
        if (!selectedRepoId.value || clearing.value) return;
        clearing.value = true;
        error.value = "";
        message.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: OFFICE_CONVERT_PLUGIN_ID,
            method: "officeConvert.clearPreviewCache",
            payload: {
              repoId: selectedRepoId.value,
            },
          });
          const removed = response.payload?.removed ?? 0;
          message.value = `已清理 ${removed} 个缓存文件`;
          await loadAll();
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          clearing.value = false;
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
            h("strong", "运行状态与缓存"),
          ]),
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            onClick: loadAll,
            disabled: loading.value,
          }, loading.value ? "刷新中" : "刷新"),
        ]),
        error.value
          ? h("p", { class: "repository-add-popover__note" }, error.value)
          : null,
        message.value
          ? h("p", { class: "repository-add-popover__note" }, message.value)
          : null,
        status.value
          ? h("div", { class: "file-metadata-card__source-grid" }, [
              row("模式", status.value.converterMode ?? "auto"),
              row("自动下载", status.value.autoDownloadLibreOffice ? "开启" : "关闭"),
              row("Microsoft Office", statusText(status.value.microsoftOffice)),
              row("系统 LibreOffice", statusText(status.value.libreofficeSystem)),
              row("自带 LibreOffice", statusText(status.value.libreofficeBundle)),
              row("自带下载地址", status.value.bundledDownloadUrl || "未配置"),
              row("守护进程", daemonText(status.value.daemon)),
            ])
          : h("p", { class: "repository-add-popover__note" }, "读取当前转换器、自带运行时与守护进程状态。"),
        h("div", { class: "file-metadata-card__source-head", style: "margin-top: 12px;" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "Preview Cache"),
            h("strong", "资源库缓存清理"),
          ]),
        ]),
        h("div", { class: "file-metadata-card__source-grid" }, [
          row("当前资源库", selectedRepository.value ? selectedRepository.value.name : "未选择"),
        ]),
        h("div", { class: "file-metadata-card__source-row", style: "gap: 8px; align-items: center;" }, [
          h("select", {
            class: "plugin-manager__field-input",
            value: selectedRepoId.value,
            onChange: (event) => {
              selectedRepoId.value = event?.target?.value ?? "";
            },
          }, [
            h("option", { value: "" }, "选择资源库"),
            ...repositories.value.map((repo) => h("option", { value: repo.repoId }, repo.name)),
          ]),
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            disabled: !selectedRepoId.value || clearing.value,
            onClick: clearCache,
          }, clearing.value ? "清理中" : "清理缓存"),
        ]),
        h("p", { class: "repository-add-popover__note" }, "转换后的 PDF 缓存位于资源库 .momo/cache/office-preview 目录。"),
      ]);
    },
  };
}

function statusText(status) {
  if (!status) return "未知";
  if (status.available) {
    const parts = [status.path || "可用"];
    if (status.version) parts.push(status.version);
    return parts.join(" | ");
  }
  return status.reason || "不可用";
}

function daemonText(daemon) {
  if (!daemon) return "未知";
  if (daemon.running) {
    const pid = daemon.pid ? `PID ${daemon.pid}` : "运行中";
    const updatedAt = daemon.updatedAt ? `更新于 ${daemon.updatedAt}` : "";
    return [pid, updatedAt].filter(Boolean).join(" | ");
  }
  return daemon.error || "未运行";
}
