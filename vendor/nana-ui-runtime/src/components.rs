use std::{
    collections::BTreeSet,
    sync::{Arc, LazyLock},
};

use nana_ui_core::{
    CardKind, ControlSize, FontFeatureSetting, FontKerningSpec, FontVariationSetting, Icon,
    LayoutStyle, LineBreakSpec, LineHeightSpec, PopoverAlignment, PopoverPlacement,
    SemanticColorRole, SwitchControlPosition, UI_BASE_TEXT_SIZE, WordBreakSpec,
};

/// The **default theme's** status-tone mapping.
///
/// Component constructors have no installed theme to ask, so they read the
/// default recipe. Paths that do have one — extraction, geometry — go through
/// `world.theme().recipes().status()` instead, which is what lets a theme move
/// the mapping. Same relationship as [`nana_ui_core::space`] to the installed
/// spacing tokens.
pub(crate) fn status_tone_role(tone: nana_ui_core::StatusTone) -> SemanticColorRole {
    nana_ui_core::StatusRecipe::DEFAULT.role(tone)
}

use crate::{NodeKind, StableNodeId};

/// Which side of a byte offset a caret belongs to — see [`TextSelection`].
///
/// The Runtime contract speaks `nana-text`'s affinity rather than a parallel
/// enum of its own: a probe hands it straight to the engine's caret geometry,
/// and a host without an engine only ever produces the default.
pub use nana_text::Affinity as TextAffinity;

static DEFAULT_LAYOUT_STYLE: LazyLock<Arc<LayoutStyle>> =
    LazyLock::new(|| Arc::new(LayoutStyle::default()));

/// Effective retained-document presence for a node.
///
/// Parked nodes keep their stable identity and application-owned view state,
/// but are excluded from layout, rendering, input, focus and accessibility
/// until the same subtree is inserted again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MountState {
    #[default]
    Mounted,
    Parked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SemanticPaint {
    pub foreground_mix: Option<nana_ui_core::SemanticColorMix>,
    pub background_mix: Option<nana_ui_core::SemanticColorMix>,
    pub border_mix: Option<nana_ui_core::SemanticColorMix>,
    pub foreground: Option<SemanticColorRole>,
    /// The quieter companion of [`Self::foreground`]: a row's detail line, a
    /// field's placeholder, a hint under a label, a slider's unit.
    ///
    /// `None` means `Muted`, which is what every component painted before this
    /// slot existed. It exists because a state that replaces `foreground` has
    /// to be able to replace this too: a focused row fills with
    /// `focus_surface`, its label follows to `focus_text`, and a detail line
    /// pinned to `Muted` stays resolved against the surface the row *used* to
    /// have — 1.09:1 in light mode. Naming the pair is the same argument
    /// [`Self::FOCUS_SURFACE`] already makes for the label.
    pub foreground_secondary: Option<SemanticColorRole>,
    pub background: Option<SemanticColorRole>,
    pub border: Option<SemanticColorRole>,
}

impl SemanticPaint {
    pub fn overlay(self, overlay: Self) -> Self {
        Self {
            foreground_mix: overlay.foreground_mix.or_else(|| {
                overlay
                    .foreground
                    .is_none()
                    .then_some(self.foreground_mix)
                    .flatten()
            }),
            background_mix: overlay.background_mix.or_else(|| {
                overlay
                    .background
                    .is_none()
                    .then_some(self.background_mix)
                    .flatten()
            }),
            border_mix: overlay.border_mix.or_else(|| {
                overlay
                    .border
                    .is_none()
                    .then_some(self.border_mix)
                    .flatten()
            }),
            foreground: overlay.foreground.or(self.foreground),
            foreground_secondary: overlay.foreground_secondary.or(self.foreground_secondary),
            background: overlay.background.or(self.background),
            border: overlay.border.or(self.border),
        }
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }

    /// What a keyboard-focused control paints when it has no border width to
    /// colour.
    ///
    /// Background *and* foreground: the fill is nearly solid in light mode, so
    /// the resting label would not survive it. Naming the pair once is what
    /// keeps eight components from each inventing a focus look — which is how
    /// three of them ended up with `border: Accent` on a zero-width border and
    /// painted nothing at all.
    /// Secondary text follows the label rather than staying `Muted`: the fill
    /// is the same for both, and a quieter grey resolved against the *resting*
    /// surface disappears into it. Focus is transient, so the one step of
    /// colour hierarchy the detail line loses here costs less than the
    /// contrast it gains — size and position still separate the two.
    pub const FOCUS_SURFACE: Self = Self {
        foreground_mix: None,
        background_mix: None,
        border_mix: None,
        foreground: Some(SemanticColorRole::FocusText),
        foreground_secondary: Some(SemanticColorRole::FocusText),
        background: Some(SemanticColorRole::FocusSurface),
        border: None,
    };

    /// What a keyboard-focused control paints when it already reserves a
    /// border width. Colour only — growing a border would move its content.
    pub const FOCUS_BORDER: Self = Self {
        foreground_mix: None,
        background_mix: None,
        border_mix: None,
        foreground: None,
        foreground_secondary: None,
        background: None,
        border: Some(SemanticColorRole::FocusBorder),
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InteractionStyle {
    /// Base paint before selected, hovered, pressed, focused and disabled states.
    pub base: SemanticPaint,
    pub selected: SemanticPaint,
    pub selected_hovered: SemanticPaint,
    pub selected_pressed: SemanticPaint,
    pub hovered: SemanticPaint,
    pub pressed: SemanticPaint,
    pub focused: SemanticPaint,
    pub disabled: SemanticPaint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TooltipVisual {
    pub label: Arc<str>,
    pub config: nana_ui_core::TooltipConfig,
    pub open: bool,
}

/// 诊断标记的严重级别（编辑器下划线颜色随之变化）。
///
/// Error / Warning 画 squiggle；Information / Hint 只走行尾文案，不画
/// 红/黄下划线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl TextDiagnosticSeverity {
    pub fn rank(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 2,
            Self::Information => 1,
            Self::Hint => 0,
        }
    }

    pub fn draws_underline(self) -> bool {
        matches!(self, Self::Error | Self::Warning)
    }
}

/// 编辑器诊断 span。`offset`/`length` 为字节偏移，宿主负责在文本变化后
/// 更新或清除；越界部分在几何计算时被钳制。`message` 供行尾文案与悬停
/// 浮窗使用；空串不画行尾、不产生悬停命中。
#[derive(Debug, Clone, PartialEq)]
pub struct TextDiagnosticSpan {
    pub offset: usize,
    pub length: usize,
    pub severity: TextDiagnosticSeverity,
    pub message: String,
}

impl TextDiagnosticSpan {
    pub fn new(offset: usize, length: usize, severity: TextDiagnosticSeverity) -> Self {
        Self {
            offset,
            length,
            severity,
            message: String::new(),
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

/// git gutter 标记种类：新增行（绿）、修改行（黄）、删除行（红）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextGitMarkKind {
    Added,
    Modified,
    Deleted,
}

/// git gutter 标记。`line` 为 1 基逻辑行号，由宿主在 git 状态变化后喂入
/// （文本变化后同样由宿主负责更新或清除）；行号 0、超过文档逻辑行数或
/// 所在行被折叠隐藏时静默跳过。渲染为 gutter 最左侧 2px 竖条，v1 对
/// Deleted 也按给定行渲染（不画在前行下缘/行间）——锚定行号由宿主负责。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextGitMark {
    pub line: u32,
    pub kind: TextGitMarkKind,
}

impl TextGitMark {
    pub fn new(line: u32, kind: TextGitMarkKind) -> Self {
        Self { line, kind }
    }
}

/// git gutter 标记条带（文本空间）：所在显示行的 2px 竖条素材，`y` 为
/// 该显示行的行顶（软换行取逻辑行行首），颜色由世界按种类解析。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextGitGutterMark {
    pub y: f32,
    pub height: f32,
    pub kind: TextGitMarkKind,
}

/// [`crate::ComponentGeometry::TextInput`] 的 git gutter 几何：按种类
/// 分组的 2px 竖条（节点空间）与已解析颜色。空种类不产生绘制批次。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextGitGutterGeometry {
    pub added: Vec<LayoutBox>,
    pub modified: Vec<LayoutBox>,
    pub deleted: Vec<LayoutBox>,
    pub added_color: [f32; 4],
    pub modified_color: [f32; 4],
    pub deleted_color: [f32; 4],
}

/// 编辑器查找匹配 span。`offset`/`length` 为字节偏移，`current` 标记"当前
/// 匹配"（渲染强调样式）。与诊断 span 相同，宿主负责在文本变化后更新或
/// 清除；越界部分在几何计算时被钳制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMatchSpan {
    pub offset: usize,
    pub length: usize,
    pub current: bool,
}

impl TextMatchSpan {
    pub fn new(offset: usize, length: usize) -> Self {
        Self {
            offset,
            length,
            current: false,
        }
    }

    /// 标记为"当前匹配"。
    pub fn current(mut self) -> Self {
        self.current = true;
        self
    }
}

/// 编辑器颜色装饰 span（VSCode color decorator 的渲染侧）：宿主给出
/// 字节区间与 RGBA 颜色，编辑器在该 span 末行行内画一个小色块方块。
/// 纯装饰：不参与布局测量与指针命中；`offset`/`length` 同诊断 span，
/// 由宿主在文本变化后更新或清除，越界部分在几何计算时被钳制。
#[derive(Debug, Clone, PartialEq)]
pub struct TextColorSwatchSpan {
    pub offset: usize,
    pub length: usize,
    /// 线性空间 RGBA（0-1）；半透明颜色由绘制层按常规 alpha 合成到背景。
    pub color: [f32; 4],
}

impl TextColorSwatchSpan {
    pub fn new(offset: usize, length: usize, color: [f32; 4]) -> Self {
        Self {
            offset,
            length,
            color,
        }
    }
}

/// Atomic inline range in a text editor: caret, delete, insert, and pointer
/// hits treat `[start, end)` as one unit. Hosts feed these after each value
/// change; invalid or overlapping ranges are ignored at use time.
///
/// `label` / `icon` / `token` are presentation: the committed value still
/// holds the host token text, but painters draw a chip instead of that text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextAtomSpan {
    pub start: usize,
    pub end: usize,
    pub label: Arc<str>,
    pub icon: nana_ui_core::Icon,
    pub token: Arc<str>,
}

impl TextAtomSpan {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            label: Arc::from(""),
            icon: nana_ui_core::Icon::File,
            token: Arc::from(""),
        }
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = label.into();
        self
    }

    pub fn icon(mut self, icon: nana_ui_core::Icon) -> Self {
        self.icon = icon;
        self
    }

    pub fn token(mut self, token: impl Into<Arc<str>>) -> Self {
        self.token = token.into();
        self
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// Chip drawn over an atomic range: body covers the token glyphs, close is a
/// hit target. Coordinates are node space once placed on
/// [`ComponentGeometry::TextInput`].
#[derive(Debug, Clone, PartialEq)]
pub struct TextAtomChip {
    pub bounds: LayoutBox,
    pub close: LayoutBox,
    pub icon: nana_ui_core::Icon,
    pub icon_bounds: LayoutBox,
    pub label: ComponentTextRegion,
    pub token: Arc<str>,
    pub start: usize,
    pub end: usize,
    pub background: [f32; 4],
    pub border: [f32; 4],
}

/// Close control on an atomic chip was activated. The editor also deletes the
/// atom range through the normal text history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAtomClosed {
    pub token: Arc<str>,
    pub start: usize,
    pub end: usize,
}

/// 代码折叠区间。字节偏移覆盖整个块（含首尾花括号），`start < end`。
///
/// 宿主在每次文本变化后重新喂 [`crate::TextArea::code_folds`]；哪些区间
/// 处于折叠态由组件内部（Runtime 侧按节点维护）管理，宿主不感知折叠
/// 状态的存储。折叠是纯视图状态：不改变 committed value。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCodeFold {
    pub start: usize,
    pub end: usize,
}

impl TextCodeFold {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 是否可折叠：区间内至少有一个换行（单行区间没有可隐藏的行）。
    pub fn collapsible_in(self, value: &str) -> bool {
        self.start < self.end
            && self.end <= value.len()
            && value[self.start.min(value.len())..self.end].contains('\n')
    }

    /// 折叠后第一个被隐藏的字节：`start` 所在行的行尾换行符位置
    /// （区间无换行时等于 `end`，即无可隐藏内容）。
    pub fn hidden_start_in(self, value: &str) -> usize {
        value[self.start.min(value.len())..]
            .find('\n')
            .map_or(self.end, |index| self.start + index)
            .min(self.end.max(self.start))
    }
}

/// 行内提示（inlay）的一条喂入条目。`offset` 是值空间锚点（实参首
/// 字节），`label` 是插入显示的装饰文本（如 `t:`）；纯插入型显示
/// span——参与布局测量与软换行，但不进缓冲区：光标/选区/查找/撤销的
/// offset 语义由显示视图映射自动免疫（插入区间内部无光标边界，点击
/// 穿透吸附最近的缓冲字符）。世界校验：锚点必须是当前值的 char
/// boundary 且不越界、文本非空且不含 `'\n'`（行数换算按 `\n` 计数），
/// 按 `(offset, label)` 排序去重，非法条目钳除。IME 组合期与折叠隐藏
/// 区间内的条目不显示（组合期整体退场，同 swatch 先例）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInlay {
    pub offset: usize,
    pub label: String,
}

impl TextInlay {
    pub fn new(offset: usize, label: impl Into<String>) -> Self {
        Self {
            offset,
            label: label.into(),
        }
    }
}

/// 宿主触发的代码片段。`body` 中的 `$N`（N 为十进制数字）是占位标记：
/// 插入时从文本中移除，`$1..$N` 按序号成为 Tab 跳位，`$0` 是插入后的
/// 初始光标位置（缺省为插入文本末尾）。其余字符原样插入，`$` 后不跟
/// 数字时保持字面量。不支持 `${N:default}` 与占位镜显。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSnippet {
    pub label: String,
    pub body: String,
}

impl TextSnippet {
    pub fn new(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            body: body.into(),
        }
    }
}

/// 活跃的 snippet 会话（每个编辑器节点至多一个）。`stops` 是 `$1..$N`
/// 在 committed value 中的光标偏移，`index` 指向当前跳位。文本被外部
/// 编辑时跳位按最小变更区间重映射，失效即结束会话。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextSnippetSession {
    pub exit_on_last: bool,
    pub placeholders: Vec<crate::SnippetPlaceholder>,
    pub stops: Vec<usize>,
    /// Selection ends for placeholder defaults; empty means all stops are carets.
    pub selection_ends: Vec<usize>,
    pub index: usize,
}

/// 文本空间内的查找匹配高亮条带（按折行拆分后可能一条 span 对应多条）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextMatchMark {
    pub rect: LayoutBox,
    pub current: bool,
}

/// 折叠摘要标记（文本空间）：折叠起始行尾的 ` …N` 文本框，可点击
/// 切换该折叠。矩形由 shape 管线用真实字形度量计算。
#[derive(Debug, Clone, PartialEq)]
pub struct TextFoldMark {
    pub rect: LayoutBox,
    pub fold: TextCodeFold,
}

/// gutter 折叠箭头（节点空间）：可点击切换的命中框，`collapsed` 决定
/// 画收起（右箭头）还是展开（下箭头）形态。颜色由世界按低对比令牌解析。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFoldGutter {
    pub bounds: LayoutBox,
    pub fold: TextCodeFold,
    pub collapsed: bool,
    pub color: [f32; 4],
}

/// 折叠摘要标记命中框（节点空间），落在折叠起始行行尾。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFoldMarker {
    pub bounds: LayoutBox,
    pub fold: TextCodeFold,
}

/// [`crate::ComponentGeometry::TextInput`] 的折叠几何集合。空集合即
/// 默认值，零额外成本。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextFoldGeometry {
    pub gutters: Vec<TextFoldGutter>,
    pub markers: Vec<TextFoldMarker>,
}

/// A multiline editor's scroll along the page's x axis, which the Scene draws
/// as a translation so the painter snaps it to whole device pixels (#223).
/// The geometry's boxes still include it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextInputScroll {
    /// The page-x displacement the scroll adds: `-scrollLeft` for horizontal
    /// text.
    pub offset_x: f32,
    /// The value's box's x without the scroll, exactly: glyphs are keyed on
    /// its sub-pixel phase, and `text.bounds.x - offset_x` can be an ulp off.
    pub text_x: f32,
}

