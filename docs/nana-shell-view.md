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
| 主区 | `view_part_primary.rs`，路由在 `route_*.rs` | `dynamic(RouteSlot)`，按 `RouteKey` | 启动页、缺失仓库页、空库页、搜索（含空库搜索）、设置、日志、拓展和动作页常驻；播放集页已写成投影加绑定（`player_playlist_page.rs`），路由仍是旧视图，接上播放条的旧视图岛后再登记常驻；其余路由的分支整块重挂 |
| 浮层 | `view_part_overlay.rs` | 按 `OverlayIdentity` 换块 | 常驻：身份不变时只经会话写信号；对话框走统一框架；没有浮层时槽位为空 |

并行改区域时各改各的文件：

- 浮层和对话框：`view_part_overlay.rs`（`overlay_branch` 里自己那种浮层的分支，身份的结构见 `OverlayIdentity`），对话框框架 `overlay_dialog.rs`、`overlay_dialog_fields.rs`，会话 `overlay_session.rs`，以及 `sidebar_dialogs.rs`、`sidebar_smart_dialog.rs`、`sidebar_popover_view.rs`、`sidebar_tree_view.rs` 的菜单、`admin_view.rs` 的任务弹层、`source_prompt.rs`、`files_menu.rs`、`workspace_dialogs.rs`、`workspace_export_dialog.rs` 等浮层视图。
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

工作台排法变了（收起或展开侧栏）时外框整块重挂，常驻路由的分支跟着重建。两种排法下外框的结构不同：工作台里外框自己是脱离挂载的根，键 `workspace-body` 不进键路径；主区独占时外面多一层底色，`workspace-body` 进了键路径。所以按键路径找回焦点和滚动在换排法时对不上（旧视图内容一样），换排法以后滚动从顶部开始。

### 首页筛选栏

筛选栏属于首页外框，有仓库的首页路由都可能显示它。信号 `FilterBarSignals` 在 `RouteSignals::filter`，除启动页和设置页以外每次同步都写（关着时不算候选和库类型快捷方式）。常驻首页路由在外框里嵌 `inspect_search_view::resident_filter_bar(signals.filter)`，显隐跟「有仓库且筛选栏打开」走 `.visible`；旧视图路由的 `route_home::page` 仍调 `filter_bar(model)`，它用这一刻的投影建一份一次性的信号，再建同一个视图。常驻首页路由的外框和滚动主体暂用 `route_search::{home_page, home_scroll}`，排版和 `route_home::page`、`scroll_body` 相同。

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
- 投影出的整份新列表写回 Store 时按键做差异：删掉没有了的、按位置插入新的、只改内容变了的，顺序变了再排（侧栏的 `sidebar_project::sync_rows`；搜索和管理页的 `admin::bind::sync_rows` 只处理相对顺序不变的情形，重排时整体写）。新日志到达只建新的一行。行内绑定用 `row_text` / `row_flag`，读 `try_with`，行已经删掉时读默认值。
- 行的类型不一定要派生 `Store`：行内绑定整行读 `item.try_with(..)`，也只在这一行变了时重跑。要按字段取下一层列表时（分组里的插件卡片 `group.cards()`）才派生。
- 不要把整个 ViewModel 放进一个信号。

### 列表和滚动

- 跟着数据增删的行用 keyed `each`，长列表用 `each_virtual`（文件页已经在用）。key 是身份：有 id 用 id。行的结构由某个状态决定时（启动步骤的圆标由状态决定）把状态放进 key，状态变了整行重建，别的行不动；没有 id 的行（启动日志）用「位置 + 内容」。
- `each(..).gap(n)` 的容器就是 `Stack::column(n)`，用它替换原来手写的那一列，不多一层节点。
- 结构块的容器不能直接给一个 `Stack`。要替换一行有样式的芯片（筛选芯片行、结果行右侧）时用 `.horizontal(n)`，再用 `css!` 补上原来那个 `Stack` 写过的字段（`flex-wrap`、`justify-content`、`width`、`flex-grow` 等），改完跑 `scene_shots` 逐字节比较。竖排父容器里的 `dynamic` 容器不改排版：内容变了整块换、里面又没有可聚焦控件的小块（插件卡片的芯片、原因和四段）可以这样包，可有可无时再给容器 `.visible(..)`。
- 一行里固定几个、可有可无的块（日志卡片的来源芯片和元数据芯片）直接建出来，各自 `.visible(..)`，不用嵌套列表。
- 滚动容器写 key，并让 key 带上内容身份（例如 `workspace-page-scroll-{区域}-{面板}`）：换内容时从顶部开始，同一内容重挂时 `remount_state` 按键路径找回偏移。常驻以后滚动容器不再重建，偏移自然留着。

