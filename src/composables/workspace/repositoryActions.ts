import {
  listRepositoryActions,
  runRepositoryAction,
} from "../../services/repositoryApi";
import {
  activePanel,
  activeRepoId,
  activeRepositoryActionId,
  error,
  fileBrowser,
  isLoadingRepositoryActions,
  isRunningRepositoryAction,
  repositoryActions,
  selectedFilePath,
  selectedFilePaths,
} from "./state";
import { loadFileBrowserForDirectory } from "./files";
import { refreshWorkspaceAfterMutation } from "./refresh";

export async function refreshRepositoryActions(repoId = activeRepoId.value) {
  if (!repoId) {
    repositoryActions.value = [];
    activeRepositoryActionId.value = null;
    return [];
  }
  isLoadingRepositoryActions.value = true;
  error.value = null;
  try {
    const actions = await listRepositoryActions(repoId);
    if (activeRepoId.value === repoId) {
      repositoryActions.value = actions;
      if (activeRepositoryActionId.value && !actions.some((action) => action.actionId === activeRepositoryActionId.value)) {
        activeRepositoryActionId.value = null;
      }
      activeRepositoryActionId.value = activeRepositoryActionId.value ?? actions[0]?.actionId ?? null;
    }
    return actions;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return [];
  } finally {
    isLoadingRepositoryActions.value = false;
  }
}

export function selectRepositoryAction(actionId: string) {
  activeRepositoryActionId.value = actionId;
  activePanel.value = "actions";
}

export async function runActiveRepositoryAction(actionId = activeRepositoryActionId.value) {
  const repoId = activeRepoId.value;
  if (!repoId || !actionId) return null;
  const targetPaths = selectedFilePaths.value.length
    ? selectedFilePaths.value
    : selectedFilePath.value ? [selectedFilePath.value] : [];
  isRunningRepositoryAction.value = true;
  error.value = null;
  try {
    const response = await runRepositoryAction({
      repoId,
      actionId,
      targetPaths,
    });
    repositoryActions.value = repositoryActions.value.map((action) => (
      action.actionId === response.action.actionId ? response.action : action
    ));
    await refreshWorkspaceAfterMutation(repoId, {
      directory: fileBrowser.value && !fileBrowser.value.specialLocation ? "current" : undefined,
      repositorySnapshot: true,
    }, loadFileBrowserForDirectory);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isRunningRepositoryAction.value = false;
  }
}
