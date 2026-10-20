use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 Unix 时间戳（毫秒）。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 文件修改时间转 Unix 毫秒。早于 1970 年的时间返回 `None`。
pub fn system_time_to_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}
