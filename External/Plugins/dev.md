# MomoBako 插件开发说明

## 1. 目标与边界

- 插件源码唯一根目录是 `External/Plugins/`
- 每个插件一个独立工程，产出独立的 `.momoplug`；Rust 插件都是仓库根 Cargo workspace 的成员，共用根 `Cargo.lock` 与 `target/`
- 新建 Rust 插件要把目录加进根 `Cargo.toml` 的 `workspace.members`，插件目录里不放 `Cargo.lock`；`template/backend-rust/` 与 `example/backend-rust/` 是示例，不在 workspace 里
- `External/Plugins/` 自带插件开发所需工具链与打包脚本
- `yarn tauri:dev` 每次都会完整构建、打包并暂存插件；主应用默认构建、`tauri build` 和 `verify` 不隐式编译插件，也不要求已有插件包；`.packages/*.momoplug` 只经 `src-tauri/tauri.external-plugins.conf.json` 由 `yarn tauri:build`、`yarn tauri:dev` 带进 Tauri 安装包或开发窗口的资源目录；发布流程由 `yarn nana:package` 放进 Nana 发布包的 `runtime/plugins`
- 插件运行时目录固定为 `<serviceRoot>/plugins`
- 默认开发 staging 目录为 `src-tauri/.service-data/plugins`
- `<serviceRoot>/plugins` 只保存 `.momoplug` 文件，不做持久解压安装
- 宿主直接读取 `.momoplug` 内部的 `manifest.json`、`plugin.toml`、原生库和资源
- 插件只有后端形态：`native-dylib`（进程内加载的 Mutsuki ABI 动态库，只给随附插件用）、`process`（独立进程里的 Mutsuki runner）、`manifest-only`（只有声明，不执行任何代码）
- 插件不带界面代码。预览、播放器、工具页、设置表单和库类型面板都由宿主原生绘制，插件只提供清单里的声明；合同见 `docs/api-design.md` 的 Plugin API

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
    plugin-package-manifest.ts
    stage-dev.mjs
  _sdk/
    mutsuki-rust/
  template/
    backend-rust/
  example/
    backend-rust/
  <plugin-slug>/
    manifest.json
    plugin.project.json
    plugin.toml          # 执行型后端（native-dylib、process）才有
    Cargo.toml           # Rust 插件才有
    src/
      lib.rs
