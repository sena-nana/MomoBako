# NanaUI 组件与宿主对应

本文锁定 `momobako-nana` 当前构建能用的 Nana 组件、和旧 Tauri/Vue 工作台的对应关系，以及还没有现成组件的宿主缺口。稳定 id 以 `src-nana/src/capability.rs` 的 `COMPILED_COMPONENT_IDS` 为准；目录变了，`compiled_catalog_matches_the_locked_product_set` 会失败。

`src-nana/Cargo.toml` 给 `nana-ui` 开 `hosted`、`bundled-fonts`、`components`、`icons-tabler`、`view-macro`，给 `nana-ui-platform` 开 `clipboard`。`components` 打开 calendar、charts、controls、graph-canvas、image-viewer、rich-text；`hosted` 带上 GPU 和 AccessKit 平台适配；`view-macro` 提供结构块容器的 `css!` 和 `#[derive(Store)]`。没开 `syntax-highlighting` 和 `packaged-resources`。Nana 修订是 `c6dc1086bb1b21406ee5e20a40ac6207c664913b`，不保留本地副本。

## 已编译组件

壳层、导航和列表：`app-shell`、`app-title-bar`、`split-pane`、`workspace`、`dock`、`dock-panel`、`pane-chrome`、`pane-tree`、`sidebar-frame`、`sidebar-section`、`sidebar-row`、`sidebar-footer`、`tree-view`、`tabs`、`list-item`、`reorder-list`。

控件和输入：`button`、`icon-button`、`text-input`、`textarea`、`hosted-textarea`、`checkbox`、`switch`、`range-field`、`select`、`segmented-control`、`dropdown`、`search-dropdown`、`form-field`、`chip`、`interactive-card`、`xy-pad`、`key-capture-layer`、`keymap-layer`。

反馈和浮层：`progress`、`spinner`、`skeleton`、`level-meter`、`status-badge`、`validation-message`、`toast`、`empty-state`、`dialog`、`confirm-dialog`、`drawer`、`tooltip`、`popover`、`action-menu`、`action-menu-item`、`anchored-action-menu`、`context-menu`、`overlay-host`、`command-palette`。

内容、预览和设置：`text`、`card`、`thumbnail`、`avatar`、`labeled-value`、`image-viewer`、`native-markdown`、`selectable-rich-text`、`gpu-view`、`gpu-texture-view`、`settings`、`settings-collapsible-card`、`appearance-section`、`about-section`。

图表和图：`calendar-heatmap`、`chart`、`graph-canvas`、`graph-minimap`、`qr-code`。

## 产品里用到的组件

只算 `src-nana/src` 里的产品代码，不算测试。产品窗口和验收文档都经 `ShellView::mount` 挂同一棵树，用到的组件相同。

| 组件 | 用在哪里 |
| --- | --- |
| `app-shell`、`app-title-bar`、`workspace` | 骨架、标题栏和工作区（`view_host.rs`、`title_bar.rs`、`render.rs`） |
| `sidebar-frame`、`list-item` | 侧栏外框和侧栏、弹层、对话框里的行（`sidebar_view.rs`、`sidebar_tree_view.rs`、`sidebar_popover_view.rs`） |
| `context-menu` | 侧栏文件夹树的右键菜单（`sidebar_tree_view.rs`）。文件右键菜单是自绘的（`files_menu.rs`） |
| `dialog`、`confirm-dialog`、`overlay-host` | 全部对话框（`overlay_dialog.rs`） |
| `button`、`icon-button`、`text-input`、`textarea`、`checkbox`、`range-field`、`select` | 各页控件；`textarea` 的类型是 `TextArea` |
| `reorder-list` | 播放集条目（`player_playlist_page.rs`） |
| `thumbnail` | 文件卡片、预览兜底和播放条封面（`files_cards.rs`、`inspect_preview_body.rs`、`player_bar.rs`） |
| `gpu-texture-view` | 图片和视频预览，纹理槽 `file-preview`（`inspect_preview_body.rs`） |
| `qr-code` | 来源插件扫码登录（`source_auth_page.rs`） |
| `empty-state`、`labeled-value` | 压缩包空目录和资源库扩展信息（`inspect_library.rs`） |
| `validation-message` | 元数据的版本冲突提示（`inspect_metadata_view.rs`） |
| `text` | 文字 |

目录外的公开类型用到 `SettingsCard`（`inspect_library.rs`、`inspect_metadata_parts.rs`）和 `List`（压缩包文件列表，`inspect_library.rs`）。文件列表用视图层的 `each_virtual` 按视口建行（`files_virtual.rs`），不用 `VirtualListLayout`。

其余已编译组件产品里没有用到，包括 `tree-view`、`sidebar-row`、`sidebar-section`、`sidebar-footer`、`native-markdown`、`selectable-rich-text`、`image-viewer`、`gpu-view`、`progress`、`spinner`、`settings`、`settings-collapsible-card`、`appearance-section`。设置页、日志页、插件页和播放条由 `Stack`、`Text`、`Button`、`RangeField` 等拼成（`admin_*.rs`、`player_bar*.rs`）。

## 探针结论

`capability.rs` 的测试锁住下面几条：

