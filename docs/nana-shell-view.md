# 壳层绑定式视图：分块和改常驻的约定

壳层正从「每条消息整块重挂」迁到 NanaUI 的绑定式视图：树只建一次，状态放进信号或 Store，绑定只改自己写的那个字段。这篇写分块的结构，以及把一块区域改成常驻时要做的事。总体数据流见 [迁移边界](./nana-migration.md) 的「壳层视图同步」。

## 分块和各自的文件

`ShellView`（`src-nana/src/shell/view_host.rs`）只做调度：写热信号和标题栏，按同一套流程调三块内容，再把各块的根放进 AppShell 和工作区的槽位。三块都实现 `ShellPart`（`view_part.rs`）：

| 方法 | 做什么 |
| --- | --- |
| `signals(model)` / `new(signals)` | 常驻的信号和 Store，在骨架的挂载闭包里建，跟骨架一起回收，整块重挂时不重建 |
| `mount(cx, model, mode)` | 首次挂载；工作台排法变了时把根挪到新槽位，侧栏和主区外框都不重建，不等输入法组合 |
| `sync(model)` | 只写本块的信号。每次整体同步都调，组合输入中也调 |
| `needs_remount(model)` | 有没有必须重挂才能跟上的变化 |
| `remount(cx, model)` | 重挂需要重挂的部分 |
| `composing(document)` | 焦点在本块里、输入法还有预编辑：这时 `ShellView` 把重挂记成延后，组合结束后的下一帧补挂 |

重挂换下来的旧内容和要找回的焦点、选区、滚动放进 `Swap`，等新根放进槽位以后由 `ShellView` 收尾。

| 块 | 模块 | 切换 | 现状 |
| --- | --- | --- | --- |
| 侧栏 | `view_part_sidebar.rs`，投影在 `sidebar_project.rs` | 无 | 常驻：`SidebarSignals`（几个信号加播放集、文件夹树、智能文件夹树三份 Store）建在骨架作用域里，同步只写变了的；第一次排成工作台时建，收起时跟着工作区停放，展开时原样回来 |
| 主区 | `view_part_primary.rs`，路由在 `route_*.rs` | `dynamic(RouteKey)` | 所有路由（启动、文件、缺失仓库、空库、搜索含空库搜索、设置、日志、拓展、动作和播放集页）都常驻；文件页和播放集页里的播放条是旧视图岛 |
| 浮层 | `view_part_overlay.rs` | 按 `OverlayIdentity` 换块 | 常驻：浮层层一直在 AppShell 的 overlay 槽位里，各块按身份换进换出；身份不变时只经会话写信号；对话框走统一框架，靠 `open` 开合，换下的放完退场才卸 |

并行改区域时各改各的文件：

- 浮层和对话框：`view_part_overlay.rs`（`overlay_branch` 里自己那种浮层的分支，身份的结构见 `OverlayIdentity`），对话框框架 `overlay_dialog.rs`、`overlay_dialog_fields.rs`，会话 `overlay_session.rs`，以及 `sidebar_dialogs.rs`、`sidebar_smart_dialog.rs`、`sidebar_popover_view.rs`、`sidebar_tree_view.rs` 的菜单、`admin_view.rs` 的任务弹层、`source_prompt.rs`、`files_menu.rs`、`workspace_dialogs.rs`、`workspace_export_dialog.rs` 等浮层视图。
- 文件页和详情：`route_files.rs` 和 `files_*.rs`、`inspect_*.rs`。
- 侧栏和其余路由：`view_part_sidebar.rs`、`sidebar_*.rs`，以及 `route_search.rs`、`route_playlists.rs`、`route_settings.rs`、`route_admin.rs`、`route_startup.rs`、`route_missing.rs`、`route_empty.rs`；首页外框和滚动主体在 `route_home.rs`。

`view_host.rs` 和 `view_part.rs` 不必动。`view_part_primary.rs` 里只改自己路由那一行：`route_view`、`islands` 和 `RouteSignals`。

