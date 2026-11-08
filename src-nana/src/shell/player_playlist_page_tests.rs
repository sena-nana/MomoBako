//! 播放集页的回归：把页面单独挂在一份文档里，信号常驻，照主区块同步的样子写投影：只有当前播放或
//! 页眉变了时列表不重建、字段原地改；重排和增删时列表和留下的行都不重建，焦点留在原来的按钮上；
//! 没点开到点开时空框和面板原地互换。每次写完都和同一 ViewModel 新挂的页面按无障碍树比一次。
//! 整条路由（筛选栏、播放条岛、不可播放项目、拖动排序和移除）的回归在 `route_playlists_tests.rs`。

use nana_ui::NanaTextShaper;
use nana_ui::runtime::view::{IntoView, widget};
use nana_ui::runtime::{
    DocumentId, Entity, LayoutViewport, MountedView, ReorderList, RuntimeDocument, StableNodeId,
    Stack,
};

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
        let mut harness = Self {
            document,
            signals: made.expect("挂载闭包已经运行"),
            shaper: NanaTextShaper::default(),
            _view: view,
        };
        harness.flush();
        harness
    }

    /// 写入这份 ViewModel 的投影，再刷新绑定和布局；写完和同一 ViewModel 新挂的页面比一次。
    fn write(&mut self, model: &ShellViewModel) {
        self.signals.write(PlaylistPageView::project(model));
        self.flush();
        let fresh = Self::mount(model);
        assert_eq!(
            semantic_lines(&self.document),
            semantic_lines(&fresh.document),
            "增量更新后的页面和新挂的不一样"
        );
    }

    fn flush(&mut self) {
        self.document
            .flush(LayoutViewport::new(1000.0, 1600.0), &mut self.shaper)
            .expect("布局");
    }

    /// 键路径最后一段是 `key` 的第一个节点。
    fn keyed(&self, key: &str) -> Option<StableNodeId> {
        let context = self.document.context();
        context
            .world()
            .document_order(self.document.document())
            .into_iter()
            .find(|id| {
                context
                    .assembly_path(*id)
                    .is_some_and(|path| path.rsplit('/').next() == Some(key))
            })
    }

    fn focused(&self) -> Option<StableNodeId> {
        self.document
            .context()
            .world()
            .focused(self.document.document())
    }

    fn focus(&mut self, id: StableNodeId) {
        let document = self.document.document();
        self.document
            .context_mut()
            .focus_node(document, id)
            .expect("聚焦");
    }

    fn hidden(&self, key: &str) -> bool {
        let id = self
            .keyed(key)
            .unwrap_or_else(|| panic!("播放集页缺少 {key}"));
        self.document
            .context()
            .world()
            .node_style(id)
            .is_some_and(|style| style.layout.hidden)
    }

    fn text(&self, key: &str) -> String {
        let id = self
            .keyed(key)
            .unwrap_or_else(|| panic!("播放集页缺少 {key}"));
        self.document
            .context()
            .world()
            .text(id)
            .unwrap_or_default()
            .to_string()
    }

    /// 条目列表里选中的条目编号。
    fn selected(&self) -> Option<String> {
        let list = self.keyed("playlist-reorder").expect("条目列表");
        self.document
            .context()
            .read(Entity::<ReorderList>::from_stable_id(list), |list| {
                list.selected_value().map(|value| value.to_string())
            })
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
                node.value
                    .as_ref()
                    .map(|value| value.as_str())
                    .unwrap_or(""),
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
    assert!(
        view.head.subline.starts_with("音频 · 2 项"),
        "{}",
        view.head.subline
    );
    assert_eq!(
        view.items
            .iter()
            .map(|item| item.item_id.as_str())
            .collect::<Vec<_>>(),
        ["item-01", "item-02"]
    );
    assert_eq!(view.items[0].mark, "MP3");
    assert!(view.items.iter().all(|item| item.ready));
    assert_eq!(
        PlaylistPageView::project(&listed()),
        view,
        "同一状态的投影相等"
    );
    let pending = ShellViewModel::for_page(crate::shell::ShellPage::Playlists);
    assert!(
        !PlaylistPageView::project(&pending).head.listed,
        "详情没读回时只有空框"
    );
}

