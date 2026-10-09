//! 文件列表的卡片仓：本目录每一张卡片的投影，以及两组卡片排好的行。
//!
//! 同步时把可见的文件行投影成卡片，按条目键写进卡片仓（[`Cards`]），只写变了的那几张；
//! 两组卡片（文件夹、文件）按展示方式和列表宽度排成行，行的身份没变时不写，虚拟列表不动。
//!
//! 算法：每条文件行先算一个指纹（卡片读到的字段），指纹没变就沿用上一份卡片（同一个 `Arc`），
//! 所以一批缩略图到达时只重算到达的那几张；卡片仓按「同一份卡片、选中和放置态相同」逐张比较，
//! 键的顺序没变时只 `set` 变了的那几张，增删或换序时整体写一次（只有建出来的卡片会重读）。

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui::runtime::view::{signal, store, Signal, StoreList, StorePath};

use super::super::files::{DisplayMode, FileContext, FileRow, FilesState};
use super::super::ShellViewModel;
use super::cards::{card_key, CardFace, CardView, Cards};
use super::virtual_rows::{self, GroupLines, GroupShape};

/// 一组卡片的排法和行。
#[derive(Clone, Copy)]
pub(super) struct GroupSignals {
    pub shape: Signal<GroupShape>,
    pub lines: Signal<GroupLines>,
}

/// 卡片仓和两组卡片的信号。句柄可复制，值在主区块的常驻作用域里。
#[derive(Clone, Copy)]
pub(super) struct BoardSignals {
    pub cards: Cards,
    /// 第 0 组是文件夹，第 1 组是文件；分类视图和智能文件夹不分组，全在第 1 组。
    pub groups: [GroupSignals; 2],
    cache: Signal<BoardCache>,
}

/// 上次同步留下的东西：每条行的指纹和卡片、写进卡片仓的卡片，以及上次排行用的输入。
#[derive(Default)]
struct BoardCache {
    faces: HashMap<Arc<str>, (u64, Arc<CardFace>)>,
    cards: Vec<CardView>,
    packed: [Option<PackInput>; 2],
}

/// 排行用到的输入：每张卡片的键、宽高比和小字行数，加上展示方式和列表宽。
#[derive(Clone, PartialEq)]
struct PackInput {
    cards: Vec<(Arc<str>, u32, usize)>,
    mode: DisplayMode,
    width: u32,
}

/// 两组卡片的展示有没有内容：文件夹、文件各有几张。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct GroupPresence {
    pub directories: bool,
    pub files: bool,
}

impl BoardSignals {
    /// 在常驻作用域里建空的卡片仓和两组的信号；第一次 [`Self::write`] 写入内容。
    pub(super) fn new() -> Self {
        let group = || GroupSignals { shape: signal(GroupShape::default()), lines: signal(GroupLines::default()) };
        Self { cards: store(Vec::new()), groups: [group(), group()], cache: signal(BoardCache::default()) }
    }

    /// 投影可见的文件行，只写变了的卡片和行。返回两组各有没有卡片。
    pub(super) fn write(&self, model: &ShellViewModel) -> GroupPresence {
        if self.cache.defined_at().is_none() {
            return GroupPresence::default();
        }
        let mut cache = BoardCache::default();
        self.cache.update(|slot| std::mem::swap(slot, &mut cache));
        let presence = self.project(model, &mut cache);
        self.cache.update(|slot| *slot = cache);
        presence
    }