/// 节点空间内的查找匹配高亮条带，颜色由世界按当前/普通匹配解析。
#[derive(Debug, Clone, PartialEq)]
pub struct TextMatchMarker {
    pub rect: LayoutBox,
    pub color: [f32; 4],
    pub current: bool,
}

/// 一条补全候选。`label` 是接受后插入的主体；`kind_label` 是右侧类型
/// 标注（如 `fn` / `struct` / `关键字`，宿主决定文案）；`detail` 是签名等
/// 次要说明，可为空；`doc` 是候选的简短文档行（弹层内 label 行下方的
/// 次要色单行，超宽省略号截断），可为空。
///
/// 过滤完全由宿主负责：宿主按当前词前缀过滤后喂入
/// [`crate::TextArea::completions`]，组件只负责展示、键盘导航与接受，
/// 不做任何候选匹配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextCompletion {
    pub label: String,
    pub kind_label: String,
    pub detail: String,
    pub doc: String,
    /// Atomic host-provided edit, anchored to the source and caret which produced it.
    /// Absent for ordinary word-prefix completions.
    pub edit: Option<TextCompletionEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextCompletionEdit {
    pub snippet: Option<String>,
    pub variables: std::collections::BTreeMap<String, String>,
    pub source: std::sync::Arc<str>,
    pub caret: usize,
    pub range: std::ops::Range<usize>,
    pub text: String,
    /// Additional replacements use offsets in `source`, not the edited result.
    pub additional: Vec<(std::ops::Range<usize>, String)>,
    /// Tab stops relative to `text`, in navigation order; first is selected on acceptance.
    pub stops: Vec<std::ops::Range<usize>>,
}

impl TextCompletion {
    pub fn new(label: impl Into<String>, kind_label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind_label: kind_label.into(),
            detail: String::new(),
            doc: String::new(),
            edit: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn edit(mut self, edit: TextCompletionEdit) -> Self {
        self.edit = Some(edit);
        self
    }

    /// 设置文档行（label 行下方的次要色单行说明）。
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }
}

/// 锚定某个偏移的 hover 文档浮窗内容。纯展示：触发与生命周期完全由
/// 宿主决定（文本编辑时宿主负责撤掉，组件不自动隐藏）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHover {
    pub offset: usize,
    pub title: String,
    pub body: String,
}

impl TextHover {
    pub fn new(offset: usize, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            offset,
            title: title.into(),
            body: body.into(),
        }
    }
}

/// 补全弹层可见行数上限（同时是键盘导航的滚动窗口大小）。
pub const TEXT_COMPLETION_VISIBLE_ROWS: usize = 8;

/// 补全弹层面板的水平内边距与内容宽度上限（"宽度自适应最长行"的
/// 上限；超出后先压缩 detail 区域，label/kind 保持完整）。
pub const TEXT_COMPLETION_PANEL_PAD: f32 = nana_ui_core::space::MD;
pub const TEXT_COMPLETION_MAX_CONTENT_WIDTH: f32 = 344.0;

/// 补全会话的只读快照（宿主查询入口；会话不存在时无值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCompletionSnapshot {
    /// 当前候选总数。
    pub count: usize,
    /// 键盘选中的候选下标。
    pub selected: usize,
    /// 第一条可见候选的绝对下标。
    pub scroll: usize,
    /// Esc 关闭标记：弹层是否处于关闭态。
    pub dismissed: bool,
}

/// shape 管线按候选列表缓存的行宽度量。`items` 是指针相等短路键：
/// 列表未变时整个度量直接复用，零测量、零分配。
#[derive(Debug, Clone, PartialEq)]
pub struct TextCompletionPopupMetrics {
    pub items: Arc<[TextCompletion]>,
    /// 最宽 label（未裁剪）。
    pub label_width: f32,
    /// 最宽 detail（未裁剪）。
    pub detail_width: f32,
    /// 最宽 kind 标注（未裁剪）。
    pub kind_width: f32,
}

/// hover 浮窗正文的最大可见行数；超出部分滚轮滚动查看。
pub const TEXT_HOVER_MAX_BODY_ROWS: usize = 10;

/// 补全弹层的一行几何（节点空间）。`bounds` 是整行（选中高亮底），
/// 文本框带内容与颜色，由场景绘制（不换行，超宽省略号截断）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextCompletionRow {
    pub bounds: LayoutBox,
    pub label: ComponentTextRegion,
    /// 次要说明；`detail` 为空的候选没有。
    pub detail: Option<ComponentTextRegion>,
    /// 右对齐类型标注；所有候选的 kind 都为空时整列省略。
    pub kind: Option<ComponentTextRegion>,
    /// 文档行：label 行下方的次要色单行（`doc` 为空的候选没有；有文档
    /// 行的候选整行占两行高）。
    pub doc: Option<ComponentTextRegion>,
}

/// [`crate::ComponentGeometry::TextInput`] 的补全弹层几何。仅聚焦多行
/// 编辑器且候选会话非空时存在；绘制在编辑器所有覆盖层（折叠、诊断、
/// 行号）之上，点击命中与键盘交互由框架命令处理。
#[derive(Debug, Clone, PartialEq)]
pub struct TextCompletionPopup {
    pub panel: LayoutBox,
    /// 当前选中项的绝对候选下标。
    pub selected: usize,
    /// 第一条可见候选的绝对下标（滚动位置）。
    pub first_row: usize,
    pub rows: Vec<TextCompletionRow>,
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub selected_background: [f32; 4],
    pub label_color: [f32; 4],
    pub detail_color: [f32; 4],
    pub kind_color: [f32; 4],
}

/// [`crate::ComponentGeometry::TextInput`] 的 hover 文档浮窗几何。宿主
/// 喂入 [`crate::TextArea::hover`] 时存在，纯展示；滚轮滚动由框架处理。
#[derive(Debug, Clone, PartialEq)]
pub struct TextHoverPopup {
    pub panel: LayoutBox,
    pub title: ComponentTextRegion,
    /// 可见正文行（按逻辑行滚动切片）。多行、超长裁剪，滚轮消费。
    pub body_rows: Vec<ComponentTextRegion>,
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub title_color: [f32; 4],
    pub body_color: [f32; 4],
}

/// 宿主喂入的签名帮助。活动参数下标越界时由几何层钳到末参。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSignatureHelp {
    pub title: String,
    pub fn_doc: String,
    pub params: Vec<(String, String)>,
    pub active_index: usize,
}

impl TextSignatureHelp {
    pub fn new(
        title: impl Into<String>,
        fn_doc: impl Into<String>,
        params: Vec<(String, String)>,
        active_index: usize,
    ) -> Self {
        Self {
            title: title.into(),
            fn_doc: fn_doc.into(),
            params,
            active_index,
        }
    }
}

/// 签名帮助浮窗几何：签名行拆成前缀 / 活动参数 / 后缀，slot 140+。
#[derive(Debug, Clone, PartialEq)]
pub struct TextSignaturePopup {
    pub panel: LayoutBox,
    pub prefix: ComponentTextRegion,
    pub active: Option<ComponentTextRegion>,
    pub suffix: ComponentTextRegion,
    pub doc: Option<ComponentTextRegion>,
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub active_background: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub enum StandardVisual {
    ModalFrame {
        title: Arc<str>,
        description: Option<Arc<str>>,
        body_text: Option<Arc<str>>,
        kind: crate::ModalSurfaceKind,
        busy: bool,
        danger: bool,
        slots: crate::ModalSlots,
    },
    Button {
        label: Arc<str>,
        icon: Option<nana_ui_core::Icon>,
        /// A glyph after the label, such as the chevron of a picker. Same
        /// size and gap as the leading one.
        trailing_icon: Option<nana_ui_core::Icon>,
        icon_size: f32,
        icon_gap: f32,
        kind: nana_ui_core::ButtonKind,
        size: ControlSize,
        loading: bool,
        loading_phase: f32,
        invalid: bool,
    },
    TextInput {
        placeholder: Arc<str>,
        size: ControlSize,
        secure: bool,
        invalid: bool,
        /// Numeric spinner affordance at the trailing edge. `NumberInput` is a
        /// text input that also steps, so it reuses this visual instead of
        /// growing a second editable contract.
        steppers: bool,
        /// 代码编辑器扩展：编译诊断 span 标记（含可选 message）。偏移由宿主
        /// 维护——文本变化后由宿主更新或清除，渲染层仅做越界钳制，不做偏移
        /// 迁移。Error/Warning 画 squiggle；有 message 时画行尾文案并可悬停。
        diagnostics: Arc<[TextDiagnosticSpan]>,
        /// 查找匹配高亮 span（普通匹配与当前匹配，见 [`TextMatchSpan`]）。
        /// 偏移同样由宿主维护。
        matches: Arc<[TextMatchSpan]>,
        /// 颜色装饰 span（行内色块，见 [`TextColorSwatchSpan`]）。偏移同样
        /// 由宿主维护；纯装饰不参与布局与命中，仅多行态派生几何。
        color_swatches: Arc<[TextColorSwatchSpan]>,
        /// Atomic inline chips (see [`TextAtomSpan`]). Hosts feed these with
        /// the committed token range plus display label/icon. Painters cover
        /// the token glyphs with a chip; close hits delete the range.
        atoms: Arc<[TextAtomSpan]>,
        /// 行号栏。行号绘制在节点左内边距区域，宿主需预留足够的 padding。
        line_numbers: bool,
        /// 缩进参考线。`Some(indent_unit)` 时在每个逻辑行的前导空白处按
        /// 缩进单位宽度画竖线（仅多行态生效）。
        indent_guides: Option<Arc<str>>,
        /// 代码折叠区间（见 [`TextCodeFold`]）。宿主在文本变化后重新喂；
        /// 哪些区间处于折叠态由组件内部维护，渲染按折叠后的视图展示。
        folds: Arc<[TextCodeFold]>,
        /// git gutter 标记（见 [`TextGitMark`]）。宿主在 git 状态与文本
        /// 变化后重新喂；行号无效或被折叠隐藏的标记在几何派生时静默跳过。
        git_marks: Arc<[TextGitMark]>,
        /// 多行编辑器的内部派生渲染选项（出现高亮、相对行号、空白字符
        /// 显示、wrap guide 列参考线）。全部默认关闭；开启的特征由世界侧
        /// 从输入状态派生几何，不给宿主增加喂入负担。
        editor_options: TextEditorRenderOptions,
    },
    Checkbox {
        checked: bool,
        /// Mixed state. Wins over `checked` when painting and in a11y.
        indeterminate: bool,
        size: ControlSize,
    },
    Icon {
        icon: Icon,
        size: f32,
        tooltip: Option<TooltipVisual>,
    },
    Switch {
        thumb_progress: f32,
        label: Arc<str>,
        hint: Option<Arc<str>>,
        checked: bool,
        control_position: SwitchControlPosition,
        size: ControlSize,
        loading: bool,
        loading_phase: f32,
        invalid: bool,
    },
    Range {
        label: Option<Arc<str>>,
        value: Arc<str>,
        unit: Option<Arc<str>>,
        size: ControlSize,
        ratio: f32,
        invalid: bool,
    },
    /// Scroll container chrome. Carries policy only: the track and thumb boxes
    /// come from the authoritative [`ScrollOffset`] / [`ScrollMetrics`] at
    /// extraction time, so no scroll position is duplicated here.
    Scrollbar {
        axes: crate::ScrollAxes,
        visibility: nana_ui_core::ScrollbarVisibility,
        revealed: bool,
        dragging: Option<nana_ui_core::ScrollbarAxis>,
    },
    Card {
        title: Option<Arc<str>>,
        kind: CardKind,
        loading: bool,
        loading_phase: f32,
    },
    ListItem {
        leading: Option<StableNodeId>,
        content: Option<StableNodeId>,
        trailing: Option<StableNodeId>,
        /// 单行补充信息；几何端生成右对齐的小字号 muted 文本区。
        detail: Option<Arc<str>>,
    },
    StatusBadge {
        label: Arc<str>,
        tone: nana_ui_core::StatusTone,
        compact: bool,
    },
    ValidationMessage {
        message: Arc<str>,
        intent: nana_ui_core::ValidationIntent,
        compact: bool,
    },
    EmptyState {
        title: Arc<str>,
        message: Option<Arc<str>>,
        icon: Option<Icon>,
        compact: bool,
        action: Option<StableNodeId>,
    },
    LabeledValue {
        label: Arc<str>,
        value: Arc<str>,
        value_role: SemanticColorRole,
        value_weight: u16,
        compact: bool,
        action: Option<StableNodeId>,
    },
    SelectionOption {
        label: Arc<str>,
        icon: Option<Icon>,
        selected: bool,
        disabled: bool,
        size: ControlSize,
        show_focus_ring: bool,
        /// Draws a radio ring, and a dot when selected, before the label.
        indicator: bool,
    },
    Progress {
        value_ratio: f32,
        label: Option<Arc<str>>,
        cancellable: bool,
    },
    Spinner {
        label: Arc<str>,
        size: f32,
        phase: f32,
    },
    LevelMeter {
        value_ratio: f32,
        girth: f32,
        tone: nana_ui_core::StatusTone,
    },
    FormField {
        label: Arc<str>,
        hint: Option<Arc<str>>,
        error: Option<Arc<str>>,
        size: ControlSize,
        control: Option<StableNodeId>,
    },
    QrCode {
        modules: Arc<[bool]>,
        width: usize,
    },
    Toast {
        title: Arc<str>,
        description: Option<Arc<str>>,
        tone: nana_ui_core::ToastTone,
        dismissible: bool,
    },
    XYPad {
        value: nana_ui_core::XYPadValue,
        nx: f32,
        ny: f32,
        size: ControlSize,
        invalid: bool,
        disabled: bool,
    },
    Select {
        label: Arc<str>,
        placeholder: bool,
        size: ControlSize,
        opened: bool,
        invalid: bool,
        loading: bool,
        options: Arc<[SelectOptionData]>,
        highlighted: Option<usize>,
        /// Whether this menu can ever draw check marks (a multi-select
        /// dropdown). The check lane is reserved for the whole menu when it
        /// can, so labels do not shift as options are checked and unchecked.
        checkable: bool,
    },
    MenuSurface {
        kind: MenuSurfaceKind,
        open: bool,
        trigger: Option<Arc<str>>,
        trigger_icon: Option<Icon>,
        /// Host-texture trigger (hover-card avatar). Painted by the node's own
        /// custom render slot; the label never becomes a text region.
        trigger_image: Option<Arc<str>>,
        gap: f32,
        /// Present when this node is an in-flow trigger; the menu items layout
        /// as a separate overlay column and must not share this node's box.
        overlay: Option<TriggeredMenuOverlay>,
        query: Option<Arc<str>>,
        rows: Arc<[SelectOptionData]>,
        highlighted: Option<usize>,
    },
    ActionMenuItem {
        label: Arc<str>,
        hint: Option<Arc<str>>,
        icon: Option<Icon>,
        danger: bool,
        active: bool,
        disabled: bool,
        size: ControlSize,
    },
    TreeView {
        rows: Arc<[crate::tree_view::TreeRowData]>,
        size: ControlSize,
    },
    CommandPalette {
        title: Arc<str>,
        query: Arc<str>,
        placeholder: Arc<str>,
        empty: Option<Arc<str>>,
        rows: Arc<[crate::command_palette::PaletteRowData]>,
    },
    #[cfg(feature = "calendar")]
    CalendarHeatmap {
        cells: Arc<[crate::calendar::CalendarHeatmapCellPaint]>,
        month_labels: Arc<[crate::calendar::CalendarHeatmapLabelPaint]>,
        day_labels: Arc<[crate::calendar::CalendarHeatmapLabelPaint]>,
        cell_size: f32,
        cell_radius: f32,
        max_level: u8,
        active: Option<usize>,
        active_title: Option<Arc<str>>,
    },
    #[cfg(feature = "charts")]
    DonutChart {
        slices: Arc<[crate::DonutSlice]>,
        cutout: f32,
        separator: f32,
        active: Option<usize>,
    },
    #[cfg(feature = "charts")]
    StackedTimeSeriesChart {
        title: Arc<str>,
        values: Arc<[f64]>,
        layers: Arc<[crate::TimeSeriesLayer]>,
        labels: Arc<[Arc<str>]>,
        active: Option<usize>,
    },
    #[cfg(feature = "charts")]
    TimeSeriesChart {
        values: Arc<[f64]>,
    },
    #[cfg(feature = "charts")]
    TimestampSeriesChart {
        samples: Arc<[(i64, Option<f64>)]>,
        unit: Option<Arc<str>>,
        time_labels: Option<(Arc<str>, Arc<str>)>,
    },
    #[cfg(feature = "controls")]
    ReorderList {
        rows: Arc<[crate::reorder_list::ReorderRowPaint]>,
        size: ControlSize,
        spacing: f32,
        insert: Option<LayoutBox>,
    },
    #[cfg(feature = "rich-text")]
    NativeMarkdown {
        blocks: Arc<[crate::MarkdownBlock]>,
        text: Arc<str>,
        selection: Option<(usize, usize)>,
    },
    #[cfg(feature = "rich-text")]
    SelectableRichText {
        text: Arc<str>,
        selection: Option<(usize, usize)>,
    },
    #[cfg(feature = "graph-canvas")]
    GraphCanvas {
        nodes: Arc<[crate::graph_canvas::GraphNodePaint]>,
        ports: Arc<[crate::graph_canvas::GraphPortPaint]>,
        edges: Arc<[crate::graph_canvas::GraphEdgePaint]>,
        connecting: Option<crate::graph_canvas::GraphEdgePaint>,
        grid_spacing: f32,
        viewport_offset_x: f32,
        viewport_offset_y: f32,
        viewport_zoom: f32,
    },
    /// Overview minimap policy. Node rectangles and the indicator stay in
    /// world space; the uniform map projection resolves at extraction time
    /// against the final widget box.
    #[cfg(feature = "graph-canvas")]
    GraphMinimap {
        bounds: nana_ui_core::GraphRect,
        nodes: Arc<[nana_ui_core::GraphRect]>,
        indicator: Option<nana_ui_core::GraphRect>,
        node_fill: Option<nana_ui_core::SemanticColorRole>,
    },
    #[cfg(feature = "image-viewer")]
    ImageViewer {
        intrinsic_size: Option<(u32, u32)>,
        name: Option<Arc<str>>,
        metadata: Option<Arc<str>>,
        zoom: f32,
        offset_x: f32,
        offset_y: f32,
    },
    KeyCaptureLayer {
        recording: bool,
    },
    KeymapLayer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOptionData {
    pub label: Arc<str>,
    pub hint: Option<Arc<str>>,
    pub disabled: bool,
    pub checked: bool,
    pub icon: Option<Icon>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuSurfaceKind {
    Popover,
    ActionMenu,
    ContextMenu,
    HoverCard,
}

/// Anchor for an in-flow trigger's overlay panel. The trigger node stays in
/// the host layout; items are placed at this origin as a viewport-fixed column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriggeredMenuOverlay {
    pub placement: PopoverPlacement,
    pub alignment: PopoverAlignment,
    pub width: f32,
    pub padding: f32,
    pub gap: f32,
    /// The child that draws the trigger ([`crate::Popover::trigger_content`]):
    /// it stays in flow inside the trigger, shown while the surface is
    /// closed, and is not one of the items the surface places.
    pub trigger_content: Option<StableNodeId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentTextRegion {
    pub bounds: LayoutBox,
    pub content: TextValue,
    pub color: Option<[f32; 4]>,
    pub font_size: f32,
    pub font_weight: Option<u16>,
}

/// Button chrome drawn behind a component's own trigger, so a menu can look
/// pressable without the caller supplying a separate button node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentTriggerSurface {
    pub bounds: LayoutBox,
    pub background: Option<[f32; 4]>,
    pub border: Option<[f32; 4]>,
}

/// Resolved spinner chrome at the trailing edge of a numeric field.
///
/// Both halves stay drawn while the field is enabled; `increment_enabled` /
/// `decrement_enabled` only report whether the value can still move, so the
/// control does not resize as it reaches a bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberSteppers {
    pub increment: LayoutBox,
    pub decrement: LayoutBox,
    pub increment_color: [f32; 4],
    pub decrement_color: [f32; 4],
    pub increment_enabled: bool,
    pub decrement_enabled: bool,
    pub glyph_size: f32,
}

impl NumberSteppers {
    /// Signed step count for a point, or `None` when it misses both halves.
    pub fn step_at(&self, x: f32, y: f32) -> Option<i32> {
        if contains(self.increment, x, y) {
            return self.increment_enabled.then_some(1);
        }
        if contains(self.decrement, x, y) {
            return self.decrement_enabled.then_some(-1);
        }
        None
    }

