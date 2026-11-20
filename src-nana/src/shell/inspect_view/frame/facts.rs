//! 预览事实与详情卡编辑器的初始数据、列表元数据合并与播放集事实行。

use super::*;

/// 预览右侧的类型、大小、硬链接和修改时间按预览条目填。
///
/// 选中素材只写入素材详情，不改这条预览条目。详情回来以后四行仍是它，
/// 不用素材摘要换掉。当前面板的列表优先，再看目录、智能文件夹和分类视图。都没有时：
/// 正在播放的队列非空就只在队列里找这条；队列是空的才用打开着的播放集页。
/// 播放集条目只有扩展名，大小、硬链接和修改时间留空，不拿详情里的字段补，也不编造。
pub(crate) fn seed_preview_facts(model: &mut ShellViewModel, path: &str) {
    let path = path.trim();
    if path.is_empty() {
        eprintln!("Nana 预览事实没有路径，不从列表填");
        return;
    }
    // 预览条目不叠素材详情。预览页开着时，编辑器四个输入跟这一条列表元数据。
    if super::super::super::files_view::previewing(model) {
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

/// 单击选中时，详情卡编辑器的四个输入跟叠层后的条目。
///
/// 叠层：素材 id 对得上时，详情元数据盖到当前列表条目上，同键以详情为准，
/// 详情没有的键留着列表的。对不上或详情还没回来时只用列表。空 `comment` 退回 `note`。
/// 列表里没有这一条时四个输入留着已经记下的素材详情。正在输入时不盖。预览页开着时不进这里。
pub(crate) fn seed_detail_card_editor(model: &mut ShellViewModel, path: &str) {
    if super::super::super::files_view::previewing(model) {
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
        let ctx = super::super::super::files::FileContext::from_model(model);
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

/// 叠层后的元数据：没有对得上的详情时原样返回列表。
pub(super) fn merged_list_metadata(
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
pub(super) struct ListedFacts {
    pub(super) extension: String,
    pub(super) size_label: String,
    pub(super) modified_at: String,
    pub(super) hardlink_state: Option<String>,
}

/// 列表里这条文件的四行。目录不当成预览事实。当前面板先看，再看另外两份列表。
pub(super) fn listed_facts(model: &ShellViewModel, path: &str) -> Option<ListedFacts> {
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
pub(super) fn list_fact_row<'a>(
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
pub(super) fn playlist_fact_extension(model: &ShellViewModel, path: &str) -> Option<String> {
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
