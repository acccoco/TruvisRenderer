use truvis_app_frame::input_event::InputEvent;
use truvis_app_frame::plugin_api::{Plugin, PluginRenderCtx};
use truvis_app_frame::render_app_api::{RenderAppHooks, RenderAppInitCtx};
use truvis_path::TruvisPath;
use truvis_render_foundation::render_view::RenderView;
use truvis_render_graph::render_graph::{RenderGraphBuilder, RgSemaphoreInfo};
use truvis_render_runtime::ray_cast::{RayCastRay, RayCastResult};
use truvis_render_runtime::render_runtime::{RenderRuntimeRayCastCtx, RenderRuntimeRenderCtx, RenderRuntimeUpdateCtx};
use truvis_shader_binding::gpu;
use truvis_world::{
    World, components::instance::Instance, components::material::SceneMaterialData, guid_new_type::SceneMeshHandle,
    procedural_mesh::ProceduralMeshKind,
};

use app_kit::camera::Camera;
use app_kit::camera_controller::CameraController;
use app_kit::gui_plugin::GuiPlugin;
use app_kit::input_state::InputManager;
use app_kit::overlay::FrameStatsOverlayData;
use app_kit::render_pipeline::RenderMode;
use app_kit::render_pipeline::common_settings::PathTracingCommonSettings;
use app_kit::render_pipeline::offline_render_graph::OfflinePipeline;
use app_kit::render_pipeline::rt_render_graph::RtPipeline;

use crate::overlay_ui::{
    DebugImageViewerData, PipelineControlsData, RaycastOverlayData, TruvisOverlayFrame, TruvisOverlayOptions,
    TruvisOverlayUi,
};

#[derive(Default)]
pub struct TruvisApp {
    gui: GuiPlugin,
    rt_pipeline: RtPipeline,
    offline_pipeline: OfflinePipeline,
    path_tracing_common_settings: PathTracingCommonSettings,
    render_mode: RenderMode,
    camera_controller: CameraController,
    input: InputManager,
    overlay_ui: TruvisOverlayUi,
    click_ray_cast_probe: ClickRayCastProbe,
}

#[derive(Clone, Copy)]
struct MaterialCubeSpec {
    name: &'static str,
    center: glam::Vec3,
    base_color: glam::Vec4,
    emissive: glam::Vec4,
    metallic: f32,
    roughness: f32,
    opaque: f32,
}

#[derive(Clone, Copy)]
struct EmissiveCubeMatrixConfig {
    /// 第一个 cube 的 world-space 中心点；整体平移矩阵时优先调这里。
    start_offset: glam::Vec3,
    /// 相邻 cube 中心点在 XYZ 三轴上的间距。
    spacing: glam::Vec3,
    /// 单个 cube 的等比缩放；程序化 cube 本身是边长 1 的单位模型。
    cube_scale: f32,
    /// XYZ 三轴实例数量；默认 20 * 1 * 10 = 200 个自发光 cube。
    counts: glam::UVec3,
}

#[derive(Clone, Copy)]
struct EmissiveCubePaletteSpec {
    name: &'static str,
    base_color: glam::Vec4,
    emissive: glam::Vec4,
}

const EMISSIVE_CUBE_MATRIX_CONFIG: EmissiveCubeMatrixConfig = EmissiveCubeMatrixConfig {
    start_offset: glam::Vec3::new(-800.0, 600.0, -425.0),
    spacing: glam::Vec3::new(75.0, 60.0, 90.0),
    cube_scale: 10.0,
    counts: glam::UVec3::new(20, 1, 10),
};

pub(crate) struct ClickRayCastProbe {
    total_time_s: f32,
    pending_ray: Option<RayCastRay>,
    pending_screen_pos: Option<glam::Vec2>,
    last_screen_pos: Option<glam::Vec2>,
    last_result: Option<RayCastResult>,
    last_error: Option<String>,
    last_cast_time_s: Option<f32>,
}

impl Default for ClickRayCastProbe {
    fn default() -> Self {
        Self {
            total_time_s: 0.0,
            pending_ray: None,
            pending_screen_pos: None,
            last_screen_pos: None,
            last_result: None,
            last_error: None,
            last_cast_time_s: None,
        }
    }
}

impl ClickRayCastProbe {
    fn update_time(&mut self, delta_time_s: f32) {
        self.total_time_s += delta_time_s.max(0.0);
    }

