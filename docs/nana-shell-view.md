# 壳层绑定式视图：分块和改常驻的约定

壳层正从「每条消息整块重挂」迁到 NanaUI 的绑定式视图：树只建一次，状态放进信号或 Store，绑定只改自己写的那个字段。这篇写分块的结构，以及把一块区域改成常驻时要做的事。总体数据流见 [迁移边界](./nana-migration.md) 的「壳层视图同步」。

## 分块和各自的文件

`ShellView`（`src-nana/src/shell/view_host.rs`）只做调度：写热信号和标题栏，按同一套流程调三块内容，再把各块的根放进 AppShell 和工作区的槽位。三块都实现 `ShellPart`（`view_part.rs`）：

| 方法 | 做什么 |
| --- | --- |
| `signals(model)` / `new(signals)` | 常驻的信号和 Store，在骨架的挂载闭包里建，跟骨架一起回收，整块重挂时不重建 |
| `mount(cx, model, mode)` | 首次挂载，或工作台排法变了时从头挂，不等输入法组合 |
| `sync(model)` | 只写本块的信号。每次整体同步都调，组合输入中也调 |
| `needs_remount(model)` | 有没有必须重挂才能跟上的变化 |
| `remount(cx, model)` | 重挂需要重挂的部分 |
| `composing(document)` | 焦点在本块里、输入法还有预编辑：这时 `ShellView` 把重挂记成延后，组合结束后的下一帧补挂 |

重挂换下来的旧内容和要找回的焦点、选区、滚动放进 `Swap`，等新根放进槽位以后由 `ShellView` 收尾。

| 块 | 模块 | 切换 | 现状 |
| --- | --- | --- | --- |
| 侧栏 | `view_part_sidebar.rs`，投影在 `sidebar_project.rs` | 无 | 常驻：`SidebarSignals`（几个信号加播放集、文件夹树、智能文件夹树三份 Store）建在骨架作用域里，同步只写变了的；排法变了（收起再展开）才整块重挂 |
| 主区 | `view_part_primary.rs`，路由在 `route_*.rs` | `dynamic(RouteSlot)`，按 `RouteKey` | 启动页、缺失仓库页、空库页常驻；其余路由的分支整块重挂 |
| 浮层 | `view_part_overlay.rs` | 按 `OverlayKey` 换整块 | 各浮层整块重挂；没有浮层时槽位为空 |

并行改区域时各改各的文件：

- 浮层和对话框：`view_part_overlay.rs`（只改自己那种浮层在 `overlay_branch` 里的分支和 `needs_remount`），以及 `sidebar_dialogs.rs`、`sidebar_popover_view.rs`、`sidebar_tree_view.rs` 的菜单、`admin_view.rs` 的任务弹层、`source_prompt.rs`、`files_menu.rs` 等浮层视图。
- 文件页和详情：`route_files.rs` 和 `files_*.rs`、`inspect_*.rs`。
- 侧栏和其余路由：`view_part_sidebar.rs`、`sidebar_*.rs`，以及 `route_search.rs`、`route_playlists.rs`、`route_settings.rs`、`route_admin.rs`、`route_startup.rs`、`route_missing.rs`、`route_empty.rs`。

`view_host.rs` 和 `view_part.rs` 不必动。`view_part_primary.rs` 里只改自己路由那一行：`resident`、`resident_view`、`legacy_view` 和 `RouteSignals`。

## 主区的路由键和分支

`RouteKey` 是身份键：启动状态、设置页、区域（有仓库、丢失、空库）和工作区面板合起来决定主区显示哪种页面。`dynamic` 的键是 `RouteSlot { route, version }`：

- 常驻路由的 `version` 固定为 0。分支只在进入路由时建一次，之后 `sync` 只写 `RouteSignals` 里它的信号。分支在刷新时由 `dynamic` 建，拿不到 `&ShellViewModel`，只能读信号；热信号要显式传进去（`hot::prop` 只在旧视图挂载期间有值）。
- 旧视图路由的内容都在本线程挂成脱离树的一块（建的时候在自己的作用域里，旧视图里建的信号有归属）。
  - 留在同一个路由里、ViewModel 版本变了：`remount` 当场把新内容放进路由容器，`ShellView` 收尾时卸掉旧内容、找回焦点、选区和滚动，和改动前一样同步完成，紧接着的按键落在新节点上。
  - 换到旧视图路由：新内容放进交接处，版本号加一写进键；下一次刷新 `dynamic` 换出空分支，分支的 `on_mount` 把这块内容放进路由容器、卸掉上一块，再找回状态。
  - 两种情况下主区外框、路由容器、侧栏和浮层都不动。

