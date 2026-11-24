//! 运行时用例：客户端 / 占用的 DTO 与下行事件的 wire 形态（RT-005）、注册表骨架（RT-006）、
//! 客户端端点与下行流断开标记（RT-007）、心跳与未注册拒绝（RT-008）、问询与应答（RT-009）、
//! 定时扫描（RT-010）。
//!
//! 契约用例只校验序列化快照，不发请求：`i64` 走字符串、字段 camelCase、枚举 wire 值与 SSE 的
//! `event:` 行同源。

use std::time::Duration;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;
use agent_core::session::RunEventKind as ContractRunEventKind;
use anyhow::{Context, ensure};
use framework_http::{HttpClient, HttpRequest};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ApiError, CLIENT_STREAM, CLIENT_UPDATE, ClientCloseReason, ClientCreatePO, ClientEvent, ClientEventKind,
    ClientUpdateKind, ClientUpdatePO, OccupancyVO, RunEvent, RunEventKind, RunStatus,
};
use manager_agent::runtime::{
    CLIENT_TTL_MS, ClientRecord, ClientState, INQUIRY_AFTER_MS, INQUIRY_WINDOW_MS, RuntimeState, SCAN_INTERVAL_MS, scan,
};
use serde_json::{Value, json};
use tokio::time::sleep;
use types_core::runtime::ClientCloseReason as ContractClientCloseReason;
use types_core::runtime::ClientEventKind as ContractClientEventKind;
use types_core::runtime::ClientUpdateKind as ContractClientUpdateKind;
use types_core::runtime::OccupancyVO as ContractOccupancyVO;

use crate::util::support::session_service;

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
        Case {
            name: "runtime.client",
            kind: CaseKind::Shared,
            prep: &[],
            run: client_endpoints,
        },
        Case {
            name: "runtime.stream",
            kind: CaseKind::Shared,
            prep: &[],
            run: stream_mark,
        },
        Case {
            name: "runtime.heartbeat",
            kind: CaseKind::Shared,
            prep: &[],
            run: heartbeat,
        },
        Case {
            name: "runtime.inquiry",
            kind: CaseKind::Shared,
            prep: &[],
            run: inquiry,
        },
        Case {
            name: "runtime.scan",
            kind: CaseKind::Shared,
            prep: &[],
            run: scan_once,
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
                tool_call_id: None,
                tool_name: None,
                tool_arguments: None,
                tool_status: None,
                tool_duration_ms: None,
                tool_round: None,
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
        let state = RuntimeState::new(session_service().await?);
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

/// 客户端端点：注册返回 `clientId` 与首个 pong，下行流是 `text/event-stream`，
/// 未注册的 `clientId` 建流被拒（RT-007）。
fn client_endpoints(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 注册：`clientId` 由服务端生成，首个 pong 落在注册响应里。
        let created = context
            .client()
            .client_create(ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        ensure!(!created.client_id.is_empty(), "注册应返回 clientId");
        ensure!(created.pong_at_ms > 0, "注册应返回首个 pong 时间戳");

        // 建流：拿到响应头即算建立，drop 响应体就是客户端断流。
        let stream = HttpClient::new()
            .stream(HttpRequest::new(stream_url(context, &created.client_id)))
            .await
            .context("建立下行流失败")?;
        ensure!(stream.status == 200, "建流应返回 200，实际 {}", stream.status);
        ensure!(
            stream.mime_type().as_deref() == Some("text/event-stream"),
            "下行流的 content-type 应为 text/event-stream，实际 {:?}",
            stream.content_type()
        );
        drop(stream);

        // 未注册的 clientId：流建立以前就拒绝，走普通错误响应（D-P1.1-005）。
        let response = HttpClient::new()
            .get(HttpRequest::new(stream_url(context, "client-missing")))
            .await
            .context("未注册建流请求失败")?;
        ensure!(response.status == 400, "未注册建流应返回 400，实际 {}", response.status);
        let error: ApiError =
            serde_json::from_slice(&response.body).context("未注册建流的错误体不是有效的 ApiError")?;
        ensure!(
            error.message == "客户端未注册，请重新连接",
            "未注册建流的错误消息不可读：{}",
            error.message
        );

        Ok(vec![
            format!("注册：clientId={} pongAtMs={}", created.client_id, created.pong_at_ms),
            "建流：200 text/event-stream，drop 响应体即断流".to_string(),
            format!("未注册建流：{} {}", error.code, error.message),
        ])
    })
}

