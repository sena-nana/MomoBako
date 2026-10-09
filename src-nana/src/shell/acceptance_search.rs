//! 搜索与筛选的对照场景：搜索面板和资源筛选栏。
//!
//! 场景名和 `tmp/vue-mock/scenes/search.ts` 的 Vue 场景同名，数据和 Vue 夹具保持一致：
//! 仓库「默认资源库」，根目录有 `assets/`、`cover.png` 和 `notes/page.pdf`（`acceptance_base.rs` 的底子）。
//! 带标签的夹具里 cover.png 是「封面、参考」、红色横版，page.pdf 是「文档」、#3fa796 竖版，标签和扩展名
//! 候选经仓库摘要进来；搜索固定命中 cover.png，元数据带 4 星评分。

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::inspect::{InspectMessage, SearchRow};
use super::super::{InspectEffect, ShellMessage, ShellViewModel, WorkspacePanel};
use super::base_scene::{dir, tagged_file, Base, REPO_ID};

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
/// 筛选栏关闭，结果区写明全部资源库里没有匹配的文件。
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

/// 文件列表页切到搜索面板。`tagged` 时用 Vue `tagged()` 的夹具，标签、颜色和形状经仓库摘要和根目录
/// 条目进来，筛选栏的候选由它们算出。
fn search_page(tagged: bool) -> ShellViewModel {
    let base = if tagged {
        Base {
            entries: vec![
                dir("assets"),
                tagged_file("cover.png", 2_400_000, &["封面", "参考"], meta(&[("color", "红色"), ("shape", "横版")])),
                tagged_file("notes/page.pdf", 1_820, &["文档"], meta(&[("color", "#3fa796"), ("shape", "竖版")])),
            ],
            ..Base::default()
        }
    } else {
        Base::default()
    };
    let mut model = base.model();
    model.workspace.panel = WorkspacePanel::Search;
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
