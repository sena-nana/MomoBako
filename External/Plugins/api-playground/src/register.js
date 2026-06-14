const FALLBACK_ENDPOINTS = [
  {
    group: "External Asset API",
    transport: "external-http",
    method: "GET",
    path: "/external/v1/health",
    summary: "检查外部 API 服务状态。",
    requiresAuth: false,
  },
  {
    group: "External Asset API",
    transport: "external-http",
    method: "GET",
    path: "/external/v1/repositories",
    summary: "列出可接收外部素材的本地仓库。",
    requiresAuth: true,
  },
  {
    group: "External Asset API",
    transport: "external-http",
    method: "POST",
    path: "/external/v1/assets:add",
    summary: "从远程 URL 添加素材到仓库。",
    requiresAuth: true,
    requestTemplate: {
      repoId: "",
      parentPath: "",
      client: {
        id: "momobako.api-playground",
        name: "API Playground",
        version: "0.1.0",
      },
      items: [
        {
          kind: "remoteUrl",
          url: "https://example.com/image.png",
          filename: "image.png",
          metadata: {
            sourceUrl: "https://example.com/image.png",
          },
        },
      ],
    },
  },
];

const TRANSPORT_FILTERS = [
  { value: "all", label: "全部" },
  { value: "external-http", label: "HTTP" },
  { value: "tauri-command", label: "Core" },
  { value: "plugin-call", label: "Plugin" },
];

const HTTP_METHODS = ["GET", "POST", "PATCH", "DELETE", "HEAD"];