/// 下行流断开：注册表出现一次 `StreamClosed` 标记（RT-007）。
///
/// 标记是进程内状态，只能直接对 `RuntimeState` 断言；注入的是服务端进程里那一个实例，测试用独立实例。
fn stream_mark(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let state = RuntimeState::new(session_service().await?);
        let client_id = "client-stream";
        state.register(ClientRecord::connected(client_id));

        ensure!(
            state.open_stream("client-missing").is_err(),
            "未注册的客户端不应能建立下行流"
        );

        let receiver = state.open_stream(client_id).expect("已注册的客户端应能建立下行流");
        ensure!(state.take_marks().is_empty(), "建流本身不应产生标记");

        // 断流：接收端 drop，watcher 在独立任务里打标记。
        drop(receiver);
        let marks = wait_marks(&state).await;
        ensure!(marks.len() == 1, "断流应只产生一条标记，实际 {marks:?}");
        ensure!(marks[0].0 == client_id, "标记的客户端不一致：{marks:?}");
        ensure!(
            marks[0].1 == ContractClientCloseReason::StreamClosed,
            "标记原因应为 StreamClosed：{marks:?}"
        );
        ensure!(
            state.client(client_id).expect("记录仍在").state == ClientState::Marked,
            "断流后记录状态应为 Marked"
        );

        sleep(Duration::from_millis(50)).await;
        ensure!(state.take_marks().is_empty(), "标记只应出现一次");

        Ok(vec![
            "未注册建流被拒".to_string(),
            "断流后出现一条 StreamClosed 标记，且只出现一次".to_string(),
        ])
    })
}

