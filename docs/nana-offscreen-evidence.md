# Nana Runtime 离屏验收证据

离屏验收用生产 `momobako-nana` 的 `RuntimeDocument`（`acceptance_document_at_width` → `mount_shell`），由 `nana-ui-devtools` 的同一个 `RuntimeAgentSession` 读真实 WGPU 像素和 Runtime 状态，不创建第二棵 UI 树。

## 命令

```bash
yarn test:nana:offscreen
```

它运行 `scripts/run-nana-offscreen-acceptance.sh`：在仓库根目录跑 `cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture`，把输出存成证据目录里的 `acceptance.log`，失败时在 `failures.log` 末尾补一行退出码。证据目录默认是仓库根目录的 `target/nana-offscreen-evidence/`，可用 `MOMOBAKO_NANA_EVIDENCE_DIR` 指定。不经脚本、直接跑 `cargo test` 时默认写到 `src-nana/target/nana-offscreen-evidence/`，没有 `acceptance.log`。

读不到离屏像素（`offscreen::pixels_available()` 为假）时，生成证据的测试打印一行说明后返回，命中测试直接返回，都不算失败。

## 场景和视口

`src-nana/tests/offscreen_acceptance.rs` 出 61 个场景，每个场景出 4 个视口（1200×800、960×600，浅色、深色），共 244 组证据：

- 15 页：`ShellPage` 的每一种，经 `ShellViewModel::for_page` → `acceptance::seed` 铺数据。
- 42 个补充场景：`acceptance_gap_models()`，定义在 `src-nana/src/shell/acceptance_*.rs` 各自的 `models()`。
- 4 个压力场景，只在这个测试里：`long-content` 根目录读回一个超长文件名，`empty-list` 根目录读回空列表，`disabled-feedback` 在设置错误页上保存设置被拒（状态区写原因），`dense-list` 任务弹层里 12 个运行中任务。都经真实消息归约。

有仓库的场景都从 `acceptance_base.rs` 的共用底子起步：照产品启动依次归约资源库列表、同步、仓库摘要、首屏目录和侧栏的读取结果，数据和 Vue 夹具 `base()` 一致。场景要别的条目、目录树、播放集、播放器或仓库时改底子的字段。

| 来源 | 场景 |
| --- | --- |
| 15 页（`ShellPage`） | `loading`、`empty-repository`、`error`、`file-list`、`selected-file`、`playlists`、`plugin-settings`、`task-running`、`playback-running`、`task-cancelling`、`conflict`、`unsaved-edit`、`settings`、`settings-error`、`logs` |
| `acceptance_shell.rs` | `live-files-plain`、`live-visual`、`missing`、`repo-switcher`、`repo-delete-dialog`、`folder-create-dialog`、`smart-folder-dialog`、`playlist-create-dialog`、`status-error`、`folder-tree-refresh` |
| `acceptance_files.rs` | `live-files`、`live-files-selected`、`files-selected-metadata`、`files-selected-folder`、`live-menu`、`copy-dialog`、`hardlink-dialog`、`export-dialog`、`files-list-mode`、`files-adaptive`、`files-masonry` |
| `acceptance_player.rs` | `playlist-open`、`playback-queue`、`still-playback`、`outside-playback`、`live-preview`、`live-asmr`、`preview-text`、`preview-image`、`preview-audio`、`preview-audio-failed` |
| `acceptance_admin.rs` | `logs-paused`、`extensions`、`downloader-settings`、`office-convert`、`source-auth-gap`、`source-auth-methods`、`foreign-tool` |
| `acceptance_search.rs` | `filter-bar`、`filter-bar-active`、`search-results`、`search-empty` |
| 压力场景（`special_models`） | `long-content`、`empty-list`、`disabled-feedback`、`dense-list` |

## 每组证据检查什么

1. 清屏色等于该主题的 `SemanticPalette::background`（`theme_map::clear_matches_background`），浅色和深色不共用写死的底色。
2. 无障碍树里有名为 `MomoBako` 的节点。
3. 场景画出了它名字所说的状态：`scene_labels` 列出的无障碍名都在，`scene_values` 列出的输入框值都在；`unsaved-edit` 另查画面上没有名为「未保存」的按钮或状态。`scene_labels` 没单列的场景按页面取标题（`page_labels`）。
4. 有激活的对话框时，先把进场动效走完再截图：离屏会话没有帧时钟，不推进会截到透明度为 0 的对话框。
5. PNG 的宽高等于视口，非背景像素比例大于 0。有非背景像素不算视觉通过。

