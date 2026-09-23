use ash::vk;

use renderer_kit::camera::Camera;
use renderer_kit::input_state::InputState;
use renderer_kit::subsystem::SubsystemLifecycle;
use renderer_render_passes::effects::transform_gizmo::{
    NO_AXIS, TransformGizmoDrawData, TransformGizmoPass, TransformGizmoRgPass,
};
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgImageHandle};
use truvis_render_runtime::render_runtime::{
    RenderRuntimeInitCtx, RenderRuntimeRenderCtx, RenderRuntimeResizeCtx, RenderRuntimeShutdownCtx,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    fn axis_id(self) -> u32 {
        self.index() as u32
    }

    fn direction(self) -> glam::Vec3 {
        match self {
            Self::X => glam::Vec3::X,
            Self::Y => glam::Vec3::Y,
            Self::Z => glam::Vec3::Z,
        }
    }

    fn all() -> [Self; 3] {
        [Self::X, Self::Y, Self::Z]
    }
}

#[derive(Clone, Copy)]
struct GizmoRay {
    origin_ws: glam::Vec3,
    direction_ws: glam::Vec3,
}

#[derive(Clone, Copy)]
struct TransformGizmoFrame {
    origin_ws: glam::Vec3,
    axis_ws: [glam::Vec3; 3],
    axis_length_ws: f32,
    projected_axes: [Option<(glam::Vec2, glam::Vec2)>; 3],
}

impl TransformGizmoFrame {
    const AXIS_LENGTH_PX: f32 = 96.0;
    const PICK_RADIUS_PX: f32 = 10.0;

    fn new(transform: glam::Mat4, camera: &Camera, viewport: glam::Vec2) -> Option<Self> {
        if !viewport.is_finite() || viewport.x <= 0.0 || viewport.y <= 0.0 {
            return None;
        }

        let origin_ws = transform.w_axis.truncate();
        if !origin_ws.is_finite() {
            return None;
        }

        let distance = (origin_ws - camera.position).length().max(0.25);
        let fov = camera.fov_deg_vertical.to_radians();
        let visible_height = 2.0 * distance * (fov * 0.5).tan();
        let axis_length_ws = visible_height * Self::AXIS_LENGTH_PX / viewport.y;
        if !axis_length_ws.is_finite() || axis_length_ws <= 0.0 {
            return None;
        }

        let axis_ws = [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z];
        let projected_origin = camera.project_to_viewport(origin_ws, viewport)?;
        let projected_axes = Axis::all().map(|axis| {
            let endpoint = origin_ws + axis.direction() * axis_length_ws;
            Some((projected_origin, camera.project_to_viewport(endpoint, viewport)?))
        });

        Some(Self {
            origin_ws,
            axis_ws,
            axis_length_ws,
            projected_axes,
        })
    }

    fn screen_ray(camera: &Camera, screen_pos: glam::Vec2, viewport: glam::Vec2) -> Option<GizmoRay> {
        if !screen_pos.is_finite()
            || !viewport.is_finite()
            || viewport.x <= 0.0
            || viewport.y <= 0.0
            || screen_pos.x < 0.0
            || screen_pos.y < 0.0
            || screen_pos.x >= viewport.x
            || screen_pos.y >= viewport.y
        {
            return None;
        }

        let uv = screen_pos / viewport;
        let ndc = glam::vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let target_vs = camera.get_projection_matrix().inverse() * glam::vec4(ndc.x, ndc.y, 1.0, 1.0);
        let direction_vs = target_vs.truncate();
        if !direction_vs.is_finite() || direction_vs.length_squared() <= f32::EPSILON {
            return None;
        }

        let direction_ws = camera.get_view_matrix().inverse().transform_vector3(direction_vs.normalize());
        if !direction_ws.is_finite() || direction_ws.length_squared() <= f32::EPSILON {
            return None;
        }

        Some(GizmoRay {
            origin_ws: camera.position,
            direction_ws: direction_ws.normalize(),
        })
    }

