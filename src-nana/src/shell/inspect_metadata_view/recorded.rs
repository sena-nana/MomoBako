//! 元数据面板的「记录信息」：创建时间、修改时间、尺寸、大小、多归属位置和来源链接这几行，以及
//! 它们读的那份元数据（详情卡、列表行或播放集条目）。

use super::*;

/// 多归属位置：去掉和当前路径相同的位置，用「，」连起来。
/// 读列表行上保存的那一份。继续加载会换掉上一页目录快照，智能文件夹结果也不在那一页里。
pub(super) fn alias_locations(row: &FileRow) -> String {
    row.alias_paths
        .iter()
        .filter(|path| path.as_str() != row.path)
        .cloned()
        .collect::<Vec<_>>()
        .join("，")
}

/// 添加到资源库到来源链接的只读行。第一行不画上边线；没有内容的行不占位。
///
/// 读 [`recorded_metadata`]：详情卡叠对得上的素材详情，预览页只用列表或播放集条目。
/// 文件修改时间在元数据里没有时退回这一条自己的 `modifiedAt`，不用素材摘要上的修改时间。
pub(super) fn recorded_view(model: &ShellViewModel, row: Option<&FileRow>) -> RecordedView {
    let (metadata, modified_at) = recorded_metadata(model, row);
    let text = |key: &str| metadata_string(&metadata, key).trim().to_string();
    let file_modified = {
        let meta = metadata_string(&metadata, "fileModifiedAt");
        if meta.is_empty() { modified_at } else { meta }
    };
    let width = raw_number(&metadata, "width");
    let height = raw_number(&metadata, "height");
    let dimensions = match (width, height) {
        (Some(width), Some(height)) if width > 0.0 && height > 0.0 => {
            format!("{} × {}", plain(width), plain(height))
        }
        _ => "未记录".into(),
    };
    RecordedView {
        added: local_time::format_or(&text("addedToLibraryAt"), "未记录"),
        created: local_time::format_or(&text("fileCreatedAt"), "未记录"),
        file_modified: local_time::format_or(&file_modified, "未记录"),
        dimensions,
        original: super::super::inspect_library::format_original_size(raw_number(
            &metadata,
            "originalSizeBytes",
        )),
        // 详情元数据优先覆盖列表；没有详情时才回退到列表行持久色板。
        // 预览取样色板只存于 inspect，不进入此行。
        palette: {
            let detail_palette = super::super::palette::from_metadata_map(&metadata);
            if !detail_palette.is_empty() {
                detail_palette
            } else {
                row.filter(|row| !row.palette.is_empty())
                    .map(|row| row.palette.clone())
                    .unwrap_or_default()
            }
        },
        indexed_tags: row.map(|row| row.tags.join("，")).unwrap_or_default(),
        aliases: row.map(alias_locations).unwrap_or_default(),
        origin_title: text("originTitle"),
        source_url: text("sourceUrl"),
    }
}

/// 只读行用的元数据和修改时间。元数据见 [`card_entry_metadata`]。
/// 文件修改时间在元数据里没有时退回这一条自己的 `modifiedAt`。
pub(super) fn recorded_metadata(
    model: &ShellViewModel,
    row: Option<&FileRow>,
) -> (BTreeMap<String, Value>, String) {
    if !super::super::files_view::previewing(model) && row.is_none() {
        eprintln!("Nana 详情卡只读行找不到列表条目，不拿素材详情硬填");
    }
    (
        card_entry_metadata(model, row),
        row.map(|row| row.modified_at.clone()).unwrap_or_default(),
    )
}

/// 详情卡作品信息、自定义行和只读行共用的元数据。封面按钮也读这一份。
///
/// 预览页：当前列表优先，没有再看播放集条目，不叠素材详情，也不读编辑草稿。
/// 详情卡：素材 id 对得上时详情盖到列表上，同键以详情为准；
/// 对不上或还没读回时只用列表。没有的键不补。
pub(super) fn card_entry_metadata(
    model: &ShellViewModel,
    row: Option<&FileRow>,
) -> BTreeMap<String, Value> {
    if super::super::files_view::previewing(model) {
        if let Some(row) = row {
            return row.metadata.clone();
        }
        let path = model.inspect.target_path.as_deref().unwrap_or("");
        return playlist_entry_metadata(model, path);
    }
    let Some(row) = row else {
        return BTreeMap::new();
    };
    let mut metadata = row.metadata.clone();
    if let Some(overlay) = model.files.detail_metadata(row.asset_id.as_deref()) {
        for (key, value) in overlay {
            metadata.insert(key.clone(), value.clone());
        }
    }
    metadata
}

