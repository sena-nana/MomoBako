//! 文件列表右侧的详情卡片，对应 Vue `FileBrowserPanel.vue` 的 `files-detail`。
//!
//! 多选时写「N 个项目」和文件夹 / 文件数；单选时是缩略图、色板、标题、路径、注释和链接、
//! 类型 / 大小 / 硬链接 / 受保护 / 修改时间各行，文件再接元数据编辑；没有选择时是当前目录的
//! 直属计数；回收站和分类视图没有选择时是居中的提示。卡片自己纵向滚动，播放条不会盖住它。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{JustifySpec, LengthSpec, OverflowSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack};

use super::super::files::{hardlink_label, local_time, FileContext, FileRow};
use super::super::ShellViewModel;
use super::style;

/// 详情卡片。`width` 为空时是窄窗里排在文件卡片下面的整行版式。
pub(super) fn detail_aside(model: &ShellViewModel, width: Option<f32>) -> AnyView {
    let (body, centered) = detail_body(model);
    let mut content = Stack::column(14.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).padding(19.0);
    if centered {
        content = content.height(LengthSpec::Fill).justify(JustifySpec::Center);
    }
    let scroller = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .children((widget(content).children((body,)).key("file-detail-content"),))
    .into_any();
    let mut frame = Stack::fill_column(0.0)
        .min_height(LengthSpec::Px(0.0))
        .surface(SemanticColorRole::Surface)
        .radius_px(super::card_radius(model))
        .with_layout(|layout| {
            layout.overflow_x = OverflowSpec::Hidden;
            layout.overflow_y = OverflowSpec::Hidden;
        });
    frame = match width {
        Some(width) => frame.width(LengthSpec::Px(width)).min_width(LengthSpec::Px(width)).grow(0.0).shrink(0.0),
        None => frame.min_width(LengthSpec::Px(0.0)),
    };
    widget(frame).children((scroller,)).key("file-detail").into_any()
}

/// 卡片内容和是否需要上下居中（只有空状态居中）。
fn detail_body(model: &ShellViewModel) -> (AnyView, bool) {
    let ctx = FileContext::from_model(model);
    let rows = model.files.visible_rows(&ctx);
    let chosen = chosen_rows(&rows, &model.files.selected, model.files.primary.as_deref());
    if chosen.len() > 1 {
        return (multi_card(&chosen), false);
    }
    if let Some(row) = current_row(&rows, model, &chosen) {
        return (entry_card(model, &ctx, &row), false);
    }
    if !ctx.trash && !ctx.is_virtual() {
        return (directory_card(model, &rows), false);
    }
    (empty_card(&ctx), true)
}

/// 主选优先；没有主选时取唯一的那条选择。
fn current_row(rows: &[FileRow], model: &ShellViewModel, chosen: &[FileRow]) -> Option<FileRow> {
    model
        .files
        .primary
        .as_ref()
        .and_then(|primary| rows.iter().find(|row| &row.path == primary).cloned())
        .or_else(|| chosen.first().cloned())
}

/// `files-detail__section`：标题 18px/700（上 4），路径 14px 弱化色（上 8），注释和链接（上 12）。
fn section(title: String, subline: Option<String>, lead: Vec<AnyView>, key: &str) -> AnyView {
    let mut rows = vec![widget(style::margin_top(style::heading(title), 4.0)).key(format!("{key}-title")).into_any()];
    if let Some(subline) = subline {
        rows.push(widget(style::margin_top(style::subline(subline), 8.0)).key(format!("{key}-subline")).into_any());
    }
    if !lead.is_empty() {
        rows.push(
            widget(Stack::column(8.0).width(LengthSpec::Fill).with_layout(|layout| layout.margin_top = Some(LengthSpec::Px(12.0))))
                .children(lead)
                .key(format!("{key}-lead"))
                .into_any(),
        );
    }
    widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(rows).key(key.to_string()).into_any()
}