    fn hit_axis(&self, mouse_pos: glam::Vec2) -> Option<Axis> {
        if !mouse_pos.is_finite() {
            return None;
        }

        let threshold_sq = Self::PICK_RADIUS_PX * Self::PICK_RADIUS_PX;
        Axis::all()
            .into_iter()
            .filter_map(|axis| {
                let (start, end) = self.projected_axes[axis.index()]?;
                let segment = end - start;
                let length_sq = segment.length_squared();
                let t = if length_sq > f32::EPSILON {
                    ((mouse_pos - start).dot(segment) / length_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let distance_sq = mouse_pos.distance_squared(start + segment * t);
                (distance_sq <= threshold_sq).then_some((axis, distance_sq))
            })
            .min_by(|(_, lhs), (_, rhs)| lhs.total_cmp(rhs))
            .map(|(axis, _)| axis)
    }

    fn axis_parameter(&self, axis: Axis, camera: &Camera, screen_pos: glam::Vec2, viewport: glam::Vec2) -> Option<f32> {
        self.axis_parameter_from_pivot(self.origin_ws, axis, camera, screen_pos, viewport)
    }

    fn axis_parameter_from_pivot(
        &self,
        pivot_ws: glam::Vec3,
        axis: Axis,
        camera: &Camera,
        screen_pos: glam::Vec2,
        viewport: glam::Vec2,
    ) -> Option<f32> {
        let ray = Self::screen_ray(camera, screen_pos, viewport)?;
        let axis_ws = self.axis_ws[axis.index()];
        let normal = Self::drag_plane_normal(axis_ws, camera)?;
        let denominator = ray.direction_ws.dot(normal);
        if denominator.abs() <= 0.00001 {
            return None;
        }
        let distance = (pivot_ws - ray.origin_ws).dot(normal) / denominator;
        if !distance.is_finite() {
            return None;
        }
        let intersection = ray.origin_ws + ray.direction_ws * distance;
        let parameter = (intersection - pivot_ws).dot(axis_ws);
        parameter.is_finite().then_some(parameter)
    }

    fn drag_plane_normal(axis_ws: glam::Vec3, camera: &Camera) -> Option<glam::Vec3> {
        let mut normal = camera.camera_forward() - axis_ws * camera.camera_forward().dot(axis_ws);
        if normal.length_squared() <= 0.00001 {
            normal = camera.camera_right() - axis_ws * camera.camera_right().dot(axis_ws);
        }
        if normal.length_squared() <= 0.00001 || !normal.is_finite() {
            return None;
        }
        Some(normal.normalize())
    }

    fn draw_data(&self, hovered_axis: Option<Axis>, active_axis: Option<Axis>) -> TransformGizmoDrawData {
        TransformGizmoDrawData {
            origin_ws: self.origin_ws,
            axes_ws: self.axis_ws,
            axis_length_ws: self.axis_length_ws,
            hovered_axis: hovered_axis.map_or(NO_AXIS, Axis::axis_id),
            active_axis: active_axis.map_or(NO_AXIS, Axis::axis_id),
        }
    }
}

struct AxisDragState {
    initial_transform: glam::Mat4,
    pivot_ws: glam::Vec3,
    axis_ws: glam::Vec3,
    start_parameter: f32,
}

#[derive(Default)]
struct TransformGizmoState {
    hovered_axis: Option<Axis>,
    active_axis: Option<Axis>,
    drag: Option<AxisDragState>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TransformGizmoUpdate {
    pub transform: Option<glam::Mat4>,
    pub drag_started: bool,
    pub drag_finished: bool,
    pub scene_pick_allowed: bool,
}

#[derive(Default)]
pub(crate) struct TransformGizmoSubsystem {
    /// 仅拥有 gizmo overlay pipeline；scene identity 和 CPU transform 由 TruvisRenderer 持有。
    resources: Option<TransformGizmoResources>,
    /// 当前选中对象的屏幕投影快照，只用于 hover 和绘制。
    frame: Option<TransformGizmoFrame>,
    /// hover/active/drag 是输入控件状态，不跨越到 GameWorld 或 RenderWorld。
    state: TransformGizmoState,
}

struct TransformGizmoResources {
    pass: TransformGizmoPass,
    present_format: vk::Format,
}

impl TransformGizmoSubsystem {
    pub(crate) fn update(
        &mut self,
        target_transform: Option<glam::Mat4>,
        camera: &Camera,
        input: &InputState,
        viewport: glam::Vec2,
    ) -> TransformGizmoUpdate {
        // update 阶段只计算输入结果；具体场景 mutation 由 TruvisRenderer 编排。
        let Some(transform) = target_transform.filter(|transform| transform.is_finite()) else {
            self.frame = None;
            return TransformGizmoUpdate {
                drag_finished: self.cancel_drag(),
                scene_pick_allowed: true,
                ..Default::default()
            };
        };

        let Some(frame) = TransformGizmoFrame::new(transform, camera, viewport) else {
            self.frame = None;
            return TransformGizmoUpdate {
                drag_finished: self.cancel_drag(),
                scene_pick_allowed: true,
                ..Default::default()
            };
        };
        self.frame = Some(frame);

        let mouse_position = input.mouse_position();
        let mouse_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
        if self.state.active_axis.is_some() {
            return self.update_drag(camera, input, viewport, mouse_pos);
        }

        let hit_position = if input.is_left_button_just_pressed() {
            let position = input.left_button_press_position;
            glam::vec2(position[0] as f32, position[1] as f32)
        } else {
            mouse_pos
        };
        self.state.hovered_axis = self.frame.as_ref().and_then(|frame| frame.hit_axis(hit_position));
        if input.is_left_button_just_pressed() {
            if let Some(axis) = self.state.hovered_axis {
                if let Some(start_parameter) =
                    self.frame.as_ref().and_then(|frame| frame.axis_parameter(axis, camera, hit_position, viewport))
                {
                    self.state.active_axis = Some(axis);
                    self.state.drag = Some(AxisDragState {
                        initial_transform: transform,
                        pivot_ws: transform.w_axis.truncate(),
                        axis_ws: axis.direction(),
                        start_parameter,
                    });
                    let mut update = self.update_drag(camera, input, viewport, mouse_pos);
                    update.drag_started = true;
                    return update;
                }
            }
        }

        TransformGizmoUpdate {
            scene_pick_allowed: true,
            ..Default::default()
        }
    }

    /// 松开所在帧仍提交最终位置；按下与松开合并到同帧时也只消费一次交互。
    fn update_drag(
        &mut self,
        camera: &Camera,
        input: &InputState,
        viewport: glam::Vec2,
        mouse_pos: glam::Vec2,
    ) -> TransformGizmoUpdate {
        let transform = self.state.active_axis.zip(self.state.drag.as_ref()).and_then(|(axis, drag)| {
            let parameter =
                self.frame.as_ref()?.axis_parameter_from_pivot(drag.pivot_ws, axis, camera, mouse_pos, viewport)?;
            let transform = glam::Mat4::from_translation(drag.axis_ws * (parameter - drag.start_parameter))
                * drag.initial_transform;
            transform.is_finite().then_some(transform)
        });
        if let Some(transform) = transform {
            self.refresh_frame(Some(transform), camera, viewport);
        }
        let finished =
            input.is_left_button_just_released() || !input.is_left_button_pressed() || self.state.drag.is_none();
        if finished {
            self.state = TransformGizmoState::default();
        }
        TransformGizmoUpdate {
            transform,
            drag_finished: finished,
            scene_pick_allowed: false,
            ..Default::default()
        }
    }

    /// 只刷新展示，不重复消费选择灯光的同一次鼠标按下。
    pub(crate) fn refresh_frame(&mut self, transform: Option<glam::Mat4>, camera: &Camera, viewport: glam::Vec2) {
        self.frame = transform.and_then(|value| TransformGizmoFrame::new(value, camera, viewport));
    }

    pub(crate) fn cancel_drag(&mut self) -> bool {
        let was_dragging = self.state.drag.is_some();
        self.state.active_axis = None;
        self.state.drag = None;
        self.state.hovered_axis = None;
        self.frame = None;
        was_dragging
    }

    pub(crate) fn contribute_passes<'a>(
        &'a self,
        graph: &mut RenderGraphBuilder<'a>,
        ctx: &'a RenderRuntimeRenderCtx<'a>,
        present_image: RgImageHandle,
        present_extent: vk::Extent2D,
    ) {
        let (Some(resources), Some(frame)) = (self.resources.as_ref(), self.frame.as_ref()) else {
            return;
        };
        graph.add_pass(
            "transform-gizmo",
            TransformGizmoRgPass {
                gizmo_pass: &resources.pass,
                record_ctx: ctx.record_ctx,
                present_image,
                extent: present_extent,
                draw_data: frame.draw_data(self.state.hovered_axis, self.state.active_axis),
            },
        );
    }
}

impl SubsystemLifecycle for TransformGizmoSubsystem {
    fn init(&mut self, ctx: &mut RenderRuntimeInitCtx<'_>) {
        let present_format = ctx.present.swapchain_image_info().image_format;
        self.resources = Some(TransformGizmoResources::new(ctx.device_ctx, present_format, ctx.shader_binding_system));
    }

    fn on_resize(&mut self, ctx: &mut RenderRuntimeResizeCtx<'_>) {
        let present_format = ctx.present.swapchain_image_info().image_format;
        match self.resources.as_mut() {
            Some(resources) => resources.rebuild_if_needed(ctx.device_ctx, present_format, ctx.shader_binding_system),
            None => {
                self.resources =
                    Some(TransformGizmoResources::new(ctx.device_ctx, present_format, ctx.shader_binding_system));
            }
        }
    }

    fn shutdown(&mut self, ctx: &mut RenderRuntimeShutdownCtx<'_>) {
        if let Some(resources) = self.resources.take() {
            resources.destroy(ctx.device_ctx);
        }
    }
}

impl TransformGizmoResources {
    fn new(
        device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>,
        present_format: vk::Format,
        shader_binding_system: &truvis_render_runtime::bindings::shader_binding_system::ShaderBindingSystem,
    ) -> Self {
        let pass = TransformGizmoPass::new(device_ctx, present_format, shader_binding_system.global_descriptor_sets());
        Self { pass, present_format }
    }

    fn rebuild_if_needed(
        &mut self,
        device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>,
        present_format: vk::Format,
        shader_binding_system: &truvis_render_runtime::bindings::shader_binding_system::ShaderBindingSystem,
    ) {
        if self.present_format == present_format {
            return;
        }
        let new_pass =
            TransformGizmoPass::new(device_ctx, present_format, shader_binding_system.global_descriptor_sets());
        let old_pass = std::mem::replace(&mut self.pass, new_pass);
        old_pass.destroy(device_ctx);
        self.present_format = present_format;
    }

    fn destroy(self, device_ctx: truvis_gfx::gfx::GfxDeviceCtx<'_>) {
        self.pass.destroy(device_ctx);
    }
}
