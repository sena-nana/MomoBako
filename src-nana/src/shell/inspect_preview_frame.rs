//! 文件预览页的外框，对齐 Vue `FilePreviewPane` 和 `.files-preview-page`。
//!
//! bg-elev 圆角面板，自上而下：页头（返回、眉题和文件名、打开与定位）、正文两栏（左边预览框加色板，
//! 右边 300px 的文件事实和元数据卡），最底下贴着播放条。正文在窗口不宽于 900px 时上下排。
//!
//! 常驻：外框、页头和底部播放条的位置只建一次，文件名、路径、导语和按钮能不能点按字段绑定，
//! 按钮的处理器现读信号里的目录和绝对路径。正文按「排法 + 预览目标」换一整块（滚动区回到顶部）；
//! 同一个目标里，预览框外框常驻，框里的内容按身份换（见 `inspect_preview_body.rs`），
//! 事实和元数据按字段绑定。

use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{dynamic, signal, untrack, widget, AnyView, IntoView, NodeRef, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, LengthSpec, NodeStyle, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack, Text,
};
use nana_ui::{ButtonKind, Icon};

use super::super::files_view::bind::{fill_container, StyleField};
use super::super::inspect_asmr::{self, AsmrPanels};
use super::super::inspect_library::{self, FactsView};
use super::super::inspect_metadata_view::{self, MetadataSignals};
use super::super::player_view::icons;
use super::super::{ShellMessage, ShellViewModel};
use super::body::{self, PreviewSnapshot};

/// Vue `@media (max-width: 900px)`：正文和页头改成单列。
const STACK_BREAKPOINT: f32 = 900.0;
/// 右侧文件事实卡宽度，对应 `--workspace-file-detail-width`。
const DETAIL_WIDTH: f32 = 300.0;
/// 正文两栏间距，对应 `--workspace-file-detail-gap`。
const DETAIL_GAP: f32 = 18.0;

/// 页头：排法、文件名和路径、注释和链接导语，以及三个按钮能不能点、点了去哪。
#[derive(Clone, Debug, Default, PartialEq)]
struct HeadView {
    stacked: bool,
    /// 去掉扩展名的文件名。
    name: String,
    /// 和文件名不同时的完整路径，否则为空。
    path: String,
    comment: String,
    link: String,
    /// 没有预览目标：三个按钮都点不了。
    empty: bool,
    can_open: bool,
    can_reveal: bool,
    /// 「返回」回到的目录。
    directory: String,
    has_repo: bool,
    /// 目标的绝对路径，打开和定位用。
    absolute: String,
}

/// 正文的整块身份：在不在预览、排法和预览目标（滚动区键里的仓库和路径）。
/// 不在预览时预览页藏着，正文不建，免得为藏着的页面解码、编码预览内容。
#[derive(Clone, Debug, PartialEq)]
struct MainKey {
    previewing: bool,
    stacked: bool,
    scope: String,
}

/// 预览页的常驻信号。元数据编辑区的信号在 [`MetadataSignals`]，和文件详情共用。
#[derive(Clone, Copy)]
pub(crate) struct PreviewSignals {
    radius: Signal<f32>,
    head: Signal<HeadView>,
    main: Signal<MainKey>,
    content: Signal<PreviewSnapshot>,
    palette: Signal<Vec<String>>,
    panels: Signal<Option<AsmrPanels>>,
    facts: Signal<FactsView>,
}

impl PreviewSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        Self {
            radius: signal(radius(model)),
            head: signal(head_view(model)),
            main: signal(main_key(model)),
            content: signal(PreviewSnapshot::project(model, None)),
            palette: signal(model.inspect.palette.clone()),
            panels: signal(panels(model)),
            facts: signal(FactsView::of(&model.inspect.facts)),
        }
    }

    /// 写入投影，只写变了的；预览框内容的身份没变时不抄大块数据。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        self.radius.try_set_if_changed(radius(model));
        self.head.try_set_if_changed(head_view(model));
        self.main.try_set_if_changed(main_key(model));
        if self.content.defined_at().is_some() {
            let snapshot = self.content.with_untracked(|previous| PreviewSnapshot::project(model, Some(previous)));
            self.content.set_if_changed(snapshot);
        }
        self.palette.try_set_if_changed(model.inspect.palette.clone());
        self.panels.try_set_if_changed(panels(model));
        self.facts.try_set_if_changed(FactsView::of(&model.inspect.facts));
    }
}