### 输入框的四条纪律

1. 受控输入用 `.model(Signal<String>)`，不要每次同步重设 `TextInput` 的值。
2. ViewModel 的值写回信号时用 `ModelField`：只在 ViewModel 的值相对上次投影变了时写。按键已经由 `.model` 写进信号，对应的消息可能还排在后台消息后面；这时把 ViewModel 里较旧的草稿写回去，下一次刷新就把刚打的字冲掉。`ModelField` 是 `Copy` 的句柄（上次投影的值也放在信号里），可以直接放进常驻的信号结构；列表行里的输入框（插件设置字段）用 `admin::bind::row_draft` 在行的作用域里建，读这一行在 ViewModel 里的值同步。
3. `update` 和 `prepare` 里只写信号，不调用 `flush_reactive`。刷新由输入路由和帧开头完成；中途刷新会让一批消息里的中间值落到输入框上。
4. 输入框的事件一律 `cx.dispatch_program_all(..)` 发消息，由归约改 ViewModel，不在事件里改信号以外的状态。

### 事件闭包

事件处理器在节点的整个生命周期里都在，常驻以后不会因为重挂换成新的。处理器只能：

- 发意图消息，参数是建节点时就确定、之后不会变的值（行的 id、常量），由归约按当时的 ViewModel 决定怎么做；或者
- 用 `sig.get_untracked()` 现读信号里的值。

不要在处理器里捕获建视图那一刻从 ViewModel 抄来的、之后会变的值。旧视图里的拖放标志（`file_drop_flags`）就是这样抄下来的，它能用是因为内容每次都整块重挂；改常驻时要换成意图消息或者读信号。设置页的复制按钮、日志的暂停、插件卡片的启用和动作页的执行都是点下去时现读信号。

`style::action` 按钮禁用时整体 45% 不透明。绑定禁用用 `admin::bind::ActionDisabled`，禁用标志和透明度一起写；只绑 `fields::button::disabled` 会留着建按钮时的透明度。

### 测试

- 投影的单测：取值、相等性，以及哪些消息改投影、哪些不改（`view_part_sidebar_tests.rs`）。
- 用 `view_harness::ShellHarness`：
  - `assert_same_as_fresh_mount()`：增量更新后的文档和同一 ViewModel 新挂的文档按无障碍树逐个比较（角色、名称、值、布局盒），每一步更新后都调。
  - 「N 次无关更新后节点 id 不变」：用 `harness.keyed("键")` 记下节点，`apply` 几条无关消息并 `flush` 后再取，应该是同一个节点；相关更新后绑定的字段原地变，结构变了的那一行才换（`route_startup_tests.rs`）。
  - `view_stats().remounts` 不变：动效帧、播放推进和常驻区域的更新都不该重挂。
  - `route_branch()`、`sidebar_root()`、`content_roots()` 判断哪一块换了（`view_part_primary_tests.rs`）。
  - 常驻路由里的输入框：组合输入中后台消息和相关消息都不换节点、不断预编辑；按键消息排在后台消息后面时刚打的字不回滚；新列表项到达时已有行的节点不换（`route_search_tests.rs`、`route_admin_tests.rs`）。
- 藏起（`.visible(false)`）的块排过一次版才退出无障碍树，取节点前先 `flush`（`ShellHarness::mount` 已经排过）。
- 改完跑 `scene_shots` 出全部场景，和改动前逐字节比较。

## 浮层：常驻和统一的对话框

浮层块（`view_part_overlay.rs`）按 `OverlayIdentity` 换块：身份是 `OverlayKey`（哪一种浮层）加上它结构上的变化（文件对话框的种类、仓库弹层的页、右键菜单的目标和落点）。身份不变时浮层常驻，同一时刻最多一块，没有浮层时槽位为空。

**会话。** 浮层视图的签名不变（`fn(&ShellViewModel) -> Option<AnyView>`），建的时候把同步要用的东西交给 `session::register`，浮层块挂这一块时用 `session::collect` 收下：

- `Projected::register(signal, project)`：一个投影放在一个信号里，每次整体同步按 ViewModel 重算，变了才写；浮层已关、取不到投影时不写。
- `Draft::register(model, read)`：输入框的草稿信号，按 `ModelField` 的规矩回写，给 `.model(..)` 用。
- 对话框框架自己登记激活和交还焦点的会话（见下）。

信号建在这一块的挂载作用域里，随浮层一起回收。列表行按内容做键（变了的行重建）或按编号做键、字段从投影里取（任务进度只改字段）。