    /// Whether a point is on either half, enabled or not: a press there
    /// belongs to the spinner even when the value cannot move that way.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        contains(self.increment, x, y) || contains(self.decrement, x, y)
    }
}

fn contains(bounds: LayoutBox, x: f32, y: f32) -> bool {
    x >= bounds.x && x < bounds.x + bounds.width && y >= bounds.y && y < bounds.y + bounds.height
}

/// Resolved radio ring chrome for one selection option.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioIndicator {
    pub ring: LayoutBox,
    pub ring_color: [f32; 4],
    /// Present only while the option is selected.
    pub dot: Option<(LayoutBox, [f32; 4])>,
}

/// One axis of resolved scrollbar chrome.
///
/// `track` is the full band along the viewport edge and is also the drag hit
/// region; `thumb` is the draggable handle inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarBar {
    pub track: LayoutBox,
    pub thumb: LayoutBox,
    /// Painted only when the container keeps its bars resident.
    pub track_background: Option<[f32; 4]>,
    pub thumb_background: [f32; 4],
    pub thumb_radius: f32,
    /// Smallest and largest content offsets this axis can reach.
    pub min_offset: f32,
    pub max_offset: f32,
}

impl ScrollbarBar {
    /// Position along the scrolling axis for a viewport point.
    pub fn axis_position(&self, axis: nana_ui_core::ScrollbarAxis, x: f32, y: f32) -> f32 {
        match axis {
            nana_ui_core::ScrollbarAxis::Horizontal => x,
            nana_ui_core::ScrollbarAxis::Vertical => y,
        }
    }

