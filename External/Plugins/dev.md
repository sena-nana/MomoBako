# MomoBako 插件开发说明

## 1. 目标与边界

- 插件源码唯一根目录是 `External/Plugins/`
- 每个插件一个独立工程，独立编译，独立产物
- `External/Plugins/` 自带插件开发所需工具链与打包脚本
- `yarn tauri:dev` 每次都会完整构建、打包并暂存插件；主应用默认构建、`tauri build` 和 `verify` 不隐式编译插件
- 插件运行时目录固定为 `<serviceRoot>/plugins`
- 默认开发 staging 目录为 `src-tauri/.service-data/plugins`
- `<serviceRoot>/plugins` 只保存 `.momoplug` 文件，不做持久解压安装
- 宿主直接读取 `.momoplug` 内部的 `manifest.json`、前端 bundle、原生库和资源

## 2. 目录规范

插件工具链与主应用一致，固定使用 Node.js 26.5.0、Corepack 0.35.0 和 Yarn 4.17.1。首次在插件目录独立工作时执行：

```bash
npm install --global corepack@0.35.0
corepack enable
corepack yarn install --immutable
```

标准结构：

```text
External/Plugins/
  dev.md
  package.json
  scripts/
    build.mjs
    package.ts
    stage-dev.mjs
  _sdk/
    backend-rust/
  template/
    plugin.project.json
    backend-rust/
  example/
    plugin.project.json
    backend-rust/
  <plugin-slug>/
    manifest.json
    plugin.project.json
    src/
      register.js
```

打包后产物：

```text
External/Plugins/.dist/<plugin-slug>/
External/Plugins/.packages/<plugin-slug>-<version>.momoplug
```

## 3. Manifest v2

示例：

```json
{
  "pluginId": "momobako.example.text-preview",
  "name": "Example Text Preview",
  "version": "0.1.0",
  "type": {
    "layer": "library-kind",
    "kind": "preview"
  },
  "kind": "preview",
  "description": "Example preview plugin for text files.",
  "capabilities": ["preview", "text"],
  "enabled": true,
  "sdk": "frontend",
  "runtime": "vue-module",
  "entry": {
    "frontend": {
      "module": "dist/register.js",
      "export": "register"
    }
  },
  "contributes": {
    "preview": {
      "extensions": ["txt", "md"]
    }
  },
  "permissions": [],
  "source": "user",
  "status": "ready",
  "compat": {
    "sdkVersion": "1",
    "legacyPluginIds": []
  }
}
```

约束：

- `type.layer` 是新真相源
- 顶层 `kind` 保留为兼容字段
- `entry.frontend` / `entry.backend` / `entry.manifestOnly` 用于描述入口
- 宿主优先读取 `type`，兼容层再回落到 `kind`

## 4. 五层职责

- `source`
  - 负责“怎么列出和读到文件/目录”
  - 例如本地、WebDAV、云盘
- `library-kind`
  - 负责“这类资源怎么理解”
  - 例如字段 schema、默认视图、交互模式、候选整理规则
- `extractor-parser`
  - 负责“这个具体文件能解析出什么”
  - 例如音频标签、视频轨道、EPUB 目录、压缩包清单
- `provider-service`
  - 负责“去哪里补信息或做外部能力”
  - 例如搜索、元数据补完、下载、OCR、ASR、封面获取
- `integration-capability-hook`
  - 负责把能力挂进核心体验
  - 例如播放列表、PiP、继续观看、下载队列、批量整理

## 5. 前端插件接口

前端插件 bundle 默认导出 `register(ctx)`：

```js
export function register(ctx) {
  ctx.registerPreview({
    supportedExtensions: ["txt"],
    component: {
      name: "ExamplePreview",
      template: "<section>Example</section>"
    }
  });
}
```

`ctx` 当前提供：

- `manifest`
- `registerPreview(definition)`
- `registerPlaylistPlayer(definition)`
- `registerLibraryExtension(definition)`
- `registerToolPage(definition)`
- `registerSettingsPage(definition)`
- `defineLazyComponent(loader)`
- `loadModule(path)`
- `getApiDesignSnapshot()`
- `getExternalApiConnectionStatus()`
- `getPluginDataDirectory()`
- `getPluginConfig()`
- `setPluginConfigValue(key, value)`
- `deletePluginConfigValue(key)`
- `invokeCommand(command, args?)`
- `callPlugin(request)`
- `logger.debug/info/warn/error(message, options)`

