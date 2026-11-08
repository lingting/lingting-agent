//! SSE 消费：把 `framework-http` 的流式响应切成一条一条的 `RunEvent`。
//!
//! 帧格式由服务端 `lib-web-core` 的 `sse::frame` 决定：`id:` / `event:` / 一行或多行 `data:`，
//! 以空行结束。事件类型与字段全在 `data` 的 JSON 里（生成的 `RunEvent`），因此 `event:` 行
//! 只做传输语义、不参与解析，`id:` 与 `retry:` 同样忽略。

use crate::client::ClientError;
use crate::sdk::RunEvent;
use framework_http::HttpStreamResponse;

/// 帧结束标记：单个空行。
const FRAME_END: &[u8] = b"\n\n";
/// 帧结束标记（CRLF 版本）。
const FRAME_END_CRLF: &[u8] = b"\r\n\r\n";

/// Run 事件流：逐条产出已经反序列化的事件。
pub struct EventStream {
    /// 流式响应，逐块提供响应体。
    response: HttpStreamResponse,
    /// 尚未凑成完整帧的字节，可能包含半帧或多帧。
    buffer: Vec<u8>,
    /// 响应体是否已经读到底。
    finished: bool,
    /// 请求地址，仅用于错误消息。
    url: String,
}

impl EventStream {
    /// 由流式响应构建事件流。
    pub(crate) fn new(url: String, response: HttpStreamResponse) -> Self {
        Self {
            response,
            buffer: Vec::new(),
            finished: false,
            url,
        }
    }

    /// 取下一个事件；事件流正常结束时返回 `Ok(None)`。
    pub async fn recv(&mut self) -> Result<Option<RunEvent>, ClientError> {
        loop {
            if let Some(frame) = self.take_frame() {
                return frame.map(Some);
            }
            if self.finished {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Err(ClientError::Decode {
                        detail: "事件流结束时仍有未闭合的事件帧".to_string(),
                    })
                };
            }
            match self.response.next().await {
                Some(Ok(chunk)) => self.buffer.extend_from_slice(&chunk),
                Some(Err(error)) => {
                    return Err(ClientError::from_send_error(&self.url, error));
                }
                None => self.finished = true,
            }
        }
    }

    /// 从缓冲区切出一个完整帧；不足一帧时返回 `None`。
    fn take_frame(&mut self) -> Option<Result<RunEvent, ClientError>> {
        let (end, next) = find_frame_end(&self.buffer)?;
        let frame = self.buffer[..end].to_vec();
        self.buffer.drain(..next);
        Some(parse_frame(&frame))
    }
}

/// 找到最早的帧结束标记，返回（帧内容长度，下一帧起点）。
fn find_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len() {
        if buffer[index..].starts_with(FRAME_END) {
            return Some((index, index + FRAME_END.len()));
        }
        if buffer[index..].starts_with(FRAME_END_CRLF) {
            return Some((index, index + FRAME_END_CRLF.len()));
        }
    }
    None
}

/// 解析一帧：拼接 `data:` 行后按 `RunEvent` 反序列化。
fn parse_frame(frame: &[u8]) -> Result<RunEvent, ClientError> {
    let text = std::str::from_utf8(frame).map_err(|error| ClientError::Decode {
        detail: format!("事件帧不是有效的 UTF-8：{error}"),
    })?;

    let mut data = String::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
    }

    if data.is_empty() {
        return Err(ClientError::Decode {
            detail: "事件帧没有 data 行".to_string(),
        });
    }

    serde_json::from_str(&data).map_err(|error| ClientError::Decode {
        detail: format!("事件数据不是有效的 RunEvent：{error}"),
    })
}