## 主区的路由键和分支

`RouteKey` 是身份键：启动状态、设置页、区域（有仓库、丢失、空库）和工作区面板合起来决定主区显示哪种页面，它本身就是 `dynamic` 的键。所有路由都常驻：

- 分支只在进入路由时建一次，之后 `sync` 只写 `RouteSignals` 里它的信号。分支在刷新时由 `dynamic` 建，拿不到 `&ShellViewModel`，只能读信号；热信号要显式传进去（`hot::prop` 只在旧视图挂载期间有值）。
- 换路由时同步只把新路由写进键（新路由的岛先挂好等着），下一次刷新 `dynamic` 换出新分支，分支的 `on_mount` 把岛放进占位节点。主区外框、路由容器、侧栏和浮层都不动。换走又在刷新前换回来时把键写回去，分支不换。

路由容器（键 `primary-route`）夹在外框和分支之间，`css!` 写成和 `Stack::fill_column(0)` 一样的排版，分支的样子和直接放在外框里一样。

工作台排法变了（收起或展开侧栏）时只挪位置，外框、分支、岛和滚动都留着。外框只挂一次：有侧栏时它自己放进工作区的主区；主区独占时 `ShellView::place` 先让工作区放下它，`PrimaryPart::hold_solo` 再把它放进一层壳层底色（第一次独占时挂，之后留着），底色当 AppShell 的 body。

- 工作区放下外框时会把它停放，外框上还带着区域打的样式和无障碍补丁（区域底色、圆角、`primary` 的无障碍名），放进底色后让外框重新投影一次换回自己的；放回工作区时区域照常重打。
- 停放会清掉焦点。挪位置前 `mount` 在外框下记下焦点、选区和滚动，放好后由 `Swap` 收尾时找回；节点没换，找回的就是原来那个节点。输入法组合会被打断，和以前一样不等。
- 侧栏收起时留在工作区的资源区里，跟着工作区一起停放，照常写信号；展开时工作区回到 body，侧栏原样回来。

### 首页筛选栏

筛选栏属于首页外框，有仓库的首页路由都可能显示它。信号 `FilterBarSignals` 在 `RouteSignals::filter`，除启动页和设置页以外每次同步都写（关着时不算候选和库类型快捷方式）。常驻首页路由（文件、搜索、播放集、日志、拓展和动作页）在外框里嵌 `inspect_search_view::resident_filter_bar(signals.filter)`，显隐跟「有仓库且筛选栏打开」走 `.visible`；文件页的 `FilesRouteSignals` 不另存筛选栏的状态，读的是同一份 `RouteSignals::filter`。首页外框和纵向滚动主体只有一套：`route_home::{home_page, home_scroll}`，缺失仓库和空库页也用它（不嵌筛选栏）；设置和启动页的整页滚动是 `route_home::scroll_route`。

## 改成常驻的步骤

以 `route_startup.rs` 为样板。

1. **投影**：写一个 `XxxView::project(model) -> Self`，`#[derive(Clone, Debug, PartialEq)]`，只放视图要读的值，按视图要显示的样子算好（文案、拼好的字符串、列表）。给 `project` 写单测：取值对，同一状态两次投影相等。
2. **信号**：投影按「谁一起变、谁读它」拆成几个信号，`XxxSignals::new(model)` 建在本块的 `signals()` 里（主区路由放进 `RouteSignals`），`write(view)` 对每个信号用 `try_set_if_changed`，值没变不触发绑定。
3. **视图只建一次**：文字和样式字段用 `.prop::<T, W>(信号或闭包)` 绑定；可有可无的一块用 `.visible(..)`，节点留着、不占布局，不改兄弟之间的间距（`when` 会多一个容器，空着也占一个间距）；结构真的要换时才用 `when` / `dynamic`。初值可以 `get_untracked()` 取，绑定在挂载时会再写一次同样的值。
4. **去掉整块重挂**：本块的 `needs_remount` 不看 ViewModel 版本，只在结构变了时返回真；主区路由在 `route_view` 里登记分支。
5. **测试**：见下文。

