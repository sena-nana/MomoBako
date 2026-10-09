# Nana Runtime 离屏验收证据

Issue #20 的验收命令是：

```bash
yarn test:nana:offscreen
```

它运行生产 `momobako-nana` 的 `RuntimeDocument`，由同一个 `RuntimeAgentSession`
读取真实 WGPU 像素和 Runtime 状态，写入 `target/nana-offscreen-evidence/`（可用
`MOMOBAKO_NANA_EVIDENCE_DIR` 指定目录）。每个场景包含真实 PNG、语义快照
(`*.semantic.json`)、布局盒/绘制探针 (`*.layout.json`) 和 `(20,20)` 交互命中结果
(`*.hits.json`)。`scene-manifest.json` 记录证据 schema、产品版本、固定 Nana revision、
视口/主题矩阵及每个文件的 SHA-256 与字节数；命令输出保存在 `acceptance.log`，失败场景
写入 `failures.log`。

矩阵覆盖 1200×800 和 960×600，浅色和深色主题，以及加载、空仓库、错误、文件列表、
文件预览、播放列表、插件设置、任务运行/取消、播放、冲突、未保存编辑、设置、设置错误、
系统日志 15 个关键状态，另有长文件名/长状态、空列表、禁用/失败反馈和高密度任务列表
四组压力场景。清单包含 Unix 生成时间、产品/Nana 版本和每个文件的 SHA-256；
`visual-review.json` 保存自动化视觉检查项和问题记录入口。离屏验收沿用生产
Runtime/GPU 所有权链，不创建第二棵 UI 树。

每个场景在写出 PNG 之前检查清屏色等于该主题 `SemanticPalette::background`。浅色和深色因此不会共用同一个写死背景。画面里有非背景像素不算视觉通过。

实况文件工作区由 `cargo test -p momobako-nana --test live_visual --offline` 验收。它使用同一份生产文档，检查 Nana 标题栏打开的窗口控件、全局搜索和筛选开关，以及文件名、播放条、展示方式和工具条的布局盒。读不到布局盒或像素时失败。15 个验收场景使用同一套产品表面。文件场景命中「根目录」「在当前目录新建文件夹」「设置」，窗口按钮命中「最小化」「最大化」「关闭」（NanaUI 框架文案表的默认中文名）。

离屏证据不替代 Windows 真窗口、拖放、IME、托盘、UIA/AccessKit、真实 GPU 设备和发布包
的设备验收。
