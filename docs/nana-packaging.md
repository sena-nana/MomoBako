# Nana 发布包

发布包是原生壳层 `momobako-nana`，用 NanaUI 的 `nana-packager` 打。本文写交付物是什么、怎么在本地打、发布流程、包里的插件和外部程序放在哪、数据写在哪。打包合同本身在 NanaUI 仓库的 `docs/reference/packaging.md`，应用路径在 `docs/reference/diagnostics.md`；MomoBako 固定的 NanaUI 修订见 `src-nana/Cargo.toml`，`nana-packager` 也装同一修订。

## 交付物

`nana-packager` 的 `installer` 方式只产出安装版的目录布局和 manifest，不产出安装程序：NanaUI 还没有 MSI、MSIX 这类安装包后端，`validate` 把「安装包往返」报成未执行。所以每次发布在 GitHub 草稿 Release 上放两个 zip，都是 NanaUI 原生的布局：

| 文件 | 发行方式 | 数据写在 | 适合 |
| --- | --- | --- | --- |
| `MomoBako_<版本>_x64_portable.zip` | `portable` | 解压目录下的 `MomoBako/data` | 放在任意目录、U 盘里用，整个目录拷走就带走数据 |
| `MomoBako_<版本>_x64.zip` | `installer` | `%APPDATA%\com.momobako.desktop`、`%LOCALAPPDATA%\com.momobako.desktop` | 解压到固定位置（例如 `%LOCALAPPDATA%\Programs`）长期用，换新版本时整个目录换掉，数据不动 |

没有另做 NSIS 或 WiX 安装程序：那是 NanaUI 合同以外的第二条打包流水线，`validate` 校验不到它装出来的东西，NanaUI 的安装包后端落地以后还得换掉。等 NanaUI 有了安装包后端，再从 `installer` 布局出安装程序。

两个 zip 里都是同一个可执行文件和同一组随附插件。插件包（`.momoplug`）照旧另外作为工作流产物 `external-plugin-packages` 上传。

## 包里有什么

zip 解开是一个 `MomoBako` 目录，布局照 NanaUI 合同：

```text
MomoBako/
├─ MomoBako.exe                  主程序，带图标和版本信息
└─ runtime/
   ├─ plugins/*.momoplug         随附插件，External/Plugins 下的全部插件（现在 44 个，其中 10 个后端插件）
   ├─ manifest/package.json      身份、布局、随附插件清单、每个文件的大小和 BLAKE3
   ├─ manifest/portable          只有便携版有，空文件，标记便携布局
   └─ bin/、resources/、tools/   现在是空的
```

- **随附插件**：`runtime/plugins` 只读。启动时宿主把 manifest 列出的插件同步进服务目录的 `plugins/builtin`：内容变了的换新，manifest 里没有的删掉（解压新版本覆盖旧目录时留下的旧版本包不算），再从那里扫描、校验和加载（`src-nana/src/plugin_host.rs`、`momobako_backend::services::runtime::bundled_plugins`）。开发时没有 manifest，读插件打包输出 `External/Plugins/.packages`，目录不存在就不同步。
- **外部程序**：不随包。ffmpeg 和 ffprobe 按随包目录 `runtime/tools`、下载目录、可执行文件旁边、`PATH` 的顺序找（`momobako_backend::services::external_tools`），都没有时第一次生成音视频缩略图时下载到缓存目录的 `tools` 下，不写进安装目录。随包分发 FFmpeg 要满足的许可条件见 `THIRD_PARTY_NOTICES.md`；要随包带时在 `nana-package.toml` 里加 `[runtime] tools`。Office 转换的 helper 在 office-convert 插件包里，随插件走。
- **资源**：界面字体和图标编在二进制里，没有 `.nrpack` 资源包。
- **图标和版本信息**：`src-nana/build.rs` 把 `src-nana/icons/icon.ico` 和版本资源（公司、产品名、版本、版权、原始文件名）编进可执行文件；窗口和任务栏用 `src-nana/icons` 里的 PNG（`app_icon.rs`）。图标从 `icon.svg` 用 `yarn icons:generate` 生成。

## 数据写在哪

位置照 NanaUI 的 `ApplicationPaths`，安装版和便携版都不往安装目录写（`src-nana/src/app_paths.rs`）：