export function register(ctx) {
  const {
    computed,
    h,
    onMounted,
    ref,
    watch,
  } = ctx.vue;

  const ApiPlayground = {
    name: "ApiPlayground",
    setup() {
      const status = ref("idle");
      const requestStatus = ref("idle");
      const errorMessage = ref("");
      const noticeMessage = ref("");
      const connection = ref(null);
      const apiDesign = ref(null);
      const endpoints = ref(FALLBACK_ENDPOINTS.map(normalizeEndpoint));
      const selectedEndpointKey = ref(endpointKey(endpoints.value[0]));
      const transportFilter = ref("all");
      const keyword = ref("");
      const method = ref("GET");
      const path = ref("/external/v1/health");
      const customPath = ref("");
      const requestText = ref("");
      const includeAuth = ref(false);
      const responseStatus = ref("");
      const responseHeaders = ref("");
      const responseBody = ref("");
      const durationMs = ref(null);

      const visibleEndpoints = computed(() => {
        const query = keyword.value.trim().toLowerCase();
        return endpoints.value.filter((endpoint) => {
          const transport = endpointTransport(endpoint);
          if (transportFilter.value !== "all" && transport !== transportFilter.value) {
            return false;
          }
          if (!query) return true;
          return [
            endpoint.group,
            endpoint.method,
            endpoint.path,
            endpoint.summary,
            endpoint.command,
            endpoint.pluginId,
            endpoint.pluginMethod,
          ]
            .filter(Boolean)
            .join(" ")
            .toLowerCase()
            .includes(query);
        });
      });
      const selectedEndpoint = computed(() => (
        endpoints.value.find((endpoint) => endpointKey(endpoint) === selectedEndpointKey.value) ?? endpoints.value[0]
      ));
      const selectedTransport = computed(() => endpointTransport(selectedEndpoint.value));
      const baseUrl = computed(() => connection.value?.baseUrl ?? "");
      const rootUrl = computed(() => baseUrl.value.replace(/\/external\/v1\/?$/, ""));
      const targetPath = computed(() => customPath.value.trim() || path.value);
      const requestUrl = computed(() => joinUrl(rootUrl.value, targetPath.value));
      const canSend = computed(() => {
        if (requestStatus.value === "loading" || !selectedEndpoint.value) return false;
        if (selectedTransport.value === "external-http") {
          return Boolean(rootUrl.value && method.value && targetPath.value);
        }
        if (selectedTransport.value === "tauri-command") {
          return Boolean(selectedEndpoint.value.command || selectedEndpoint.value.path);
        }
        if (selectedTransport.value === "plugin-call") {
          return Boolean(selectedEndpoint.value.pluginId && selectedEndpoint.value.pluginMethod);
        }
        return false;
      });
      const tokenPreview = computed(() => {
        const token = connection.value?.token ?? "";
        return token ? `${token.slice(0, 10)}...${token.slice(-6)}` : "未加载";
      });
      const requestSummary = computed(() => {
        const endpoint = selectedEndpoint.value;
        if (!endpoint) return "等待契约";
        return `${transportLabel(endpoint)} ${endpointTarget(endpoint, requestUrl.value)}`;
      });

      async function loadPlaygroundData() {
        status.value = "loading";
        errorMessage.value = "";
        noticeMessage.value = "";
        const [connectionResult, apiResult] = await Promise.allSettled([
          ctx.getExternalApiConnectionStatus(),
          ctx.getApiDesignSnapshot(),
        ]);

        if (connectionResult.status === "fulfilled") {
          connection.value = connectionResult.value;
        } else {
          connection.value = null;
          noticeMessage.value = `外部 API 连接未加载：${errorText(connectionResult.reason)}`;
        }

        if (apiResult.status === "fulfilled") {
          apiDesign.value = apiResult.value;
          const snapshotEndpoints = (apiResult.value?.endpoints ?? []).map(normalizeEndpoint);
          endpoints.value = snapshotEndpoints.length ? snapshotEndpoints : FALLBACK_ENDPOINTS.map(normalizeEndpoint);
          status.value = "ready";
        } else {
          apiDesign.value = null;
          endpoints.value = FALLBACK_ENDPOINTS.map(normalizeEndpoint);
          errorMessage.value = `API 契约加载失败：${errorText(apiResult.reason)}`;
          status.value = "error";
        }

        if (!endpoints.value.some((endpoint) => endpointKey(endpoint) === selectedEndpointKey.value)) {
          selectedEndpointKey.value = endpointKey(endpoints.value[0]);
        }
      }

      function applyEndpoint(endpoint) {
        method.value = endpoint.method;
        path.value = endpoint.path;
        customPath.value = "";
        includeAuth.value = Boolean(endpoint.requiresAuth);
        requestText.value = formatRequestTemplate(endpoint);
      }

      async function copyRequest() {
        noticeMessage.value = "";
        errorMessage.value = "";
        try {
          await navigator.clipboard.writeText(buildRequestSnippet({
            endpoint: selectedEndpoint.value,
            method: method.value,
            url: requestUrl.value,
            token: includeAuth.value ? connection.value?.token : "",
            requestText: requestText.value,
          }));
          noticeMessage.value = "请求已复制。";
        } catch (cause) {
          errorMessage.value = `复制失败：${errorText(cause)}`;
        }
      }

      async function sendRequest() {
        noticeMessage.value = "";
        errorMessage.value = "";
        responseStatus.value = "";
        responseHeaders.value = "";
        responseBody.value = "";
        durationMs.value = null;

        const endpoint = selectedEndpoint.value;
        if (!endpoint || !canSend.value) {
          errorMessage.value = "当前 API 尚未准备好。";
          return;
        }

        requestStatus.value = "loading";
        const startedAt = performance.now();
        try {
          let ok = true;
          if (selectedTransport.value === "external-http") {
            ok = await sendHttpRequest(endpoint);
          } else if (selectedTransport.value === "tauri-command") {
            ok = await sendTauriCommand(endpoint);
          } else if (selectedTransport.value === "plugin-call") {
            ok = await sendPluginCall(endpoint);
          } else {
            throw new Error(`unsupported transport: ${selectedTransport.value}`);
          }
          durationMs.value = Math.max(0, Math.round(performance.now() - startedAt));
          requestStatus.value = ok ? "success" : "error";
        } catch (cause) {
          durationMs.value = Math.max(0, Math.round(performance.now() - startedAt));
          errorMessage.value = errorText(cause);
          responseStatus.value = "ERROR";
          responseBody.value = formatResponsePayload({ error: errorText(cause) });
          requestStatus.value = "error";
        }
      }

      async function sendHttpRequest(endpoint) {
        const headers = new Headers();
        if (includeAuth.value && connection.value?.token) {
          headers.set("Authorization", `Bearer ${connection.value.token}`);
        }
        const init = { method: method.value, headers };
        if (method.value !== "GET" && method.value !== "HEAD") {
          const body = parseRequestJson(requestText.value, "请求 JSON 无效");
          headers.set("Content-Type", "application/json");
          init.body = JSON.stringify(body ?? {});
        }
        const response = await fetch(requestUrl.value, init);
        responseStatus.value = `${response.status} ${response.statusText}`.trim();
        responseHeaders.value = JSON.stringify(Object.fromEntries(response.headers.entries()), null, 2);
        const text = await response.text();
        responseBody.value = formatResponseText(text);
        return response.ok;
      }

      async function sendTauriCommand(endpoint) {
        if (typeof ctx.invokeCommand !== "function") {
          throw new Error("当前插件 SDK 未提供 invokeCommand。");
        }
        const command = endpoint.command || endpoint.path;
        const args = parseRequestJson(requestText.value, "命令参数 JSON 无效") ?? {};
        if (!isPlainObject(args)) {
          throw new Error("Tauri 命令参数必须是 JSON object。");
        }
        const payload = await ctx.invokeCommand(command, args);
        responseStatus.value = "OK";
        responseHeaders.value = JSON.stringify({
          transport: "tauri-command",
          command,
        }, null, 2);
        responseBody.value = formatResponsePayload(payload);
        return true;
      }

      async function sendPluginCall(endpoint) {
        const payload = parseRequestJson(requestText.value, "插件 Payload JSON 无效") ?? {};
        const response = await ctx.callPlugin({
          pluginId: endpoint.pluginId,
          method: endpoint.pluginMethod,
          payload,
        });
        responseStatus.value = "OK";
        responseHeaders.value = JSON.stringify({
          transport: "plugin-call",
          pluginId: endpoint.pluginId,
          method: endpoint.pluginMethod,
        }, null, 2);
        responseBody.value = formatResponsePayload(response);
        return true;
      }

      watch(selectedEndpoint, (endpoint) => {
        if (endpoint) applyEndpoint(endpoint);
      }, { immediate: true });

      watch(visibleEndpoints, (items) => {
        if (!items.length) return;
        if (!items.some((endpoint) => endpointKey(endpoint) === selectedEndpointKey.value)) {
          selectedEndpointKey.value = endpointKey(items[0]);
        }
      });

      onMounted(() => {
        void loadPlaygroundData();
      });

      return {
        apiDesign,
        baseUrl,
        canSend,
        connection,
        customPath,
        durationMs,
        endpoints,
        errorMessage,
        includeAuth,
        keyword,
        method,
        noticeMessage,
        path,
        requestStatus,
        requestSummary,
        requestText,
        requestUrl,
        responseBody,
        responseHeaders,
        responseStatus,
        rootUrl,
        selectedEndpoint,
        selectedEndpointKey,
        selectedTransport,
        status,
        tokenPreview,
        transportFilter,
        visibleEndpoints,
        copyRequest,
        loadPlaygroundData,
        sendRequest,
      };
    },
    render() {
      const endpointSource = this.visibleEndpoints.length ? this.visibleEndpoints : this.endpoints;
      const endpointOptions = endpointSource.map((endpoint) => h("option", {
        value: endpointKey(endpoint),
      }, `${transportShortLabel(endpoint)} ${endpoint.group} / ${endpoint.method} ${endpointLabel(endpoint)}`));
      const isHttp = this.selectedTransport === "external-http";
      const isHttpRead = isHttp && ["GET", "HEAD"].includes(this.method);

      return h("section", { class: "api-playground" }, [
        h("header", { class: "api-playground__header" }, [
          h("div", [
            h("p", { class: "asset-browser__eyebrow" }, "API Playground"),
            h("h1", "后端 API 调试"),
            h("p", { class: "api-playground__subline" }, this.requestSummary),
          ]),
          h("div", { class: "api-playground__actions" }, [
            h("button", {
              type: "button",
              class: "ghost",
              disabled: this.status === "loading",
              onClick: this.loadPlaygroundData,
            }, "刷新契约"),
            h("button", {
              type: "button",
              class: "primary",
              disabled: !this.canSend,
              onClick: this.sendRequest,
            }, this.requestStatus === "loading" ? "发送中" : "发送"),
          ]),
        ]),
        this.errorMessage
          ? h("div", { class: "asset-browser__state asset-browser__state--error" }, this.errorMessage)
          : null,
        this.noticeMessage
          ? h("div", { class: "asset-browser__state" }, this.noticeMessage)
          : null,
        h("div", { class: "api-playground__meta" }, [
          h("span", ["API ", h("strong", String(this.endpoints.length))]),
          h("span", ["Base URL ", h("strong", this.baseUrl || "未加载")]),
          h("span", ["Token ", h("strong", this.tokenPreview)]),
          h("span", ["契约 ", h("strong", this.apiDesign?.transport ?? "未加载")]),
        ]),
        h("div", { class: "api-playground__body" }, [
          h("section", { class: "api-playground__request" }, [
            h("div", { class: "api-playground__row api-playground__row--filters" }, [
              h("label", [
                h("span", "Transport"),
                h("select", {
                  value: this.transportFilter,
                  onChange: (event) => {
                    this.transportFilter = event.target.value;
                  },
                }, TRANSPORT_FILTERS.map((option) => h("option", { value: option.value }, option.label))),
              ]),
              h("label", [
                h("span", "Search"),
                h("input", {
                  value: this.keyword,
                  placeholder: "plugin / repository / command",
                  onInput: (event) => {
                    this.keyword = event.target.value;
                  },
                }),
              ]),
            ]),
            h("div", { class: "api-playground__row api-playground__row--endpoint" }, [
              h("label", [
                h("span", "Endpoint"),
                h("select", {
                  value: this.selectedEndpointKey,
                  onChange: (event) => {
                    this.selectedEndpointKey = event.target.value;
                  },
                }, endpointOptions),
              ]),
              h("label", [
                h("span", "Method"),
                h("select", {
                  value: this.method,
                  disabled: !isHttp,
                  onChange: (event) => {
                    this.method = event.target.value;
                  },
                }, (isHttp ? HTTP_METHODS : [this.method]).map((value) => h("option", { value }, value))),
              ]),
            ]),
            this.selectedEndpoint?.summary
              ? h("p", { class: "api-playground__endpoint-summary" }, this.selectedEndpoint.summary)
              : null,
            h("label", { class: "api-playground__field" }, [
              h("span", "Target"),
              h("input", {
                value: isHttp ? (this.customPath || this.path) : endpointTarget(this.selectedEndpoint, this.requestUrl),
                readOnly: !isHttp,
                placeholder: isHttp ? "/external/v1/health" : "",
                onInput: (event) => {
                  if (isHttp) this.customPath = event.target.value;
                },
              }),
            ]),
            isHttp
              ? h("label", { class: "api-playground__check" }, [
                h("input", {
                  type: "checkbox",
                  checked: this.includeAuth,
                  onChange: (event) => {
                    this.includeAuth = event.target.checked;
                  },
                }),
                h("span", "Authorization: Bearer token"),
              ])
              : null,
            h("label", { class: "api-playground__field api-playground__field--body" }, [
              h("span", "Request JSON"),
              h("textarea", {
                value: this.requestText,
                placeholder: "{ }",
                disabled: isHttpRead,
                onInput: (event) => {
                  this.requestText = event.target.value;
                },
              }),
            ]),
            h("div", { class: "api-playground__request-actions" }, [
              h("button", {
                type: "button",
                class: "ghost",
                disabled: !this.selectedEndpoint,
                onClick: this.copyRequest,
              }, "复制请求"),
              h("button", {
                type: "button",
                class: "primary",
                disabled: !this.canSend,
                onClick: this.sendRequest,
              }, this.requestStatus === "loading" ? "发送中" : "发送请求"),
            ]),
          ]),
          h("section", { class: "api-playground__response" }, [
            h("div", { class: "api-playground__response-head" }, [
              h("div", [
                h("span", "Response"),
                h("strong", this.responseStatus || "未发送"),
              ]),
              this.durationMs != null ? h("em", `${this.durationMs} ms`) : null,
            ]),
            h("div", { class: "api-playground__response-grid" }, [
              h("label", [
                h("span", "Headers"),
                h("textarea", {
                  readOnly: true,
                  value: this.responseHeaders,
                }),
              ]),
              h("label", [
                h("span", "Body"),
                h("textarea", {
                  readOnly: true,
                  value: this.responseBody,
                }),
              ]),
            ]),
          ]),
        ]),
      ]);
    },
  };

  ctx.registerToolPage({
    toolPageId: "momobako.tool.api-playground",
    label: "API Playground",
    description: "调试后端与插件 API",
    order: 10,
    component: ApiPlayground,
  });
}

