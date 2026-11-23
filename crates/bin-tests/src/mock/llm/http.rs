//! 最小 HTTP/1.1 服务端：请求解析与响应写出。
//!
//! 只覆盖 Mock LLM 需要的能力：一条连接一个请求（响应固定 `Connection: close`）、
//! `Content-Length` 定长请求体、不支持分块上传。故意不引入 Web 框架（D-P2-021）。

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 请求头长度上限。
const MAX_HEAD: usize = 16 * 1024;
/// 请求体长度上限。
const MAX_BODY: usize = 1024 * 1024;
/// 单次读取的缓冲大小。
const READ_BUFFER: usize = 4096;

/// 解析后的请求。
pub(crate) struct Request {
    pub(crate) method: String,
    /// 不含查询串的路径。
    pub(crate) path: String,
    /// 查询参数（保持原顺序；同名取第一个）。
    pub(crate) query: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Request {
    /// 取查询参数。
    pub(crate) fn query_value(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// 请求体按 JSON 解析；不是 JSON 时返回 `None`（由调用方决定如何回应）。
    pub(crate) fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// 读一个完整的请求；客户端提前关闭或请求格式非法时返回错误。
pub(crate) async fn read_request(socket: &mut TcpStream) -> Result<Request> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(position) = find(&buffer, b"\r\n\r\n") {
            break position;
        }
        if buffer.len() >= MAX_HEAD {
            bail!("请求头超过 {MAX_HEAD} 字节");
        }
        let mut chunk = [0u8; READ_BUFFER];
        let read = socket.read(&mut chunk).await.context("读取请求头失败")?;
        if read == 0 {
            bail!("客户端在读请求头时关闭连接");
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&buffer[..head_end]).context("请求头不是 UTF-8")?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().context("缺少请求行")?;
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().context("请求行缺少目标地址")?;
    if method.is_empty() || parts.next().is_none() {
        bail!("请求行格式非法：{request_line}");
    }
    let (path, query) = split_target(target);

    let mut content_length = 0usize;
    for line in lines {
        let (name, value) = line.split_once(':').context("请求头缺少冒号")?;
        let name = name.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().context("Content-Length 不是数字")?;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            bail!("不支持分块请求体");
        }
    }
    if content_length > MAX_BODY {
        bail!("请求体超过 {MAX_BODY} 字节");
    }

    // `buffer` 里可能已经带上了请求体的一部分。
    let mut body = buffer.split_off(head_end + 4);
    while body.len() < content_length {
        let mut chunk = [0u8; READ_BUFFER];
        let read = socket.read(&mut chunk).await.context("读取请求体失败")?;
        if read == 0 {
            bail!("客户端在发送请求体时关闭连接");
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(Request {
        method,
        path,
        query,
        body,
    })
}

/// 写一个 JSON 响应。
pub(crate) async fn write_json(socket: &mut TcpStream, status: u16, body: &Value) -> Result<()> {
    let payload = serde_json::to_vec(body).context("序列化响应体失败")?;
    write_body(socket, status, "application/json", &payload).await
}

/// 写一个原样响应体：`InvalidJson` 场景需要故意写坏 JSON。
pub(crate) async fn write_body(socket: &mut TcpStream, status: u16, content_type: &str, payload: &[u8]) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        reason(status),
        payload.len()
    );
    socket.write_all(head.as_bytes()).await.context("写出响应头失败")?;
    socket.write_all(payload).await.context("写出响应体失败")?;
    Ok(())
}

/// 写出 SSE 响应头。
///
/// 不写 `content-length`：响应以关闭连接收尾，客户端逐块读取。
pub(crate) async fn begin_sse(socket: &mut TcpStream) -> io::Result<()> {
    let head =
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
    socket.write_all(head.as_bytes()).await
}

/// 写一条 SSE 事件（`data: <payload>` + 空行）。
pub(crate) async fn write_event(socket: &mut TcpStream, payload: &str) -> io::Result<()> {
    socket.write_all(format!("data: {payload}\n\n").as_bytes()).await
}

/// 客户端是否已经关闭连接：读到 EOF，或出现除「暂无数据」以外的读错误。
///
/// 写失败不一定立刻发生（内核缓冲可能还能收下一次写入），因此在每个分块之间主动探一次。
pub(crate) fn is_closed(socket: &TcpStream) -> bool {
    let mut probe = [0u8; 32];
    match socket.try_read(&mut probe) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => error.kind() != io::ErrorKind::WouldBlock,
    }
}

/// 状态码对应的原因短语；只覆盖 Mock 会回的几种。
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// 把目标地址拆成路径与查询参数；不做百分号解码（参数只有场景名与数字）。
fn split_target(target: &str) -> (String, Vec<(String, String)>) {
    let Some((path, query)) = target.split_once('?') else {
        return (target.to_string(), Vec::new());
    };
    let pairs = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (pair.to_string(), String::new()),
        })
        .collect();
    (path.to_string(), pairs)
}

/// 查找子串首次出现的字节位置。
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}