    fn project(&self, model: &ShellViewModel, cache: &mut BoardCache) -> GroupPresence {
        let ctx = FileContext::from_model(model);
        let rows = visible_rows(&model.files, &ctx);
        let picked = picked_paths(&model.files);
        let drop_folder = (model.input.internal_active || model.input.dragging_files).then(|| model.input.hover_folder.as_deref()).flatten();
        let bindings = super::cards::builtin_bindings();
        let mut faces = HashMap::with_capacity(rows.len());
        let cards = rows
            .iter()
            .map(|row| {
                let print = fingerprint(row);
                let key = row.key();
                let face = match cache.faces.get(key.as_str()) {
                    Some((known, face)) if *known == print => face.clone(),
                    _ => Arc::new(CardFace::of(row, bindings)),
                };
                faces.insert(face.key.clone(), (print, face.clone()));
                CardView {
                    selected: picked.contains(row.path.as_str()),
                    drop_target: row.kind == "directory" && drop_folder == Some(row.path.as_str()),
                    face,
                }
            })
            .collect::<Vec<_>>();
        cache.faces = faces;
        self.write_cards(cache, cards);
        let virtual_view = ctx.is_virtual();
        let (directories, files): (Vec<_>, Vec<_>) = if virtual_view {
            (Vec::new(), cache.cards.iter().map(|card| card.face.clone()).collect())
        } else {
            cache.cards.iter().map(|card| card.face.clone()).partition(|face| &*face.kind == "directory")
        };
        let presence = GroupPresence { directories: !directories.is_empty(), files: !files.is_empty() };
        let mode = model.files.display_mode;
        let width = model.files.list_width.unwrap_or_else(|| estimated_list_width(model));
        for (index, faces) in [directories, files].into_iter().enumerate() {
            let input = PackInput {
                cards: faces.iter().map(|face| (face.key.clone(), face.aspect().to_bits(), face.body_lines())).collect(),
                mode,
                width: width.to_bits(),
            };
            if cache.packed[index].as_ref() == Some(&input) {
                continue;
            }
            let (shape, lines) = virtual_rows::pack(&faces, mode, width);
            self.groups[index].shape.try_set_if_changed(shape);
            self.groups[index].lines.try_set_if_changed(lines);
            cache.packed[index] = Some(input);
        }
        presence
    }

    /// 写卡片仓：键的顺序没变时只写变了的那几张；增删或换序时整体写一次。
    fn write_cards(&self, cache: &mut BoardCache, cards: Vec<CardView>) {
        let same_keys = cache.cards.len() == cards.len() && cache.cards.iter().zip(&cards).all(|(old, new)| old.face.key == new.face.key);
        if same_keys {
            let keyed = self.cards.keyed(card_key as fn(&CardView) -> Arc<str>);
            for (old, new) in cache.cards.iter().zip(&cards) {
                if !old.same_as(new) {
                    keyed.at(&new.face.key).set(new.clone());
                }
            }
        } else {
            self.cards.set(cards.clone());
        }
        cache.cards = cards;
    }
}

/// 可见的文件行，不复制：智能文件夹用查询结果；分类视图隐藏文件夹。和 `FilesState::visible_rows` 同一规则。
fn visible_rows<'a>(files: &'a FilesState, ctx: &FileContext) -> Vec<&'a FileRow> {
    if ctx.smart_folder {
        return files.virtual_rows.iter().collect();
    }
    if ctx.category_virtual {
        return files.rows.iter().filter(|row| row.kind != "directory").collect();
    }
    files.rows.iter().collect()
}

/// 多选路径加主选。主选不在多选里时也算选中。
fn picked_paths(files: &FilesState) -> HashSet<&str> {
    files.selected.iter().map(String::as_str).chain(files.primary.as_deref()).collect()
}

/// 卡片读到的字段的指纹。像素按缓冲区地址和长度认：缩略图像素整份替换，不原地改。
fn fingerprint(row: &FileRow) -> u64 {
    let mut hasher = DefaultHasher::new();
    row.kind.hash(&mut hasher);
    row.path.hash(&mut hasher);
    row.name.hash(&mut hasher);
    row.extension.hash(&mut hasher);
    row.size_label.hash(&mut hasher);
    row.modified_at.hash(&mut hasher);
    row.hardlink_state.hash(&mut hasher);
    row.thumbnail_path.hash(&mut hasher);
    row.texture_ready.hash(&mut hasher);
    row.pixel_width.hash(&mut hasher);
    row.pixel_height.hash(&mut hasher);
    if let Some((width, height, rgba)) = &row.thumbnail_rgba {
        (width, height, rgba.as_ptr() as usize, rgba.len()).hash(&mut hasher);
    }
    hasher.finish()
}

/// 还没有布局回报时，按窗口宽估算列表内容宽：减去侧栏、主区左右 24、详情 300 和间距 18、
/// 卡片描边和列表左右内边距。窗口不超过 900 时详情排到下面，不再减详情宽。
pub(super) fn estimated_list_width(model: &ShellViewModel) -> f32 {
    let sidebar = model.motion.sidebar_presented_width();
    let detail = if model.viewport_width <= 900.0 { 0.0 } else { 300.0 + 18.0 };
    (model.viewport_width - sidebar - 48.0 - detail - 2.0 - super::grid::LIST_PADDING_X * 2.0).max(virtual_rows::MASONRY_COLUMN)
}
