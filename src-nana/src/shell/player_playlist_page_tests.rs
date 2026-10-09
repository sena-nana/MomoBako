//! 播放集页常驻写法的回归。播放集路由现在还是旧视图路由，这里把页面单独挂在一份文档里，信号常驻，
//! 照常驻路由的样子写投影：只有当前播放或页眉变了时列表不重建、字段原地改；重排和增删时整个列表
//! 重建，焦点回到原来那个条目的同一个按钮上；没点开到点开时空框和面板原地互换。每次写完都和
//! 同一 ViewModel 新挂的页面按无障碍树比一次。

use nana_ui::runtime::view::{widget, IntoView};
use nana_ui::runtime::{DocumentId, Entity, LayoutViewport, MountedView, ReorderList, RuntimeDocument, Stack, StableNodeId};
use nana_ui::NanaTextShaper;

use super::{PlaylistPageSignals, PlaylistPageView};
use crate::shell::ShellViewModel;

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 打开了演示播放列表、两首曲子的 ViewModel。
fn listed() -> ShellViewModel {
    scene("playlist-open")
}

/// 单独挂在一份文档里的播放集页，信号在挂载作用域里常驻。
struct PageHarness {
    document: RuntimeDocument,
    signals: PlaylistPageSignals,
    shaper: NanaTextShaper,
    _view: MountedView,
}

impl PageHarness {
    fn mount(model: &ShellViewModel) -> Self {
        let mut document = RuntimeDocument::new(DocumentId::new(1).expect("文档编号"));
        let document_id = document.document();
        let mut made = None;
        let view = document
            .context_mut()
            .mount_view_root(document_id, || {
                let signals = PlaylistPageSignals::new(PlaylistPageView::project(model));
                made = Some(signals);
                widget(Stack::column(0.0)).children((super::view(signals, ().into_any()),))
            })
            .expect("挂载播放集页");
        let mut harness = Self { document, signals: made.expect("挂载闭包已经运行"), shaper: NanaTextShaper::default(), _view: view };
        harness.flush();
        harness
    }

    /// 写入这份 ViewModel 的投影，再刷新绑定和布局；写完和同一 ViewModel 新挂的页面比一次。
    fn write(&mut self, model: &ShellViewModel) {
        self.signals.write(PlaylistPageView::project(model));
        self.flush();
        let fresh = Self::mount(model);
        assert_eq!(semantic_lines(&self.document), semantic_lines(&fresh.document), "增量更新后的页面和新挂的不一样");
    }

    fn flush(&mut self) {
        self.document.flush(LayoutViewport::new(1000.0, 1600.0), &mut self.shaper).expect("布局");
    }

    /// 键路径最后一段是 `key` 的第一个节点。
    fn keyed(&self, key: &str) -> Option<StableNodeId> {
        let context = self.document.context();
        context
            .world()
            .document_order(self.document.document())
            .into_iter()
            .find(|id| context.assembly_path(*id).is_some_and(|path| path.rsplit('/').next() == Some(key)))
    }

    fn key_of(&self, id: StableNodeId) -> Option<String> {
        self.document.context().assembly_path(id).and_then(|path| path.rsplit('/').next().map(str::to_string))
    }

    fn focused(&self) -> Option<StableNodeId> {
        self.document.context().world().focused(self.document.document())
    }

    fn focus(&mut self, id: StableNodeId) {
        let document = self.document.document();
        self.document.context_mut().focus_node(document, id).expect("聚焦");
    }

    fn hidden(&self, key: &str) -> bool {
        let id = self.keyed(key).unwrap_or_else(|| panic!("播放集页缺少 {key}"));
        self.document.context().world().node_style(id).is_some_and(|style| style.layout.hidden)
    }

    fn text(&self, key: &str) -> String {
        let id = self.keyed(key).unwrap_or_else(|| panic!("播放集页缺少 {key}"));
        self.document.context().world().text(id).unwrap_or_default().to_string()
    }

    /// 条目列表里选中的条目编号。
    fn selected(&self) -> Option<String> {
        let list = self.keyed("playlist-reorder").expect("条目列表");
        self.document
            .context()
            .read(Entity::<ReorderList>::from_stable_id(list), |list| list.selected_value().map(|value| value.to_string()))
            .expect("读条目列表")
    }
}

/// 文档的无障碍树：角色、名称、值和布局盒（取到 0.1 像素），节点编号不比。
fn semantic_lines(document: &RuntimeDocument) -> Vec<String> {
    document
        .context()
        .world()
        .project_accessibility(document.document())
        .into_iter()
        .map(|node| {
            let bounds = node.bounds;
            format!(
                "{:?} {:?} {:?} {:.1},{:.1} {:.1}x{:.1}",
                node.role,
                node.label.as_deref().unwrap_or(""),
                node.value.as_ref().map(|value| value.as_str()).unwrap_or(""),
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height
            )
        })
        .collect()
}

