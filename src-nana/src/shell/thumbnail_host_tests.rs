//! 缩略图宿主纹理（`thumbnail_host.rs`）从产品的准备帧走：空闲帧不再登记、不叫重画；挂着的纹理不卸，
//! 换了目录卸掉不在列表里的，滚出视口的超过上限按最近最少使用卸，卸掉的行再滚回来时重新解码。
//!
//! 每一帧照产品 `prepare`：准备帧（计时器、动效、挂着的缩略图、预取、同步）、布局，再上传和回收纹理。
//! 解码请求当场“解出”2×2 的像素，像 `update` 那样先交给宿主再归约。纹理表记下每一次登记和移除。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use nana_ui::runtime::{component_descriptors, LayoutBox};
use nana_ui::FrameDemand;

use crate::backend::services::repository::{FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState};
use crate::thumbnail_host::{TextureTable, ThumbnailHost};

use super::files::{DisplayMode, FileRow, FilesMessage};
use super::view_harness::ShellHarness;
use super::{thumbnail_slot, FilesEffect, ShellMessage, ShellViewModel, ThumbnailFrame};

/// 记下登记和移除的纹理表。`live` 是现在登记着的槽。
#[derive(Default)]
struct Table {
    calls: Vec<String>,
    live: BTreeSet<String>,
}

impl TextureTable<u32> for Table {
    fn register(&mut self, slot: &str, _texture: &u32, _width: u32, _height: u32) {
        self.calls.push(format!("登记 {slot}"));
        self.live.insert(slot.to_string());
    }

    fn remove(&mut self, slot: &str) {
        self.calls.push(format!("移除 {slot}"));
        self.live.remove(slot);
    }
}

/// 一扇窗口：生产壳层、缩略图宿主端和记账的纹理表。
struct Window {
    harness: ShellHarness,
    host: ThumbnailHost<u32>,
    table: Table,
    /// 移除时还挂着的缩略图（会闪回类型图标）。
    flashed: Vec<String>,
}

impl Window {
    fn mount(model: ShellViewModel, capacity: usize) -> Self {
        Self { harness: ShellHarness::mount(model), host: ThumbnailHost::with_capacity(capacity), table: Table::default(), flashed: Vec::new() }
    }

    /// 产品的一帧，再把这一帧排下的解码请求当场送回。移除的槽要是这一帧还挂着，记进 `flashed`。
    fn frame(&mut self) {
        self.harness.frame();
        let before = self.table.calls.len();
        self.host.publish(&mut self.harness.model, &mut self.table, |_| Ok(7));
        let mounted = self.mounted();
        for call in &self.table.calls[before..] {
            if let Some(path) = call.strip_prefix("移除 thumb:").filter(|path| mounted.contains(*path)) {
                self.flashed.push(path.to_string());
            }
        }
        self.decode();
    }

    /// 走够一秒（60 帧）：空闲预取的 420ms 等得到，解码回来的也登记了。
    fn settle(&mut self) {
        for _ in 0..60 {
            self.frame();
        }
    }

    fn decode(&mut self) {
        let paths: Vec<String> = self
            .harness
            .model
            .files
            .take_effects()
            .into_iter()
            .flat_map(|effect| match effect {
                FilesEffect::DecodeThumbnails { paths } => paths,
                _ => Vec::new(),
            })
            .collect();
        if paths.is_empty() {
            return;
        }
        let mut frames: Vec<ThumbnailFrame> = paths
            .iter()
            .map(|path| ThumbnailFrame { path: path.clone(), natural_width: 64, natural_height: 48, width: 2, height: 2, rgba: vec![255; 16] })
            .collect();
        self.host.queue(&mut frames);
        self.harness.apply(ShellMessage::ThumbnailPixels(frames));
    }

    fn mounted(&self) -> HashSet<String> {
        self.harness.model.files.mounted_thumbnails().cloned().unwrap_or_default()
    }

    fn registered(&self, path: &str) -> bool {
        self.table.live.contains(&thumbnail_slot(path))
    }

    fn ready(&self, path: &str) -> bool {
        self.harness.model.files.rows.iter().any(|row| row.thumbnail_path.as_deref() == Some(path) && row.texture_ready)
    }

    /// 文件列表滚动区的框。
    fn list_box(&self) -> LayoutBox {
        let document = self.harness.document();
        let context = document.context();
        let id = context
            .world()
            .nodes_of_component(document.document(), component_descriptors::SCROLL_VIEW.type_id)
            .find(|node| context.assembly_path(*node).is_some_and(|path| path.rsplit('/').next().is_some_and(|key| key.starts_with("files-scroll-"))))
            .expect("文件列表滚动区");
        context.world().layout_box(id).expect("滚动区有框")
    }

    /// 在文件列表上滚一下滚轮，负数往下。
    fn scroll(&mut self, delta_y: f32) {
        let area = self.list_box();
        self.harness.wheel(area.x + area.width / 2.0, area.y + area.height / 2.0, delta_y);
    }
}

