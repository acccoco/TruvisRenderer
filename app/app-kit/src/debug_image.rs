//! Debug image 的 CPU 选择状态与 ImGui 控件。
//!
//! 本模块只保存稳定 ID、显示开关和候选项标签，不接触 GPU image/view、RenderGraph handle
//! 或资源 layout。具体 pipeline 在 render 阶段根据选择 ID 解析当前 `FrameLabel` 的图像，
//! 从而避免 UI draw data 跨阶段携带 GPU 资源身份。

use imgui::Ui;

/// 可供用户选择的 debug image 元数据。
///
/// ID 必须在对应 pipeline 内稳定且唯一；label 只用于 UI 展示。该类型刻意不包含 extent、
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

/// App 持有的 debug image 显示选择器。
///
/// 该状态只在 render thread 的 ImGui update 与 render hook 之间传递稳定 ID。pipeline 切换后，
/// `build_contents` 会把失效选择归一化为新候选集的第一项；`visible` 关闭时保留选择，但
/// `selected_id` 返回 `None`，因此 present graph 不会导入或读取任何 debug image。
#[derive(Debug)]
pub struct DebugImageSelector {
    visible: bool,
    selected_id: Option<&'static str>,
}

impl Default for DebugImageSelector {
    fn default() -> Self {
        Self {
            visible: true,
            selected_id: None,
        }
    }
}

impl DebugImageSelector {
    /// 构建独立选择器窗口，供不包含 App 级 overlay 编排器的 sample 使用。
    pub fn build_window(&mut self, ui: &Ui, options: &[DebugImageOption]) {
        ui.window("Debug Images")
            .position([370.0, 10.0], imgui::Condition::FirstUseEver)
            .size([280.0, 90.0], imgui::Condition::FirstUseEver)
            .build(|| self.build_contents(ui, options));
    }

    /// 构建可嵌入其它 ImGui window/section 的选择器内容。
    pub fn build_contents(&mut self, ui: &Ui, options: &[DebugImageOption]) {
        self.normalize_options(options);
        ui.checkbox("Show", &mut self.visible);

        let preview = self
            .selected_id
            .and_then(|id| options.iter().find(|option| option.id == id))
            .map(|option| option.label)
            .unwrap_or("None");

        if let Some(_combo) = ui.begin_combo("Image", preview) {
            for option in options {
                let selected = self.selected_id == Some(option.id);
                if ui.selectable_config(option.label).selected(selected).build() {
                    self.selected_id = Some(option.id);
                }
                if selected {
                    ui.set_item_default_focus();
                }
            }
        }

        if options.is_empty() {
            ui.text("No debug image");
        }
    }

    /// 返回 render 阶段应显示的稳定 ID；关闭显示时不暴露已保存的选择。
    pub fn selected_id(&self) -> Option<&'static str> {
        self.visible.then_some(self.selected_id).flatten()
    }

    /// 同步当前 pipeline 的候选集，使模式切换不依赖选择器窗口是否实际构建。
    pub fn normalize_options(&mut self, options: &[DebugImageOption]) {
        let selected_is_valid = self.selected_id.is_some_and(|id| options.iter().any(|option| option.id == id));
        if !selected_is_valid {
            self.selected_id = options.first().map(|option| option.id);
        }
    }
}
