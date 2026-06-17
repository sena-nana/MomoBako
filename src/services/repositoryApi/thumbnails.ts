import type { ThumbnailRequest, ThumbnailResponse } from "../../types/repository";
import { invokeCommand } from "./core";

export function ensureThumbnail(request: ThumbnailRequest) {
  return invokeCommand<ThumbnailResponse>("ensure_thumbnail", { request });
}
