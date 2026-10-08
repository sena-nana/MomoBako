//! 文件列表右侧详情。
//!
//! 对应 Vue `FileBrowserPanel` 的 `files-detail`。
//! 未选中时写直属文件、直属子文件夹和当前视图总条目。
//! 选中单文件时画已有页图或类型图标，并接上现有元数据编辑。

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{LengthSpec, ScrollAxes, ScrollView, Stack};

use super::super::files::{hardlink_label, FileContext, FileRow};
use super::super::ShellViewModel;

/// 列表主区右侧。预览页自己有一列，这里只在目录列表上出现。
pub(super) fn detail_aside(model: &ShellViewModel) -> AnyView {
    let body = detail_body(model);
    // 高度跟左列对齐。内容超出时只滚详情，播放条不再盖住标签。
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Px(300.0));
        layout.min_width = Some(LengthSpec::Px(280.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.height = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.align_self = Some(nana_ui::runtime::AlignSpec::Stretch);
    }))
    .children((body,))
    .key("file-detail")
    .into_any()
}

fn detail_body(model: &ShellViewModel) -> AnyView {
    let ctx = FileContext::from_model(model);
    let rows = model.files.visible_rows(&ctx);
    let chosen = chosen_rows(&rows, &model.files.selected, model.files.primary.as_deref());
    let card = if chosen.len() > 1 {
        multi_card(chosen.len())
    } else if let Some(row) = chosen.first() {
        entry_card(model, &ctx, row)
    } else if ctx.is_virtual() || ctx.trash {
        empty_card(&ctx)
    } else {
        directory_card(model, &rows)
    };
    widget(Stack::column(4.0).padding_xy(8.0, 4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((card,))
        .into_any()
}

/// 直属计数按当前视图里已经列出的行，不含还没加载的下一页。
fn directory_card(model: &ShellViewModel, rows: &[FileRow]) -> AnyView {
    let folders = rows.iter().filter(|row| row.kind == "directory").count();
    let files = rows.len().saturating_sub(folders);
    let total = files + folders;
    widget(Stack::column(12.0)).children((
        widget(super::super::workbench::page_title(directory_title(model))).key("file-detail-title"),
        widget(super::super::workbench::meta(directory_subline(&model.files.current_path))).key("file-detail-path"),
        stat("直属文件", files.to_string(), "file-detail-files", false),
        stat("直属子文件夹", folders.to_string(), "file-detail-folders", true),
        stat("当前视图总条目", total.to_string(), "file-detail-total", true),
    )).into_any()
}

fn entry_card(model: &ShellViewModel, ctx: &FileContext, row: &FileRow) -> AnyView {
    let facts = (model.inspect.target_path.as_deref() == Some(row.path.as_str())).then_some(&model.inspect.facts);
    let mut rows = vec![
        detail_preview(row),
        widget(super::super::workbench::page_title(row.name.clone())).key("file-detail-name").into_any(),
    ];
    if row.path != row.name && !row.path.is_empty() {
        rows.push(widget(super::super::workbench::meta(row.path.clone())).key("file-detail-entry-path").into_any());
    }
    rows.push(stat("类型", type_label(row), "file-detail-type", false));
    rows.push(stat("大小", size_text(row, facts), "file-detail-size", true));
    let hardlink = hardlink_label(row.hardlink_state.as_deref());
    if !hardlink.is_empty() {
        rows.push(stat("硬链接", hardlink.to_string(), "file-detail-hardlink", true));
    }
    rows.push(stat("修改时间", modified_text(row, facts), "file-detail-modified", true));
    if row.kind != "directory" && !ctx.trash && model.inspect.target_path.as_deref() == Some(row.path.as_str()) {
        rows.push(super::super::inspect_view::metadata_panel(model));
    }
    widget(Stack::column(4.0)).children(rows).key("file-detail-entry").into_any()
}

fn multi_card(count: usize) -> AnyView {
    widget(Stack::column(8.0)).children((
        widget(super::super::workbench::page_title(format!("{count} 个项目"))).key("file-detail-multi"),
        widget(super::super::workbench::meta("在中间列表中选择一个目标查看详情。")).key("file-detail-multi-hint"),
    )).into_any()
}

fn empty_card(ctx: &FileContext) -> AnyView {
    let eyebrow = if ctx.smart_folder {
        "智能文件夹"
    } else if ctx.trash {
        "回收站"
    } else {
        "文件管理"
    };
    let message = if ctx.trash {
        "在中间列表中选择目标，然后可执行还原或彻底删除。"
    } else {
        "在中间列表中选择目标查看详情。"
    };
    widget(Stack::column(8.0)).children((
        widget(super::super::workbench::eyebrow(eyebrow)).key("file-detail-empty-eyebrow"),
        widget(super::super::workbench::page_title("选择一个文件或文件夹")).key("file-detail-empty-title"),
        widget(super::super::workbench::meta(message)).key("file-detail-empty-copy"),
    )).into_any()
}

/// 有整页像素时缩进预览盒，页边留在画面里。否则沿用缩略图；没有像素时只留类型图标。
fn detail_preview(row: &FileRow) -> AnyView {
    let child = if let Some((width, height, rgba)) = row.page_rgba.as_ref() {
        super::rgba_preview("file-detail-page", 268.0, 160.0, *width, *height, rgba, false)
            .unwrap_or_else(|| super::thumbnail(row, 268.0, 120.0, false))
    } else {
        let (width, height) = if row.thumbnail_rgba.is_some() { (268.0, 160.0) } else { (268.0, 120.0) };
        super::thumbnail(row, width, height, false)
    };
    widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((child,))
        .key("file-detail-preview")
        .into_any()
}

fn chosen_rows(rows: &[FileRow], selected: &[String], primary: Option<&str>) -> Vec<FileRow> {
    let mut paths = selected.to_vec();
    if let Some(primary) = primary {
        if !paths.iter().any(|item| item == primary) {
            paths.push(primary.to_string());
        }
    }
    rows.iter().filter(|row| paths.iter().any(|item| item == &row.path)).cloned().collect()
}

fn directory_title(model: &ShellViewModel) -> String {
    let path = model.files.current_path.trim();
    if path.is_empty() {
        let name = model.repository_name.trim();
        if name.is_empty() { "根目录".into() } else { name.to_string() }
    } else {
        path.rsplit('/').next().filter(|part| !part.is_empty()).unwrap_or(path).to_string()
    }
}

fn directory_subline(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() { "根目录".into() } else { path.to_string() }
}

fn type_label(row: &FileRow) -> String {
    if row.kind == "directory" {
        "文件夹".into()
    } else {
        row.extension.as_deref().map(str::trim).filter(|text| !text.is_empty()).unwrap_or("文件").to_string()
    }
}

fn size_text(row: &FileRow, facts: Option<&super::super::inspect::FileFacts>) -> String {
    if let Some(label) = facts.map(|item| item.size_label.trim()).filter(|text| !text.is_empty()) {
        return label.to_string();
    }
    if !row.size_label.trim().is_empty() {
        return row.size_label.clone();
    }
    "目录项".into()
}

fn modified_text(row: &FileRow, facts: Option<&super::super::inspect::FileFacts>) -> String {
    if let Some(label) = facts.map(|item| item.modified_at.trim()).filter(|text| !text.is_empty()) {
        return label.to_string();
    }
    if !row.modified_at.trim().is_empty() {
        return row.modified_at.clone();
    }
    "未记录".into()
}

/// 和 Vue `asset-meta__row` 一样：标签在上，数值在下。除第一行外加一条上边分割。
fn stat(label: &str, value: String, key: &'static str, divided: bool) -> AnyView {
    let mut column = Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    if divided {
        column = super::super::workbench::with_top_divider(column);
    }
    widget(column)
        .key(key)
        .children((
            widget(super::super::workbench::eyebrow(label)).key(format!("{key}-label")),
            text(value).key(format!("{key}-value")),
        ))
        .into_any()
}
