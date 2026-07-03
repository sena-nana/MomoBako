//! Local preview HTTP server for prepared repository sources.

use super::PREVIEW_HOST;
use crate::services::repository::RepositoryState;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::Arc,
    thread,
};
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};

const PREVIEW_PATH_PREFIX: &str = "/preview/";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ByteRange {
    pub(crate) start: u64,
    pub(crate) end: u64,
}

/// Starts the local preview server that serves registered preview-source tokens.
pub(crate) fn start_preview_server(
    repository_state: Arc<RepositoryState>,
) -> Result<String, String> {
    let server = Server::http(format!("{PREVIEW_HOST}:0")).map_err(|error| error.to_string())?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| "preview server did not bind to a TCP address".to_string())?;
    let preview_addr = format!("{PREVIEW_HOST}:{}", addr.port());

    thread::spawn(move || {
        for request in server.incoming_requests() {
            let repository_state = repository_state.clone();
            thread::spawn(move || {
                handle_preview_request(request, &repository_state);
            });
        }
    });

    Ok(preview_addr)
}

fn handle_preview_request(request: Request, repository_state: &Arc<RepositoryState>) {
    match preview_token_from_url(request.url()) {
        Some(token) if request.method() == &Method::Get || request.method() == &Method::Head => {
            let range_header = request
                .headers()
                .iter()
                .find(|item| item.field.equiv("Range"))
                .map(|item| item.value.as_str().to_string());
            let response =
                repository_state
                    .open_preview_file_source(token)
                    .and_then(|(file, media_type)| {
                        build_preview_file_response(file, &media_type, range_header.as_deref())
                    });
            match response {
                Ok(response) => {
                    let _ = request.respond(response);
                }
                Err(error) => {
                    let _ = request
                        .respond(Response::from_string(error).with_status_code(StatusCode(404)));
                }
            }
        }
        Some(_) => {
            let _ = request.respond(
                Response::from_string("method not allowed").with_status_code(StatusCode(405)),
            );
        }
        None => {
            let _ = request
                .respond(Response::from_string("not found").with_status_code(StatusCode(404)));
        }
    }
}

fn build_preview_file_response(
    mut file: File,
    media_type: &str,
    range_header: Option<&str>,
) -> Result<ResponseBox, String> {
    let file_size = file.metadata().map_err(|error| error.to_string())?.len();
    let mut headers = base_preview_headers(media_type);
    headers.push(header("Accept-Ranges", "bytes"));

    if let Some(range_value) = range_header {
        let Some(range) = parse_byte_range(range_value, file_size) else {
            return Ok(Response::from_string("range not satisfiable")
                .with_status_code(StatusCode(416))
                .with_header(header("Content-Range", &format!("bytes */{file_size}")))
                .with_header(header("Access-Control-Allow-Origin", "*"))
                .boxed());
        };
        file.seek(SeekFrom::Start(range.start))
            .map_err(|error| error.to_string())?;
        let length = range.end - range.start + 1;
        headers.push(header(
            "Content-Range",
            &format!("bytes {}-{}/{}", range.start, range.end, file_size),
        ));
        return Ok(Response::new(
            StatusCode(206),
            headers,
            file.take(length),
            Some(length as usize),
            None,
        )
        .boxed());
    }

    Ok(Response::from_file(file)
        .with_header(header("Content-Type", media_type))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("Access-Control-Allow-Origin", "*"))
        .with_header(header("Accept-Ranges", "bytes"))
        .boxed())
}

fn base_preview_headers(media_type: &str) -> Vec<Header> {
    vec![
        header("Content-Type", media_type),
        header("Cache-Control", "no-store"),
        header("Access-Control-Allow-Origin", "*"),
    ]
}

pub(crate) fn parse_byte_range(value: &str, file_size: u64) -> Option<ByteRange> {
    if file_size == 0 {
        return None;
    }
    let range_value = value.trim().strip_prefix("bytes=")?;
    if range_value.contains(',') {
        return None;
    }
    let (start_value, end_value) = range_value.split_once('-')?;
    if start_value.is_empty() {
        let suffix_length = end_value.parse::<u64>().ok()?;
        if suffix_length == 0 {
            return None;
        }
        let start = file_size.saturating_sub(suffix_length);
        return Some(ByteRange {
            start,
            end: file_size - 1,
        });
    }

    let start = start_value.parse::<u64>().ok()?;
    if start >= file_size {
        return None;
    }
    let end = if end_value.is_empty() {
        file_size - 1
    } else {
        end_value.parse::<u64>().ok()?.min(file_size - 1)
    };
    if end < start {
        return None;
    }
    Some(ByteRange { start, end })
}

pub(crate) fn preview_token_from_url(url: &str) -> Option<&str> {
    let token = url
        .strip_prefix(PREVIEW_PATH_PREFIX)?
        .split(['?', '#'])
        .next()?;
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(token)
    } else {
        None
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("preview header should be valid")
}
