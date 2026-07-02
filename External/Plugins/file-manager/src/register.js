/**
 * 文件导入工具页。
 * 通过工作区事件桥复用宿主已有导入链路。
 */
export function register(ctx) {
  const { computed, h, ref } = ctx.vue;

  const FileManagerToolPage = {
    name: "FileManagerToolPage",
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
      const status = ref("idle");
      const notice = ref("");
      const error = ref("");

      const canImport = computed(() => (
        Boolean(props.activeRepoId)
        && props.isRepositoryWritable
        && !props.isTrashPanel
        && !props.isVirtualView
      ));
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

      async function handleImportFromFolder() {
        const selected = await ctx.openDialog({
          title: "选择导入文件夹",
          directory: true,
          multiple: false,
        });
        if (typeof selected !== "string" || !selected.trim()) return;
        await runImport({
          action: "folder",
          sourcePath: selected,
        }, "文件夹导入完成。");
      }

      async function handleImportFromZip() {
        const selected = await ctx.openDialog({
          title: "选择 ZIP 压缩包",
          directory: false,
          multiple: false,
          filters: [{ name: "ZIP", extensions: ["zip"] }],
        });
        if (typeof selected !== "string" || !selected.trim()) return;
        await runImport({
          action: "zip",
          archivePath: selected,
        }, "ZIP 导入完成。");
      }

      /**
       * 通过 request/response 事件桥复用工作区导入链路。
       */
      async function runImport(payload, successMessage) {
        if (!canImport.value || !props.activeRepoId) {
          error.value = disabledReason.value || "当前目标不可导入。";
          return;
        }
        status.value = "loading";
        notice.value = "";
        error.value = "";
        try {
          await requestWorkspaceImport(ctx, {
            requestId: createRequestId("file-manager"),
            repoId: props.activeRepoId,
            parentPath: props.currentDirectoryPath || "",
            ...payload,
          });
          notice.value = `${successMessage} 目标目录：${targetDirectoryLabel.value}`;
          status.value = "success";
        } catch (cause) {
          error.value = errorText(cause);
          status.value = "error";
        }
      }

      return {
        canImport,
        disabledReason,
        error,
        isBusy,
        notice,
        targetDirectoryLabel,
        targetRepositoryLabel,
        handleImportFromFolder,
        handleImportFromZip,
      };
    },
    render() {
      return h("section", { class: "tool-page-shell" }, [
        h("header", { class: "tool-page-shell__header" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "文件导入"),
            h("h1", this.manifest?.name || "File Manager"),
            h("p", { class: "tool-page-shell__subline" }, "目标固定为当前工作区的当前目录。"),
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
        h("div", { class: "tool-page-shell__actions" }, [
          h("button", {
            type: "button",
            class: "primary",
            disabled: !this.canImport || this.isBusy,
            onClick: this.handleImportFromFolder,
          }, this.isBusy ? "处理中..." : "从文件夹导入"),
          h("button", {
            type: "button",
            class: "ghost",
            disabled: !this.canImport || this.isBusy,
            onClick: this.handleImportFromZip,
          }, "从 ZIP 导入"),
        ]),
        h("div", { class: "tool-page-shell__notes" }, [
          h("p", "ZIP 首版固定支持 .zip，并按解压导入保留内部目录结构。"),
        ]),
      ]);
    },
  };

  ctx.registerToolPage({
    toolPageId: "momobako.tool.file-manager",
    label: "文件导入",
    description: "从文件夹或 ZIP 导入到当前目录",
    order: 20,
    component: FileManagerToolPage,
  });
}

function createRequestId(prefix) {
  return `${prefix}:${Date.now()}:${Math.random().toString(36).slice(2, 10)}`;
}

function errorText(cause) {
  return cause instanceof Error ? cause.message : String(cause);
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
