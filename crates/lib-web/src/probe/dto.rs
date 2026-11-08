//! 探针 DTO。
//!
//! 类型统一用 `auto_type` 标注：`camelCase` 字段名、`i64` 按字符串传输、`Option` 字段可省略，
//! 与后续正式 API DTO 的写法一致，同时可被 SDK 生成器导出。

use framework_proc_auto::auto_type;

/// 回声请求体。
#[auto_type]
pub struct ProbeEchoPO {
    /// 业务 ID：`i64` 按项目约定以字符串传输。
    pub id: i64,
    /// 必填文本，用于观察缺字段时的错误响应。
    pub text: String,
    /// 标签列表，用于观察 `Vec` 的传输形态。
    pub tags: Vec<String>,
    /// 可选开关，用于观察 `Option` 字段省略时的默认值。
    pub flag: Option<bool>,
}

/// 回声响应。
#[auto_type]
pub struct ProbeEchoVO {
    /// 回显的业务 ID。
    pub id: i64,
    /// 回显的文本。
    pub text: String,
    /// 标签数量。
    pub tag_count: i64,
    /// 请求中的开关，省略时为 `false`。
    pub flag: bool,
}

/// 查询参数，仅用于 `GET` 的 `Query<T>` 提取。
#[auto_type]
pub struct ProbeQueryPO {
    /// 关键字，可选。
    pub keyword: Option<String>,
    /// 数量，可选；`i64` 依赖 `DisplayFromStr` 从查询字符串解析。
    pub size: Option<i64>,
    /// 是否倒序，可选；用于观察 `bool` 查询参数的提取结果。
    pub desc: Option<bool>,
}

/// 查询结果。
#[auto_type]
pub struct ProbeQueryVO {
    /// 回显的关键字。
    pub keyword: Option<String>,
    /// 生效的数量。
    pub size: i64,
    /// 生效的排序方向。
    /// 生效的排序方向。
    pub desc: bool,
}

/// 健康检查结果（`api/probe/health`）。
#[auto_type]
pub struct ProbeHealthVO {
    /// 服务版本。
    pub version: String,
    /// API 协议版本。
    pub protocol_version: String,
    /// 服务启动时间（Unix 毫秒）。
    pub started_at_ms: i64,
    /// 已运行时长（毫秒）。
    pub uptime_ms: i64,
    /// 未结束的 Run 数量（`QUEUED` / `RUNNING`），供客户端 `status` 展示。
    pub running_run_count: i64,
}

/// 版本信息（`api/probe/version`）。
#[auto_type]
pub struct ProbeVersionVO {
    /// 服务版本。
    pub version: String,
    /// API 协议版本。
    pub protocol_version: String,
}
