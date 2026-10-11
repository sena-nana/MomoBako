//! 智能文件夹增改删和查询的结果落到壳层上（Vue `smartFolders.ts`）：新建以后打开新建的那个，编辑以后
//! 重查正在看的（它或它的祖先被改了），删掉正在看的以后回到文件面板；文件页头写正在看的文件夹的名字（子级
//! 也认得）和查询结果的摘要（Vue `smartFolderSummary`）。

use crate::backend::services::repository::{FileBrowserEntry, SearchMetadataFilter, SmartFolderFilter};
use crate::shell::files::{FileRow, VirtualQuery};
use crate::shell::sidebar::{SidebarEffect, SidebarSmartFolder, SmartFolderChange, SmartFolderOutcome, SmartFolderResult};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage, WorkspacePanel};

/// 验收底子里的仓库。
const REPO: &str = "acceptance-repo";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

fn folder(id: &str, parent: Option<&str>, name: &str, children: Vec<SidebarSmartFolder>) -> SidebarSmartFolder {
    SidebarSmartFolder { id: id.into(), parent_id: parent.map(str::to_string), name: name.into(), filter: Default::default(), children }
}

/// 两棵树：「任一」下面有「子级」，旁边是「别的」。
fn tree() -> Vec<SidebarSmartFolder> {
    vec![
        folder("parent", None, "任一", vec![folder("child", Some("parent"), "子级", Vec::new())]),
        folder("other", None, "别的", Vec::new()),
    ]
}

/// 文件页场景，侧栏读到 [`tree`]。
fn with_tree() -> ShellViewModel {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarSmartFoldersLoaded { repo_id: REPO.into(), result: Ok(tree()) }));
    model.sidebar.take_effects();
    model
}

fn saved(model: &mut ShellViewModel, folders: Vec<SidebarSmartFolder>, change: SmartFolderChange) {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SmartFolderSaved { repo_id: REPO.into(), result: Ok(SmartFolderOutcome { folders, change }) }));
}

/// 排下的智能文件夹查询。
fn queries(model: &mut ShellViewModel) -> Vec<String> {
    model
        .sidebar
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            SidebarEffect::QuerySmartFolder { smart_folder_id, .. } => Some(smart_folder_id),
            _ => None,
        })
        .collect()
}

/// 查询结果里的一个文件。
fn hit(index: usize) -> FileRow {
    FileRow::from_entry(&FileBrowserEntry {
        path: format!("hits/hit-{index}.png"),
        name: format!("hit-{index}.png"),
        kind: "file".into(),
        extension: Some("png".into()),
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
        metadata: Default::default(),
        is_virtual: true,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    })
}

/// 查询结果：`count` 行，继承来的筛选是 `filter`。
fn result(name: &str, count: usize, filter: SmartFolderFilter) -> SmartFolderResult {
    SmartFolderResult { name: name.into(), filter, query: VirtualQuery { count, rows: (0..count).map(hit).collect() } }
}

fn queried(model: &mut ShellViewModel, id: &str, result: SmartFolderResult) {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarSmartFolderQueried {
        repo_id: REPO.into(),
        smart_folder_id: id.into(),
        result: Ok(result),
    }));
}

/// 新建以后打开新建的那个（Vue `createSmartFolderInWorkspace` 的 `selectSmartFolder`），在设置页上建的也
/// 离开设置页；服务没带回新文件夹时不打开。
#[test]
fn creating_opens_the_new_folder() {
    let mut model = with_tree();
    model.reduce(ShellMessage::OpenSettings);
    let mut folders = tree();
    folders.push(folder("new", None, "新的", Vec::new()));
    saved(&mut model, folders.clone(), SmartFolderChange::Created(Some("new".into())));
    assert_eq!(model.workspace.panel, WorkspacePanel::SmartFolder);
    assert_eq!(model.sidebar.active_smart_folder_id.as_deref(), Some("new"));
    assert_eq!(queries(&mut model), ["new"], "打开时查询新建的那个");
    assert_eq!(model.page, ShellPage::FileList, "在设置页上建的也去看它");

    let mut model = with_tree();
    saved(&mut model, folders, SmartFolderChange::Created(None));
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert!(queries(&mut model).is_empty());
}

/// 编辑以后：正在看的就是它，或者改的是它的祖先（继承的条件跟着变），重查；改别的、或者智能文件夹面板
/// 没开着时不查，也不把用户拉回智能文件夹面板。
#[test]
fn editing_requeries_the_folder_on_screen() {
    let mut model = with_tree();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder("child".into())));
    assert_eq!(queries(&mut model), ["child"]);

    for (edited, expected) in [("child", vec!["child"]), ("parent", vec!["child"]), ("other", Vec::new())] {
        saved(&mut model, tree(), SmartFolderChange::Updated(edited.into()));
        assert_eq!(queries(&mut model), expected, "改了 {edited}");
    }

    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files));
    saved(&mut model, tree(), SmartFolderChange::Updated("child".into()));
    assert!(queries(&mut model).is_empty(), "智能文件夹面板没开着时不查");
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
}