### Signal 还是 Store

- 几个平铺的值、读者不多：`Signal<T>`，`T` 是投影里的一段结构，绑定用 `move || sig.with(|v| v.field.clone())` 取字段。值变了以后读者重跑，按字段比较，没变的字段不打补丁，成本很低。
- 嵌套的状态，或者列表里每行的字段各自在变、行数又多：`store(..)` 加 `#[derive(Store)]`（壳层已开 `view-macro`），行用 `keyed(|item| item.id).each(..)`，行内绑定读 `item.title()` 这样的路径，改一行只重跑读这一行的绑定。整体 `set` 列表会让所有行的绑定重跑一遍再按字段比较，大列表写回时只改变了的项（`at(&key).set(..)`、`push`、`retain`）。
- 投影出的整份新列表写回 Store 时按键做差异：删掉没有了的、按位置插入新的、只改内容变了的，顺序变了再排一次；键重复时两边都只认第一行。所有列表共用 `row_sync::sync_rows`（侧栏三份树、播放集条目、搜索结果、日志和插件面板），删、插、排只通知列表本身，留下的行一个绑定都不重跑。新日志到达只建新的一行。行内绑定用 `row_text` / `row_flag`，读 `try_with`，行已经删掉时读默认值。
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

不要在处理器里捕获建视图那一刻从 ViewModel 抄来的、之后会变的值。旧视图里的拖放标志（`file_drop_flags`）就是这样抄下来的，它能用是因为内容每次都整块重挂；改常驻时要换成意图消息或者读信号。文件列的拖放现在发 `FilesMessage::HostDrop`，归约时按当时的仓库条件收成宿主拖放消息。设置页的复制按钮、日志的暂停、插件卡片的启用和动作页的执行都是点下去时现读信号。

`style::action` 按钮禁用时整体 45% 不透明。绑定禁用用 `admin::bind::ActionDisabled`，禁用标志和透明度一起写；只绑 `fields::button::disabled` 会留着建按钮时的透明度。

### 常驻路由里的旧视图岛

常驻路由里还嵌着别的模块的旧视图时（文件页和播放集页里的播放条），分支里给它留一个占位节点（`NodeRef`），在 `view_part_primary.rs` 的 `islands` 里按路由登记成 `Island { slot, build, stamp }`：

- 进路由时主区块按当前 ViewModel 把岛的内容挂成脱离树的一块，分支挂好（`on_mount`）时放进占位节点；
- 之后 `stamp(model)` 变了才当场重建这一块，记下并找回岛里的焦点、选区和滚动，常驻部分不动；焦点在主区、输入法还在组合时照常延后；
- `stamp` 按这块内容真正读到的值算版本。用 `model.revision` 会每次归约都重建；内容会自己发消息的岛（播放条量到宽度就发 `BarResized`）这样做还会量了又建、建了又量，变成每帧重建。播放条的版本是 `player_view::bar_stamp`，三处播放条岛共用；
- 占位节点要排成和原来直接放在那里一样（`Stack::column(0)` 高度随内容），不能放进会自己重建的结构块里：结构块换分支时占位节点跟着换，岛要到下一次同步才放得回去。要显隐时用 `.visible`；
- 岛是过渡办法：那块改成常驻（或者对话框挪进浮层块）以后删掉登记。

### 测试

