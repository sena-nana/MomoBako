# NanaUI 离屏验收证据

离屏证据必须来自 `momobako-nana` 的生产 `RuntimeDocument`，不能使用静态 HTML、
历史截图或独立 mock。测试命令为：

```text
cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture
```

当前测试生成以下真实 WGPU 输出（目录由 Cargo 的 `CARGO_TARGET_TMPDIR` 决定）：

| 场景 | 视口 / 主题 | 输出 |
| --- | --- | --- |
| 主窗口加载 | 1200×800 / 浅色 | `light-main.png` |
| 最小窗口加载 | 960×600 / 深色 | `dark-min.png` |
| 加载中 | 1200×800 / 默认 | `state-loading.png` + `state-loading.json` |
| 空仓库 | 1200×800 / 默认 | `state-empty-repository.png` + `state-empty-repository.json` |
| 错误 | 1200×800 / 默认 | `state-error.png` + `state-error.json` |
| 文件列表 | 1200×800 / 默认 | `state-file-list.png` + `state-file-list.json` |
| 选中文件 / 预览入口 | 1200×800 / 默认 | `state-selected-file.png` + `state-selected-file.json` |
| 插件设置 | 1200×800 / 默认 | `state-plugin-settings.png` + `state-plugin-settings.json` |
| 任务进行中 | 1200×800 / 默认 | `state-task-running.png` + `state-task-running.json` |
| 冲突 | 1200×800 / 默认 | `state-conflict.png` + `state-conflict.json` |
| 编辑未保存 | 1200×800 / 默认 | `state-unsaved-edit.png` + `state-unsaved-edit.json` |
| 应用设置 / 系统日志 | 1200×800 / 默认 | `state-settings.*` + `state-logs.*` |

JSON 同时保存可访问性树、`scene_probe` 布局/绘制信息和 `(20,20)` 命中结果，
与对应 PNG 来自同一个 `RuntimeAgentSession`。

## 当前人工检查记录

- 语义标签、状态文本和真实 WGPU 像素均已生成。
- 壳层已按确认方案改为 Nana 原生 `Stack/Row/Column` 三栏布局：标题栏 48px、
  导航栏 220px、主内容弹性占用、过程区 240px；选中文件场景的布局探针确认主内容
  从 `(244,68)` 开始，导航和过程区未挤出视口。
- 浅色主窗口、深色最小窗口和五个资源库状态已重新通过真实 WGPU 渲染验收。
- Windows 原生窗口、拖放、IME、托盘、无障碍服务端桥接、真实 GPU 设备和发布包仍需
  设备验收，离屏结果不替代这些验收。