function normalizeEndpoint(endpoint) {
  const transport = endpointTransport(endpoint);
  return {
    ...endpoint,
    transport,
    method: endpoint.method || (transport === "tauri-command" ? "INVOKE" : "GET"),
    path: endpoint.path || endpoint.command || endpoint.pluginMethod || "",
  };
}

function endpointTransport(endpoint) {
  if (endpoint?.transport) return endpoint.transport;
  if (endpoint?.pluginId || endpoint?.pluginMethod || endpoint?.method === "PLUGIN") return "plugin-call";
  if (endpoint?.command || endpoint?.method === "INVOKE") return "tauri-command";
  return "external-http";
}

function endpointKey(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "tauri-command") return `${transport}:${endpoint.command || endpoint.path}`;
  if (transport === "plugin-call") return `${transport}:${endpoint.pluginId}:${endpoint.pluginMethod}`;
  return `${transport}:${endpoint.method}:${endpoint.path}`;
}

function endpointLabel(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "tauri-command") return endpoint.command || endpoint.path;
  if (transport === "plugin-call") return `${endpoint.pluginId}:${endpoint.pluginMethod}`;
  return trimExternalPrefix(endpoint.path);
}

function endpointTarget(endpoint, requestUrl) {
  if (!endpoint) return "";
  const transport = endpointTransport(endpoint);
  if (transport === "external-http") return requestUrl || endpoint.path;
  if (transport === "tauri-command") return endpoint.command || endpoint.path;
  if (transport === "plugin-call") return `${endpoint.pluginId}:${endpoint.pluginMethod}`;
  return endpoint.path;
}

