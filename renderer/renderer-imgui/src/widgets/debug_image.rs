use renderer_kit::debug_image::{DebugImageOption, DebugImageSelection};

/// 纯 CPU debug image 选择状态的 ImGui 视图，不保存 GPU image/view。
pub struct DebugImageSelectorView;

impl DebugImageSelectorView {
    pub fn build_window(ui: &imgui::Ui, selection: &mut DebugImageSelection, options: &[DebugImageOption]) {
        ui.window("Debug Images")
            .position([370.0, 10.0], imgui::Condition::FirstUseEver)
            .size([280.0, 90.0], imgui::Condition::FirstUseEver)
            .build(|| Self::build_contents(ui, selection, options));
    }

    pub fn build_contents(ui: &imgui::Ui, selection: &mut DebugImageSelection, options: &[DebugImageOption]) {
        selection.normalize_options(options);

        let mut visible = selection.is_visible();
        if ui.checkbox("Show", &mut visible) {
            selection.set_visible(visible);
        }

        let preview = selection
            .selected_option_id()
            .and_then(|id| options.iter().find(|option| option.id == id))
            .map(|option| option.label)
            .unwrap_or("None");

        if let Some(_combo) = ui.begin_combo("Image", preview) {
            for option in options {
                let selected = selection.selected_option_id() == Some(option.id);
                if ui.selectable_config(option.label).selected(selected).build() {
                    selection.set_selected_id(Some(option.id));
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
}