#[test]
fn projection_reads_the_playlist_detail() {
    let view = PlaylistPageView::project(&listed());
    assert!(view.head.listed);
    assert_eq!(view.head.name, "演示播放列表");
    assert!(view.head.subline.starts_with("音频 · 2 项"), "{}", view.head.subline);
    assert_eq!(view.items.iter().map(|item| item.item_id.as_str()).collect::<Vec<_>>(), ["item-01", "item-02"]);
    assert_eq!(view.items[0].mark, "MP3");
    assert!(view.items.iter().all(|item| item.ready));
    assert_eq!(PlaylistPageView::project(&listed()), view, "同一状态的投影相等");
    let pending = ShellViewModel::for_page(crate::shell::ShellPage::Playlists);
    assert!(!PlaylistPageView::project(&pending).head.listed, "详情没读回时只有空框");
}

/// 只有当前播放变了：列表和行一个都不重建，列表里选中的条目原地换。
#[test]
fn a_current_item_change_keeps_the_list() {
    let mut model = listed();
    let mut harness = PageHarness::mount(&model);
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let rows = ["playlist-item-item-01", "playlist-item-item-02"].map(|key| harness.keyed(key).expect("条目行"));
    assert_eq!(harness.selected(), None);

    model.player.current_id = Some("item-02".into());
    harness.write(&model);
    assert_eq!(harness.keyed("playlist-reorder"), Some(list), "只有当前播放变了，列表不该重建");
    assert_eq!(["playlist-item-item-01", "playlist-item-item-02"].map(|key| harness.keyed(key).expect("条目行")), rows);
    assert_eq!(harness.selected().as_deref(), Some("item-02"));

    let detail = model.player.listed.as_mut().expect("详情");
    detail.playlist.name = "新名字".into();
    detail.items[0].status = "missing".into();
    detail.items[0].status_reason = Some("文件不存在".into());
    harness.write(&model);
    assert_eq!(harness.keyed("playlist-reorder"), Some(list), "页眉和条目字段变了，列表不该重建");
    assert_eq!(harness.text("playlist-page-title"), "新名字");
    assert!(harness.hidden("playlist-item-path-item-01") && !harness.hidden("playlist-item-status-item-01"));
    assert_eq!(harness.text("playlist-item-status-item-01"), "文件不存在");
}

/// 重排：整个列表重建，焦点回到原来那个条目的「移除」上，哪怕它挪了位置。
#[test]
fn a_reorder_rebuilds_the_list_and_keeps_focus_on_the_item() {
    let mut model = listed();
    let mut harness = PageHarness::mount(&model);
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let remove = harness.keyed("playlist-remove-item-02").expect("第二条的移除");
    harness.focus(remove);
    assert_eq!(harness.focused(), Some(remove));

    model.player.listed.as_mut().expect("详情").items.reverse();
    harness.write(&model);
    assert_ne!(harness.keyed("playlist-reorder"), Some(list), "重排要重建列表");
    let focused = harness.focused().expect("重排后焦点应该找回来");
    assert_ne!(focused, remove, "焦点应在新列表的节点上");
    assert_eq!(harness.key_of(focused).as_deref(), Some("playlist-remove-item-02"), "焦点没回到原来那个条目上");
    let world = harness.document.context().world();
    let new_list = harness.keyed("playlist-reorder").expect("新列表");
    let first = world.node(new_list).and_then(|node| node.children.first().copied()).expect("第一行");
    assert_eq!(harness.key_of(first).as_deref(), Some("playlist-item-item-02"), "行要按新顺序排");

    let mut added = model.clone();
    let mut extra = added.player.listed.as_ref().expect("详情").items[0].clone();
    extra.playlist_item_id = "item-03".into();
    extra.filename = "track-03.mp3".into();
    added.player.listed.as_mut().expect("详情").items.push(extra);
    harness.write(&added);
    assert!(harness.keyed("playlist-item-item-03").is_some(), "新条目没有建出来");
    assert_eq!(harness.key_of(harness.focused().expect("焦点")).as_deref(), Some("playlist-remove-item-02"), "加一条以后焦点要留在原条目上");
}

/// 没点开到点开：空框和面板原地互换，列表按条目建出来；再清空条目时换成「还是空的」空框。
#[test]
fn listing_swaps_the_empty_frame_for_the_panel() {
    let mut model = listed();
    let detail = model.player.listed.take();
    let mut harness = PageHarness::mount(&model);
    let empty = harness.keyed("playlist-page-empty-frame").expect("没点开时的空框");
    assert!(!harness.hidden("playlist-page-empty-frame") && harness.hidden("playlist-page"));

    model.player.listed = detail;
    harness.write(&model);
    assert_eq!(harness.keyed("playlist-page-empty-frame"), Some(empty));
    assert!(harness.hidden("playlist-page-empty-frame") && !harness.hidden("playlist-page"));
    assert!(harness.keyed("playlist-item-item-01").is_some(), "点开后应建出条目");

    model.player.listed.as_mut().expect("详情").items.clear();
    harness.write(&model);
    assert!(!harness.hidden("playlist-page-no-items-frame"), "条目清空后显示「还是空的」");
}
