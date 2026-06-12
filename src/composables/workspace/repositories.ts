import {
  attachRepositoryFolder,
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
} from "./state";
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

export async function createNewRepository(
  name: string,
  path: string,
  backendPluginId?: string,
  backendConfig?: Record<string, unknown>,
) {
  const progressId = startOperationProgress("创建资源库", "初始化资源库并扫描文件", { initial: 8 });
  try {
    await createRepository({ name, path, backendPluginId, backendConfig });
    await repositoryDependencies().loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function importExistingRepository(name: string, path: string) {
  const progressId = startOperationProgress("导入资源库", "读取资源库元数据并扫描文件", { initial: 8 });
  try {
    await importRepository({ name, path });
    await repositoryDependencies().loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function attachRepository(path: string) {
  const progressId = startOperationProgress("挂载资源库", "检查文件夹并读取索引", { initial: 8 });
  try {
    await attachRepositoryFolder({ path });
    await repositoryDependencies().loadRepositories();
    finishOperationProgress(progressId);
  } catch (cause) {
    cancelOperationProgress(progressId);
    throw cause;
  }
}

export async function removeRepository(repoId: string) {
  await deleteRepository(repoId);
  await repositoryDependencies().loadRepositories();
}

export async function relocateMissingRepository(repoId: string, path: string) {
  const progressId = startOperationProgress("重定向资源库", "校验资源库位置", { initial: 12 });
  try {
    const response = await relocateRepository({ repoId, path });
    updateOperationProgress(progressId, { detail: "刷新资源库列表", value: 64 });
    await repositoryDependencies().loadRepositories();
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