    /// Re-derive the axis track so drag maths stay in one place.
    pub fn track_geometry(
        &self,
        axis: nana_ui_core::ScrollbarAxis,
    ) -> nana_ui_core::ScrollbarTrack {
        match axis {
            nana_ui_core::ScrollbarAxis::Horizontal => nana_ui_core::ScrollbarTrack {
                origin: self.track.x,
                length: self.track.width,
                thumb_origin: self.thumb.x,
                thumb_length: self.thumb.width,
                min_offset: self.min_offset,
                max_offset: self.max_offset,
            },
            nana_ui_core::ScrollbarAxis::Vertical => nana_ui_core::ScrollbarTrack {
                origin: self.track.y,
                length: self.track.height,
                thumb_origin: self.thumb.y,
                thumb_length: self.thumb.height,
                min_offset: self.min_offset,
                max_offset: self.max_offset,
            },
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.track.x
            && x < self.track.x + self.track.width
            && y >= self.track.y
            && y < self.track.y + self.track.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentElevation {
    pub color: [f32; 4],
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread_radius: f32,
    /// CSS `box-shadow: inset`. Outset drop shadows keep this `false`.
    pub inset: bool,
}

impl ComponentElevation {
    /// One elevation step of a theme, as the scene consumes it.
    pub fn from_shadow(shadow: nana_ui_core::ShadowToken) -> Self {
        Self {
            color: shadow.color.as_rgba_array(),
            offset_x: shadow.offset_x,
            offset_y: shadow.offset_y,
            blur_radius: shadow.blur_radius,
            spread_radius: shadow.spread_radius,
            inset: shadow.inset,
        }
    }

    /// The **default theme's** lifted-surface shadow for `theme_mode`.
    ///
    /// Lilia `--shadow-surface`: `0 10px 30px -24px` (dark) / `0 10px 26px
    /// -24px` (light). The two used to be a `match theme_mode` written here,
    /// which made this function the authority on how high a menu floats. It is
    /// now [`ElevationRole::Surface`](nana_ui_core::ElevationRole) on the
    /// theme; callers holding a world should ask it via
    /// `world.theme().shadow(..)` so an installed theme can move the ramp.
    pub fn surface_shadow(theme_mode: nana_ui_core::ThemeMode) -> Self {
        Self::from_shadow(
            nana_ui_core::EffectTokens::for_mode(theme_mode)
                .shadow(nana_ui_core::ElevationRole::Surface),
        )
    }

    pub fn from_box_shadow(shadow: nana_ui_core::BoxShadowSpec) -> Self {
        Self {
            color: shadow.color,
            offset_x: shadow.offset_x,
            offset_y: shadow.offset_y,
            blur_radius: shadow.blur_radius,
            spread_radius: shadow.spread_radius,
            inset: shadow.inset,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarHoverGeometry {
    pub ring: LayoutBox,
    pub tooltip: LayoutBox,
    pub title: ComponentTextRegion,
    pub ring_color: [f32; 4],
    pub tooltip_fill: [f32; 4],
    pub tooltip_border: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
// Keep the extracted geometry contract inline: boxing the hot text variant adds
// an allocation per projection and changes the shared Runtime/Scene DTO.
#[allow(clippy::large_enum_variant)]
pub enum ComponentGeometry {
    ModalFrame {
        scrim: LayoutBox,
        surface: LayoutBox,
        body: LayoutBox,
        title: ComponentTextRegion,
        description: Option<ComponentTextRegion>,
        body_text: Option<ComponentTextRegion>,
        background: [f32; 4],
        border: [f32; 4],
        elevation: ComponentElevation,
    },
    Button {
        icon: Option<(nana_ui_core::Icon, LayoutBox)>,
        trailing_icon: Option<(nana_ui_core::Icon, LayoutBox)>,
        label: ComponentTextRegion,
        spinner: Option<LayoutBox>,
        background: Option<[f32; 4]>,
        border: Option<[f32; 4]>,
        border_width: f32,
        focus_ring: Option<[f32; 4]>,
    },
    TextInput {
        resize_grip: Option<LayoutBox>,
        text: ComponentTextRegion,
        multiline: bool,
        /// `None` for a single-line field, whose scroll only follows its caret.
        scroll: Option<TextInputScroll>,
        selection: Vec<LayoutBox>,
        caret: Option<LayoutBox>,
        /// 附加多光标（收起态）的 caret 矩形；主光标仍在 `caret`。
        additional_carets: Vec<LayoutBox>,
        /// 附加光标颜色（主光标 `caret_color` 的半透明变体，保持同形）。
        additional_caret_color: [f32; 4],
        preedit: Vec<LayoutBox>,
        /// 诊断下划线条带（节点空间矩形 + 已解析的颜色）。
        diagnostic_markers: Vec<(LayoutBox, [f32; 4])>,
        /// 行尾诊断文案（节点空间文字区域 + 已解析颜色）。
        diagnostic_labels: Vec<ComponentTextRegion>,
        /// 查找匹配高亮条带（节点空间矩形 + 已解析的颜色；`current` 为当前
        /// 匹配，绘制层级在普通匹配之上）。
        match_markers: Vec<TextMatchMarker>,
        /// 颜色装饰 swatch（节点空间矩形 + 宿主给定的颜色；单一批次绘制）。
        swatch_markers: Vec<(LayoutBox, [f32; 4])>,
        /// swatch 细描边的统一颜色（与空白字符 `whitespace_color` 同为
        /// 世界按令牌解析的共享色）。
        swatch_border_color: [f32; 4],
        /// Inline atom chips covering token glyphs (icon + label + close).
        atom_chips: Vec<TextAtomChip>,
        /// 光标所在行的低对比背景条。仅聚焦且选区收起时存在，占用选区层
        /// （选区与当前行条互斥）。
        caret_line: Option<(LayoutBox, [f32; 4])>,
        /// 光标相邻括号与其配对端的描边框（节点空间矩形 + 已解析的颜色）。
        bracket_markers: Vec<(LayoutBox, [f32; 4])>,
        /// 出现高亮填充条带（节点空间矩形 + 已解析的颜色；聚焦时存在，
        /// 弱于查找匹配的淡底色）。
        occurrence_markers: Vec<(LayoutBox, [f32; 4])>,
        /// 拖拽移动选中文本的落点指示线（节点空间细竖线 + 已解析的
        /// 颜色）。仅在选区拖拽态存在；纯视觉指示，无命中框。
        drop_indicator: Option<(LayoutBox, [f32; 4])>,
        /// 空白字符标记（节点空间字符单元矩形 + 种类；颜色统一由
        /// `whitespace_color` 给出，几何层按视口裁剪）。
        whitespace_marks: Vec<(LayoutBox, TextWhitespaceKind)>,
        whitespace_color: [f32; 4],
        /// wrap guide 全高竖线（节点空间矩形 + 已解析的颜色）。
        wrap_guides: Vec<(LayoutBox, [f32; 4])>,
        /// 缩进参考线竖线（节点空间矩形 + 已解析的颜色）。
        indent_guides: Vec<(LayoutBox, [f32; 4])>,
        /// 行号标签（节点空间 y，行号从 1 起）。
        line_labels: Vec<LineLabel>,
        /// 折叠几何：gutter 箭头（可点击切换）与折叠摘要标记命中框。
        folds: TextFoldGeometry,
        /// git gutter 竖条（按种类分组，节点空间矩形 + 已解析的颜色）。
        git_marks: TextGitGutterGeometry,
        /// 行号文本颜色与字号。
        line_labels_color: [f32; 4],
        line_labels_font_size: f32,
        /// 补全弹层（聚焦多行编辑器 + 非空候选会话时存在）。绘制在
        /// 全部编辑器覆盖层之上（slot 90+）。
        completion_popup: Option<TextCompletionPopup>,
        /// hover 文档浮窗（宿主喂入时存在，纯展示；slot 120+）。
        hover_popup: Option<TextHoverPopup>,
        /// 签名帮助浮窗（宿主喂入时存在，锚定 caret；slot 140+）。
        signature_popup: Option<TextSignaturePopup>,
        /// minimap 竖条（开启选项的多行编辑器存在；slot 70/71/72）。
        minimap: Option<TextMinimapGeometry>,
        /// sticky scroll 钉住行（开启选项且滚动视口落在宿主喂入的折叠区间
        /// 内部时存在；slot 80-82）。
        sticky_line: Option<TextStickyLineGeometry>,
        background: Option<[f32; 4]>,
        border: Option<[f32; 4]>,
        border_width: f32,
        focus_ring: Option<[f32; 4]>,
        selection_color: [f32; 4],
        caret_color: [f32; 4],
        preedit_color: [f32; 4],
        steppers: Option<NumberSteppers>,
    },
    Switch {
        thumb_progress: f32,
        label: ComponentTextRegion,
        hint: Option<ComponentTextRegion>,
        control: LayoutBox,
        track_background: [f32; 4],
        track_border: [f32; 4],
        thumb_background: [f32; 4],
    },
    Range {
        label: Option<ComponentTextRegion>,
        value: ComponentTextRegion,
        unit: Option<ComponentTextRegion>,
        track: LayoutBox,
    },
    Scrollbar {
        horizontal: Option<ScrollbarBar>,
        vertical: Option<ScrollbarBar>,
    },
    Card {
        title: Option<ComponentTextRegion>,
        content: LayoutBox,
        elevation: Option<ComponentElevation>,
        spinner: Option<LayoutBox>,
    },
    ListItem {
        leading: Option<LayoutBox>,
        content: Option<LayoutBox>,
        trailing: Option<LayoutBox>,
        detail: Option<ComponentTextRegion>,
    },
    StatusBadge {
        indicator: LayoutBox,
        label: ComponentTextRegion,
        background: [f32; 4],
        foreground: [f32; 4],
    },
    ValidationMessage {
        indicator: LayoutBox,
        label: ComponentTextRegion,
        foreground: [f32; 4],
    },
    EmptyState {
        root_clip: LayoutBox,
        content_clip: LayoutBox,
        icon: Option<(Icon, LayoutBox, [f32; 4])>,
        title: ComponentTextRegion,
        message: Option<ComponentTextRegion>,
        action: Option<LayoutBox>,
    },
    LabeledValue {
        label: ComponentTextRegion,
        value: ComponentTextRegion,
        action: Option<LayoutBox>,
    },
    SelectionOption {
        icon: Option<(Icon, LayoutBox, [f32; 4])>,
        label: ComponentTextRegion,
        focus_ring: Option<[f32; 4]>,
        indicator: Option<RadioIndicator>,
    },
    Progress {
        track: LayoutBox,
        fill: LayoutBox,
        label: Option<ComponentTextRegion>,
        cancel: Option<LayoutBox>,
        corner_radius: f32,
    },
    FormField {
        label: ComponentTextRegion,
        support: Option<ComponentTextRegion>,
        indicator: Option<(LayoutBox, [f32; 4])>,
        control: Option<LayoutBox>,
    },
    QrCode {
        field: LayoutBox,
        module_size: f32,
        dark: Vec<LayoutBox>,
    },
    Toast {
        indicator: LayoutBox,
        title: ComponentTextRegion,
        description: Option<ComponentTextRegion>,
        dismiss: Option<LayoutBox>,
    },
    XYPad {
        pad: LayoutBox,
        thumb: LayoutBox,
        h_axis: LayoutBox,
        v_axis: LayoutBox,
        background: Option<[f32; 4]>,
        border: Option<[f32; 4]>,
        border_width: f32,
        thumb_color: [f32; 4],
        axis_color: [f32; 4],
    },
    Select {
        label: ComponentTextRegion,
        handle: LayoutBox,
        handle_color: [f32; 4],
        background: Option<[f32; 4]>,
        border: Option<[f32; 4]>,
        border_width: f32,
        menu: Option<SelectMenuGeometry>,
    },
    MenuSurface {
        trigger: Option<ComponentTextRegion>,
        trigger_icon: Option<(Icon, LayoutBox)>,
        /// Image trigger (hover-card avatar): the chrome is circular and the
        /// texture rides the node's own custom render slot.
        trigger_image: Option<Arc<str>>,
        trigger_surface: Option<ComponentTriggerSurface>,
        surface: LayoutBox,
        search: Option<ComponentTextRegion>,
        search_field: Option<LayoutBox>,
        options: Vec<SelectOptionGeometry>,
        elevation: ComponentElevation,
        background: [f32; 4],
        border: [f32; 4],
    },
    ActionMenuItem {
        icon: Option<(Icon, LayoutBox, [f32; 4])>,
        label: ComponentTextRegion,
        hint: Option<ComponentTextRegion>,
        background: Option<[f32; 4]>,
    },
    TreeView {
        rows: Vec<crate::tree_view::TreeRowGeometry>,
    },
    CommandPalette {
        scrim: LayoutBox,
        surface: LayoutBox,
        title: ComponentTextRegion,
        input: ComponentTextRegion,
        empty: Option<ComponentTextRegion>,
        rows: Vec<crate::command_palette::PaletteRowGeometry>,
        background: [f32; 4],
        input_background: [f32; 4],
        input_border: [f32; 4],
        elevation: ComponentElevation,
    },
    CalendarHeatmap {
        cells: Vec<(LayoutBox, [f32; 4])>,
        labels: Vec<ComponentTextRegion>,
        hover: Option<CalendarHoverGeometry>,
    },
    DonutChart {
        regions: Vec<(LayoutBox, Vec<[f32; 2]>, [f32; 4])>,
        width: f32,
    },
    StackedTimeSeriesChart {
        bars: Vec<(LayoutBox, [f32; 4])>,
        legend: Vec<(LayoutBox, [f32; 4])>,
        grid: Vec<LayoutBox>,
        line: Vec<[f32; 2]>,
        labels: Vec<ComponentTextRegion>,
        marker: Option<LayoutBox>,
        grid_color: [f32; 4],
        line_color: [f32; 4],
    },
    TimeSeriesChart {
        grid: Vec<LayoutBox>,
        area: Vec<LayoutBox>,
        line: Vec<[f32; 2]>,
        grid_color: [f32; 4],
        area_color: [f32; 4],
        line_color: [f32; 4],
    },
    TimestampSeriesChart {
        grid: Vec<LayoutBox>,
        area: Vec<LayoutBox>,
        segments: Vec<Vec<[f32; 2]>>,
        labels: Vec<ComponentTextRegion>,
        grid_color: [f32; 4],
        area_color: [f32; 4],
        line_color: [f32; 4],
    },
    ReorderList {
        rows: Vec<(LayoutBox, ComponentTextRegion, Option<[f32; 4]>)>,
        insert: Option<(LayoutBox, [f32; 4])>,
    },
    NativeMarkdown {
        #[cfg(feature = "rich-text")]
        drawing: crate::MarkdownDrawing,
        text: ComponentTextRegion,
        selection: Vec<LayoutBox>,
        selection_color: [f32; 4],
    },
    SelectableRichText {
        text: ComponentTextRegion,
        selection: Vec<LayoutBox>,
        selection_color: [f32; 4],
    },
    GraphCanvas {
        nodes: Vec<(LayoutBox, ComponentTextRegion, [f32; 4], Option<[f32; 4]>)>,
        separators: Vec<LayoutBox>,
        ports: Vec<(LayoutBox, [f32; 4], [f32; 4], f32)>,
        port_labels: Vec<(ComponentTextRegion, crate::TextHorizontalAlignment)>,
        edges: Vec<(Vec<[f32; 2]>, [f32; 4])>,
        edge_labels: Vec<ComponentTextRegion>,
        grid: Vec<LayoutBox>,
        background: [f32; 4],
        grid_color: [f32; 4],
        separator_color: [f32; 4],
    },
    GraphMinimap {
        nodes: Vec<LayoutBox>,
        node_fill: [f32; 4],
        indicator: Option<LayoutBox>,
        indicator_fill: [f32; 4],
        indicator_border: [f32; 4],
    },
    /// The viewer's painted chrome. Its controls are child nodes
    /// ([`crate::AppContext::assemble_image_viewer`]), not part of this.
    ImageViewer {
        scrim: LayoutBox,
        surface: LayoutBox,
        stage: LayoutBox,
        name: Option<ComponentTextRegion>,
        metadata: Option<ComponentTextRegion>,
        content: LayoutBox,
        scrim_color: [f32; 4],
        surface_color: [f32; 4],
        stage_color: [f32; 4],
    },
    KeyCaptureLayer {
        badge: ComponentTextRegion,
        background: Option<[f32; 4]>,
    },
    KeymapLayer {
        badge: ComponentTextRegion,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectMenuGeometry {
    pub surface: LayoutBox,
    pub elevation: ComponentElevation,
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub options: Vec<SelectOptionGeometry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectOptionGeometry {
    pub bounds: LayoutBox,
    pub label: ComponentTextRegion,
    pub selected: bool,
    pub checked: bool,
    pub disabled: bool,
    pub background: Option<[f32; 4]>,
    pub icon: Option<(Icon, LayoutBox, [f32; 4])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextHorizontalAlignment {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextVerticalAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

impl InteractionStyle {
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct NodeStyle {
    pub layout: Arc<LayoutStyle>,
    pub foreground: Option<SemanticColorRole>,
    pub background: Option<SemanticColorRole>,
    pub border: Option<SemanticColorRole>,
    /// Radius step, resolved against the installed [`nana_ui_core::ThemeMetrics`].
    ///
    /// Same shape as [`Self::background`] one field up: the component names
    /// the design step and the theme decides the number. Writing
    /// `layout.border_radius = Some(UI_METRICS.radius_sm)` instead spends the
    /// token at construction time, and no theme installed afterwards can move
    /// it — which is why the Appearance radius setting could not reach a
    /// control. When this is set it wins over `layout.border_radius`.
    pub radius: Option<nana_ui_core::RadiusTier>,
    /// Per-corner radius steps — top-left, top-right, bottom-right,
    /// bottom-left — resolved against the installed
    /// [`nana_ui_core::ThemeMetrics`] into `layout.paint.border_radii`.
    ///
    /// The four-corner form of [`Self::radius`], for shapes whose corners are
    /// not all the same step. `None` is a square corner: two blocks joined
    /// edge to edge round their outer corners and meet flush, and any radius
    /// at the join — even the smallest step — leaves a notch where the two
    /// curves part. Spending `paint.border_radii` directly
    /// fixes those numbers at construction, so an Appearance radius change
    /// would move every uniform corner in the window and leave these behind.
    /// When set it wins over [`Self::radius`], as `border_radii` already wins
    /// over `border_radius` when the box is painted.
    pub corner_radii: Option<[Option<nana_ui_core::RadiusTier>; 4]>,
    /// Control-size step for this node's own box, resolved against the
    /// installed [`nana_ui_core::ThemeMetrics`]. Sibling of [`Self::radius`].
    ///
    /// `ControlSize::height()` resolves against the compile-time constant, so
    /// a control that called it had already fixed its height before any theme
    /// was installed. Naming the step here keeps the decision open.
    pub control_height: Option<nana_ui_core::ControlHeight>,
    /// Control-size step for this node's horizontal inset, resolved against
    /// the installed [`nana_ui_core::ThemeMetrics`]. Sibling of
    /// [`Self::control_height`] on the other axis, and separate from it
    /// because a node picks the two independently. When set it writes both
    /// `layout.padding_left` and `layout.padding_right`.
    pub control_padding_x: Option<nana_ui_core::ControlPadding>,
    /// Vertical counterpart of [`Self::control_padding_x`]. A multiline field
    /// names `Field` here so the same token can inset the block axis without
    /// also changing a single-line field's line box.
    pub control_padding_y: Option<nana_ui_core::ControlPadding>,
    /// Panel / card inset. Fills unset padding edges from
    /// [`nana_ui_core::ThemeMetrics::panel_padding_x`] / `panel_padding_y`.
    pub surface_padding: Option<nana_ui_core::SurfacePadding>,
    /// Square min box (`min_width` and `min_height`), resolved against the
    /// installed metrics. Icon buttons name [`nana_ui_core::SquareSize::IconButton`]
    /// so a density setting can move them.
    pub square: Option<nana_ui_core::SquareSize>,
    pub interaction: InteractionStyle,
    pub text_horizontal_alignment: TextHorizontalAlignment,
    pub text_vertical_alignment: TextVerticalAlignment,
    /// 节点自绘（Issue #217）。挂上后由 painter 决定这个节点自己的外观，
    /// 子节点照常绘制；见 [`crate::Painter`]。
    pub painter: Option<crate::NodePainter>,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self::visible()
    }
}

/// Field by field, with the layout compared by pointer first: a projection
/// usually hands back the very `Arc` the world holds, and `Arc`'s own
/// equality only takes that shortcut for `Eq` contents, which a layout of
/// floats is not.
impl PartialEq for NodeStyle {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            layout,
            foreground,
            background,
            border,
            radius,
            corner_radii,
            control_height,
            control_padding_x,
            control_padding_y,
            surface_padding,
            square,
            interaction,
            text_horizontal_alignment,
            text_vertical_alignment,
            painter,
        } = self;
        foreground == &other.foreground
            && background == &other.background
            && border == &other.border
            && radius == &other.radius
            && corner_radii == &other.corner_radii
            && control_height == &other.control_height
            && control_padding_x == &other.control_padding_x
            && control_padding_y == &other.control_padding_y
            && surface_padding == &other.surface_padding
            && square == &other.square
            && text_horizontal_alignment == &other.text_horizontal_alignment
            && text_vertical_alignment == &other.text_vertical_alignment
            && interaction == &other.interaction
            && painter == &other.painter
            && (Arc::ptr_eq(layout, &other.layout) || **layout == *other.layout)
    }
}

impl NodeStyle {
    pub fn visible() -> Self {
        Self {
            layout: Arc::clone(&DEFAULT_LAYOUT_STYLE),
            foreground: None,
            background: None,
            border: None,
            radius: None,
            corner_radii: None,
            control_height: None,
            control_padding_x: None,
            control_padding_y: None,
            surface_padding: None,
            square: None,
            interaction: InteractionStyle::default(),
            text_horizontal_alignment: TextHorizontalAlignment::Start,
            text_vertical_alignment: TextVerticalAlignment::Top,
            painter: None,
        }
    }

    /// 挂上节点自绘。见 [`crate::Painter`]。
    pub fn painter(mut self, painter: impl Into<crate::NodePainter>) -> Self {
        self.painter = Some(painter.into());
        self
    }

    /// Name the radius step instead of spending it. See [`Self::radius`].
    pub fn radius(mut self, tier: nana_ui_core::RadiusTier) -> Self {
        self.radius = Some(tier);
        self
    }

    /// Name a radius step per corner. See [`Self::corner_radii`].
    pub fn corner_radii(mut self, tiers: [Option<nana_ui_core::RadiusTier>; 4]) -> Self {
        self.corner_radii = Some(tiers);
        self
    }

    /// Name the control-height step. See [`Self::control_height`].
    pub fn control_height(mut self, height: nana_ui_core::ControlHeight) -> Self {
        self.control_height = Some(height);
        self
    }

    /// Name the horizontal-inset step. See [`Self::control_padding_x`].
    pub fn control_padding_x(mut self, padding: nana_ui_core::ControlPadding) -> Self {
        self.control_padding_x = Some(padding);
        self
    }

    /// Name the vertical-inset step. See [`Self::control_padding_y`].
    pub fn control_padding_y(mut self, padding: nana_ui_core::ControlPadding) -> Self {
        self.control_padding_y = Some(padding);
        self
    }

    /// Name the panel inset. See [`Self::surface_padding`].
    pub fn surface_padding(mut self, padding: nana_ui_core::SurfacePadding) -> Self {
        self.surface_padding = Some(padding);
        self
    }

    /// Name the square min box. See [`Self::square`].
    pub fn square(mut self, size: nana_ui_core::SquareSize) -> Self {
        self.square = Some(size);
        self
    }

    /// Resolve a background mix from the current theme, before interaction overrides.
    pub fn surface_mix(mut self, mix: nana_ui_core::SemanticColorMix) -> Self {
        self.interaction.base.background = None;
        self.interaction.base.background_mix = Some(mix);
        self
    }

    /// Set a theme-relative outline color together with its width.
    pub fn outline_mix(mut self, mix: nana_ui_core::SemanticColorMix, width: f32) -> Self {
        self.interaction.base.border = None;
        self.interaction.base.border_mix = Some(mix);
        Arc::make_mut(&mut self.layout).border_width = Some(width.max(0.0));
        self
    }

    /// 设置语义背景色角色。
    pub fn surface(mut self, role: SemanticColorRole) -> Self {
        self.background = Some(role);
        self.interaction.base.background = None;
        self.interaction.base.background_mix = None;
        self
    }

    /// 一次性写全边框：语义色角色与宽度必须同时给出，缺一边框不会绘制。
    pub fn outline(mut self, role: SemanticColorRole, width: f32) -> Self {
        self.border = Some(role);
        self.interaction.base.border = None;
        self.interaction.base.border_mix = None;
        let layout = Arc::make_mut(&mut self.layout);
        layout.border_width = Some(width.max(0.0));
        self
    }

    /// 设置圆角半径（物理 px）——一次性的显式覆盖，**不**跟随主题。
    ///
    /// 想跟随主题请用 [`Self::radius`] 说出档位。
    pub fn radius_px(mut self, radius: f32) -> Self {
        Arc::make_mut(&mut self.layout).border_radius = Some(radius.max(0.0));
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub foreground: SemanticColorRole,
    pub color: Option<[f32; 4]>,
    pub background: Option<[f32; 4]>,
    pub border_color: Option<[f32; 4]>,
    pub opacity: f32,
    /// CSS `visibility` after inheritance (`visible` / `hidden`).
    pub visibility: nana_ui_core::VisibilitySpec,
    /// Self and every ancestor generate a layout box (`hidden` /
    /// `display: none` or an inactive overlay/menu branch anywhere up the chain
    /// makes this `false`). Unlike CSS
    /// `visibility`, a descendant cannot override it back to `true`.
    pub box_visible: bool,
    pub visible: bool,
    /// CSS `pointer-events` after inheritance (`auto` / `none`).
    pub pointer_events: nana_ui_core::PointerEventsSpec,
    /// CSS `cursor` after inheritance (initial `default`).
    pub cursor: nana_ui_core::CursorSpec,
    /// Whether the cursor value comes from an authored declaration in this
    /// node or one of its ancestors (distinguishes explicit `default` from
    /// the initial value used by editable controls).
    pub cursor_specified: bool,
    /// CSS `user-select` after inheritance (initial `auto`).
    pub user_select: nana_ui_core::UserSelectSpec,
    /// Author `::selection` background after inheritance. `None` uses theme accent.
    pub selection_background: Option<[f32; 4]>,
    /// Author `::selection` foreground after inheritance. `None` keeps text color.
    pub selection_color: Option<[f32; 4]>,
    pub font_size: f32,
    pub font_weight: Option<u16>,
    pub italic: bool,
    pub font_family: Option<Arc<str>>,
    pub line_height: Option<LineHeightSpec>,
    pub letter_spacing: f32,
    pub font_features: Vec<FontFeatureSetting>,
    pub font_variations: Vec<FontVariationSetting>,
    pub font_kerning: FontKerningSpec,
    pub word_break: WordBreakSpec,
    pub line_break: LineBreakSpec,
    /// CSS `direction` after inherit (initial LTR).
    pub direction: nana_ui_core::DirSpec,
    /// CSS `writing-mode` after inherit (initial `horizontal-tb`).
    pub writing_mode: nana_ui_core::WritingModeSpec,
    /// CSS `text-orientation` after inherit (initial `mixed`).
    pub text_orientation: nana_ui_core::TextOrientationSpec,
}

impl ComputedStyle {
    /// The writing mode and direction this node lays out in, after
    /// inheritance. See [`nana_ui_core::WritingContext`].
    pub fn writing_context(&self) -> nana_ui_core::WritingContext {
        nana_ui_core::WritingContext::used(self.writing_mode, self.direction, self.text_orientation)
    }
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            foreground: SemanticColorRole::Text,
            color: None,
            background: None,
            border_color: None,
            opacity: 1.0,
            visibility: nana_ui_core::VisibilitySpec::Visible,
            box_visible: true,
            visible: true,
            pointer_events: nana_ui_core::PointerEventsSpec::Auto,
            cursor: nana_ui_core::CursorSpec::Default,
            cursor_specified: false,
            user_select: nana_ui_core::UserSelectSpec::Auto,
            selection_background: None,
            selection_color: None,
            font_size: UI_BASE_TEXT_SIZE,
            font_weight: None,
            italic: false,
            font_family: None,
            line_height: None,
            letter_spacing: 0.0,
            font_features: Vec::new(),
            font_variations: Vec::new(),
            font_kerning: FontKerningSpec::Auto,
            word_break: WordBreakSpec::Normal,
            line_break: LineBreakSpec::Auto,
            direction: nana_ui_core::DirSpec::Ltr,
            writing_mode: nana_ui_core::WritingModeSpec::HorizontalTb,
            text_orientation: nana_ui_core::TextOrientationSpec::Mixed,
        }
    }
}

/// Text a node shows or a shaper measures.
///
/// The value is a [`TextValue`]: cloning it is a reference count, and one
/// taken from an editor's session carries the stamp that names its bytes, so
/// a host that retained geometry for it recognises it without comparing
/// (Issue #182).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextContent {
    pub value: TextValue,
}

/// Shared, cheaply cloned text; see [`nana_text::SharedText`].
pub type TextValue = nana_text::SharedText;

impl TextContent {
    pub fn new(value: impl Into<TextValue>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// First-line ascent in CSS px (line box top to alphabetic baseline).
    /// `None` = layout falls back to [`nana_ui_core::TEXT_APPROX_ASCENT_EM`].
    pub ascent: Option<f32>,
}

/// Shaped intrinsic text owned by an EmptyState rather than application child
/// nodes. Runtime layout and every renderer consume the same measured runs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct EmptyStateTextPresentation {
    pub title: TextMetrics,
    pub message: Option<TextMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct ModalTextPresentation {
    pub title: TextMetrics,
    pub description: Option<TextMetrics>,
    pub body: Option<TextMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextShapeConstraints {
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub wrap: bool,
    pub ellipsis: bool,
    pub max_lines: Option<u16>,
    pub shaping: TextShaping,
    pub preserve_lines: bool,
    pub wrap_break: nana_ui_core::TextWrapBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextShaping {
    #[default]
    Auto,
    Advanced,
}

/// A pointer hit resolved onto a caret: the UTF-8 boundary, plus which side of
/// it the point fell on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextHit {
    pub offset: usize,
    pub affinity: TextAffinity,
}

impl TextHit {
    /// A hit whose backend cannot tell the two sides of an offset apart.
    pub const fn downstream(offset: usize) -> Self {
        Self {
            offset,
            affinity: TextAffinity::Downstream,
        }
    }
}

pub trait TextShaper {
    /// Run related probes against one immutable text snapshot. Backends may keep
    /// its validated layout resident for the callback; individual probe results
    /// and probes of other text must retain their normal semantics.
    fn with_text_probes<R>(
        &mut self,
        _text: &TextContent,
        _style: &ComputedStyle,
        _constraints: TextShapeConstraints,
        consume: impl FnOnce(&mut dyn TextShaper) -> R,
    ) -> R
    where
        Self: Sized,
    {
        consume(self)
    }

    fn shape(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> TextMetrics;

    /// Whether this node's [`Self::shape`] is answered from a layout the host
    /// already holds for it.
    ///
    /// A content-addressed cache in front of such a node is pure overhead: it
    /// would copy and hash the whole text to find a measurement that is one
    /// read away, on every frame that touches the node -- and an editor's text
    /// is the longest text in the document. Hosts that retain nothing keep
    /// `false` and stay cached.
    fn retains_measurement(&self, _id: StableNodeId) -> bool {
        false
    }

    /// Generation of the font set [`Self::shape`] measures against. A change
    /// makes every metric measured under the old one stale, so text resolved
    /// before it is resolved again. A host whose fonts never change keeps 0.
    fn font_generation(&self) -> u64 {
        0
    }

    /// The `nana-text` engine plain text nodes resolve through, for a host
    /// that draws the retained [`nana_text::TextLayout`]s they then hold.
    /// `None` keeps plain text on [`Self::shape`]. Measuring with one engine
    /// and drawing with another would put two measurement authorities on one
    /// node, so a host returns an engine only if its renderer draws layouts.
    fn text_engine(&self) -> Option<nana_text::SharedTextEngine> {
        None
    }

    /// Text work the host did inside [`Self::shape`] since the last call, for
    /// hosts whose shaper reports it (a `nana-text` engine's cache and layout
    /// counts). The pass counts the nodes themselves.
    fn take_text_work(&mut self) -> nana_text::TextWorkCounters {
        nana_text::TextWorkCounters::default()
    }

    /// Shape using the Runtime-owned [`crate::GlyphCache`].
    ///
    /// The default ignores the cache so hosts without a glyph backend leave
    /// `glyph_cache_*` as `None`. [`MeasureTextShaper`] and `NanaTextShaper`
    /// record per-glyph advances here.
    fn shape_cached(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
        glyphs: &mut crate::GlyphCache,
    ) -> TextMetrics {
        let _ = glyphs;
        self.shape(id, text, style, constraints)
    }

    /// Return the exact horizontal position of a UTF-8 boundary in one
    /// unwrapped line. Backends should override this with their paragraph
    /// engine; the prefix-shaped default keeps lightweight test shapers valid.
    fn horizontal_offset(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        style: &ComputedStyle,
    ) -> f32 {
        if byte_offset > text.value.len() || !text.value.is_char_boundary(byte_offset) {
            return 0.0;
        }
        self.shape(
            id,
            &TextContent {
                value: text.value[..byte_offset].to_owned().into(),
            },
            style,
            TextShapeConstraints {
                shaping: TextShaping::Advanced,
                ..TextShapeConstraints::default()
            },
        )
        .width
    }

    /// Return the visual origin of a UTF-8 boundary in multiline text.
    ///
    /// The backend-neutral fallback handles explicit line breaks and delegates
    /// each line's horizontal shaping to [`Self::horizontal_offset`]. Backends
    /// with paragraph-level wrapping information may override this method.
    fn text_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        style: &ComputedStyle,
        _constraints: TextShapeConstraints,
    ) -> (f32, f32, f32) {
        if byte_offset > text.value.len()
            || !text.value.is_char_boundary(byte_offset)
            || !is_grapheme_boundary(&text.value, byte_offset)
        {
            return (0.0, 0.0, 0.0);
        }
        let (line, line_start, line_end) = explicit_line_at(&text.value, byte_offset);
        let line_height = resolved_text_line_height(style);
        let line_text = TextContent {
            value: text.value[line_start..line_end].to_owned().into(),
        };
        (
            self.horizontal_offset(id, &line_text, byte_offset - line_start, style),
            line as f32 * line_height,
            line_height,
        )
    }

    /// Where a caret with this affinity draws, as [`Self::text_position`]
    /// reports a boundary's origin.
    ///
    /// The default ignores affinity: a backend that does not wrap, or cannot
    /// tell the two sides of a wrap apart, draws one position per offset.
    /// Backends with paragraph geometry override this and answer the side the
    /// caret is on.
    fn text_caret_position(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        byte_offset: usize,
        _affinity: TextAffinity,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> (f32, f32, f32) {
        self.text_position(id, text, byte_offset, style, constraints)
    }

    /// The caret nearest a paragraph-local point, for backends that can answer
    /// it from their layout directly. `None` makes callers search with
    /// [`Self::text_position`] probes instead.
    ///
    /// The hit carries affinity because the point decides it: at a soft wrap
    /// with no hung whitespace the line's end and the next line's start are
    /// one offset, and only the side that was clicked says which.
    fn text_hit_at_point(
        &mut self,
        _id: StableNodeId,
        _text: &TextContent,
        _x: f32,
        _y: f32,
        _style: &ComputedStyle,
        _constraints: TextShapeConstraints,
    ) -> Option<TextHit> {
        None
    }

    /// The caret one position visually left or right of this one, for a
    /// backend that can answer it from its layout.
    ///
    /// Visual order is what an arrow key follows: in `abc ابج` a right arrow
    /// walks the Arabic word from its logical end towards its start, because
    /// that is left to right on screen. It also crosses a soft wrap onto the
    /// neighbouring line, and the two sides of a wrap are one offset with two
    /// affinities, so the answer carries the affinity.
    ///
    /// `None` makes the caller step by grapheme in logical order -- which is
    /// all a backend without wrapping or direction information could mean by
    /// "left" anyway.
    fn text_caret_visual_step(
        &mut self,
        _id: StableNodeId,
        _text: &TextContent,
        _offset: usize,
        _affinity: TextAffinity,
        _rightwards: bool,
        _style: &ComputedStyle,
        _constraints: TextShapeConstraints,
    ) -> Option<TextHit> {
        None
    }

    /// Return highlight rectangles in paragraph-local coordinates.
    ///
    /// The default preserves the backend-neutral explicit-newline behavior.
    /// Paragraph backends should override this to split ranges at their real
    /// visual wrapping boundaries.
    fn text_highlights(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        selection: (usize, usize),
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> Vec<LayoutBox> {
        let (start, end) = selection;
        if start >= end
            || end > text.value.len()
            || !text.value.is_char_boundary(start)
            || !text.value.is_char_boundary(end)
            || !is_grapheme_boundary(&text.value, start)
            || !is_grapheme_boundary(&text.value, end)
        {
            return Vec::new();
        }
        let mut highlights = Vec::new();
        for (line_start, line_end, next_line_start) in explicit_lines(&text.value) {
            let segment_start = start.max(line_start).min(line_end);
            let segment_end = end.min(line_end).max(line_start);
            let includes_line_break =
                next_line_start > line_end && start <= line_end && end >= next_line_start;
            if segment_start < segment_end || includes_line_break {
                let (from_x, from_y, line_height) =
                    self.text_position(id, text, segment_start, style, constraints);
                let (to_x, _, _) = self.text_position(id, text, segment_end, style, constraints);
                highlights.push(LayoutBox {
                    x: from_x,
                    y: from_y,
                    width: (to_x - from_x).max(if includes_line_break { 1.0 } else { 0.0 }),
                    height: line_height,
                });
            }
            if line_end == text.value.len() || line_end >= end {
                break;
            }
        }
        highlights
    }
}

/// Finite-metrics shaper for CPU hosts (framework `text-table`, layout-stop).
/// Wrap height stays against the last content box. Per-character advances go
/// through Runtime [`crate::GlyphCache`] lookup/insert.
#[derive(Debug, Default, Clone, Copy)]
pub struct MeasureTextShaper;

impl TextShaper for MeasureTextShaper {
    fn shape(
        &mut self,
        _id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
    ) -> TextMetrics {
        measure_em_text(text, style, constraints)
    }

    fn shape_cached(
        &mut self,
        _id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: TextShapeConstraints,
        glyphs: &mut crate::GlyphCache,
    ) -> TextMetrics {
        if constraints.preserve_lines {
            return measure_em_text(text, style, constraints);
        }
        let em = style.font_size.max(1.0);
        let mut intrinsic = 0.0;
        for ch in text.value.chars() {
            intrinsic += glyphs.lookup_or_insert(ch, style, em);
        }
        em_metrics(intrinsic, em, constraints)
    }
}

pub(crate) fn measure_em_text(
    text: &TextContent,
    style: &ComputedStyle,
    constraints: TextShapeConstraints,
) -> TextMetrics {
    let em = style.font_size.max(1.0);
    let _ = style.writing_mode;
    if constraints.preserve_lines {
        let line_height = resolved_text_line_height(style).max(em);
        let mut max_w = 0.0f32;
        let mut lines = 0usize;
        let wrap_width = constraints.max_width.filter(|w| *w > 0.0);
        for line in text.value.split('\n') {
            let w = line.chars().count() as f32 * em;
            if constraints.wrap {
                let max = wrap_width.unwrap_or(w).max(em);
                let wrapped = ((w / max).ceil() as usize).max(1);
                lines += wrapped;
                max_w = max_w.max(w.min(max));
            } else {
                lines += 1;
                max_w = max_w.max(w);
            }
        }
        return TextMetrics {
            width: max_w,
            height: line_height * lines.max(1) as f32,
            ascent: Some(em * nana_ui_core::TEXT_APPROX_ASCENT_EM),
        };
    }
    em_metrics(text.value.chars().count() as f32 * em, em, constraints)
}

fn em_metrics(intrinsic: f32, em: f32, constraints: TextShapeConstraints) -> TextMetrics {
    let width = constraints.max_width.unwrap_or(intrinsic).min(intrinsic);
    let height = if constraints.wrap && width + f32::EPSILON < intrinsic {
        em * (intrinsic / width.max(em)).ceil()
    } else {
        em
    };
    TextMetrics {
        width,
        height,
        ascent: Some(em * nana_ui_core::TEXT_APPROX_ASCENT_EM),
    }
}

fn explicit_lines(value: &str) -> Vec<(usize, usize, usize)> {
    use unicode_segmentation::UnicodeSegmentation;

    let mut lines = Vec::new();
    let mut start = 0;
    for (index, grapheme) in value.grapheme_indices(true) {
        if matches!(grapheme, "\n" | "\r" | "\r\n" | "\n\r") {
            lines.push((start, index, index + grapheme.len()));
            start = index + grapheme.len();
        }
    }
    lines.push((start, value.len(), value.len()));
    lines
}

fn explicit_line_at(value: &str, offset: usize) -> (usize, usize, usize) {
    let lines = explicit_lines(value);
    let last_line = lines.len().saturating_sub(1);
    lines
        .into_iter()
        .enumerate()
        .find_map(|(line, (start, end, next))| {
            (offset <= end || offset < next).then_some((line, start, end))
        })
        .unwrap_or((last_line, value.len(), value.len()))
}

pub(crate) fn resolved_text_line_height(style: &ComputedStyle) -> f32 {
    match style.line_height {
        Some(LineHeightSpec::Absolute(value)) => value.max(0.0),
        Some(LineHeightSpec::Relative(value)) => style.font_size * value.max(0.0),
        None => style.font_size * 1.2,
    }
}

/// 诊断标记的可视矩形（文本空间坐标，随行折叠切分）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextDiagnosticMark {
    pub rect: LayoutBox,
    pub severity: TextDiagnosticSeverity,
}

/// 行尾诊断文案（文本空间，overlay，不进缓冲区、不参与光标/换行）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextDiagnosticLabel {
    pub rect: LayoutBox,
    pub text: String,
    pub severity: TextDiagnosticSeverity,
}

/// 诊断悬停命中框（文本空间，整行高的 span 像素 + 行尾文案）。
#[derive(Debug, Clone, PartialEq)]
pub struct TextDiagnosticHit {
    pub rect: LayoutBox,
    pub offset: usize,
    pub message: String,
}

/// 颜色装饰 swatch（文本空间）：锚定在 span 末显示行的行内覆盖方块，
/// 尺寸随行高缩放、垂直居中；颜色按宿主给定的 RGBA 直传。
#[derive(Debug, Clone, PartialEq)]
pub struct TextSwatchMark {
    pub rect: LayoutBox,
    pub color: [f32; 4],
}

/// 多行编辑器的内部派生渲染选项（[`StandardVisual::TextInput`] 携带，
/// 世界侧派生几何）。除括号配对着色默认开启外全部默认关闭；关闭项派生
/// 路径零分配短路。
#[derive(Debug, Clone, PartialEq)]
pub struct TextEditorRenderOptions {
    pub resize_vertical: bool,
    /// 光标处单词/选中文本的出现高亮（内部派生，聚焦时绘制）。
    pub occurrence_highlight: bool,
    /// 相对行号：光标行显示绝对行号，其余行显示与光标所在显示行的距离
    /// （Zed 惯例，见 zed-industries/zed#62311）。仅在行号栏开启时生效。
    pub relative_line_numbers: bool,
    /// 空白字符显示：空格画中点、Tab 画箭头（行首缩进与行尾空白一并可见）。
    pub show_whitespace: bool,
    /// wrap guide 列参考线：在每个字符列位置画全高竖线。列宽按 `'0'`
    /// 字形宽度估算（等宽字体假设）；文档最宽行不足该列时不画。
    pub wrap_guides: Arc<[usize]>,
    /// 代码编辑器 minimap：内容区右缘 64px 覆盖竖条，每逻辑行一条 2px
    /// 行条（宽度 ∝ 行非空白长度），并画当前视口指示器；点击/拖动导航
    /// 视口。仅多行编辑器生效。默认 `false`。
    pub minimap: bool,
    /// sticky scroll：滚动视口顶部落在宿主喂入的折叠区间内部时，在内容
    /// 区顶部钉住显示该区间头行（取首视觉行，软换行只钉第一行）；嵌套
    /// 区间钉最内层。区间头自然滚回视口时钉住行消失。折叠只影响折叠
    /// 语义，不改变钉住派生。仅多行编辑器生效。默认 `false`。
    pub sticky_scroll: bool,
    /// 括号配对着色：配对成功的 `()[]{}` 按嵌套深度循环取主题色阶
    /// （蓝/绿/黄/红/中性灰），未配对括号用淡化前景。渲染为字形变色，
    /// 不改布局。仅多行编辑器生效。默认 `true`。
    pub bracket_pair_colors: bool,
}

impl Default for TextEditorRenderOptions {
    fn default() -> Self {
        Self {
            resize_vertical: false,
            occurrence_highlight: false,
            relative_line_numbers: false,
            show_whitespace: false,
            wrap_guides: Arc::from([]),
            minimap: false,
            sticky_scroll: false,
            bracket_pair_colors: true,
        }
    }
}

/// [`TextInputPresentation::bracket_color_spans`] 中未配对括号的深度哨兵：
/// 渲染层据此使用淡化前景色而非深度色阶。
pub const TEXT_BRACKET_UNMATCHED_DEPTH: usize = usize::MAX;

/// 代码编辑器 sticky scroll 的钉住行几何（节点空间）。纯装饰只读渲染：
/// 不产生命中框，不参与文本编辑。
#[derive(Debug, Clone, PartialEq)]
pub struct TextStickyLineGeometry {
    /// 钉住行背景条（内容区顶部一整行高，接近不透明的编辑器底色，
    /// 覆盖滚动内容之上）。
    pub panel: LayoutBox,
    /// 背景条底缘的 1px 分割线。
    pub divider: LayoutBox,
    /// 钉住行的头行文本（区间头行首个视觉行的全文，超宽由内容区裁剪）。
    pub text: ComponentTextRegion,
    pub background: [f32; 4],
    pub divider_color: [f32; 4],
}

/// minimap 行条的每行纵向节距（2px 一行，条与条相接）。
pub(crate) const TEXT_MINIMAP_BAR_PITCH: f32 = nana_ui_core::space::XXS;
/// minimap 竖条宽度（内容区右缘覆盖条）。
pub(crate) const TEXT_MINIMAP_STRIP_WIDTH: f32 = 64.0;

/// 代码编辑器 minimap 竖条几何（节点空间）。绘制与指针导航共用同一
/// 投影：条内行换算按 `stride` 采样步长与逻辑行数推导。
#[derive(Debug, Clone, PartialEq)]
pub struct TextMinimapGeometry {
    /// 竖条背景（内容区右缘 64px 覆盖条，全内容区高）。
    pub panel: LayoutBox,
    /// 竖条与内容区之间的 1px 分隔线（与行条同色）。
    pub separator: LayoutBox,
    /// 行条（按采样步长抽稀；空白行不产生条）。
    pub bars: Vec<LayoutBox>,
    /// 视口指示器（跟随滚动换算；文档在视口内放得下时 `None`）。
    pub indicator: Option<LayoutBox>,
    pub panel_color: [f32; 4],
    pub bar_color: [f32; 4],
    pub indicator_color: [f32; 4],
    /// 采样步长（行数超出条高容纳量时 >1，步长取整）。
    pub stride: usize,
    /// 逻辑行数（含折叠隐藏行：折叠只影响主视图）。
    pub line_count: usize,
}

impl TextMinimapGeometry {
    /// 条内纵向节距（采样后每逻辑行占用的条高）。
    pub(crate) fn bar_pitch(&self) -> f32 {
        TEXT_MINIMAP_BAR_PITCH / self.stride.max(1) as f32
    }

    /// 条内 y 坐标 → 逻辑行（0 起，钳到文档范围）。不在条内时 `None`。
    pub(crate) fn line_at(&self, y: f32) -> Option<usize> {
        if self.line_count == 0 || y < self.panel.y || y > self.panel.y + self.panel.height {
            return None;
        }
        let offset = (y - self.panel.y).max(0.0);
        let line = (offset / self.bar_pitch()).floor() as usize;
        Some(line.min(self.line_count - 1))
    }
}

/// [`TextWhitespaceMark`] 的种类：空格与 Tab。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWhitespaceKind {
    Space,
    Tab,
}

/// 空白字符显示标记（文本空间字符单元矩形，随滚动平移）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextWhitespaceMark {
    pub rect: LayoutBox,
    pub kind: TextWhitespaceKind,
}

/// 行号标签（文本空间 y 坐标，逻辑行序号从 1 起）。
#[derive(Debug, Clone, PartialEq)]
pub struct LineLabel {
    pub y: f32,
    pub height: f32,
    pub number: u32,
}

/// Shaped editing presentation. The committed value remains in
/// [`TextInputState`]; this derived component only carries renderer geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextInputPresentation {
    /// Full shaped text extent, including visual soft-wrapped rows.
    pub content_size: TextMetrics,
    pub display_value: TextValue,
    pub placeholder: bool,
    pub selection: Option<(f32, f32)>,
    /// 选区条带（文本空间）：主选区与附加光标选区的视觉行矩形合并在同一
    /// 向量里（多光标选区集互不重叠，条带天然不重叠）。
    pub selection_lines: Vec<LayoutBox>,
    pub caret_x: f32,
    pub caret_y: f32,
    pub line_height: f32,
    pub preedit: Option<(f32, f32)>,
    pub preedit_lines: Vec<LayoutBox>,
    /// 附加光标（收起的 additional selection）的文本空间位置；主光标仍在
    /// `caret_x`/`caret_y`。仅多行态计算。
    pub additional_carets: Vec<(f32, f32)>,
    /// 诊断下划线条带（文本空间），仅多行态计算。
    pub diagnostic_marks: Vec<TextDiagnosticMark>,
    /// 行尾诊断文案（文本空间 overlay），仅多行态计算。
    pub diagnostic_labels: Vec<TextDiagnosticLabel>,
    /// 诊断悬停命中框（文本空间），仅多行态计算。
    pub diagnostic_hits: Vec<TextDiagnosticHit>,
    /// 查找匹配高亮条带（文本空间），仅多行态计算。
    pub match_marks: Vec<TextMatchMark>,
    /// 颜色装饰 swatch（文本空间，仅多行态计算；每个 span 取末显示行一个
    /// 覆盖方块）。宿主未喂 span 时为空向量，零分配短路。
    pub swatch_marks: Vec<TextSwatchMark>,
    /// Inline atom chips in text space. Hosts feed [`TextAtomSpan`]; empty
    /// when none are present.
    pub atom_chips: Vec<TextAtomChip>,
    /// 光标相邻括号与其配对端的描边框（文本空间，仅聚焦多行态计算）。
    pub bracket_marks: Vec<LayoutBox>,
    /// 括号配对着色 span：`(start, end, depth)`（字节区间 + 嵌套深度，
    /// 渲染层按深度循环取色）。未配对括号的 depth 为
    /// [`TEXT_BRACKET_UNMATCHED_DEPTH`]（淡化前景）。仅多行且开启
    /// `bracket_pair_colors` 的非占位/非组合态计算，随文本版本 memo。
    pub bracket_color_spans: Arc<[(usize, usize, usize)]>,
    /// 出现高亮条带（文本空间）。主光标的词/单行选中文本在文档内的其他
    /// 出现；仅开启选项且聚焦的多行态计算（不聚焦零成本跳过）。
    pub occurrence_marks: Vec<LayoutBox>,
    /// 空白字符显示标记（文本空间字符单元，仅开启选项的多行态计算）。
    pub whitespace_marks: Vec<TextWhitespaceMark>,
    /// wrap guide 列的文本空间 x 位置（仅多行态计算；文档宽度不足的列
    /// 不出现）。
    pub wrap_guides: Vec<f32>,
    /// 缩进参考线竖线（文本空间，仅多行代码编辑态计算）。
    pub indent_guides: Vec<LayoutBox>,
    /// 各逻辑行的 y 起点（启用行号栏时计算）。
    pub line_tops: Vec<f32>,
    /// 与 `line_tops` 对齐的原始逻辑行号（折叠隐藏行后显示行索引不再
    /// 等于行号；为空时行号 = 索引 + 1）。
    pub line_numbers: Vec<u32>,
    /// 折叠摘要标记（文本空间，存在折叠态区间时计算）。
    pub fold_marks: Vec<TextFoldMark>,
    /// git gutter 标记条带（文本空间，仅多行态计算；宿主未喂标记时为空
    /// 向量，零分配短路）。
    pub git_marks: Vec<TextGitGutterMark>,
    /// 锚定浮层度量（补全弹层行宽缓存 + hover 锚点）。列表指针相等时
    /// 行宽度量整段复用；无浮层时全部为 `None`（零分配）。
    pub overlay_metrics: TextOverlayMetrics,
    /// minimap 行条长度表：每个逻辑行的非空白字符数（原始文档逻辑行，
    /// 含折叠隐藏行；折叠只影响主视图）。仅开启选项的多行态收集，
    /// 关闭时为空向量（零分配短路）。
    pub minimap_line_lengths: Vec<u32>,
}

/// [`TextInputPresentation`] 的浮层度量集合。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextOverlayMetrics {
    /// 补全弹层行宽度量；候选会话不存在时为 `None`。
    pub completion: Option<TextCompletionPopupMetrics>,
    /// hover 锚点（文本空间 `(x, y)`，`offset` 所在行的字形位置）。
    pub hover_anchor: Option<(f32, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutInput {
    pub id: StableNodeId,
    pub parent: Option<StableNodeId>,
    pub children: Arc<Vec<StableNodeId>>,
    pub style: Arc<LayoutStyle>,
    /// The writing mode and direction this node lays out in, after
    /// inheritance.
    pub writing: nana_ui_core::WritingContext,
    /// Its containing block's (its parent's): the frame its own percentage
    /// margins and paddings resolve in.
    pub containing_writing: nana_ui_core::WritingContext,
    pub text_metrics: Option<TextMetrics>,
    pub modal: Option<ModalLayoutInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModalLayoutInput {
    pub kind: crate::ModalSurfaceKind,
    pub slots: crate::ModalSlots,
    pub title: TextMetrics,
    pub description: Option<TextMetrics>,
    pub body_text: Option<TextMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollOffset {
    pub x: f32,
    pub y: f32,
}

/// Derived scrollport and content extents in logical pixels. Absence means
/// the layout backend has not measured this scroll container yet.
///
/// A [`ScrollOffset`] is physical: the content is drawn translated by
/// `-offset`, and `0` shows the scrollport's own box. An axis whose start edge
/// is the right or the bottom (`direction: rtl`, `vertical-rl`'s block axis,
/// `*-reverse` flex) overflows toward the left or the top, so its offsets run
/// negative, from `origin_x` / `origin_y` up to `0` — CSSOM's negative
/// `scrollLeft` / `scrollTop`. Every other axis has an origin of `0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub content_width: f32,
    pub content_height: f32,
    /// Smallest reachable offset on each axis; never positive.
    pub origin_x: f32,
    pub origin_y: f32,
}

/// Exclusive overlay state attached to an overlay host. `active` must be a
/// direct child of the host; `restore_focus` remains in the same document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverlayHostState {
    pub active: Option<StableNodeId>,
    pub restore_focus: Option<StableNodeId>,
}

impl ScrollMetrics {
    /// The scrolling area (CSSOM View) of a scrollport `viewport` whose
    /// descendant boxes span `content` (`[left, top, right, bottom]`, in the
    /// same space; an empty span has `left > right`). Each axis is the
    /// scrollport stretched to the content overflowing past its end edge and
    /// nothing past its start edge — the left / top, or the right / bottom
    /// where `far_start` says the axis starts there. Content on the start
    /// side is unreachable, as in CSS.
    pub fn scrolling_area(viewport: LayoutBox, content: [f32; 4], far_start: [bool; 2]) -> Self {
        let axis = |start: f32, length: f32, low: f32, high: f32, far: bool| {
            if far {
                let extent = length.max(start + length - low);
                (extent, length - extent)
            } else {
                (length.max(high - start).max(0.0), 0.0)
            }
        };
        let (content_width, origin_x) = axis(
            viewport.x,
            viewport.width,
            content[0],
            content[2],
            far_start[0],
        );
        let (content_height, origin_y) = axis(
            viewport.y,
            viewport.height,
            content[1],
            content[3],
            far_start[1],
        );
        Self {
            viewport_width: viewport.width,
            viewport_height: viewport.height,
            content_width,
            content_height,
            origin_x,
            origin_y,
        }
    }

    /// The scroll origin: the offset showing each axis's start edge after
    /// the content overflowing past it. `0` unless the axis starts at the
    /// right / bottom.
    pub fn min_offset(self) -> ScrollOffset {
        ScrollOffset {
            x: self.origin_x,
            y: self.origin_y,
        }
    }

    pub fn max_offset(self) -> ScrollOffset {
        ScrollOffset {
            x: self.origin_x + (self.content_width - self.viewport_width).max(0.0),
            y: self.origin_y + (self.content_height - self.viewport_height).max(0.0),
        }
    }

    pub fn clamp(self, offset: ScrollOffset) -> ScrollOffset {
        let (min, max) = (self.min_offset(), self.max_offset());
        ScrollOffset {
            x: offset.x.clamp(min.x, max.x),
            y: offset.y.clamp(min.y, max.y),
        }
    }
}

impl LayoutBox {
    pub fn contains(self, x: f32, y: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.x
            && y >= self.y
            && x < self.x + self.width
            && y < self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractionState {
    pub pointer_events: bool,
    pub focusable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessibilityRole {
    Document,
    Text,
    Button,
    TextInput,
    Checkbox,
    Switch,
    Slider,
    ComboBox,
    ProgressIndicator,
    List,
    ListItem,
    Table,
    Row,
    Cell,
    ColumnHeader,
    TabList,
    Tab,
    RadioGroup,
    Radio,
    Separator,
    Dialog,
    AlertDialog,
    Menu,
    MenuItem,
    Tooltip,
    Status,
    Toolbar,
    Image,
    Main,
    Navigation,
    Banner,
    ContentInfo,
    Complementary,
    Region,
    Search,
    Form,
    #[default]
    Generic,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccessibilityState {
    pub role: AccessibilityRole,
    pub label: Option<Arc<str>>,
    pub value: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub disabled: bool,
    pub checked: Option<bool>,
    /// Tri-state checkbox in its mixed state. Wins over `checked`.
    pub mixed: bool,
    /// Layout direction of a composite such as a radio group or tab list.
    pub orientation: Option<crate::SelectionOrientation>,
    pub selected: Option<bool>,
    pub multiline: bool,
    pub editable: bool,
    pub modal: bool,
    pub busy: bool,
    pub invalid: bool,
    pub numeric_minimum: Option<f64>,
    pub numeric_maximum: Option<f64>,
    pub numeric_step: Option<f64>,
    pub numeric_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityNode {
    pub id: StableNodeId,
    pub parent: Option<StableNodeId>,
    pub children: Vec<StableNodeId>,
    pub role: AccessibilityRole,
    pub label: Option<Arc<str>>,
    pub value: Option<TextValue>,
    pub description: Option<Arc<str>>,
    pub disabled: bool,
    pub checked: Option<bool>,
    pub mixed: bool,
    pub orientation: Option<crate::SelectionOrientation>,
    pub selected: Option<bool>,
    pub multiline: bool,
    pub editable: bool,
    pub selection: Option<TextSelection>,
    pub modal: bool,
    pub busy: bool,
    pub invalid: bool,
    pub numeric_minimum: Option<f64>,
    pub numeric_maximum: Option<f64>,
    pub numeric_step: Option<f64>,
    pub numeric_value: Option<f64>,
    pub focused: bool,
    pub bounds: LayoutBox,
    /// The writing mode and direction its text reads in (#59): which way an
    /// assistive technology walks a text run.
    pub writing: nana_ui_core::WritingContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityDelta {
    pub generation: u64,
    pub updated: Vec<AccessibilityNode>,
    pub removed: Vec<StableNodeId>,
}

/// One platform-facing accessibility transaction.
///
/// Retained hosts normally emit [`Self::Delta`]. [`Self::Full`] is the bounded
/// fallback when a consumer has not drained changes quickly enough.
#[derive(Debug, Clone, PartialEq)]
pub enum AccessibilityUpdate {
    Full {
        /// `None` is reserved for snapshot-only hosts without retained revisions.
        generation: Option<u64>,
        nodes: Vec<AccessibilityNode>,
    },
    Delta(AccessibilityDelta),
}

/// Backend-neutral action requested by a platform accessibility service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessibilityAction {
    Click,
    Focus,
    SetValue(String),
    SetSelection(TextSelection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityActionRequest {
    pub target: StableNodeId,
    pub action: AccessibilityAction,
}

/// Stable capture/target/bubble route derived from the retained hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRoute {
    /// Ancestors from the document root down to the target's parent.
    pub capture: Vec<StableNodeId>,
    pub target: StableNodeId,
    /// Ancestors from the target's parent back to the document root.
    pub bubble: Vec<StableNodeId>,
}

/// Subscribed DOM/Vue event names. Capture/bubble paths stay on [`EventRoute`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventListeners {
    events: BTreeSet<String>,
}

impl EventListeners {
    pub fn contains(&self, event: &str) -> bool {
        self.events.contains(event)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.events.iter().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub(crate) fn set(&mut self, event: String, enabled: bool) {
        if enabled {
            self.events.insert(event);
        } else {
            self.events.remove(&event);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerCaptureChange {
    pub pointer_id: u64,
    pub target: StableNodeId,
    pub captured: bool,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            pointer_events: true,
            focusable: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeComposition {
    pub text: String,
    pub selection: Option<(usize, usize)>,
}

impl ImeComposition {
    pub fn view(&self) -> ImeView<'_> {
        ImeView {
            text: &self.text,
            selection: self.selection,
        }
    }
}

impl PartialEq<ImeView<'_>> for ImeComposition {
    fn eq(&self, other: &ImeView<'_>) -> bool {
        other == self
    }
}

/// An editor's IME state as the world holds it: the preedit text and the
/// IME's own cursor or selection inside it, in bytes of that text.
///
/// Derived from the editor's [`nana_text::EditSession`]; an IME that is
/// attached with nothing composed reads as an empty `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeView<'a> {
    pub text: &'a str,
    pub selection: Option<(usize, usize)>,
}

impl ImeView<'_> {
    pub fn to_composition(&self) -> ImeComposition {
        ImeComposition {
            text: self.text.to_owned(),
            selection: self.selection,
        }
    }
}

impl PartialEq<ImeComposition> for ImeView<'_> {
    fn eq(&self, other: &ImeComposition) -> bool {
        self.text == other.text && self.selection == other.selection
    }
}

/// An editor's committed text and selections as the world holds them: a
/// read-only view of its [`nana_text::EditSession`], the one storage of that
/// text (Issue #182). Nothing is copied to read it; [`Self::to_state`] makes
/// an owned [`TextInputState`] for code that wants to edit a copy.
#[derive(Debug, Clone, Copy)]
pub struct TextInputView<'a> {
    pub value: &'a str,
    pub selection: TextSelection,
    /// The cursors besides `selection`, in document order.
    pub additional_selections: &'a [TextSelection],
    session: &'a nana_text::EditSession,
}

impl<'a> TextInputView<'a> {
    pub fn of(session: &'a nana_text::EditSession) -> Self {
        Self {
            value: session.as_str(),
            selection: session.selection(),
            additional_selections: session.additional_selections(),
            session,
        }
    }

    pub fn session(&self) -> &'a nana_text::EditSession {
        self.session
    }

    /// The committed text as an O(1) shared copy that names its bytes.
    pub fn value_shared(&self) -> TextValue {
        self.session.snapshot()
    }

    pub fn has_additional_selections(&self) -> bool {
        !self.additional_selections.is_empty()
    }

    /// Every selection in document order, the primary among them.
    pub fn selections(&self) -> std::borrow::Cow<'a, [TextSelection]> {
        self.session.selections()
    }

    pub fn to_state(&self) -> TextInputState {
        TextInputState {
            value: self.session.snapshot(),
            selection: self.selection,
            additional_selections: self.additional_selections.to_vec(),
        }
    }
}

impl PartialEq for TextInputView<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.selection == other.selection
            && self.additional_selections == other.additional_selections
            && (self.value_shared().same_identity(&other.value_shared())
                || self.value == other.value)
    }
}

impl PartialEq<TextInputState> for TextInputView<'_> {
    fn eq(&self, other: &TextInputState) -> bool {
        self.selection == other.selection
            && self.additional_selections == other.additional_selections.as_slice()
            // By identity first: a state holding the session's own buffer
            // compares without reading it.
            && self.value_shared() == other.value
    }
}

impl PartialEq<TextInputView<'_>> for TextInputState {
    fn eq(&self, other: &TextInputView<'_>) -> bool {
        other == self
    }
}

/// A caret (`anchor == focus`) or a selection, in committed-value bytes.
///
/// `affinity` belongs to `focus` — the caret end. One byte offset can be two
/// positions on screen: the end of the line before a soft wrap
/// ([`TextAffinity::Upstream`]) and the start of the line after it
/// ([`TextAffinity::Downstream`]), and either side of a BiDi boundary. Only a
/// pointer hit and visual motion know which one the user meant, so they carry
/// it here; everything that derives a selection from bytes alone
/// (`TextSelection::new`, `TextSelection::caret`, every edit) leaves the
/// default, and text that does not wrap draws both the same. Equality counts
/// the affinity: the same offset on the other side is another place on screen.
///
/// The editor session's own selection type (Issue #182): the value a Runtime
/// editor reports is the one its [`nana_text::EditSession`] holds.
pub type TextSelection = nana_text::EditSelection;

/// Scans the logical line around `offset`, not the whole value.
fn is_grapheme_boundary(value: &str, offset: usize) -> bool {
    nana_text::editable::navigation::is_grapheme_boundary(value, offset)
}

/// `selection` with both ends snapped back onto grapheme boundaries of
/// `value`, the way the editor session snaps what it is handed: a focus the
/// snap moved loses its affinity.
fn snapped_selection(value: &str, selection: TextSelection) -> TextSelection {
    if selection.is_valid_for(value) {
        return selection;
    }
    let snap =
        |offset: usize| nana_text::editable::navigation::snap_to_grapheme(value, offset, false);
    let focus = snap(selection.focus);
    TextSelection {
        anchor: snap(selection.anchor),
        focus,
        affinity: if focus == selection.focus {
            selection.affinity
        } else {
            TextAffinity::Downstream
        },
    }
}

/// Committed editable text and its selection, as components hold it and the
/// world is handed it. The world's editor session is where the text lives
/// (Issue #182) and `value` shares its buffer. IME preedit is not here: it
/// lives in the session only ([`crate::UiWorld::ime`]), so cancelling a
/// composition never touches committed text.
///
/// Beyond the primary [`TextSelection`], a multiline editor can hold
/// `additional_selections` (Zed-style multiple cursors). The invariant set:
/// spans are valid for `value`, sorted by offset, and pairwise disjoint (the
/// full collection is the merge of `selection` and `additional_selections`;
/// [`TextInputState::selections`] reports it). Keep the collection normalized
/// after every edit with [`TextInputState::normalize_selections`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextInputState {
    /// Shared with whatever else holds this text — the world's editor
    /// session among them (Issue #182): cloning a state copies no text, and
    /// the first edit of a shared value is the only copy it makes.
    pub value: TextValue,
    pub selection: TextSelection,
    /// Extra cursors/selections, kept empty for the single-cursor fast path.
    pub additional_selections: Vec<TextSelection>,
}

impl TextInputState {
    pub fn new(value: impl Into<TextValue>) -> Self {
        let value = value.into();
        let selection = TextSelection::caret(value.len());
        Self {
            value,
            selection,
            additional_selections: Vec::new(),
        }
    }

    /// A session holding this state: what the world stores for an editor.
    /// Shares the value's buffer.
    pub fn to_session(&self) -> nana_text::EditSession {
        nana_text::EditSession::with_selections(
            self.value.clone(),
            self.selection,
            self.additional_selections.iter().copied(),
        )
    }

    /// Whether more than one cursor/selection is active. Hot paths check this
    /// first so single-cursor editing and painting stay allocation-free.
    pub fn has_additional_selections(&self) -> bool {
        !self.additional_selections.is_empty()
    }

    /// The complete selection set: [`TextInputState::selection`] plus
    /// [`TextInputState::additional_selections`], sorted by span start with
    /// overlapping/touching spans fused. With one cursor this borrows a
    /// one-element slice instead of allocating.
    pub fn selections(&self) -> std::borrow::Cow<'_, [TextSelection]> {
        if self.additional_selections.is_empty() {
            std::borrow::Cow::Borrowed(std::slice::from_ref(&self.selection))
        } else {
            let (primary, additional) = nana_text::editable::normalize_selections(
                self.selection,
                self.additional_selections.iter().copied(),
            );
            let mut all = Vec::with_capacity(additional.len() + 1);
            all.push(primary);
            all.extend(additional);
            all.sort_by_key(|selection| {
                let range = selection.ordered();
                (range.start, range.end)
            });
            std::borrow::Cow::Owned(all)
        }
    }

    /// Restore the multi-selection invariants — the same ones the editor
    /// session keeps, by the same rule ([`nana_text::editable::normalize_selections`]):
    /// ends off a grapheme boundary snap back onto one, overlapping/touching
    /// spans fuse, and a span that fuses with the primary becomes the primary
    /// (the primary cursor's identity never moves onto another span).
    pub fn normalize_selections(&mut self) {
        self.selection = snapped_selection(&self.value, self.selection);
        if self.additional_selections.is_empty() {
            return;
        }
        let value = &self.value;
        let additional: Vec<TextSelection> = self
            .additional_selections
            .drain(..)
            .map(|selection| snapped_selection(value, selection))
            .collect();
        let (primary, additional) =
            nana_text::editable::normalize_selections(self.selection, additional);
        self.selection = primary;
        self.additional_selections = additional;
    }

    /// Add candidate selections, fusing overlaps/touching spans and dropping
    /// candidates that already exist in the set. Reports whether the set grew.
    pub fn add_selections(&mut self, candidates: &[TextSelection]) -> bool {
        if candidates.is_empty() {
            return false;
        }
        let existing = self.selections();
        let mut added = Vec::with_capacity(candidates.len());
        for &candidate in candidates {
            let candidate = snapped_selection(&self.value, candidate);
            if existing.contains(&candidate) || added.contains(&candidate) {
                continue;
            }
            added.push(candidate);
        }
        if added.is_empty() {
            return false;
        }
        drop(existing);
        let (primary, additional) = nana_text::editable::normalize_selections(
            self.selection,
            self.additional_selections.iter().copied().chain(added),
        );
        self.selection = primary;
        self.additional_selections = additional;
        true
    }

    /// Drop every cursor except the primary selection.
    pub fn collapse_selections(&mut self) {
        self.additional_selections.clear();
    }

    /// Remap `additional_selections` across one committed edit that replaced
    /// `removed` bytes at `start` with `inserted` bytes, then restore the
    /// invariants. Single-cursor states stay untouched.
    fn remap_selections_after_edit(&mut self, start: usize, removed: usize, inserted: usize) {
        if self.additional_selections.is_empty() {
            return;
        }
        let end = start + removed;
        let delta = inserted as isize - removed as isize;
        for selection in &mut self.additional_selections {
            let map = |offset: usize| -> usize {
                if offset <= start {
                    offset
                } else if offset >= end {
                    ((offset as isize + delta).max(start as isize)) as usize
                } else {
                    // Inside the replaced span: collapse onto the edit point.
                    start + inserted
                }
            };
            let focus = map(selection.focus);
            // 偏移被编辑挪动过，它当初解析在软换行哪一侧就不再作数。
            if focus != selection.focus {
                selection.affinity = TextAffinity::Downstream;
            }
            selection.anchor = map(selection.anchor);
            selection.focus = focus;
        }
    }

    /// Replace the text of every selection with `text` (one cursor replaces
    /// its own selection; multiple cursors each receive an insertion) and
    /// park every cursor at the end of its inserted text.
    pub fn replace_selection(&mut self, text: &str) -> bool {
        if !self.selection.is_valid_for(&self.value) {
            return false;
        }
        if !self.has_additional_selections() {
            let range = self.selection.ordered();
            let caret = range.start + text.len();
            self.value.replace_range(range.clone(), text);
            self.selection = TextSelection::caret(caret);
            return true;
        }
        // Multi-cursor: one insertion per selection against the pre-edit
        // value, spliced in a single pass.
        let selections = self.selections();
        let primary_index = selections
            .iter()
            .position(|selection| *selection == self.selection)
            .unwrap_or(0);
        let edits: Vec<(TextSelection, Option<crate::text_editing::CursorEdit>)> = selections
            .iter()
            .map(|&selection| {
                let edit = if selection.is_valid_for(&self.value) {
                    let range = selection.ordered();
                    let caret = range.start + text.len();
                    Some(crate::text_editing::CursorEdit::Span(
                        crate::text_editing::TextReplacement {
                            range,
                            insert: text.to_owned(),
                            caret,
                        },
                    ))
                } else {
                    None
                };
                (selection, edit)
            })
            .collect();
        let Some((next_value, next_selections)) =
            crate::text_editing::apply_cursor_edits(&self.value, &edits)
        else {
            return false;
        };
        self.value = next_value.into();
        self.selection = next_selections
            .get(primary_index)
            .copied()
            .unwrap_or_else(|| TextSelection::caret(self.value.len()));
        self.additional_selections = next_selections
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != primary_index)
            .map(|(_, selection)| *selection)
            .collect();
        self.normalize_selections();
        true
    }

    /// Replace only the primary selection's text (IME commit path); every
    /// other cursor survives the edit through offset remapping. This is the
    /// documented multi-cursor IME restriction: composition commits to the
    /// primary cursor alone.
    pub fn replace_primary_selection(&mut self, text: &str) -> bool {
        if !self.selection.is_valid_for(&self.value) {
            return false;
        }
        let range = self.selection.ordered();
        let caret = range.start + text.len();
        self.value.replace_range(range.clone(), text);
        self.selection = TextSelection::caret(caret);
        self.remap_selections_after_edit(range.start, range.len(), text.len());
        self.normalize_selections();
        true
    }

    /// Delete UTF-8 bytes before and after the current selection, and the
    /// selection with them.
    ///
    /// IME preedit is stored separately and is not touched. Returns false when
    /// the selection is invalid, the span is empty, the range would overflow,
    /// or either end is not a character boundary.
    pub fn delete_surrounding(&mut self, before_bytes: usize, after_bytes: usize) -> bool {
        self.delete_ime_surrounding(before_bytes, after_bytes, false)
    }

    /// [`Self::delete_surrounding`] as an IME asks for it. While `composing`,
    /// the selection is the text the preedit stands in for: it is kept, and
    /// only the bytes around it go, so cancelling the composition still finds
    /// it. The rule is `nana-text`'s ([`nana_text::editable::ime`]).
    pub fn delete_ime_surrounding(
        &mut self,
        before_bytes: usize,
        after_bytes: usize,
        composing: bool,
    ) -> bool {
        if !self.selection.is_valid_for(&self.value) {
            return false;
        }
        let anchor = self.selection.ordered();
        let Some(deletion) = nana_text::editable::ime::surrounding_deletion(
            &self.value,
            anchor.clone(),
            before_bytes,
            after_bytes,
            composing,
        ) else {
            return false;
        };
        // The later range first, so the earlier one's offsets still hold.
        for range in [deletion.after.clone(), deletion.before.clone()] {
            if range.is_empty() {
                continue;
            }
            self.value.replace_range(range.clone(), "");
            self.remap_selections_after_edit(range.start, range.len(), 0);
        }
        self.selection = if composing {
            TextSelection::new(
                deletion.map_offset(anchor.start),
                deletion.map_offset(anchor.end),
            )
        } else {
            TextSelection::caret(deletion.before.start)
        };
        self.normalize_selections();
        true
    }

    /// Replace a controlled value while keeping a valid selection when
    /// possible. If the old offsets no longer land on UTF-8 boundaries, move
    /// the caret to the new end. A wholesale value replacement also drops any
    /// additional cursors: their offsets have no meaning in foreign text.
    pub fn replace_value(&mut self, value: impl Into<TextValue>) {
        self.value = value.into();
        if !self.selection.is_valid_for(&self.value) {
            self.selection = TextSelection::caret(self.value.len());
        }
        self.additional_selections.clear();
    }

    /// Accept a complete value snapshot from a native editor.
    ///
    /// Native controlled editors commonly report the resulting value without
    /// exposing their internal selection. Infer the caret from the minimal
    /// changed span so retained text, accessibility, and application events
    /// commit one value without forcing every edit to the end.
    pub fn synchronize_editor_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        if self.value == value {
            return;
        }
        let prefix = self
            .value
            .chars()
            .zip(value.chars())
            .take_while(|(current, next)| current == next)
            .map(|(character, _)| character.len_utf8())
            .sum::<usize>();
        let suffix = self.value[prefix..]
            .chars()
            .rev()
            .zip(value[prefix..].chars().rev())
            .take_while(|(current, next)| current == next)
            .map(|(_, character)| character.len_utf8())
            .sum::<usize>();
        let caret = value.len() - suffix;
        self.value = value.into();
        self.selection = TextSelection::caret(caret);
        // Native editor snapshots carry a single selection; drop the rest.
        self.additional_selections.clear();
    }
}

/// Backend-neutral custom render content attached to one retained UI node.
///
/// First-class layout/Scene citizen: the same clip, z-index, hit-test, and
/// document order as Quad/Text. `renderer` selects an installed renderer
/// extension and `resource` is an opaque application-owned lookup key. Neither
/// field exposes a GPU backend object, so the same extraction can be consumed
/// by WGPU or a future RHI.
///
/// [`Self::fit`] is presentation-only. Host-texture sampling uses it to choose
/// a destination rect; other renderers ignore Fill.
///
/// [`Self::params`] carries per-node presentation values whose meaning is
/// defined by `renderer` alone. Runtime never interprets them, so a renderer
/// can reach its own painter without Runtime learning a shading model.
/// [`Self::dedicated_pass`] asks the painter to open a pass for this node
/// instead of joining the current one.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomRenderNode {
    pub renderer: Arc<str>,
    pub resource: Arc<str>,
    pub revision: u64,
    pub fit: nana_ui_core::ContentFit,
    pub params: Option<Arc<[f32]>>,
    pub dedicated_pass: bool,
    /// 在纹理下方绘制 alpha 棋盘底（host texture 渲染器契约）。
    pub checkerboard: bool,
    /// 纹理缩放系数，1.0 为适配原始大小（host texture 渲染器契约）。
    pub zoom: f32,
    /// 采样方式（host texture 渲染器契约）：默认单层双线性，宿主按
    /// `painted_extent` 准备尺寸；[`nana_ui_core::ImageSampling::Mipmap`]
    /// 对宿主提供的 mip 链做三线性采样。
    pub sampling: nana_ui_core::ImageSampling,
}

impl CustomRenderNode {
    pub fn new(
        renderer: impl Into<Arc<str>>,
        resource: impl Into<Arc<str>>,
        revision: u64,
    ) -> Self {
        Self {
            renderer: renderer.into(),
            resource: resource.into(),
            revision,
            fit: nana_ui_core::ContentFit::Fill,
            params: None,
            dedicated_pass: false,
            checkerboard: false,
            zoom: 1.0,
            sampling: nana_ui_core::ImageSampling::Resample,
        }
    }

