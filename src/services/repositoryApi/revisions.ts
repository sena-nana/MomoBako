import type {
  RevisionActionRequest,
  RevisionActionResponse,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function undoLastRevision(request: RevisionActionRequest) {
  return invokeCommand<RevisionActionResponse>("undo_last_revision", { request });
}

export function redoLastRevision(request: RevisionActionRequest) {
  return invokeCommand<RevisionActionResponse>("redo_last_revision", { request });
}
