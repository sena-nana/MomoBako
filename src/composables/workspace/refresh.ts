import {
  getRepositorySnapshot,
  getRepositoryTree,
  listHardlinkCandidates,
  listRepositories,
} from "../../services/repositoryApi";
import {
  activeRepoId,
  activeSnapshot,
  currentDirectoryPath,
  fileTree,
  hardlinkCandidates,
  repositories,
} from "./state";

export type WorkspaceRefreshPlan = {
  directory?: "current" | "currentWithTree" | "trash";
  hardlinkCandidates?: boolean;
  repositorySnapshot?: boolean;
  repositorySummary?: boolean;
};

export async function refreshRepositorySummaries() {
  const items = await listRepositories();
  repositories.value = items;
}

export async function refreshHardlinkCandidates(repoId = activeRepoId.value) {
  if (!repoId) {
    hardlinkCandidates.value = [];
    return;
  }
  const response = await listHardlinkCandidates(repoId);
  if (activeRepoId.value === repoId) {
    hardlinkCandidates.value = response.candidates;
  }
}

export async function refreshRepositorySnapshot(repoId: string) {
  const snapshot = await getRepositorySnapshot(repoId);
  activeSnapshot.value = snapshot;
}

export async function refreshRepositoryTree(repoId: string) {
  const snapshot = await getRepositoryTree(repoId);
  if (activeRepoId.value === repoId) {
    fileTree.value = snapshot.tree;
  }
}

export async function refreshWorkspaceAfterMutation(
  repoId: string,
  plan: WorkspaceRefreshPlan,
  loadDirectory: (directoryPath: string, options?: { includeTree?: boolean; specialLocation?: "trash" }) => Promise<unknown>,
) {
  const tasks: Array<Promise<unknown>> = [];
  if (plan.repositorySnapshot) {
    tasks.push(refreshRepositorySnapshot(repoId));
  }
  if (plan.repositorySummary) {
    tasks.push(refreshRepositorySummaries());
  }
  if (plan.hardlinkCandidates) {
    tasks.push(refreshHardlinkCandidates(repoId));
  }
  if (plan.directory === "currentWithTree") {
    tasks.push(refreshRepositoryTree(repoId));
  }
  await Promise.all(tasks);

  if (plan.directory === "current") {
    await loadDirectory(currentDirectoryPath.value, { includeTree: false });
  } else if (plan.directory === "currentWithTree") {
    await loadDirectory(currentDirectoryPath.value, { includeTree: false });
  } else if (plan.directory === "trash") {
    await loadDirectory(currentDirectoryPath.value, { specialLocation: "trash" });
  }
}
