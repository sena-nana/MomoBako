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
      const stoppingDaemon = ref(false);
      const selfChecking = ref(false);
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

      async function shutdownDaemon() {
        if (stoppingDaemon.value) return;
        stoppingDaemon.value = true;
        error.value = "";
        message.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: OFFICE_CONVERT_PLUGIN_ID,
            method: "officeConvert.shutdownDaemon",
            payload: {},
          });
          message.value = response.payload?.stopped
            ? "LibreOffice 守护进程已关闭"
            : "当前没有运行中的 LibreOffice 守护进程";
          await loadAll();
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          stoppingDaemon.value = false;
        }
      }

      async function runSelfCheck() {
        if (selfChecking.value) return;
        selfChecking.value = true;
        error.value = "";
        message.value = "";
        try {
          const response = await ctx.callPlugin({
            pluginId: OFFICE_CONVERT_PLUGIN_ID,
            method: "officeConvert.runRuntimeSelfCheck",
            payload: {},
          });
          const ok = response.payload?.ok === true;
          message.value = ok ? "运行时自检通过" : (response.payload?.error || "运行时自检失败");
          await loadAll();
        } catch (cause) {
          error.value = cause instanceof Error ? cause.message : String(cause);
        } finally {
          selfChecking.value = false;
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
              row("Helper 类型", status.value.daemon?.helperType || "未声明"),
              row("Helper 端口", status.value.daemon?.port ? String(status.value.daemon.port) : "未分配"),
              row("Helper 地址", status.value.daemon?.baseUrl || "未生成"),
              row("健康检查", status.value.daemon?.healthy ? "通过" : "未通过"),
              row("Soffice 就绪", sofficeReadyText(status.value.daemon)),
              row("Soffice PID", status.value.daemon?.sofficePid ? String(status.value.daemon.sofficePid) : "未上报"),
              row("UNO 可用", booleanText(status.value.daemon?.unoAvailable)),
              row("Python 有效", booleanText(status.value.daemon?.pythonValid)),
              row("Python 路径", status.value.daemon?.pythonPath || "未上报"),
              row("控制方式", daemonControlText(status.value.daemon)),
              row("最近转换", daemonConvertText(status.value.daemon)),
              row("最近自检", daemonSelfCheckText(status.value.daemon)),
              row("自检样本", status.value.daemon?.lastSelfCheck?.samplePath || "未记录"),
              row("自检输出", status.value.daemon?.lastSelfCheck?.pdfPath || "未记录"),
              row("自检转换器", selfCheckConverterText(status.value.daemon?.lastSelfCheck)),
            ])
          : h("p", { class: "repository-add-popover__note" }, "读取当前转换器、自带运行时与守护进程状态。"),
        h("div", { class: "file-metadata-card__source-row", style: "gap: 8px; margin-top: 8px;" }, [
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            disabled: selfChecking.value,
            onClick: runSelfCheck,
          }, selfChecking.value ? "自检中" : "运行自检"),
          h("button", {
            type: "button",
            class: "repository-add-popover__action",
            disabled: stoppingDaemon.value || !status.value?.daemon?.running,
            onClick: shutdownDaemon,
          }, stoppingDaemon.value ? "关闭中" : "关闭守护进程"),
        ]),
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
    const soffice = daemon.sofficeReady ? "Soffice 已就绪" : "Soffice 未就绪";
    const updatedAt = daemon.updatedAt ? `更新于 ${daemon.updatedAt}` : "";
    return [pid, soffice, updatedAt].filter(Boolean).join(" | ");
  }
  return daemon.error || "未运行";
}

function sofficeReadyText(daemon) {
  if (daemon?.sofficeReady === true) return "已就绪";
  if (daemon?.sofficeReady === false) return "未就绪";
  return "未上报";
}

function booleanText(value) {
  if (value === true) return "是";
  if (value === false) return "否";
  return "未上报";
}

function daemonControlText(daemon) {
  if (!daemon?.control) return "未声明";
  const parts = [];
  if (daemon.control.health) parts.push(`health=${daemon.control.health}`);
  if (daemon.control.shutdown) parts.push(`shutdown=${daemon.control.shutdown}`);
  return parts.join(" | ") || "未声明";
}

function daemonConvertText(daemon) {
  const lastConvert = daemon?.lastConvert;
  if (!lastConvert) return "暂无记录";
  const parts = [];
  if (lastConvert.phase) parts.push(lastConvert.phase);
  if (lastConvert.conversionMode) parts.push(lastConvert.conversionMode);
  if (lastConvert.sourcePath) parts.push(lastConvert.sourcePath);
  if (lastConvert.updatedAt) parts.push(lastConvert.updatedAt);
  return parts.join(" | ");
}

function daemonSelfCheckText(daemon) {
  const lastSelfCheck = daemon?.lastSelfCheck;
  if (!lastSelfCheck) return "暂无记录";
  const parts = [];
  parts.push(lastSelfCheck.ok ? "通过" : "失败");
  if (lastSelfCheck.converter) parts.push(lastSelfCheck.converter);
  if (lastSelfCheck.converterVersion) parts.push(lastSelfCheck.converterVersion);
  if (lastSelfCheck.conversionMode) parts.push(lastSelfCheck.conversionMode);
  if (typeof lastSelfCheck.pdfSizeBytes === "number") parts.push(`${lastSelfCheck.pdfSizeBytes} bytes`);
  if (typeof lastSelfCheck.durationMs === "number") parts.push(`${lastSelfCheck.durationMs} ms`);
  if (lastSelfCheck.completedAt) parts.push(lastSelfCheck.completedAt);
  return parts.join(" | ");
}

function selfCheckConverterText(lastSelfCheck) {
  if (!lastSelfCheck) return "未记录";
  const parts = [];
  if (lastSelfCheck.converter) parts.push(lastSelfCheck.converter);
  if (lastSelfCheck.converterVersion) parts.push(lastSelfCheck.converterVersion);
  if (lastSelfCheck.converterPath) parts.push(lastSelfCheck.converterPath);
  return parts.join(" | ") || "未记录";
}