- 投影的单测：取值、相等性，以及哪些消息改投影、哪些不改（`view_part_sidebar_tests.rs`）。
- 用 `view_harness::ShellHarness`：
  - `assert_same_as_fresh_mount()`：增量更新后的文档和同一 ViewModel 新挂的文档逐个比较，每一步更新后都调。先比无障碍树（角色、名称、值、布局盒），再按文档顺序比每个节点（含不进无障碍树的容器）的组装路径、组件类型、无障碍状态（禁用、选中、勾选、忙、无效）和整份样式，只绑在样式和状态上的字段也比得到。路径里 `#v0`、`#adopt-1` 这类自动起名的段不比：先挂后插的行和一次建出的行名字不同，结构一样。滚动偏移、焦点和悬停不比。
  - 「N 次无关更新后节点 id 不变」：用 `harness.keyed("键")` 记下节点，`apply` 几条无关消息并 `flush` 后再取，应该是同一个节点；相关更新后绑定的字段原地变，结构变了的那一行才换（`route_startup_tests.rs`）。
  - `view_stats().remounts` 不变：动效帧、播放推进和常驻区域的更新都不该重挂。
  - `route_branch()`、`sidebar_root()`、`content_roots()` 判断哪一块换了（`view_part_primary_tests.rs`）。
  - 常驻路由里的输入框：组合输入中后台消息和相关消息都不换节点、不断预编辑；按键消息排在后台消息后面时刚打的字不回滚；新列表项到达时已有行的节点不换（`route_search_tests.rs`、`route_admin_tests.rs`）。
- 藏起（`.visible(false)`）的块排过一次版才退出无障碍树，取节点前先 `flush`（`ShellHarness::mount` 已经排过）。
- 改完跑 `scene_shots` 出全部场景，和改动前逐字节比较。

## 全局状态区

侧栏顶部的状态条对应 Vue `WorkspaceSidebarStatus.vue`。状态在 `status.rs`：`ShellViewModel::status` 只有一个槽位，记最近一次失败（来源 `FailureSource` 加文案），后记的顶替先记的。投影 `StatusLine` 放进侧栏投影（`SidebarView::status`），顺序照 Vue：失败 > 忙碌 > 同步进度。视图里错误条和忙碌行两块都留着，按投影 `.visible` 互换，侧栏不重挂。

**记什么。** 只记没有就近显示的失败：

| 来源 | 失败 | 在哪里记 |
| --- | --- | --- |
| `Host` | 打开、定位、拖出文件（原 `input.error`，已删） | `host_bridge::perform`、`drag_out::apply_result` |
| `Tray` | 最小化到托盘（原来借 `input.notice`，关闭确认框以外没有显示位）；收进托盘成功时作废 | `host_bridge::perform` |
| `Repository` | 启动以后读资源库列表、读仓库摘要失败；添加资源库时的文件夹选择失败（弹层已关） | `shell.rs` 归约、`input_reduce.rs` |
| `Directory` | 文件区以外发起的目录读取失败（文件区自己的写在文件列表里） | `shell.rs` 归约 |
| `Asset` | 读素材详情失败 | `shell.rs` 归约 |
| `Metadata` | 保存元数据、撤销、重做失败 | `inspect::reduce_message` |
| `Playlist` | 播放集的读写、条目增删排序、播放器类型 | `shell.rs` 归约 |
| `Player` | 播放控制、播放集成员写入 | `PlayerState::note_failure`，归约结束时取走 |
| `Settings`、`Logs` | 读写应用设置、系统服务状态；读系统日志 | `admin_reduce.rs` |
| `SmartFolder` | 智能文件夹删除失败（编辑对话框没开着）；查询、读列表失败 | `sidebar.rs` 归约；`sidebar.smart_error` |
| `FolderTree` | 文件夹树读取失败 | `sidebar.tree_error` |
| `Files` | 文件操作失败 | `files.error` |

文案先写失败的对象（「定位失败：」「无法读取文件元数据：」），后接系统返回的原因。`files.error`、`sidebar.tree_error`、`sidebar.smart_error` 三个字段被盯着（`observe_failures`）：新写一次记一次，字段清掉时它记下的那一条一起清。宿主在归约以外写下的失败（服务派发、拖出结果）在 `update` 同步视图之前、`prepare` 里收进来。

对话框错误行、搜索面板、缺失仓库页、空库页、启动页、插件面板、导出对话框、预览和播放条这些已经就近显示的失败不进状态区。文件操作失败写在文件列表的状态框里，列表显示着（文件路由、不在预览页）时状态区让位，换到别的面板时状态区显示它。

