# Nana `Rgba32Float` 离屏回退方案

## 结论

采用能力探测后的自动回退，不改离屏目标格式，也不创建第二棵 UI 树。
`Rgba32Float` 只用于 Nana compositor motion 的可选 GPU 评估管线；PNG 目标仍使用
`BGRA8_UNORM_SRGB`。适配器不支持 `RENDER_ATTACHMENT` 时，Runtime 继续由同一个
`SceneWgpuPainter` 绘制，并把 motion 评估留在 CPU。

不能把 motion 中间纹理直接改成 BGRA8：它承载浮点 transform/opacity 评估结果，
改变格式会损失精度并改变 shader 的契约。

## NanaUI 修复边界

上游 `MotionGpuResources::new_with_policy` 应按适配器能力创建可选管线：

1. 用 `adapter.get_texture_format_features(Rgba32Float)` 检查
   `RENDER_ATTACHMENT`。
2. 不支持时返回 `eval_pipeline: None`，禁止 `expect("motion pipeline")` 触发设备级 panic。
3. `SceneWgpuPainter` 将该状态标记为 CPU motion；quad/text/host texture 的正常绘制
   和目标 PNG 不受影响。
4. 支持 `Rgba16Float` 且 shader 契约明确兼容时才可作为 GPU 候选，否则直接 CPU 回退。

这项改动必须在 NanaUI 依赖仓库落地并更新 MomoBako 的固定 revision；MomoBako 不应
复制 Nana 的 renderer 或维护第二份 UI 树。

## 验收要求

- 支持 `Rgba32Float` 的适配器：GPU motion + 真实 PNG。
- 不支持 `Rgba32Float` 的适配器：CPU motion + 真实 PNG，进程不 panic。
- 场景清单记录 `motion_backend`（`gpu` 或 `cpu`）、适配器和回退原因。
- 1200×800/960×600、浅色/深色及全部压力场景的 PNG/语义/布局/命中结果继续来自
  同一个 `RuntimeAgentSession`。
