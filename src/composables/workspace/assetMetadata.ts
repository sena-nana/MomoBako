import {
  redoLastRevision,
  undoLastRevision,
  updateAssetMetadata,
} from "../../services/repositoryApi";
import type { AssetDetail } from "../../types/repository";
import {
  activeAssetDetail,
  activeAssetId,
  activeRepoId,
  activeSnapshot,
  error,
  fileBrowser,
  isSavingMetadata,
} from "./state";

export async function saveAssetMetadata(metadata: Record<string, unknown>) {
  if (!activeRepoId.value || !activeAssetDetail.value) return null;

  isSavingMetadata.value = true;
  error.value = null;

  try {
    const response = await updateAssetMetadata({
      repoId: activeRepoId.value,
      assetId: activeAssetDetail.value.summary.assetId,
      expectedVersion: activeAssetDetail.value.summary.version,
      metadata,
      source: "desktop",
    });

    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  } finally {
    isSavingMetadata.value = false;
  }
}

export async function undoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await undoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

export async function redoAssetRevision() {
  if (!activeRepoId.value || !activeAssetId.value) return null;

  try {
    const response = await redoLastRevision({
      repoId: activeRepoId.value,
      assetId: activeAssetId.value,
    });
    applyAssetResponse(response);
    return response;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
    return null;
  }
}

function applyAssetResponse(response: { asset: AssetDetail }) {
  activeAssetDetail.value = response.asset;
  activeAssetId.value = response.asset.summary.assetId;
  const metadata = Object.fromEntries(response.asset.metadata.map((entry) => [entry.key, entry.value]));

  if (fileBrowser.value) {
    fileBrowser.value = {
      ...fileBrowser.value,
      entries: fileBrowser.value.entries.map((entry) => (
        entry.assetId === response.asset.summary.assetId
          ? {
              ...entry,
              metadata,
            }
          : entry
      )),
    };
  }

  if (!activeSnapshot.value) return;

  activeSnapshot.value = {
    ...activeSnapshot.value,
    assets: activeSnapshot.value.assets.map((asset) => (
      asset.assetId === response.asset.summary.assetId ? response.asset.summary : asset
    )),
    recentRevisionCount: activeSnapshot.value.recentRevisionCount + 1,
  };
}