**什么时候清。** 照 Vue `error.value = null` 的时机：用户开始一个会写全局错误的操作时清掉上一次失败，成功不专门清（托盘例外）。开始的迹象：

- 消息本身（`status::starts_operation`）：打开、定位、拖出（过了 Vue 的守卫：有仓库、有路径、能拖出），换仓库、刷新资源库列表、重试启动，刷新文件夹树、打开智能文件夹；
- 忙碌标志由假变真、换了文件（`status::Activity`）：读目录（非静默）、文件变更、保存元数据（也包括撤销重做，Vue 这两样不清）、搜索、导出、智能文件夹增改删、插件操作、执行仓库动作，以及选中别的文件（Vue `selectAsset`）。
- 后台的静默刷新不清。Vue 结构更新后的静默刷新会经 `refreshRepositoryActions` 顺带清掉错误，用户还没看到的失败就被抹掉了，这里不照抄。

清只作用在归约开始时已经记下的失败上：同一次归约里新记的失败按序号留着。

**忙碌。** Vue `isBusy`：启动以后在读资源库列表（`workspace.list_loading`）、读仓库摘要（`workspace.snapshot_loading`）或读素材详情（`inspect.detail_loading`）。显示「正在同步仓库状态」，前面的转圈读热信号。启动中由启动页显示进度。

**同步进度。** Vue 的第三档是仓库同步的进度（文件夹树的「刷新」会同步整个仓库）。Nana 启动以后没有仓库同步，文件夹树的刷新只重读树，所以没有这一档。

## 浮层：常驻和统一的对话框

浮层块（`view_part_overlay.rs`）按 `OverlayIdentity` 换块：身份是 `OverlayKey`（哪一种浮层）加上它结构上的变化（文件对话框的种类、仓库弹层的页、右键菜单的目标和落点）。身份不变时浮层常驻，同一时刻最多显示一块。

**浮层层。** 骨架挂载时把一个空容器（`view_part_overlay::layer`，键 `shell-overlay`）直接交给 AppShell 的 `.overlay(..)`，之后一直在。浮层块把各块挂成脱离树的内容，再用 `reconcile_children` 放进浮层层：还在放退场的块在前，现在这块在最后；换下就卸的块被省掉、停放，随这次换块卸掉。AppShell 盯着浮层层的子节点：有没隐藏的子节点时挡住下面的点击，空了就放开，增删子节点不重新装配，`place_shell` 只管 body。

- 浮层层上不放绑定（AppShell 的布局补丁只在它投影时写）。各块的根不是槽位根，可以带绑定：任务弹层的淡入和上移直接绑在弹层根上。
- 各块自己铺满浮层层：对话框的外层绝对定位铺满，弹层是撑满的一列，右键菜单是铺满的叠层。
- 块里的定位相对浮层层（它铺满窗口），用绝对定位，不用固定定位：固定定位的节点按自己的层级参与整窗排序，不写层级就排在浮层层（层级 1）下面，点击被浮层层吞掉；绝对定位的节点留在浮层层里，按先后叠放，不用写层级。右键菜单的透明底和菜单因此不再写层级。

**会话。** 浮层视图的签名不变（`fn(&ShellViewModel) -> Option<AnyView>`），建的时候把同步要用的东西交给 `session::register`，浮层块挂这一块时用 `session::collect` 收下：

- `Projected::register(signal, project)`：一个投影放在一个信号里，每次整体同步按 ViewModel 重算，变了才写；浮层已关、取不到投影时不写。
- `Draft::register(model, read)`：输入框的草稿信号，按 `ModelField` 的规矩回写，给 `.model(..)` 用。
- 对话框框架自己登记开合会话 `Presence`（见下）。

会话还有三个钩子：`show()` 声明打开、`hide()` 声明关上（返回要不要等退场）、`settled()` 说退场放完没有。默认实现是没有退场的浮层：换下就卸。