/// 作品信息上的封面按钮要读的那一份元数据，和 [`card_entry_metadata`] 同一条。
pub(in crate::shell) fn current_card_metadata(model: &ShellViewModel) -> BTreeMap<String, Value> {
    card_entry_metadata(model, target_row(model))
}

/// 播放集条目自己的元数据。正在放的是另一张时，不用打开着的这一页，也不拿素材详情补。
pub(super) fn playlist_entry_metadata(
    model: &ShellViewModel,
    path: &str,
) -> BTreeMap<String, Value> {
    let listed = model.player.listed.as_ref();
    let playing = model
        .player
        .queue
        .iter()
        .find(|item| !item.transient)
        .map(|item| item.playlist_id.as_str());
    let page = match (playing, listed) {
        (Some(playing), Some(page)) if page.playlist.playlist_id != playing => {
            eprintln!("Nana 预览只读行不拿另一张播放集的元数据：{path}");
            return BTreeMap::new();
        }
        (_, Some(page)) => page,
        (Some(_), None) => {
            eprintln!("Nana 正在播放的播放集没有条目元数据，只读行不拿素材详情补：{path}");
            return BTreeMap::new();
        }
        (None, None) => return BTreeMap::new(),
    };
    let Some(item) = page.items.iter().find(|item| item.path == path) else {
        return BTreeMap::new();
    };
    match item.metadata.as_ref() {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        Some(other) => {
            eprintln!("Nana 播放集条目的元数据不是对象，只读行不采用：{path}：{other}");
            BTreeMap::new()
        }
        None => BTreeMap::new(),
    }
}

/// 取字符串：只有字符串算数，别的类型当成空，不退回另一份元数据。
pub(super) fn metadata_string(metadata: &BTreeMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(other) => {
            eprintln!("Nana 元数据 {key} 不是字符串：{other}");
            String::new()
        }
        None => String::new(),
    }
}

/// 取数字：只认有限数字。键在但类型不对时记日志，不当成列表上的旧值。
pub(super) fn raw_number(metadata: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    match metadata.get(key) {
        Some(Value::Number(number)) => {
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .or_else(|| {
                    eprintln!("Nana 元数据 {key} 不是有限数字");
                    None
                })
        }
        Some(other) => {
            eprintln!("Nana 元数据 {key} 不是数字：{other}");
            None
        }
        None => None,
    }
}

/// 测试读当前文件的只读行。
#[cfg(test)]
pub(in crate::shell) fn recorded_rows(model: &ShellViewModel) -> RecordedView {
    recorded_view(model, target_row(model))
}

