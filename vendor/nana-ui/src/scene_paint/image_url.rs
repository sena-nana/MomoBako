//! Resolve `background-image: url(...)` for GPU texture upload.

use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(test)]
use nana_ui_core::path_to_file_url;
use nana_ui_core::{
    MAX_LOCAL_URL_BYTES, file_url_to_path, href_is_protocol_relative_or_unc, is_packaged_url,
    packaged_logical_path, packaged_url, path_looks_network, percent_decode_bytes,
    read_bytes_within_jail, read_packaged, resolve_filesystem_href,
};
use nana_ui_platform::{FetchCancellation, FetchRequest, SharedFetchHost};

static BACKGROUND_IMAGE_URL_BASE: OnceLock<PathBuf> = OnceLock::new();

// Per-test-thread override. Cargo libtest runs each test on one thread, so
// parallel tests cannot see each other's base. Hosts still first-set
// BACKGROUND_IMAGE_URL_BASE.
#[cfg(test)]
thread_local! {
    static TEST_URL_BASE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Document or workspace base for relative `url(...)` paths.
///
/// First call wins for hosts. When unset, relative URLs resolve against the
/// application's runtime resources location in an installed or portable
/// package (so the working directory the user launched from never matters),
/// and against the current working directory in development and embedded
/// hosts without application paths.
pub fn set_background_image_url_base(base: PathBuf) {
    #[cfg(test)]
    {
        TEST_URL_BASE.with(|slot| *slot.borrow_mut() = Some(base.clone()));
    }
    let _ = BACKGROUND_IMAGE_URL_BASE.set(base);
}

/// Blocking GET through the document's policy-gated transport.
///
/// `None` without a host, when the policy refuses the origin or a
/// redirect hop, on any transport error, on a non-2xx status, or on a body
/// above `max_bytes`. The effective cap is the smaller of `max_bytes` and the
/// policy's `max_response_bytes`, which is what bounds the peak allocation.
///
/// `cancellation` goes through [`FetchHost::fetch_cancellable`], so cancelling
/// it shuts the socket down mid-read instead of leaving the worker parked until
/// the policy timeout. It ends the *network* wait only: bytes already received
/// are decoded to completion, since neither `image` nor `resvg` has a
/// cancellation point.
fn fetch_resource_bytes(
    url: &str,
    host: Option<&SharedFetchHost>,
    max_bytes: usize,
    cancellation: &FetchCancellation,
) -> Option<Vec<u8>> {
    let response = host?
        .fetch_cancellable(FetchRequest::get(url), cancellation.clone())
        .ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    (response.body.len() <= max_bytes).then_some(response.body)
}

fn fallback_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// The base the host set with [`set_background_image_url_base`], if any.
fn host_base() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_URL_BASE.with(|slot| slot.borrow().clone())
    }
    #[cfg(not(test))]
    {
        BACKGROUND_IMAGE_URL_BASE.get().cloned()
    }
}

/// Base for relative URLs: the host's base, else the package's runtime
/// resources (installed / portable), else the working directory.
fn url_base() -> PathBuf {
    host_base()
        .or_else(crate::application_builder::packaged_resources_base)
        .unwrap_or_else(fallback_cwd)
}

/// The jail absolute and `file:` URLs were always checked against: the
/// host's base, else the working directory. Resolving relative URLs against
/// the package does not narrow what absolute ones may reach.
fn legacy_jail() -> PathBuf {
    host_base().unwrap_or_else(fallback_cwd)
}

/// `path` exists and canonicalizes inside `base`. A textual prefix check
/// would let `base/../x` through when either side cannot be resolved.
fn under(path: &std::path::Path, base: &std::path::Path) -> bool {
    match (path.canonicalize(), base.canonicalize()) {
        (Ok(path), Ok(base)) => path.starts_with(base),
        _ => false,
    }
}

