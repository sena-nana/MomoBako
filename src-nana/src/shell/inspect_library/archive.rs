//! 压缩包舞台：目录标签、条目请求、列表与工具栏、分栏、面包屑、行与图标，以及压缩包目录解析。

use super::*;

/// 右边舞台上的提示，分空闲、准备、不能内联和失败四种。
pub(in super::super) struct ArchiveStage<'a> {
    pub title: &'a str,
    pub detail: &'a str,
    /// 失败标题用危险色。其余标题是正文色。
    pub failed: bool,
}

/// 当前名单该写的提示。正在取字节时优先写「准备内部文件」，不看上一次的失败。
pub(in super::super) fn archive_stage(inspect: &InspectState) -> (&str, &str, bool) {
    if let Some(path) = inspect.archive_request.as_deref().map(request_entry) {
        return ("准备内部文件", path, false);
    }
    match inspect.archive_notice.as_ref() {
        Some(ArchiveFileNotice::Inline(name)) => (name.as_str(), "无法内联预览该文件类型", false),
        Some(ArchiveFileNotice::Failed(message)) => ("无法预览内部文件", message.as_str(), true),
        None => (directory_label(inspect), "选择内部文件进行预览", false),
    }
}

/// 当前层的目录名。根上写「根目录」。
pub(super) fn directory_label(inspect: &InspectState) -> &str {
    let directory = match inspect.archive_nests.last() {
        Some(nest) => nest.directory.as_str(),
        None => inspect.archive_directory.as_str(),
    };
    if directory.is_empty() {
        "根目录"
    } else {
        directory
    }
}

/// 在途请求键的最后一段是这一条在当前层里的路径。
pub(super) fn request_entry(key: &str) -> &str {
    key.rsplit('\u{1}')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(key)
}

/// 把压缩包摘要画成计数和条目。目录行单击只选中，连点才进入；文件行单击打开这一条。面包屑回到上层。
/// 没有条目时仍写「0 项」和「空目录」，工具条照常按条目数计数，不另编一句说明。
/// 名单在左、舞台在右。`opened` 有值时舞台是这一条的预览；
/// 否则 `stage` 写空闲、准备、不能内联或失败。
pub(in super::super) fn archive_list(
    content: &str,
    picked: Option<&str>,
    stage: ArchiveStage,
    archive_name: &str,
    opened: Option<AnyView>,
) -> AnyView {
    let parsed = parse_archive(content);
    let count = parsed
        .rows
        .iter()
        .filter(|row| !matches!(row, ArchiveRow::Ellipsis))
        .count();
    let directory = parsed.directory.clone().unwrap_or_default();
    // 有层目录头，或整包空到解析不出目录头，都画出面包屑。rar 的整路径列表没有目录头、但有文件行，不在这里。
    let crumbs = (parsed.directory.is_some() || parsed.rows.is_empty())
        .then(|| archive_breadcrumbs(&directory, archive_name));
    let list = if parsed.rows.is_empty() {
        widget(EmptyState::new("空目录").compact(true))
            .key("inspect-archive-empty")
            .into_any()
    } else {
        let open_files = parsed.directory.is_some();
        let items = parsed
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| archive_row(index, row, &directory, open_files, picked))
            .collect::<Vec<_>>();
        widget(List::new())
            .children(items)
            .key("inspect-archive-list")
            .into_any()
    };
    let stage = match opened {
        Some(view) => view,
        None => archive_stage_view(stage),
    };
    let mut rows = Vec::new();
    match crumbs {
        Some(crumbs) => rows.push(archive_toolbar(crumbs, &format!("{count} 项"))),
        None => rows.push(
            text(format!("{count} 项"))
                .key("inspect-archive-count")
                .into_any(),
        ),
    }
    rows.push(archive_split(list, stage));
    widget(
        Stack::fill_column(8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children(rows)
    .key("inspect-archive")
    .into_any()
}

/// 工具条：面包屑在左，项数在右，计数和路径分开。
pub(super) fn archive_toolbar(crumbs: AnyView, count: &str) -> AnyView {
    widget(
        Stack::row(12.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::SpaceBetween)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        crumbs,
        text(count.to_string())
            .key("inspect-archive-count")
            .into_any(),
    ))
    .key("inspect-archive-toolbar")
    .into_any()
}

