import { fireEvent, render, screen, waitFor } from "@testing-library/vue";
import { beforeEach, describe, expect, it } from "vitest";
import SourceAuthenticationSettings from "../src/components/SourceAuthenticationSettings.vue";
import {
  resetRepositoryWorkspaceForTests,
  useRepositoryWorkspace,
} from "../src/composables/useRepositoryWorkspace";
import type { PluginManifest } from "../src/types/repository";
import {
  getInvokeCalls,
  getPluginCallCalls,
  mockPluginCallResponse,
  seedMockRepositories,
  selectMockFolder,
} from "./setupTests";
import { pluginManifest, type MockRepository } from "./fixtures/repositoryFixtures";

/** 构造只声明认证协议、没有自定义前端页面的 Source manifest。 */
function sourceManifest(): PluginManifest {
  const manifest = pluginManifest(
    "momobako.netease.source",
    ["momobako.source.netease-cloud-music"],
    "Netease Cloud Music Source",
    "0.2.0",
    "source",
    "netease-cloud-music",
    "网易云音乐来源。",
    ["browse", "sync", "authentication"],
    true,
    "backend",
    "native-dylib",
  );
  manifest.contributes = {
    source: {
      authentication: {
        kind: "qr",
        createSessionMethod: "auth.createQrSession",
        pollSessionMethod: "auth.pollQrSession",
        statusMethod: "auth.getLoginStatus",
        clearMethod: "auth.clearLogin",
        repositoryProvisioning: {
          sourceUriScheme: "netease-cloud-music",
          repoIdPrefix: "netease-cloud-music",
          requiresLocalCache: true,
        },
      },
    },
  };
  return manifest;
}

function repository(localCacheStatus: "ready" | "missing" | "unconfigured" = "ready"): MockRepository {
  return {
    repoId: "netease-cloud-music-123456",
    name: "云村 Aura",
    path: localCacheStatus === "ready" ? "C:/Mock/NeteaseCache" : "netease-cloud-music://account/123456",
    backend: {
      pluginId: "momobako.netease.source",
      kind: "netease-cloud-music",
      name: "Netease Cloud Music Source",
      capabilities: ["browse", "sync", "authentication"],
    },
    status: localCacheStatus === "ready" ? "ready" : "missing",
    assetCount: 2,
    updatedAt: "2026-08-11T00:00:00Z",
    localCache: {
      required: true,
      path: localCacheStatus === "ready" ? "C:/Mock/NeteaseCache" : null,
      status: localCacheStatus,
    },
    authentication: {
      required: true,
      loggedIn: true,
      loginExpired: false,
    },
  };
}

async function loadRepositories(items: MockRepository[]) {
  resetRepositoryWorkspaceForTests();
  seedMockRepositories(items);
  await useRepositoryWorkspace().ensureRepositoryWorkspace();
}

function mockQrSuccess() {
  mockPluginCallResponse("momobako.netease.source", "auth.createQrSession", {
    unikey: "qr-key-1",
    qrimg: "data:image/svg+xml;base64,bW9jaw==",
  });
  mockPluginCallResponse("momobako.netease.source", "auth.pollQrSession", {
    code: 803,
    credentialRef: "keyring:momobako.netease.source:123456",
    backendConfig: {
      accountId: "123456",
      credentialRef: "keyring:momobako.netease.source:123456",
    },
    account: { id: 123456, userName: "Aura" },
    profile: { nickname: "云村 Aura" },
  });
}

describe("Source authentication settings", () => {
  beforeEach(async () => {
    await loadRepositories([]);
  });

  it("creates a repository from a successful QR login without exposing Cookie", async () => {
    mockQrSuccess();
    selectMockFolder("C:/Mock/NeteaseCache");
    render(SourceAuthenticationSettings, { props: { manifest: sourceManifest() } });

    await fireEvent.click(screen.getByRole("button", { name: "连接新账号" }));
    expect(await screen.findByAltText("扫码登录二维码")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "选择缓存目录" }));
    await fireEvent.click(screen.getByRole("button", { name: "检查登录结果" }));

    await waitFor(() => {
      const request = getInvokeCalls("create_repository").at(-1)?.args?.request;
      expect(request).toMatchObject({
        repoId: "netease-cloud-music-123456",
        path: "C:/Mock/NeteaseCache",
        backendPluginId: "momobako.netease.source",
        backendConfig: {
          accountId: "123456",
          credentialRef: "keyring:momobako.netease.source:123456",
        },
      });
      expect(request?.backendConfig).not.toHaveProperty("cookie");
      expect(request?.backendConfig).not.toHaveProperty("accountCookie");
    });
  });

  it("cancels an unfinished QR session without creating a repository", async () => {
    mockQrSuccess();
    render(SourceAuthenticationSettings, { props: { manifest: sourceManifest() } });

    await fireEvent.click(screen.getByRole("button", { name: "连接新账号" }));
    expect(await screen.findByAltText("扫码登录二维码")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "取消" }));

    expect(screen.queryByAltText("扫码登录二维码")).not.toBeInTheDocument();
    expect(getInvokeCalls("create_repository")).toHaveLength(0);
  });

  it("reuses the existing repository and cache during re-login", async () => {
    await loadRepositories([repository()]);
    mockQrSuccess();
    mockPluginCallResponse("momobako.netease.source", "auth.getLoginStatus", {
      loggedIn: true,
      accountId: "123456",
      credentialRef: "keyring:momobako.netease.source:123456",
    });
    render(SourceAuthenticationSettings, { props: { manifest: sourceManifest() } });

    await fireEvent.click(screen.getByRole("button", { name: "重新登录" }));
    await fireEvent.click(await screen.findByRole("button", { name: "检查登录结果" }));

    await waitFor(() => {
      expect(getInvokeCalls("update_repository_backend_config").at(-1)?.args).toMatchObject({
        request: { repoId: "netease-cloud-music-123456" },
      });
    });
    expect(getInvokeCalls("create_repository")).toHaveLength(0);
  });

  it("marks an expired login and preserves repository data on logout", async () => {
    const expiredRepository = repository();
    expiredRepository.authentication = {
      required: true,
      loggedIn: false,
      loginExpired: true,
    };
    await loadRepositories([expiredRepository]);
    mockPluginCallResponse("momobako.netease.source", "auth.getLoginStatus", {
      loggedIn: false,
      loginExpired: true,
      accountId: "123456",
      credentialRef: "keyring:momobako.netease.source:123456",
    });
    mockPluginCallResponse("momobako.netease.source", "auth.clearLogin", { cleared: true });
    render(SourceAuthenticationSettings, { props: { manifest: sourceManifest() } });

    expect(await screen.findByText(/登录失效/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "检查" }));
    expect(await screen.findByText(/登录失效/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "退出" }));

    await waitFor(() => {
      expect(getPluginCallCalls("momobako.netease.source", "auth.clearLogin")).toHaveLength(1);
      expect(getInvokeCalls("update_repository_backend_config").at(-1)?.args).toMatchObject({
        request: {
          repoId: "netease-cloud-music-123456",
          backendConfig: { loginExpired: true },
        },
      });
    });
    expect(getInvokeCalls("delete_repository")).toHaveLength(0);
  });
});
