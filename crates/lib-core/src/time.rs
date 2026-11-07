//! 时间取值：全项目统一使用 Unix 毫秒时间戳。

/// 当前 Unix 毫秒时间戳。
pub fn now_millis() -> i64 {
    framework_datetime::current_millis().unwrap_or_default()
}
