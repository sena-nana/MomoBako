//! 文件预览页的外框，对齐 Vue `FilePreviewPane` 和 `.files-preview-page`。
//!
//! bg-elev 圆角面板，自上而下：页头（返回、眉题和文件名、已保存的注释与链接、打开与定位）、正文两栏（左边预览框加色板，
//! 右边 300px 的文件事实和元数据卡），最底下贴着播放条。正文在窗口不宽于 900px 时上下排。
//!
//! 常驻：外框、页头和底部播放条的位置只建一次，文件名、路径、导语和按钮能不能点按字段绑定，
//! 按钮的处理器现读信号里的绝对路径。「返回」只收起预览，不重读目录。正文按「排法 + 预览目标」换一整块（滚动区回到顶部）；
//! 同一个目标里，预览框外框常驻，框里的内容按身份换（见 `inspect_preview_body.rs`），
//! 事实和元数据按字段绑定。

use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{
    AnyView, IntoView, NodeRef, Signal, dynamic, signal, untrack, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, LengthSpec, NodeStyle, RadiusTier, ScrollAxes, ScrollView,
    SemanticColorRole, Stack, Text,
};
use nana_ui::{ButtonKind, Icon};

use super::super::files_view::bind::{StyleField, fill_container};
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
    /// 照 Vue `entryDisplayTitle` 取的标题。目录保留原名，文件只在名称以条目扩展名结尾时去掉。
    name: String,
    /// 和文件名不同时的完整路径，否则为空。
    path: String,
    comment: String,
    link: String,
    /// 没有预览目标：三个按钮都点不了。
    empty: bool,
    can_open: bool,
    can_reveal: bool,
    has_repo: bool,
    /// 目标的绝对路径，打开和定位用。
    absolute: String,
    /// 目标在仓库里的相对路径，「打开」时记访问用。
    target: String,
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
            head: signal(head_view(model)),
            main: signal(main_key(model)),
            content: signal(PreviewSnapshot::project(model, None)),
            palette: signal(displayed_palette(model)),
            panels: signal(panels(model)),
            facts: signal(FactsView::of(&model.inspect.facts)),
        }
    }

    /// 写入投影，只写变了的；预览框内容的身份没变时不抄大块数据。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        self.head.try_set_if_changed(head_view(model));
        self.main.try_set_if_changed(main_key(model));
        if self.content.defined_at().is_some() {
            let snapshot = self
                .content
                .with_untracked(|previous| PreviewSnapshot::project(model, Some(previous)));
            self.content.set_if_changed(snapshot);
        }
        self.palette.try_set_if_changed(displayed_palette(model));
        self.panels.try_set_if_changed(panels(model));
        self.facts
            .try_set_if_changed(FactsView::of(&model.inspect.facts));
    }
}

/// ASMR 队列和播放列表面板：只在预览页打开时投影，藏着的预览页不建它们。
fn panels(model: &ShellViewModel) -> Option<AsmrPanels> {
    super::super::files_view::previewing(model)
        .then(|| inspect_asmr::preview_panels(model))
        .flatten()
}

fn main_key(model: &ShellViewModel) -> MainKey {
    let previewing = super::super::files_view::previewing(model);
    let scope = if previewing {
        body::scope(model)
    } else {
        String::new()
    };
    MainKey {
        previewing,
        stacked: model.viewport_width <= STACK_BREAKPOINT,
        scope,
    }
}

fn head_view(model: &ShellViewModel) -> HeadView {
    let path = model
        .inspect
        .target_path
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    let (title, shown_path) = preview_heading(model, &path);
    let ctx = super::super::files::FileContext::from_model(model);
    let repository = model.workspace.active_repository();
    let absolute = super::super::input::resolve_entry_absolute(model, &path);
    let can_open = !path.is_empty() && !absolute.trim().is_empty();
    // Vue 页头读列表条目的 metadata，不读编辑器草稿。草稿要等保存写回这一行才出现。
    let (comment, link) = saved_lead(model);
    HeadView {
        stacked: model.viewport_width <= STACK_BREAKPOINT,
        name: title,
        path: shown_path,
        comment,
        link,
        empty: path.is_empty(),
        can_open,
        can_reveal: can_open && !ctx.trash,
        has_repo: repository.is_some(),
        absolute,
        target: path,
    }
}

