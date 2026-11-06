//! 探针接口。
//!
//! 覆盖 `web_api` 的返回值形态（`Void` / `Type` / `Result<T>` / `WebResponse` / `WebResponseOf<T>`）、
//! 参数提取（`Json<T>` / `Query<T>`）、`auth` 显式声明与错误响应。

use crate::probe::dto::{ProbeEchoPO, ProbeEchoVO, ProbeQueryPO, ProbeQueryVO};
use anyhow::Result;
use framework_web::{AuthRule, Json, Query, WebError, WebResponse, WebResponseOf, web_api_get, web_api_post};

/// 探针响应的自定义头，用于区分手工构造的响应。
const PROBE_HEADER: &str = "x-probe-kind";
/// 查询参数未传数量时的默认值。
const DEFAULT_SIZE: i64 = 10;
/// 探针允许的数量上限，超过时走错误分支。
const MAX_SIZE: i64 = 100;

/// 无参数、无返回值：验证 204 空响应。
#[web_api_get(path = "api/probe/void")]
pub async fn probe_void() {}

/// `Json<T>` 请求体 + `Result<T>` 返回值：验证请求体提取与错误响应。
#[web_api_post(path = "api/probe/echo")]
pub async fn probe_echo(echo: Json<ProbeEchoPO>) -> Result<ProbeEchoVO> {
    let params = echo.0;
    Ok(ProbeEchoVO {
        id: params.id,
        text: params.text,
        tag_count: params.tags.len() as i64,
        flag: params.flag.unwrap_or_default(),
    })
}

/// `Query<T>` 查询参数 + `Result<T>` 返回值：验证查询参数提取。
#[web_api_get(path = "api/probe/query")]
pub async fn probe_query(query: Query<ProbeQueryPO>) -> Result<ProbeQueryVO> {
    Ok(probe_result(query.0))
}

/// 直接返回载荷类型，并显式声明匿名访问：验证 `R<T>` 包装与 `auth` 参数。
#[web_api_get(path = "api/probe/typed", auth = AuthRule::anonymous())]
pub async fn probe_typed(query: Query<ProbeQueryPO>) -> ProbeQueryVO {
    probe_result(query.0)
}

/// 显式返回业务错误：验证 `WebError` 的状态码与响应体。
#[web_api_get(path = "api/probe/fail")]
pub async fn probe_fail(query: Query<ProbeQueryPO>) -> Result<ProbeQueryVO> {
    let params = query.0;
    let size = params.size.unwrap_or(DEFAULT_SIZE);
    if size > MAX_SIZE {
        return Err(
            WebError::parameter("size 超出探针允许范围", format!("请求数量 {size} 超过上限 {MAX_SIZE}")).into(),
        );
    }
    Ok(probe_result(params))
}

/// 直接返回 `WebResponse`：验证自定义状态码与响应头，并且在元数据里被标记为 Blob。
#[web_api_get(path = "api/probe/response")]
pub async fn probe_response() -> WebResponse {
    let mut response = WebResponse::from_t("probe-blob", None);
    response.headers.set(PROBE_HEADER, "web-response");
    response
}

/// 返回带载荷类型的 `WebResponseOf<T>`：验证“自定义响应 + 有类型的返回值”。
#[web_api_get(path = "api/probe/response_of")]
pub async fn probe_response_of() -> WebResponseOf<ProbeQueryVO> {
    let mut response = WebResponseOf::from_t(probe_result(ProbeQueryPO::default()), None);
    response.headers.set(PROBE_HEADER, "web-response-of");
    response
}

/// 把查询参数整理成结果载荷。
fn probe_result(params: ProbeQueryPO) -> ProbeQueryVO {
    ProbeQueryVO {
        keyword: params.keyword,
        size: params.size.unwrap_or(DEFAULT_SIZE),
        desc: params.desc.unwrap_or_default(),
    }
}
