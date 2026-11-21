//! 元数据面板投影和交互的测试。

use std::collections::BTreeMap;

use nana_ui::runtime::{Entity, IconButton, SemanticColorRole, StableNodeId};
use serde_json::Value;

use super::custom_fields;
use crate::shell::input::InputMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{FilesMessage, ShellMessage};

/// 通用字段、后端种下的系统字段都不进自定义行；作品信息认领的键只在区块画出来时才让出。
#[test]
fn custom_rows_skip_reserved_and_seeded_keys() {
    let mut custom = BTreeMap::new();
    for key in [
        "comment",
        "note",
        "link",
        "rating",
        "tagGroups",
        "width",
        "height",
        "originalSizeBytes",
        "addedToLibraryAt",
        "fileCreatedAt",
        "fileModifiedAt",
        "palette",
        "thumbnailPalette",
        "originTitle",
        "sourceUrl",
        "title",
        "type",
        "favorite",
        "color",
        "artist",
        "workTitle",
    ] {
        custom.insert(key.to_string(), "x".to_string());
    }
    let shown = |claimed| {
        custom_fields(&custom, claimed)
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(false), ["artist", "workTitle"]);
    assert_eq!(shown(true), ["artist"]);
}

/// F7：标签菜单先列仓库的标签分组（去掉草稿里已有的，空了的分组不列，不按输入过滤），再列整个资源库用过的标签：
/// 仓库摘要里每项素材的标签，不只是当前目录读到的那几行，按 zh-CN 排序、按输入过滤。菜单收着时不算候选。
#[test]
fn tag_menu_offers_repository_groups_and_library_wide_tags() {
    use crate::backend::services::repository::RepositoryTagGroup;
    use crate::shell::{FilesMessage, ShellMessage};
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-selected-metadata")
        .map(|(_, model)| model)
        .expect("files-selected-metadata 场景");
    assert_eq!(model.inspect.draft_tags(), ["封面", "参考"]);
    model
        .inspect
        .search_ui
        .facets
        .push(super::super::inspect::AssetFacet {
            tags: vec!["远处的标签".into(), "动画".into()],
            extension: "png".into(),
        });
    let group = |id: &str, name: &str, tags: &[&str]| RepositoryTagGroup {
        tag_group_id: id.into(),
        name: name.into(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
    };
    model.inspect.search_ui.tag_groups = vec![
        group("g1", "角色", &["主角", "封面"]),
        group("g2", "已用完", &["参考"]),
    ];
    assert!(
        super::tags_view(&model).options.is_empty(),
        "菜单收着时不算候选"
    );

    model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
    let view = super::tags_view(&model);
    assert_eq!(
        view.groups,
        [super::TagGroupOption {
            id: "g1".into(),
            name: "角色".into(),
            tags: vec!["主角".into()]
        }]
    );
    assert_eq!(
        view.options,
        ["动画", "远处的标签"],
        "整库的标签，不在当前目录也列出"
    );

    model.reduce(ShellMessage::Files(FilesMessage::SetTagDraft("远".into())));
    let view = super::tags_view(&model);
    assert_eq!(view.options, ["远处的标签"]);
    assert_eq!(view.groups.len(), 1, "标签分组不按输入过滤");
}

/// 选中带完整元数据的 cover.png：评分、注释、尺寸这些只在各自的行里出现一次，不再按原始键名重复；
/// 后端种下的标题、类型、收藏和主色也不出现，真正的自定义字段照常列出。
#[test]
fn selected_metadata_lists_each_reserved_field_once() {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-selected-metadata")
        .map(|(_, model)| model)
        .expect("files-selected-metadata 场景");
    let row = model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == "cover.png")
        .expect("cover.png");
    for (key, value) in [
        ("title", "cover.png"),
        ("type", "png"),
        ("color", "#c7a566"),
        ("artist", "Momo"),
    ] {
        row.metadata.insert(key.into(), Value::String(value.into()));
    }
    row.metadata.insert("favorite".into(), Value::Bool(false));
    let document = crate::acceptance_document_for_model(model).expect("生产文档");
    let nodes = document
        .context()
        .world()
        .project_accessibility_in_viewport(document.document());
    let labelled = |label: &str| {
        nodes
            .iter()
            .filter(|node| node.label.as_deref() == Some(label))
            .count()
    };
    for key in [
        "comment",
        "link",
        "rating",
        "width",
        "height",
        "originalSizeBytes",
        "addedToLibraryAt",
        "fileCreatedAt",
        "title",
        "type",
        "favorite",
        "color",
    ] {
        assert_eq!(labelled(key), 0, "保留字段 {key} 不该按原始键名再列一行");
    }
    assert_eq!(labelled("尺寸"), 1);
    assert_eq!(labelled("1920 × 1080"), 1);
    assert_eq!(labelled("artist"), 1, "真正的自定义字段照常列出");
    assert_eq!(labelled("Momo"), 1);
}

