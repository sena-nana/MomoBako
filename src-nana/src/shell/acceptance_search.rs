//! 搜索与筛选的对照场景：搜索面板和资源筛选栏。
//!
//! 场景名和 `tmp/vue-mock/scenes/search.ts` 的 Vue 场景同名，数据和 Vue 夹具保持一致：
//! 仓库「默认资源库」，根目录有 `assets/`、`cover.png` 和 `notes/page.pdf`（`acceptance_base.rs` 的底子）。
//! 带标签的夹具里 cover.png 是「封面、参考」、红色横版，page.pdf 是「文档」、#3fa796 竖版，标签和扩展名
//! 候选经仓库摘要进来；搜索固定命中 cover.png，元数据带 4 星评分。
//! `search-no-library` 不从这份底子起步：资源库列表是空的，只切到搜索面板。
//! `search-failed` 仍从这份底子起步：有资源库，搜索在领域服务没起来时失败，只显示交回的原因。

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::inspect::{InspectMessage, SearchRow};
use super::super::{InspectEffect, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::base_scene::{Base, REPO_ID, dir, tagged_file};

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("filter-bar", filter_bar_scene()),
        ("filter-bar-active", filter_bar_active_scene()),
        ("search-results", search_results_scene()),
        ("search-empty", search_empty_scene()),
        ("search-failed", search_failed_scene()),
        ("search-no-library", search_no_library_scene()),
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

/// 没有资源库时的搜索页。
///
/// 空库启动走完以后资源库列表是空的，再切到搜索面板。不写入仓库、查询、命中或时间。
/// 空状态照 Vue `SearchPanel.vue`：没有仓库时标题是「还没有可搜索的资源库」，
/// 说明是「先在资源库页面添加一个仓库，再执行跨仓库搜索。」，优先于「等待搜索条件」。
fn search_no_library_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::EmptyRepository);
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
    if !model.workspace.repositories.is_empty()
        || model.workspace.panel != WorkspacePanel::Search
        || !model.inspect.query.is_empty()
        || !model.inspect.results.is_empty()
    {
        eprintln!(
            "Nana 无资源库搜索场景状态不对：仓库 {} 个，面板 {:?}，查询 {:?}，结果 {} 条",
            model.workspace.repositories.len(),
            model.workspace.panel,
            model.inspect.query,
            model.inspect.results.len(),
        );
    }
    model
}

/// 有资源库，但搜索派发出去时领域服务没起来。
///
/// 查询照搜索框输入「封面」，等过 250ms 的搜索延时。失败原因来自服务没起来时交回的那一句，
/// 不写入命中，也不把失败说成没有匹配。没有失败结果时不编错误句子。
fn search_failed_scene() -> ShellViewModel {
    let mut model = search_page(false);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetQuery(
        "封面".into(),
    )));
    let request = super::await_inspect_effect(&mut model, |effect| match effect {
        InspectEffect::Search { generation, .. } => Some(generation),
        _ => None,
    });
    let Some(generation) = request else {
        eprintln!("Nana 搜索失败场景没有等到搜索请求：{}", model.inspect.query);
        return model;
    };
    let Some(error) = crate::task_guard::failure_when_services_missing("搜索") else {
        eprintln!("Nana 搜索失败场景没有拿到服务未启动时的失败原因");
        return model;
    };
    model.reduce(ShellMessage::Inspect(InspectMessage::SearchFinished {
        generation,
        result: Err(error),
    }));
    if !model.inspect.results.is_empty() {
        eprintln!(
            "Nana 搜索失败场景出现了命中：{}",
            model.inspect.results.len()
        );
    }
    if model.inspect.search_error.is_empty() {
        eprintln!("Nana 搜索失败场景没有留下失败原因");
    }
    model
}

/// 查询没有命中：在标题栏搜索框输入，等过 250ms 的搜索延时，搜索应答是空结果。
/// 筛选栏关闭，结果区写明全部资源库里没有匹配的文件。
fn search_empty_scene() -> ShellViewModel {
    let mut model = search_page(false);
    model.reduce(ShellMessage::Inspect(InspectMessage::SetQuery(
        "不存在的文件".into(),
    )));
    let request = super::await_inspect_effect(&mut model, |effect| match effect {
        InspectEffect::Search { generation, .. } => Some(generation),
        _ => None,
    });
    match request {
        Some(generation) => model.reduce(ShellMessage::Inspect(InspectMessage::SearchFinished {
            generation,
            result: Ok(Vec::new()),
        })),
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
                tagged_file(
                    "cover.png",
                    2_400_000,
                    &["封面", "参考"],
                    meta(&[("color", "红色"), ("shape", "横版")]),
                ),
                tagged_file(
                    "notes/page.pdf",
                    1_820,
                    &["文档"],
                    meta(&[("color", "#3fa796"), ("shape", "竖版")]),
                ),
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
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), Value::String(value.to_string())))
        .collect()
}
