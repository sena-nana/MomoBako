import type {
  MetadataUpdateRequest,
  MetadataUpdateResponse,
  SearchRequest,
  SearchResponse,
} from "../../types/repository";
import { invokeCommand } from "./core";

export function searchAssets(request: SearchRequest) {
  return invokeCommand<SearchResponse>("search_assets", { request });
}

export function updateAssetMetadata(request: MetadataUpdateRequest) {
  return invokeCommand<MetadataUpdateResponse>("update_asset_metadata", { request });
}
