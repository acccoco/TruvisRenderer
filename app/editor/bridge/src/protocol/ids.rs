use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Server 为当前 WebSocket connection 分配的内部路由 ID。
///
/// ClientId 不进入 Web 请求，也不表示场景对象；连接关闭后即可失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

/// Web 为一次 Query / Command 分配的请求 ID。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// JSON 中无损表达 `SceneStore` 内部 `u64` version 的十进制字符串。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SceneVersion(pub String);

impl SceneVersion {
    pub fn from_u64(value: u64) -> Self {
        Self(value.to_string())
    }

    pub fn parse(&self) -> Result<u64, std::num::ParseIntError> {
        self.0.parse()
    }
}

/// 当前 Truvis session 内的 instance opaque ID。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct InstanceId(pub String);

impl InstanceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// 当前 Truvis session 内的 mesh opaque ID。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct MeshId(pub String);

impl MeshId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// 当前 Truvis session 内的 material opaque ID。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct MaterialId(pub String);

impl MaterialId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// 当前 Truvis session 内的 texture opaque ID。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct TextureId(pub String);

impl TextureId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
