//! 搜索与筛选的对照场景：搜索面板和资源筛选栏。
//!
//! 场景名和 `tmp/vue-mock/scenes/search.ts` 的 Vue 场景同名，数据和 Vue 夹具保持一致：
//! 仓库「默认资源库」，根目录有 `assets/`、`cover.png` 和 `notes/page.pdf`。
//! 带标签的夹具里 cover.png 是「封面、参考」、红色横版，page.pdf 是「文档」、#3fa796 竖版；
//! 搜索固定命中 cover.png，元数据带 4 星评分。

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::files::FileRow;
use super::super::inspect::{AssetFacet, InspectMessage, SearchRow};
use super::super::{InspectEffect, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::REPO_ID;

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("filter-bar", filter_bar_scene()),
        ("filter-bar-active", filter_bar_active_scene()),
        ("search-results", search_results_scene()),
        ("search-empty", search_empty_scene()),
    ]
}

/// 打开筛选栏：格式候选来自仓库摘要，没有标签、颜色和形状候选。
fn filter_bar_scene() -> ShellViewModel {
    let mut model = search_page(false);
    model.inspect.filter_bar_open = true;
    model
}

/// 已选 png、红色和 3 星+，候选里有标签、颜色和形状。
fn filter_bar_active_scene() -> ShellViewModel {
    let mut model = search_page(true);
    model.inspect.filter_bar_open = true;
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filters.colors = vec!["红色".into()];
    model.inspect.filters.min_rating = Some(3.0);
    model.inspect.results = vec![cover_hit(&model)];
    model
}

/// 查询「封面」并选 png、参考、红色，命中 cover.png。
fn search_results_scene() -> ShellViewModel {
    let mut model = search_page(true);
    model.inspect.filter_bar_open = true;
    model.inspect.query = "封面".into();
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filters.tags = vec!["参考".into()];
    model.inspect.filters.colors = vec!["红色".into()];
    model.inspect.results = vec![cover_hit(&model)];
    model
}

/// 查询没有命中：在标题栏搜索框输入，等过 250ms 的搜索延时，搜索应答是空结果。
/// 筛选栏关闭，结果区是等待搜索条件。
fn search_empty_scene() -> ShellViewModel {
    let mut model = search_page(false);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetQuery("不存在的文件".into())));
    let request = super::await_inspect_effect(&mut model, |effect| match effect {
        InspectEffect::Search { generation, .. } => Some(generation),
        _ => None,
    });
    match request {
        Some(generation) => model.reduce(ShellMessage::Inspect(InspectMessage::SearchFinished { generation, result: Ok(Vec::new()) })),
        None => eprintln!("Nana 搜索场景没有等到搜索请求：{}", model.inspect.query),
    }
    model
}

/// 文件列表页切到搜索面板，挂上仓库摘要的标签和扩展名，以及根目录的三条条目。
fn search_page(tagged: bool) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Search;
    let (cover_tags, cover_meta, page_tags, page_meta) = if tagged {
        (
            vec!["封面".to_string(), "参考".to_string()],
            meta(&[("color", "红色"), ("shape", "横版")]),
            vec!["文档".to_string()],
            meta(&[("color", "#3fa796"), ("shape", "竖版")]),
        )
    } else {
        (Vec::new(), BTreeMap::new(), Vec::new(), BTreeMap::new())
    };
    model.inspect.search_ui.facets_repo = Some(REPO_ID.into());
    model.inspect.search_ui.facets = vec![
        AssetFacet { tags: cover_tags.clone(), extension: "png".into() },
        AssetFacet { tags: page_tags.clone(), extension: "pdf".into() },
    ];
    model.files.rows = vec![
        row("assets", "directory", Vec::new(), BTreeMap::new()),
        row("cover.png", "file", cover_tags, cover_meta),
        row("notes/page.pdf", "file", page_tags, page_meta),
    ];
    model.files.total_entries = model.files.rows.len();
    model
}

/// 搜索命中 cover.png：标签、颜色、形状和 4 星评分。
fn cover_hit(model: &ShellViewModel) -> SearchRow {
    let mut metadata = meta(&[("color", "红色"), ("shape", "横版")]);
    metadata.insert("rating".into(), Value::from(4));
    SearchRow {
        repo_id: REPO_ID.into(),
        asset_id: "cover.png".into(),
        path: "cover.png".into(),
        filename: "cover.png".into(),
        repo_name: model.repository_name.clone(),
        tags: vec!["封面".into(), "参考".into()],
        metadata,
    }
}

fn meta(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(key, value)| (key.to_string(), Value::String(value.to_string()))).collect()
}

fn row(path: &str, kind: &str, tags: Vec<String>, metadata: BTreeMap<String, Value>) -> FileRow {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = (kind == "file").then(|| name.rsplit_once('.').map(|(_, ext)| ext.to_string())).flatten();
    FileRow {
        path: path.into(),
        name,
        kind: kind.into(),
        asset_id: (kind == "file").then(|| path.replace('/', "-")),
        is_virtual: false,
        thumbnail_path: None,
        hardlink_state: None,
        extension,
        pixel_width: 0,
        pixel_height: 0,
        texture_ready: false,
        thumbnail_rgba: None,
        page_rgba: None,
        palette: Vec::new(),
        size_label: String::new(),
        modified_at: String::new(),
        tags,
        thumbnail_custom: false,
        provider_id: None,
        source_payload: None,
        metadata,
    }
}