/// 多归属位置跟正在看的列表条目走。智能文件夹的查询结果不在上一页目录里，
/// 继续加载也会把 `browser_entries` 换成后面那一页；两种情况下位置都还在。
#[test]
fn alias_locations_follow_the_list_entry_not_the_latest_directory_page() {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-selected-metadata")
        .map(|(_, model)| model)
        .expect("files-selected-metadata 场景");
    let mut entry = model
        .files
        .rows
        .iter()
        .find(|row| row.path == "cover.png")
        .expect("cover.png")
        .clone();
    entry.alias_paths = vec![
        "cover.png".into(),
        "albums/cover.png".into(),
        "sets/cover.png".into(),
    ];
    model.inspect.target_path = Some("cover.png".into());
    model.files.rows.clear();
    model.files.virtual_rows = vec![entry];
    model.workspace.panel = super::super::workspace::WorkspacePanel::SmartFolder;
    // 上一页目录快照已经不在了。旧逻辑从这里找位置，会把这一行写成空。
    model.browser_entries.clear();

    let row = super::target_row(&model);
    let recorded = super::recorded_view(&model, row);
    assert_eq!(recorded.aliases, "albums/cover.png，sets/cover.png");
}

/// 来源链接两个按钮的悬停提示是「打开链接」「复制链接」。
/// 能开的地址点「打开链接」交给系统，点「复制链接」写入剪贴板。
/// `javascript:` 打不开，按钮停用，点下去不发打开。
#[test]
fn source_link_buttons_show_titles_and_open_or_copy() {
    let https = source_link_model("https://example.com/cover");
    let mut harness = ShellHarness::mount_at(https, 1200.0, 2000.0);
    let open = harness.node("打开链接");
    let copy = harness.node("复制链接");
    assert_eq!(tooltip(&harness, open).as_deref(), Some("打开链接"));
    assert_eq!(tooltip(&harness, copy).as_deref(), Some("复制链接"));
    assert!(
        hover_uses_text_and_accent(&harness, open),
        "悬停应把图标改正文色、边框改强调色"
    );
    assert!(!disabled(&harness, open), "https 链接可以打开");

    let (x, y) = harness.center(copy);
    let messages = harness.click_messages(x, y);
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ShellMessage::Files(FilesMessage::CopyText(url)) if url == "https://example.com/cover"
        )),
        "复制链接应写入这条地址，实际 {} 条消息",
        messages.len()
    );
    let (x, y) = harness.center(open);
    let messages = harness.click_messages(x, y);
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ShellMessage::Input(InputMessage::OpenExternalUrl { url }) if url == "https://example.com/cover"
        )),
        "打开链接应交给系统，实际 {} 条消息",
        messages.len()
    );

    let mut blocked =
        ShellHarness::mount_at(source_link_model("javascript:alert(1)"), 1200.0, 2000.0);
    let open = blocked.node("打开链接");
    assert!(disabled(&blocked, open), "javascript: 不能打开");
    assert_eq!(tooltip(&blocked, open).as_deref(), Some("打开链接"));
    let (x, y) = blocked.center(open);
    let messages = blocked.click_messages(x, y);
    assert!(
        messages.iter().all(|message| !matches!(
            message,
            ShellMessage::Input(InputMessage::OpenExternalUrl { .. })
        )),
        "停用的打开链接不应发出打开，实际 {} 条消息",
        messages.len()
    );
    let copy = blocked.node("复制链接");
    assert!(!disabled(&blocked, copy), "复制不受协议限制");
    let (x, y) = blocked.center(copy);
    let messages = blocked.click_messages(x, y);
    assert!(
        messages.iter().any(|message| matches!(
            message,
            ShellMessage::Files(FilesMessage::CopyText(url)) if url == "javascript:alert(1)"
        )),
        "打不开的链接仍可复制，实际 {} 条消息",
        messages.len()
    );
}

fn source_link_model(url: &str) -> crate::shell::ShellViewModel {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "files-selected-metadata")
        .map(|(_, model)| model)
        .expect("files-selected-metadata 场景");
    let row = model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == "cover.png")
        .expect("cover.png");
    row.metadata.insert(
        "sourceUrl".into(),
        serde_json::Value::String(url.to_string()),
    );
    model
}

fn tooltip(harness: &ShellHarness, id: StableNodeId) -> Option<String> {
    harness
        .document()
        .context()
        .read(Entity::<IconButton>::from_stable_id(id), |button| {
            button.tooltip.as_ref().map(|tip| tip.label.to_string())
        })
        .unwrap_or_else(|_| panic!("来源链接按钮应该是图标按钮"))
}

fn hover_uses_text_and_accent(harness: &ShellHarness, id: StableNodeId) -> bool {
    harness
        .document()
        .context()
        .read(Entity::<IconButton>::from_stable_id(id), |button| {
            let hover = &button.style.interaction.hovered;
            hover.foreground == Some(SemanticColorRole::Text)
                && hover.border == Some(SemanticColorRole::Accent)
        })
        .unwrap_or(false)
}

fn disabled(harness: &ShellHarness, id: StableNodeId) -> bool {
    harness
        .document()
        .context()
        .world()
        .accessibility(id)
        .is_some_and(|state| state.disabled)
}