/// 拼出服务的绝对地址：查询参数由调用方附加，没有路径占位符（D-008）。
fn url(context: &TestContext, path: &str) -> String {
    format!(
        "{}/{}",
        context.base_url().trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// 下行流的地址：`clientId` 只走查询参数。
fn stream_url(context: &TestContext, client_id: &str) -> String {
    format!("{}?clientId={}", url(context, CLIENT_STREAM.path), client_id)
}

/// 心跳：已注册 ping 前进 `lastSeen` 并回 pong，未注册被拒，问询应答不动 `lastSeen`（RT-008）。
fn heartbeat(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 端点侧：注册后 ping 成功，pong 在响应体里；未注册的 clientId 一律 400。
        let created = context
            .client()
            .client_create(ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        sleep(Duration::from_millis(5)).await;
        let pong = context
            .client()
            .client_update(ClientUpdatePO {
                client_id: created.client_id.clone(),
                kind: ClientUpdateKind::Ping,
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            pong.pong_at_ms >= created.pong_at_ms,
            "ping 的 pong 不应早于注册时的 pong：{} < {}",
            pong.pong_at_ms,
            created.pong_at_ms
        );

        let response = HttpClient::new()
            .post(
                HttpRequest::new(url(context, CLIENT_UPDATE.path))
                    .with_content_type("application/json")
                    .with_body(r#"{"clientId":"client-missing","kind":"PING"}"#),
            )
            .await
            .context("未注册的心跳请求失败")?;
        ensure!(response.status == 400, "未注册心跳应返回 400，实际 {}", response.status);
        let error: ApiError =
            serde_json::from_slice(&response.body).context("未注册心跳的错误体不是有效的 ApiError")?;
        ensure!(
            error.message == "客户端未注册，请重新连接",
            "未注册心跳的错误消息不可读：{}",
            error.message
        );

        // 注册表侧：只有 ping 前进 `lastSeen`，心跳路径不产生标记（业务事件由事件文件承担）。
        let state = RuntimeState::new(session_service().await?);
        let client_id = "client-heartbeat";
        state.register(ClientRecord::connected(client_id));
        let registered = state.client(client_id).expect("记录仍在").last_seen_ms;

        sleep(Duration::from_millis(5)).await;
        let pong_at_ms = state
            .heartbeat(client_id, ContractClientUpdateKind::Ping)
            .expect("已注册的客户端心跳应成功");
        let seen = state.client(client_id).expect("记录仍在").last_seen_ms;
        ensure!(seen > registered, "ping 应前进 lastSeen：{seen} <= {registered}");
        ensure!(
            pong_at_ms == seen,
            "pong 时刻应与 lastSeen 一致：{pong_at_ms} != {seen}"
        );

        sleep(Duration::from_millis(5)).await;
        state
            .heartbeat(client_id, ContractClientUpdateKind::InquiryAck)
            .expect("问询应答应成功");
        ensure!(
            state.client(client_id).expect("记录仍在").last_seen_ms == seen,
            "问询应答不应刷新 lastSeen"
        );
        ensure!(state.take_marks().is_empty(), "心跳路径不应产生标记");
        ensure!(
            state
                .heartbeat("client-missing", ContractClientUpdateKind::Ping)
                .is_err(),
            "未注册的客户端心跳应失败"
        );

        Ok(vec![
            format!("ping：pongAtMs={}", pong.pong_at_ms),
            format!("未注册心跳：{} {}", error.code, error.message),
            "ping 前进 lastSeen，问询应答不动 lastSeen，心跳不产生标记".to_string(),
        ])
    })
}

/// 问询与应答：到时机下行一条 `inquiry`，同刻不叠加；无应答过窗口、有应答清除待答状态（RT-009）。
///
/// 时间轴由调用方给：`inquire` / `inquiry_due` 都接受 `now_ms`，因此不必真实等待 30s。
fn inquiry(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let state = RuntimeState::new(session_service().await?);
        let client_id = "client-inquiry";
        state.register(ClientRecord::connected(client_id));
        let mut events = state.open_stream(client_id).expect("已注册的客户端应能建立下行流");
        let registered = state.client(client_id).expect("记录仍在").last_seen_ms;

        // 未到时机：`lastSeen` 还没超过 `INQUIRY_AFTER` 就不投递。
        ensure!(
            state.inquire(client_id, registered + INQUIRY_AFTER_MS).is_none(),
            "未到问询时机不应投递"
        );
        ensure!(events.try_recv().is_err(), "未到时机不应有下行事件");

        // 到时机：下行一条 `inquiry`，载荷带投递与截止时刻，`seq` 从 0 开始。
        let now = registered + INQUIRY_AFTER_MS + 1;
        let delivered = state.inquire(client_id, now).expect("到时机应投递问询");
        ensure!(delivered.requested_at_ms == now, "问询投递时刻不一致：{delivered:?}");
        ensure!(
            delivered.deadline_ms == now + INQUIRY_WINDOW_MS,
            "问询截止时刻应为投递时刻 + 窗口：{delivered:?}"
        );
        let event = events.try_recv().expect("投递后下行通道应有 inquiry 事件");
        ensure!(
            event.event == ContractClientEventKind::Inquiry,
            "下行事件应为 inquiry：{event:?}"
        );
        ensure!(event.seq == 0, "首个下行事件的 seq 应从 0 开始，实际 {}", event.seq);
        ensure!(event.inquiry_at_ms == Some(now), "inquiry 帧应带投递时刻：{event:?}");
        ensure!(
            event.inquiry_deadline_ms == Some(delivered.deadline_ms),
            "inquiry 帧应带应答截止时刻：{event:?}"
        );

        // 不叠加：已有未答问询时再问询既不投递，也不产生第二条事件。
        ensure!(state.inquire(client_id, now + 1).is_none(), "已有未答问询时不应再投递");
        ensure!(events.try_recv().is_err(), "不应叠加第二条 inquiry");

        // 无应答：过了应答窗口即判定过期；标记本身由定时任务做，问询只置待答状态。
        let record = state.client(client_id).expect("记录仍在");
        ensure!(
            !record.inquiry_expired(delivered.deadline_ms - 1),
            "应答窗口内不应判定过期"
        );
        ensure!(record.inquiry_expired(delivered.deadline_ms), "过了应答窗口应判定过期");
        ensure!(state.take_marks().is_empty(), "问询本身不应产生标记");

        // 有应答：清掉待答状态，且不刷新 `lastSeen`。
        let pong_at_ms = state
            .heartbeat(client_id, ContractClientUpdateKind::InquiryAck)
            .expect("问询应答应成功");
        let record = state.client(client_id).expect("记录仍在");
        ensure!(record.inquiry.is_none(), "应答后不应再有待答问询");
        ensure!(!record.inquiry_expired(pong_at_ms), "应答后不应判定过期：{record:?}");
        ensure!(record.last_seen_ms == registered, "问询应答不应刷新 lastSeen");
        ensure!(state.take_marks().is_empty(), "有应答不应产生标记");

        Ok(vec![
            format!("问询：requestedAtMs={now} deadlineMs={}", delivered.deadline_ms),
            "同一客户端同刻只有一条未答问询".to_string(),
            "无应答过窗口判定过期；有应答清除待答状态且不标记".to_string(),
        ])
    })
}

/// 定时扫描：任务已注册、TTL 超时标记、问询先投递后标记、关闭后停手（RT-010）。
///
/// 时间轴全部由 `scan` 的 `now_ms` 给，因此不引入真实等待；`live` 直接表达“进程是否仍在服务”。
fn scan_once(_context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        // 任务注册与参数自洽：宏上的字面量必须与常量一致，否则超时窗口会与文档不符。
        let interval = framework_timer::timer_iter()
            .find(|registration| registration.name == "client_scan")
            .map(|registration| registration.interval);
        ensure!(
            interval == Some(SCAN_INTERVAL_MS),
            "定时任务 client_scan 的间隔应为 {SCAN_INTERVAL_MS}ms，实际 {interval:?}"
        );
        ensure!(
            INQUIRY_AFTER_MS + INQUIRY_WINDOW_MS < CLIENT_TTL_MS,
            "问询必须在心跳超时之前走完，否则永远不会投递"
        );

        let state = RuntimeState::new(session_service().await?);

        // 心跳超时：TTL 边界内不标记，超出一毫秒的第一次扫描即标记，且只标记一次。
        let idle = "client-scan-idle";
        state.register(ClientRecord::connected(idle));
        let registered = state.client(idle).expect("记录仍在").last_seen_ms;
        scan(&state, registered + CLIENT_TTL_MS, true);
        ensure!(state.take_marks().is_empty(), "TTL 边界内不应标记");
        scan(&state, registered + CLIENT_TTL_MS + 1, true);
        let marks = state.take_marks();
        ensure!(
            marks.len() == 1 && marks[0].1 == ContractClientCloseReason::HeartbeatTimeout,
            "超过 TTL 应标记 HeartbeatTimeout：{marks:?}"
        );
        scan(&state, registered + CLIENT_TTL_MS + SCAN_INTERVAL_MS, true);
        ensure!(state.take_marks().is_empty(), "已标记的客户端不应重复标记");

        // 问询链路：先投递一条 inquiry，窗口过后才标记 InquiryTimeout。
        let asking = "client-scan-asking";
        state.register(ClientRecord::connected(asking));
        let mut events = state.open_stream(asking).expect("已注册的客户端应能建立下行流");
        let seen = state.client(asking).expect("记录仍在").last_seen_ms;
        scan(&state, seen + INQUIRY_AFTER_MS + 1, true);
        ensure!(state.take_marks().is_empty(), "问询阶段不应标记");
        let event = events.try_recv().expect("扫描应投递一条 inquiry");
        ensure!(
            event.event == ContractClientEventKind::Inquiry,
            "扫描应投递 inquiry：{event:?}"
        );
        let deadline = event.inquiry_deadline_ms.expect("inquiry 帧应带应答截止时刻");
        scan(&state, deadline, true);
        let marks = state.take_marks();
        ensure!(
            marks.len() == 1 && marks[0].1 == ContractClientCloseReason::InquiryTimeout,
            "问询无应答应标记 InquiryTimeout：{marks:?}"
        );

        // 关闭：`live == false` 时既不判定也不投递。
        let closing = "client-scan-closing";
        state.register(ClientRecord::connected(closing));
        let mut events = state.open_stream(closing).expect("已注册的客户端应能建立下行流");
        let seen = state.client(closing).expect("记录仍在").last_seen_ms;
        scan(&state, seen + CLIENT_TTL_MS + SCAN_INTERVAL_MS, false);
        ensure!(state.take_marks().is_empty(), "关闭后不应标记");
        ensure!(events.try_recv().is_err(), "关闭后不应投递问询");

        Ok(vec![
            format!("定时任务 client_scan：{SCAN_INTERVAL_MS}ms / TTL {CLIENT_TTL_MS}ms"),
            "超过 TTL 标记 HeartbeatTimeout，重复扫描不再标记".to_string(),
            "问询无应答在窗口过后标记 InquiryTimeout".to_string(),
            "关闭后不标记、不投递".to_string(),
        ])
    })
}

/// 等 watcher 打上标记：watcher 在独立任务里跑，这里轮询而不是固定 sleep。
async fn wait_marks(state: &RuntimeState) -> Vec<(String, ContractClientCloseReason)> {
    for _ in 0..100 {
        let marks = state.take_marks();
        if !marks.is_empty() {
            return marks;
        }
        sleep(Duration::from_millis(10)).await;
    }
    Vec::new()
}