function transportLabel(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "external-http") return "HTTP";
  if (transport === "tauri-command") return "Core";
  if (transport === "plugin-call") return "Plugin";
  return transport;
}

function transportShortLabel(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "external-http") return "HTTP";
  if (transport === "tauri-command") return "CORE";
  if (transport === "plugin-call") return "PLUG";
  return String(transport).toUpperCase();
}

function joinUrl(rootUrl, path) {
  if (!rootUrl || !path) return "";
  if (/^https?:\/\//i.test(path)) return path;
  return `${rootUrl.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`;
}

function trimExternalPrefix(path) {
  return String(path ?? "").replace(/^\/external\/v1\/?/, "/");
}

function formatRequestTemplate(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "external-http" && ["GET", "HEAD"].includes(endpoint.method)) return "";
  const template = endpoint.requestTemplate ?? defaultRequestTemplate(endpoint);
  if (template == null) return "";
  return JSON.stringify(template, null, 2);
}

function defaultRequestTemplate(endpoint) {
  const transport = endpointTransport(endpoint);
  if (transport === "tauri-command") return {};
  if (transport === "plugin-call") return {};
  return null;
}

function parseRequestJson(text, label) {
  const trimmed = text.trim();
  if (!trimmed) return {};
  try {
    return JSON.parse(trimmed);
  } catch (cause) {
    throw new Error(`${label}：${errorText(cause)}`);
  }
}

function isPlainObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function formatResponsePayload(payload) {
  if (payload == null) return "";
  if (typeof payload === "string") return formatResponseText(payload);
  return JSON.stringify(payload, null, 2);
}

function formatResponseText(text) {
  if (!text) return "";
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function buildRequestSnippet({ endpoint, method, url, token, requestText }) {
  const transport = endpointTransport(endpoint);
  if (transport === "external-http") {
    return buildCurlCommand({ method, url, token, body: requestText });
  }
  if (transport === "tauri-command") {
    return `await invoke(${JSON.stringify(endpoint.command || endpoint.path)}, ${requestText.trim() || "{}"});`;
  }
  if (transport === "plugin-call") {
    return JSON.stringify({
      pluginId: endpoint.pluginId,
      method: endpoint.pluginMethod,
      payload: parseRequestJson(requestText, "插件 Payload JSON 无效"),
    }, null, 2);
  }
  return requestText;
}

function buildCurlCommand({ method, url, token, body }) {
  const lines = [`curl -X ${method} ${quoteShell(url)}`];
  if (token) lines.push(`  -H ${quoteShell(`Authorization: Bearer ${token}`)}`);
  if (method !== "GET" && method !== "HEAD") {
    lines.push(`  -H ${quoteShell("Content-Type: application/json")}`);
    if (body.trim()) lines.push(`  --data ${quoteShell(body.trim())}`);
  }
  return lines.join(" \\\n");
}

function quoteShell(value) {
  return `'${String(value).replace(/'/g, "'\"'\"'")}'`;
}

function errorText(cause) {
  return cause instanceof Error ? cause.message : String(cause);
}
