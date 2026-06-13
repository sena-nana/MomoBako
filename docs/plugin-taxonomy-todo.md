# 插件分层后续 TODO

本文记录插件类别与职责分层已经落地后的剩余工作。当前已完成 manifest 层的 `category`、依赖、hook、贡献声明，以及第一批官方 `library-kind`、`parser`、`service` manifest-only 插件；以下 TODO 仍需继续实现。

## P0 核心宿主能力

- 候选确认队列：统一承载 parser、provider、library-kind 产出的 metadata 补全、封面、归并、重命名和移动候选。
- metadata 合并与冲突处理：按字段来源、置信度、人工编辑优先级生成可确认 diff。
- 批量整理计划：核心负责生成预览、冲突检查、事务执行、审计记录和回滚依据；插件只声明规则和候选。
- 动作执行器扩展：现有核心动作表已可承载 Eagle actions 和 metadata/tag 原生步骤；后续为移动、复制、重命名、删除、导出等危险步骤补齐 dry-run、确认、审计和回滚。
- 统一进度模型：支持观看、收听、阅读、游玩状态，记录当前位置、百分比、最后打开时间、评分和收藏。
- 播放列表与 PiP 宿主：核心维护队列、快捷键、窗口状态和进度回写，media preview 和库类型插件只通过 hook 接入。
- 下载任务队列：核心维护任务状态、失败重试、输出路径和候选写入，下载服务插件只提供执行能力。

## P0 插件运行时与编排

- 插件依赖解析：已读取 manifest 的 `requires` / `optional` 并在插件管理中展示缺失依赖、禁用原因和降级结果；当前已先接入通用插件调用入口，required 依赖不可用会拦截执行，optional 依赖不可用会在调用响应中记录降级。完整 Hook 调度器的运行记录仍待实现。
- 权限授权：已在插件管理中展示 manifest `permissions`；后续按 `readRepository`、`readMetadata`、`readArchive`、`network`、`runCommand`、`deriveAI`、`useProvider`、`writeCandidates`、`suggestRename`、`suggestMove` 做核心授权。
- Hook 调度器：把 `playlist`、`pip`、`progress`、`candidateQueue`、`batchOrganize`、`downloadQueue`、`metadataMerge`、`renameMove`、`auditLog`、`search` 接入核心动作表和运行记录。
- 降级提示：缺 library-kind 回退通用库；缺 parser 只保留原始文件信息；缺 preview 显示不可预览；缺 service 禁用对应动作。
- 插件管理 UI：按 `source`、`library-kind`、`parser`、`preview`、`service` 分组，显示依赖、权限、hook 和贡献内容。

## P1 Parser 与 Provider

- Parser 注册表：按扩展名、MIME、目录信号和探测结果选择解析器，输出标准化候选。
- 本地媒体解析：接入音频、视频、图片、字体、电子书、压缩包 parser 的实际 runtime。
- 容器索引：压缩包、CBZ/CBR、安装包不默认解包，先记录内部清单、入口文件、封面候选和重复线索。
- Provider 标准协议：统一搜索候选、详情补全、封面候选、外部 ID、来源、置信度、限流和失败状态。
- 手动联网搜刮：MusicBrainz、TMDB、Bangumi 等 provider 必须由用户触发或库级设置开启。

## P1 Library-kind 体验

- 库类型选择：支持仓库级或文件夹级 `libraryKind`，并允许通用视图降级。
- 配置化视图：根据 `library-kind` 的字段、facets、sortFields、views 渲染列表列、筛选器、详情字段和快捷动作。
- 文件夹中心模型：先以文件夹表达作品、专辑、剧集、项目，不急于引入强作品实体。
- 类型化质检：缺封面、缺标题、缺作者、缺日期、缺字幕、低清晰度、重复文件、字段冲突、无法识别文件夹。
- 官方库类型迭代：音频、ASMR、影视、番剧、漫画、电子书、图片、设计、3D、字体、游戏、软件、压缩包、项目逐步补齐字段模板和视图细节。

## P2 扩展能力

- AI 增强：OCR、ASR、封面识别、标签建议、摘要、相似资源检索只作为增强能力，不作为基础扫描依赖。
- 第三方插件分发：完善用户插件安装、兼容检查、权限提示、签名或信任分级。
- 插件 schema migration：manifest 声明 schemaVersion/coreApiVersion/migration，核心执行迁移并兼容旧数据。
- 健康度统计：按库类型展示可识别率、元数据完整率、重复率、封面覆盖率、进度覆盖率。

## 验收标准

- 插件缺失时仓库仍能打开并逐层降级。
- 插件不能直接写 metadata 或移动/重命名文件，只能创建候选。
- 所有危险写操作由核心确认、执行、审计。
- 官方插件和第三方插件使用同一 manifest taxonomy。
- 新增库类型不需要修改核心 UI 页面，只需声明字段、视图、hook 和依赖。