/// 只有当前播放变了：列表和行一个都不重建，列表里选中的条目原地换。
#[test]
fn a_current_item_change_keeps_the_list() {
    let mut model = listed();
    let mut harness = PageHarness::mount(&model);
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let rows = ["playlist-item-item-01", "playlist-item-item-02"]
        .map(|key| harness.keyed(key).expect("条目行"));
    assert_eq!(harness.selected(), None);

    model.player.current_id = Some("item-02".into());
    harness.write(&model);
    assert_eq!(
        harness.keyed("playlist-reorder"),
        Some(list),
        "只有当前播放变了，列表不该重建"
    );
    assert_eq!(
        ["playlist-item-item-01", "playlist-item-item-02"]
            .map(|key| harness.keyed(key).expect("条目行")),
        rows
    );
    assert_eq!(harness.selected().as_deref(), Some("item-02"));

    let detail = model.player.listed.as_mut().expect("详情");
    detail.playlist.name = "新名字".into();
    detail.items[0].status = "missing".into();
    detail.items[0].status_reason = Some("文件不存在".into());
    harness.write(&model);
    assert_eq!(
        harness.keyed("playlist-reorder"),
        Some(list),
        "页眉和条目字段变了，列表不该重建"
    );
    assert_eq!(harness.text("playlist-page-title"), "新名字");
    assert!(
        harness.hidden("playlist-item-path-item-01")
            && !harness.hidden("playlist-item-status-item-01")
    );
    assert_eq!(harness.text("playlist-item-status-item-01"), "文件不存在");
}

/// 条目列表里各行的节点，按显示顺序。行直接是列表的子节点，中间没有别的容器。
fn rows(harness: &PageHarness) -> Vec<StableNodeId> {
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    harness
        .document
        .context()
        .world()
        .node(list)
        .map(|node| node.children.to_vec())
        .unwrap_or_default()
}

/// 重排、加一条、删一条：列表和留下的行都是原来的节点，只按新顺序挪位置；焦点一直在原来那个按钮上。
#[test]
fn reordering_adding_and_removing_keep_the_remaining_rows() {
    let mut model = listed();
    let mut harness = PageHarness::mount(&model);
    let list = harness.keyed("playlist-reorder").expect("条目列表");
    let [first, second] = ["playlist-item-item-01", "playlist-item-item-02"]
        .map(|key| harness.keyed(key).expect("条目行"));
    assert_eq!(rows(&harness), [first, second], "行直接建在列表里");
    let remove = harness
        .keyed("playlist-remove-item-02")
        .expect("第二条的移除");
    harness.focus(remove);

    model.player.listed.as_mut().expect("详情").items.reverse();
    harness.write(&model);
    assert_eq!(
        harness.keyed("playlist-reorder"),
        Some(list),
        "重排不该重建列表"
    );
    assert_eq!(
        rows(&harness),
        [second, first],
        "行按新顺序挪位置，节点不换"
    );
    assert_eq!(harness.focused(), Some(remove), "焦点留在原来那个按钮上");

    let mut extra = model.player.listed.as_ref().expect("详情").items[0].clone();
    extra.playlist_item_id = "item-03".into();
    extra.filename = "track-03.mp3".into();
    model
        .player
        .listed
        .as_mut()
        .expect("详情")
        .items
        .push(extra);
    harness.write(&model);
    let third = harness
        .keyed("playlist-item-item-03")
        .expect("新条目没有建出来");
    assert_eq!(rows(&harness), [second, first, third], "加一条只建新的一行");
    assert_eq!(
        harness.focused(),
        Some(remove),
        "加一条以后焦点留在原条目上"
    );

    model
        .player
        .listed
        .as_mut()
        .expect("详情")
        .items
        .retain(|item| item.playlist_item_id != "item-01");
    harness.write(&model);
    assert_eq!(rows(&harness), [second, third], "删一条只拿掉那一行");
    assert!(
        !harness.document.context().world().contains(first),
        "删掉的行回收"
    );
    assert_eq!(harness.focused(), Some(remove));
    assert_eq!(harness.keyed("playlist-reorder"), Some(list));
}