/// 左边名单、右边舞台。宽度按 `minmax(220px, 0.42fr)` 和 `minmax(260px, 1fr)` 分。
pub(super) fn archive_split(list: AnyView, stage: AnyView) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.align_items = AlignSpec::Stretch;
            }),
    )
    .children((
        archive_pane(list, 0.42, "inspect-archive-list-pane"),
        archive_pane(stage, 1.0, "inspect-archive-stage-pane"),
    ))
    .key("inspect-archive-body")
    .into_any()
}

/// 并排的一列。`grow` 是剩余宽度里的份数，窄的时候允许缩到 0，避免被挤成上下两行。
pub(super) fn archive_pane(child: AnyView, grow: f32, key: &str) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_grow = Some(grow);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((child,))
    .key(key.to_string())
    .into_any()
}

/// 右边舞台上的提示。失败标题是危险色，其余是正文色，说明是弱色。
pub(super) fn archive_stage_view(stage: ArchiveStage) -> AnyView {
    let title_role = if stage.failed {
        SemanticColorRole::Danger
    } else {
        SemanticColorRole::Text
    };
    let mut title = typed_text(Text::new(stage.title), 13.0, TextWeight::Semibold)
        .color(title_role)
        .line_height(20.15);
    title.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut detail = typed_text(Text::new(stage.detail), 12.0, TextWeight::Regular)
        .color(SemanticColorRole::Muted)
        .line_height(18.6);
    detail.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::column(6.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .min_height(LengthSpec::Px(120.0)),
    )
    .children((
        widget(title).key("inspect-archive-stage-title"),
        widget(detail).key("inspect-archive-stage-detail"),
    ))
    .key("inspect-archive-stage")
    .into_any()
}

/// 面包屑：第一段是带扩展名的文件名，后面是已经进入的目录名。当前段不可再点。
pub(super) fn archive_breadcrumbs(directory: &str, archive_name: &str) -> AnyView {
    let mut path = String::new();
    let root = archive_root_label(archive_name);
    let mut crumbs = vec![archive_crumb(
        root,
        String::new(),
        directory.is_empty(),
        "inspect-archive-crumb-root",
    )];
    if !directory.is_empty() {
        let parts: Vec<&str> = directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let last = parts.len().saturating_sub(1);
        for (index, part) in parts.into_iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            crumbs.push(archive_crumb(
                part,
                path.clone(),
                index == last,
                &format!("inspect-archive-crumb-{index}"),
            ));
        }
    }
    widget(Stack::row(4.0).align(AlignSpec::Center))
        .children(crumbs)
        .key("inspect-archive-crumbs")
        .into_any()
}

/// 面包屑第一段：带扩展名的文件名，也就是路径最后一段。空白或没有名字时仍写「压缩包」，不编一个假名字。
pub(super) fn archive_root_label(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() { "压缩包" } else { name }
}

pub(super) fn archive_crumb(label: &str, path: String, current: bool, key: &str) -> AnyView {
    widget(Button::new(label).kind(ButtonKind::Ghost).disabled(current))
        .key(key.to_string())
        .on_cx(move |_, _: &Activate, cx| {
            if !current {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::EnterArchive(
                    path.clone(),
                )));
            }
        })
        .into_any()
}

pub(super) fn archive_row(
    index: usize,
    row: &ArchiveRow,
    directory: &str,
    open_files: bool,
    picked: Option<&str>,
) -> AnyView {
    let key = format!("inspect-archive-row-{index}");
    match row {
        ArchiveRow::Dir(name) => {
            let next = if directory.is_empty() {
                name.clone()
            } else {
                format!("{directory}/{name}")
            };
            let mut item = ListItem::new(name.clone())
                .detail("目录")
                .role(ListItemRole::Button);
            if picked == Some(next.as_str()) {
                item = item.selected(true);
            }
            widget(item)
                .key(key.clone())
                .leading(archive_kind_mark("DIR", &archive_kind_key(&key)))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Inspect(archive_directory_activation(
                        &next,
                    )));
                })
                .into_any()
        }
        ArchiveRow::File(name, note) => {
            archive_file_row(key, name, note, directory, open_files, picked)
        }
        ArchiveRow::Ellipsis => widget(ListItem::new("…").read_only(true))
            .key(key)
            .into_any(),
    }
}

