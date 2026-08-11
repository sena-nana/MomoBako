<!-- Source 插件的宿主认证设置页，只渲染声明式认证能力。 -->
<script setup lang="ts">
import { computed, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { KeyRound, LoaderCircle, LogOut, RefreshCw } from "@lucide/vue";
import { useWorkspaceRepository } from "../composables/useRepositoryWorkspace";
import {
  callPlugin,
  configureSourceRepositoryCache,
  syncRepository,
  updateRepositoryBackendConfig,
} from "../services/repositoryApi";
import type { PluginManifest, RepositorySummary } from "../types/repository";

type QrSession = {
  sessionId?: string;
  unikey?: string;
  qrUrl?: string;
  qrurl?: string;
  qrImage?: string | null;
  qrimg?: string | null;
};

type AuthenticationResult = {
  code?: number;
  state?: string;
  message?: string | null;
  loggedIn?: boolean;
  loginExpired?: boolean;
  accountId?: string | number;
  credentialRef?: string;
  backendConfig?: Record<string, unknown>;
  account?: { id?: string | number; userName?: string | null } | null;
  profile?: { nickname?: string | null } | null;
};

const props = defineProps<{ manifest: PluginManifest }>();
const {
  repositories,
  createNewRepository,
  refreshRepositoryWorkspace,
  selectRepository,
} = useWorkspaceRepository();

const session = ref<QrSession | null>(null);
const targetRepositoryId = ref<string | null>(null);
const expectedAccountId = ref<string | null>(null);
const cachePath = ref("");
const busy = ref(false);
const message = ref("");
const error = ref("");
const statusByRepository = ref<Record<string, AuthenticationResult>>({});

const authentication = computed(() => props.manifest.contributes?.source?.authentication ?? null);
const sourceRepositories = computed(() => {
  const acceptedIds = new Set([
    props.manifest.pluginId,
    ...(props.manifest.legacyPluginIds ?? []),
    ...(props.manifest.compat?.legacyPluginIds ?? []),
  ]);
  return repositories.value.filter((repository) => acceptedIds.has(repository.backend.pluginId));
});
const sessionKey = computed(() => session.value?.sessionId ?? session.value?.unikey ?? "");
const qrImage = computed(() => session.value?.qrImage ?? session.value?.qrimg ?? null);
const requiresLocalCache = computed(() => (
  authentication.value?.repositoryProvisioning?.requiresLocalCache ?? false
));
const canPoll = computed(() => (
  Boolean(sessionKey.value)
  && (!requiresLocalCache.value || Boolean(cachePath.value.trim()))
  && !busy.value
));

function accountIdFrom(value: unknown) {
  const normalized = String(value ?? "").trim();
  return normalized || null;
}

function accountIdFromResult(result: AuthenticationResult) {
  return accountIdFrom(
    result.accountId
      ?? result.backendConfig?.accountId
      ?? result.account?.id,
  );
}

function repositoryName(result: AuthenticationResult, accountId: string) {
  return result.profile?.nickname
    || result.account?.userName
    || `${props.manifest.name} ${accountId}`;
}

function publicBackendConfig(result: AuthenticationResult, accountId: string) {
  const source = result.backendConfig ?? {};
  const safeConfig = Object.fromEntries(Object.entries(source).filter(([key]) => {
    const normalized = key.toLowerCase();
    return !normalized.includes("cookie")
      && !normalized.includes("password")
      && !normalized.includes("secret")
      && normalized !== "token";
  }));
  const credentialRef = accountIdFrom(result.credentialRef ?? safeConfig.credentialRef);
  if (!credentialRef) {
    throw new Error("Source 未返回安全凭据引用，已拒绝保存登录配置。");
  }
  const provisioning = authentication.value?.repositoryProvisioning;
  return {
    ...safeConfig,
    accountId,
    credentialRef,
    sourceUri: `${provisioning?.sourceUriScheme ?? props.manifest.kind}://account/${accountId}`,
    ...(cachePath.value.trim() ? { localCachePath: cachePath.value.trim() } : {}),
    lastSyncAt: new Date().toISOString(),
  };
}

function resetNotice() {
  message.value = "";
  error.value = "";
}

/** 查询认证状态时只传仓库 ID，后端配置由宿主注入。 */
async function refreshStatus(repository: RepositorySummary) {
  const contribution = authentication.value;
  if (!contribution || busy.value) return;
  busy.value = true;
  resetNotice();
  try {
    const response = await callPlugin<AuthenticationResult>({
      pluginId: props.manifest.pluginId,
      method: contribution.statusMethod,
      repositoryId: repository.repoId,
      payload: {},
    });
    statusByRepository.value = {
      ...statusByRepository.value,
      [repository.repoId]: response.payload ?? {},
    };
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    error.value = reason;
    console.error("source authentication status failed", { pluginId: props.manifest.pluginId, reason });
  } finally {
    busy.value = false;
  }
}

/** 创建新的 Source 扫码会话，重登录时保留原仓库缓存路径。 */
async function beginAuthentication(repository: RepositorySummary | null = null) {
  const contribution = authentication.value;
  if (!contribution || busy.value) return;
  busy.value = true;
  resetNotice();
  session.value = null;
  targetRepositoryId.value = repository?.repoId ?? null;
  cachePath.value = requiresLocalCache.value
    ? repository?.localCache?.path ?? ""
    : repository?.path ?? "";
  expectedAccountId.value = null;
  try {
    if (repository) {
      const statusResponse = await callPlugin<AuthenticationResult>({
        pluginId: props.manifest.pluginId,
        method: contribution.statusMethod,
        repositoryId: repository.repoId,
        payload: {},
      });
      expectedAccountId.value = accountIdFromResult(statusResponse.payload ?? {});
    }
    const response = await callPlugin<QrSession>({
      pluginId: props.manifest.pluginId,
      method: contribution.createSessionMethod,
      payload: { qrImage: true, qrimg: true },
    });
    session.value = response.payload ?? null;
    message.value = "请扫码并在手机端确认，然后检查登录结果。";
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    error.value = reason;
    console.error("source authentication session failed", { pluginId: props.manifest.pluginId, reason });
  } finally {
    busy.value = false;
  }
}

function cancelAuthentication() {
  if (busy.value) return;
  session.value = null;
  targetRepositoryId.value = null;
  expectedAccountId.value = null;
  cachePath.value = "";
  resetNotice();
}

async function chooseCachePath() {
  if (busy.value) return;
  const selected = await open({
    directory: true,
    multiple: false,
    title: `选择${props.manifest.name}缓存目录`,
  });
  if (typeof selected === "string" && selected.trim()) cachePath.value = selected.trim();
}

/** 登录成功后原子创建或更新仓库，再在后台触发首次同步。 */
async function pollAuthentication() {
  const contribution = authentication.value;
  if (!contribution || !canPoll.value) return;
  busy.value = true;
  resetNotice();
  try {
    const response = await callPlugin<AuthenticationResult>({
      pluginId: props.manifest.pluginId,
      method: contribution.pollSessionMethod,
      payload: {
        key: sessionKey.value,
        sessionId: sessionKey.value,
        persistSession: true,
      },
    });
    const result = response.payload ?? {};
    const accountId = accountIdFromResult(result);
    if (!accountId || !(result.loggedIn || result.backendConfig || result.credentialRef)) {
      message.value = result.message || "尚未完成确认，请稍后重新检查。";
      return;
    }
    if (expectedAccountId.value && expectedAccountId.value !== accountId) {
      throw new Error("扫码账号与当前仓库不一致，请使用原账号重新登录。");
    }

    const provisioning = contribution.repositoryProvisioning;
    const repoId = targetRepositoryId.value
      ?? `${provisioning?.repoIdPrefix ?? props.manifest.kind}-${accountId}`;
    const backendConfig = publicBackendConfig(result, accountId);
    const existing = sourceRepositories.value.find((item) => item.repoId === repoId);
    if (existing) {
      await updateRepositoryBackendConfig({ repoId, backendConfig });
      if (requiresLocalCache.value && cachePath.value.trim()) {
        await configureSourceRepositoryCache({
          repoId,
          path: cachePath.value.trim(),
          migrateLegacyCache: true,
        });
      }
      await refreshRepositoryWorkspace();
      await selectRepository(repoId);
      message.value = `已更新 ${existing.name} 的登录状态，正在同步。`;
    } else {
      const name = repositoryName(result, accountId);
      const repositoryPath = cachePath.value.trim()
        || `${provisioning?.sourceUriScheme ?? props.manifest.kind}://account/${accountId}`;
      await createNewRepository(
        name,
        repositoryPath,
        props.manifest.pluginId,
        backendConfig,
        repoId,
        { skipInitialSync: true },
      );
      message.value = `已创建 ${name}，正在同步。`;
    }
    session.value = null;
    void syncRepository({ repoId })
      .then(() => refreshRepositoryWorkspace())
      .catch((cause) => {
        const reason = cause instanceof Error ? cause.message : String(cause);
        error.value = `后台同步失败：${reason}`;
        console.error("source repository sync failed", { pluginId: props.manifest.pluginId, repoId, reason });
      });
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    error.value = reason;
    console.error("source authentication provisioning failed", { pluginId: props.manifest.pluginId, reason });
  } finally {
    busy.value = false;
  }
}

async function clearAuthentication(repository: RepositorySummary) {
  const contribution = authentication.value;
  if (!contribution || busy.value) return;
  busy.value = true;
  resetNotice();
  try {
    const status = statusByRepository.value[repository.repoId]
      ?? (await callPlugin<AuthenticationResult>({
        pluginId: props.manifest.pluginId,
        method: contribution.statusMethod,
        repositoryId: repository.repoId,
        payload: {},
      })).payload
      ?? {};
    await callPlugin({
      pluginId: props.manifest.pluginId,
      method: contribution.clearMethod,
      repositoryId: repository.repoId,
      payload: {},
    });
    const accountId = accountIdFromResult(status);
    if (accountId && status.credentialRef) {
      await updateRepositoryBackendConfig({
        repoId: repository.repoId,
        backendConfig: {
          ...publicBackendConfig(status, accountId),
          loginExpired: true,
        },
      });
      await refreshRepositoryWorkspace();
    }
    statusByRepository.value = {
      ...statusByRepository.value,
      [repository.repoId]: { loggedIn: false, loginExpired: true },
    };
    message.value = `已退出 ${repository.name}，仓库和缓存仍保留。`;
  } catch (cause) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    error.value = reason;
    console.error("source authentication clear failed", { pluginId: props.manifest.pluginId, reason });
  } finally {
    busy.value = false;
  }
}

