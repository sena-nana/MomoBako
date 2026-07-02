/**
 * Eagle 导入工具页。
 * 支持复制导入和剪切导入，并展示宿主返回的摘要与警告。
 */
export function register(ctx) {
  const { computed, h, ref } = ctx.vue;

  const EagleImporterToolPage = {
    name: "EagleImporterToolPage",
    props: {
      manifest: { type: Object, required: true },
      activeRepoId: { type: String, default: null },
      activeRepository: { type: Object, default: null },
      currentDirectoryPath: { type: String, default: "" },
      isRepositoryWritable: { type: Boolean, default: false },
      isTrashPanel: { type: Boolean, default: false },
      isVirtualView: { type: Boolean, default: false },
    },
    setup(props) {
      const libraryPath = ref("");
      const status = ref("idle");
      const notice = ref("");
      const error = ref("");
      const lastResult = ref(null);

      const canImport = computed(() => (
        Boolean(props.activeRepoId)
        && props.isRepositoryWritable
        && !props.isTrashPanel
        && !props.isVirtualView
      ));
      const canSubmit = computed(() => canImport.value && Boolean(libraryPath.value.trim()));
      const disabledReason = computed(() => {
        if (!props.activeRepoId) return "当前没有可用仓库。";
        if (!props.isRepositoryWritable) return "当前仓库处于只读状态。";
        if (props.isTrashPanel) return "回收站视图不支持导入。";
        if (props.isVirtualView) return "虚拟视图不支持导入。";
        return "";
      });
      const targetDirectoryLabel = computed(() => props.currentDirectoryPath || "/");
      const targetRepositoryLabel = computed(() => props.activeRepository?.name || "未选择仓库");
      const isBusy = computed(() => status.value === "loading");

      async function chooseLibraryPath() {
        const selected = await ctx.openDialog({
          title: "选择 EagleLibrary 目录",
          directory: true,
          multiple: false,
        });
        if (typeof selected !== "string" || !selected.trim()) return;
        libraryPath.value = selected;
      }

      async function handleCopyImport() {
        await runImport("copy");
      }

      async function handleMoveImport() {
        await runImport("move");
      }

      /**
       * 请求宿主执行 EagleLibrary 合并，并记录导入摘要。
       */
      async function runImport(mode) {
        if (!canImport.value || !props.activeRepoId) {
          error.value = disabledReason.value || "当前目标不可导入。";
          return;
        }
        if (!libraryPath.value.trim()) {
          error.value = "请先选择 EagleLibrary 目录。";
          return;
        }
        status.value = "loading";
        notice.value = "";
        error.value = "";
        lastResult.value = null;
        try {
          const response = await requestWorkspaceImport(ctx, {
            requestId: createRequestId("eagle-importer"),
            action: "eagle",
            repoId: props.activeRepoId,
            parentPath: props.currentDirectoryPath || "",
            libraryPath: libraryPath.value.trim(),
            mode,
          });
          lastResult.value = response.result || null;
          notice.value = `${mode === "move" ? "剪切导入" : "复制导入"}完成，目标目录：${targetDirectoryLabel.value}`;
          status.value = "success";
        } catch (cause) {
          error.value = errorText(cause);
          status.value = "error";
        }
      }

      return {
        canImport,
        canSubmit,
        disabledReason,
        error,
        isBusy,
        lastResult,
        libraryPath,
        notice,
        targetDirectoryLabel,
        targetRepositoryLabel,
        chooseLibraryPath,
        handleCopyImport,
        handleMoveImport,
      };
    },
    render() {
      const summary = this.lastResult?.summary;
      const warnings = Array.isArray(this.lastResult?.warnings) ? this.lastResult.warnings : [];
      return h("section", { class: "tool-page-shell" }, [
        h("header", { class: "tool-page-shell__header" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "Eagle 导入"),
            h("h1", this.manifest?.name || "Eagle Importer"),
            h("p", { class: "tool-page-shell__subline" }, "导入目标固定为当前工作区的当前目录。"),
          ]),
        ]),
        renderTargetCard(h, {
          repository: this.targetRepositoryLabel,
          directory: this.targetDirectoryLabel,
        }),
        this.disabledReason
          ? h("div", { class: "asset-browser__state" }, this.disabledReason)
          : null,
        this.error
          ? h("div", { class: "asset-browser__state asset-browser__state--error" }, this.error)
          : null,
        this.notice
          ? h("div", { class: "asset-browser__state" }, this.notice)
          : null,
        h("div", { class: "tool-page-shell__card" }, [
          h("div", { class: "tool-page-shell__row" }, [
            h("span", "EagleLibrary"),
            h("code", this.libraryPath || "尚未选择"),
          ]),
          h("div", { class: "tool-page-shell__actions" }, [
            h("button", {
              type: "button",
              class: "ghost",
              disabled: !this.canImport || this.isBusy,
              onClick: this.chooseLibraryPath,
            }, "选择目录"),
            h("button", {
              type: "button",
              class: "primary",
              disabled: !this.canSubmit || this.isBusy,
              onClick: this.handleCopyImport,
            }, this.isBusy ? "处理中..." : "复制导入"),
            h("button", {
              type: "button",
              class: "ghost",
              disabled: !this.canSubmit || this.isBusy,
              onClick: this.handleMoveImport,
            }, "剪切导入"),
          ]),
        ]),
        summary
          ? h("div", { class: "tool-page-shell__card" }, [
              h("p", { class: "asset-browser__eyebrow" }, "导入结果"),
              h("div", { class: "tool-page-shell__summary" }, [
                renderSummaryItem(h, "文件", summary.importedFiles),
                renderSummaryItem(h, "目录", summary.importedDirectories),
                renderSummaryItem(h, "回收站", summary.importedTrashEntries),
                renderSummaryItem(h, "快捷入口", summary.importedShortcuts),
                renderSummaryItem(h, "智能文件夹", summary.importedSmartFolders),
                renderSummaryItem(h, "仓库动作", summary.importedRepositoryActions),
                renderSummaryItem(h, "标签组", summary.importedTagGroups),
                renderSummaryItem(h, "Alias 组", summary.importedAliasGroups),
                renderSummaryItem(h, "硬链接组", summary.importedHardlinkGroups),
              ]),
              warnings.length
                ? h("div", { class: "tool-page-shell__warnings" }, [
                    h("p", { class: "asset-browser__eyebrow" }, `警告 ${warnings.length}`),
                    ...warnings.slice(0, 20).map((item, index) => (
                      h("div", { class: "tool-page-shell__row", key: `${item.type || item.warningType || "warning"}-${index}` }, [
                        h("span", item.type || item.warningType || "warning"),
                        h("code", warningText(item)),
                      ])
                    )),
                  ])
                : null,
            ])
          : null,
      ]);
    },
  };

  ctx.registerToolPage({
    toolPageId: "momobako.tool.eagle-importer",
    label: "Eagle 导入",
    description: "从 EagleLibrary 复制或剪切导入",
    order: 30,
    component: EagleImporterToolPage,
  });
}