/// ASMR 队列和播放列表面板：只在预览页打开时投影，藏着的预览页不建它们。
fn panels(model: &ShellViewModel) -> Option<AsmrPanels> {
    super::super::files_view::previewing(model).then(|| inspect_asmr::preview_panels(model)).flatten()
}

fn radius(model: &ShellViewModel) -> f32 {
    crate::theme_map::radius_2xl(model.admin.corner_radius as f32)
}

fn main_key(model: &ShellViewModel) -> MainKey {
    let previewing = super::super::files_view::previewing(model);
    let scope = if previewing { body::scope(model) } else { String::new() };
    MainKey { previewing, stacked: model.viewport_width <= STACK_BREAKPOINT, scope }
}

fn head_view(model: &ShellViewModel) -> HeadView {
    let path = model.inspect.target_path.clone().unwrap_or_default().trim().to_string();
    let name = file_name(&path);
    let title = super::super::player_view::bar::display_title(&name, &extension_of(&name));
    let ctx = super::super::files::FileContext::from_model(model);
    let repository = model.workspace.active_repository();
    let root = repository.map(|item| item.path.clone()).unwrap_or_default();
    let absolute = super::super::input::repository_absolute(&root, &path);
    let can_open = !path.is_empty() && !absolute.trim().is_empty();
    HeadView {
        stacked: model.viewport_width <= STACK_BREAKPOINT,
        name: title,
        path: if !path.is_empty() && path != name { path.clone() } else { String::new() },
        comment: model.inspect.draft_comment().trim().to_string(),
        link: model.inspect.draft_link().trim().to_string(),
        empty: path.is_empty(),
        can_open,
        can_reveal: can_open && !ctx.trash,
        directory: model.files.current_path.clone(),
        has_repo: repository.is_some(),
        absolute,
    }
}

/// 预览页。`bar` 是页底播放条的占位节点，里面的播放条是旧视图岛。
pub(super) fn preview_page(signals: PreviewSignals, metadata: MetadataSignals, bar: NodeRef) -> AnyView {
    let radius = signals.radius;
    let main = dynamic(signals.main, move |key: &MainKey| main(key, signals, metadata)).css(fill_container());
    widget(Stack::column(0.0).style(page_style(radius.get_untracked())))
        .prop::<NodeStyle, StyleField>(move || page_style(radius.get()))
        .children((header(signals.head), main, widget(Stack::column(0.0)).node_ref(bar).key("files-island-preview-bar")))
        .key("inspect-preview-page")
        .into_any()
}

/// 外框：bg-elev 圆角面板，裁掉溢出，四边留 1px。
fn page_style(radius: f32) -> NodeStyle {
    Stack::fill_column(0.0)
        .surface(SemanticColorRole::Surface)
        .radius_px(radius)
        .min_height(LengthSpec::Px(0.0))
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            layout.padding_top = Some(LengthSpec::Px(1.0));
            layout.padding_left = Some(LengthSpec::Px(1.0));
            layout.padding_right = Some(LengthSpec::Px(1.0));
            layout.padding_bottom = Some(LengthSpec::Px(1.0));
        })
        .node_style()
}

