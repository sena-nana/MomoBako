# 项目需求：Git 风格 Asset Repository（资源仓库）管理系统

## 项目背景

设计并实现一个面向文件资源管理的 Repository 系统。

系统管理多个独立资源库（Repository）。

每个资源库本质上是一个普通文件夹，但拥有自己的 Metadata 数据库、索引、缓存和历史记录。

设计理念参考 Git：

* Repository 是独立单元
* 用户文件保持原样
* 系统数据存储在隐藏目录 `.meta`
* Metadata 与文件解耦
* 支持历史记录与版本追踪
* 支持多个程序同时访问
* 支持未来扩展为分布式系统

---

# 核心概念

## Repository

每个资源库是一个普通目录：

AnimeAssets/
├── Characters/
├── Backgrounds/
└── .meta/

WorkDocs/
├── Reports/
├── Contracts/
└── .meta/

Repository 必须拥有：

* 唯一 RepoId（UUID）
* Repository Metadata
* Metadata 数据库
* 搜索索引
* Revision 历史
* 缓存

---

## Asset

Repository 中的每个文件称为 Asset。

Asset 具有：

* AssetId（UUID）
* 文件路径
* 文件状态
* Metadata
* Revision 历史

Asset 不允许使用路径作为唯一标识。

路径变化不影响 AssetId。

---

# 目录结构

Repository/

├── User Files
│
├── FolderA/
├── FolderB/
│
└── .meta/
├── repository.json
├── metadata.db
├── revisions.db
├── cache/
├── thumbnails/
├── logs/
└── indexes/

---

# Repository Metadata

repository.json

示例：

{
"repoId": "uuid",
"name": "AnimeAssets",
"createdAt": "...",
"schemaVersion": 1
}

要求：

* Repository 唯一身份
* Schema 版本管理
* Repository 配置管理

---

# 文件身份识别

Asset 必须拥有永久 UUID。

记录：

* assetId
* path
* filename
* extension
* size
* createTime
* modifyTime
* hash
* status

要求：

* 重命名不影响 AssetId
* 移动不影响 AssetId
* Metadata 永远绑定 AssetId

---

# Metadata 系统

采用动态 Key-Value 模型。

支持：

* string
* number
* boolean
* datetime
* json

示例：

{
"title": "Cat",
"rating": 5,
"favorite": true,
"tags": [
"cat",
"cute"
]
}

要求：

* 任意扩展字段
* 不需要修改数据库结构
* 用户自定义字段

---

# Revision 系统

所有 Metadata 修改必须产生 Revision。

记录：

* revisionId
* assetId
* timestamp
* operation
* before
* after
* source

支持：

* Undo
* Redo
* 历史查询
* 审计

参考 Git Commit 思想。

---

# 数据库存储

使用 SQLite。

要求：

* WAL 模式
* 多进程访问
* ACID 事务
* 索引优化

设计以下表：

## repositories

Repository 信息

## assets

文件记录

## metadata

Metadata Key-Value

## tags

标签索引

## revisions

历史记录

## events

文件系统事件

## schema_version

数据库版本

输出完整 SQL Schema。

---

# 文件系统监听

支持：

Windows

* FileSystemWatcher

Linux

* inotify

macOS

* FSEvents

监听：

* 新增
* 删除
* 修改
* 移动
* 重命名

---

# 自动同步

新增文件：

自动创建 Asset

自动生成 Metadata

自动建立索引

删除文件：

标记 deleted

不立即删除 Metadata

重命名：

更新路径

移动：

更新路径

---

# Repository Service

设计独立后台服务。

职责：

* Repository 管理
* 文件监听
* Metadata 管理
* Revision 管理
* 索引管理
* 搜索服务

禁止多个程序直接操作数据库。

所有客户端通过 Service API 访问。

---

# 并发控制

支持多个客户端同时访问同一个 Repository。

例如：

* 桌面客户端
* Web 客户端
* AI 插件
* 自动标签服务

要求：

## WAL 模式

支持多读单写

## 乐观锁

Metadata 包含：

* version
* updatedAt

更新时检查版本号。

## 冲突处理

支持：

* success
* conflict
* merged

设计完整冲突解决机制。

---

# Repository Registry

系统允许同时管理多个 Repository。

设计全局 Registry。

结构：

MetaHub/
└── repositories.db

记录：

* repoId
* name
* path
* status

支持：

* 创建仓库
* 打开仓库
* 删除仓库
* 导入仓库
* 导出仓库

---

# 全局搜索

设计跨 Repository 搜索能力。

支持：

* 文件名
* 标签
* Metadata
* 路径
* 全文

示例：

tag = "cat"

rating >= 4

author = "Tom"

搜索结果返回：

* repoId
* assetId
* path
* metadata

要求设计索引策略。

---

# 缓存系统

设计：

Metadata Cache

Thumbnail Cache

Query Cache

要求：

* LRU
* 自动失效
* 配置化

---

# 插件系统

支持未来扩展：

AI 自动标签

OCR

视频分析

人脸识别

向量检索

云同步

自定义 Metadata Provider

要求设计插件架构。

---

# API 设计

提供 REST API 或 gRPC。

至少包含：

Repository API

Asset API

Metadata API

Revision API

Search API

Plugin API

给出完整接口设计。

---

# 非功能需求

支持：

10万文件稳定运行

100万文件可扩展

要求：

* 高性能搜索
* 增量扫描
* 崩溃恢复
* 数据一致性
* 低内存占用

---