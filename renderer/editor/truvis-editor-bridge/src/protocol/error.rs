use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Editor 协议稳定错误码。
///
/// Web 应根据 code 决定可恢复行为，`message` 只用于向用户展示和诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum EditorErrorCode {
    InvalidRequest,
    Busy,
    Timeout,
    NotFound,
    StaleObject,
    Conflict,
    Internal,
}

/// Query / Command 失败时返回的可序列化错误。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditorError {
    pub code: EditorErrorCode,
    pub message: String,
}

impl EditorError {
    pub fn new(code: EditorErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
