//! 搜索与筛选的对照场景：搜索面板和资源筛选栏。
//!
//! 场景名和 `tmp/vue-mock/scenes/search.ts` 的 Vue 场景同名，数据和 Vue 夹具保持一致。

use super::super::{ShellPage, ShellViewModel, WorkspacePanel};
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

/// 搜索面板打开、筛选栏打开，没有任何条件。
fn filter_bar_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.filter_bar_open = true;
    model
}

/// 已选 png、红色和 3 星+，命中 cover.png。
fn filter_bar_active_scene() -> ShellViewModel {
    let mut model = filter_bar_scene();
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filters.colors = vec!["红色".into()];
    model.inspect.filters.min_rating = Some(3.0);
    model.inspect.results = vec![cover_hit(&model)];
    model
}

/// 查询「封面」并选 png、参考、红色，命中 cover.png。
fn search_results_scene() -> ShellViewModel {
    let mut model = filter_bar_scene();
    model.inspect.query = "封面".into();
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filters.tags = vec!["参考".into()];
    model.inspect.filters.colors = vec!["红色".into()];
    model.inspect.results = vec![cover_hit(&model)];
    model
}

/// 查询没有命中，筛选栏关闭。
fn search_empty_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.query = "不存在的文件".into();
    model
}

fn cover_hit(model: &ShellViewModel) -> super::super::inspect::SearchRow {
    super::super::inspect::SearchRow {
        repo_id: REPO_ID.into(),
        asset_id: "cover.png".into(),
        path: "cover.png".into(),
        filename: "cover.png".into(),
        repo_name: model.repository_name.clone(),
    }
}