/// zip/7z 的文件行单击就打开，点开的这一行标成当前项。
/// rar 的整路径列表没有目录头，行只显示，不接指针也不进 Tab。行首种类不改大小。
pub(super) fn archive_file_row(
    key: String,
    name: &str,
    note: &str,
    directory: &str,
    open_files: bool,
    picked: Option<&str>,
) -> AnyView {
    let kind_key = archive_kind_key(&key);
    let mut item = ListItem::new(name.to_string()).detail(note.to_string());
    if !open_files {
        return widget(item.read_only(true))
            .key(key)
            .leading(archive_kind_mark(&file_kind_label(name), &kind_key))
            .into_any();
    }
    let full = if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    };
    if picked == Some(full.as_str()) {
        item = item.selected(true);
    }
    widget(item.role(ListItemRole::Button))
        .key(key)
        .leading(archive_kind_mark(&file_kind_label(name), &kind_key))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::OpenArchiveFile(
                full.clone(),
            )));
        })
        .into_any()
}

/// 行首种类的键，跟在对应名单行后面。
pub(super) fn archive_kind_key(row_key: &str) -> String {
    row_key.replacen("inspect-archive-row-", "inspect-archive-kind-", 1)
}

/// 名单行首的小字。目录固定写「DIR」；文件走 [`file_kind_label`]。
pub(super) fn archive_kind_mark(label: &str, key: &str) -> AnyView {
    let mark = crate::theme_map::typed_text(Text::new(label.to_string()), 11.0, TextWeight::Bold)
        .color(SemanticColorRole::Muted);
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(30.0))
            .min_width(LengthSpec::Px(30.0)),
    )
    .children((widget(mark).key(key.to_string()),))
    .into_any()
}

/// 行首种类：扩展名前四位转成大写。空扩展名、只有前导点、或点后面是空的，都写「FILE」。
pub(super) fn file_kind_label(name: &str) -> String {
    let extension = path_extension(name);
    let raw = if extension.is_empty() {
        "FILE"
    } else {
        extension
    };
    raw.chars().take(4).collect::<String>().to_ascii_uppercase()
}

/// 文件名的扩展名，规则和 `Path::extension` 一样，只看最后一段。
pub(super) fn path_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => extension,
        _ => "",
    }
}

pub(super) enum ArchiveRow {
    Dir(String),
    File(String, String),
    Ellipsis,
}

pub(super) struct ParsedArchive {
    /// `Some` 表示 zip/7z 的当前目录，空字符串是根。`None` 是 rar 的整路径列表或空包。
    pub(super) directory: Option<String>,
    pub(super) rows: Vec<ArchiveRow>,
}

/// 头部 `压缩包 / 路径 · N 项` 是当前层。rar 用「个文件」，没有目录行。
pub(super) fn parse_archive(content: &str) -> ParsedArchive {
    let mut directory = None;
    let mut rows = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(path) = layered_directory(line) {
            directory = Some(path);
            continue;
        }
        if line.starts_with("压缩包") {
            continue;
        }
        if line == "…" {
            rows.push(ArchiveRow::Ellipsis);
            continue;
        }
        let (name, note) = line.rsplit_once(" · ").unwrap_or((line, ""));
        if note == "目录" {
            rows.push(ArchiveRow::Dir(name.to_string()));
        } else {
            rows.push(ArchiveRow::File(name.to_string(), note.to_string()));
        }
    }
    ParsedArchive { directory, rows }
}

pub(super) fn layered_directory(line: &str) -> Option<String> {
    let (head, tail) = line.rsplit_once(" · ")?;
    let count = tail.strip_suffix(" 项")?;
    if count.is_empty() || !count.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if let Some(path) = head.strip_prefix("压缩包 / ") {
        Some(path.to_string())
    } else if head == "压缩包" {
        Some(String::new())
    } else {
        None
    }
}
