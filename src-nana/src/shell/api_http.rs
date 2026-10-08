//! API Playground 的外部 HTTP 请求。
//!
//! 外部素材 API 跑在本机 `http://127.0.0.1`，这里用标准库 TCP 发一次 HTTP/1.1 请求，
//! `Connection: close` 后读到底，再按 `Content-Length` 或分块编码取出正文。只支持 http://，
//! 其它协议直接报错，不静默降级。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

/// 一次请求。
#[derive(Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// 一次响应。
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// 拆出主机、端口和路径。只认 `http://`。
fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(format!("只支持 http:// 地址：{url}"));
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| format!("端口无效：{authority}"))?),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(format!("地址缺少主机：{url}"));
    }
    Ok((host.to_string(), port, path.to_string()))
}

/// 发送请求并读完响应。网络错误和格式错误都原样返回给页面。
pub fn execute(request: &HttpRequest) -> Result<HttpResponse, String> {
    let (host, port, path) = split_url(&request.url)?;
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("解析地址失败：{error}"))?
        .next()
        .ok_or_else(|| format!("解析不到地址：{host}"))?;
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).map_err(|error| format!("连接失败：{error}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|error| format!("设置读超时失败：{error}"))?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|error| format!("设置写超时失败：{error}"))?;
    let body = request.body.clone().unwrap_or_default();
    let mut head = format!("{} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nAccept: */*\r\n", request.method);
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if request.body.is_some() {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(|error| format!("发送请求失败：{error}"))?;
    stream.write_all(body.as_bytes()).map_err(|error| format!("发送请求体失败：{error}"))?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|error| format!("读取响应失败：{error}"))?;
    parse_response(&raw, request.method == "HEAD")
}

/// 解析状态行、响应头和正文。
pub fn parse_response(raw: &[u8], head_only: bool) -> Result<HttpResponse, String> {
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").ok_or("响应没有完整的头部")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let rest = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("响应没有状态行")?;
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next();
    let status = parts.next().and_then(|code| code.parse::<u16>().ok()).ok_or_else(|| format!("状态行无效：{status_line}"))?;
    let status_text = parts.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|line| line.split_once(':').map(|(name, value)| (name.trim().to_string(), value.trim().to_string())))
        .collect::<Vec<_>>();
    let header = |name: &str| headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.clone());
    let body = if head_only {
        Vec::new()
    } else if header("transfer-encoding").is_some_and(|value| value.to_ascii_lowercase().contains("chunked")) {
        dechunk(rest)?
    } else if let Some(length) = header("content-length").and_then(|value| value.parse::<usize>().ok()) {
        rest[..length.min(rest.len())].to_vec()
    } else {
        rest.to_vec()
    };
    Ok(HttpResponse { status, status_text, headers, body: String::from_utf8_lossy(&body).to_string() })
}

/// 解分块编码。
fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|window| window == b"\r\n").ok_or("分块长度行不完整")?;
        let size_text = String::from_utf8_lossy(&data[..line_end]).to_string();
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| format!("分块长度无效：{size_text}"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size {
            return Err("分块正文不完整".into());
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn splits_only_http_urls() {
        assert_eq!(split_url("http://127.0.0.1:41595/external/v1/health"), Ok(("127.0.0.1".into(), 41595, "/external/v1/health".into())));
        assert_eq!(split_url("http://localhost"), Ok(("localhost".into(), 80, "/".into())));
        assert!(split_url("https://example.com/").is_err());
        assert!(split_url("http://:80/").is_err());
        assert!(split_url("http://host:port/").is_err());
    }

    #[test]
    fn parses_length_chunked_and_head_responses() {
        let response = parse_response(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}extra", false).expect("响应");
        assert_eq!((response.status, response.status_text.as_str(), response.body.as_str()), (200, "OK", "{}"));
        assert_eq!(response.headers[0], ("Content-Type".into(), "application/json".into()));
        let chunked = parse_response(b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=1\r\nde\r\n0\r\n\r\n", false).expect("分块");
        assert_eq!((chunked.status, chunked.body.as_str()), (404, "abcde"));
        let head = parse_response(b"HTTP/1.1 204 No Content\r\nContent-Length: 10\r\n\r\n", true).expect("HEAD");
        assert!(head.body.is_empty());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n", false).is_err());
        assert!(parse_response(b"garbage\r\n\r\n", false).is_err());
    }

    #[test]
    fn sends_headers_and_body_to_a_loopback_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("本机端口");
        let port = listener.local_addr().expect("地址").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("连接");
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !String::from_utf8_lossy(&request).contains("{\"a\":1}") {
                let read = stream.read(&mut buffer).expect("读请求");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 4\r\n\r\ndone").expect("写响应");
            String::from_utf8_lossy(&request).to_string()
        });
        let response = execute(&HttpRequest {
            method: "POST".into(),
            url: format!("http://127.0.0.1:{port}/external/v1/assets:add"),
            headers: vec![("Authorization".into(), "Bearer t".into())],
            body: Some("{\"a\":1}".into()),
        })
        .expect("请求");
        assert_eq!((response.status, response.body.as_str()), (201, "done"));
        let request = server.join().expect("服务线程");
        assert!(request.starts_with("POST /external/v1/assets:add HTTP/1.1\r\n"));
        assert!(request.contains("Authorization: Bearer t\r\n"));
        assert!(request.contains("Content-Length: 7\r\n"));
    }
}
