# MomoBako 样式标准

视觉语言见 `DESIGN.md`。本文写 Nana 原生界面里样式落在哪里、按什么规矩写。原来 Vue 界面的 CSS 分层（`src/styles/*.css` 和 `.ui-*` 标准类）随 Vue 源码一起删了，Vue/Tauri 源码最后一次出现在提交 `7bc847c`。

## 入口分层

- `src-nana/src/theme_map.rs`：`DESIGN.md` 的令牌（`--bg`、`--text`、`--accent` 等）到 Nana `SemanticPalette` 角色的对应，以及字号角色、侧栏宽度和圆角刻度。颜色的具体数值由 NanaUI 主题持有，这里不复制。
- `src-nana/src/appearance.rs`：设置里的主题和圆角落到 Nana 主题；对话框的站位、分区、开合动效和遮罩写成主题配方（`DialogRecipe`、`EffectTokens`），深浅两套相同。
- `src-nana/src/shell/admin_style.rs`、`files_style.rs`、`palette.rs`、`motion.rs`：各区域共用的布局辅助、调色和动效时长。
- `src-nana/src/capability.rs`：本构建编译的 Nana 组件目录。组件和旧界面的对应见 [Nana 组件对应](../nana-component-map.md)。

## 标准组件

- 按钮用 NanaUI `Button`：主操作 `ButtonKind::Primary`，普通操作 `ButtonKind::Ghost`，危险操作 `ButtonKind::Danger`；纯图标按钮用 `IconButton`。按钮不用边框区分类型，底色、字色和边线都取主题角色。
- 对话框用 `Dialog` / `ConfirmDialog`（`src-nana/src/shell/overlay_dialog.rs`），常驻在浮层层下，站位、分区和动效来自主题配方。
- 列表、分段控制、选择框、输入框等优先用 `capability.rs` 里已编译的组件，没有现成组件时再在壳层组合，并记进组件对应文档。

## 扩展规则

- 颜色只取主题角色。画按钮的壳层文件里不写颜色字面量，也不用 `ButtonPaintOverride`；`src-nana/src/theme_map_button_tests.rs` 扫源码检查，确实要写死、两套主题共用的颜色列进它的允许清单并写清出处。
- 圆角用 `RadiusTier` 刻度，不写固定像素；刻度随设置里的圆角半径变（`theme_map::corner_metrics`）。
- 字号用 `theme_map.rs` 的字号角色，不随窗口宽度缩放。
- 页面级的布局辅助放进对应区域的样式模块，不在视图里重复同一组尺寸。

## 评审清单

- 控件是否用了标准组件和按钮种类。
- 深浅主题是否只依赖主题角色。
- 新加的颜色、圆角和字号是否来自 `theme_map.rs` 的对应。
- 按钮、输入、列表项、菜单和弹层的 hover、active、disabled、focus 状态是否与现有组件一致。