/// 页头的注释和链接。
///
/// Vue `FilePreviewPane` 读的是列表条目的 `entry.metadata`，编辑器草稿要等保存写回这一行才出现。
/// 注释是空字符串时改用 `note`。当前列表里没有这条时，用已经读回或保存过的素材详情，同样不读草稿。
fn saved_lead(model: &ShellViewModel) -> (String, String) {
    let path = model.inspect.open_path().map(str::trim).unwrap_or("");
    if !path.is_empty() {
        let ctx = super::super::files::FileContext::from_model(model);
        if let Some(row) = model
            .files
            .listed_rows(&ctx)
            .into_iter()
            .find(|row| row.path == path)
        {
            return (entry_comment(&row.metadata), entry_link(&row.metadata));
        }
    }
    (
        model.inspect.saved_comment().trim().to_string(),
        model.inspect.saved_link().trim().to_string(),
    )
}

/// Vue `metadataString`：不是字符串的值当成空。
fn metadata_string(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> String {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

/// Vue `metadataComment`：`comment` 是空字符串时用 `note`。显示时去掉首尾空白。
fn entry_comment(metadata: &std::collections::BTreeMap<String, serde_json::Value>) -> String {
    let comment = metadata_string(metadata, "comment");
    let chosen = if comment.is_empty() {
        metadata_string(metadata, "note")
    } else {
        comment
    };
    chosen.trim().to_string()
}

/// 链接。显示时去掉首尾空白，空白不占一行。
fn entry_link(metadata: &std::collections::BTreeMap<String, serde_json::Value>) -> String {
    metadata_string(metadata, "link").trim().to_string()
}

/// 预览页打开、定位现在认的绝对路径。测试用来核对页头和 `resolve_entry_absolute` 是同一条。
#[cfg(test)]
pub(crate) fn preview_open_target(model: &ShellViewModel) -> (bool, bool, String) {
    let head = head_view(model);
    (head.can_open, head.can_reveal, head.absolute)
}

/// 预览页。`bar` 是页底播放条的占位节点，里面的播放条是旧视图岛。
pub(super) fn preview_page(
    signals: PreviewSignals,
    metadata: MetadataSignals,
    bar: NodeRef,
    sweep: Signal<f32>,
) -> AnyView {
    let main = dynamic(signals.main, move |key: &MainKey| {
        main(key, signals, metadata, sweep)
    })
    .css(fill_container());
    widget(Stack::column(0.0).style(page_style()))
        .children((
            header(signals.head),
            main,
            widget(Stack::column(0.0))
                .node_ref(bar)
                .key("files-island-preview-bar"),
        ))
        .key("inspect-preview-page")
        .into_any()
}

/// 外框：bg-elev、`--radius-2xl` 的面板，裁掉溢出，四边留 1px。
fn page_style() -> NodeStyle {
    Stack::fill_column(0.0)
        .surface(SemanticColorRole::Surface)
        .radius(RadiusTier::Xxl)
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
    let back = action(
        ("返回", icons::ARROW_LEFT, 15.0, 34.0),
        "inspect-back",
        back_disabled,
    )
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Files(
            super::super::files::FilesMessage::ExitPreview,
        ));
    });
    let open = action(
        ("打开", icons::EYE, 14.0, 32.0),
        "inspect-open",
        open_disabled,
    )
    .on_cx(move |_, _: &Activate, cx| {
        let (has_repo, absolute_path, path) =
            head.with_untracked(|view| (view.has_repo, view.absolute.clone(), view.target.clone()));
        cx.dispatch_program_all(ShellMessage::Input(
            super::super::input::InputMessage::OpenEntry {
                has_repo,
                absolute_path,
                path,
            },
        ));
    });
    let reveal = action(
        ("定位", icons::FOLDER_OPEN, 14.0, 32.0),
        "inspect-reveal",
        reveal_disabled,
    )
    .on_cx(move |_, _: &Activate, cx| {
        let absolute_path = head.with_untracked(|view| view.absolute.clone());
        cx.dispatch_program_all(ShellMessage::Input(
            super::super::input::InputMessage::RevealEntry { absolute_path },
        ));
    });
    let actions = widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .grow(0.0)
            .shrink(0.0),
    )
    .key("inspect-preview-actions")
    .children((open, reveal));
    let titles = widget(
        Stack::column(0.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children(title_lines(head))
    .key("inspect-preview-titles");
    let back_cell = widget(Stack::row(0.0).grow(0.0).shrink(0.0))
        .children((back,))
        .key("inspect-back-cell");
    widget(Stack::column(0.0).style(header_style(current.stacked)))
        .prop::<NodeStyle, StyleField>(move || header_style(head.with(|view| view.stacked)))
        .children((back_cell, titles, actions))
        .key("inspect-preview-header")
        .into_any()
}

/// 页头的排法：窄窗口上下排，宽窗口一行居中对齐；四周内边距、底边发丝线。
fn header_style(stacked: bool) -> NodeStyle {
    let frame = if stacked {
        Stack::column(14.0)
    } else {
        Stack::bar(14.0).align(AlignSpec::Center)
    };
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
        widget(
            Stack::column(4.0)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0)),
        )
        .visible(move || !value().is_empty())
        .children((
            widget(eyebrow_bold(label)),
            widget(text(&untrack(value), 13.0, 500, SemanticColorRole::Text, 19.5).wrap_anywhere())
                .prop::<String, fields::text::value>(value),
        ))
        .key(key)
        .into_any()
    };
    let lead = widget(
        Stack::column(8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.margin_top = Some(LengthSpec::Px(12.0));
            }),
    )
    .visible(move || head.with(|view| !view.comment.is_empty() || !view.link.is_empty()))
    .children((
        lead_row("注释", "inspect-lead-comment", |view| {
            view.comment.clone()
        }),
        lead_row("链接", "inspect-lead-link", |view| view.link.clone()),
    ))
    .key("inspect-preview-lead")
    .into_any();
    (
        widget(eyebrow("文件预览"))
            .key("inspect-preview-eyebrow")
            .into_any(),
        widget(spaced(
            text(&current.name, 20.0, 700, SemanticColorRole::Text, 31.0).wrap_anywhere(),
            4.0,
        ))
        .prop::<String, fields::text::value>(move || head.with(|view| view.name.clone()))
        .key("inspect-preview-name")
        .into_any(),
        widget(spaced(
            text(&current.path, 14.0, 400, SemanticColorRole::Muted, 21.7).wrap_anywhere(),
            4.0,
        ))
        .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || {
            head.with(|view| view.path.clone())
        })
        .key("inspect-preview-path")
        .into_any(),
        lead,
    )
}

