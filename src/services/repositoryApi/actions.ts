import type {
  RepositoryAction,
  RepositoryActionEnabledRequest,
  RepositoryActionMutationResponse,
  RepositoryActionRunRequest,
  RepositoryActionRunResponse,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function listRepositoryActions(repoId: string) {
  return invokeCommand<RepositoryAction[]>("list_repository_actions", { repoId });
}

export function getRepositoryAction(repoId: string, actionId: string) {
  return invokeCommand<RepositoryAction>("get_repository_action", { repoId, actionId });
}

export function setRepositoryActionEnabled(request: RepositoryActionEnabledRequest) {
  return invokeCommand<RepositoryActionMutationResponse>("set_repository_action_enabled", {
    request,
  });
}

export function runRepositoryAction(request: RepositoryActionRunRequest) {
  return invokeCommand<RepositoryActionRunResponse>("run_repository_action", { request });
}
