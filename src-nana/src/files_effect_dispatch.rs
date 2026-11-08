//! 文件浏览相关的缩略图、浏览和条目操作副作用。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::{
    FileBrowserRequest, FileCreateRequest, FileRenameRequest, ThumbnailRequest,
};
use crate::shell::{AsmrMessage, EnsuredThumbnail, FilesMessage, ShellMessage};
use crate::task_guard::{settle, spawn, spawn_service};
use crate::MomoBakoApplication;

use super::{files_message, snapshot_message};

/// 写入、清除自定义缩略图，或者请后端按源文件刷新缩略图。清除后文件没有路径时再刷新，恢复默认图。
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_thumbnail(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    kind: String,
    action: String,
    source_path: Option<String>,
    image_bytes: Option<Vec<u8>>,
) {
    let what = match action.as_str() {
        "refresh" => "刷新缩略图",
        "clear" => "取消自定义缩略图",
        _ => "自定义缩略图",
    };
    let failed = |error| files_message(FilesMessage::NoteError(error));
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
            let request = thumbnail_request(
                &repo_id,
                &path,
                &action,
                source_path.clone(),
                image_bytes.clone(),
                None,
            );
            let saved = match executor.block_on(browser.ensure_thumbnail(request)) {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("Nana {what}失败：{path}：{error}");
                    return files_message(FilesMessage::NoteError(error));
                }
            };
            let saved = if action == "clear" && saved.thumbnail_path.is_none() && kind == "file" {
                let refresh = thumbnail_request(&repo_id, &path, "refresh", None, None, None);
                match executor.block_on(browser.ensure_thumbnail(refresh)) {
                    Ok(response) => response,
                    Err(error) => {
                        eprintln!("Nana 恢复默认缩略图失败：{error}");
                        return files_message(FilesMessage::NoteError(error));
                    }
                }
            } else {
                saved
            };
            files_message(FilesMessage::ThumbnailSaved {
                path,
                thumbnail_path: saved.thumbnail_path,
                custom: saved.thumbnail_custom,
            })
        }
    });
}

/// 浏览时给一个还没有缩略图的条目要一张。`source_url` 有值时用 `save` 下载来源封面，否则用 `ensure` 从本地文件生成。
/// 成功和失败都交回 `ThumbnailEnsured`：队列等它回来才排下一条，服务没起来、任务排不进去或者 panic 也照样交回失败。
pub(super) fn dispatch_ensure(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    source_url: Option<String>,
) {
    let (fail_repo, fail_path) = (repo_id.clone(), path.clone());
    let failed = move |error| ensured_message(fail_repo.clone(), fail_path.clone(), Err(error));
    let what = if source_url.is_some() {
        "预取来源封面"
    } else {
        "生成缩略图"
    };
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
            let action = if source_url.is_some() {
                "save"
            } else {
                "ensure"
            };
            let request =
                thumbnail_request(&repo_id, &path, action, None, None, source_url.clone());
            let result = executor
                .block_on(browser.ensure_thumbnail(request))
                .map(|response| EnsuredThumbnail {
                    thumbnail_path: response.thumbnail_path,
                    custom: response.thumbnail_custom,
                    metadata: response.metadata,
                });
            if let Err(error) = &result {
                eprintln!("Nana {what}失败：{path}：{error}");
            }
            ensured_message(repo_id, path, result)
        }
    });
}

fn ensured_message(
    repo_id: String,
    path: String,
    result: Result<EnsuredThumbnail, String>,
) -> ShellMessage {
    files_message(FilesMessage::ThumbnailEnsured {
        repo_id,
        path,
        result,
    })
}

/// 用户点「保存封面」或候选上的「封面」：按这个 http(s) 地址 `save`，不占用浏览队列的在途名额。
/// 失败也交回，按钮才能从「保存中」恢复。
pub(super) fn dispatch_save_cover(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    source_url: String,
) {
    let (fail_repo, fail_path) = (repo_id.clone(), path.clone());
    let failed = move |error| cover_saved(fail_repo.clone(), fail_path.clone(), Err(error));
    spawn_service(
        app.services.as_ref(),
        context,
        "保存封面",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || {
                let request =
                    thumbnail_request(&repo_id, &path, "save", None, None, Some(source_url));
                let result = executor
                    .block_on(browser.ensure_thumbnail(request))
                    .map(|response| EnsuredThumbnail {
                        thumbnail_path: response.thumbnail_path,
                        custom: response.thumbnail_custom,
                        metadata: response.metadata,
                    });
                if let Err(error) = &result {
                    eprintln!("Nana 保存封面失败：{path}：{error}");
                }
                cover_saved(repo_id, path, result)
            }
        },
    );
}

