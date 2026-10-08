//! 搜索展示数据测试：候选、色块、结果芯片、范围和摘要、库类型快捷方式。

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::super::files::FileRow;
use super::super::super::inspect::{AssetFacet, SearchRow, SortDirection};
use super::super::super::inspect_shortcuts::SearchShortcut;
use super::super::super::workspace::WorkspaceRepository;
use super::super::super::ShellViewModel;
use super::*;

fn shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.present_repository(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "默认资源库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    });
    model
}

fn meta(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
}

fn hit(filename: &str, tags: &[&str], metadata: BTreeMap<String, Value>) -> SearchRow {
    SearchRow {
        repo_id: "repo".into(),
        asset_id: filename.into(),
        path: filename.into(),
        filename: filename.into(),
        repo_name: "默认资源库".into(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        metadata,
    }
}

fn file(path: &str, metadata: BTreeMap<String, Value>) -> FileRow {
    FileRow {
        path: path.into(),
        name: path.into(),
        kind: "file".into(),
        asset_id: Some(path.into()),
        is_virtual: false,
        thumbnail_path: None,
        hardlink_state: None,
        extension: None,
        pixel_width: 0,
        pixel_height: 0,
        texture_ready: false,
        thumbnail_rgba: None,
        page_rgba: None,
        palette: Vec::new(),
        size_label: String::new(),
        modified_at: String::new(),
        tags: Vec::new(),
        thumbnail_custom: false,
        provider_id: None,
        source_payload: None,
        metadata,
    }
}

#[test]
fn options_merge_snapshot_results_and_folder_metadata() {
    let mut model = shell();
    model.inspect.search_ui.facets_repo = Some("repo".into());
    model.inspect.search_ui.facets = vec![
        AssetFacet { tags: vec!["封面".into(), " 参考 ".into()], extension: "png".into() },
        AssetFacet { tags: vec!["文档".into()], extension: "pdf".into() },
        AssetFacet { tags: Vec::new(), extension: String::new() },
    ];
    model.inspect.results = vec![hit("clip.MP4", &["参考", "动画"], meta(&[("color", Value::from("红色")), ("shape", Value::from(3))]))];
    model.files.rows = vec![
        file("cover.png", meta(&[("color", Value::from("#3fa796")), ("shape", Value::from("横版"))])),
        file("page.pdf", meta(&[("color", Value::from(true))])),
    ];
    let options = filter_options(&model);
    assert_eq!(options.formats, ["mp4", "pdf", "png"]);
    assert_eq!(options.tags, ["参考", "动画", "封面", "文档"]);
    assert_eq!(options.colors, ["#3fa796", "红色", "true"], "zh 排序：符号、汉字、拉丁字母");
    assert_eq!(options.shapes, ["3", "横版"]);

    model.inspect.search_ui.facets_repo = Some("other".into());
    let options = filter_options(&model);
    assert_eq!(options.formats, ["mp4"], "别的仓库的摘要不参与候选");
}

#[test]
fn selected_values_stay_in_the_options_so_they_can_be_removed() {
    let mut model = shell();
    model.inspect.filters.colors = vec!["青色".into()];
    model.inspect.filters.tags = vec!["待整理".into()];
    let options = filter_options(&model);
    assert_eq!(options.colors, ["青色"], "手动添加的颜色有芯片");
    assert_eq!(options.tags, ["待整理"], "已选标签即使数据里没有也显示");
    assert!(options.formats.is_empty() && options.shapes.is_empty());
}

#[test]
fn swatches_take_hex_then_known_names_then_the_accent() {
    assert_eq!(swatch_color("#3FA796"), SwatchColor::Rgb([0x3f, 0xa7, 0x96]));
    assert_eq!(swatch_color(" #e05252 "), SwatchColor::Rgb([0xe0, 0x52, 0x52]));
    assert_eq!(swatch_color("红色"), SwatchColor::Rgb([0xe0, 0x52, 0x52]));
    assert_eq!(swatch_color("Grey"), SwatchColor::Rgb([0x8c, 0x92, 0x99]));
    assert_eq!(swatch_color("#abc"), SwatchColor::Accent, "三位色值不是 #RRGGBB");
    assert_eq!(swatch_color("青色"), SwatchColor::Accent);
}

#[test]
fn result_context_lists_format_tags_color_shape_and_rating() {
    let row = hit(
        "cover.PNG",
        &["封面", "参考", "海报", "多余"],
        meta(&[("color", Value::from("红色")), ("shape", Value::from("横版")), ("rating", Value::from(4))]),
    );
    assert_eq!(result_context(&row), ["png", "封面", "参考", "海报", "红色", "横版", "4 星"]);
    let half = hit("notes", &[], meta(&[("rating", Value::from(4.5))]));
    assert_eq!(result_context(&half), ["文件", "4.5 星"]);
    let text_rating = hit("a.txt", &[], meta(&[("rating", Value::from("5")), ("color", Value::Null)]));
    assert_eq!(result_context(&text_rating), ["txt"], "字符串评分和空颜色不显示");
    let zero = hit("a.txt", &[], meta(&[("rating", Value::from(0))]));
    assert_eq!(result_context(&zero), ["txt"]);
}

#[test]
fn scope_and_summary_follow_filters_and_query() {
    let mut model = shell();
    assert_eq!(scope_label(&model), "全局搜索");
    assert_eq!(summary(&model), "输入关键词、标签或评分条件后，这里会展示跨仓库结果。");
    model.inspect.query = "封面".into();
    assert_eq!(summary(&model), "当前查询: 封面");
    model.inspect.filters.formats = vec!["png".into()];
    assert_eq!(scope_label(&model), "默认资源库内筛选");
    assert_eq!(summary(&model), "当前资源库筛选: 封面");
    model.inspect.query = "  ".into();
    assert_eq!(summary(&model), "按当前资源库筛选结果。");
}

#[test]
fn shortcuts_show_only_when_entries_or_results_belong_to_the_library() {
    let mut model = shell();
    model.inspect.shortcuts = vec![SearchShortcut {
        id: "works".into(),
        label: "ASMR 作品".into(),
        metadata: "libraryKind=asmr".into(),
        sort_field: "metadata.workId".into(),
        sort_direction: SortDirection::Asc,
        library_kind: "asmr".into(),
        plugin_id: "momobako.library.asmr".into(),
    }];
    model.files.rows = vec![file("cover.png", BTreeMap::new())];
    assert!(active_shortcuts(&model).is_empty());
    model.inspect.results = vec![hit("voice.mp3", &[], meta(&[("workId", Value::from("RJ01"))]))];
    assert_eq!(active_shortcuts(&model).len(), 1);
    model.inspect.results.clear();
    model.files.rows = vec![file("voice.mp3", meta(&[("libraryKind", Value::from("asmr"))]))];
    assert_eq!(active_shortcuts(&model).len(), 1);
}