| 内容 | 安装版 | 便携版 | 开发（从 `target/` 运行） |
| --- | --- | --- | --- |
| 领域服务目录（资源库注册表、插件、插件数据、外部 API 连接文件、系统日志） | `%APPDATA%\com.momobako.desktop\service-data` | `<根>\data\service-data` | 当前目录下的 `.service-data` |
| 应用设置和界面偏好（`settings.json`、`sidebar.json` 等） | `%APPDATA%\com.momobako.desktop\config` | `<根>\data\config` | `%LOCALAPPDATA%\MomoBako` |
| 主窗口位置和尺寸 | `%APPDATA%\com.momobako.desktop` | `<根>\data` | `%APPDATA%\com.momobako.desktop` |
| 下载的 ffmpeg | `%LOCALAPPDATA%\com.momobako.desktop\Cache\tools` | `<根>\data\cache\tools` | `%LOCALAPPDATA%\com.momobako.desktop\Cache\tools` |
| NanaUI 诊断日志和崩溃快照 | `%LOCALAPPDATA%\com.momobako.desktop\Logs`、`Crash` | `<根>\data\logs`、`crash` | 同安装版 |

应用路径解析不出来时（缺 `APPDATA` 之类的环境变量）按开发时的位置读写，不同步随附插件。

## 应用身份和版本

- id `com.momobako.desktop`、名称 `MomoBako`、发行者 `Sena` 只在 `src-nana/build.rs` 写一份，经 `application_identity!` 写进二进制（`NANA-IDENTITY-V1` 标记），版本取 `src-nana/Cargo.toml` 的 `version`。
- `src-nana/nana-package.toml` 的 `[application]` 和 `[executable]` 要和它一致，`nana-packager` 打包时比对二进制里的标记，不一致就拒绝；`tests/nanaPackage.test.ts` 也比对这三处。
- 升级版本时改 `src-nana/Cargo.toml` 的 `version` 和 `nana-package.toml` 的 `version`；发布用的 tag 要是 `v<版本>`，发布流程用 `--expect-tag` 检查。

## 本地打包

在仓库根目录：

```text
corepack yarn install --immutable                      # 根目录依赖，打包脚本只用 Node 自带模块
(cd External/Plugins && corepack yarn install --immutable)
corepack yarn plugins:build                            # 插件发布构建，产物在 External/Plugins/.dist
corepack yarn plugins:package                          # .momoplug 在 External/Plugins/.packages
corepack yarn nana:build                               # cargo build -p momobako-nana --profile dist --locked
corepack yarn nana:package                             # 打包、校验、压 zip
```

`nana:package`（`scripts/nana-package.ts`）做这些事：

1. 按 `src-nana/Cargo.toml` 里固定的 NanaUI 修订装 `nana-packager` 到 `target/nana-packager`（`cargo install --git https://github.com/sena-nana/NanaUI.git --rev <修订> --locked nana-packager`，装过同一版时跳过）。也可以用 `--packager <路径>` 或环境变量 `NANA_PACKAGER` 指定。
2. 读 `External/Plugins/.packages` 下每个 `.momoplug` 的包信封，核对格式是第 2 版、目标三元组是这台机器的或 `any`、插件 id 不重复。
3. 以 `src-nana/nana-package.toml` 为底，为安装版和便携版各生成一份配置（`<输出>/config/*.toml`），每个随附插件一条 `[[runtime.plugins]]`。
4. 两种方式各跑一次 `nana-packager package` 和 `nana-packager validate`，再用 Windows 自带的 `tar` 压成 zip。
5. 把便携版的 zip 解到 `<输出>/check`，跑 `validate --run --tamper-suite`：应用以 `NANA_PACKAGE_VALIDATE=1` 启动，在开窗前自检后退出，诊断日志只写进解开目录的 `data/`，不碰本机的 MomoBako 数据。安装版不跑 `--run`，它会把诊断日志写进本机的 `%LOCALAPPDATA%`。

输出默认在 `target/nana-package`，可以用 `--out` 换；可执行文件默认是 `$CARGO_TARGET_DIR/dist/momobako-nana.exe`（没设时是 `target/dist`），可以用 `--exe` 换；`--build-id` 默认是当前提交的短哈希。

`dist` 配置在根 `Cargo.toml`：照 NanaUI 合同在 `release` 上开 fat LTO、单代码生成单元、去掉符号，但保留 `panic = "unwind"`，因为宿主的任务守卫和缩略图解码要在任务里接住 panic。插件照旧用 `release` 编。

## 发布流程

`.github/workflows/release.yml`，推 `v*` tag 或手动触发：

