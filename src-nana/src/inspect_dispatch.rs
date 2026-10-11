//! 把预览、元数据和搜索副作用交给已有的仓库查询与交互 ViewModel。
//!
//! 图片解码失败写回错误态。文本超过上限不截断冒充成功。搜索结果按代次丢弃。任务都经 `task_guard` 派发：服务没
//! 起来、任务排不进去或解码时 panic 都按失败写回，预览不会停在读取中。

use std::collections::BTreeMap;

use nana_ui::RuntimeProgramContext;
use serde_json::Value;

use crate::backend::services::repository::{
    FileReadRequest, MetadataUpdateRequest, SearchDateFilter, SearchMetadataFilter,
    SearchNumberFilter, SearchRequest, SearchSort,
};
use crate::shell::{
    DateBound, InspectEffect, InspectMessage, NumberBound, SearchRequestDraft, SearchRow, ShellMessage,
};
use crate::task_guard::spawn_service;
use crate::{decode_preview_pixels, prepare_preview_text, MomoBakoApplication};

/// 执行预览和搜索归约留下的请求。
pub fn dispatch_inspect_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.inspect.take_effects() {
        match effect {
            InspectEffect::LoadImage { repo_id, path } => dispatch_image(app, context, repo_id, path),
            InspectEffect::LoadText { repo_id, path, markdown, generation } => {
                dispatch_text(app, context, repo_id, path, markdown, generation);
            }
            InspectEffect::LoadNative { repo_id, path, view_id, generation } => {
                dispatch_native(app, context, repo_id, path, view_id, generation);
            }
            InspectEffect::Autoplay { path, generation } => {
                app.shell.reduce(ShellMessage::Inspect(InspectMessage::Autoplay { path, generation }));
            }
            InspectEffect::LoadMedia { repo_id, path, generation } => {
                dispatch_media(app, context, repo_id, path, generation);
            }
            InspectEffect::SaveMetadata { repo_id, asset_id, expected_version, metadata } => {
                dispatch_save(app, context, repo_id, asset_id, expected_version, metadata);
            }
            InspectEffect::LoadAsset { repo_id, asset_id } => dispatch_asset(app, context, repo_id, asset_id),
            InspectEffect::Search { generation, request } => dispatch_search(app, context, generation, request),
        }
    }
}

fn dispatch_image(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, path: String) {
    let (fail_repo, fail_path) = (repo_id.clone(), path.clone());
    let failed = move |error| ShellMessage::PreviewPixelsLoaded { source: empty_source(&fail_repo, &fail_path), pixels: Err(error) };
    spawn_service(app.services.as_ref(), context, "图片预览", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let prepared = executor.block_on(query.prepare_preview_file_source(FileReadRequest {
                repo_id: repo_id.clone(),
                path: path.clone(),
            }));
            let (source, pixels) = match prepared {
                Ok(source) => {
                    let pixels = executor
                        .block_on(query.read_file(FileReadRequest { repo_id, path }))
                        .and_then(|bytes| {
                            if !source.media_type.starts_with("image/") {
                                return Err(format!("原生纹理预览暂不支持 {}", source.media_type));
                            }
                            decode_preview_pixels(&bytes)
                        });
                    (source, pixels)
                }
                Err(error) => (empty_source(&repo_id, &path), Err(error)),
            };
            ShellMessage::PreviewPixelsLoaded { source, pixels }
        }
    });
}

fn dispatch_media(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    generation: u64,
) {
    let fail_path = path.clone();
    let failed = move |error| media_message(fail_path.clone(), generation, Err(error));
    let extension = std::path::Path::new(&path).extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_string();
    spawn_service(app.services.as_ref(), context, "音视频预览", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            // 和播放条装当前项走同一个解码：坏的 wav、mp3、flac、ogg 报它自己的原因，预览页写出来的和播放条一致。
            let result = executor
                .block_on(query.read_file(FileReadRequest { repo_id: repo_id.clone(), path: path.clone() }))
                .and_then(|bytes| crate::shell::player::decode_media(&repo_id, &extension, &bytes));
            media_message(path, generation, result)
        }
    });
}

fn dispatch_text(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    markdown: bool,
    generation: u64,
) {
    let fail_path = path.clone();
    let failed = move |error| body_error(fail_path.clone(), markdown, generation, error);
    spawn_service(app.services.as_ref(), context, "文本预览", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor
                .block_on(query.read_file(FileReadRequest { repo_id, path: path.clone() }))
                .map(|bytes| prepare_preview_text(&bytes));
            if let Err(error) = &result {
                eprintln!("Nana 读取文本预览失败：{path}：{error}");
            }
            ShellMessage::Inspect(InspectMessage::BodyLoaded { path, markdown, generation, result })
        }
    });
}

fn dispatch_native(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    view_id: String,
    generation: u64,
) {
    let fail_path = path.clone();
    let failed = move |error| native_message(fail_path.clone(), generation, Err(error));
    spawn_service(app.services.as_ref(), context, "原生预览", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor
                .block_on(query.read_file(FileReadRequest { repo_id, path: path.clone() }))
                .and_then(|bytes| crate::shell::load_native_preview(&view_id, &bytes));
            native_message(path, generation, result)
        }
    });
}