    pub const fn with_fit(mut self, fit: nana_ui_core::ContentFit) -> Self {
        self.fit = fit;
        self
    }

    /// 选择采样方式，见 [`nana_ui_core::ImageSampling`]。
    pub const fn with_sampling(mut self, sampling: nana_ui_core::ImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// 在纹理下方绘制 alpha 棋盘底。
    pub const fn with_checkerboard(mut self, checkerboard: bool) -> Self {
        self.checkerboard = checkerboard;
        self
    }

    /// 设置纹理缩放系数（非有限值忽略，保持原值语义为 1.0 由调用方保证）。
    pub const fn with_zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }

    /// Attaches renderer-defined presentation values. Non-finite entries become
    /// zero so a painter never uploads NaN into a uniform.
    pub fn with_params(mut self, params: impl IntoIterator<Item = f32>) -> Self {
        self.params = Some(
            params
                .into_iter()
                .map(|value| if value.is_finite() { value } else { 0.0 })
                .collect(),
        );
        self
    }

    /// Requests a dedicated render pass for this node.
    pub const fn with_dedicated_pass(mut self, dedicated_pass: bool) -> Self {
        self.dedicated_pass = dedicated_pass;
        self
    }

    /// Renderer-defined parameter at `index`, or `None` when absent.
    pub fn param(&self, index: usize) -> Option<f32> {
        self.params
            .as_ref()
            .and_then(|params| params.get(index))
            .copied()
    }
}

/// One document-level text selection. Not a second TextInput.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentTextSelection {
    pub node: StableNodeId,
    pub start: usize,
    pub end: usize,
    pub lines: Vec<LayoutBox>,
}