```

打包后产物：

```text
External/Plugins/.dist/<plugin-slug>/
External/Plugins/.packages/<plugin-slug>-<version>-<目标三元组或 any>.momoplug
```

## 3. Manifest v2

示例：

```json
{
  "pluginId": "momobako.example.backend-ping",
  "name": "Example Backend Ping",
  "version": "0.1.0",
  "type": {
    "layer": "provider-service",
    "kind": "service"
  },
  "kind": "service",
  "description": "Example backend plugin for ping-style service calls.",
  "capabilities": ["service", "ping"],
  "enabled": true,
  "sdk": "backend",
  "runtime": "native-dylib",
  "entry": {
    "backend": {
      "library": "momobako_example_backend_ping"
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
- `sdk` 只能是 `backend`；`runtime` 只能是 `native-dylib`、`process` 或 `manifest-only`。别的形态（包括原来的前端插件 `sdk: "frontend"`、`runtime: "vue-module"`）宿主读包时拒绝，插件管理里显示为读取失败的插件
- 入口：`native-dylib` 写 `entry.backend.library`，`process` 写 `entry.backend.executable`，`manifest-only` 的 `entry` 是 `{}`
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

## 5. 宿主读取的声明

插件通过 manifest 的 `contributes` 把能力交给宿主，宿主按声明原生绘制和调用。各字段的完整合同在 `docs/api-design.md` 的 Plugin API：

- `settings`：`fields[]` 声明配置 schema，`settingsPage` 给出插件设置区的标题和说明。插件管理面板按字段画表单，配置值由宿主写入插件自有数据目录下的 `config.json`，后端调用时作为 `runtime.pluginConfig` 交给插件。插件不能自带设置界面
- `apiTests[]`：后端插件声明可调试的方法，每项使用 `method`、可选 `summary`、`payload` 或 `requestTemplate`。API Playground 会把这些声明和 provider / metadataDefaults 贡献点一起合并到宿主 API 快照中
- `toolPages[]`：把宿主内置的工具页（文件导入、Eagle 导入、API Playground）放到拓展页上。插件不能自带页面，宿主没有的 `toolPageId` 只画页头和说明
- `playlistPlayers[]`：登记播放集的播放类型，宿主用自己的解码器播放
- `libraryKind`、`libraryExtension`：库类型的字段、视图、整理规则和筛选栏快捷方式
- `source`：来源插件的操作、元数据镜像、认证、媒体和条目动作
- `metadataDefaults`、`provider.lookup`：同步时补默认元数据、手动查询元数据候选

## 6. 后端插件 ABI

执行型后端插件只支持 Mutsuki ABI v2。`.momoplug` 必须同时包含 `manifest.json` 与 `plugin.toml`，两份清单的插件 ID 和版本必须一致；`manifest-only` 插件不需要 `plugin.toml`。

原生动态库及 companion artifacts 不会持久解压安装。宿主（`momobako_backend::services::mutsuki_host`）只会在运行期将通过路径与 SHA-256 校验的执行文件提取到按包内容哈希隔离的缓存目录。

后端调用 envelope 会带上插件数据目录、运行缓存目录和宿主从 `config.json` 读取的当前 key-value 配置快照。

后端插件日志作为 Mutsuki `DomainEvent` 随这次调用的结果交回，不再同步回调宿主。

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

宿主收下调用结果时把这些事件写进系统日志（`src-backend/src/services/plugin_events.rs`），日志页按插件筛选。

## 7. `.momoplug` 包结构

`.momoplug` 物理格式是 zip，但产品、文档、安装器和宿主校验都只接受 `.momoplug` 扩展名。

包内要求：

- 必须包含单插件根目录
- 根目录下必须包含 `manifest.json` 和包信封 `momobako.package.json`
- 执行型后端必须同时包含 Mutsuki `plugin.toml`；`manifest-only` 插件不需要伪造该文件
- 双清单的插件 ID 与版本必须一致
- 后端入口和资源路径都使用包内相对路径
- `plugin.toml` 的主 artifact 与 companion artifacts 必须存在于包内，并携带匹配内容的规范小写 SHA-256
- 包信封的 `deployment` 跟 `runtime` 对应：`native-dylib` 是 `abi`，`process` 是 `process`，`manifest-only` 是 `manifest`

典型结构：

```text
parser-asmr-folder-0.1.0-x86_64-pc-windows-msvc.momoplug
  parser-asmr-folder/
    manifest.json
    momobako.package.json
    plugin.toml
    momobako_parser_asmr_folder.dll
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
- 后端动态库和进程 runner: 提取到宿主临时目录，按内容哈希缓存
- 插件更新后，缓存 key 会变化，旧缓存自动失效
- 宿主退出后，临时文件允许被清理

以下情况必须进入 `error` 或 `unavailable`，且宿主 UI 不能崩溃：

- 包损坏
- 缺失 `manifest.json`
- 不支持的插件形态（不是后端的 `native-dylib`、`process`、`manifest-only`）
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
2. 复制 `template/backend-rust/` 创建新插件工程，补上 `plugin.project.json` 和 `plugin.toml`（照现有原生插件，例如 `parser-asmr-folder/`），并把目录加进根 `Cargo.toml` 的 `workspace.members`；只有声明的插件只要 `manifest.json`，`plugin.project.json` 写 `build.type` 为 `manifest-only`（不写这个文件时也按它构建）
3. 编写 `manifest.json` 与 `src/lib.rs`
4. 回到主仓库根目录执行 `yarn tauri:dev`

`yarn tauri:dev` 会依次执行完整插件构建、打包、暂存和桌面端启动；`yarn tauri:dev:with-plugins` 保留为兼容别名。主仓库根的 `plugins:build`、`plugins:package`、`plugins:stage:dev` 只是对 `External/Plugins/package.json` 脚本的转发，方便单独排查构建阶段，不再承载插件工具链本体。
不传 `<serviceRoot>` 时，`stage:dev` 会把 `.momoplug` 复制到 `src-tauri/.service-data/plugins`；传入 `<serviceRoot>` 时仍复制到 `<serviceRoot>/plugins`。

`template/backend-rust/` 与 `example/backend-rust/` 不在根 workspace 里，也不经 `yarn build` 构建；它们没有 `plugin.toml` 和 `plugin.project.json`，要打成可安装的 v2 包，照上面第 2 步补齐并加进 workspace。
双清单与 artifact 规则可用 `yarn test:package` 做定向验证。

## 10. 模板与示例

- `template/backend-rust/`
  - 后端原生插件模板
  - 演示如何使用 `External/Plugins/_sdk/mutsuki-rust`
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
  "pluginId": "momobako.parser.asmr-folder",
  "build": {
    "type": "cargo-native",
    "sourceDir": "src",
    "outputDir": ".dist/parser-asmr-folder"
  }
}
```

常见 `build.type`：

- `cargo-native`
- `cargo-process`
- `manifest-only`
