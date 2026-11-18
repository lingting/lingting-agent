//! 运行时契约用例：客户端 / 占用的 DTO 与下行事件的 wire 形态（RT-005）。
//!
//! 只校验序列化快照，不发请求：`i64` 走字符串、字段 camelCase、枚举 wire 值与 SSE 的
//! `event:` 行同源。端点本身在后续任务落地。

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;
use anyhow::ensure;
use lib_bin_cli::sdk::{
    ClientCloseReason, ClientEvent, ClientEventKind, ClientUpdateKind, ClientUpdatePO, OccupancyVO, RunEvent,
    RunEventKind, RunStatus,
};
use serde_json::{Value, json};
use service_core::runtime::{ClientRecord, ClientState, RuntimeState};
use types_core::runtime::ClientCloseReason as ContractClientCloseReason;
use types_core::runtime::ClientEventKind as ContractClientEventKind;
use types_core::runtime::OccupancyVO as ContractOccupancyVO;
use types_core::session::RunEventKind as ContractRunEventKind;

/// 运行时领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "runtime.contract",
            kind: CaseKind::Shared,
            prep: &[],
            run: contract,
        },
        Case {
            name: "runtime.registry",
            kind: CaseKind::Shared,
            prep: &[],
            run: registry,
        },
    ]
}

fn contract(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();

        // 上行：字段 camelCase，枚举 wire 值是 SCREAMING_SNAKE_CASE。
        let update: ClientUpdatePO = serde_json::from_str(r#"{"clientId":"c-1","kind":"INQUIRY_ACK"}"#)?;
        ensure!(update.client_id == "c-1", "请求体字段名应为 camelCase");
        ensure!(
            update.kind == ClientUpdateKind::InquiryAck,
            "INQUIRY_ACK 未解析成 InquiryAck"
        );
        ensure!(
            serde_json::to_value(ClientUpdateKind::Ping)? == "PING",
            "Ping 的 wire 值应为 PING"
        );
        ensure!(
            serde_json::to_value(ClientUpdateKind::InquiryAck)? == "INQUIRY_ACK",
            "InquiryAck 的 wire 值应为 INQUIRY_ACK"
        );
        notes.push("上行：{\"clientId\":\"c-1\",\"kind\":\"INQUIRY_ACK\"}".to_string());

        // 下行：判别字段是 `event`，`seq` 以字符串传输，`session_event` 承载整条 `RunEvent`。
        let event = ClientEvent {
            event: ClientEventKind::SessionEvent,
            seq: 7,
            created_at_ms: 1_700_000_000_000,
            session_event: Some(RunEvent {
                event: RunEventKind::RunStarted,
                seq: 0,
                run_id: "run-1".to_string(),
                session_id: "session-1".to_string(),
                created_at_ms: 1_700_000_000_000,
                delta: None,
                message_id: Some("message-1".to_string()),
                status: Some(RunStatus::Running),
                error: None,
                input_tokens: Some(12),
                output_tokens: None,
            }),
            inquiry_at_ms: None,
            inquiry_deadline_ms: None,
            close_reason: None,
        };
        let value = serde_json::to_value(&event)?;
        ensure!(
            value.get("event") == Some(&json!("session_event")),
            "下行事件判别字段应为 event=session_event：{value}"
        );
        ensure!(value.get("seq") == Some(&json!("7")), "seq 应以字符串传输：{value}");
        ensure!(
            value.get("createdAtMs") == Some(&json!("1700000000000")),
            "createdAtMs 应以字符串传输：{value}"
        );
        ensure!(
            value.pointer("/sessionEvent/event") == Some(&json!("run_started")),
            "session_event 应原样承载 RunEvent：{value}"
        );
        ensure!(
            value.get("closeReason") == Some(&Value::Null),
            "非 client_closed 事件不应带关闭原因：{value}"
        );
        let parsed: ClientEvent = serde_json::from_value(value.clone())?;
        ensure!(parsed == event, "下行事件往返不一致");
        notes.push(format!("下行 session_event：{value}"));

        // `event:` 行与 JSON 的 `event` 字段同值：帧层用契约枚举的 `Display`，SDK 侧只有 serde。
        for (kind, wire) in [
            (ClientEventKind::Inquiry, "inquiry"),
            (ClientEventKind::SessionEvent, "session_event"),
            (ClientEventKind::ClientClosed, "client_closed"),
        ] {
            ensure!(
                serde_json::to_value(kind)? == json!(wire),
                "{kind:?} 的 wire 值应为 {wire}"
            );
        }
        for (kind, wire) in [
            (ContractClientEventKind::Inquiry, "inquiry"),
            (ContractClientEventKind::SessionEvent, "session_event"),
            (ContractClientEventKind::ClientClosed, "client_closed"),
        ] {
            ensure!(kind.to_string() == wire, "{kind:?} 的 event 行取值应为 {wire}");
        }
        notes.push("下行 event 行：inquiry / session_event / client_closed".to_string());

        // 新增的取消事件：wire 值是 run_cancelled，`Display` 与 serde 同值。
        ensure!(
            ContractRunEventKind::RunCancelled.to_string() == "run_cancelled",
            "RunCancelled 的 Display 应为 run_cancelled"
        );
        ensure!(
            serde_json::to_value(RunEventKind::RunCancelled)? == json!("run_cancelled"),
            "RunCancelled 的 wire 值应为 run_cancelled"
        );
        notes.push("取消事件：run_cancelled".to_string());

        // 关闭原因：六条来源的 wire 值是 SCREAMING_SNAKE_CASE。
        for (reason, wire) in [
            (ClientCloseReason::HeartbeatTimeout, "HEARTBEAT_TIMEOUT"),
            (ClientCloseReason::InquiryTimeout, "INQUIRY_TIMEOUT"),
            (ClientCloseReason::ClientClosed, "CLIENT_CLOSED"),
            (ClientCloseReason::StreamClosed, "STREAM_CLOSED"),
            (ClientCloseReason::ServerShutdown, "SERVER_SHUTDOWN"),
            (ClientCloseReason::JournalError, "JOURNAL_ERROR"),
        ] {
            ensure!(
                serde_json::to_value(reason)? == json!(wire),
                "{reason:?} 的 wire 值应为 {wire}"
            );
        }
        notes.push("关闭原因：六种取值的 wire 值均为 SCREAMING_SNAKE_CASE".to_string());

        // 占用：`acquiredAtMs` 以字符串传输，往返一致。
        let occupancy = OccupancyVO {
            session_id: "session-1".to_string(),
            client_id: "c-1".to_string(),
            runtime_session_id: "runtime-1".to_string(),
            acquired_at_ms: 1_700_000_000_000,
        };
        let value = serde_json::to_value(&occupancy)?;
        ensure!(
            value.get("acquiredAtMs") == Some(&json!("1700000000000")),
            "acquiredAtMs 应以字符串传输：{value}"
        );
        ensure!(
            value.get("runtimeSessionId") == Some(&json!("runtime-1")),
            "runtimeSessionId 字段缺失：{value}"
        );
        let parsed: OccupancyVO = serde_json::from_value(value)?;
        ensure!(parsed == occupancy, "占用 VO 往返不一致");
        notes.push("占用 VO：sessionId / clientId / runtimeSessionId / acquiredAtMs".to_string());

        Ok(notes)
    })
}