/// Whether `url` names an absolute location (`file:` or an absolute path).
/// Only those may use the legacy jail; relative URLs resolve and are
/// checked against [`url_base`] alone, so an installed package never falls
/// back to the working directory.
fn is_absolute_local_url(url: &str) -> bool {
    let url = url.trim();
    if url.to_ascii_lowercase().starts_with("file:") {
        // `file:a.png` (no drive on Windows) is relative in disguise.
        return file_url_to_path(url).is_some_and(|path| path.is_absolute());
    }
    std::path::Path::new(url).is_absolute()
}

/// Whether a resolved fetch key may be loaded for `@font-face` (and similar).
///
/// `url` is the reference as written, `resolved` what
/// [`resolve_background_image_url`] made of it. `data:` and packaged
/// (`nana://res/`) pass. `http(s):` does not: `@font-face` has no remote
/// transport, matching the Vue stylesheet path. Filesystem paths must stay
/// under the URL base (relative references) or the host base / working
/// directory (absolute ones), so `file:` / `..` cannot escape.
pub fn resolved_resource_is_allowed(url: &str, resolved: &str) -> bool {
    let resolved = resolved.trim();
    if resolved.starts_with("data:") || is_packaged_url(resolved) {
        return true;
    }
    if resolved.starts_with("http://") || resolved.starts_with("https://") {
        return false;
    }
    if resolved.is_empty() {
        return false;
    }
    let path = std::path::Path::new(resolved);
    under(path, &url_base()) || (is_absolute_local_url(url) && under(path, &legacy_jail()))
}

/// Resolve a parsed CSS URL to a fetch/load key (absolute URL or filesystem path).
///
/// Resolution is not authorization: `http(s)` keys come back intact and are
/// judged later by the document's `FetchPolicy` (see [`fetch_resource_bytes`]);
/// `data:` stays loadable. Non-local `file:` hosts, protocol-relative `//`, and
/// UNC are refused here (same helper as stylesheet / `@font-face` jail).
pub fn resolve_background_image_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || href_is_protocol_relative_or_unc(trimmed) {
        return None;
    }
    if is_packaged_url(trimmed) {
        return packaged_logical_path(trimmed, None).map(|path| packaged_url(&path));
    }
    if trimmed.starts_with("data:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return Some(trimmed.to_string());
    }
    if trimmed.to_ascii_lowercase().starts_with("file:") {
        let path = file_url_to_path(trimmed)?;
        if path_looks_network(&path) {
            return None;
        }
        return Some(path.to_string_lossy().into_owned());
    }
    join_relative(trimmed)
}

fn join_relative(rel: &str) -> Option<String> {
    let path = resolve_filesystem_href(rel, None, &url_base())?;
    Some(path.to_string_lossy().into_owned())
}

/// Decode a resolved CSS URL into straight-alpha RGBA8. Cached by the quad
/// painter; not invoked every frame after the first hit.
///
/// SVG bytes (inline `data:image/svg+xml`, `url(.svg)`, or sniffed markup) share
/// this path with raster images so the quad URL cache keys by URL/id.
///
/// Local only: with no fetch host, a remote URL is refused without a socket.
pub(super) fn decode_url_rgba(url: &str) -> Option<(u32, u32, Vec<u8>)> {
    // The synchronous path has no one to cancel it: the caller is blocked on
    // this very call. Workers use `decode_url_rgba_with` and keep the token.
    decode_url_rgba_with(url, None, &FetchCancellation::new())
}

