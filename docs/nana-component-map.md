# NanaUI 组件与宿主对应

本文锁定 `momobako-nana` 当前构建能用的 Nana 组件、和旧 Tauri/Vue 工作台的对应关系，以及还没有现成组件的宿主缺口。稳定 id 以 `src-nana/src/capability.rs` 的 `COMPILED_COMPONENT_IDS` 为准；目录变化时测试会失败。

构建 feature 是 `hosted`、`bundled-fonts`、`components`、`icons-tabler`。`components` 打开 calendar、charts、controls、graph-canvas、image-viewer、rich-text。`hosted` 带上 GPU 和 AccessKit。未启用 `syntax-highlighting` 和 `packaged-resources`。Nana 修订是 `9dd590a53dee822a2e4cfb8ab37070fa6ca5b19f`，不保留本地副本。

## 已编译组件

壳层、导航和列表：`app-shell`、`app-title-bar`、`split-pane`、`workspace`、`dock`、`dock-panel`、`pane-chrome`、`pane-tree`、`sidebar-frame`、`sidebar-section`、`sidebar-row`、`sidebar-footer`、`tree-view`、`tabs`、`list-item`、`reorder-list`。

控件和输入：`button`、`icon-button`、`text-input`、`textarea`、`hosted-textarea`、`checkbox`、`switch`、`range-field`、`select`、`segmented-control`、`dropdown`、`search-dropdown`、`form-field`、`chip`、`interactive-card`、`xy-pad`、`key-capture-layer`、`keymap-layer`。

反馈和浮层：`progress`、`spinner`、`skeleton`、`level-meter`、`status-badge`、`validation-message`、`toast`、`empty-state`、`dialog`、`confirm-dialog`、`drawer`、`tooltip`、`popover`、`action-menu`、`action-menu-item`、`anchored-action-menu`、`context-menu`、`overlay-host`、`command-palette`。

内容、预览和设置：`text`、`card`、`thumbnail`、`avatar`、`labeled-value`、`image-viewer`、`native-markdown`、`selectable-rich-text`、`gpu-view`、`gpu-texture-view`、`settings`、`settings-collapsible-card`、`appearance-section`、`about-section`。

仅在插件贡献声明了对应视图时使用：`calendar-heatmap`、`donut-chart`、`time-series-chart`、`graph-canvas`、`graph-minimap`、`qr-code`。缓存和任务不做成装饰图表。

## 探针结论

`MediaTransportBar` 可以从 `nana_ui::components` 构造，事件包括播放暂停、提交 seek、拖动开始和结束、音量。它不在 `component_catalog` 里。播放条迁移直接用这个类型；如果后续修订把它移出公开导出，再改用 `Button` 加 `RangeField`。

`List` 只是一个带列表角色的容器，不按视口裁剪子节点。一万行文件列表用 `VirtualListLayout`：800px 视口、28px 行高、56px overscan 的窗口远小于一万行。文件浏览必须走虚拟列表，不能把全部条目挂成 `List` 的子节点。

`SettingsCard` 和 `SettingsRow` 是公开类型，没有单独的目录 id。设置页使用 `settings`、`settings-collapsible-card`、`appearance-section`，行组件作为这些分区的内部结构。

## 旧框架对应

| Tauri / Vue | Nana |
| --- | --- |
| `/` 与 `/settings` | 同一个 `RuntimeDocument` 里的工作台状态和设置页 |
| `WorkspacePanelKey` | 工作台 `panel`，和加载、错误、缺失仓库、对话框正交 |
| 侧栏快捷方式和目录树 | `SidebarRow`、`TreeView` |
| composable 分支 | `ShellMessage` 和 `ShellViewModel`，记在逻辑矩阵 |
| `repositoryApi` invoke | `NativeServices` 上已有的 ViewModel |
| 宿主事件 | `HostEvent` / `HostEventSink` |
| 侧栏宽度 220–480，默认 276 | `theme_map::SIDEBAR_*`。当前壳层仍写死 220，Phase 1 再改 |
| CSS 变量 | `theme_map::COLOR_ROLES` 指向的 `SemanticPalette` 字段 |
| Vue 插件组件 | `NativePluginContribution`，否则 `UpgradeRequired` |
| 窗口、对话框、外部打开 | `host_api` 请求 |

产品窗口和 15 个验收场景共用 `SidebarFrame`、`TreeView`、`Dialog`、`Thumbnail`、虚拟列表、`MediaTransportBar`、`NativeMarkdown`、`SelectableRichText`、`ReorderList`、`Progress` 和 `Workspace`。原来的验收按钮列表已经退役。

## 主题

颜色角色见 `src-nana/src/theme_map.rs`。`--bg` 对 `background`，`--bg-elev` 对 `surface`，`--text` / `--text-muted` / `--text-faint` 对 `text` / `muted` / `faint`，`--accent*` 对同名 accent 字段，`--ok` / `--warn` / `--err` 对 `success` / `warning` / `danger`。离屏清屏色必须等于当前主题的 `background`，壳层按钮源码不允许写 RGB 或 `ButtonPaintOverride`。

字号：页面标题用 Nana `title`（18px）。设计标准的 14px 工作文本对应 Nana `section`，13px 行标签对应 Nana `body`。Nana 默认主题把这两个数字对调了，迁移时按这个角色用，不在控件上再写一个 14。元信息用 `meta`（12px），微型标签用 `hint`（11px）。

间距用 Nana `space` 阶梯。产品侧栏宽度是上面的 220 / 276 / 480，不属于间距阶梯。

## 图标

壳层目录已经能解析 Lucide 别名里的 folder、search、settings、file、sidebar、close、add。回收站、标签、时钟、归档、书签、播放、暂停、音量和列表没有壳层别名，`Icon::parse_name("trash")` 返回空。

这些产品图标使用 `nana_ui::icons_tabler`：`ARCHIVE`、`BOOKMARK`、`CLOCK`、`FILE`、`FOLDER`、`LIST`、`PLAYER_PLAY`、`PLAYER_PAUSE`、`SEARCH`、`SETTINGS`、`TAG`、`TRASH`、`VOLUME`。未引用的 Tabler 常量由链接器丢掉。Tabler 没有对应字形时再补产品自有 `IconData`，不用文字冒充图标。

## 没有现成组件的部分

- 视频画面和 PDF 页面。`GpuView` 只能嵌自定义绘制。ZIP、7z、RAR、FlateDecode PDF、Open XML、OLE 文本和 OBJ/glTF/GLB/STL/3MF/VRM 已由内置预览读出文本或列表。FBX 和 BLEND 只报文件头。播放列表的 PCM WAV 在正式 Windows 构建里用 winmm 出声。其余旧 Vue 插件显示升级提示。
- 拖放、系统文件对话框、托盘、全局快捷键和窗口位置恢复。离屏只断言发出的 `host_api` 请求，真效果记在 `nana-device-matrix.md`。
- 代码高亮。需要时再开 `syntax-highlighting`，并补离屏场景。