路由容器（键 `primary-route`）夹在外框和分支之间，`css!` 写成和 `Stack::fill_column(0)` 一样的排版，分支的样子和直接放在外框里一样。

## 改成常驻的步骤

以 `route_startup.rs` 为样板。

1. **投影**：写一个 `XxxView::project(model) -> Self`，`#[derive(Clone, Debug, PartialEq)]`，只放视图要读的值，按视图要显示的样子算好（文案、拼好的字符串、列表）。给 `project` 写单测：取值对，同一状态两次投影相等。
2. **信号**：投影按「谁一起变、谁读它」拆成几个信号，`XxxSignals::new(model)` 建在本块的 `signals()` 里（主区路由放进 `RouteSignals`），`write(view)` 对每个信号用 `try_set_if_changed`，值没变不触发绑定。
3. **视图只建一次**：文字和样式字段用 `.prop::<T, W>(信号或闭包)` 绑定；可有可无的一块用 `.visible(..)`，节点留着、不占布局，不改兄弟之间的间距（`when` 会多一个容器，空着也占一个间距）；结构真的要换时才用 `when` / `dynamic`。初值可以 `get_untracked()` 取，绑定在挂载时会再写一次同样的值。
4. **去掉整块重挂**：本块（或本路由）的 `needs_remount` 不再看 ViewModel 版本，只在结构变了时返回真；主区路由在 `resident` 里登记。
5. **测试**：见下文。

### Signal 还是 Store

- 几个平铺的值、读者不多：`Signal<T>`，`T` 是投影里的一段结构，绑定用 `move || sig.with(|v| v.field.clone())` 取字段。值变了以后读者重跑，按字段比较，没变的字段不打补丁，成本很低。
- 嵌套的状态，或者列表里每行的字段各自在变、行数又多：`store(..)` 加 `#[derive(Store)]`（壳层已开 `view-macro`），行用 `keyed(|item| item.id).each(..)`，行内绑定读 `item.title()` 这样的路径，改一行只重跑读这一行的绑定。整体 `set` 列表会让所有行的绑定重跑一遍再按字段比较，大列表写回时只改变了的项（`at(&key).set(..)`、`push`、`retain`）。
- 不要把整个 ViewModel 放进一个信号。

### 列表和滚动

- 跟着数据增删的行用 keyed `each`，长列表用 `each_virtual`（文件页已经在用）。key 是身份：有 id 用 id。行的结构由某个状态决定时（启动步骤的圆标由状态决定）把状态放进 key，状态变了整行重建，别的行不动；没有 id 的行（启动日志）用「位置 + 内容」。
- `each(..).gap(n)` 的容器就是 `Stack::column(n)`，用它替换原来手写的那一列，不多一层节点。
- 滚动容器写 key，并让 key 带上内容身份（例如 `workspace-page-scroll-{区域}-{面板}`）：换内容时从顶部开始，同一内容重挂时 `remount_state` 按键路径找回偏移。常驻以后滚动容器不再重建，偏移自然留着。

### 输入框的四条纪律

1. 受控输入用 `.model(Signal<String>)`，不要每次同步重设 `TextInput` 的值。
2. ViewModel 的值写回信号时用 `ModelField`：只在 ViewModel 的值相对上次投影变了时写。按键已经由 `.model` 写进信号，对应的消息可能还排在后台消息后面；这时把 ViewModel 里较旧的草稿写回去，下一次刷新就把刚打的字冲掉。
3. `update` 和 `prepare` 里只写信号，不调用 `flush_reactive`。刷新由输入路由和帧开头完成；中途刷新会让一批消息里的中间值落到输入框上。
4. 输入框的事件一律 `cx.dispatch_program_all(..)` 发消息，由归约改 ViewModel，不在事件里改信号以外的状态。

