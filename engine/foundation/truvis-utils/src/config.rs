/// 通用配置解析工具。
///
/// 该类型只聚合无状态 helper，不持有配置源或运行时状态。具体环境变量的读取、
/// 默认值和错误日志仍由各自 owner 负责，避免基础工具层反向承载业务策略。
pub struct ConfigUtils;

impl ConfigUtils {
    /// 解析宽容形式的 bool 配置值。
    ///
    /// 这里只解释字符串本身，不读取环境变量，也不决定非法值的 fallback 策略。
    /// 调用方应根据自己的配置语义记录日志或应用默认值。
    pub fn parse_bool_env(value: &str) -> Option<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" | "enable" | "enabled" => Some(true),
            "0" | "false" | "off" | "no" | "disable" | "disabled" => Some(false),
            _ => None,
        }
    }
}