impl DocumentTextSelection {
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn ordered(&self) -> (usize, usize) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedNode {
    pub id: StableNodeId,
    /// Shared with `UiWorld` so idle extract is an Arc bump, not a `NodeKind` clone.
    pub kind: Arc<NodeKind>,
    pub parent: Option<StableNodeId>,
    /// Shared with retained hierarchy. Idle extract does not clone the child list.
    pub children: Arc<Vec<StableNodeId>>,
    pub layout: LayoutBox,
    pub scroll_offset: ScrollOffset,
    pub source_style: NodeStyle,
    /// Shared with `UiWorld` resolved style. Copy-on-write when a dirty node mutates.
    pub style: Arc<ComputedStyle>,
    pub text: Option<TextContent>,
    pub text_metrics: Option<TextMetrics>,
    /// The `nana-text` layout `text_metrics` were read from, when a plain text
    /// node resolved through an engine (Issue #95). `None` for text measured by
    /// the host shaper, which a renderer still lays out itself.
    pub text_layout: Option<crate::RetainedTextLayout>,
    /// Whether an authored newline in [`Self::text`] is a line break rather
    /// than a space, as the constraints this node was measured with say.
    ///
    /// Carried rather than re-derived: a renderer that lays this text out
    /// again has to reach the same answer, and `white-space` plus "is this a
    /// multiline editor" is not something the scene can see.
    pub text_preserve_lines: bool,
    pub z_index: i32,
    pub focused: bool,
    /// Whether the node is a text editor: its text is shaped as one and
    /// drawn from the editor presentation, not as plain text.
    pub editable: bool,
    pub text_spans: Vec<ExtractedTextSpan>,
    pub standard_visual: Option<StandardVisual>,
    /// Boxed: the `TextInput` variant alone is ~1.8 KB, which would otherwise
    /// dominate `ExtractedNode` and be memcpy'd for every extracted node.
    pub component_geometry: Option<Box<ComponentGeometry>>,
    pub standard_visual_foreground: Option<[f32; 4]>,
    /// Radius steps resolved against the installed theme, for the framework
    /// chrome the Scene paints around this node's visual.
    ///
    /// Sibling of [`Self::standard_visual_foreground`] one field up, for the
    /// same reason: a value the theme decided, carried to the renderer so the
    /// renderer does not go looking for it. Before this the Scene read the
    /// `UI_METRICS` constant and a themed radius could not reach menu
    /// surfaces, modal frames or palette rows.
    pub chrome_radii: nana_ui_core::ChromeRadii,
    pub custom_render: Option<CustomRenderNode>,
    /// File-drop hover overlay resolved from the theme. Scene paints this as a
    /// fill + border on the target box; it is not the node's own background.
    pub drop_hover: Option<DropHoverOverlay>,
    /// Document-level text selection highlights in paragraph-local coordinates.
    pub document_text_selection: Vec<LayoutBox>,
    /// Fill for [`Self::document_text_selection`]: author `::selection` background,
    /// else theme `accent_soft`.
    pub document_text_selection_color: [f32; 4],
    /// Compositor-class overlay bindings and optional advanced layer request.
    /// Logical style stays on [`Self::source_style`]; presentation is not baked in.
    pub compositor: ExtractedCompositor,
    /// What the node's [`crate::Painter`] recorded, resolved against the
    /// installed theme (Issue #217). `None` for every node without a painter,
    /// which keeps the built-in paint path.
    pub custom_paint: Option<Arc<crate::PaintRecording>>,
}

/// Scene layer promotion hint produced at extract. Empty for layout/paint tracks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtractedCompositor {
    /// Winning compositor-class overlay tracks at extract time.
    pub bindings: Vec<nana_ui_core::motion::MotionTrackId>,
    /// Advanced API: request a layer even without an overlay.
    pub request_layer: bool,
}