    fn request_cast(&mut self, screen_pos: glam::Vec2, ray: Option<RayCastRay>) {
        self.last_screen_pos = Some(screen_pos);
        match ray {
            Some(ray) => {
                self.pending_ray = Some(ray);
                self.pending_screen_pos = Some(screen_pos);
                self.last_error = None;
            }
            None => {
                self.pending_ray = None;
                self.pending_screen_pos = None;
                self.last_result = None;
                self.last_error = Some("click position is outside the viewport".to_owned());
                self.last_cast_time_s = None;
            }
        }
    }

    fn take_pending_cast(&mut self) -> Option<(RayCastRay, glam::Vec2)> {
        let ray = self.pending_ray.take()?;
        let screen_pos = self.pending_screen_pos.take().expect("pending raycast missing screen position");
        Some((ray, screen_pos))
    }

    fn finish_cast(&mut self, screen_pos: glam::Vec2, result: Result<RayCastResult, String>) {
        match result {
            Ok(result) => {
                self.last_result = Some(result);
                self.last_error = None;
            }
            Err(err) => {
                self.last_result = None;
                self.last_error = Some(err);
            }
        }
        self.last_screen_pos = Some(screen_pos);
        self.last_cast_time_s = Some(self.total_time_s);
    }

    pub(crate) fn has_pending_cast(&self) -> bool {
        self.pending_ray.is_some()
    }

    pub(crate) fn last_screen_pos(&self) -> Option<glam::Vec2> {
        self.last_screen_pos
    }

    pub(crate) fn last_cast_time_s(&self) -> Option<f32> {
        self.last_cast_time_s
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(crate) fn last_result(&self) -> Option<&RayCastResult> {
        self.last_result.as_ref()
    }
}

impl TruvisApp {
    pub fn overlay_options(&self) -> &TruvisOverlayOptions {
        self.overlay_ui.options()
    }

    pub fn overlay_options_mut(&mut self) -> &mut TruvisOverlayOptions {
        self.overlay_ui.options_mut()
    }