- `MediaTransportBar` 能从 `nana_ui::components` 构造，事件有播放暂停、提交 seek、拖动开始和结束、音量；它不在 `component_catalog` 里。产品的播放条没有用它，是 `IconButton`、`RangeField` 等拼的（`player_bar.rs`、`player_bar_parts.rs`）。
- `VirtualListLayout` 在 800px 视口、28px 行高、56px overscan 下，一万行的可见窗口少于 100 行。
- `media-transport-bar`、`settings-card`、`settings-row`、`runtime-list` 是没有目录 id 的公开类型（`UNCATALOGUED_PUBLIC_WIDGETS`），进了目录测试会失败。
- 壳层图标目录认得 `lucide-folder`、`lucide-search`、`lucide-settings`、`lucide-file`；`trash`、`player-play` 解析为空，这些图标用 Tabler 常量。

## 旧框架对应

| Tauri / Vue | Nana |
| --- | --- |
| `/` 与 `/settings` | 同一个 `RuntimeDocument`。主区按 `RouteKey` 切换路由，设置页是其中一条（`view_part_primary.rs`） |
| `WorkspacePanelKey` | `WorkspacePanel`，和启动、加载错误、缺失仓库、空库（`MainRegion`）正交 |
| 侧栏快捷方式和目录树 | `SidebarFrame` 里的 `ListItem` 行；文件夹树和智能文件夹树是按显示顺序展开的扁平行，用 `keyed(..).each` 建（`sidebar_tree_view.rs`），不用 `TreeView` |
| composable 分支 | `ShellMessage` 和 `ShellViewModel`，记在逻辑矩阵 |
| `repositoryApi` invoke | `NativeServices` 上的 ViewModel |
| 宿主事件 | `HostEvent` / `HostEventSink` |
| 侧栏宽度 220–480，默认 276 | `theme_map::SIDEBAR_MIN_PX` / `SIDEBAR_DEFAULT_PX` / `SIDEBAR_MAX_PX`；工作区资源区的宽度跟着侧栏呈现宽度（`render.rs`、`hot.rs`），宽度存进 `sidebar.json` |
| CSS 变量 | `theme_map::COLOR_ROLES` 指向的 `SemanticPalette` 字段 |
| 对话框（`.modal-overlay`、`.modal-card`） | NanaUI `Dialog` / `ConfirmDialog`，距顶、分区、遮罩和开合动效写在主题配方里（`appearance.rs`） |
| Vue 插件组件 | 不进原生树。原生贡献用 `NativePluginContribution`（`plugin_api.rs`）描述，现状见 [迁移边界](./nana-migration.md) |
| 窗口、对话框、外部打开 | `host_api` 请求和 Nana 平台窗口命令 |

## 主题

颜色角色见 `src-nana/src/theme_map.rs`。`--bg` 对 `background`，`--bg-elev` 对 `surface`，`--text` / `--text-muted` / `--text-faint` 对 `text` / `muted` / `faint`，`--accent*` 对同名 accent 字段，`--ok` / `--warn` / `--err` 对 `success` / `warning` / `danger`，完整对应在 `COLOR_ROLES`。离屏验收检查清屏色等于当前主题的 `background`（`clear_matches_background`）。`theme_map.rs` 的 `shell_buttons_do_not_hardcode_paint_colors` 只扫 `shell/render.rs`，不禁止别的文件写颜色。

字号：页面标题用 Nana `title`（18px）。设计标准的 14px 工作文本对应 Nana `section`，13px 行标签对应 Nana `body`。Nana 默认主题把这两个数字对调了，迁移时按这个角色用，不在控件上再写一个 14。元信息用 `meta`（12px），微型标签用 `hint`（11px）。对应表是 `TYPE_ROLES`。

圆角基数来自设置（0–20，默认 8），`corner_metrics` 换成 xs 到 xl 五档，2xl 用 `radius_2xl`。

间距用 Nana `space` 阶梯。产品侧栏宽度是上面的 220 / 276 / 480，不属于间距阶梯。

## 图标

产品图标有两个来源：

- `nana_ui::icons_tabler` 的常量，侧栏、文件页、菜单等处用。未引用的常量由链接器丢掉。
- 照 Vue 用的 lucide 源 SVG 登记的产品 `IconData`：`player_icons.rs` 给预览、播放条和播放集页，`admin_icons.rs` 给设置、插件、日志、动作页和任务弹层，形状和 Vue 一致。

不用文字冒充图标。

## 没有现成组件的部分

- 视频画面、PDF 页面和 3D 模型。视频帧解码后放进宿主纹理槽 `file-preview`，由 `GpuTextureView` 显示；PDF 页和模型网格软件光栅成 RGBA 帧，预览框把帧编码成 PNG 画出来（`inspect_preview_page.rs`）。`GpuView` 只有内置调色板着色器，没有网格管线，产品没有用它。压缩包、Office 文本和 FBX、BLEND 文件头由内置预览读出（`native_preview.rs`）。
- 系统托盘、拖出、窗口几何和系统媒体传输控件。宿主用 `tray-icon`、`drag`、`windows` crate 和 `nana_ui_platform` 实现（`tray.rs`、`drag_out.rs`、`window_state.rs`、`system_media.rs`）。系统文件对话框和文件拖入用 Nana 平台的 `WindowCommand::OpenFileDialog` 和 `FileDropEvent`。测试只断言排出的请求和命令，真效果记在 `nana-device-matrix.md`。
- 全局快捷键：没有实现。
- 代码高亮：没开 `syntax-highlighting`。文本预览和 Vue `text-preview` 一样显示等宽原文，不着色。
