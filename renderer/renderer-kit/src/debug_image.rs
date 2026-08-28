//! Debug image 的纯 CPU 选择状态。
//!
//! 本模块只保存稳定 ID、显示开关和候选项标签，不接触 GPU image/view、RenderGraph handle
//! 或资源 layout。具体渲染子系统在 render 阶段根据选择 ID 解析当前 `FrameLabel` 的图像，
//! 从而避免 UI draw data 跨阶段携带 GPU 资源身份。

/// 可供用户选择的 debug image 元数据。
///
/// ID 必须在对应渲染子系统内稳定且唯一；label 只用于 UI 展示。该类型刻意不包含 extent、
/// format 或 GPU handle，避免 ImGui 状态参与图像生命周期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugImageOption {
    pub id: &'static str,
    pub label: &'static str,
}

impl DebugImageOption {
    pub const fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label }
    }
}

/// Renderer 持有的 debug image 显示选择状态。
///
/// 该状态只在 render thread 的 ImGui update 与 render hook 之间传递稳定 ID。渲染子系统切换后，
/// 候选集变化时可把失效选择归一化为第一项；`visible` 关闭时保留选择，但
/// `selected_id` 返回 `None`，因此 present graph 不会导入或读取任何 debug image。
#[derive(Debug)]
pub struct DebugImageSelection {
    visible: bool,
    selected_id: Option<&'static str>,
}

impl Default for DebugImageSelection {
    fn default() -> Self {
        Self {
            visible: true,
            selected_id: None,
        }
    }
}

impl DebugImageSelection {
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 返回已保存的选择；与显示开关无关，供界面显示当前候选项。
    pub fn selected_option_id(&self) -> Option<&'static str> {
        self.selected_id
    }

    pub fn set_selected_id(&mut self, selected_id: Option<&'static str>) {
        self.selected_id = selected_id;
    }

    /// 返回 render 阶段应显示的稳定 ID；关闭显示时不暴露已保存的选择。
    pub fn selected_id(&self) -> Option<&'static str> {
        self.visible.then_some(self.selected_id).flatten()
    }

    /// 同步当前渲染子系统的候选集，使模式切换不依赖选择器窗口是否实际构建。
    pub fn normalize_options(&mut self, options: &[DebugImageOption]) {
        let selected_is_valid = self.selected_id.is_some_and(|id| options.iter().any(|option| option.id == id));
        if !selected_is_valid {
            self.selected_id = options.first().map(|option| option.id);
        }
    }
}
