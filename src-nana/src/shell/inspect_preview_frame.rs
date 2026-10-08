//! 文件预览页的外框，对齐 Vue `FilePreviewPane` 和 `.files-preview-page`。
//!
//! bg-elev 圆角面板，自上而下：页头（返回、眉题和文件名、打开与定位）、正文两栏（左边预览框加色板，
//! 右边 300px 的文件事实和元数据卡），最底下贴着播放条。正文在窗口不宽于 900px 时上下排。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack, Text,
};
use nana_ui::{ButtonKind, Icon};

use super::super::player_view::icons;
use super::super::{ShellMessage, ShellViewModel};

/// Vue `@media (max-width: 900px)`：正文和页头改成单列。
const STACK_BREAKPOINT: f32 = 900.0;
/// 右侧文件事实卡宽度，对应 `--workspace-file-detail-width`。
const DETAIL_WIDTH: f32 = 300.0;
/// 正文两栏间距，对应 `--workspace-file-detail-gap`。
const DETAIL_GAP: f32 = 18.0;

/// 预览页。`body` 是预览框里的内容，由调用方按文件类型给出。
pub(super) fn preview_page(model: &ShellViewModel, body: AnyView) -> AnyView {
    let radius = crate::theme_map::radius_2xl(model.admin.corner_radius as f32);
    let stacked = model.viewport_width <= STACK_BREAKPOINT;
    let mut rows = vec![header(model, stacked), main(model, body, stacked)];
    rows.push(super::super::player_view::hosted_bar(model));
    widget(
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
            }),
    )
    .children(rows)
    .key("inspect-preview-page")
    .into_any()
}

/// 页头：返回在左，眉题、文件名和路径在中，打开、定位在右。底边一条 border-soft。
fn header(model: &ShellViewModel, stacked: bool) -> AnyView {
    let path = model.inspect.target_path.clone().unwrap_or_default();
    let path = path.trim().to_string();
    let enabled = !path.is_empty();
    let ctx = super::super::files::FileContext::from_model(model);
    let root = model.workspace.active_repository().map(|item| item.path.clone()).unwrap_or_default();
    let absolute = super::super::input::repository_absolute(&root, &path);
    let has_repo = model.workspace.active_repository().is_some();
    let can_open = enabled && !absolute.trim().is_empty();
    let can_reveal = can_open && !ctx.trash;
    let directory = model.files.current_path.clone();
    let open_path = absolute.clone();
    let reveal_path = absolute;
    let back = widget(action_button("返回", icons::ARROW_LEFT, 15.0, 34.0, !enabled))
        .key("inspect-back")
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::OpenDirectory(directory.clone())));
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0))
        .key("inspect-preview-actions")
        .children((
            widget(action_button("打开", icons::EYE, 14.0, 32.0, !can_open)).key("inspect-open").on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Input(super::super::input::InputMessage::OpenEntry {
                    has_repo,
                    absolute_path: open_path.clone(),
                }));
            }),
            widget(action_button("定位", icons::FOLDER_OPEN, 14.0, 32.0, !can_reveal)).key("inspect-reveal").on_cx(
                move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Input(super::super::input::InputMessage::RevealEntry {
                        absolute_path: reveal_path.clone(),
                    }));
                },
            ),
        ))
        .into_any();
    let titles = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children(title_lines(model, &path))
        .key("inspect-preview-titles")
        .into_any();
    let frame = if stacked { Stack::column(14.0) } else { Stack::bar(14.0).align(AlignSpec::Center) };
    let frame = super::super::workbench::with_bottom_divider(frame.with_layout(|layout| {
        layout.padding_top = Some(LengthSpec::Px(16.0));
        layout.padding_bottom = Some(LengthSpec::Px(16.0));
        layout.padding_left = Some(LengthSpec::Px(18.0));
        layout.padding_right = Some(LengthSpec::Px(18.0));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }));
    let back_cell = widget(Stack::row(0.0).grow(0.0).shrink(0.0)).children((back,)).key("inspect-back-cell").into_any();
    widget(frame).children((back_cell, titles, actions)).key("inspect-preview-header").into_any()
}