/// 多选：标题写条目数，下面是「N 个文件夹 · N 个文件」。
fn multi_card(chosen: &[FileRow]) -> AnyView {
    let folders = chosen.iter().filter(|row| row.kind == "directory").count();
    let files = chosen.len() - folders;
    let summary = [
        (folders > 0).then(|| format!("{folders} 个文件夹")),
        (files > 0).then(|| format!("{files} 个文件")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    card(vec![section(format!("{} 个项目", chosen.len()), Some(summary), Vec::new(), "file-detail-multi")], "file-detail-card")
}

/// 单个条目：缩略图、色板、标题区、事实行，文件再接元数据编辑。
fn entry_card(model: &ShellViewModel, ctx: &FileContext, row: &FileRow) -> AnyView {
    let entry = model.browser_entries.iter().find(|entry| entry.path == row.path && entry.kind == row.kind);
    let mut parts = vec![detail_preview(row)];
    let palette = if row.palette.is_empty() && model.inspect.target_path.as_deref() == Some(row.path.as_str()) {
        model.inspect.palette.clone()
    } else {
        row.palette.clone()
    };
    if let Some(swatches) = super::super::palette::swatches(&palette, "file-detail-palette") {
        parts.push(swatches);
    }
    let subline = (row.path != row.name && !row.path.is_empty()).then(|| row.path.clone());
    parts.push(section(row.display_title(), subline, lead_rows(row), "file-detail-section"));
    let mut stats = vec![
        fact("类型", type_label(row), "file-detail-type", true),
        fact("大小", if row.size_label.trim().is_empty() { "目录项".into() } else { row.size_label.clone() }, "file-detail-size", false),
    ];
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if !hardlink.is_empty() {
        stats.push(fact("硬链接", hardlink.to_string(), "file-detail-hardlink", false));
    }
    if let Some(folder) = entry.and_then(|entry| entry.folder_metadata.as_ref()).filter(|folder| folder.protected) {
        let tip = folder.password_tip.clone().filter(|tip| !tip.trim().is_empty()).unwrap_or_else(|| "Eagle 迁移提示".into());
        stats.push(fact("受保护", tip, "file-detail-protected", false));
    }
    stats.push(fact("修改时间", local_time::format_or(&row.modified_at, "未记录"), "file-detail-modified", false));
    if ctx.trash {
        let deleted = row.metadata.get("deletedAt").and_then(|value| value.as_str()).unwrap_or("");
        stats.push(fact("删除时间", local_time::format_or(deleted, "未记录"), "file-detail-deleted", false));
    }
    parts.push(widget(Stack::column(12.0).width(LengthSpec::Fill)).children(stats).key("file-detail-stats").into_any());
    if row.kind != "directory" && !ctx.trash {
        parts.push(super::super::inspect_metadata_view::metadata_panel(model));
    }
    card(parts, "file-detail-entry")
}

/// 注释和链接：最弱色小标签在上，13px/500 的值在下。都没有时不占位。
fn lead_rows(row: &FileRow) -> Vec<AnyView> {
    let text = |key: &str| row.metadata.get(key).and_then(|value| value.as_str()).map(str::trim).filter(|value| !value.is_empty());
    let comment = text("comment").or_else(|| text("note"));
    let link = text("link");
    [("注释", comment, "file-detail-comment"), ("链接", link, "file-detail-link")]
        .into_iter()
        .filter_map(|(label, value, key)| {
            let value = value?;
            Some(
                widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
                    .children((
                        widget(style::row_label(label)).key(format!("{key}-label")),
                        widget(style::wrapping(style::text(value, 13.0, 500, SemanticColorRole::Text, 19.5))).key(format!("{key}-value")),
                    ))
                    .key(key)
                    .into_any(),
            )
        })
        .collect()
}

/// 没有选择：当前目录名（根目录写资源库名）、路径，以及直属计数。
fn directory_card(model: &ShellViewModel, rows: &[FileRow]) -> AnyView {
    let folders = rows.iter().filter(|row| row.kind == "directory").count();
    let files = rows.len() - folders;
    let path = model.files.current_path.trim();
    let title = if path.is_empty() {
        let name = model.repository_name.trim();
        if name.is_empty() { "根目录".to_string() } else { name.to_string() }
    } else {
        path.rsplit('/').next().filter(|part| !part.is_empty()).unwrap_or(path).to_string()
    };
    let subline = if path.is_empty() { "根目录".to_string() } else { path.to_string() };
    let stats = widget(Stack::column(12.0).width(LengthSpec::Fill))
        .children((
            fact("直属文件", files.to_string(), "file-detail-files", true),
            fact("直属子文件夹", folders.to_string(), "file-detail-folders", false),
            fact("当前视图总条目", rows.len().to_string(), "file-detail-total", false),
        ))
        .key("file-detail-stats")
        .into_any();
    card(vec![section(title, Some(subline), Vec::new(), "file-detail-directory"), stats], "file-detail-card")
}

/// 回收站或分类视图里还没有选择：眉题、标题和下一步提示，上下居中。
fn empty_card(ctx: &FileContext) -> AnyView {
    let eyebrow = if ctx.smart_folder {
        "智能文件夹"
    } else if ctx.trash {
        "回收站"
    } else {
        "文件管理"
    };
    let message = if ctx.trash && !ctx.smart_folder {
        "在中间列表中选择目标，然后可执行还原或彻底删除。"
    } else {
        "在中间列表中选择目标查看详情。"
    };
    widget(Stack::column(0.0).width(LengthSpec::Fill))
        .children((
            widget(style::eyebrow(eyebrow)).key("file-detail-empty-eyebrow"),
            widget(style::margin_top(style::heading("选择一个文件或文件夹"), 4.0)).key("file-detail-empty-title"),
            widget(style::margin_top(style::subline(message), 8.0)).key("file-detail-empty-copy"),
        ))
        .key("file-detail-empty")
        .into_any()
}

/// `files-detail__card`：各块之间 16。
fn card(parts: Vec<AnyView>, key: &'static str) -> AnyView {
    widget(Stack::column(16.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(parts).key(key).into_any()
}

/// `files-detail__preview`：至少 160 高的 `Xl` 圆角盒。文件夹是棕色渐变，文件是占位灰；
/// 有缩略图时 cover 铺满，没有时居中画 34px 白色类型图标。
fn detail_preview(row: &FileRow) -> AnyView {
    widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((super::cards::preview_fill(row, 160.0, 34.0, RadiusTier::Xl),))
        .key("file-detail-preview")
        .into_any()
}

/// `asset-meta__row`：小标签和值。`first` 是组里的第一行，不画上边线。
pub(super) fn fact(label: &str, value: String, key: &str, first: bool) -> AnyView {
    widget(style::meta_row(first))
        .children((
            widget(style::row_label(label)).key(format!("{key}-label")),
            widget(style::value(value)).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

fn type_label(row: &FileRow) -> String {
    if row.kind == "directory" {
        "文件夹".into()
    } else {
        row.extension.as_deref().map(str::trim).filter(|text| !text.is_empty()).unwrap_or("文件").to_string()
    }
}

fn chosen_rows(rows: &[FileRow], selected: &[String], primary: Option<&str>) -> Vec<FileRow> {
    rows.iter()
        .filter(|row| selected.iter().any(|item| item == &row.path) || primary == Some(row.path.as_str()))
        .cloned()
        .collect()
}