### 事件闭包

事件处理器在节点的整个生命周期里都在，常驻以后不会因为重挂换成新的。处理器只能：

- 发意图消息，参数是建节点时就确定、之后不会变的值（行的 id、常量），由归约按当时的 ViewModel 决定怎么做；或者
- 用 `sig.get_untracked()` 现读信号里的值。

不要在处理器里捕获建视图那一刻从 ViewModel 抄来的、之后会变的值。旧视图里的拖放标志（`file_drop_flags`）就是这样抄下来的，它能用是因为内容每次都整块重挂；改常驻时要换成意图消息或者读信号。

### 测试

- 投影的单测：取值、相等性，以及哪些消息改投影、哪些不改（`view_part_sidebar_tests.rs`）。
- 用 `view_harness::ShellHarness`：
  - `assert_same_as_fresh_mount()`：增量更新后的文档和同一 ViewModel 新挂的文档按无障碍树逐个比较（角色、名称、值、布局盒），每一步更新后都调。
  - 「N 次无关更新后节点 id 不变」：用 `harness.keyed("键")` 记下节点，`apply` 几条无关消息并 `flush` 后再取，应该是同一个节点；相关更新后绑定的字段原地变，结构变了的那一行才换（`route_startup_tests.rs`）。
  - `view_stats().remounts` 不变：动效帧、播放推进和常驻区域的更新都不该重挂。
  - `route_branch()`、`sidebar_root()`、`content_roots()` 判断哪一块换了（`view_part_primary_tests.rs`）。
- 改完跑 `scene_shots` 出全部场景，和改动前逐字节比较。

## 不再扫描文档补登

树常驻以后，后来才建出来的节点不会再被「挂载后扫一遍」补登，下面三件事都改成了声明式或全局：

- **Escape**：`MomoBakoApplication::input_event` 收到运行时没处理掉的 Escape 按下（`prevent_default` 为假）时，按 `escape_layer(model)` 判断还有没有能关的层，有就发 `GapMessage::Escape`，归约时 `dismiss_top` 照同一个顺序关掉最上面一层。焦点在哪都一样。下拉框收起选项这类控件自己处理的 Escape 不会再关别的层。新的可关闭层加进 `EscapeLayer`。
- **系统文件拖放**：在建拖放目标的视图里给它一个 `node_ref`，调用 `input::accept_file_drops(node_ref)`，挂上后由 `on_mount` 登记。
- **字段的无障碍名**：标题和控件分开写的字段，标题加 `.node_ref(caption)`，下拉框这类没有自己名称的控件加 `.labelled_by(caption)`（`sidebar_dialogs.rs` 的 `select_field`）。没有可见标题时写控件自己的 `label`。

## NanaUI 缺口

这次搭底座时遇到、在 MomoBako 里绕开的：

- `dynamic` 没有同步重建单个结构块的公开入口（`run_structural_now` 是 crate 内部的），分支只能在下一次刷新时换，而 `update` 里又不能刷新。换到旧视图路由时因此「同步挂好、刷新时交接」；同一路由里的更新不经过 `dynamic`，直接换路由容器里的内容。
- 结构块的容器只收 `class` / `class_when` / `css` / `visible`，不能给一个现成的 `Stack` 或 `node_ref`。路由容器的排版只好开 `view-macro` 写 `css!`，找容器靠键。
- `mount_view_detached` 挂出的根不在装配键表里，根节点的键路径是父节点的路径，自己的键不出现。`remount_state` 对根节点按父路径加类型找回，能用，但根的键（例如 `settings-scroll`）查不到。
- 不挂在 `OverlayHost` 下的 `Dialog` 收不到 Escape 和点外面的关闭请求（`DialogCloseRequested` 只发给 `activate_overlay` 打开的对话框），壳层的 Escape 全局处理。
- 元素上没有声明式的拖放目标，要靠 `on_mount` 调 `set_drop_target_node`。
- AppShell 只在自己投影时按「有没有子节点」决定 overlay 槽位挡不挡点击；槽位里的内容由结构块在刷新时换，它不会跟着重新判断。所以浮层块在没有浮层时把槽位清空，不用常驻的浮层根。