/// 58px 方块照 Vue `playlistItemThumbnailSrc`：可读的本地 `thumbnailPath` 按 cover 画上，扩展名不再叠在图上；
/// 路径空、只有网址、读失败或不是图片时仍写大写扩展名，不下载、不编造像素。换封面不重建这一格，点它仍打开预览。
#[test]
fn playlist_row_draws_the_local_thumbnail_or_keeps_the_extension() {
    let dir = std::env::temp_dir().join(format!("momobako-playlist-cover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let red = write_png(&dir, "red.png", [200, 20, 20, 255]);
    let note = dir.join("note.txt");
    std::fs::write(&note, b"not an image").expect("写入非图片");
    let red_url = encoded(&red, [200, 20, 20, 255]);
    let missing = dir.join("missing.png").to_string_lossy().into_owned();
    let note_path = note.to_string_lossy().into_owned();

    let mut model = listed();
    model.player.listed.as_mut().expect("详情").items[0].thumbnail_path = Some(red.clone());
    let mut harness = PageHarness::mount(&model);
    let mark = harness
        .keyed("playlist-item-ext-item-01")
        .expect("第一条的方块");
    assert_cover(
        &harness,
        "playlist-item-ext-item-01",
        Some(red_url.as_str()),
    );
    assert_eq!(
        harness.text("playlist-item-ext-item-01"),
        "",
        "有封面时不写扩展名"
    );
    assert_eq!(
        harness.text("playlist-item-ext-item-02"),
        "MP3",
        "没有缩略图的条目仍写扩展名"
    );
    assert!(
        painted(&harness, "playlist-item-ext-item-02").is_none(),
        "没有缩略图时不编造像素"
    );

    for path in [
        None,
        Some("  ".into()),
        Some(String::new()),
        Some("https://example.com/cover.png".into()),
        Some("HTTP://example.com/cover.png".into()),
        Some(missing),
        Some(note_path),
    ] {
        model.player.listed.as_mut().expect("详情").items[0].thumbnail_path = path;
        harness.write(&model);
        assert_eq!(
            harness.keyed("playlist-item-ext-item-01"),
            Some(mark),
            "换封面不该重建方块"
        );
        assert_cover(&harness, "playlist-item-ext-item-01", None);
        assert_eq!(harness.text("playlist-item-ext-item-01"), "MP3");
    }

    model.player.listed.as_mut().expect("详情").items[0].thumbnail_path = Some(red.clone());
    harness.write(&model);
    assert_eq!(
        harness.keyed("playlist-item-ext-item-01"),
        Some(mark),
        "封面回来仍是原来那一格"
    );
    assert_cover(
        &harness,
        "playlist-item-ext-item-01",
        Some(red_url.as_str()),
    );

    let mut shell = crate::shell::view_harness::ShellHarness::mount(scene("playlist-open"));
    shell.model.player.listed.as_mut().expect("详情").items[0].thumbnail_path = Some(red);
    shell.sync();
    shell.flush();
    let id = shell
        .keyed("playlist-item-ext-item-01")
        .expect("壳层里的方块");
    let (x, y) = shell.center(id);
    let messages = shell.click_messages(x, y);
    assert!(
        matches!(messages.as_slice(), [crate::shell::ShellMessage::Player(crate::shell::player::PlayerMessage::OpenPreview { item_id: Some(id) })] if id == "item-01"),
        "点封面应该打开这一条的预览：{} 条消息",
        messages.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 这一格画上的封面。没有内容图就是没画像素。
fn painted(harness: &PageHarness, key: &str) -> Option<String> {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let style = harness
        .document
        .context()
        .world()
        .node_style(id)
        .unwrap_or_else(|| panic!("{key} 没有样式"));
    match &style.layout.paint.content_image {
        Some(nana_ui_core::BackgroundImage::Url { url, fit, .. }) => {
            assert_eq!(
                *fit,
                nana_ui_core::BackgroundImageFit::Cover,
                "{key} 应按 cover 铺满"
            );
            Some(url.clone())
        }
        None => None,
        _ => panic!("{key} 的封面不是解码出的本地图"),
    }
}

/// 方块应是 58px。`image` 为 `None` 时不应有像素。
fn assert_cover(harness: &PageHarness, key: &str, image: Option<&str>) {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    let box_ = harness
        .document
        .context()
        .world()
        .layout_box(id)
        .unwrap_or_else(|| panic!("{key} 没有布局"));
    assert!(
        (box_.width - 58.0).abs() < 0.5 && (box_.height - 58.0).abs() < 0.5,
        "{key} 应是 58px，实际 {}x{}",
        box_.width,
        box_.height
    );
    assert_eq!(painted(harness, key).as_deref(), image);
}

/// 写成一张纯色 PNG。
fn write_png(dir: &std::path::Path, name: &str, color: [u8; 4]) -> String {
    let path = dir.join(name);
    image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
        .save(&path)
        .unwrap_or_else(|error| panic!("写入 {name}：{error}"));
    path.to_string_lossy().into_owned()
}

/// 和界面同一套解码、编码，用来对上画出来的 data URL。
fn encoded(path: &str, color: [u8; 4]) -> String {
    let frame = crate::shell::decode_thumbnail_file(path)
        .unwrap_or_else(|error| panic!("测试图解不开：{error}"));
    assert_eq!(&frame.rgba[..4], &color, "测试图的颜色应能对上");
    crate::shell::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
        .expect("测试图能编码")
}

/// 没点开到点开：空框和面板原地互换，列表按条目建出来；再清空条目时换成「还是空的」空框。
#[test]
fn listing_swaps_the_empty_frame_for_the_panel() {
    let mut model = listed();
    let detail = model.player.listed.take();
    let mut harness = PageHarness::mount(&model);
    let empty = harness
        .keyed("playlist-page-empty-frame")
        .expect("没点开时的空框");
    assert!(!harness.hidden("playlist-page-empty-frame") && harness.hidden("playlist-page"));

    model.player.listed = detail;
    harness.write(&model);
    assert_eq!(harness.keyed("playlist-page-empty-frame"), Some(empty));
    assert!(harness.hidden("playlist-page-empty-frame") && !harness.hidden("playlist-page"));
    assert!(
        harness.keyed("playlist-item-item-01").is_some(),
        "点开后应建出条目"
    );

    model.player.listed.as_mut().expect("详情").items.clear();
    harness.write(&model);
    assert!(
        !harness.hidden("playlist-page-no-items-frame"),
        "条目清空后显示「还是空的」"
    );
}