function statusLabel(repository: RepositorySummary) {
  const status = statusByRepository.value[repository.repoId] ?? repository.authentication;
  if (!status) return "未检查";
  if (status.loggedIn && !status.loginExpired) return "已登录";
  return "登录失效";
}
</script>

<template>
  <section class="source-auth-settings">
    <div class="source-auth-settings__head">
      <div>
        <strong>账号与仓库</strong>
        <p>认证由 Source 插件处理，宿主只保存安全凭据引用。</p>
      </div>
      <button type="button" class="primary" :disabled="busy" @click="beginAuthentication(null)">
        <KeyRound :size="14" aria-hidden="true" />
        连接新账号
      </button>
    </div>

    <div v-if="sourceRepositories.length" class="source-auth-settings__repositories">
      <div v-for="repository in sourceRepositories" :key="repository.repoId" class="source-auth-settings__repository">
        <div>
          <strong>{{ repository.name }}</strong>
          <span>{{ statusLabel(repository) }} · {{ repository.localCache?.path ?? repository.path }}</span>
        </div>
        <div class="source-auth-settings__actions">
          <button type="button" class="ghost" :disabled="busy" @click="refreshStatus(repository)">
            <RefreshCw :size="13" aria-hidden="true" />
            检查
          </button>
          <button type="button" class="ghost" :disabled="busy" @click="beginAuthentication(repository)">重新登录</button>
          <button type="button" class="ghost danger" :disabled="busy" @click="clearAuthentication(repository)">
            <LogOut :size="13" aria-hidden="true" />
            退出
          </button>
        </div>
      </div>
    </div>

    <div v-if="session" class="source-auth-settings__flow">
      <button v-if="requiresLocalCache" type="button" class="ghost" :disabled="busy" @click="chooseCachePath">
        {{ cachePath ? "重新选择缓存目录" : "选择缓存目录" }}
      </button>
      <p v-if="cachePath" class="source-auth-settings__path">{{ cachePath }}</p>
      <img v-if="qrImage" :src="qrImage" alt="扫码登录二维码" class="source-auth-settings__qr" />
      <div class="source-auth-settings__actions">
        <button type="button" class="ghost" :disabled="busy" @click="cancelAuthentication">
          取消
        </button>
        <button type="button" class="ghost" :disabled="busy" @click="beginAuthentication(targetRepositoryId ? sourceRepositories.find((item) => item.repoId === targetRepositoryId) ?? null : null)">
          刷新二维码
        </button>
        <button type="button" class="primary" :disabled="!canPoll" @click="pollAuthentication">
          <LoaderCircle v-if="busy" :size="14" class="spin" aria-hidden="true" />
          检查登录结果
        </button>
      </div>
    </div>

    <p v-if="error" class="source-auth-settings__notice source-auth-settings__notice--error">{{ error }}</p>
    <p v-else-if="message" class="source-auth-settings__notice">{{ message }}</p>
  </section>
</template>

<style scoped>
.source-auth-settings,
.source-auth-settings__repositories,
.source-auth-settings__flow {
  display: grid;
  gap: 12px;
}

.source-auth-settings__head,
.source-auth-settings__repository,
.source-auth-settings__actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.source-auth-settings__head p,
.source-auth-settings__repository span,
.source-auth-settings__path,
.source-auth-settings__notice {
  margin: 4px 0 0;
  color: var(--text-muted);
  font-size: 12px;
}

.source-auth-settings__repository,
.source-auth-settings__flow {
  padding: 12px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-md);
  background: var(--bg-subtle);
}

.source-auth-settings__repository > div:first-child {
  display: grid;
  gap: 2px;
  min-width: 0;
}

.source-auth-settings__actions {
  justify-content: flex-end;
  flex-wrap: wrap;
}

.source-auth-settings__path {
  overflow-wrap: anywhere;
}

.source-auth-settings__qr {
  width: 180px;
  height: 180px;
  justify-self: center;
  border-radius: var(--radius-sm);
  background: white;
}

.source-auth-settings__notice--error {
  color: var(--err);
}
</style>