每组写出 PNG、语义快照（`*.semantic.json`：页面、主题、清屏色、视口和无障碍树）、根节点的布局盒和绘制探针（`*.layout.json`），以及 `(20,20)` 处的命中结果（`*.hits.json`）。`scene-manifest.json` 记录证据 schema（`momobako.nana.offscreen/v1`）、产品版本、Unix 生成时间、运行时、固定的 Nana 修订、命令、视口矩阵、每个证据文件的 SHA-256 和字节数，以及失败的场景。`visual-review.json` 列出自动检查项和各自的证据文件（其中禁用和失败反馈一项指向 `disabled-feedback-*`，画面见上一节），问题记录 `visual_issue_records` 为空。有场景失败时写 `failures.log`，测试失败。

同一文件里还有两个测试：

- `native_actions_are_reachable_through_runtime_hit_testing`：在文件页依次点击「根目录」「在当前目录新建文件夹」「设置」「网格」「最小化」「最大化」「关闭」，都要命中。窗口按钮的名字来自 NanaUI 框架文案表的默认中文。
- `scene_ids_are_unique`：页面名和补充场景名不重复，证据文件不会互相覆盖。

离屏验收不归约视图量到尺寸后发回的消息（播放条宽、文件列表宽），这一步只在 `scene_shots` 里做。

## 按场景出图：`scene_shots`

```bash
NANA_SCENES=live-files,settings NANA_SIZES=1200x800,960x600 NANA_THEMES=light,dark \
  cargo test -p momobako-nana --test scene_shots -- --nocapture
```

`src-nana/tests/scene_shots.rs` 只渲染 `NANA_SCENES` 点名的场景，可选的是 15 页和 42 个补充场景，共 57 个，不含压力场景。`NANA_SIZES` 默认 `1200x800`，`NANA_THEMES` 默认 `light`。没给 `NANA_SCENES` 时不出图；名字不认识时测试失败，并列出可选的场景。

- 截图前先把视图量到尺寸后发回的程序消息（播放条宽、文件列表宽）归约、同步，最多四轮，截到的是稳定以后的画面；四轮后还有消息就打印一行提示。
- 有激活的对话框时同样先走完进场动效。
- 每张图输出 `src-nana/target/nana-scene-shots/<场景>-<主题>-<宽>x<高>.png`，旁边的 `.nodes.txt` 列出每个语义节点的编号、角色、名称和布局盒。

改视图前后各出一遍图逐字节比较，见 [壳层绑定式视图](./nana-shell-view.md) 的「测试」。

## 实况视觉和点击

- `cargo test -p momobako-nana --test live_visual`：用实况文件工作区（仓库「动画素材」）在 1200×800、960×600 的浅色和深色下检查标题栏（侧栏开关、全局搜索、筛选开关、窗口按钮）、展示方式下拉、文件工具条、文件名、播放条和侧栏底部入口的布局盒；再查启动页的四个步骤和缺失仓库页；点击侧栏开关、最小化、最大化、关闭，在全局搜索里输入，开关筛选栏；打开「新建智能文件夹」对话框，填名称后提交。另有两个输入测试：两次按键之间同步视图，以及光标在文字中间时同步视图。
- `cargo test -p momobako-nana --test live_surfaces`：WAV 预览（含 390×844 窄屏）和点击播放、导入工具页打开路径对话框、来源扫码登录、日志页追踪和暂停时的列表位置。

两个文件的截图写到 `src-nana/target/nana-live-visual/`。出图的测试读不到像素时失败。

## Vue 对照

逐场景和 Vue 对照用本机的 `tmp/vue-mock`（`tmp/` 被 git 忽略，不在仓库里）。Vue 侧用同名场景、同一份夹具在无头 Edge 里截图，Nana 侧用上面的 `scene_shots`，两边出差异热图和差异像素比例。比例只用来发现问题，不作为通过标准。做法见 [Nana 与 Vue 对照](./nana-vue-parity.md#对照方法)。

## 不替代的验收

离屏证据不替代 Windows 真窗口、拖放、IME、托盘、UIA/AccessKit、真实 GPU 设备和发布包的验收，见 [设备矩阵](./nana-device-matrix.md)。