/// 页头：返回在左，眉题、文件名和路径在中，打开、定位在右。底边一条 border-soft。
fn header(head: Signal<HeadView>) -> AnyView {
    let current = head.get_untracked();
    let back_disabled = move || head.with(|view| view.empty);
    let open_disabled = move || head.with(|view| !view.can_open);
    let reveal_disabled = move || head.with(|view| !view.can_reveal);
    let back = action(("返回", icons::ARROW_LEFT, 15.0, 34.0), "inspect-back", back_disabled)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::OpenDirectory(head.with_untracked(|view| view.directory.clone()))));
    let open = action(("打开", icons::EYE, 14.0, 32.0), "inspect-open", open_disabled).on_cx(move |_, _: &Activate, cx| {
        let (has_repo, absolute_path) = head.with_untracked(|view| (view.has_repo, view.absolute.clone()));
        cx.dispatch_program_all(ShellMessage::Input(super::super::input::InputMessage::OpenEntry { has_repo, absolute_path }));
    });
    let reveal = action(("定位", icons::FOLDER_OPEN, 14.0, 32.0), "inspect-reveal", reveal_disabled).on_cx(move |_, _: &Activate, cx| {
        let absolute_path = head.with_untracked(|view| view.absolute.clone());
        cx.dispatch_program_all(ShellMessage::Input(super::super::input::InputMessage::RevealEntry { absolute_path }));
    });
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).key("inspect-preview-actions").children((open, reveal));
    let titles = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children(title_lines(head))
        .key("inspect-preview-titles");
    let back_cell = widget(Stack::row(0.0).grow(0.0).shrink(0.0)).children((back,)).key("inspect-back-cell");
    widget(Stack::column(0.0).style(header_style(current.stacked)))
        .prop::<NodeStyle, StyleField>(move || header_style(head.with(|view| view.stacked)))
        .children((back_cell, titles, actions))
        .key("inspect-preview-header")
        .into_any()
}

/// 页头的排法：窄窗口上下排，宽窗口一行居中对齐；四周内边距、底边发丝线。
fn header_style(stacked: bool) -> NodeStyle {
    let frame = if stacked { Stack::column(14.0) } else { Stack::bar(14.0).align(AlignSpec::Center) };
    super::super::workbench::with_bottom_divider(frame.with_layout(|layout| {
        layout.padding_top = Some(LengthSpec::Px(16.0));
        layout.padding_bottom = Some(LengthSpec::Px(16.0));
        layout.padding_left = Some(LengthSpec::Px(18.0));
        layout.padding_right = Some(LengthSpec::Px(18.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }))
    .node_style()
}

/// 眉题「文件预览」、20px 文件名（去掉扩展名）、和文件名不同时的完整路径，以及注释、链接两行导语。
fn title_lines(head: Signal<HeadView>) -> (AnyView, AnyView, AnyView, AnyView) {
    let current = head.get_untracked();
    let lead_row = |label: &str, key: &'static str, pick: fn(&HeadView) -> String| {
        let value = move || head.with(|view| pick(view));
        widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
            .visible(move || !value().is_empty())
            .children((
                widget(eyebrow_bold(label)),
                widget(text(&untrack(value), 13.0, 500, SemanticColorRole::Text, 19.5).wrap_anywhere()).prop::<String, fields::text::value>(value),
            ))
            .key(key)
            .into_any()
    };
    let lead = widget(Stack::column(8.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).with_layout(|layout| {
        layout.margin_top = Some(LengthSpec::Px(12.0));
    }))
    .visible(move || head.with(|view| !view.comment.is_empty() || !view.link.is_empty()))
    .children((
        lead_row("注释", "inspect-lead-comment", |view| view.comment.clone()),
        lead_row("链接", "inspect-lead-link", |view| view.link.clone()),
    ))
    .key("inspect-preview-lead")
    .into_any();
    (
        widget(eyebrow("文件预览")).key("inspect-preview-eyebrow").into_any(),
        widget(spaced(text(&current.name, 20.0, 700, SemanticColorRole::Text, 31.0).wrap_anywhere(), 4.0))
            .prop::<String, fields::text::value>(move || head.with(|view| view.name.clone()))
            .key("inspect-preview-name")
            .into_any(),
        widget(spaced(text(&current.path, 14.0, 400, SemanticColorRole::Muted, 21.7).wrap_anywhere(), 4.0))
            .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || head.with(|view| view.path.clone()))
            .key("inspect-preview-path")
            .into_any(),
        lead,
    )
}