fn dispatch_save(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    asset_id: String,
    expected_version: i64,
    metadata: BTreeMap<String, Value>,
) {
    let failed = |error| saved_message(Err(error));
    spawn_service(app.services.as_ref(), context, "保存元数据", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let request = MetadataUpdateRequest { repo_id, asset_id, expected_version, metadata, source: Some("desktop".into()) };
            saved_message(executor.block_on(query.update_asset_metadata(request)).map(|response| (response.outcome, response.asset)))
        }
    });
}

fn dispatch_asset(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, asset_id: String) {
    let failed = |error| ShellMessage::AssetDetailLoaded(Err(error));
    spawn_service(app.services.as_ref(), context, "打开搜索结果", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || ShellMessage::AssetDetailLoaded(executor.block_on(query.get_asset_detail(repo_id, asset_id)))
    });
}

fn dispatch_search(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    generation: u64,
    request: SearchRequestDraft,
) {
    let service_request = to_service_request(request);
    let failed = move |error| search_message(generation, Err(error));
    spawn_service(app.services.as_ref(), context, "搜索", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor
                .block_on(query.search_assets(service_request))
                .map(|response| response.results.into_iter().map(SearchRow::from_hit).collect());
            search_message(generation, result)
        }
    });
}

fn to_service_request(request: SearchRequestDraft) -> SearchRequest {
    SearchRequest {
        query: request.query,
        repo_id: request.repo_id,
        exclude_query: request.exclude_query,
        metadata_key: None,
        metadata_value: None,
        tag: None,
        tags: some_list(request.tags),
        metadata_filters: some_metadata(request.metadata_filters),
        exclude_tags: some_list(request.exclude_tags),
        exclude_formats: some_list(request.exclude_formats),
        exclude_metadata_filters: some_metadata(request.exclude_metadata_filters),
        exclude_path_prefixes: some_list(request.exclude_path_prefixes),
        exclude_number_filters: some_numbers(request.exclude_number_filters),
        exclude_date_filters: some_dates(request.exclude_date_filters),
        number_filters: some_numbers(request.number_filters),
        date_filters: some_dates(request.date_filters),
        formats: some_list(request.formats),
        min_rating: request.min_rating,
        // 筛选栏和 Vue 一样没有匹配方式开关，多个条件总是全部满足，不传 `matchMode`。
        match_mode: None,
        sort: request.sort_field.map(|field| SearchSort {
            field,
            direction: request.sort_direction.unwrap_or_else(|| "asc".into()),
        }),
        limit: request.limit,
    }
}

fn some_list(values: Vec<String>) -> Option<Vec<String>> {
    (!values.is_empty()).then_some(values)
}

fn some_metadata(values: Vec<(String, String)>) -> Option<Vec<SearchMetadataFilter>> {
    (!values.is_empty()).then(|| values.into_iter().map(|(key, value)| SearchMetadataFilter { key, value }).collect())
}

fn some_numbers(values: Vec<NumberBound>) -> Option<Vec<SearchNumberFilter>> {
    (!values.is_empty()).then(|| {
        values
            .into_iter()
            .map(|bound| SearchNumberFilter {
                key: bound.key,
                min: bound.min.and_then(|value| value.parse().ok()),
                max: bound.max.and_then(|value| value.parse().ok()),
            })
            .collect()
    })
}

fn some_dates(values: Vec<DateBound>) -> Option<Vec<SearchDateFilter>> {
    (!values.is_empty()).then(|| values.into_iter().map(|bound| SearchDateFilter { key: bound.key, from: bound.from, to: bound.to }).collect())
}

fn media_message(path: String, generation: u64, result: Result<crate::shell::MediaParts, String>) -> ShellMessage {
    let (result, pcm, frames) = match result {
        Ok(parts) => (Ok(parts.session), parts.pcm, parts.frames),
        Err(error) => (Err(error), None, None),
    };
    ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result, pcm, frames })
}

fn native_message(path: String, generation: u64, result: Result<crate::shell::NativeLoad, String>) -> ShellMessage {
    ShellMessage::Inspect(InspectMessage::NativeLoaded { path, generation, result })
}

fn body_error(path: String, markdown: bool, generation: u64, error: String) -> ShellMessage {
    ShellMessage::Inspect(InspectMessage::BodyLoaded { path, markdown, generation, result: Err(error) })
}

fn saved_message(result: Result<(String, crate::backend::services::repository::AssetDetail), String>) -> ShellMessage {
    ShellMessage::Inspect(InspectMessage::MetadataSaved(result))
}

fn search_message(generation: u64, result: Result<Vec<SearchRow>, String>) -> ShellMessage {
    ShellMessage::Inspect(InspectMessage::SearchFinished { generation, result })
}

fn empty_source(repo_id: &str, path: &str) -> crate::backend::services::repository::FilePreviewSourceResponse {
    crate::backend::services::repository::FilePreviewSourceResponse {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        token: String::new(),
        source_url: None,
        local_path: None,
        media_type: "application/octet-stream".into(),
        size_bytes: 0,
        modified_at: None,
    }
}