/// 注册表骨架：注册 / 查询 / 移除、标记的 take 语义与占用表的双向索引（RT-006）。
///
/// 直接构造 `RuntimeState` 而不是用进程级注入：注入只能发生一次，进程内的共享用例不该互相影响。
fn registry(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let state = RuntimeState::new();
        let client_id = "client-1";
        ensure!(state.client(client_id).is_none(), "初始注册表应为空");

        // 注册与查询。
        ensure!(
            state.register(ClientRecord::connected(client_id)).is_none(),
            "首次注册不应覆盖已有记录"
        );
        let record = state.client(client_id).expect("注册后应能查到记录");
        ensure!(record.client_id == client_id, "记录 ID 不一致");
        ensure!(record.state == ClientState::Connected, "注册后的状态应为 Connected");

        // 重复标记只处理一次：第二次标记（不同原因）被吞掉，take 只返回第一次的原因。
        ensure!(
            state.mark(client_id, ContractClientCloseReason::HeartbeatTimeout),
            "首次标记应写入"
        );
        ensure!(
            !state.mark(client_id, ContractClientCloseReason::InquiryTimeout),
            "重复标记不应再写入"
        );
        ensure!(
            state.client(client_id).expect("记录仍在").state == ClientState::Marked,
            "标记后的状态应为 Marked"
        );
        let marks = state.take_marks();
        ensure!(marks.len() == 1, "take 应只返回一条标记，实际 {}", marks.len());
        ensure!(
            marks[0].0 == client_id && marks[0].1 == ContractClientCloseReason::HeartbeatTimeout,
            "take 返回的原因应是第一次标记的原因：{marks:?}"
        );
        ensure!(state.take_marks().is_empty(), "take 之后标记集合应为空");

        // 未注册的客户端不能被标记（反注册后旧 clientId 的上行不会再产生标记）。
        ensure!(
            !state.mark("client-missing", ContractClientCloseReason::ClientClosed),
            "未注册的客户端不应被标记"
        );

        // 占用表：双向索引一致，释放后两侧都清空。
        let occupancy = ContractOccupancyVO {
            session_id: "session-1".to_string(),
            client_id: client_id.to_string(),
            runtime_session_id: "runtime-1".to_string(),
            acquired_at_ms: 1_700_000_000_000,
        };
        ensure!(state.occupy(occupancy.clone()).is_none(), "首次占用不应有旧占用");
        ensure!(
            state.occupancy_of_client(client_id) == Some(occupancy.clone()),
            "按客户端查不到占用"
        );
        ensure!(
            state.occupancy_of_session("session-1") == Some(occupancy.clone()),
            "按会话查不到占用"
        );
        ensure!(state.release(client_id) == Some(occupancy), "释放应返回被释放的占用");
        ensure!(
            state.occupancy_of_session("session-1").is_none(),
            "释放后会话侧索引应清空"
        );
        ensure!(state.release(client_id).is_none(), "重复释放应为空");

        // 移除（反注册）：返回被移除的记录，之后查询为空。
        let removed = state.remove(client_id).expect("移除应返回被移除的记录");
        ensure!(removed.client_id == client_id, "移除的记录 ID 不一致");
        ensure!(state.client(client_id).is_none(), "移除后不应再查到记录");
        ensure!(state.remove(client_id).is_none(), "重复移除应为空");

        Ok(vec![
            "注册 / 查询 / 移除：返回被替换与被移除的记录".to_string(),
            "重复标记只处理一次，take 后标记集合清空".to_string(),
            "占用按客户端与会话双向可查，释放后两侧清空".to_string(),
        ])
    })
}