`ctx.logger.*` 会自动补齐：

- `pluginId`
- `sourceKind: "frontend-plugin"`
- 插件名称作为 `sourceLabel`
- 当前前端模块路径作为 `location.modulePath` / `location.file`

`options` 推荐字段：

- `category`
- `action`
- `repoId`
- `context`
- 可选 `location`

后端插件可以通过 manifest 的 `contributes.apiTests[]` 声明可调试 API；每项使用 `method`、可选 `summary`、`payload` 或 `requestTemplate`。API Playground 会把这些声明和内置 provider / metadataDefaults 贡献点一起合并到宿主 API 快照中。

插件可以通过 manifest 的 `contributes.settings.fields[]` 声明配置 schema，并通过 `settingsPage` 声明设置页元信息。前端插件如需完全自定义 UI，可调用 `registerSettingsPage`；宿主会把它和 schema 表单放在同一个插件设置入口内。配置值由宿主写入插件自有数据目录下的 `config.json`，前端 SDK 的 key-value API 与插件管理面板共享同一份数据。

宿主加载流程：

1. `GET /plugins`
2. 读取插件 manifest
3. 对 `runtime: "vue-module"` 的插件，从 `.momoplug` 直接读取 `entry.frontend.module`
4. 把 bundle 文本转成可加载 URL
5. 动态 `import()` 模块
6. 执行 `register(ctx)`

前端 bundle 不进入主前端构建产物。

## 6. 后端插件 ABI

执行型后端插件只支持 Mutsuki ABI v2。`.momoplug` 必须同时包含 `manifest.json` 与 `plugin.toml`，两份清单的插件 ID 和版本必须一致；纯前端和 manifest-only 插件不需要 `plugin.toml`。

原生动态库及 companion artifacts 不会持久解压安装。MutsukiTauriHost 只会在运行期将通过路径与 SHA-256 校验的执行文件提取到按包内容哈希隔离的缓存目录。

后端调用 envelope 会带上插件数据目录、运行缓存目录和宿主从 `config.json` 读取的当前 key-value 配置快照。

后端插件日志通过 Mutsuki `DomainEvent` 进入观察协议，不再同步回调宿主。

`External/Plugins/_sdk/mutsuki-rust` 已提供：

```rust
write_host_log_silently(runtime, "info", "taskStarted", "开始处理任务。", serde_json::json!({
    "taskId": "demo-1"
}));
```

这个 helper 会自动补齐：

- `pluginId = runtime.plugin_id`
- `kind = "momobako.plugin.log"`
- 日志上下文序列化到领域事件 payload

宿主可通过 Mutsuki 观察协议把事件适配到日志中心。

## 7. `.momoplug` 包结构

`.momoplug` 物理格式是 zip，但产品、文档、安装器和宿主校验都只接受 `.momoplug` 扩展名。

包内要求：

- 必须包含单插件根目录
- 根目录下必须包含 `manifest.json`
- 执行型后端必须同时包含 Mutsuki `plugin.toml`；纯前端与 `manifest-only` 插件不需要伪造该文件
- 双清单的插件 ID 与版本必须一致
- 前端入口、后端入口、资源路径都使用包内相对路径
- `plugin.toml` 的主 artifact 与 companion artifacts 必须存在于包内，并携带匹配内容的规范小写 SHA-256

典型结构：

```text
example-text-preview-0.1.0.momoplug
  example-text-preview/
    manifest.json
    dist/register.js
    _sdk/README.md
```

执行型后端的产物由 `plugin.toml` 声明：

```toml
[manifest.artifact]
artifact_type = "abi"
path = "momobako_service_office_convert.dll"
sha256 = "sha256:<build 阶段生成的 64 位小写十六进制>"

[[manifest.artifact.companion_artifacts]]
path = "office-convert-helper.exe"
sha256 = "sha256:<build 阶段生成的 64 位小写十六进制>"
executable = true
role = "office-convert-helper"
```

需要额外 Cargo 构建的 companion 在 `plugin.project.json` 中使用相同包内路径：

