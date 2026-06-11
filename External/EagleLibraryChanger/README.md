# EagleLibraryChanger

把 EagleLibrary 转换成 MomoBako 可直接导入的本地仓库。

## 用法

```bash
python External/EagleLibraryChanger/convert.py \
  --input External/Examples/TestBench.library \
  --output External/Examples/TestBench.momo
```

可选参数：

- `--name`：指定仓库名称，默认使用输出目录名
- `--dry-run`：只预览，不写文件
- `--yes`：跳过确认直接执行
- `--force`：允许复用已存在的空输出目录

## 行为

- 读取 Eagle 顶层 `metadata.json`、`tags.json` 等文件生成转换计划
- 把 `images/<assetId>.info/` 中的原文件移动到标准目录
- 将 Eagle `isDeleted: true` 素材写入 MomoBako `.momo/trash` 和 `.momo/trash.json`，保留可恢复的原路径语义
- 把 Eagle 缩略图迁移到输出仓库的 `.momo/thumbnails/`
- 将可等价表达的 Eagle `smartFolders` 与 `saved-filters.json` 条目写入 MomoBako 智能文件夹
- 生成 `.momo/repository.json` 与 `.momo/metadata.db`
- 输出仓库旁路报告 `<输出目录名>.import-report.json`，记录路径映射、重名处理、能力降级和警告

## 降级规则

- 多文件夹素材只保留第一个文件夹，其余归属仅写入报告
- 已删除素材进入 MomoBako 回收站，报告中标记 `isDeleted`、`status` 与 `trashRelativePath`
- 智能文件夹采用准确优先策略；含 OR、排除、日期、尺寸、排序、数量限制或未知条件的 Eagle 筛选不会创建 MomoBako 智能文件夹，只写入报告
- 转换后的智能文件夹均为根级模板，不保留 Eagle 层级，避免父级继承语义改变结果
- 无法映射到 MomoBako 原生字段的 Eagle 信息不会写入仓库
- 无法完整转换的产品能力统一记录在 `External/todo.md`