/// 正文：左边预览框、色板和资源库面板，右边文件事实卡。单列时事实卡跟在下面，正文整体滚动。
/// 不在预览时没有正文。
fn main(
    key: &MainKey,
    signals: PreviewSignals,
    metadata: MetadataSignals,
    sweep: Signal<f32>,
) -> AnyView {
    if !key.previewing {
        return ().into_any();
    }
    let stacked = key.stacked;
    let frame = if stacked {
        Stack::fill_column(DETAIL_GAP)
    } else {
        Stack::fill_row(DETAIL_GAP).align(AlignSpec::Stretch)
    };
    let frame = frame
        .padding(18.0)
        .min_height(LengthSpec::Px(0.0))
        .with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(1.0);
            layout.height = Some(LengthSpec::Fill);
        });
    let content = widget(frame)
        .children((
            preview_shell(signals, stacked, sweep),
            stats(signals, metadata, stacked, &key.scope),
        ))
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
fn preview_shell(signals: PreviewSignals, stacked: bool, sweep: Signal<f32>) -> AnyView {
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
    widget(
        frame
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children((
        preview_box(signals.content, sweep),
        dynamic(signals.palette, |colors: &Vec<String>| palette_slot(colors)),
        dynamic(panels, |panels: &Option<AsmrPanels>| {
            panels
                .as_ref()
                .map_or_else(|| ().into_any(), inspect_asmr::panels_view)
        })
        .visible(move || panels.with(Option::is_some)),
    ))
    .key("inspect-preview-shell")
    .into_any()
}

/// 预览框：bg 底、12px 圆角。插件预览在顶部多一圈 accent-soft 径向光晕。框常驻，里面的内容按身份换。
fn preview_box(content: Signal<PreviewSnapshot>, sweep: Signal<f32>) -> AnyView {
    let plugin = content.with_untracked(|snapshot| snapshot.content.plugin);
    let inner = dynamic(content, move |snapshot: &PreviewSnapshot| {
        super::preview_paint::loading_sweep_scope(sweep, || body::content_view(&snapshot.content))
    })
    .css(fill_container());
    widget(Stack::column(0.0).style(box_style(plugin)))
        .prop::<NodeStyle, StyleField>(move || {
            box_style(content.with(|snapshot| snapshot.content.plugin))
        })
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
    let frame = if plugin {
        frame.painter(super::preview_paint::PluginGlow)
    } else {
        frame.surface(SemanticColorRole::Background)
    };
    frame.node_style()
}

/// 预览页色板。元数据里的颜色优先；没有时用这条缩略图解码后取出的颜色。
fn displayed_palette(model: &ShellViewModel) -> Vec<String> {
    let metadata = if !model.inspect.palette.is_empty() {
        model.inspect.palette.clone()
    } else {
        list_palette(model)
    };
    super::super::palette::swatch_colors(&metadata, preview_thumbnail(model), &model.files)
}

/// 列表行上已经记下的元数据色板。详情还没读回来时用它，避免把取样颜色当成元数据。
fn list_palette(model: &ShellViewModel) -> Vec<String> {
    let Some(path) = model
        .inspect
        .target_path
        .as_deref()
        .or(model.files.primary.as_deref())
    else {
        return Vec::new();
    };
    model
        .files
        .rows
        .iter()
        .chain(model.files.virtual_rows.iter())
        .chain(model.files.category_rows.iter())
        .find(|row| row.path == path)
        .map(|row| row.palette.clone())
        .unwrap_or_default()
}

/// 这条预览对应的缩略图文件。素材详情里的路径优先，还没有时用列表行上的。
fn preview_thumbnail(model: &ShellViewModel) -> Option<&str> {
    model
        .inspect
        .facts
        .thumbnail_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .or_else(|| {
            let path = model
                .inspect
                .target_path
                .as_deref()
                .or(model.files.primary.as_deref())?;
            model
                .files
                .rows
                .iter()
                .chain(model.files.virtual_rows.iter())
                .chain(model.files.category_rows.iter())
                .find(|row| row.path == path)
                .and_then(|row| row.thumbnail_path.as_deref())
        })
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
            .key(format!(
                "inspect-palette-{index}-{}",
                color.trim_start_matches('#')
            ))
            .into_any()
        })
        .collect::<Vec<_>>();
    widget(
        Stack::row(8.0)
            .wrap(true)
            .min_height(LengthSpec::Px(14.0))
            .width(LengthSpec::Fill),
    )
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
    let channel = |index: usize| {
        u8::from_str_radix(&hex[index..index + 2], 16)
            .ok()
            .map(|value| f32::from(value) / 255.0)
    };
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