/// 正文：左边预览框、色板和资源库面板，右边文件事实卡。单列时事实卡跟在下面，正文整体滚动。
/// 不在预览时没有正文。
fn main(key: &MainKey, signals: PreviewSignals, metadata: MetadataSignals) -> AnyView {
    if !key.previewing {
        return ().into_any();
    }
    let stacked = key.stacked;
    let frame = if stacked {
        Stack::fill_column(DETAIL_GAP)
    } else {
        Stack::fill_row(DETAIL_GAP).align(AlignSpec::Stretch)
    };
    let frame = frame.padding(18.0).min_height(LengthSpec::Px(0.0)).with_layout(|layout| {
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.height = Some(LengthSpec::Fill);
    });
    let content = widget(frame)
        .children((preview_shell(signals, stacked), stats(signals, metadata, stacked, &key.scope)))
        .key("inspect-preview-body")
        .into_any();
    if !stacked {
        return content;
    }
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .children((content,))
    .key(format!("preview-body-{}", key.scope))
    .into_any()
}

/// 预览框（铺满）、色板槽（至少 14px）和资源库扩展的预览面板，行距 10。
fn preview_shell(signals: PreviewSignals, stacked: bool) -> AnyView {
    let panels = signals.panels;
    let frame = if stacked {
        Stack::column(10.0).min_height(LengthSpec::Px(360.0))
    } else {
        Stack::fill_column(10.0).with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        })
    };
    widget(frame.min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(0.0)))
        .children((
            preview_box(signals.content),
            dynamic(signals.palette, |colors: &Vec<String>| palette_slot(colors)),
            dynamic(panels, |panels: &Option<AsmrPanels>| panels.as_ref().map_or_else(|| ().into_any(), inspect_asmr::panels_view))
                .visible(move || panels.with(Option::is_some)),
        ))
        .key("inspect-preview-shell")
        .into_any()
}

/// 预览框：bg 底、12px 圆角。插件预览在顶部多一圈 accent-soft 径向光晕。框常驻，里面的内容按身份换。
fn preview_box(content: Signal<PreviewSnapshot>) -> AnyView {
    let plugin = content.with_untracked(|snapshot| snapshot.content.plugin);
    let inner = dynamic(content, |snapshot: &PreviewSnapshot| body::content_view(&snapshot.content)).css(fill_container());
    widget(Stack::column(0.0).style(box_style(plugin)))
        .prop::<NodeStyle, StyleField>(move || box_style(content.with(|snapshot| snapshot.content.plugin)))
        .children((inner,))
        .key("inspect-preview-box")
        .into_any()
}

fn box_style(plugin: bool) -> NodeStyle {
    let frame = Stack::fill_column(0.0)
        .radius(RadiusTier::Xl)
        .min_width(LengthSpec::Px(0.0))
        .min_height(LengthSpec::Px(0.0))
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            layout.padding_top = Some(LengthSpec::Px(1.0));
            layout.padding_left = Some(LengthSpec::Px(1.0));
            layout.padding_right = Some(LengthSpec::Px(1.0));
            layout.padding_bottom = Some(LengthSpec::Px(1.0));
        });
    let frame = if plugin { frame.painter(super::preview_paint::PluginGlow) } else { frame.surface(SemanticColorRole::Background) };
    frame.node_style()
}

/// 色板槽：没有颜色时只占 14px；有颜色时是最多五枚 34×12 的胶囊色块。
fn palette_slot(colors: &[String]) -> AnyView {
    let swatches = colors
        .iter()
        .filter_map(|color| parse_hex(color).map(|rgba| (color.clone(), rgba)))
        .take(5)
        .enumerate()
        .map(|(index, (color, rgba))| {
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(34.0))
                    .height(LengthSpec::Px(12.0))
                    .grow(0.0)
                    .shrink(0.0)
                    .outline(SemanticColorRole::BorderSoft, 1.0)
                    .radius_px(999.0)
                    .with_layout(move |layout| layout.background = Some(rgba)),
            )
            .key(format!("inspect-palette-{index}-{}", color.trim_start_matches('#')))
            .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true).min_height(LengthSpec::Px(14.0)).width(LengthSpec::Fill))
        .children(swatches)
        .key("inspect-palette-slot")
        .into_any()
}

/// `#RRGGBB` 换成 0..=1 的 RGBA。色板只收这一种写法。
fn parse_hex(color: &str) -> Option<[f32; 4]> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok().map(|value| f32::from(value) / 255.0);
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

