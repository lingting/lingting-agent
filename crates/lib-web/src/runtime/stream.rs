//! 把运行时产出的 `ClientEvent` 流转成 SSE 响应体（D-P1.1-001）。
//!
//! 一帧 = 客户端内的 `seq` 作 `id` + 事件枚举的显示值作 `event:` + 整条事件的 JSON 作 `data:`；
//! 空闲时每 [`KEEPALIVE_INTERVAL`] 注入一条注释帧作写探测，不进入 `ClientEvent`、不占 `seq`。
//! `event:` 行与 JSON 里的 `event` 字段同源（见 `ClientEventKind` 的 serde / strum 约定）。

use std::time::Duration;

use bytes::Bytes;
use futures_util::stream::{BoxStream, unfold};
use lib_web_core::sse;
use tokio::sync::mpsc;
use tokio::time::{Interval, MissedTickBehavior, interval};
use types_core::runtime::{ClientEvent, ClientEventKind};

/// 空闲保活间隔：与客户端心跳同频，只充当写失败探测。
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// 保活注释帧：不占 `seq`、不落事件文件。
const KEEPALIVE_FRAME: &str = ": keepalive\n\n";

/// 组装 SSE 响应体：`client_closed` 帧之后结束，接收端 drop 同样结束。
pub fn body(events: mpsc::Receiver<ClientEvent>) -> BoxStream<'static, Result<Bytes, std::io::Error>> {
    let mut keepalive = interval(KEEPALIVE_INTERVAL);
    // 首帧不立刻发：`reset` 把下一次到期时间推到 `now + 间隔`。
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    keepalive.reset();
    let state = StreamState {
        events,
        keepalive,
        done: false,
    };
    Box::pin(unfold(state, |mut state| async move {
        // 上一拍已经发出了关流帧：这一拍结束响应体。
        if state.done {
            return None;
        }
        let StreamState { events, keepalive, .. } = &mut state;
        tokio::select! {
            received = events.recv() => {
                let event = received?;
                // 序列化失败按单帧错误上报，由框架结束这条流。
                let frame = frame(&event).map(Bytes::from);
                // `client_closed` 是服务端主动关流的最后一帧：发完即结束响应体。
                // 发送端还可能被下行流的 watcher 持有，不能指望它被 drop 来收尾（D-P1.1-001）。
                if event.event == ClientEventKind::ClientClosed {
                    state.done = true;
                }
                Some((frame, state))
            }
            _ = keepalive.tick() => Some((Ok(Bytes::from_static(KEEPALIVE_FRAME.as_bytes())), state)),
        }
    }))
}

/// 帧合成的状态：下行通道与保活定时器。
struct StreamState {
    events: mpsc::Receiver<ClientEvent>,
    keepalive: Interval,
    /// 是否已经发出关流帧：下一拍结束响应体。
    done: bool,
}

/// 一条下行事件的帧：`id` 用客户端内序号，`data` 是整条事件（`session_event` 由嵌套字段承载）。
fn frame(event: &ClientEvent) -> Result<String, std::io::Error> {
    sse::json_frame(event.seq, &event.event.to_string(), event).map_err(std::io::Error::other)
}
