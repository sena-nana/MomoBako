import type { ExternalApiConnectionStatus } from "../../types/repository";
import { invokeCommand } from "./core";

export function getExternalApiConnectionStatus() {
  return invokeCommand<ExternalApiConnectionStatus>("get_external_api_connection_status");
}