/// 右侧事实卡：bg 底、12px 圆角、14px 内边距，行距 12，内容超高时卡内滚动。滚动区按预览目标取键。
fn stats(signals: PreviewSignals, metadata: MetadataSignals, stacked: bool, scope: &str) -> AnyView {
    let rows = widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((inspect_library::fact_column(signals.facts), inspect_metadata_view::metadata_panel(metadata)));
    let card = ScrollView::new(ScrollAxes::Vertical).with_layout(move |layout| {
        layout.width = Some(if stacked { LengthSpec::Fill } else { LengthSpec::Px(DETAIL_WIDTH) });
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        if !stacked {
            layout.height = Some(LengthSpec::Fill);
        }
        layout.padding_top = Some(LengthSpec::Px(15.0));
        layout.padding_bottom = Some(LengthSpec::Px(15.0));
        layout.padding_left = Some(LengthSpec::Px(15.0));
        layout.padding_right = Some(LengthSpec::Px(15.0));
    });
    let mut style = card.style.clone();
    style.background = Some(SemanticColorRole::Background);
    style.radius = Some(RadiusTier::Xl);
    widget(card.style(style)).children((rows,)).key(format!("preview-stats-{scope}")).into_any()
}

/// 页头按钮：透明底、正文色，图标在字前，悬停铺 bg-hover。禁用时字和图标在面板底色上淡化。
fn action_button(label: &str, icon: Icon, icon_size: f32, height: f32, disabled: bool) -> Button {
    let mut button = Button::new(label).kind(ButtonKind::Ghost).icon(icon).icon_size(icon_size).icon_gap(6.0).disabled(disabled);
    button.style.interaction.disabled = nana_ui::runtime::SemanticPaint::default();
    if disabled {
        button.style.interaction.base.foreground_mix = Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Surface,
            super::super::player_view::bar::DISABLED_OPACITY,
        ));
    }
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(height));
    layout.min_height = Some(LengthSpec::Px(height));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.font_weight = Some(500);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 页头按钮的文字、图标、图标尺寸和高度。
type ActionLook = (&'static str, Icon, f32, f32);

/// 页头按钮，能不能点按字段绑定：禁用态和淡化的样式一起变。
fn action(look: ActionLook, key: &'static str, disabled: impl Fn() -> bool + Send + Clone + 'static) -> nana_ui::runtime::view::El<Button> {
    let (label, icon, icon_size, height) = look;
    let style_disabled = disabled.clone();
    widget(action_button(label, icon, icon_size, height, untrack(&disabled)))
        .prop::<bool, fields::button::disabled>(disabled)
        .prop::<NodeStyle, StyleField>(move || action_button(label, icon, icon_size, height, style_disabled()).style)
        .key(key)
}

/// 11px 半粗、弱色、字距 0.4 的眉题。
pub(super) fn eyebrow(label: &str) -> Text {
    let mut node = text(label, 11.0, 600, SemanticColorRole::Faint, 17.05);
    std::sync::Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    node
}

/// 同眉题，字重 700。事实行和导语的标签用它。
pub(super) fn eyebrow_bold(label: &str) -> Text {
    let mut node = eyebrow(label);
    std::sync::Arc::make_mut(&mut node.style.layout).font_weight = Some(700);
    node
}

pub(super) fn text(label: &str, size: f32, weight: u16, role: SemanticColorRole, line_height: f32) -> Text {
    let mut node = Text::new(label).color(role).font_size(size).font_weight(weight).line_height(line_height);
    let layout = std::sync::Arc::make_mut(&mut node.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    node
}

/// 文字上方留出外边距。
fn spaced(mut node: Text, margin: f32) -> Text {
    std::sync::Arc::make_mut(&mut node.style.layout).margin_top = Some(LengthSpec::Px(margin));
    node
}

/// 长路径和长文件名按任意位置折行，不撑宽页头。
trait WrapAnywhere {
    fn wrap_anywhere(self) -> Self;
}

impl WrapAnywhere for Text {
    fn wrap_anywhere(mut self) -> Self {
        let layout = std::sync::Arc::make_mut(&mut self.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        self
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().filter(|part| !part.is_empty()).unwrap_or(path).to_string()
}

fn extension_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, ext)| ext.to_string()).unwrap_or_default()
}
