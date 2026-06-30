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
  RepositoryExportRequest,
  RepositoryExportResponse,
} from "../../types/repository";
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
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function importExistingRepository(name: string, path: string) {
  const progressId = startOperationProgress("导入资源库", "读取资源库元数据并扫描文件", { initial: 8 });
  try {
    const response = await importRepository({ name, path });
    upsertRepositorySummary(response.repository);
    await repositoryDependencies().selectRepository(response.repository.repoId);
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function attachRepository(path: string) {
  const progressId = startOperationProgress("挂载资源库", "检查文件夹并读取索引", { initial: 8 });
  try {
    const response = await attachRepositoryFolder({ path });
    upsertRepositorySummary(response.repository);
    await repositoryDependencies().selectRepository(response.repository.repoId);
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function removeRepository(repoId: string) {
  await deleteRepository(repoId);
  removeRepositorySummary(repoId);
  if (activeRepoId.value !== repoId) return;
  const nextRepoId = selectNextRepositoryAfterRemoval(repoId);
  if (nextRepoId) {
    await repositoryDependencies().selectRepository(nextRepoId);
  }
}

export async function relocateMissingRepository(repoId: string, path: string) {
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
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function exportCurrentRepository(
  request: Omit<RepositoryExportRequest, "repoId">,
): Promise<RepositoryExportResponse | null> {
  if (!activeRepoId.value) return null;

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
    finishOperationProgress(progressId);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    cancelOperationProgress(progressId);
    return null;
  }
}