/// 右侧事实卡：bg 底、12px 圆角、14px 内边距，行距 12，内容超高时卡内滚动。滚动区按预览目标取键。
fn stats(
    signals: PreviewSignals,
    metadata: MetadataSignals,
    stacked: bool,
    scope: &str,
) -> AnyView {
    let rows = widget(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        inspect_library::fact_column(signals.facts),
        inspect_metadata_view::metadata_panel(metadata),
    ));
    let card = ScrollView::new(ScrollAxes::Vertical).with_layout(move |layout| {
        layout.width = Some(if stacked {
            LengthSpec::Fill
        } else {
            LengthSpec::Px(DETAIL_WIDTH)
        });
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
    widget(card.style(style))
        .children((rows,))
        .key(format!("preview-stats-{scope}"))
        .into_any()
}

/// 页头按钮：透明底、正文色，图标在字前，悬停铺 bg-hover。禁用时字和图标在面板底色上淡化。
fn action_button(label: &str, icon: Icon, icon_size: f32, height: f32, disabled: bool) -> Button {
    let mut button = Button::new(label)
        .kind(ButtonKind::Ghost)
        .icon(icon)
        .icon_size(icon_size)
        .icon_gap(6.0)
        .disabled(disabled);
    button.style.interaction.disabled = nana_ui::runtime::SemanticPaint::default();
    if disabled {
        button.style.interaction.base.foreground_mix = Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Surface,
            crate::theme_map::DISABLED_OPACITY,
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
fn action(
    look: ActionLook,
    key: &'static str,
    disabled: impl Fn() -> bool + Send + Clone + 'static,
) -> nana_ui::runtime::view::El<Button> {
    let (label, icon, icon_size, height) = look;
    let style_disabled = disabled.clone();
    widget(action_button(
        label,
        icon,
        icon_size,
        height,
        untrack(&disabled),
    ))
    .prop::<bool, fields::button::disabled>(disabled)
    .prop::<NodeStyle, StyleField>(move || {
        action_button(label, icon, icon_size, height, style_disabled()).style
    })
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

pub(super) fn text(
    label: &str,
    size: f32,
    weight: u16,
    role: SemanticColorRole,
    line_height: f32,
) -> Text {
    let mut node = Text::new(label)
        .color(role)
        .font_size(size)
        .font_weight(weight)
        .line_height(line_height);
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

/// 页头标题和它下面的路径。
///
/// Vue `previewFileEntry` 再 `entryDisplayTitle`：当前列表里的这条优先（目录保留原名，文件只在名称以条目扩展名结尾时去掉）。
/// 列表里没有时，正在播放的播放集里有这条就用它的文件名和扩展名，预览临时插进去的条目不算。
/// 播放集是空的再用打开着的播放集页。都没有时才按路径最后一段去掉扩展名。
/// 路径和条目名称不同时，标题下面仍写出完整路径。
fn preview_heading(model: &ShellViewModel, path: &str) -> (String, String) {
    if let Some((name, title)) = preview_entry_title(model, path) {
        let shown = if !path.is_empty() && path != name {
            path.to_string()
        } else {
            String::new()
        };
        return (title, shown);
    }
    let name = file_name(path);
    let title = super::super::player_view::bar::display_title(&name, &extension_of(&name));
    let shown = if !path.is_empty() && path != name {
        path.to_string()
    } else {
        String::new()
    };
    (title, shown)
}

/// 当前列表、正在播放的播放集、打开着的播放集页，按 Vue 的顺序找这条的名称和标题。
fn preview_entry_title(model: &ShellViewModel, path: &str) -> Option<(String, String)> {
    if path.is_empty() {
        return None;
    }
    let ctx = super::super::files::FileContext::from_model(model);
    if let Some(row) = model
        .files
        .listed_rows(&ctx)
        .into_iter()
        .find(|row| row.path == path)
    {
        return Some((row.name.clone(), row.display_title()));
    }
    playlist_preview_name(model, path).map(|(name, extension)| {
        let title =
            super::super::files::entry_display_title("file", &name, Some(extension.as_str()));
        (name, title)
    })
}

/// 播放集里的文件名和扩展名。正在播放的队列里有非临时条目时只认这份，认不到就不去翻打开着的那一页。
fn playlist_preview_name(model: &ShellViewModel, path: &str) -> Option<(String, String)> {
    let persistent: Vec<_> = model
        .player
        .queue
        .iter()
        .filter(|item| !item.transient)
        .collect();
    if !persistent.is_empty() {
        return persistent
            .into_iter()
            .find(|item| item.path == path)
            .map(|item| (item.filename.clone(), item.extension.clone()));
    }
    model.player.listed.as_ref().and_then(|detail| {
        detail
            .items
            .iter()
            .find(|item| item.path == path)
            .map(|item| (item.filename.clone(), item.extension.clone()))
    })
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn extension_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_string())
        .unwrap_or_default()
}

/// 预览右侧的类型、大小、硬链接和修改时间按 Vue `previewFileEntry` 填。
///
/// `selectAsset` 只写入 `activeAssetDetail`，不改这条预览条目。详情回来以后四行仍是它，
/// 不用素材摘要换掉。当前面板的列表优先，再看目录、智能文件夹和分类视图。都没有时：
/// 正在播放的队列非空就只在队列里找这条；队列是空的才用打开着的播放集页。
/// 播放集条目只有扩展名，大小、硬链接和修改时间留空，不拿详情里的字段补，也不编造。
pub(crate) fn seed_preview_facts(model: &mut ShellViewModel, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        eprintln!("Nana 预览事实没有路径，不从列表填");
        return;
    }
    // Vue `previewFileEntry` 不经 `withActiveAssetMetadata`。预览页开着时，编辑器四个输入跟这一条列表元数据。
    if super::super::files_view::previewing(model) {
        let metadata = list_fact_row(model, path).map(|row| row.metadata.clone());
        if let Some(metadata) = metadata {
            crate::shell::inspect::support::adopt_preview_entry(&mut model.inspect, &metadata);
        }
    }
    if let Some(row) = listed_facts(model, path) {
        model.inspect.facts.extension = row.extension;
        model.inspect.facts.size_label = row.size_label;
        model.inspect.facts.modified_at = row.modified_at;
        model.inspect.facts.hardlink_state = row.hardlink_state;
        return;
    }
    if let Some(extension) = playlist_fact_extension(model, path) {
        model.inspect.facts.extension = extension;
        // 播放集条目没有大小、硬链接和修改时间。详情里的素材摘要不能把这三行补上。
        model.inspect.facts.size_label.clear();
        model.inspect.facts.modified_at.clear();
        model.inspect.facts.hardlink_state = None;
    }
}

/// 单击选中时，详情卡编辑器的四个输入跟 Vue `currentFileEntry`。
///
/// `withActiveAssetMetadata`：素材 id 对得上时，详情元数据盖到当前列表条目上，同键以详情为准，
/// 详情没有的键留着列表的。对不上或详情还没回来时只用列表。空 `comment` 退回 `note`。
/// 列表里没有这一条时四个输入留着已经记下的素材详情。正在输入时不盖。预览页开着时不进这里。
pub(crate) fn seed_detail_card_editor(model: &mut ShellViewModel, path: &str) {
    if super::super::files_view::previewing(model) {
        return;
    }
    let path = path.trim();
    if path.is_empty() {
        eprintln!("Nana 详情卡编辑器没有路径，不从列表填");
        return;
    }
    if model.inspect.dirty() {
        return;
    }
    let metadata = {
        let ctx = super::super::files::FileContext::from_model(model);
        let Some(row) = model
            .files
            .listed_rows(&ctx)
            .into_iter()
            .find(|row| row.path == path && row.kind != "directory")
        else {
            eprintln!("Nana 详情卡编辑器在列表里找不到这一条，四个输入留着素材详情：{path}");
            return;
        };
        merged_list_metadata(
            &row.metadata,
            model.files.detail_metadata(row.asset_id.as_deref()),
        )
    };
    crate::shell::inspect::support::adopt_preview_entry(&mut model.inspect, &metadata);
}

/// Vue `withActiveAssetMetadata` 的元数据：没有对得上的详情时原样返回列表。
fn merged_list_metadata(
    row: &std::collections::BTreeMap<String, serde_json::Value>,
    overlay: Option<&std::collections::BTreeMap<String, serde_json::Value>>,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    let Some(overlay) = overlay else {
        return row.clone();
    };
    let mut merged = row.clone();
    for (key, value) in overlay {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

/// 从列表抄下来的四行。先抄出来再写回事实，避免一边借列表一边改预览状态。
struct ListedFacts {
    extension: String,
    size_label: String,
    modified_at: String,
    hardlink_state: Option<String>,
}

/// 列表里这条文件的四行。目录不当成预览事实。当前面板先看，再看另外两份列表。
fn listed_facts(model: &ShellViewModel, path: &str) -> Option<ListedFacts> {
    let row = list_fact_row(model, path)?;
    Some(ListedFacts {
        extension: row.extension.clone().unwrap_or_default(),
        size_label: row.size_label.clone(),
        modified_at: row.modified_at.clone(),
        hardlink_state: row
            .hardlink_state
            .clone()
            .filter(|text| !text.trim().is_empty()),
    })
}

/// 列表里这条文件。目录不当成预览事实。当前面板先看，再看另外两份列表。
fn list_fact_row<'a>(
    model: &'a ShellViewModel,
    path: &str,
) -> Option<&'a crate::shell::files::FileRow> {
    let files = &model.files;
    let primary: &[crate::shell::files::FileRow] =
        if model.workspace.panel == crate::shell::WorkspacePanel::SmartFolder {
            &files.virtual_rows
        } else if model.workspace.panel == crate::shell::WorkspacePanel::Files
            && model.workspace.library_category != crate::shell::LibraryCategory::All
        {
            &files.category_rows
        } else {
            &files.rows
        };
    primary
        .iter()
        .chain(files.rows.iter())
        .chain(files.virtual_rows.iter())
        .chain(files.category_rows.iter())
        .find(|row| row.path == path && row.kind != "directory")
}

/// 播放集上的扩展名。队列里已经有条目时不去翻打开着的另一页。
fn playlist_fact_extension(model: &ShellViewModel, path: &str) -> Option<String> {
    if !model.player.queue.is_empty() {
        return model
            .player
            .queue
            .iter()
            .find(|item| item.path == path)
            .map(|item| item.extension.clone());
    }
    let page = model.player.listed.as_ref()?;
    page.items
        .iter()
        .find(|item| item.path == path)
        .map(|item| item.extension.clone())
}

#[cfg(test)]
mod lead_tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use crate::backend::services::repository::{
        AssetDetail, AssetSummary, FileBrowserEntry, MetadataEntry, PlaylistDetail, PlaylistItem,
        PlaylistSummary,
    };
    use crate::shell::player::QueueItem;
    use crate::shell::{FileRow, InspectMessage, ShellMessage, ShellViewModel};

    use super::head_view;

    fn entry(path: &str, metadata: BTreeMap<String, Value>) -> FileBrowserEntry {
        FileBrowserEntry {
            path: path.into(),
            name: path.rsplit('/').next().unwrap_or(path).into(),
            kind: "file".into(),
            extension: Some("mp3".into()),
            size_bytes: Some(12),
            size_label: Some("12 B".into()),
            modified_at: None,
            asset_id: Some("asset".into()),
            status: Some("ready".into()),
            thumbnail_path: None,
            thumbnail_custom: false,
            hardlink_group_id: None,
            hardlink_state: None,
            tags: Vec::new(),
            alias_paths: Vec::new(),
            folder_metadata: None,
            metadata,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        }
    }

    fn detail(path: &str, comment: &str, link: &str) -> AssetDetail {
        AssetDetail {
            summary: AssetSummary {
                asset_id: "asset".into(),
                repo_id: "repo".into(),
                path: path.into(),
                filename: "a.mp3".into(),
                extension: "mp3".into(),
                size_bytes: 12,
                size_label: "12 B".into(),
                status: "ready".into(),
                modified_at: String::new(),
                last_accessed_at: None,
                version: 1,
                tags: Vec::new(),
                thumbnail_path: None,
                hardlink_group_id: None,
                hardlink_state: None,
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            },
            metadata: vec![
                MetadataEntry {
                    key: "comment".into(),
                    value_type: "string".into(),
                    value: json!(comment),
                    version: 1,
                    updated_at: String::new(),
                },
                MetadataEntry {
                    key: "link".into(),
                    value_type: "string".into(),
                    value: json!(link),
                    version: 1,
                    updated_at: String::new(),
                },
            ],
            revisions: Vec::new(),
        }
    }

    /// 页头注释和链接跟着列表里已经保存的条目走。编辑器草稿变了不提前出现；
    /// 保存写回这一行以后才换。列表里没有这条时用已保存的素材详情，仍不读草稿。
    /// 注释字段是空的时候用 note，和 Vue `metadataComment` 一样。
    #[test]
    fn preview_header_lead_stays_on_the_saved_entry_while_the_draft_changes() {
        let path = "voice/a.mp3";
        let mut metadata = BTreeMap::new();
        metadata.insert("note".into(), json!("列表备注"));
        metadata.insert("link".into(), json!(" https://saved.example "));
        let mut model = ShellViewModel::default();
        model.files.rows = vec![FileRow::from_entry(&entry(path, metadata))];
        model
            .inspect
            .note_selected_detail(&detail(path, "详情注释", "https://detail.example"), false);
        model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
            "草稿注释".into(),
        )));
        model.reduce(ShellMessage::Inspect(InspectMessage::SetLink(
            "https://draft.example".into(),
        )));

        let head = head_view(&model);
        assert_eq!(
            head.comment, "列表备注",
            "空 comment 用列表里的 note，不读草稿或详情"
        );
        assert_eq!(
            head.link, "https://saved.example",
            "链接用列表里已保存的，去掉首尾空白"
        );
        assert_eq!(model.inspect.draft_comment(), "草稿注释");

        model.files.rows[0]
            .metadata
            .insert("comment".into(), json!("已写入"));
        model.files.rows[0]
            .metadata
            .insert("link".into(), json!("https://written.example"));
        let written = head_view(&model);
        assert_eq!(
            written.comment, "已写入",
            "保存写回这一行以后，页头换成已保存的注释"
        );
        assert_eq!(written.link, "https://written.example");

        model.files.rows.clear();
        model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
            "还是草稿".into(),
        )));
        let fallback = head_view(&model);
        assert_eq!(
            fallback.comment, "详情注释",
            "列表里没有这条时用已保存的详情，不读草稿"
        );
        assert_eq!(fallback.link, "https://detail.example");
        assert_eq!(model.inspect.draft_comment(), "还是草稿");
    }

    /// 页头标题照 Vue `entryDisplayTitle`，不按路径最后一段猜扩展名。
    ///
    /// 当前列表优先：扩展名对不上时保留原名，目录保留带点的名字。列表里没有时用正在播放的播放集；
    /// 播放集是空的再用打开着的播放集页。正在播放的队列里没有这条时，不用另一页的文件名。
    #[test]
    fn preview_header_title_uses_the_entry_name_and_extension() {
        let mut model = ShellViewModel::default();
        model.inspect.target_path = Some("dir/notes.md".into());
        model.files.rows = vec![named_row("dir/notes.md", "notes.md", "file", Some("txt"))];
        let listed = head_view(&model);
        assert_eq!(listed.name, "notes.md", "扩展名不是 md，标题保留原名");
        assert_eq!(listed.path, "dir/notes.md", "路径和名称不同时仍写出路径");

        model.files.rows = vec![named_row(
            "albums/photos.v2",
            "photos.v2",
            "directory",
            Some("v2"),
        )];
        model.inspect.target_path = Some("albums/photos.v2".into());
        let directory = head_view(&model);
        assert_eq!(directory.name, "photos.v2", "目录不去掉扩展名");
        assert_eq!(directory.path, "albums/photos.v2");

        model.inspect.target_path = Some("dir/notes.md".into());
        model.files.rows = vec![named_row("dir/notes.md", "列表.md", "file", Some("txt"))];
        model.player.queue = vec![queue_item("dir/notes.md", "队列.md", "md", false)];
        assert_eq!(head_view(&model).name, "列表.md", "当前列表优先于播放集");

        model.files.rows.clear();
        model.player.queue.clear();
        model.player.listed = Some(playlist_page("album/track.mp3", "夜曲.mp3", "mp3"));
        model.inspect.target_path = Some("album/track.mp3".into());
        let page = head_view(&model);
        assert_eq!(
            page.name, "夜曲",
            "没在播放时用打开着的播放集页，扩展名对得上才去掉"
        );
        assert_eq!(page.path, "album/track.mp3");

        model.player.queue = vec![queue_item("album/track.mp3", "track.mp3", "mp3", true)];
        assert_eq!(
            head_view(&model).name,
            "夜曲",
            "预览临时条目不算正在播放的播放集"
        );

        model.player.queue = vec![queue_item("other/a.mp3", "别的.mp3", "mp3", false)];
        let skipped = head_view(&model);
        assert_eq!(
            skipped.name, "track",
            "正在播放的播放集里没有这条，不用另一页的文件名"
        );
        assert_ne!(skipped.name, "夜曲");
    }

    fn named_row(path: &str, name: &str, kind: &str, extension: Option<&str>) -> FileRow {
        let mut row = FileRow::from_entry(&entry(path, BTreeMap::new()));
        row.name = name.into();
        row.kind = kind.into();
        row.extension = extension.map(str::to_string);
        row
    }

    fn queue_item(path: &str, filename: &str, extension: &str, transient: bool) -> QueueItem {
        QueueItem {
            id: "item".into(),
            playlist_id: "pl".into(),
            asset_id: "asset".into(),
            path: path.into(),
            filename: filename.into(),
            extension: extension.into(),
            status: "ready".into(),
            status_reason: None,
            transient,
            player_type_id: "audio".into(),
            player_label: "音频".into(),
            file_class: "audio".into(),
            thumbnail_path: None,
            is_virtual: false,
            local_absolute_path: None,
        }
    }

    fn playlist_page(path: &str, filename: &str, extension: &str) -> PlaylistDetail {
        PlaylistDetail {
            playlist: PlaylistSummary {
                playlist_id: "pl".into(),
                repo_id: "repo".into(),
                name: "早晨".into(),
                player_type_id: "audio".into(),
                player_plugin_id: "audio".into(),
                player_label: "音频".into(),
                file_class: "audio".into(),
                item_count: 1,
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            items: vec![PlaylistItem {
                playlist_item_id: "item".into(),
                playlist_id: "pl".into(),
                asset_id: "asset".into(),
                path: path.into(),
                filename: filename.into(),
                extension: extension.into(),
                thumbnail_path: None,
                status: "ready".into(),
                status_reason: None,
                sort_order: 0,
                added_at: String::new(),
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                metadata: None,
                local_absolute_path: None,
            }],
        }
    }
}

#[cfg(test)]
#[path = "inspect_preview_facts_tests.rs"]
mod fact_tests;