fn entry(path: &str) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        kind: "file".into(),
        extension: Some("png".into()),
        size_bytes: Some(1024),
        size_label: Some("1 KB".into()),
        modified_at: None,
        asset_id: Some(path.into()),
        status: None,
        thumbnail_path: Some(format!("thumbs/{path}")),
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// `directory` 下 `count` 张图片的目录快照，缩略图在 `thumbs/` 下。
fn listing(repo_id: &str, directory: &str, count: usize) -> FileBrowserSnapshot {
    let entries: Vec<FileBrowserEntry> = (0..count)
        .map(|index| {
            let name = format!("img-{index:03}.png");
            entry(&if directory.is_empty() { name } else { format!("{directory}/{name}") })
        })
        .collect();
    FileBrowserSnapshot {
        repo_id: repo_id.into(),
        root_path: "D:/Libraries/Anime".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: directory.into(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

/// 文件页场景换成 `count` 张图片的根目录，按列表展示（一行一张，行高至少 72）。
fn files_model(count: usize) -> ShellViewModel {
    let mut model = super::acceptance_gap_models().into_iter().find(|(name, _)| *name == "live-files").expect("文件页场景").1;
    model.files.import_open = false;
    model.files.eagle_open = false;
    model.files.display_mode = DisplayMode::List;
    let repo_id = model.workspace.active_repo_id.clone().expect("验收仓库");
    model.files.rows = listing(&repo_id, "", count).entries.iter().map(FileRow::from_entry).collect();
    model
}

/// 缩略图登记好以后，接着走的空闲帧既不再登记，也不移除，不标脏，不要连续帧：宿主纹理表不会叫窗口重画。
#[test]
fn idle_frames_neither_register_again_nor_ask_for_a_redraw() {
    let mut window = Window::mount(files_model(3), 240);
    window.settle();
    for index in 0..3 {
        let path = format!("thumbs/img-{index:03}.png");
        assert!(window.registered(&path), "{path} 该登记了：{:?}", window.table.calls);
        assert!(window.ready(&path));
    }
    assert_eq!(window.table.calls.len(), 3, "每张只登记一次：{:?}", window.table.calls);
    let revision = window.harness.model.revision;
    for _ in 0..5 {
        window.frame();
    }
    assert_eq!(window.table.calls.len(), 3, "空闲帧又碰了纹理表：{:?}", window.table.calls);
    assert_eq!(window.harness.model.revision, revision, "空闲帧不该标脏");
    assert_eq!(window.harness.window.demand, FrameDemand::OnDemand, "空闲时不要连续帧");
}

/// 往下滚：滚出视口的行留作缓存，超过上限时最早滚走的先卸，挂着的一张也不卸（不闪）——选中的第一张一直
/// 画在右侧详情里，它最早登记、最久没在列表里出现，也不卸；卸掉的行回到类型图标。滚回顶上，这些行重新
/// 解码、重新登记。
#[test]
fn scrolled_away_rows_give_up_their_textures_least_recently_used_first() {
    let mut window = Window::mount(files_model(60), 24);
    window.settle();
    let top = window.mounted();
    assert!(top.len() >= 6 && top.len() < 24, "首屏挂着的缩略图：{top:?}");
    assert!(top.iter().all(|path| window.registered(path)));
    window.harness.apply(ShellMessage::Files(FilesMessage::ActivateRow("img-000.png".into())));
    let pinned = "thumbs/img-000.png";
    for _ in 0..8 {
        window.scroll(-400.0);
        window.settle();
    }
    let bottom = window.mounted();
    assert!(bottom.contains(pinned) && window.registered(pinned) && window.ready(pinned), "详情里的缩略图一直挂着，不卸");
    let bottom: HashSet<String> = bottom.into_iter().filter(|path| path != pinned).collect();
    assert!(bottom.is_disjoint(&top), "滚到下面以后首屏的行不再挂着：{bottom:?}");
    assert!(bottom.iter().all(|path| window.registered(path)), "挂着的都解码登记了");
    assert!(window.host.registered() <= 24, "缓存不超过上限：{}", window.host.registered());
    assert!(window.flashed.is_empty(), "卸了挂着的缩略图：{:?}", window.flashed);
    let released: Vec<&String> = top.iter().filter(|path| !window.registered(path)).collect();
    assert!(!released.is_empty(), "首屏的行最早滚走，超过上限时先卸：首屏 {top:?}，登记着 {:?}", window.table.live);
    assert!(released.iter().all(|path| !window.ready(path)), "卸掉的行回到类型图标");

    window.scroll(100_000.0);
    window.settle();
    let again = window.mounted();
    assert_eq!(again, top, "滚回顶上，挂着的还是首屏那些");
    assert!(again.iter().all(|path| window.registered(path) && window.ready(path)), "卸掉的行重新解码登记");
    assert!(window.flashed.is_empty(), "卸了挂着的缩略图：{:?}", window.flashed);
}

/// 换目录：新列表到之前旧卡片还挂着，纹理留着；新列表换上、旧卡片卸了以后，不在列表里的纹理立刻卸掉，
/// 新目录的缩略图解码登记。重读同一个目录时卡片不闪回类型图标，也不重新解码。
#[test]
fn changing_directory_drops_the_old_textures_and_a_reload_keeps_them() {
    let mut window = Window::mount(files_model(4), 240);
    window.settle();
    let root: Vec<String> = (0..4).map(|index| format!("thumbs/img-{index:03}.png")).collect();
    assert!(root.iter().all(|path| window.registered(path)));
    let repo_id = window.harness.model.workspace.active_repo_id.clone().expect("验收仓库");

    let calls = window.table.calls.len();
    window.harness.apply(ShellMessage::FileBrowserLoaded(Ok(listing(&repo_id, "", 4))));
    window.settle();
    assert_eq!(window.table.calls.len(), calls, "重读同一个目录不重新登记：{:?}", &window.table.calls[calls..]);
    assert!(root.iter().all(|path| window.ready(path)), "重读以后卡片还是缩略图");

    window.harness.apply(ShellMessage::FileBrowserLoaded(Ok(listing(&repo_id, "photos", 2))));
    window.settle();
    assert!(root.iter().all(|path| !window.registered(path)), "旧目录的纹理卸掉：{:?}", window.table.live);
    for index in 0..2 {
        assert!(window.registered(&format!("thumbs/photos/img-{index:03}.png")), "新目录的缩略图登记了");
    }
    assert!(window.flashed.is_empty(), "卸了挂着的缩略图：{:?}", window.flashed);
}