**对话框框架**（`overlay_dialog.rs`）。所有对话框都用 `DialogFrame`：

```rust
DialogFrame::new("folder-dialog", move || view.with(|view| view.title.to_string()), || close_message())
    .danger()                        // 标题用错误色
    .width(DialogWidth::Normal)      // Narrow 460 / Normal 520 / Export 560 / Wide 720，取最近的 DialogSize
    .busy(move || view.with(|view| view.busy))
    .close_button()                  // 标题右侧的关闭位（导出）
    .dialog(body, footer(None, vec![action(..), action(..)]))
// 确认框：.confirm(message, intent_button(..), intent_button(..), || confirm_message())
```

- 对话框是 NanaUI `Dialog` / `ConfirmDialog`，挂在自己的 `OverlayHost` 下，`activate_overlay` 打开；焦点陷阱、焦点归还、无障碍角色、初始焦点和开合动效都归框架。
- 关闭策略是 `DialogClosePolicy::requests_only()`：Escape、点外面、关闭位只在对话框上发一次 `DialogCloseRequested`，框架换成 `new` 给的关闭消息；处理中（`busy`）什么也不发。确认框的取消和确认由框架变成 `ConfirmIntent`，按钮自己不发消息。
- 激活时机：浮层块把这一块挂成脱离树的内容，`ShellView` 之后才放进 AppShell 的槽位，挂载时的 `on_mount` 运行时宿主还不在树里。所以对话框旁边放 `when(placed, ..)`：浮层块挂好后调会话的 `placed()` 置真，下一次刷新建出分支，分支的 `on_mount` 激活。外层是普通 `Stack`，放进槽位时 AppShell 按「有子节点」挡住点击，激活前也不会漏点。
- 换下前浮层块调会话的 `retire()`：`dismiss_overlay(host)` 让框架关掉对话框、把焦点还给打开前的位置，然后再卸掉这一块。
- 外层里宿主下面还有一层补色（`veil`）：框架的遮罩在线性空间混合、也没有背景模糊，补色层再压暗一层、做背景模糊，合起来和 Vue `.modal-overlay` 一样。它随 `placed` 从透明淡入（`El::animate` 的隐式过渡，时长和曲线同框架的表面淡入），不挡点击。NanaUI 补上遮罩令牌和模糊语义后删掉（见 `docs/nana-vue-parity.md`「NanaUI 缺口」）。
- 字段、按钮、底栏、错误行用框架里的 `text_field` / `text_area_field` / `select_field` / `two_columns` / `action` / `footer` / `error_line` / `busy_note`，照 Vue `.dialog-field` 和 `.dialog-card__actions`。

**先后和 Escape。** `OverlayKey` 的顺序就是同时打开时谁显示：关闭确认最先；其后是原来浮层槽位里的对话框、弹层和菜单；文件页的导出和文件对话框原来画在主区、压在浮层槽位下面，排在最后。Escape 先关显示着的那一层：显示着的对话框已经激活，由运行时拿到 Escape、发自己的关闭请求（`prevent_default` 为真，全局 Escape 不再发）；显示着的是弹层或菜单时走全局 Escape，`escape_layer` 里弹层和菜单都排在文件页的导出和文件对话框前面，等着的对话框不会先被关掉。`escape_layer` 里文件夹菜单排在仓库弹层前面、和 `OverlayKey` 相反，但两者都由点击打开，开着一个时浮层槽位挡住另一个的入口，不会同时开着。

**组合输入。** 浮层换块会换掉输入框，新开的对话框一激活还会把焦点从别处的输入框拿走，所以文档里任何获得焦点的输入框还有预编辑时，浮层块都把换块延后到组合结束（`composing`）。打开期间的变化只写信号，不受影响。

**离屏截图。** 离屏会话没有帧时钟，激活的对话框和补色层停在开场动效的第一帧（透明度 0）。`scene_shots` 截图前在有激活的浮层时把动效走完；没有激活的浮层时什么也不做，别的场景的像素不受影响。

## 不再扫描文档补登

树常驻以后，后来才建出来的节点不会再被「挂载后扫一遍」补登，下面三件事都改成了声明式或全局：

