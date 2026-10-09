//! 重挂保留滚动位置的回归：文件列表和设置页滚到中间后整棵重挂，偏移和视口里的行都不变。

use nana_ui::runtime::{component_descriptors, Entity, LayoutViewport, RuntimeDocument, ScrollOffset, ScrollView, StableNodeId};
use nana_ui::NanaTextShaper;

use crate::backend::services::repository::{FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState};
use crate::shell::files::DisplayMode;
use crate::shell::{mount_shell, ShellMessage, ShellPage, ShellViewModel, WorkspaceRepository};

const WIDTH: f32 = 1200.0;

fn layout(document: &mut RuntimeDocument, height: f32) {
    document.flush(LayoutViewport::new(WIDTH, height), &mut NanaTextShaper::default()).expect("布局");
}

fn mounted(model: &ShellViewModel, height: f32) -> RuntimeDocument {
    let mut document = crate::acceptance_document_for_model(model.clone()).expect("生产文档");
    layout(&mut document, height);
    document
}

/// 有一个可写仓库、根目录里有 `count` 个文件的工作区。
fn files_model(count: usize) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.startup.finish();
    model.workspace.apply_repository_list(None, Ok(vec![WorkspaceRepository {
        repo_id: "repo".into(),
        name: "动画素材".into(),
        path: "D:/Libraries/Anime".into(),
        status: "ready".into(),
        backend_plugin_id: "local".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    }]));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(snapshot(count))));
    model
}

fn snapshot(count: usize) -> FileBrowserSnapshot {
    let entries = (0..count)
        .map(|index| {
            let name = format!("file-{index:03}.txt");
            FileBrowserEntry {
                path: name.clone(),
                name,
                kind: "file".into(),
                extension: Some("txt".into()),
                size_bytes: None,
                size_label: None,
                modified_at: None,
                asset_id: None,
                status: None,
                thumbnail_path: None,
                thumbnail_custom: false,
                hardlink_group_id: None,
                hardlink_state: None,
                tags: Vec::new(),
                alias_paths: Vec::new(),
                folder_metadata: None,
                metadata: std::collections::BTreeMap::new(),
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            }
        })
        .collect::<Vec<_>>();
    FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "D:/Libraries/Anime".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

/// 名称以 `label` 开头的节点外面最近的滚动容器。文件行的名称是「文件名 + 说明」。
fn scroll_around(document: &RuntimeDocument, label: &str) -> StableNodeId {
    let world = document.context().world();
    let node = world
        .project_accessibility(document.document())
        .into_iter()
        .find(|node| node.label.as_deref().is_some_and(|text| text.starts_with(label)))
        .unwrap_or_else(|| panic!("没有 {label}"));
    let mut cursor = node.id;
    loop {
        if world.component_type(cursor).is_some_and(|kind| kind.as_str() == component_descriptors::SCROLL_VIEW.type_id) {
            return cursor;
        }
        cursor = world.parent_id(cursor).unwrap_or_else(|| panic!("{label} 不在滚动容器里"));
    }
}

/// 键路径以 `key` 结尾的滚动容器。
fn scroll_keyed(document: &RuntimeDocument, key: &str) -> StableNodeId {
    let context = document.context();
    context
        .world()
        .nodes_of_component(document.document(), component_descriptors::SCROLL_VIEW.type_id)
        .find(|id| context.assembly_path(*id).is_some_and(|path| path.ends_with(key)))
        .unwrap_or_else(|| panic!("没有键为 {key} 的滚动容器"))
}

fn offset(document: &RuntimeDocument, id: StableNodeId) -> ScrollOffset {
    document.context().world().scroll_offset(id).unwrap_or_default()
}

/// 滚动容器视口里看得见的文件行：无障碍盒（已经减去滚动偏移）和容器的布局盒相交。
fn visible_rows(document: &RuntimeDocument, scroll: StableNodeId) -> Vec<String> {
    let world = document.context().world();
    let viewport = world.layout_box(scroll).expect("滚动容器有布局盒");
    let mut rows = world
        .project_accessibility(document.document())
        .into_iter()
        .filter(|node| {
            node.bounds.y < viewport.y + viewport.height
                && node.bounds.y + node.bounds.height > viewport.y
                && node.bounds.height > 0.0
        })
        .filter_map(|node| node.label.map(|label| label.to_string()))
        .filter(|label| label.starts_with("file-"))
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    rows
}

#[test]
fn file_list_keeps_its_scroll_and_rows_across_a_remount() {
    let mut model = files_model(200);
    model.files.display_mode = DisplayMode::List;
    let mut document = mounted(&model, 800.0);
    let scroll = scroll_around(&document, "file-000.txt");
    document
        .context_mut()
        .scroll_to(Entity::<ScrollView>::from_stable_id(scroll), ScrollOffset { x: 0.0, y: 3600.0 })
        .expect("滚动");
    layout(&mut document, 800.0);
    layout(&mut document, 800.0);
    let scrolled = offset(&document, scroll);
    let rows = visible_rows(&document, scroll);
    assert_eq!(scrolled.y, 3600.0, "测试列表要能滚到 3600");
    assert!(!rows.is_empty(), "滚动后视口里要有行");
    assert!(!rows.iter().any(|row| row.starts_with("file-000")), "滚动后视口里不该还是第一行：{rows:?}");

    mount_shell(&mut document, &model).expect("重挂");
    layout(&mut document, 800.0);
    let scroll = scroll_around(&document, rows.first().expect("滚动后有行"));
    assert_eq!(offset(&document, scroll), scrolled, "重挂后滚动位置变了");
    assert_eq!(visible_rows(&document, scroll), rows, "重挂后视口里的行变了");
}

#[test]
fn settings_page_keeps_its_scroll_across_a_remount() {
    let mut model = files_model(3);
    model.page = ShellPage::Settings;
    let mut document = mounted(&model, 420.0);
    let scroll = scroll_keyed(&document, "settings-scroll");
    document
        .context_mut()
        .scroll_to(Entity::<ScrollView>::from_stable_id(scroll), ScrollOffset { x: 0.0, y: 160.0 })
        .expect("滚动");
    layout(&mut document, 420.0);
    assert_eq!(offset(&document, scroll).y, 160.0, "设置页要能滚到 160");

    mount_shell(&mut document, &model).expect("重挂");
    layout(&mut document, 420.0);
    assert_eq!(offset(&document, scroll_keyed(&document, "settings-scroll")).y, 160.0, "重挂后设置页回到了顶部");

    // 换到首页：主体是另一个滚动容器，不继承设置页的偏移。
    model.page = ShellPage::FileList;
    model.workspace.panel = crate::shell::WorkspacePanel::Logs;
    mount_shell(&mut document, &model).expect("重挂");
    layout(&mut document, 420.0);
    assert_eq!(offset(&document, scroll_keyed(&document, "workspace-page-scroll-HasRepository-Logs")).y, 0.0);
}
