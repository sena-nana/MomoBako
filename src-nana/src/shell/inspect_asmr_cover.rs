//! ASMR 作品信息的「保存封面」，以及候选卡片上的「封面」。
//!
//! 对齐 `library-asmr` 的 `saveCover`：元数据或候选字段里有 http(s) 的 `coverUrl`（没有时用 `cover`）
//! 才出现按钮。点了用 `thumbnails:ensure` 的 `save + sourceUrl` 把这张图存成自定义缩略图。
//! 不是 http(s)、回收站、没有素材，或地址不是当前文件和它的候选上的那一个，都不下载，也不编造封面。
//! 同一时间只保存一张；进行中作品信息的按钮写「保存中」，候选上的「封面」停用。

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::ShellViewModel;
use super::super::files::EnsuredThumbnail;
use super::super::inspect_library::CoverButton;

/// 正在保存的那一张。结果按仓库和路径认，换了仓库不写回列表。
#[derive(Clone, Debug, Default)]
pub(super) struct CoverSave {
    pending: Option<PendingCover>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingCover {
    repo_id: String,
    path: String,
    source_url: String,
}

/// `coverUrl`，没有时 `cover`。只接受去掉空白后的 http(s)，大小写不限。其它类型和协议返回空。
pub(super) fn remote_cover(metadata: &BTreeMap<String, Value>) -> Option<String> {
    http_text(metadata.get("coverUrl")).or_else(|| http_text(metadata.get("cover")))
}

/// 作品信息上要画的按钮。没有地址时整枚不出现。
pub(super) fn attach(
    sections: &mut [super::super::inspect_library::LibrarySection],
    model: &ShellViewModel,
) {
    let Some(works) = sections
        .iter_mut()
        .find(|section| section.key == "inspect-asmr-works")
    else {
        return;
    };
    works.cover = offer(model);
}

/// 点了「保存封面」或候选上的「封面」。地址必须是当前文件或它的候选上的 http(s)。
pub(super) fn begin(model: &mut ShellViewModel, url: String) {
    if model.asmr.cover.pending.is_some() {
        eprintln!("Nana 保存封面还在进行，不重复提交");
        return;
    }
    if model.files.trash {
        eprintln!("Nana 回收站里不保存封面");
        return;
    }
    let Some(repo_id) = model.workspace.active_repo_id.clone() else {
        eprintln!("Nana 保存封面没有活动仓库");
        return;
    };
    let Some(path) = model.inspect.target_path.clone() else {
        eprintln!("Nana 保存封面没有当前文件");
        return;
    };
    if !editable_for_button(model) {
        eprintln!("Nana 当前文件不能保存封面：{path}");
        return;
    }
    let url = url.trim().to_string();
    if !allowed(model, &url) {
        eprintln!("Nana 保存封面的地址不是这个文件或候选上的 http(s) 封面：{url}");
        return;
    }
    model.asmr.cover.pending = Some(PendingCover {
        repo_id: repo_id.clone(),
        path: path.clone(),
        source_url: url.clone(),
    });
    model.files.queue_cover_save(repo_id, path, url);
}

/// 保存结果回来了。不是这次提交的仓库和路径就丢掉，按钮留在「保存中」。
/// 换了仓库只结束这次提交，不把缩略图写进现在的列表。失败写进文件列的错误。
pub(super) fn finish(
    model: &mut ShellViewModel,
    repo_id: String,
    path: String,
    result: Result<EnsuredThumbnail, String>,
) {
    let matches = model
        .asmr
        .cover
        .pending
        .as_ref()
        .is_some_and(|item| item.repo_id == repo_id && item.path == path);
    if !matches {
        eprintln!("Nana 丢掉不属于这次保存封面的结果：{repo_id} {path}");
        return;
    }
    model.asmr.cover.pending = None;
    if model.workspace.active_repo_id.as_deref() != Some(repo_id.as_str()) {
        eprintln!("Nana 保存封面时已经换了仓库，不写回：{repo_id} {path}");
        return;
    }
    match result {
        Ok(ensured) => model.files.note_cover_saved(&path, ensured),
        Err(error) => model.files.note_error(error),
    }
}

/// 正在保存封面。候选上的「封面」在这期间停用。
pub(super) fn busy(model: &ShellViewModel) -> bool {
    model.asmr.cover.pending.is_some()
}

fn offer(model: &ShellViewModel) -> Option<CoverButton> {
    // 封面地址跟作品信息同一条叠层，不跟还没保存的草稿。
    let url = remote_cover(&super::super::inspect_metadata_view::current_card_metadata(
        model,
    ))?;
    let current = model.inspect.target_path.as_deref();
    let saving_this = model
        .asmr
        .cover
        .pending
        .as_ref()
        .is_some_and(|item| Some(item.path.as_str()) == current);
    Some(CoverButton {
        url,
        label: if saving_this {
            "保存中"
        } else {
            "保存封面"
        }
        .to_string(),
        disabled: busy(model) || !editable_for_button(model),
    })
}

/// Vue `canEdit`：文件，而且有素材 id。目录和还没读到素材的条目不能保存。
pub(super) fn editable_for_button(model: &ShellViewModel) -> bool {
    if model.inspect.current_asset_id().is_none() {
        return false;
    }
    match super::current_row(model) {
        Some(row) => row.kind == "file",
        None => model.inspect.target_path.is_some(),
    }
}

/// 地址必须是当前文件自己的封面，或它列出的某条候选的封面。别的网址不下载。
fn allowed(model: &ShellViewModel, url: &str) -> bool {
    if !is_http(url) {
        return false;
    }
    let card = super::super::inspect_metadata_view::current_card_metadata(model);
    if remote_cover(&card).as_deref() == Some(url) {
        return true;
    }
    // 候选上的封面跟候选卡片同一份叠层，不跟还没保存的草稿。
    super::candidates::cover_urls(&card)
        .iter()
        .any(|item| item == url)
}

fn http_text(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    is_http(text).then(|| text.to_string())
}

fn is_http(text: &str) -> bool {
    let bytes = text.as_bytes();
    (bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"https://"))
        || (bytes.len() >= 7 && bytes[..7].eq_ignore_ascii_case(b"http://"))
}