- **Escape**：激活的对话框由运行时直接拿到 Escape、发自己的关闭请求（见上一节）。其余的层：`MomoBakoApplication::input_event` 收到运行时没处理掉的 Escape 按下（`prevent_default` 为假）时，按 `escape_layer(model)` 判断还有没有能关的层，有就发 `GapMessage::Escape`，归约时 `dismiss_top` 照同一个顺序关掉最上面一层。焦点在哪都一样。下拉框收起选项这类控件自己处理的 Escape 不会再关别的层。新的可关闭层加进 `EscapeLayer`。
- **系统文件拖放**：在建拖放目标的视图里给它一个 `node_ref`，调用 `input::accept_file_drops(node_ref)`，挂上后由 `on_mount` 登记。
- **字段的无障碍名**：标题和控件分开写的字段，标题加 `.node_ref(caption)`，下拉框这类没有自己名称的控件加 `.labelled_by(caption)`（对话框框架 `overlay_dialog_fields.rs` 的 `select_field`）。没有可见标题时写控件自己的 `label`。

## NanaUI 缺口

这次搭底座时遇到、在 MomoBako 里绕开的：

- `dynamic` 没有同步重建单个结构块的公开入口（`run_structural_now` 是 crate 内部的），分支只能在下一次刷新时换，而 `update` 里又不能刷新。换到旧视图路由时因此「同步挂好、刷新时交接」；同一路由里的更新不经过 `dynamic`，直接换路由容器里的内容。
- 结构块的容器只收 `class` / `class_when` / `css` / `visible`，不能给一个现成的 `Stack` 或 `node_ref`。路由容器的排版只好开 `view-macro` 写 `css!`，找容器靠键。
- `mount_view_detached` 挂出的根不在装配键表里，根节点的键路径是父节点的路径，自己的键不出现。`remount_state` 对根节点按父路径加类型找回，能用，但根的键（例如 `settings-scroll`）查不到。
- `activate_overlay` 要求宿主已经在树里：在脱离树的挂载里激活，框架算不出初始焦点（候选必须已挂上），开场动效也不播。浮层块因此借 `when(placed, ..)` 在放进槽位后的那次刷新里激活；视图层没有「挂进树时」的钩子，也没有声明式的 `open`。
- 元素上没有声明式的拖放目标，要靠 `on_mount` 调 `set_drop_target_node`。
- AppShell 只在自己投影时按「有没有子节点」决定 overlay 槽位挡不挡点击；槽位里的内容由结构块在刷新时换，它不会跟着重新判断（槽位根是 `OverlayHost` 时由宿主按激活的浮层决定，但弹层不是框架的浮层种类，放不进宿主）。所以浮层块按身份换块，没有浮层时把槽位清空，不用常驻的浮层根。
- `TreeView` 是一个自绘节点：`nodes` 只能整份绑定，展开、计数变了都整树重投影；行不是节点，放不了行内按钮、右侧计数、右键菜单和行级无障碍。侧栏的文件夹树和智能文件夹树改成按显示顺序展开的扁平行，放进按键对照的 Store，用 `keyed(..).each` 建：展开只插子级，三角、当前目录和计数原地改。
- `ReorderList` 拖动时按自己的直接子节点取行的盒子（`reorder_row_boxes`），`each` 总会多一层容器，也没有把行直接放进现成控件的入口（只有 `each_virtual` 有 `.within`），所以条目放不进带键的 `each`。播放集页改用按条目顺序做键的 `dynamic`：增删、重排时整个列表重建，当前播放和条目字段原地绑定。
- `#[derive(Store)]` 给每个字段生成同名访问器，但 `Item` 自己有 `id()`（键的哈希），名叫 `id` 的字段在行上读不到，行结构里的编号字段要换个名字（`playlist_id`、`item_id`）。
- 结构块先删旧分支再建新分支，删的时候焦点被清空；`on_cleanup` 拿不到 `AppContext`，没有在换分支之前能读焦点的回调。播放集页要在换列表之前记焦点，只好分两步：`watch_effect` 看到条目顺序变了先登记 `on_mount`，回调里（旧列表还在）记下焦点和滚动，再把顺序写进 `dynamic` 的键；下一轮换出新列表，新列表的 `on_mount` 按键路径找回。两步在同一次刷新里完成。

搜索、设置和管理页改常驻时遇到的：

- 控件表里没有的可绑定字段要自己写 `FieldWrite`：`ScrollView.follow_end`（`route_search::FollowEnd`）、`TextInput.read_only`、`Button.icon`、整份 `NodeStyle`（`admin::bind`），以及按地址重新编码的 `QrCode`（`source_auth_page::QrPayload`）。
- `css!` 的 `min-width: 0` 除了 `min_width` 还写 `allow_shrink`，和构建器的 `min_width(Px(0))` 字段不完全相同；现在布局不读它，画面一样。
- `StorePath` 没有 `with_untracked`，同步里比较 Store 的现值用 `untrack(|| store.with(..))`。
- `layout.hidden` 的子树要等排版以后才退出无障碍投影，没刷新的文档投影出来还带着藏起的节点。
