//! 占用用例：一对一约束、冲突表达与释放后重占（RT-014）。
//!
//! 全部走 HTTP：占用涉及运行时会话与事件文件，只有服务端进程里那一份状态才是真实装配。

use anyhow::{Context, ensure};
use framework_http::{HttpClient, HttpRequest};
use lib_bin_cli::client::ClientError;
use lib_bin_cli::sdk::{
    ApiClient, ApiError, OCCUPANCY_CREATE, OccupancyCreatePO, OccupancyDeletePO, OccupancyGetParams, SessionCreatePO,
};
use serde_json::json;

use crate::case::{Case, CaseFuture, CaseKind};
use crate::context::TestContext;

/// 占用领域用例。
pub(crate) fn cases() -> Vec<Case> {
    vec![Case {
        name: "runtime.occupancy",
        kind: CaseKind::Shared,
        prep: &[],
        run: occupancy,
    }]
}

/// 三类冲突、幂等重入与释放后重占（RT-014）。
fn occupancy(context: &TestContext) -> CaseFuture<'_> {
    Box::pin(async move {
        let first = context
            .client()
            .client_create(lib_bin_cli::sdk::ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        let second = context
            .client()
            .client_create(lib_bin_cli::sdk::ClientCreatePO {})
            .await
            .map_err(ClientError::from_call)?;
        let session_a = create_session(context).await?;
        let session_b = create_session(context).await?;

        // 建立占用：返回运行时会话 ID，每次占用都不同。
        let occupied = context
            .client()
            .occupancy_create(OccupancyCreatePO {
                client_id: first.client_id.clone(),
                session_id: session_a.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            occupied.client_id == first.client_id && occupied.session_id == session_a,
            "占用返回的归属不一致：{occupied:?}"
        );
        ensure!(
            !occupied.runtime_session_id.is_empty() && occupied.acquired_at_ms > 0,
            "占用应带运行时会话 ID 与建立时间：{occupied:?}"
        );

        // 幂等：同一对重复调用返回当前占用，不重建运行时会话。
        let again = context
            .client()
            .occupancy_create(OccupancyCreatePO {
                client_id: first.client_id.clone(),
                session_id: session_a.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            again.runtime_session_id == occupied.runtime_session_id,
            "同一对重复占用不应换运行时会话：{again:?} != {occupied:?}"
        );

        // 冲突一：第二个客户端占用同一会话。
        let message = post_error(
            context,
            OCCUPANCY_CREATE.path,
            json!({ "clientId": second.client_id, "sessionId": session_a }),
        )
        .await?;
        ensure!(
            message == "会话已被其他客户端占用",
            "会话被占用时应返回固定 message，实际：{message}"
        );

        // 冲突二：已占一个会话的客户端再占第二个（不做隐式切换）。
        let message = post_error(
            context,
            OCCUPANCY_CREATE.path,
            json!({ "clientId": first.client_id, "sessionId": session_b }),
        )
        .await?;
        ensure!(
            message == "客户端已占用其他会话",
            "一对一约束应返回固定 message，实际：{message}"
        );

        // 查询：当前占用人可见；无人占用的会话是 404。
        let found = context
            .client()
            .occupancy_get(OccupancyGetParams {
                session_id: session_a.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            found.runtime_session_id == occupied.runtime_session_id,
            "查询占用结果不一致：{found:?}"
        );
        let missing = context
            .client()
            .occupancy_get(OccupancyGetParams {
                session_id: session_b.clone(),
            })
            .await
            .expect_err("无人占用的会话应返回 404");
        let error = ClientError::from_call(missing);
        ensure!(
            format!("{error}").contains("会话未被占用"),
            "无人占用时应返回固定 message，实际：{error}"
        );

        // 释放：幂等；释放后会话可被另一方占用，且是新的运行时会话与新的文件。
        context
            .client()
            .occupancy_delete(OccupancyDeletePO {
                client_id: first.client_id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        context
            .client()
            .occupancy_delete(OccupancyDeletePO {
                client_id: first.client_id.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        let taken = context
            .client()
            .occupancy_create(OccupancyCreatePO {
                client_id: second.client_id.clone(),
                session_id: session_a.clone(),
            })
            .await
            .map_err(ClientError::from_call)?;
        ensure!(
            taken.runtime_session_id != occupied.runtime_session_id,
            "重新占用应产生新的运行时会话：{taken:?} == {occupied:?}"
        );

        // 收尾：把占用还回去，避免影响同一服务端的其他用例。
        context
            .client()
            .occupancy_delete(OccupancyDeletePO {
                client_id: second.client_id,
            })
            .await
            .map_err(ClientError::from_call)?;

        Ok(vec![
            format!("占用：runtimeSessionId={}", occupied.runtime_session_id),
            "同一对重复占用幂等，不重建运行时会话".to_string(),
            "两类冲突各自返回固定 message，查询无人占用为 404".to_string(),
            "释放幂等，重新占用产生新的运行时会话".to_string(),
        ])
    })
}

/// 新建一个会话，返回会话 ID。
async fn create_session(context: &TestContext) -> anyhow::Result<String> {
    let session = context
        .client()
        .session_create(SessionCreatePO {
            title: Some(context.token()),
            model: None,
        })
        .await
        .map_err(ClientError::from_call)?;
    Ok(session.id)
}

/// 发送一个预期失败的 POST 请求，返回错误 message。
async fn post_error(context: &TestContext, path: &str, body: serde_json::Value) -> anyhow::Result<String> {
    let response = HttpClient::new()
        .post(
            HttpRequest::new(format!("{}/{}", context.base_url().trim_end_matches('/'), path))
                .with_content_type("application/json")
                .with_body(body.to_string()),
        )
        .await
        .context("冲突用例的请求失败")?;
    ensure!(response.status == 400, "冲突应返回 400，实际 {}", response.status);
    let error: ApiError = serde_json::from_slice(&response.body).context("冲突响应不是有效的 ApiError")?;
    Ok(error.message)
}
