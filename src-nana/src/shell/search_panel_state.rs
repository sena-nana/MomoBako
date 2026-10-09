//! 搜索面板的投影和常驻信号。
//!
//! [`SearchPanelView`] 从 ViewModel 算出搜索面板要显示的文案、状态和结果行，按显示的样子算好；
//! [`SearchPanelSignals`] 在主区块的常驻作用域里建（`RouteSignals`），同步时只写变了的部分。
//! 结果行放进 Store，按「仓库 + 素材 id」做键（没有素材 id 的按路径），行内字段各自绑定：
//! 再搜一次只换变了的行，留下的行节点不动。

use nana_ui::runtime::view::{signal, store, Signal, Store};

use super::super::admin::bind::sync_rows;
use super::super::admin::style::key_part;
use super::super::inspect::SearchRow;
use super::super::ShellViewModel;
use super::presenter;

/// 搜索面板要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPanelView {
    pub head: SearchHead,
    pub status: SearchStatus,
    /// 结果行；结果列表不占位时为空，不建藏起来的行。
    pub hits: Vec<HitRow>,
}

/// 页头：范围眉题、摘要和两个计数。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SearchHead {
    pub scope: String,
    pub summary: String,
    /// 「N 个仓库」。
    pub repositories: String,
    /// 「N 条结果」。
    pub hits: String,
}

/// 页头下面的状态块：哪几块显示、写什么。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SearchStatus {
    /// 搜索失败的原因，空时不显示错误行。
    pub error: String,
    pub searching: bool,
    /// 空状态的标题和说明；有结果列表时为 `None`。
    pub empty: Option<(String, String)>,
    /// 结果列表占位：有仓库，并且正在搜索或有结果。正在搜索时列表为空也占一个间距，和旧视图一样。
    pub listed: bool,
}

/// 一条结果。`node_key` 是行的身份，也拼进节点键。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HitRow {
    pub node_key: String,
    pub repo_id: String,
    pub asset_id: String,
    pub filename: String,
    /// 「仓库 / 路径」。
    pub detail: String,
    pub chips: Vec<HitChip>,
}

/// 结果行右侧的一枚芯片。芯片没有自己的编号，身份是位置加文字（格式和标签可能同名）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HitChip {
    pub index: usize,
    pub text: String,
}

impl SearchPanelView {
    /// 从 ViewModel 取搜索面板的投影，取舍和旧视图 `search_panel` 一致。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let inspect = &model.inspect;
        let no_repository = model.workspace.repositories.is_empty();
        let empty = if no_repository {
            Some(("还没有可搜索的资源库".to_string(), "先在资源库页面添加一个仓库，再执行跨仓库搜索。".to_string()))
        } else if !inspect.searching && inspect.results.is_empty() {
            presenter::empty_result(model).map(|(title, message)| (title.to_string(), message))
        } else {
            None
        };
        let listed = !no_repository && (inspect.searching || !inspect.results.is_empty());
        Self {
            head: SearchHead {
                scope: presenter::scope_label(model),
                summary: presenter::summary(model),
                repositories: format!("{} 个仓库", model.workspace.repositories.len()),
                hits: format!("{} 条结果", inspect.results.len()),
            },
            status: SearchStatus { error: inspect.search_error.clone(), searching: inspect.searching, empty, listed },
            hits: if listed { inspect.results.iter().map(HitRow::from_row).collect() } else { Vec::new() },
        }
    }
}

impl HitRow {
    fn from_row(row: &SearchRow) -> Self {
        // 没有素材 id 的结果按路径区分，免得同仓库的几条撞成一个键。
        let identity = if row.asset_id.is_empty() { row.path.as_str() } else { row.asset_id.as_str() };
        Self {
            node_key: key_part(&format!("{}:{identity}", row.repo_id)),
            repo_id: row.repo_id.clone(),
            asset_id: row.asset_id.clone(),
            filename: row.filename.clone(),
            detail: format!("{} / {}", row.repo_name, row.path),
            chips: presenter::result_context(row).into_iter().enumerate().map(|(index, text)| HitChip { index, text }).collect(),
        }
    }

    /// 结果列表里行的键。
    pub(crate) fn key(row: &HitRow) -> String {
        row.node_key.clone()
    }
}

/// 搜索面板的信号。句柄是 `Copy` 的 id，值在建它的作用域里。
#[derive(Clone, Copy)]
pub(crate) struct SearchPanelSignals {
    pub(crate) head: Signal<SearchHead>,
    pub(crate) status: Signal<SearchStatus>,
    pub(crate) hits: Store<Vec<HitRow>>,
}

impl SearchPanelSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self { head: signal(SearchHead::default()), status: signal(SearchStatus::default()), hits: store(Vec::new()) }
    }

    /// 写入投影，只写变了的。结果行按键写差异：留下的行节点不动，只有内容变了的行字段重写。
    pub(crate) fn write(&self, view: SearchPanelView) {
        self.head.try_set_if_changed(view.head);
        self.status.try_set_if_changed(view.status);
        sync_rows(self.hits, HitRow::key, view.hits);
    }
}

#[cfg(test)]
mod tests {
    use super::SearchPanelView;
    use crate::shell::acceptance_gap_models;

    fn scene(name: &str) -> crate::shell::ShellViewModel {
        acceptance_gap_models().into_iter().find(|(scene, _)| *scene == name).map(|(_, model)| model).expect("场景")
    }

    #[test]
    fn projection_follows_results_and_states() {
        let model = scene("search-results");
        let view = SearchPanelView::project(&model);
        assert_eq!(view.head.scope, "默认资源库内筛选");
        assert_eq!(view.head.summary, "当前资源库筛选: 封面");
        assert_eq!((view.head.repositories.as_str(), view.head.hits.as_str()), ("1 个仓库", "1 条结果"));
        assert!(view.status.listed && view.status.empty.is_none() && view.status.error.is_empty());
        assert_eq!(view.hits.len(), 1);
        let hit = &view.hits[0];
        assert_eq!(hit.detail, "默认资源库 / cover.png");
        let chips = hit.chips.iter().map(|chip| chip.text.as_str()).collect::<Vec<_>>();
        assert_eq!(chips, ["png", "封面", "参考", "红色", "横版", "4 星"]);
        assert_eq!(SearchPanelView::project(&model), view, "同一状态的投影相等");

        let empty = SearchPanelView::project(&scene("search-empty"));
        assert!(!empty.status.listed && empty.hits.is_empty());
        assert_eq!(empty.status.empty.as_ref().map(|(title, _)| title.as_str()), Some("没有匹配的文件"));
    }

    /// 没有素材 id 的结果按路径做键，同一仓库的几条不会撞键。
    #[test]
    fn hits_without_an_asset_id_are_keyed_by_path() {
        let mut model = scene("search-results");
        let mut first = model.inspect.results[0].clone();
        first.asset_id.clear();
        let mut second = first.clone();
        second.path = "notes/page.pdf".into();
        model.inspect.results = vec![first, second];
        let view = SearchPanelView::project(&model);
        assert_ne!(view.hits[0].node_key, view.hits[1].node_key);
        assert!(view.hits.iter().all(|hit| !hit.node_key.contains('/')), "键里不能有路径分隔符");
    }
}
