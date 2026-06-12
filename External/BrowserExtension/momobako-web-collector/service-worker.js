const CLIENT = {
  id: "momobako.external-browser-client",
  name: "MomoBako External Asset Client",
  version: "0.1.0",
};

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "momobako-add-image",
    title: "Add image to MomoBako",
    contexts: ["image"],
  });
  chrome.contextMenus.create({
    id: "momobako-add-link",
    title: "Add link target to MomoBako",
    contexts: ["link"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.srcUrl || info.linkUrl;
  if (!url) return;
  const settings = await getSettings();
  if (!settings.baseUrl || !settings.token || !settings.repoId) {
    await chrome.storage.local.set({ lastResult: "Configure MomoBako connection first." });
    return;
  }
  const metadata = {
    sourceUrl: url,
    originTitle: tab?.title || "",
    originReferrer: info.pageUrl || tab?.url || "",
  };
  const result = await addExternalAsset(settings, [{
    kind: "remoteUrl",
    url,
    filename: filenameFromUrl(url),
    metadata,
  }]);
  await chrome.storage.local.set({ lastResult: summarizeResult(result) });
});

async function getSettings() {
  return chrome.storage.local.get(["baseUrl", "token", "repoId", "parentPath"]);
}

async function addExternalAsset(settings, items) {
  const response = await fetch(`${settings.baseUrl.replace(/\/+$/, "")}/assets:add`, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${settings.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      repoId: settings.repoId,
      parentPath: settings.parentPath || "",
      client: CLIENT,
      items,
    }),
  });
  const payload = await response.json().catch(() => null);
  if (!response.ok && !payload?.status) {
    throw new Error(payload?.message || `MomoBako returned HTTP ${response.status}`);
  }
  return payload;
}

function filenameFromUrl(url) {
  try {
    const pathname = new URL(url).pathname;
    const filename = decodeURIComponent(pathname.split("/").filter(Boolean).pop() || "");
    return filename || undefined;
  } catch {
    return undefined;
  }
}

function summarizeResult(result) {
  if (!result) return "No response.";
  if (result.status === "success") return `Added ${result.summary.imported} asset(s).`;
  if (result.status === "partial") return `Added ${result.summary.imported}, failed ${result.summary.failed}.`;
  return result.failed?.[0]?.message || "Add failed.";
}