信号建在这一块的挂载作用域里，随浮层一起回收。列表行按内容做键（变了的行重建）或按编号做键、字段从投影里取（任务进度只改字段）。

**对话框框架**（`overlay_dialog.rs`）。所有对话框都用 `DialogFrame`：

```rust
DialogFrame::new("folder-dialog", move || view.with(|view| view.title.to_string()), || close_message())
    .size(MODAL_CARD)                // 照 Vue 的宽度类：MODAL_CARD 是 min(520px, 92vw)，导出 DialogSize::capped(560.0, 92.0)
    .danger(true)                    // 危险口气：标题和标题图标用主题的危险色
    .title_icon(COPY)                // Vue 标题前有图标的才写
    .busy(move || view.with(|view| view.busy))
    .close_button()                  // 标题右侧的关闭位（导出）
    .dialog(body, footer(None, vec![action(..), action(..)]))
// 确认框：.confirm(message, intent_button(..), intent_button(..), || confirm_message())
```

- 对话框是 NanaUI `Dialog` / `ConfirmDialog`，常驻在自己的 `OverlayHost` 下，靠 `open` 开合；焦点陷阱、焦点归还、无障碍角色、初始焦点和开合动效都归框架。距顶、最高、圆角、三段内边距和分隔线、遮罩和进退场归主题配方（`appearance.rs`），对话框上只写宽度、口气和标题图标。
- 关闭策略是 `DialogClosePolicy::requests_only()`：Escape、点外面、关闭位只在对话框上发一次 `DialogCloseRequested`，框架换成 `new` 给的关闭消息；处理中（`busy`）什么也不发。确认框的取消和确认由框架变成 `ConfirmIntent`，按钮自己不发消息。开合仍然只由归约决定。
- 开合信号：对话框 `.model(open)`，`open` 由会话 `Presence` 写。新块挂出来时是假；放进浮层层以后浮层块调 `show()` 写真，下一次刷新时对话框、宿主和插槽都在树上，框架打开它。建的时候不写真：那样插进树的那一刻就打开，排在换下的对话框关上之前，新对话框记下的「打开前的焦点」会落在旧对话框里。
- 换块：旧块的会话 `hide()` 写假，框架连退场一起关上它、把焦点还给打开前的位置；这块留在浮层层里放退场。新块在同一次同步里放进来、`show()` 写真。旧的先写、新的后写，刷新时先关旧的再开新的，焦点一路按打开的先后还回去（关闭确认压在新建文件夹上、取消以后新建文件夹回来，再关掉时焦点回到最初的搜索框）。
- 退场放完：宿主清掉激活时（`settled()`），`OverlayPart::sweep` 卸掉这块，AppShell 跟着放开点击。`ShellView` 每次整体同步和每帧准备（`settle_overlays`）都调它；还有块在放退场时准备帧一直要帧（`window.demand`），放完的下一帧就卸。卸掉以前浮层层还挡着点击，和 Vue 离场过渡时一样。不要在关的时候连节点拿掉，那样没有退场。
- 宿主自己关掉对话框（被同一宿主上别的浮层顶替、所在的块被停放）时发 `DialogToggled { open: false }`，`.model` 把开合信号写回假。不同步回 ViewModel：这不是用户的意思，开合只听归约。这块还是现在显示的块（ViewModel 还要它开着）时，下一帧 `sweep` 调 `show()` 再写真，放回树上以后重新打开，不发消息。只用 `.open(信号)` 不听回写的话，信号一直是真，再写真不算变化，就打不开了。
- 字段、按钮、底栏、错误行用框架里的 `text_field` / `text_area_field` / `select_field` / `two_columns` / `action` / `footer` / `error_line` / `busy_note`，照 Vue `.dialog-field` 和 `.dialog-card__actions`。