```json
{
  "build": {
    "companionArtifacts": [
      {
        "manifestPath": "helper/Cargo.toml",
        "binaryName": "office-convert-helper",
        "path": "office-convert-helper.exe"
      }
    ]
  }
}
```

构建器仅刷新清单已经声明的文件哈希；打包器会再次校验相对路径、文件存在性和 SHA-256，不会隐式补入未声明产物。

安装动作本质上是：

1. 校验扩展名为 `.momoplug`
2. 读取包内 `manifest.json`
3. 复制包文件到 `<serviceRoot>/plugins`
4. 刷新插件索引

不会做持久解压。

## 8. 压缩包直读与临时执行策略

- manifest: 直接从 zip 读取
- 前端 bundle: 直接读取文本并在内存中动态加载；宿主会自动处理包内根目录前缀
- 后端动态库: 提取到宿主临时目录，按内容哈希缓存
- 插件更新后，缓存 key 会变化，旧缓存自动失效
- 宿主退出后，临时文件允许被清理

以下情况必须进入 `error` 或 `unavailable`，且宿主 UI 不能崩溃：

- 包损坏
- 缺失 `manifest.json`
- 缺失声明入口
- 非法相对路径或路径越界
- 动态库加载失败

## 9. 构建与调试

主线只保留可选辅助命令：

- `yarn plugins:build`
- `yarn plugins:package`
- `yarn plugins:stage:dev`
- `yarn tauri:dev`
- `yarn tauri:dev:with-plugins`（兼容别名）

推荐流程：

1. 进入 `External/Plugins/` 并执行 `yarn install`
2. 复制 `template/` 创建新插件工程
3. 编写 `manifest.json` 与 `src/register.js`
4. 回到主仓库根目录执行 `yarn tauri:dev`

`yarn tauri:dev` 会依次执行完整插件构建、打包、暂存和桌面端启动；`yarn tauri:dev:with-plugins` 保留为兼容别名。主仓库根的 `plugins:build`、`plugins:package`、`plugins:stage:dev` 只是对 `External/Plugins/package.json` 脚本的转发，方便单独排查构建阶段，不再承载插件工具链本体。
不传 `<serviceRoot>` 时，`stage:dev` 会把 `.momoplug` 复制到 `src-tauri/.service-data/plugins`；传入 `<serviceRoot>` 时仍复制到 `<serviceRoot>/plugins`。

`example/` 与 `template/` 也包含 `plugin.project.json`，可通过 `cd External/Plugins && yarn build example` 或 `yarn build template` 做定向构建验证；默认批量打包不会把它们产出到 `.packages/`。
如需验证完整链路，可继续执行 `yarn package example` 生成 `example-<version>.momoplug`，再用 `yarn stage:dev <serviceRoot>` 定向复制到某个运行时目录下的 `plugins/`。
双清单与 artifact 规则可用 `yarn test:package` 做定向验证。

## 10. 模板与示例

- `template/`
  - 复制即用
  - 包含最小前端预览插件骨架
- `template/backend-rust/`
  - 后端原生插件模板
  - 演示如何使用 `External/Plugins/_sdk/mutsuki-rust`
- `example/`
  - 演示从源码到 `.momoplug` 再到宿主发现和运行时加载的完整链路
- `example/backend-rust/`
  - 演示后端 `.momoplug` 的最小 Mutsuki ABI v2 插件工程

## 11. 兼容与版本

- `compat.sdkVersion` 当前为 `1`
- 旧顶层 `kind` 仍保留
- `legacyPluginIds` 为可选别名字段；当前官方插件默认留空，仅在明确需要宿主侧别名迁移时才使用
- 本轮不支持同一 `pluginId` 多版本并存
- 若 `<serviceRoot>/plugins` 中出现重复 `pluginId`，当前策略是拒绝安装第二个包
`plugin.project.json` 用于描述该独立工程的本地构建方式，例如：

```json
{
  "pluginId": "momobako.preview.media",
  "build": {
    "type": "frontend-module",
    "sourceDir": "src",
    "entry": "dist/register.js",
    "outputDir": ".dist/media-preview"
  }
}
```

常见 `build.type`：

- `frontend-module`
- `cargo-native`
- `manifest-only`