/// [`decode_url_rgba`] for the `"nana-image"` worker: remote loads go through
/// `host`, the egress of the document that requested them, and stop when
/// `cancellation` fires.
pub(super) fn decode_url_rgba_with(
    url: &str,
    host: Option<&SharedFetchHost>,
    cancellation: &FetchCancellation,
) -> Option<(u32, u32, Vec<u8>)> {
    let resolved = resolve_background_image_url(url)?;
    if resolved.starts_with("data:") {
        return decode_data_url_rgba(&resolved);
    }
    if resolved.starts_with("http://") || resolved.starts_with("https://") {
        return decode_http_rgba(&resolved, host, cancellation);
    }
    if is_packaged_url(&resolved) {
        let bytes = read_packaged(&resolved, None, MAX_LOCAL_URL_BYTES)?;
        return decode_image_bytes_with_hint(&bytes, looks_like_svg_url(&resolved));
    }
    // Relative URLs resolve against `url_base` only (never the working
    // directory of an installed package); absolute ones keep the old jail.
    let absolute = is_absolute_local_url(url);
    let bytes = read_bytes_within_jail(url, &url_base(), MAX_LOCAL_URL_BYTES).or_else(|| {
        absolute
            .then(|| read_bytes_within_jail(url, &legacy_jail(), MAX_LOCAL_URL_BYTES))
            .flatten()
    })?;
    decode_image_bytes_with_hint(&bytes, looks_like_svg_url(&resolved))
}

fn decode_data_url_rgba(url: &str) -> Option<(u32, u32, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let meta_l = meta.to_ascii_lowercase();
    let is_base64 = meta_l.split(';').any(|part| part.trim() == "base64");
    let bytes = if is_base64 {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .ok()?
    } else {
        percent_decode_bytes(payload)?
    };
    decode_image_bytes_with_hint(&bytes, meta_l.contains("svg"))
}

fn decode_http_rgba(
    url: &str,
    host: Option<&SharedFetchHost>,
    cancellation: &FetchCancellation,
) -> Option<(u32, u32, Vec<u8>)> {
    let (bytes, svg) = fetch_remote_bytes(url, host, cancellation)?;
    decode_image_bytes_with_hint(&bytes, svg)
}

/// The body of a remote image URL and whether to decode it as SVG, for a
/// caller that keeps the encoded bytes. `None` for a URL that is not remote
/// and for every refusal [`decode_url_rgba_with`] would make.
pub(super) fn fetch_remote_bytes(
    url: &str,
    host: Option<&SharedFetchHost>,
    cancellation: &FetchCancellation,
) -> Option<(Vec<u8>, bool)> {
    let resolved = resolve_background_image_url(url)?;
    if !(resolved.starts_with("http://") || resolved.starts_with("https://")) {
        return None;
    }
    let bytes = fetch_resource_bytes(&resolved, host, MAX_LOCAL_URL_BYTES as usize, cancellation)?;
    Some((bytes, looks_like_svg_url(&resolved)))
}

/// Decode already-fetched image bytes into straight-alpha RGBA8.
pub(super) fn decode_bytes_rgba(bytes: &[u8], svg: bool) -> Option<(u32, u32, Vec<u8>)> {
    decode_image_bytes_with_hint(bytes, svg)
}

#[cfg(test)]
fn decode_image_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    decode_image_bytes_with_hint(bytes, false)
}

fn decode_image_bytes_with_hint(bytes: &[u8], prefer_svg: bool) -> Option<(u32, u32, Vec<u8>)> {
    if prefer_svg || looks_like_svg(bytes) {
        match decode_svg_rgba(bytes) {
            Some(decoded) => return Some(decoded),
            None if prefer_svg => return None,
            None => {}
        }
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((width, height, rgba.into_raw()))
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text.trim_start();
    text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg"))
}

fn looks_like_svg_url(url: &str) -> bool {
    let path = url
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(url)
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(url);
    PathExt(path).has_svg_extension()
}

struct PathExt<'a>(&'a str);

impl PathExt<'_> {
    fn has_svg_extension(self) -> bool {
        let bytes = self.0.as_bytes();
        let Some(dot) = bytes.iter().rposition(|b| *b == b'.') else {
            return false;
        };
        bytes[dot + 1..].eq_ignore_ascii_case(b"svg")
    }
}

const MAX_SVG_EDGE: u32 = 2048;

