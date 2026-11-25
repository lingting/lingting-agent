//! `client.post` 契约用例（S-8）：三种 `kind` 各有正确响应，四种字段组合错误各返回固定 400 文案。
//!
//! 这条用例只关心上行通道本身：`TOOL_CONFIRM` 的成功路径要挂起一个 Run，归
//! `tool.confirm.*`；这里用「未注册客户端」这种确定性失败来证明鉴权层仍在。

use anyhow::{Result, ensure};
use framework_http::{HttpClient, HttpRequest};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{ApiClient, ClientCreatePO, ClientDecision, ClientDeletePO, ClientPostKind, ClientPostPO};
use serde_json::{Value, json};

use super::case;
use crate::case::{Case, CaseFuture, CaseKind};
use crate::util::context::TestContext;

/// 缺 `toolCallId` 的固定文案（`lib-web` 的 `MESSAGE_TOOL_CALL_ID_MISSING`）。
const TOOL_CALL_ID_MISSING: &str = "工具确认缺少 toolCallId";
/// 缺 `decision` 的固定文案。
const DECISION_MISSING: &str = "工具确认缺少 decision";
/// 心跳带确认字段的固定文案。
const CONFIRM_FIELDS_UNEXPECTED: &str = "心跳与问询应答不得携带工具确认字段";
/// 未注册客户端的固定文案。
const CLIENT_NOT_REGISTERED: &str = "客户端未注册，请重新连接";
/// 请求体字段转换失败的固定文案（框架层）：`kind` 不在枚举取值里时走这里。
const KIND_INVALID: &str = "请求参数转换失败";

/// `client.post` 用例。
pub(super) fn cases() -> Vec<Case> {
    // 独立服务端：本用例会注册 / 反注册客户端，不能落在共享实例上。
    vec![case("client.post", CaseKind::Isolated, post)]
}

/// 组装一次上行请求。
fn po(
    client_id: &str,
    kind: ClientPostKind,
    tool_call_id: Option<&str>,
    decision: Option<ClientDecision>,
) -> ClientPostPO {
    ClientPostPO {
        client_id: client_id.to_string(),
        kind,
        tool_call_id: tool_call_id.map(str::to_owned),
        decision,
    }
}

/// 断言一次上行返回固定 400。
async fn expect_400(context: &TestContext, po: ClientPostPO, expected: &str) -> Result<String> {
    match context.client().client_post(po).await.map_err(ClientError::from_call) {
        Err(ClientError::Status { code, message }) => {
            ensure!(code == 400, "字段组合错误应返回 400，实际 {code}");
            ensure!(message == expected, "固定文案不符：{message} != {expected}");
            Ok(message)
        }
        other => anyhow::bail!("字段组合错误应返回固定 400，实际 {other:?}"),
    }
}

/// 三种 `kind` 与四种字段组合错误。
fn post(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let mut notes = Vec::new();
        let created = context
            .client()
            .client_create(ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        let client_id = created.client_id.clone();

        // 一、PING：刷新 lastSeen 并同步回 pong。
        let pong = context
            .client()
            .client_post(po(&client_id, ClientPostKind::Ping, None, None))
            .await
            .map_err(ClientError::from_call)?;
        ensure!(pong.pong_at_ms > 0, "PING 应返回 pong：{:?}", pong);
        notes.push(format!("PING → pong_at_ms={}", pong.pong_at_ms));

        // 二、INQUIRY_ACK：清掉待答状态（不刷新 lastSeen，但同样回 pong）。
        let ack = context
            .client()
            .client_post(po(&client_id, ClientPostKind::InquiryAck, None, None))
            .await
            .map_err(ClientError::from_call)?;
        ensure!(ack.pong_at_ms > 0, "INQUIRY_ACK 应返回 pong：{:?}", ack);
        notes.push(format!("INQUIRY_ACK → pong_at_ms={}", ack.pong_at_ms));

        // 三、四种字段组合错误。
        let missing_id = expect_400(
            context,
            po(
                &client_id,
                ClientPostKind::ToolConfirm,
                None,
                Some(ClientDecision::Approve),
            ),
            TOOL_CALL_ID_MISSING,
        )
        .await?;
        let missing_decision = expect_400(
            context,
            po(&client_id, ClientPostKind::ToolConfirm, Some("call-x"), None),
            DECISION_MISSING,
        )
        .await?;
        let unexpected = expect_400(
            context,
            po(
                &client_id,
                ClientPostKind::Ping,
                Some("call-x"),
                Some(ClientDecision::Reject),
            ),
            CONFIRM_FIELDS_UNEXPECTED,
        )
        .await?;
        let unknown_kind = unknown_kind_400(context, &client_id).await?;
        notes.push(format!(
            "四种 400：{missing_id} / {missing_decision} / {unexpected} / {unknown_kind}"
        ));

        // 四、TOOL_CONFIRM 的鉴权第一层：未注册的客户端连确认都不许发。
        let unregistered = expect_400(
            context,
            po(
                "client-not-exist",
                ClientPostKind::ToolConfirm,
                Some("call-x"),
                Some(ClientDecision::Approve),
            ),
            CLIENT_NOT_REGISTERED,
        )
        .await?;
        notes.push(format!("未注册客户端：{unregistered}"));

        context
            .client()
            .client_delete(ClientDeletePO {
                client_id: client_id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        Ok(notes)
    })
}

/// 未知 `kind`：SDK 的枚举发不出来，只能直接发一段 JSON（字段值不在枚举内）。
async fn unknown_kind_400(context: &TestContext, client_id: &str) -> Result<String> {
    let body: Value = json!({"clientId": client_id, "kind": "NOPE"});
    let response = HttpClient::new()
        .post(
            HttpRequest::new(format!("{}/api/client/post", context.base_url()))
                .with_content_type("application/json")
                .with_body(serde_json::to_vec(&body)?),
        )
        .await
        .map_err(|error| anyhow::anyhow!("POST api/client/post 失败：{error:#}"))?;
    ensure!(response.status == 400, "未知 kind 应返回 400，实际 {}", response.status);
    let envelope: Value = serde_json::from_str(&response.string())?;
    let message = envelope["message"].as_str().unwrap_or_default().to_string();
    ensure!(message == KIND_INVALID, "未知 kind 的固定文案不符：{message}");
    Ok(message)
}