/// 眉题「文件预览」、20px 文件名（去掉扩展名）、和文件名不同时的完整路径，以及注释、链接两行导语。
fn title_lines(model: &ShellViewModel, path: &str) -> Vec<AnyView> {
    let name = file_name(path);
    let title = super::super::player_view::bar::display_title(&name, &extension_of(&name));
    let mut lines = vec![
        widget(eyebrow("文件预览")).key("inspect-preview-eyebrow").into_any(),
        widget(spaced(text(&title, 20.0, 700, SemanticColorRole::Text, 31.0).wrap_anywhere(), 4.0))
            .key("inspect-preview-name")
            .into_any(),
    ];
    if !path.is_empty() && path != name {
        lines.push(
            widget(spaced(text(path, 14.0, 400, SemanticColorRole::Muted, 21.7).wrap_anywhere(), 4.0))
                .key("inspect-preview-path")
                .into_any(),
        );
    }
    let comment = model.inspect.draft_comment().trim().to_string();
    let link = model.inspect.draft_link().trim().to_string();
    if comment.is_empty() && link.is_empty() {
        return lines;
    }
    let mut lead = Vec::new();
    for (label, value, key) in [("注释", comment, "inspect-lead-comment"), ("链接", link, "inspect-lead-link")] {
        if value.is_empty() {
            continue;
        }
        lead.push(
            widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
                .children((
                    widget(eyebrow_bold(label)),
                    widget(text(&value, 13.0, 500, SemanticColorRole::Text, 19.5).wrap_anywhere()),
                ))
                .key(key)
                .into_any(),
        );
    }
    lines.push(
        widget(Stack::column(8.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).with_layout(|layout| {
            layout.margin_top = Some(LengthSpec::Px(12.0));
        }))
        .children(lead)
        .key("inspect-preview-lead")
        .into_any(),
    );
    lines
}

/// 正文：左边预览框、色板和资源库面板，右边文件事实卡。单列时事实卡跟在下面。
fn main(model: &ShellViewModel, body: AnyView, stacked: bool) -> AnyView {
    let shell = preview_shell(model, body, stacked);
    let stats = stats(model, stacked);
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
    let content = widget(frame).children((shell, stats)).key("inspect-preview-body").into_any();
    if !stacked {
        return content;
    }
    let scroll_key = content_key(model, "preview-body");
    // 单列时正文整体滚动，事实卡不再限高。
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .children((content,))
    .key(scroll_key)
    .into_any()
}

/// 滚动区按显示的内容取键：换文件时回到顶部，同一个文件重挂时位置保持。
pub(super) fn content_key(model: &ShellViewModel, kind: &str) -> String {
    let repo = super::super::player_view::key_part(model.workspace.active_repo_id.as_deref().unwrap_or_default());
    let path = super::super::player_view::key_part(model.inspect.target_path.as_deref().unwrap_or_default());
    format!("{kind}-{repo}-{path}")
}

/// 预览框（铺满）、色板槽（至少 14px）和资源库扩展的预览面板，行距 10。
fn preview_shell(model: &ShellViewModel, body: AnyView, stacked: bool) -> AnyView {
    let mut rows = vec![body, palette_slot(&model.inspect.palette)];
    if let Some(panels) = super::super::inspect_asmr::preview_panels(model) {
        rows.push(panels);
    }
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
        .children(rows)
        .key("inspect-preview-shell")
        .into_any()
}

/// 预览框：bg 底、12px 圆角。插件预览在顶部多一圈 accent-soft 径向光晕。
pub(super) fn preview_box(content: AnyView, plugin: bool) -> AnyView {
    let mut frame = Stack::fill_column(0.0)
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
    frame = if plugin {
        frame.painter(super::preview_paint::PluginGlow)
    } else {
        frame.surface(SemanticColorRole::Background)
    };
    widget(frame).children((content,)).key("inspect-preview-box").into_any()
}

/// 色板槽：没有颜色时只占 14px；有颜色时是最多五枚 34×12 的胶囊色块，提示写色值。
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

/// 右侧事实卡：bg 底、12px 圆角、14px 内边距，行距 12，内容超高时卡内滚动。
fn stats(model: &ShellViewModel, stacked: bool) -> AnyView {
    let rows = widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children((
        super::super::inspect_library::fact_column(&model.inspect.facts),
        super::super::inspect_metadata_view::metadata_panel(model),
    ));
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
    widget(card.style(style)).children((rows,)).key(content_key(model, "preview-stats")).into_any()
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