fn decode_svg_rgba(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(any(feature = "rich-text", feature = "bundled-fonts"))]
    let font = Some(nana_svg_raster::SvgFont {
        bytes: nana_ui_core::fonts::UI_FONT_REGULAR,
        family: nana_ui_core::fonts::UI_FONT_FAMILY,
    });
    #[cfg(not(any(feature = "rich-text", feature = "bundled-fonts")))]
    let font = None;
    let raster = nana_svg_raster::rasterize_document_capped_with_font(bytes, MAX_SVG_EDGE, font)?;
    Some((raster.width, raster.height, raster.rgba.to_vec()))
}

#[cfg(test)]
pub(super) fn reset_test_url_base() {
    TEST_URL_BASE.with(|slot| *slot.borrow_mut() = None);
}

/// Test double authorizing any `127.0.0.1` origin.
///
/// Loopback tests bind port 0, so no exact-match [`FetchPolicy`] can be written
/// before the port is known. This authorizes per request instead, and it keeps
/// the tests going through the real `FetchHost` seam.
#[cfg(test)]
#[derive(Debug)]
pub(super) struct LoopbackFetchHost {
    policy: nana_ui_platform::FetchPolicy,
}

#[cfg(test)]
fn loopback_origin(url: &str) -> Option<String> {
    let authority = url.strip_prefix("http://")?.split('/').next()?;
    (authority.split(':').next()? == "127.0.0.1").then(|| format!("http://{authority}"))
}

#[cfg(test)]
impl nana_ui_platform::FetchHost for LoopbackFetchHost {
    fn fetch(
        &self,
        request: FetchRequest,
    ) -> Result<nana_ui_platform::FetchResponse, nana_ui_platform::FetchError> {
        let origin = loopback_origin(&request.url).ok_or_else(|| {
            nana_ui_platform::FetchError::new(
                nana_ui_platform::FetchErrorKind::Policy,
                format!("test host serves loopback only: `{}`", request.url),
            )
        })?;
        let policy = nana_ui_platform::FetchPolicy::default().with_allowed_origin(&origin)?;
        nana_ui_platform::NativeFetchHost::new(policy).fetch(request)
    }

    /// Forwarded, not inherited: the default trait implementation only checks
    /// the token once at the boundary, so inheriting it would leave the real
    /// socket-shutdown path untested.
    fn fetch_cancellable(
        &self,
        request: FetchRequest,
        cancellation: nana_ui_platform::FetchCancellation,
    ) -> Result<nana_ui_platform::FetchResponse, nana_ui_platform::FetchError> {
        let origin = loopback_origin(&request.url).ok_or_else(|| {
            nana_ui_platform::FetchError::new(
                nana_ui_platform::FetchErrorKind::Policy,
                format!("test host serves loopback only: `{}`", request.url),
            )
        })?;
        let policy = nana_ui_platform::FetchPolicy::default().with_allowed_origin(&origin)?;
        nana_ui_platform::NativeFetchHost::new(policy).fetch_cancellable(request, cancellation)
    }

    fn policy(&self) -> &nana_ui_platform::FetchPolicy {
        &self.policy
    }
}