/// Theme-resolved drop-target hover overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropHoverOverlay {
    pub fill: [f32; 4],
    pub border: [f32; 4],
}

/// Theme-resolved committed-text span ready for Scene paint.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedTextSpan {
    pub start: usize,
    pub end: usize,
    pub color: [f32; 4],
}

impl StandardVisual {
    /// Optional component family owning this visual; base geometry has no feature.
    pub fn required_feature(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "calendar")]
            Self::CalendarHeatmap { .. } => Some("calendar"),
            #[cfg(feature = "charts")]
            Self::TimeSeriesChart { .. }
            | Self::TimestampSeriesChart { .. }
            | Self::DonutChart { .. }
            | Self::StackedTimeSeriesChart { .. } => Some("charts"),
            #[cfg(feature = "controls")]
            Self::ReorderList { .. } => Some("controls"),
            #[cfg(feature = "rich-text")]
            Self::NativeMarkdown { .. } => Some("rich-text"),
            #[cfg(feature = "rich-text")]
            Self::SelectableRichText { .. } => Some("rich-text"),
            #[cfg(feature = "graph-canvas")]
            Self::GraphCanvas { .. } => Some("graph-canvas"),
            #[cfg(feature = "graph-canvas")]
            Self::GraphMinimap { .. } => Some("graph-canvas"),
            #[cfg(feature = "image-viewer")]
            Self::ImageViewer { .. } => Some("image-viewer"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TextAffinity, TextInputState, TextSelection};

    #[test]
    fn affinity_is_part_of_where_a_caret_is() {
        // 同一偏移的两侧是屏幕上的两个位置，所以它们不是同一个选区——
        // 否则换一侧不会标脏，caret 不会重画。
        let downstream = TextSelection::caret(3);
        let upstream = downstream.with_affinity(TextAffinity::Upstream);
        assert_eq!(downstream.affinity, TextAffinity::Downstream);
        assert_ne!(downstream, upstream);
        assert_eq!(upstream.ordered(), downstream.ordered());
        assert!(upstream.is_valid_for("abcdef"));
    }

    #[test]
    fn an_edit_that_moves_an_extra_cursor_drops_its_affinity() {
        let mut multi = state(
            "abcdef",
            TextSelection::caret(0),
            vec![
                TextSelection::caret(1).with_affinity(TextAffinity::Upstream),
                TextSelection::caret(5).with_affinity(TextAffinity::Upstream),
            ],
        );
        // 在 3 处插一个字节：1 不动，5 右移——右移的那个当初解析在软换行
        // 哪一侧不再作数。
        multi.remap_selections_after_edit(3, 0, 1);
        assert_eq!(
            multi.additional_selections,
            vec![
                TextSelection::caret(1).with_affinity(TextAffinity::Upstream),
                TextSelection::caret(6),
            ]
        );
    }

    fn state(
        value: &str,
        selection: TextSelection,
        additional_selections: Vec<TextSelection>,
    ) -> TextInputState {
        TextInputState {
            value: value.into(),
            selection,
            additional_selections,
        }
    }

    #[test]
    fn selections_reports_the_sorted_set_and_borrows_a_single_cursor() {
        let single = state("abcd", TextSelection::caret(2), Vec::new());
        assert!(matches!(single.selections(), std::borrow::Cow::Borrowed(_)));
        assert_eq!(single.selections().len(), 1);

        // 附加光标按 offset 排序；主光标保持自身身份（不必排在最前）。
        let multi = state(
            "abcd",
            TextSelection::new(3, 4),
            vec![TextSelection::caret(0)],
        );
        assert_eq!(
            multi.selections().into_owned(),
            vec![TextSelection::caret(0), TextSelection::new(3, 4)]
        );
        assert_eq!(multi.selection, TextSelection::new(3, 4));
    }

    #[test]
    fn normalize_fuses_touching_spans_into_the_primary() {
        let mut multi = state(
            "abcdef",
            TextSelection::new(2, 3),
            vec![
                TextSelection::new(3, 5),
                TextSelection::caret(5),
                TextSelection::caret(0),
            ],
        );
        multi.normalize_selections();
        // 主光标吸收与其相接的 span（身份不转移），其余保持附加集合。
        // caret(5) 与 [2,5) 相接但为空跨度，不延长并集。
        assert_eq!(multi.selection, TextSelection::new(2, 5));
        assert_eq!(multi.additional_selections, vec![TextSelection::caret(0)]);
    }

    #[test]
    fn normalize_clamps_invalid_offsets_onto_boundaries() {
        let value = "ab\u{1F600}cd";
        let mut multi = state(
            value,
            TextSelection::caret(1),
            vec![TextSelection::caret(3)],
        );
        multi.normalize_selections();
        // 偏移 3 落在 emoji 中间，收敛到最近的字符边界 2。
        assert_eq!(multi.additional_selections, vec![TextSelection::caret(2)]);
    }

    #[test]
    fn replace_selection_splices_every_cursor_in_one_pass() {
        let mut multi = state(
            "ab_cd",
            TextSelection::caret(2),
            vec![TextSelection::caret(5)],
        );
        assert!(multi.replace_selection("X"));
        assert_eq!(multi.value, "abX_cdX");
        assert_eq!(multi.selection, TextSelection::caret(3));
        assert_eq!(multi.additional_selections, vec![TextSelection::caret(7)]);

        // 相邻光标会先合并，只插入一次。
        let mut touching = state(
            "abc",
            TextSelection::caret(1),
            vec![TextSelection::caret(1)],
        );
        assert!(touching.replace_selection("X"));
        assert_eq!(touching.value, "aXbc");
        assert!(touching.additional_selections.is_empty());
    }

    #[test]
    fn replace_primary_selection_scopes_ime_commits_and_remaps_others() {
        let mut multi = state(
            "ab_cd",
            TextSelection::caret(2),
            vec![TextSelection::caret(5)],
        );
        assert!(multi.replace_primary_selection("X"));
        assert_eq!(multi.value, "abX_cd");
        assert_eq!(multi.selection, TextSelection::caret(3));
        // 附加光标随编辑平移，仍然有效。
        assert_eq!(multi.additional_selections, vec![TextSelection::caret(6)]);
    }

    #[test]
    fn add_selections_skips_duplicates_and_collapses_back() {
        let mut multi = state("abcdef", TextSelection::caret(0), Vec::new());
        assert!(multi.add_selections(&[
            TextSelection::caret(3),
            TextSelection::caret(3),
            TextSelection::caret(5),
        ]));
        assert_eq!(
            multi.additional_selections,
            vec![TextSelection::caret(3), TextSelection::caret(5)]
        );
        // 已有光标处不再添加。
        assert!(!multi.add_selections(&[TextSelection::caret(5)]));
        multi.collapse_selections();
        assert!(multi.additional_selections.is_empty());
        assert_eq!(multi.selection, TextSelection::caret(0));
    }

    #[test]
    fn wholesale_value_replacements_drop_additional_cursors() {
        let mut multi = state(
            "abcd",
            TextSelection::caret(1),
            vec![TextSelection::caret(3)],
        );
        multi.replace_value("xy");
        assert!(multi.additional_selections.is_empty());

        let mut multi = state(
            "abcd",
            TextSelection::caret(1),
            vec![TextSelection::caret(4)],
        );
        multi.synchronize_editor_value("abcdefg");
        assert!(multi.additional_selections.is_empty());
        // 最小变更区间的末端落在新值的插入点。
        assert_eq!(multi.selection, TextSelection::caret(7));
    }
}
