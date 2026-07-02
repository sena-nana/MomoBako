# EagleLibraryChanger

把 EagleLibrary 转换成 MomoBako 可直接导入的本地仓库。

## 用法

```bash
python External/EagleLibraryChanger/convert.py \
  --input External/Examples/TestBench.library \
  --mode copy \
  --output External/Examples/TestBench.momo
```

可选参数：

- `--name`：指定仓库名称，默认使用输出目录名
- `--mode`：素材导入模式，固定为 `copy` 或 `move`
- `--dry-run`：只预览，不写文件
- `--yes`：跳过确认直接执行
- `--force`：允许复用已存在的空输出目录

## 行为

- 读取 Eagle 顶层 `metadata.json`、`tags.json` 等文件生成转换计划
- 把 `images/<assetId>.info/` 中的原文件按 `--mode` 复制或移动到标准目录
- 将 Eagle `isDeleted: true` 素材写入 MomoBako `.momo/trash` 和 `.momo/trash.json`，保留可恢复的原路径语义
- 保留 Eagle 素材来源与原始属性：`url` 写入 `metadata.link`，导入/创建/修改时间写入 `addedToLibraryAt`、`fileCreatedAt`、`fileModifiedAt`，宽高与原始大小写入 `width`、`height`、`originalSizeBytes`
- 把 Eagle 缩略图迁移到输出仓库的 `.momo/thumbnails/`
- 将可等价表达的 Eagle `smartFolders` 与 `saved-filters.json` 条目写入 MomoBako 智能文件夹
- 将非空 `actions.json` 写入 MomoBako 仓库动作表；导入时不执行任何动作
- 生成 `.momo/repository.json` 与 `.momo/metadata.db`
- 输出仓库旁路报告 `<输出目录名>.import-report.json`，记录路径映射、重名处理、能力降级和警告

模式说明：

- `copy`：复制 Eagle 原文件与缩略图，保留源目录
- `move`：移动 Eagle 原文件与缩略图，消费源目录

## 能力补全边界

本转换器以 MomoBako 原生能力承接 Eagle 仓库数据：

- 文件归属：多文件夹素材转为主路径和 alias 路径，保持同一素材的多位置访问。
- 回收站：`isDeleted` 素材进入 `.momo/trash`，保留可恢复路径。
- 快捷入口：`quickAccess` 写入 repository shortcuts。
- 标签组：`tagsGroups` 和 starred tags 写入 repository tag groups。
- 智能文件夹：支持包含条件、排除条件、数值/日期范围、排序、数量限制和 OR 语义。
- Actions：`actions.json` 写入 repository actions；可识别的 metadata/tag 步骤转为可手动执行的原生动作，未知或危险步骤保留 raw payload 并禁用。

明确不做 Eagle 文件夹密码迁移：

- 不读取、保存或导出 Eagle 明文 `password`。
- 不把 Eagle 密码转换为 MomoBako 访问控制、加密、锁定目录或权限系统。
- 仅保留 `protected=true` 与 `passwordTip` 作为迁移提示，方便用户识别原 Eagle 中受保护的文件夹。

## 降级规则

- 多文件夹素材写入主路径和 alias 路径；本地文件优先硬链接，失败时回退复制
- 已删除素材进入 MomoBako 回收站，报告中标记 `isDeleted`、`status` 与 `trashRelativePath`
- 智能文件夹支持 OR、排除文本、排除路径、排除数值/日期范围、排序和数量限制；无法识别的字段才会写入跳过报告
- 转换后的智能文件夹均为根级模板，不保留 Eagle 层级，避免父级继承语义改变结果
- 来源、时间、尺寸和大小字段会作为通用 metadata 保留，并可被智能文件夹的日期/数值过滤引用
- `actions.json` 中可识别的评分、收藏、注释、链接、metadata 和标签步骤会转成原生动作步骤；未知、外部或危险步骤保留 raw payload，标记为 `unsupported`，并默认禁用包含它的动作
- 无法映射到 MomoBako 原生字段或通用 metadata 的 Eagle 信息不会写入仓库
- 无法完整转换的产品能力以每次导出的 import report 为准；`External/todo.md` 只说明报告来源，不再用空静态列表声明“暂无缺口”