function createRequestId(prefix) {
  return `${prefix}:${Date.now()}:${Math.random().toString(36).slice(2, 10)}`;
}

function errorText(cause) {
  return cause instanceof Error ? cause.message : String(cause);
}

function warningText(item) {
  if (!item || typeof item !== "object") return "";
  if (typeof item.reason === "string" && item.reason.trim()) return item.reason;
  if (typeof item.assetId === "string" && item.assetId.trim()) return item.assetId;
  if (typeof item.sourceId === "string" && item.sourceId.trim()) return item.sourceId;
  if (item.details != null) {
    try {
      return JSON.stringify(item.details);
    } catch {
      return String(item.details);
    }
  }
  return "";
}

function renderSummaryItem(h, label, value) {
  return h("div", { class: "tool-page-shell__summary-item" }, [
    h("span", label),
    h("strong", String(value ?? 0)),
  ]);
}

function renderTargetCard(h, target) {
  return h("div", { class: "tool-page-shell__card" }, [
    h("div", { class: "tool-page-shell__row" }, [
      h("span", "目标仓库"),
      h("strong", target.repository),
    ]),
    h("div", { class: "tool-page-shell__row" }, [
      h("span", "目标目录"),
      h("code", target.directory),
    ]),
  ]);
}

function requestWorkspaceImport(ctx, payload) {
  return new Promise((resolve, reject) => {
    const timeoutId = window.setTimeout(() => {
      dispose();
      reject(new Error("导入请求超时，请稍后重试。"));
    }, 120000);
    const dispose = ctx.onPluginEvent("workspace:import-response", (response) => {
      if (!response || response.requestId !== payload.requestId) return;
      window.clearTimeout(timeoutId);
      dispose();
      if (response.status === "success") {
        resolve(response);
        return;
      }
      reject(new Error(response.message || "导入失败。"));
    });
    ctx.emitPluginEvent("workspace:import-request", payload);
  });
}