**先后和 Escape。** `OverlayKey` 的顺序就是同时打开时谁显示：关闭确认最先；其后是原来浮层槽位里的对话框、弹层和菜单；文件页的导出和文件对话框原来画在主区、压在浮层槽位下面，排在最后。Escape 先关显示着的那一层：显示着的对话框已经打开，由运行时拿到 Escape、发自己的关闭请求（`prevent_default` 为真，全局 Escape 不再发）；在放退场的对话框已经关上，运行时不再把 Escape 交给它；显示着的是弹层或菜单时走全局 Escape，`escape_layer` 里弹层和菜单都排在文件页的导出和文件对话框前面，等着的对话框不会先被关掉。`escape_layer` 里文件夹菜单排在仓库弹层前面、和 `OverlayKey` 相反，但两者都由点击打开，开着一个时浮层槽位挡住另一个的入口，不会同时开着。

**组合输入。** 浮层换块会换掉输入框，新开的对话框一激活还会把焦点从别处的输入框拿走，所以文档里任何获得焦点的输入框还有预编辑时，浮层块都把换块延后到组合结束（`composing`）。打开期间的变化只写信号，不受影响。

**离屏截图和测试。** 离屏会话没有帧时钟，打开的对话框停在开场动效的第一帧（透明度 0），换下的对话框停在退场开头、一直留在浮层层里。`scene_shots` 截图前在有激活的浮层时把动效走完；没有激活的浮层时什么也不做，别的场景的像素不受影响。测试支撑的 `finish_motion` 先刷新、再把动画推到放完、卸掉放完退场的块；`assert_same_as_fresh_mount` 比较之前两边都先走完动效。

## 不再扫描文档补登

树常驻以后，后来才建出来的节点不会再被「挂载后扫一遍」补登，下面三件事都改成了声明式或全局：

- **Escape**：激活的对话框由运行时直接拿到 Escape、发自己的关闭请求（见上一节）。其余的层：`MomoBakoApplication::input_event` 收到运行时没处理掉的 Escape 按下（`prevent_default` 为假）时，按 `escape_layer(model)` 判断还有没有能关的层，有就发 `GapMessage::Escape`，归约时 `dismiss_top` 照同一个顺序关掉最上面一层。焦点在哪都一样。下拉框收起选项这类控件自己处理的 Escape 不会再关别的层。新的可关闭层加进 `EscapeLayer`。
- **系统文件拖放**：在建拖放目标的视图里给它一个 `node_ref`，调用 `input::accept_file_drops(node_ref)`，挂上后由 `on_mount` 登记。
- **字段的无障碍名**：标题和控件分开写的字段，标题加 `.node_ref(caption)`，下拉框这类没有自己名称的控件加 `.labelled_by(caption)`（对话框框架 `overlay_dialog_fields.rs` 的 `select_field`）。没有可见标题时写控件自己的 `label`。

## NanaUI 缺口

这次搭底座时遇到、在 MomoBako 里绕开的：