1. 装插件依赖，跑 `yarn test:backend`；
2. 构建并打包插件（`External/Plugins` 的 `yarn build`、`yarn package`）；
3. `yarn nana:build` 编 dist 版；
4. `yarn nana:package --out <runner 临时目录> --expect-tag <tag>`：打包、校验、压 zip；
5. 用 `gh release create --draft` 建草稿 Release 并上传两个 zip（同名草稿已经在时用 `gh release upload --clobber` 覆盖），说明里写两个 zip 的区别，后面接自动生成的更新记录；
6. 照旧把 `.momoplug` 作为工作流产物上传。

CI（`ci.yml`）跑 `yarn verify`（Node 脚本类型检查、vitest 和 `momobako-backend`、`momobako-nana` 的编译检查）和 `yarn test:backend`，另外构建并打包插件，不打主程序的发布包。

## 校验

`nana-packager validate` 在两种布局上的结果（本地试跑，2026-10-11，NanaUI `1d65284f9`）：

| 结果 | 检查项 |
| --- | --- |
| 通过 | `layout.single-root-entry`（根目录只有 `MomoBako.exe` 和 `runtime/`）、`files.manifest-match`、`identity.binary-matches-manifest`、`dependencies.discoverable`、`distribution.no-updater`；便携版另有 `run.launch-from-foreign-cwd`、`tamper.control`、`tamper.manifest-byte` |
| 警告 | `manifest.signature`、`signing.publisher`：没有发布者签名。`dependencies.vc-runtime`：动态链接 VC++ 运行库（`MSVCP140.dll`、`VCRUNTIME140.dll`、`VCRUNTIME140_1.dll`）。`dependencies.assumed-system`：`dcomp.dll`（DirectComposition，Windows 自带，只是不在 NanaUI 的系统库名单里） |
| 未执行 | `startup.early-splash`、`startup.ui-ready-handoff`、`installer.round-trip`、`static-app-plan`：NanaUI 还没实现 |

自检只验证到开窗以前。本地另外把便携版解到一个短路径下直接启动：窗口进到工作区（还没有资源库），48 个随附插件（当时还有 4 个只有前端的预览插件，删掉以后是 44 个）同步进 `data/service-data/plugins/builtin`，10 个后端插件都加载上、进程插件的运行器起来了；关窗（`WM_CLOSE`）以后进程和运行器都退出，所有文件都在 `data/` 下，本机的 `%APPDATA%`、`%LOCALAPPDATA%` 没有动。

dist 版主程序约 87 MB，两个 zip 各约 53 MiB。本机 16 核冷编译主程序约 35 分钟（fat LTO、单代码生成单元），插件发布构建约 9 分钟。

## 已知问题

- **VC++ 运行库**：主程序和插件的 DLL、进程插件都动态链接 VC++ 运行库，没装 VC++ 2015–2022 运行库的机器上主程序启动不了。后续要么随安装程序带运行库，要么主程序和插件一起改成静态链接 CRT（只改主程序时插件仍然加载不了）。
- **路径长度**：插件解包在 `service-data\mutsuki-plugin-cache\plugin-<64 位哈希>\` 下，进程插件的运行器路径比服务目录长 137 个字符。服务目录超过 122 个字符时（便携版的应用根目录超过约 104 个字符）运行器路径超过 `MAX_PATH`，进程起不来，整代插件都会被拒绝。本地试跑把便携版解在很深的目录里时就碰到了。
- **插件一个坏全都不可用**：插件宿主加载一代插件时，任何一个失败整代都不用（`mutsuki_host::MomoPluginRuntime::reload`）。
- **漫游目录**：安装版的服务目录照合同放在 `<data>`（`%APPDATA%`，漫游）下，里面也有随附插件的副本和插件解包缓存（几十 MB）；用漫游配置文件的机器上会跟着同步。领域服务目录还没有分出数据和缓存两部分，后续可以把缓存类的子目录挪到 `<cache>`。

## 还没有的

- 安装程序、开始菜单快捷方式和卸载：等 NanaUI 的安装包后端。
- 代码签名和发布者签名：没有证书和签名密钥；`nana-package.toml` 没有 `[signing]`。
- 自动更新：NanaUI 的自更新还没实现，发布包里没有更新程序。
- CI 不打包：`ci.yml` 不跑 `nana:build`、`nana:package`，打包脚本和配置的问题要到发布时才暴露。
- 只打 Windows x64。