    fn request_model(world: &mut World, camera: &mut Camera) {
        camera.position = glam::vec3(270.0, 194.0, -64.0);
        camera.euler_yaw_deg = 90.0;
        camera.euler_pitch_deg = 0.0;

        world.register_point_light(gpu::light::PointLight {
            pos: glam::vec3(-800.0, 50.0, 400.0).into(),
            color: (glam::vec3(1.0, 0.0, 0.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::light::PointLight {
            pos: glam::vec3(-100.0, 50.0, 400.0).into(),
            color: (glam::vec3(0.0, 1.0, 0.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        world.register_point_light(gpu::light::PointLight {
            pos: glam::vec3(600.0, 50.0, 400.0).into(),
            color: (glam::vec3(0.0, 0.0, 1.0) * 5000.0).into(),
            _pos_padding: Default::default(),
            _color_padding: Default::default(),
        });
        // RT 中 SpotLight 是半径 0.5 的 sphere emitter 再叠加 cone falloff；
        // 主场景保留几盏显式 spot，方便观察 Analytic NEE 开关和 NeeAnalytic debug channel。
        world.register_spot_light(gpu::light::SpotLight {
            pos: glam::vec3(-450.0, 100.0, 400.0).into(),
            inner_angle: 30.0_f32.to_radians(),
            color: (glam::vec3(1.0, 1.0, 0.0) * 9000.0).into(),
            outer_angle: 60.0_f32.to_radians(),
            dir: glam::vec3(0.0, -1.0, 0.0).into(),
            _dir_padding: Default::default(),
        });
        world.register_spot_light(gpu::light::SpotLight {
            pos: glam::vec3(250.0, 100.0, 400.0).into(),
            inner_angle: 30.0_f32.to_radians(),
            color: (glam::vec3(0.0, 1.0, 1.0) * 9000.0).into(),
            outer_angle: 60.0_f32.to_radians(),
            dir: glam::vec3(0.0, -1.0, 0.0).into(),
            _dir_padding: Default::default(),
        });
        // AreaLight 的正面法线由 cross(half_u, half_v) 决定；这里使用 X/Z 方向半轴，
        // 让矩形灯法线朝 -Y，单面照向 Sponza 场景内部。
        world.register_area_light(gpu::light::AreaLight {
            center: glam::vec3(-100.0, 200.0, 400.0).into(),
            half_u: glam::vec3(70.0, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 18.0).into(),
            radiance: (glam::vec3(1.0, 0.16, 0.12) * 10.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });
        world.register_area_light(gpu::light::AreaLight {
            center: glam::vec3(600.0, 200.0, 400.0).into(),
            half_u: glam::vec3(26.0, 0.0, 0.0).into(),
            half_v: glam::vec3(0.0, 0.0, 26.0).into(),
            radiance: (glam::vec3(0.12, 0.16, 1.0) * 10.0).into(),
            _center_padding: Default::default(),
            _half_u_padding: Default::default(),
            _half_v_padding: Default::default(),
            _radiance_padding: Default::default(),
        });

        log::info!("start load sponza model");
        world.request_model_import(TruvisPath::assets_path("fbx/sponza/sponza.fbx"));
    }

    fn spawn_material_test_cubes(world: &mut World) {
        const MATERIAL_SOURCE: &str = "procedural://material-test-cubes";
        const CUBE_SCALE: f32 = 100.0;

        let cube_kind = ProceduralMeshKind::Cube;
        let cube_mesh = world.register_mesh(cube_kind.mesh_data()).expect("failed to register procedural cube mesh");
        let cube_y = 100.0;
        let cube_z = -25.0;
        let cube_specs = [
            MaterialCubeSpec {
                name: "glass",
                center: glam::vec3(-800.0, cube_y, cube_z),
                base_color: glam::vec4(0.65, 0.85, 1.0, 1.0),
                emissive: glam::Vec4::ZERO,
                metallic: 0.0,
                roughness: 0.0,
                opaque: 0.25,
            },
            MaterialCubeSpec {
                name: "mirror",
                center: glam::vec3(-450.0, cube_y, cube_z),
                base_color: glam::vec4(0.96, 0.96, 0.92, 1.0),
                emissive: glam::Vec4::ZERO,
                metallic: 1.0,
                roughness: 0.0,
                opaque: 1.0,
            },
            MaterialCubeSpec {
                name: "glossy-plastic",
                center: glam::vec3(-100.0, cube_y, cube_z),
                base_color: glam::vec4(0.95, 0.08, 0.18, 1.0),
                emissive: glam::Vec4::ZERO,
                metallic: 0.0,
                roughness: 0.18,
                opaque: 1.0,
            },
            MaterialCubeSpec {
                name: "rough-plastic",
                center: glam::vec3(250.0, cube_y, cube_z),
                base_color: glam::vec4(0.18, 0.95, 0.25, 1.0),
                emissive: glam::Vec4::ZERO,
                metallic: 0.0,
                roughness: 0.75,
                opaque: 1.0,
            },
            MaterialCubeSpec {
                name: "emissive-reference",
                center: glam::vec3(600.0, cube_y, cube_z),
                base_color: glam::vec4(1.0, 0.65, 0.18, 1.0),
                emissive: glam::vec4(4.0, 2.2, 0.5, 1.0),
                metallic: 0.0,
                roughness: 1.0,
                opaque: 1.0,
            },
        ];

        // cube 为单位模型，scale=100 且中心 y=100，使所有顶点落在给定场景范围内；
        // 这些材质参数刻意覆盖当前 shader 的透明、镜面、光泽/粗糙 diffuse 和 emissive 分支。
        for spec in cube_specs {
            let material = world
                .register_material(SceneMaterialData {
                    base_color: spec.base_color,
                    emissive: spec.emissive,
                    metallic: spec.metallic,
                    roughness: spec.roughness,
                    opaque: spec.opaque,
                    diffuse_texture: None,
                    normal_texture: None,
                    name: format!("material-test-cube-{}-{}", MATERIAL_SOURCE, spec.name),
                })
                .expect("failed to register material test cube material");

            world
                .register_instance(Instance {
                    mesh: cube_mesh,
                    materials: vec![material],
                    transform: glam::Mat4::from_scale_rotation_translation(
                        glam::Vec3::splat(CUBE_SCALE),
                        glam::Quat::IDENTITY,
                        spec.center,
                    ),
                })
                .expect("failed to register material test cube instance");
        }

        Self::spawn_emissive_cube_matrix(world, cube_mesh, EMISSIVE_CUBE_MATRIX_CONFIG);
    }

    fn spawn_emissive_cube_matrix(world: &mut World, cube_mesh: SceneMeshHandle, config: EmissiveCubeMatrixConfig) {
        let palette_specs = [
            EmissiveCubePaletteSpec {
                name: "warm-amber",
                base_color: glam::vec4(1.0, 0.72, 0.32, 1.0),
                emissive: glam::vec4(4.8, 2.7, 0.8, 1.0) * 5.0,
            },
            EmissiveCubePaletteSpec {
                name: "rose",
                base_color: glam::vec4(1.0, 0.36, 0.54, 1.0),
                emissive: glam::vec4(4.2, 0.9, 1.8, 1.0) * 5.0,
            },
            EmissiveCubePaletteSpec {
                name: "cyan",
                base_color: glam::vec4(0.42, 0.95, 1.0, 1.0),
                emissive: glam::vec4(1.2, 3.8, 4.8, 1.0) * 5.0,
            },
            EmissiveCubePaletteSpec {
                name: "lime",
                base_color: glam::vec4(0.54, 1.0, 0.38, 1.0),
                emissive: glam::vec4(1.4, 4.5, 1.0, 1.0) * 5.0,
            },
            EmissiveCubePaletteSpec {
                name: "violet",
                base_color: glam::vec4(0.72, 0.48, 1.0, 1.0),
                emissive: glam::vec4(2.2, 1.2, 4.8, 1.0) * 5.0,
            },
        ];
        let emissive_materials = palette_specs
            .into_iter()
            .map(|spec| {
                world
                    .register_material(SceneMaterialData {
                        base_color: spec.base_color,
                        emissive: spec.emissive,
                        metallic: 0.0,
                        roughness: 1.0,
                        opaque: 1.0,
                        diffuse_texture: None,
                        normal_texture: None,
                        name: format!("emissive-cube-matrix-{}", spec.name),
                    })
                    .expect("failed to register emissive cube material")
            })
            .collect::<Vec<_>>();

        let mut cube_index = 0usize;
        // 配置使用“第一个 cube 中心点 + XYZ 间距”的语义，方便在场景中手工平移和拉开矩阵。
        // 自发光 cube 仍只是普通 material emission：当前 RT 路径只在命中 surface 时累加 emission，
        // 不会把这些 cube 注册成 emissive triangle NEE 光源。
        for y in 0..config.counts.y {
            for z in 0..config.counts.z {
                for x in 0..config.counts.x {
                    let center = config.start_offset
                        + glam::vec3(
                            x as f32 * config.spacing.x,
                            y as f32 * config.spacing.y,
                            z as f32 * config.spacing.z,
                        );
                    let material = emissive_materials[cube_index % emissive_materials.len()];

                    world
                        .register_instance(Instance {
                            mesh: cube_mesh,
                            materials: vec![material],
                            transform: glam::Mat4::from_scale_rotation_translation(
                                glam::Vec3::splat(config.cube_scale),
                                glam::Quat::IDENTITY,
                                center,
                            ),
                        })
                        .expect("failed to register emissive cube instance");

                    cube_index += 1;
                }
            }
        }
    }

    fn cast_single_ray(ctx: &mut RenderRuntimeRayCastCtx<'_>, ray: RayCastRay) -> Result<RayCastResult, String> {
        ctx.cast_sync(std::slice::from_ref(&ray))
            .map_err(|err| err.to_string())
            .map(|mut results| results.pop().expect("single raycast result missing"))
    }
}

impl RenderAppHooks for TruvisApp {
    fn init(&mut self, ctx: &mut RenderAppInitCtx<'_>) {
        self.render_mode = RenderMode::initial_from_env();
        self.gui.set_hidpi_factor(ctx.scale_factor);
        self.gui.set_display_size(ctx.window_size);

        Self::spawn_material_test_cubes(&mut *ctx.runtime.world);
        Self::request_model(&mut *ctx.runtime.world, self.camera_controller.camera_mut());
    }

    fn visit_plugins_mut(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.rt_pipeline);
        visit(&mut self.offline_pipeline);
        visit(&mut self.gui);
    }

    fn visit_plugins_mut_rev(&mut self, visit: &mut dyn FnMut(&mut dyn Plugin)) {
        visit(&mut self.gui);
        visit(&mut self.offline_pipeline);
        visit(&mut self.rt_pipeline);
    }

    fn on_input(&mut self, events: &[InputEvent]) {
        self.input.begin_frame();
        for event in events {
            if !self.gui.on_input(event) {
                self.input.process_event(event);
            }
        }
    }

    fn update(&mut self, ctx: &mut RenderRuntimeUpdateCtx) {
        self.click_ray_cast_probe.update_time(ctx.delta_time_s);

        let delta = std::time::Duration::from_secs_f32(ctx.delta_time_s);
        let viewport_size = glam::vec2(ctx.swapchain_extent.width as f32, ctx.swapchain_extent.height as f32);
        self.camera_controller.update_with_wheel_zoom(self.input.state(), viewport_size, delta);

        if self.input.state().is_left_button_just_pressed() {
            let mouse_position = self.input.state().mouse_position();
            let screen_pos = glam::vec2(mouse_position[0] as f32, mouse_position[1] as f32);
            let ray = self.camera_controller.make_screen_raycast(mouse_position, viewport_size);
            self.click_ray_cast_probe.request_cast(screen_pos, ray);
        }

        self.gui.begin_frame(delta);
        {
            let ui = self.gui.ui();
            let offline_sample_count = self.offline_pipeline.sample_count();
            let frame = TruvisOverlayFrame {
                ui,
                stats: FrameStatsOverlayData {
                    camera: self.camera_controller.camera(),
                    swapchain_extent: ctx.swapchain_extent,
                    accum_frames_num: ctx.view_accum.accum_frames_num(),
                    delta_time_s: ctx.delta_time_s,
                },
                pipeline: PipelineControlsData {
                    render_mode: &mut self.render_mode,
                    dlss_options: ctx.dlss_options,
                    common_settings: Some(&mut self.path_tracing_common_settings),
                    rt_settings: Some(self.rt_pipeline.settings_mut()),
                    offline_settings: Some(self.offline_pipeline.settings_mut()),
                    offline_sample_count: Some(offline_sample_count),
                },
                raycast: RaycastOverlayData {
                    probe: &self.click_ray_cast_probe,
                    world: ctx.world,
                },
                debug_images: DebugImageViewerData { gui: &self.gui },
            };
            self.overlay_ui.build(frame);
        }
        self.gui.end_frame();
    }

    fn after_prepare(&mut self, ctx: &mut RenderRuntimeRayCastCtx<'_>) {
        if let Some(request) = self.camera_controller.take_pending_pivot_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_pivot_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_drag_pan_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_drag_pan_raycast(request, result);
        }

        if let Some(request) = self.camera_controller.take_pending_wheel_zoom_raycast() {
            let result = Self::cast_single_ray(ctx, request.ray);
            self.camera_controller.finish_wheel_zoom_raycast(request, result);
        }

        if let Some((ray, screen_pos)) = self.click_ray_cast_probe.take_pending_cast() {
            let result = Self::cast_single_ray(ctx, ray);
            self.click_ray_cast_probe.finish_cast(screen_pos, result);
        }
    }

    fn render(&mut self, ctx: &RenderRuntimeRenderCtx) {
        let plugin_ctx = PluginRenderCtx {
            device_ctx: ctx.device_ctx,
            resource_ctx: ctx.resource_ctx,
            queue_ctx: ctx.queue_ctx,
            device_info_ctx: ctx.device_info_ctx,
            record_ctx: ctx.record_ctx,
            render_scene: ctx.render_scene,
            present: ctx.present,
            timeline: ctx.timeline,
        };
        let frame_label = ctx.record_ctx.frame_timing.frame_label();
        let frame_id = ctx.record_ctx.frame_timing.frame_id();

        // 离线累计失效由 App 在每帧 render 前统一判断：相机、场景和离线设置都已经进入
        // 本帧确定状态，pipeline 只保存历史签名并在变化时清空自己的 accum_image。
        self.offline_pipeline.update_accum_signature(
            self.camera_controller.camera().render_view().accum_signature(),
            ctx.render_scene.accum_signature(frame_label),
            &self.path_tracing_common_settings,
        );

        self.gui.begin_debug_image_frame();
        // debug image 的来源跟随当前模式选择。App 只把所选 pipeline 的图像交给 GUI，
        // 图像生命周期、layout 导出和 bindless 注册仍由各 pipeline 自己维护。
        let debug_images = match self.render_mode {
            RenderMode::Realtime => self.rt_pipeline.collect_debug_images(frame_label, *ctx.record_ctx.dlss_options),
            RenderMode::Offline => self.offline_pipeline.collect_debug_images(frame_label),
        };
        for debug_image in debug_images {
            self.gui.register_debug_image(debug_image);
        }
        self.gui.prepare_render_data(&plugin_ctx);

        // App 持有实时/离线模式选择；具体 pipeline 只负责向 RenderGraph 贡献自己的 compute subgraph。
        // 两条分支都生成同一队列上的第一段 submit，保证后续 present graph 可按统一顺序消费结果。
        let compute_submit = match self.render_mode {
            RenderMode::Realtime => {
                let mut graph = RenderGraphBuilder::new();
                self.rt_pipeline.contribute_compute_passes(&mut graph, &plugin_ctx, &self.path_tracing_common_settings);
                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_RT_COMPUTE_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_RT_COMPUTE_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.rt_pipeline.compute_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-compute-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
            RenderMode::Offline => {
                let mut graph = RenderGraphBuilder::new();
                self.offline_pipeline.contribute_compute_passes(
                    &mut graph,
                    &plugin_ctx,
                    &self.path_tracing_common_settings,
                );
                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_OFFLINE_COMPUTE_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_OFFLINE_COMPUTE_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.offline_pipeline.compute_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "offline-compute-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
        };

        // present subgraph 同样按模式委派给对应 pipeline；GUI 与 debug viewer 只读取该分支导出的
        // render target，避免 realtime/offline 两套资源在同一帧互相暴露状态。
        let present_submit = match self.render_mode {
            RenderMode::Realtime => {
                let mut graph = RenderGraphBuilder::new();
                graph.signal_semaphore(RgSemaphoreInfo::timeline(
                    ctx.timeline.handle(),
                    ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                    frame_id,
                ));
                let present_targets = self.rt_pipeline.contribute_present_passes(
                    &mut graph,
                    &plugin_ctx,
                    &self.path_tracing_common_settings,
                );
                let debug_graph_entries = present_targets.debug_graph_entries();
                self.gui.contribute_passes(
                    &mut graph,
                    &plugin_ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                    &debug_graph_entries,
                );

                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_RT_PRESENT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_RT_PRESENT_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.rt_pipeline.present_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "rt-present-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
            RenderMode::Offline => {
                let mut graph = RenderGraphBuilder::new();
                graph.signal_semaphore(RgSemaphoreInfo::timeline(
                    ctx.timeline.handle(),
                    ash::vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                    frame_id,
                ));
                let present_targets = self.offline_pipeline.contribute_present_passes(
                    &mut graph,
                    &plugin_ctx,
                    &self.path_tracing_common_settings,
                );
                let debug_graph_entries = present_targets.debug_graph_entries();
                self.gui.contribute_passes(
                    &mut graph,
                    &plugin_ctx,
                    present_targets.present_image,
                    ctx.present.swapchain_image_info().image_extent,
                    &debug_graph_entries,
                );

                let compiled_graph = graph.compile();
                if log::log_enabled!(log::Level::Debug) {
                    static PRINT_OFFLINE_PRESENT_DEBUG_INFO: std::sync::Once = std::sync::Once::new();
                    PRINT_OFFLINE_PRESENT_DEBUG_INFO.call_once(|| {
                        compiled_graph.print_execution_plan();
                    });
                }

                let cmd = self.offline_pipeline.present_cmd(frame_label);
                cmd.begin(ash::vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT, "offline-present-graph");
                compiled_graph.execute(cmd, ctx.record_ctx.gfx_resource_manager);
                cmd.end();
                compiled_graph.build_submit_info(std::slice::from_ref(cmd))
            }
        };

        // 两种模式都保持 compute -> present 的提交顺序。timeline signal 放在 present graph，
        // 因此上层 runtime 只需要等待同一个 frame_id 即可观察最终 swapchain 写入完成。
        ctx.queue_ctx.gfx_queue().submit(vec![compute_submit, present_submit], None);
    }

    fn render_view(&self) -> RenderView {
        self.camera_controller.camera().render_view()
    }
}