- `dynamic` 没有同步重建单个结构块的公开入口（`run_structural_now` 是 crate 内部的），分支只能在下一次刷新时换，而 `update` 里又不能刷新。换路由时因此只写键，岛先挂好、分支挂好时再放进占位节点。
- 结构块的容器只收 `class` / `class_when` / `css` / `visible`，不能给一个现成的 `Stack` 或 `node_ref`。路由容器的排版只好开 `view-macro` 写 `css!`，找容器靠键。
- `mount_view_detached` 挂出的根不在装配键表里，根节点的键路径是父节点的路径，自己的键不出现。`remount_state` 对根节点按父路径加类型找回，能用，但根的键（例如 `settings-scroll`）查不到。
- （已补）`activate_overlay` 要求宿主已经在树里，视图层原来没有声明式的 `open`，浮层块只好借 `when(placed, ..)` 在放进槽位后的那次刷新里激活。现在 `Dialog` / `ConfirmDialog` 有 `.open` / `.model`，插进树、插槽装好以后自己打开，写假时连退场一起关上。
- 元素上没有声明式的拖放目标，要靠 `on_mount` 调 `set_drop_target_node`。
- （已补）AppShell 原来只在自己投影时按「有没有子节点」决定 overlay 槽位挡不挡点击，浮层块只好每次换块都换掉 overlay 槽位、重新装配。现在 AppShell 盯着槽位的子节点（增删、显隐都重新判断，隐藏的不算），浮层层改成常驻的槽位内容。
- `TreeView` 是一个自绘节点：`nodes` 只能整份绑定，展开、计数变了都整树重投影；行不是节点，放不了行内按钮、右侧计数、右键菜单和行级无障碍。侧栏的文件夹树和智能文件夹树改成按显示顺序展开的扁平行，放进按键对照的 Store，用 `keyed(..).each` 建：展开只插子级，三角、当前目录和计数原地改。
- （已补）`ReorderList` 拖动时按自己的直接子节点取行的盒子（`reorder_row_boxes`），`each` 原来总会多一层容器，播放集页只好用按条目顺序做键的 `dynamic`，增删、重排时整个列表重建。现在 `each(..).container(元素)` 把行直接建进给定的元素，播放集条目用 Store 的 `keyed(..).each(..)` 建进 `ReorderList`，留下的行不重建。
- `ReorderList` 按下行就开始拖动手势、把指针抓走，行里的按钮收不到点击，除非这一行的条目用 `.tools(节点)` 标出可点的区域，而且一行只能标一块。播放集条目把扩展名方块、标题、播放和移除包成一段登记成 `tools`（行挂上时写进登记表，条目跟着重算），从拖动柄和行的留白拖动。
- `#[derive(Store)]` 给每个字段生成同名访问器，但 `Item` 自己有 `id()`（键的哈希），名叫 `id` 的字段在行上读不到，行结构里的编号字段要换个名字（`playlist_id`、`item_id`）。
- 结构块先删旧分支再建新分支，删的时候焦点被清空；`on_cleanup` 拿不到 `AppContext`，没有在换分支之前能读焦点的回调。原来播放集页换列表前要分两步记焦点；条目改成带键的 `each` 以后列表不再整块换，现在没有用到这条的地方。

搜索、设置和管理页改常驻时遇到的：

- 控件表里没有的可绑定字段要自己写 `FieldWrite`：`ScrollView.follow_end`（`route_search::FollowEnd`）、`TextInput.read_only`、`Button.icon`、整份 `NodeStyle`（`admin::bind`），以及按地址重新编码的 `QrCode`（`source_auth_page::QrPayload`）。
- `css!` 的 `min-width: 0` 除了 `min_width` 还写 `allow_shrink`，和构建器的 `min_width(Px(0))` 字段不完全相同；现在布局不读它，画面一样。
- `StorePath` 没有 `with_untracked`，同步里比较 Store 的现值用 `untrack(|| store.with(..))`。
- `layout.hidden` 的子树要等排版以后才退出无障碍投影，没刷新的文档投影出来还带着藏起的节点。

文件页改常驻时遇到的：

- `each` 的容器只有 `Stack::column(n)` 和 `Stack::row(n)`，换行的药丸行（面包屑、标签片）要再写 `css! { flex-wrap: wrap; row-gap: 8px; }`；一行里「标签片 + 末尾的加号」只好把加号也当成一项。
- `each` / `each_virtual` 的数据源要 `Readable`，闭包不行，派生的列表要包一层 `computed`。
- `El<C, K>` 的子节点类型写在类型参数里，带子节点的元素没法在函数之间传递后再 `.visible(..)`，只好把显隐当参数传进去。
- 无障碍树在布局之前不认 `layout.hidden`（`resolved.visible` 要排过版才算），不排版就查标签的测试会看到藏着的节点。
- `Store` 没有不追踪的 `try_with`，建视图时取初值要 `untrack`。`Icon` 没有 `PartialEq`，比较要用 `as_ptr`。
- 每个新建的节点第一次布局都会发 `SizeChanged`。监听它、再发消息改状态的旧视图要是每次归约都整块重挂，就会量了又建、建了又量（文件列表的 `ListResized`、播放条的 `BarResized`）。
