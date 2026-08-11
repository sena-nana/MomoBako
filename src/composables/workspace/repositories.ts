import { computed, ref } from "vue";
import {
  attachRepositoryFolder,
  configureNeteaseRepositoryCache,
  createRepository,
  deleteRepository,
  exportRepository,
  importRepository,
  relocateRepository,
} from "../../services/repositoryApi";
import type {
  RepositoryDeleteMode,
  RepositoryExportRequest,
  RepositoryExportResponse,
  RepositorySummary,
} from "../../types/repository";
import { emitSystemLogSilently } from "../../services/systemLog";
import {
  activeRepoId,
  activeSnapshot,
  error,
  repositories,
} from "./state";
import { resetWorkspaceSelection } from "./lifecycle";
import {
  cancelOperationProgress,
  finishOperationProgress,
  startOperationProgress,
  updateOperationProgress,
} from "./tasks";

type RepositoryWorkspaceDependencies = {
  loadRepositories: () => Promise<unknown>;
  selectRepository: (repoId: string) => Promise<unknown>;
};

let dependencies: RepositoryWorkspaceDependencies | null = null;
const repositoryDeleteDialogRepoId = ref<string | null>(null);
const repositoryDeleteDialogOpen = ref(false);
const repositoryDeleteError = ref("");
const deletingRepositoryMode = ref<RepositoryDeleteMode | null>(null);

function repositoryUsesLocalMetadata(repository: RepositorySummary) {
  if (repository.backend.capabilities.includes("localRootPath")) return true;
  return repository.localCache?.required === true
    && repository.localCache.status !== "unconfigured";
}

function repositoryHasAccessibleLocalMetadata(repository: RepositorySummary) {
  if (repository.localCache?.required) {
    return repository.localCache?.status === "ready";
  }
  return repository.status === "ready";
}

export const pendingDeleteRepository = computed(() => (
  repositories.value.find((item) => item.repoId === repositoryDeleteDialogRepoId.value) ?? null
));

export const canDeletePendingRepositoryMetadata = computed(() => {
  const repository = pendingDeleteRepository.value;
  if (!repository) return false;
  return repositoryUsesLocalMetadata(repository)
    ? repositoryHasAccessibleLocalMetadata(repository)
    : true;
});

export const canDeletePendingRepositoryFolder = computed(() => {
  const repository = pendingDeleteRepository.value;
  if (!repository) return false;
  return repositoryUsesLocalMetadata(repository)
    && repositoryHasAccessibleLocalMetadata(repository);
});

export const isDeletingRepository = computed(() => deletingRepositoryMode.value !== null);
export const repositoryDeleteDialogVisible = computed(() => repositoryDeleteDialogOpen.value);
export const repositoryDeleteDialogError = computed(() => repositoryDeleteError.value);

export function configureRepositoryWorkspaceActions(nextDependencies: RepositoryWorkspaceDependencies) {
  dependencies = nextDependencies;
}

function repositoryDependencies() {
  if (!dependencies) {
    throw new Error("repository workspace actions are not configured");
  }
  return dependencies;
}

function sortRepositories(items: typeof repositories.value) {
  return [...items].sort((left, right) => left.name.localeCompare(right.name, "zh-CN"));
}

function upsertRepositorySummary(repository: (typeof repositories.value)[number]) {
  repositories.value = sortRepositories([
    ...repositories.value.filter((item) => item.repoId !== repository.repoId),
    repository,
  ]);
}

function removeRepositorySummary(repoId: string) {
  repositories.value = repositories.value.filter((item) => item.repoId !== repoId);
}

function selectNextRepositoryAfterRemoval(removedRepoId: string) {
  const items = repositories.value;
  if (!items.length) {
    resetWorkspaceSelection({ clearRememberedRepository: true });
    return null;
  }
  const next = items.find((item) => item.repoId !== removedRepoId) ?? items[0];
  return next.repoId;
}