#[cfg(test)]
pub(super) fn loopback_fetch_host() -> SharedFetchHost {
    nana_ui_platform::shared_fetch_host(LoopbackFetchHost {
        policy: nana_ui_platform::FetchPolicy::default(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{Ipv4Addr, TcpListener};
    use std::path::Path;

    use super::*;

    #[test]
    fn only_absolute_references_may_use_the_legacy_jail() {
        #[cfg(not(windows))]
        {
            assert!(is_absolute_local_url("file:///tmp/a.ttf"));
            assert!(is_absolute_local_url("/tmp/a.ttf"));
        }
        #[cfg(windows)]
        {
            assert!(is_absolute_local_url("file:///C:/a.ttf"));
            assert!(is_absolute_local_url("C:\\a.ttf"));
            // No drive: relative in disguise.
            assert!(!is_absolute_local_url("file:///tmp/a.ttf"));
        }
        assert!(!is_absolute_local_url("../a.ttf"));
        assert!(!is_absolute_local_url("fonts/a.ttf"));
        assert!(!is_absolute_local_url("nana://res/a.ttf"));
    }

    #[test]
    fn relative_font_urls_cannot_climb_out_of_the_base() {
        let root = std::env::temp_dir().join(format!("nana-font-jail-{}", std::process::id()));
        let base = root.join("resources");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(root.join("outside.ttf"), b"x").unwrap();
        std::fs::write(base.join("inside.ttf"), b"x").unwrap();
        set_background_image_url_base(base.clone());
        let resolved = resolve_background_image_url("../outside.ttf").unwrap();
        assert!(!resolved_resource_is_allowed("../outside.ttf", &resolved));
        let resolved = resolve_background_image_url("inside.ttf").unwrap();
        assert!(resolved_resource_is_allowed("inside.ttf", &resolved));
        assert!(resolved_resource_is_allowed(
            "nana://res/a.ttf",
            "nana://res/a.ttf"
        ));
        reset_test_url_base();
        let _ = std::fs::remove_dir_all(&root);
    }

    fn png_1x1() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([7, 8, 9, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode png");
        bytes.into_inner()
    }

    /// Binds a port nothing ever answers on. A later `accept` reporting
    /// `WouldBlock` is the assertion that no connection was attempted.
    fn silent_listener() -> (TcpListener, String) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let url = format!("http://{}/a.png", listener.local_addr().expect("addr"));
        (listener, url)
    }

    fn assert_never_connected(listener: &TcpListener) {
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "policy must refuse before a socket is opened"
        );
    }

    /// Serves one canned response, then closes.
    fn one_response_server(status: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let url = format!("http://{}/a.png", listener.local_addr().expect("addr"));
        let status = status.to_owned();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut chunk = [0_u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&chunk[..read]),
                }
            }
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });
        (url, handle)
    }

    #[test]
    fn http_image_without_a_fetch_host_is_refused() {
        let (listener, url) = silent_listener();
        assert!(
            decode_url_rgba(&url).is_none(),
            "no fetch host must mean no remote image"
        );
        assert_never_connected(&listener);
    }

    #[test]
    fn a_cancelled_token_refuses_an_allowlisted_http_image_before_connecting() {
        let (listener, url) = silent_listener();
        let cancellation = FetchCancellation::new();
        cancellation.cancel();

        assert!(
            decode_url_rgba_with(&url, Some(&loopback_fetch_host()), &cancellation).is_none(),
            "a cancelled request must not load, even from an allowed origin"
        );
        // Pre-cancelled rather than raced: `register_socket` rejects an already
        // cancelled token, so this asserts the token reaches the socket layer
        // without depending on timing.
        assert_never_connected(&listener);
    }

    #[test]
    fn http_image_from_a_non_allowlisted_origin_is_refused_before_connecting() {
        let (listener, url) = silent_listener();
        let policy = nana_ui_platform::FetchPolicy::default()
            .with_allowed_origin("http://allowed.example")
            .expect("origin");
        let host =
            nana_ui_platform::shared_fetch_host(nana_ui_platform::NativeFetchHost::new(policy));
        assert!(
            decode_url_rgba_with(&url, Some(&host), &FetchCancellation::new()).is_none(),
            "an origin outside the allowlist must not load"
        );
        assert_never_connected(&listener);
    }

    #[test]
    fn http_image_from_an_allowlisted_origin_decodes() {
        let (url, server) = one_response_server("200 OK", png_1x1());
        let decoded = decode_url_rgba_with(
            &url,
            Some(&loopback_fetch_host()),
            &FetchCancellation::new(),
        );
        server.join().expect("server");
        let (width, height, rgba) = decoded.expect("allowlisted origin must decode");
        assert_eq!((width, height), (1, 1));
        assert_eq!(&rgba[..4], &[7, 8, 9, 255]);
    }

    #[test]
    fn http_image_with_a_non_2xx_status_is_refused() {
        let (url, server) = one_response_server("404 Not Found", b"<html>nope</html>".to_vec());
        let decoded = decode_url_rgba_with(
            &url,
            Some(&loopback_fetch_host()),
            &FetchCancellation::new(),
        );
        server.join().expect("server");
        assert!(
            decoded.is_none(),
            "an error page must not be fed to the image decoder"
        );
    }

    #[test]
    fn relative_url_joins_cwd_when_no_base() {
        reset_test_url_base();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let resolved = resolve_background_image_url("assets/icon.png").unwrap();
        assert_eq!(
            Path::new(&resolved),
            cwd.join("assets/icon.png").as_path(),
            "relative url must join cwd, not return the input unchanged"
        );
    }

    #[test]
    fn set_background_image_url_base_prefixes_relative() {
        let base =
            std::env::temp_dir().join(format!("nana-url-base-{}-{}", std::process::id(), "prefix"));
        set_background_image_url_base(base.clone());
        let resolved = resolve_background_image_url("assets/icon.png").unwrap();
        assert_eq!(
            Path::new(&resolved),
            base.join("assets/icon.png").as_path(),
            "set_background_image_url_base must prefix the relative path"
        );
        reset_test_url_base();
    }

    #[test]
    fn file_url_becomes_path() {
        let resolved = resolve_background_image_url("file:///tmp/test.png").unwrap();
        assert_file_url_path(&resolved);
    }

    #[test]
    fn file_localhost_url_becomes_path() {
        let resolved = resolve_background_image_url("file://localhost/tmp/test.png").unwrap();
        assert_file_url_path(&resolved);
    }

    #[test]
    fn file_remote_host_is_rejected() {
        assert!(resolve_background_image_url("file://evil.example/secret.png").is_none());
        assert!(decode_url_rgba("file://evil.example/secret.png").is_none());
        assert!(resolve_background_image_url("//cdn.example.com/x.png").is_none());
        assert!(resolve_background_image_url(r"\\cdn.example.com\share\x.png").is_none());
        assert!(resolve_background_image_url("%2f%2fcdn.example.com/x.png").is_none());
        assert!(decode_url_rgba("//cdn.example.com/x.png").is_none());
    }

    #[test]
    fn local_file_outside_jail_is_not_read() {
        let pid = std::process::id();
        let jail = std::env::temp_dir().join(format!("nanaui-img-jail-{pid}"));
        let outside = std::env::temp_dir().join(format!("nanaui-img-out-{pid}"));
        let _ = std::fs::remove_dir_all(&jail);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&jail).expect("jail");
        std::fs::create_dir_all(&outside).expect("outside");
        let secret = outside.join("secret.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255]))
            .save(&secret)
            .expect("secret png");
        set_background_image_url_base(jail.clone());
        assert!(
            decode_url_rgba(&secret.to_string_lossy()).is_none(),
            "absolute path outside jail must not decode"
        );
        assert!(
            decode_url_rgba("../secret.png").is_none(),
            "relative escape must not decode"
        );
        let ok = jail.join("ok.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .save(&ok)
            .expect("ok png");
        let (w, h, rgba) = decode_url_rgba("ok.png").expect("jailed relative");
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[..4], &[1, 2, 3, 255]);
        let (aw, ah, argb) =
            decode_url_rgba(&ok.to_string_lossy()).expect("absolute path inside jail");
        assert_eq!((aw, ah), (2, 2));
        assert_eq!(&argb[..4], &[1, 2, 3, 255]);
        let file_url = path_to_file_url(&ok);
        let (fw, fh, frgba) = decode_url_rgba(&file_url).expect("file url inside jail");
        assert_eq!((fw, fh), (2, 2));
        assert_eq!(&frgba[..4], &[1, 2, 3, 255]);
        reset_test_url_base();
        let _ = std::fs::remove_dir_all(&jail);
        let _ = std::fs::remove_dir_all(&outside);
    }

    fn assert_file_url_path(resolved: &str) {
        let normalized = resolved.replace('\\', "/");
        assert!(
            normalized.ends_with("tmp/test.png"),
            "file url must become a filesystem path ending in tmp/test.png, got {resolved}"
        );
    }

    #[test]
    fn svg_bytes_rasterize_straight_rgba() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2" fill="#00ff00"/></svg>"##;
        let (width, height, rgba) = decode_image_bytes(svg).expect("svg decode");
        assert_eq!((width, height), (4, 2));
        assert_eq!(&rgba[..4], &[0x00, 0xff, 0x00, 0xff]);
    }

    #[test]
    fn svg_image_href_does_not_read_local_file() {
        let dir = std::env::temp_dir().join(format!("nanaui-svg-href-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let secret = dir.join("secret.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .save(&secret)
            .expect("secret");
        let href = secret.to_string_lossy().replace('\\', "/");
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><image href=\"{href}\" width=\"4\" height=\"4\"/></svg>"
        );
        let (_, _, rgba) = decode_image_bytes(svg.as_bytes()).expect("svg");
        let red = rgba
            .chunks(4)
            .filter(|pixel| pixel[0] > 200 && pixel[3] > 16)
            .count();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(red, 0, "usvg must not fs::read image href");
    }

    #[test]
    fn svg_data_url_decodes() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"2\"><rect width=\"2\" height=\"2\" fill=\"#ff0000\"/></svg>";
        let url = format!("data:image/svg+xml,{svg}");
        let (width, height, rgba) = decode_url_rgba(&url).expect("svg data url");
        assert_eq!((width, height), (2, 2));
        assert_eq!(&rgba[..4], &[0xff, 0x00, 0x00, 0xff]);
    }

    #[test]
    fn inline_svg_path_rasterizes_via_resvg() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M0 0 H10 V10 H0 Z" fill="#ff00ff"/></svg>"##;
        let (width, height, rgba) = decode_image_bytes(svg).expect("path svg");
        assert!(width >= 1 && height >= 1, "resvg must produce a raster");
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        let inked = rgba.chunks(4).filter(|pixel| pixel[3] > 16).count();
        assert!(
            inked > 0,
            "inline svg path must ink the raster, got {inked} opaque pixels"
        );
        assert_eq!(&rgba[..4], &[0xff, 0x00, 0xff, 0xff]);
    }

    #[test]
    fn svg_raster_size_is_capped_without_aspect_distortion() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8000" height="2000"><rect width="8000" height="2000" fill="#0000ff"/></svg>"##;
        let (width, height, rgba) = decode_image_bytes(svg).expect("capped svg");
        assert_eq!((width, height), (2048, 512));
        assert_eq!(rgba.len(), (2048 * 512 * 4) as usize);
        assert_eq!(&rgba[..4], &[0x00, 0x00, 0xff, 0xff]);
    }

    #[test]
    fn svg_file_extension_hint_uses_resvg() {
        assert!(looks_like_svg_url("icons/mark.svg"));
        assert!(looks_like_svg_url("https://cdn.example/a.SVG?v=1"));
        assert!(!looks_like_svg_url("icons/mark.png"));
    }

    #[test]
    fn svg_base64_data_url_decodes() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><rect width=\"1\" height=\"1\" fill=\"#010203\"/></svg>";
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(svg.as_bytes());
        let url = format!("data:image/svg+xml;base64,{encoded}");
        let (width, height, rgba) = decode_url_rgba(&url).expect("base64 svg");
        assert_eq!((width, height), (1, 1));
        assert_eq!(&rgba[..4], &[0x01, 0x02, 0x03, 0xff]);
    }
    #[cfg(any(feature = "rich-text", feature = "bundled-fonts"))]
    #[test]
    fn svg_text_uses_the_existing_ui_font_for_latin_and_cjk() {
        for text in ["Mermaid", "流程图"] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="40"><text x="2" y="30" font-family="sans-serif" font-size="24" fill="red">{text}</text></svg>"#
            );
            let (width, height, pixels) = decode_svg_rgba(svg.as_bytes()).unwrap();
            assert_eq!((width, height), (180, 40));
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0)
                    .count()
                    > 50,
                "SVG text must produce glyph pixels"
            );
        }
    }
}