/// 只读行：五个事实一直在，调色板、索引标签、多归属位置、来源标题和来源链接有内容才占位。
pub(super) fn recorded_grid(recorded: Signal<RecordedView>) -> AnyView {
    let read = move |pick: fn(&RecordedView) -> String| move || recorded.with(|view| pick(view));
    let shown = move |pick: fn(&RecordedView) -> bool| move || recorded.with(|view| pick(view));
    let palette = dynamic(
        move || recorded.with(|view| view.palette.clone()),
        |colors: &Vec<String>| {
            super::super::palette::metadata_chips(colors, "inspect-metadata-palette")
                .unwrap_or_else(|| ().into_any())
        },
    );
    let always = || true;
    let rows = (
        fact_row(
            "添加到资源库",
            read(|view| view.added.clone()),
            "inspect-recorded-added",
            true,
            always,
        ),
        fact_row(
            "创建时间",
            read(|view| view.created.clone()),
            "inspect-recorded-created",
            false,
            always,
        ),
        fact_row(
            "文件修改时间",
            read(|view| view.file_modified.clone()),
            "inspect-recorded-modified",
            false,
            always,
        ),
        fact_row(
            "尺寸",
            read(|view| view.dimensions.clone()),
            "inspect-recorded-size",
            false,
            always,
        ),
        fact_row(
            "原始大小",
            read(|view| view.original.clone()),
            "inspect-recorded-bytes",
            false,
            always,
        ),
        widget(style::meta_row(false))
            .visible(shown(|view| style::any_color(&view.palette)))
            .children((
                widget(style::row_label("调色板")).key("inspect-palette-label"),
                palette,
            ))
            .key("inspect-palette"),
        fact_row(
            "索引标签",
            read(|view| view.indexed_tags.clone()),
            "inspect-indexed-tags",
            false,
            shown(|view| !view.indexed_tags.is_empty()),
        ),
        right_fact(
            "多归属位置",
            read(|view| view.aliases.clone()),
            "inspect-alias-paths",
            shown(|view| !view.aliases.is_empty()),
        ),
        right_fact(
            "来源标题",
            read(|view| view.origin_title.clone()),
            "inspect-origin-title",
            shown(|view| !view.origin_title.is_empty()),
        ),
        source_link_row(recorded, shown(|view| !view.source_url.is_empty())),
    );
    widget(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children(rows)
    .key("inspect-recorded")
    .into_any()
}

/// 小标签和值的一行，和详情事实行同一种画法。值是常量。
pub(super) fn fact(label: &str, value: String, key: &str, first: bool) -> AnyView {
    widget(style::meta_row(first))
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(style::value(value)).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 值右对齐的一行：多归属位置和来源标题。没有内容时不占位。
pub(super) fn right_fact(
    label: &str,
    value: impl Fn() -> String + Send + Clone + 'static,
    key: &str,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let mut node = style::value(nana_ui::runtime::view::untrack(&value));
    node.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(node)
                .prop::<String, fields::text::value>(value)
                .key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 来源链接：右对齐的地址，后面是「打开链接」「复制链接」两个 26 方的描边按钮。
/// 悬停提示就是按钮名。只有 http、https、mailto、tel 才能打开（`host_api::openable_url`）。
/// 点下时现读地址。没有地址时不占位。
pub(super) fn source_link_row(
    recorded: Signal<RecordedView>,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let url = move || recorded.with(|view| view.source_url.clone());
    let mut value = style::value(recorded.with_untracked(|view| view.source_url.clone()));
    value.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    let openable = move || recorded.with(|view| openable_url(&view.source_url));
    let actions = widget(Stack::row(4.0).grow(0.0).shrink(0.0))
        .key("inspect-source-actions")
        .children((
            source_action(EXTERNAL_LINK, "inspect-source-open", "打开链接")
                .prop::<bool, fields::icon_button::disabled>(move || !openable())
                .on_cx(move |_, _: &Activate, cx| {
                    let url = recorded.with_untracked(|view| view.source_url.clone());
                    if openable_url(&url) {
                        cx.dispatch_program_all(ShellMessage::Input(
                            InputMessage::OpenExternalUrl { url },
                        ));
                    } else {
                        eprintln!("Nana 来源链接不能打开：{url}");
                    }
                }),
            source_action(COPY, "inspect-source-copy", "复制链接").on_cx(
                move |_, _: &Activate, cx| {
                    let url = recorded.with_untracked(|view| view.source_url.clone());
                    if url.is_empty() {
                        eprintln!("Nana 没有可复制的来源链接");
                        return;
                    }
                    cx.dispatch_program_all(ShellMessage::Files(FilesMessage::CopyText(url)));
                },
            ),
        ));
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label("来源链接")).key("inspect-source-label"),
            widget(
                Stack::row(8.0)
                    .align(AlignSpec::Start)
                    .justify(JustifySpec::End)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children((
                widget(value)
                    .prop::<String, fields::text::value>(url)
                    .key("inspect-source-value"),
                actions,
            ))
            .key("inspect-source-body"),
        ))
        .key("inspect-source")
        .into_any()
}

/// 来源链接旁的 26 方按钮。`title` 是无障碍名和悬停提示。
///
/// 平时弱色图标、`--bg` 底、`--border-soft` 细边。悬停时图标改正文色、边框改强调色。
/// 打不开的链接停用时，图标和边框按 0.45 盖在按钮底色上，
/// 深浅等同整枚 `opacity: 0.45`，但不再整枚淡一层。
pub(super) fn source_action(icon: Icon, key: &str, title: &'static str) -> El<IconButton> {
    let mut button = IconButton::new(icon, title)
        .with_tooltip(title)
        .colors_from_style();
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.background = Some(SemanticColorRole::Background);
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Sm);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style = button
        .style
        .clone()
        .border(SemanticColorRole::BorderSoft, 1.0);
    let hover = SemanticPaint {
        foreground: Some(SemanticColorRole::Text),
        border: Some(SemanticColorRole::Accent),
        background: Some(SemanticColorRole::Background),
        ..SemanticPaint::default()
    };
    button.style.interaction.hovered = hover;
    button.style.interaction.pressed = hover;
    button.style.interaction.disabled = SemanticPaint {
        foreground_mix: Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Muted,
            SemanticColorRole::Background,
            crate::theme_map::DISABLED_OPACITY,
        )),
        border_mix: Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::BorderSoft,
            SemanticColorRole::Background,
            crate::theme_map::DISABLED_OPACITY,
        )),
        background: Some(SemanticColorRole::Background),
        ..SemanticPaint::default()
    };
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(26.0));
    layout.height = Some(LengthSpec::Px(26.0));
    layout.min_width = Some(LengthSpec::Px(26.0));
    layout.min_height = Some(LengthSpec::Px(26.0));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    widget(button).key(key.to_string())
}