fn cover_saved(
    repo_id: String,
    path: String,
    result: Result<EnsuredThumbnail, String>,
) -> ShellMessage {
    ShellMessage::Asmr(AsmrMessage::CoverSaved {
        repo_id,
        path,
        result,
    })
}

fn thumbnail_request(
    repo_id: &str,
    path: &str,
    action: &str,
    source_path: Option<String>,
    image_bytes: Option<Vec<u8>>,
    source_url: Option<String>,
) -> ThumbnailRequest {
    ThumbnailRequest {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        action: Some(action.to_string()),
        source_path,
        source_url,
        image_bytes,
        media_type: None,
    }
}

/// 解码缩略图。每个文件单独接住 panic（图片解码器出错只丢这一张），解不出的记日志；整个任务排不进去时交回空的一批，
/// 这些行留着类型图标。
pub(super) fn dispatch_thumbnails(
    context: &RuntimeProgramContext<ShellMessage>,
    paths: Vec<String>,
) {
    spawn(
        context,
        "缩略图解码",
        |_| ShellMessage::ThumbnailPixels(Vec::new()),
        move || {
            let mut frames = Vec::new();
            for path in paths {
                match settle(
                    "缩略图解码",
                    || crate::shell::decode_thumbnail_file(&path),
                    Err,
                ) {
                    Ok(frame) => frames.push(frame),
                    Err(error) => eprintln!("Nana 缩略图解码失败：{path}：{error}"),
                }
            }
            ShellMessage::ThumbnailPixels(frames)
        },
    );
}

pub(super) fn dispatch_browse(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    trash: bool,
    offset: usize,
    limit: usize,
) {
    let request = FileBrowserRequest {
        repo_id,
        directory_path: if trash || path.is_empty() {
            None
        } else {
            Some(path)
        },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(offset),
        limit: Some(limit),
    };
    let failed = |error| ShellMessage::FileBrowserLoaded(Err(error));
    spawn_service(
        app.services.as_ref(),
        context,
        "文件浏览",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || {
                ShellMessage::FileBrowserLoaded(
                    executor.block_on(browser.get_file_browser(request)),
                )
            }
        },
    );
}

/// 下一页只为生成缩略图。结果走 `VirtualThumbnailPage`，不进会改列表的 `FileBrowserLoaded`。
pub(super) fn dispatch_virtual_page(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    trash: bool,
    offset: usize,
    limit: usize,
) {
    let request = FileBrowserRequest {
        repo_id: repo_id.clone(),
        directory_path: if trash || path.is_empty() {
            None
        } else {
            Some(path.clone())
        },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(offset),
        limit: Some(limit),
    };
    let failed_repo = repo_id.clone();
    let failed_path = path.clone();
    let failed = move |error| ShellMessage::VirtualThumbnailPage {
        repo_id: failed_repo.clone(),
        path: failed_path.clone(),
        trash,
        offset,
        result: Err(error),
    };
    spawn_service(
        app.services.as_ref(),
        context,
        "下一页缩略图",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || ShellMessage::VirtualThumbnailPage {
                repo_id,
                path,
                trash,
                offset,
                result: executor.block_on(browser.get_file_browser(request)),
            }
        },
    );
}

pub(super) fn dispatch_create(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    name: String,
    directory: bool,
) {
    let created_name = if directory { None } else { Some(name.clone()) };
    let request = FileCreateRequest {
        repo_id,
        parent_path: parent,
        name,
    };
    let failed_name = created_name.clone();
    let failed = move |error| snapshot_message(Err(error), failed_name.clone());
    spawn_service(
        app.services.as_ref(),
        context,
        "新建条目",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || {
                let result = if directory {
                    executor.block_on(browser.create_directory(request))
                } else {
                    executor.block_on(browser.create_file(request))
                };
                snapshot_message(result, created_name)
            }
        },
    );
}

pub(super) fn dispatch_rename(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    new_name: String,
) {
    let request = FileRenameRequest {
        repo_id,
        path,
        new_name,
    };
    let failed = |error| snapshot_message(Err(error), None);
    spawn_service(
        app.services.as_ref(),
        context,
        "重命名",
        failed,
        |services| {
            let browser = services.file_browser.clone();
            let executor = services.executor.clone();
            move || snapshot_message(executor.block_on(browser.rename_entry(request)), None)
        },
    );
}