export async function createNewRepository(
  name: string,
  path: string,
  backendPluginId?: string,
  backendConfig?: Record<string, unknown>,
  repoId?: string,
  options?: {
    skipInitialSync?: boolean;
  },
) {
  emitSystemLogSilently("info", {
    category: "repository",
    action: "createStart",
    message: "开始创建资源库。",
    context: { name, path, backendPluginId, skipInitialSync: options?.skipInitialSync ?? false },
  });
  const progressId = startOperationProgress(
    "创建资源库",
    options?.skipInitialSync ? "初始化资源库" : "初始化资源库并扫描文件",
    { initial: 8 },
  );
  try {
    const response = await createRepository({
      repoId,
      name,
      path,
      backendPluginId,
      backendConfig,
      skipInitialSync: options?.skipInitialSync,
    });
    updateOperationProgress(progressId, {
      detail: "切换到新资源库",
      value: options?.skipInitialSync ? 72 : 88,
      indeterminate: false,
    });
    upsertRepositorySummary(response.repository);
    await repositoryDependencies().selectRepository(response.repository.repoId);
    emitSystemLogSilently("info", {
      category: "repository",
      action: "createSuccess",
      message: "资源库创建完成。",
      repoId: response.repository.repoId,
      context: { name: response.repository.name, path: response.repository.path },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    emitSystemLogSilently("error", {
      category: "repository",
      action: "createFailed",
      message: "资源库创建失败。",
      context: { name, path, error: cause instanceof Error ? cause.message : String(cause) },
    });
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function importExistingRepository(name: string, path: string) {
  emitSystemLogSilently("info", {
    category: "repository",
    action: "importStart",
    message: "开始导入资源库。",
    context: { name, path },
  });
  const progressId = startOperationProgress("导入资源库", "读取资源库元数据并扫描文件", { initial: 8 });
  try {
    const response = await importRepository({ name, path });
    upsertRepositorySummary(response.repository);
    await repositoryDependencies().selectRepository(response.repository.repoId);
    emitSystemLogSilently("info", {
      category: "repository",
      action: "importSuccess",
      message: "资源库导入完成。",
      repoId: response.repository.repoId,
      context: { name: response.repository.name, path: response.repository.path },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    emitSystemLogSilently("error", {
      category: "repository",
      action: "importFailed",
      message: "资源库导入失败。",
      context: { name, path, error: cause instanceof Error ? cause.message : String(cause) },
    });
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function attachRepository(path: string) {
  emitSystemLogSilently("info", {
    category: "repository",
    action: "attachStart",
    message: "开始挂载资源库。",
    context: { path },
  });
  const progressId = startOperationProgress("挂载资源库", "检查文件夹并读取索引", { initial: 8 });
  try {
    const response = await attachRepositoryFolder({ path });
    upsertRepositorySummary(response.repository);
    await repositoryDependencies().selectRepository(response.repository.repoId);
    emitSystemLogSilently("info", {
      category: "repository",
      action: "attachSuccess",
      message: "资源库挂载完成。",
      repoId: response.repository.repoId,
      context: { path: response.repository.path },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    emitSystemLogSilently("error", {
      category: "repository",
      action: "attachFailed",
      message: "资源库挂载失败。",
      context: { path, error: cause instanceof Error ? cause.message : String(cause) },
    });
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function removeRepository(repoId: string, mode: RepositoryDeleteMode = "recordOnly") {
  emitSystemLogSilently("info", {
    category: "repository",
    action: "deleteStart",
    message: "开始删除资源库。",
    repoId,
    context: { mode },
  });
  await deleteRepository({ repoId, mode });
  removeRepositorySummary(repoId);
  if (activeRepoId.value !== repoId) return;
  const nextRepoId = selectNextRepositoryAfterRemoval(repoId);
  if (nextRepoId) {
    await repositoryDependencies().selectRepository(nextRepoId);
  }
  emitSystemLogSilently("warn", {
    category: "repository",
    action: "deleteSuccess",
    message: "资源库已删除。",
    repoId,
    context: { mode },
  });
}

export function openRepositoryDeleteDialog(repoId: string) {
  if (isDeletingRepository.value) return;
  if (!repositories.value.some((item) => item.repoId === repoId)) return;
  repositoryDeleteDialogRepoId.value = repoId;
  repositoryDeleteError.value = "";
  repositoryDeleteDialogOpen.value = true;
}

export function closeRepositoryDeleteDialog() {
  if (isDeletingRepository.value) return;
  repositoryDeleteDialogOpen.value = false;
  repositoryDeleteDialogRepoId.value = null;
  repositoryDeleteError.value = "";
}

export async function confirmRepositoryDelete(mode: RepositoryDeleteMode) {
  const repoId = repositoryDeleteDialogRepoId.value;
  if (!repoId || isDeletingRepository.value) return;
  deletingRepositoryMode.value = mode;
  repositoryDeleteError.value = "";
  try {
    await removeRepository(repoId, mode);
    repositoryDeleteDialogOpen.value = false;
    repositoryDeleteDialogRepoId.value = null;
  } catch (cause) {
    repositoryDeleteError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    deletingRepositoryMode.value = null;
  }
}

export async function relocateMissingRepository(repoId: string, path: string) {
  emitSystemLogSilently("info", {
    category: "repository",
    action: "relocateStart",
    message: "开始重定向资源库。",
    repoId,
    context: { path },
  });
  const progressId = startOperationProgress("重定向资源库", "校验资源库位置", { initial: 12 });
  try {
    const response = await relocateRepository({ repoId, path });
    updateOperationProgress(progressId, { detail: "更新资源库状态", value: 64 });
    upsertRepositorySummary(response.repository);
    if (activeRepoId.value !== response.repository.repoId || !activeSnapshot.value) {
      await repositoryDependencies().selectRepository(response.repository.repoId);
    }
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function configureNeteaseRepositoryCacheInWorkspace(repoId: string, path: string) {
  const progressId = startOperationProgress("配置网易云缓存", "初始化缓存目录并迁移本机状态", { initial: 12 });
  try {
    const response = await configureNeteaseRepositoryCache({
      repoId,
      path,
      migrateLegacyCache: true,
    });
    updateOperationProgress(progressId, { detail: "更新资源库状态", value: 72 });
    upsertRepositorySummary(response.repository);
    if (activeRepoId.value !== response.repository.repoId || !activeSnapshot.value) {
      await repositoryDependencies().selectRepository(response.repository.repoId);
    }
    emitSystemLogSilently("info", {
      category: "repository",
      action: "relocateSuccess",
      message: "资源库位置已更新。",
      repoId: response.repository.repoId,
      context: { path: response.repository.path },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    emitSystemLogSilently("error", {
      category: "repository",
      action: "relocateFailed",
      message: "资源库位置更新失败。",
      repoId,
      context: { path, error: cause instanceof Error ? cause.message : String(cause) },
    });
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function exportCurrentRepository(
  request: Omit<RepositoryExportRequest, "repoId">,
): Promise<RepositoryExportResponse | null> {
  if (!activeRepoId.value) return null;

  emitSystemLogSilently("info", {
    category: "repository",
    action: "exportStart",
    message: request.target === "git" ? "开始导出到 Git。" : "开始导出资源库。",
    repoId: activeRepoId.value,
    context: { target: request.target },
  });
  error.value = null;
  const progressId = startOperationProgress(
    request.target === "git" ? "上传到 Git" : "导出资源库",
    request.target === "git" ? "准备提交并推送资源库" : "准备打包资源库文件",
    { initial: 8 },
  );

  try {
    const response = await exportRepository({
      ...request,
      repoId: activeRepoId.value,
    });
    updateOperationProgress(progressId, { detail: response.message, value: 92 });
    emitSystemLogSilently("info", {
      category: "repository",
      action: "exportSuccess",
      message: response.message,
      repoId: activeRepoId.value,
      context: { target: request.target, outputPath: response.outputPath ?? null },
    });
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    emitSystemLogSilently("error", {
      category: "repository",
      action: "exportFailed",
      message: "资源库导出失败。",
      repoId: activeRepoId.value,
      context: { target: request.target, error: error.value },
    });
    cancelOperationProgress(progressId);
    return null;
  }
}