/// 删掉正在看的（这里删的是它的祖先，子级一起删了）：清掉它的结果，回到文件面板。删别的不动。
#[test]
fn deleting_the_folder_on_screen_returns_to_files() {
    let mut model = with_tree();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder("child".into())));
    queried(&mut model, "child", result("子级", 2, SmartFolderFilter::default()));
    assert_eq!(model.files.virtual_rows.len(), 2);

    saved(&mut model, tree(), SmartFolderChange::Deleted("other".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::SmartFolder, "删的不是正在看的");
    assert_eq!(model.sidebar.active_smart_folder_id.as_deref(), Some("child"));

    saved(&mut model, vec![folder("other", None, "别的", Vec::new())], SmartFolderChange::Deleted("parent".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::Files, "正在看的跟着祖先删了，回到文件面板");
    assert_eq!(model.sidebar.active_smart_folder_id, None);
    assert_eq!(model.sidebar.smart_result, None);
    assert!(model.files.virtual_rows.is_empty(), "被删的文件夹的结果清掉");
}

/// 在回收站面板上删掉以前看过的智能文件夹：只清掉它的结果，面板不动（Vue 会切到文件面板）。
#[test]
fn deleting_elsewhere_keeps_the_panel() {
    let mut model = with_tree();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder("other".into())));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(crate::shell::ShortcutId::Trash)));
    assert_eq!(model.workspace.panel, WorkspacePanel::Trash);
    saved(&mut model, tree().into_iter().take(1).collect(), SmartFolderChange::Deleted("other".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::Trash);
    assert_eq!(model.sidebar.active_smart_folder_id, None);
}

/// 摘要行照 Vue `smartFolderSummary`：条数打头，后面按顺序接上有值的条件。
#[test]
fn the_summary_lists_the_inherited_conditions() {
    let filter = SmartFolderFilter {
        query: Some("cover\nposter".into()),
        path_prefix: Some("Campaigns".into()),
        formats: Some(vec!["psd".into(), "png".into()]),
        tags: Some(vec!["封面".into()]),
        colors: Some(vec!["红色".into()]),
        shapes: Some(vec!["方形".into()]),
        min_rating: Some(4.0),
        metadata_filters: Some(vec![
            SearchMetadataFilter { key: "artist".into(), value: "demo".into() },
            SearchMetadataFilter { key: "source".into(), value: "web".into() },
        ]),
        ..SmartFolderFilter::default()
    };
    assert_eq!(
        super::summary_text(3, &filter),
        "3 条结果 · 关键词 cover + poster · 路径 Campaigns · 格式 psd / png · 标签 封面 · 颜色 红色 · 形状 方形 · 4 星+ · 2 个元数据条件"
    );
    assert_eq!(super::summary_text(0, &SmartFolderFilter::default()), "0 条结果");
}

/// 文件页头：打开子级时标题是它自己的名字（原来只在顶层找，写成「智能文件夹」）；查回来以后摘要行写条数和
/// 继承来的条件，换到另一个、新的还没查回来时不写上一个的摘要。
#[test]
fn the_header_names_nested_folders_and_summarises_the_result() {
    /// 页头面包屑上的标题（虚拟视图只有这一枚）。
    fn title(harness: &ShellHarness) -> Option<String> {
        let id = harness.keyed("file-crumb-root")?;
        harness.nodes().into_iter().find(|node| node.id == id).and_then(|node| node.label.map(|label| label.to_string()))
    }
    let mut harness = ShellHarness::mount(with_tree());
    harness.apply(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder("child".into())));
    harness.flush();
    assert_eq!(title(&harness).as_deref(), Some("子级"), "页头标题应该是子级自己的名字");
    assert_eq!(harness.model.sidebar.smart_summary(), "", "还没查回来时不写摘要");

    let filter = SmartFolderFilter { formats: Some(vec!["psd".into()]), match_mode: Some("or".into()), ..SmartFolderFilter::default() };
    queried(&mut harness.model, "child", result("子级", 2, filter));
    harness.sync();
    harness.flush();
    assert!(harness.find("2 条结果 · 格式 psd").is_some(), "页头没有写摘要行");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolder("other".into())));
    harness.flush();
    assert_eq!(title(&harness).as_deref(), Some("别的"));
    assert!(harness.find("2 条结果 · 格式 psd").is_none(), "换了文件夹还写着上一个的摘要");
}
