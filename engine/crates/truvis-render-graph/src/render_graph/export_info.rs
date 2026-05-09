use crate::render_graph::RgImageState;
use crate::render_graph::semaphore_info::RgSemaphoreInfo;

/// 导出资源信息
///
/// 描述资源在渲染图执行完成后的最终状态和同步需求。
#[derive(Clone, Debug)]
pub struct RgExportInfo {
    /// 资源的最终状态（layout, access, stage）
    pub final_state: RgImageState,
    /// 可选的信号 semaphore
    pub signal_semaphore: Option<RgSemaphoreInfo>,
}
