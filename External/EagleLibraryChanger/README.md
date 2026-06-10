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
- 把 Eagle 缩略图迁移到输出仓库的 `.momo/thumbnails/`
- 生成 `.momo/repository.json` 与 `.momo/metadata.db`
- 输出仓库旁路报告 `<输出目录名>.import-report.json`，记录路径映射、重名处理、能力降级和警告

## 降级规则

- 多文件夹素材只保留第一个文件夹，其余归属仅写入报告
- 无法映射到 MomoBako 原生字段的 Eagle 信息不会写入仓库
- 无法完整转换的产品能力统一记录在 `External/todo.md`
