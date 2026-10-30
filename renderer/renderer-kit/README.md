# renderer-kit

`renderer-kit` 是 Renderer 域的基础能力层，只保存生命周期契约、相机/输入和与具体界面、渲染实现无关的纯 CPU 状态。

## 主要职责

- `SubsystemLifecycle`：只约束需要显式资源管理的子系统 `init` / 可选 `on_resize` / `shutdown`；具体 Renderer 静态持有并直接调用。
- `SubsystemRenderCtx`：只转发子系统实际消费的 resource/queue/record、只读 scene 和 present；队列提交与 timeline 信号编排仍归 Renderer。resource/queue 可用于 ImGui 上传，不是 GPU 只读权限。
- `Camera` / `CameraController` / `InputManager` / `InputState`：Renderer-owned 相机与输入状态，生成 runtime prepare 所需的 `RenderView` 快照。
- `DebugImageOption` / `DebugImageSelection`：稳定候选 ID、标签、显示开关与当前选择；不依赖 ImGui，不保存 GPU image/view 或 RenderGraph handle。

## 所有权与依赖

相机位置、near、raycast 距离、平移与滚轮锚点距离均使用 m；默认 near 为 0.01 m，
移动速度为 3.2 m/s，screen ray 最大距离为 1000 m。FOV/旋转角、像素坐标和滚轮比例为各自原有量纲。
投影仍为右手系、Y-Up、NDC depth [0,1] 的 infinite perspective；near 不等同于 RT secondary ray epsilon，
screen ray 的 TMin 也不复用 near。
`Camera::project_to_viewport`、`project_segment_to_viewport` 与 `screen_ray_direction` 共用无 jitter 矩阵和原生窗口局部物理像素。
线段先在齐次空间裁剪 near plane，再投影；无效投影返回 None。灯光与 gizmo 复用投影，相机导航与 gizmo 复用反投影。
屏幕图形构造、选中状态与命中优先级仍归具体 Renderer。
视图、投影反投影和局部移动共用 YXZ 姿态（含 roll）；Q/E 仍沿世界 Y 轴移动。
Camera 的程序配置必须保持位置/角度有限、aspect/near 为有限正数、垂直 FOV 在 (0,180) 度内；
矩阵生成及 RenderView 发布检查有限性（含逆矩阵），非法配置明确失败，不替换成默认相机。
Controller 对非法 viewport、不可表示为有限 f32 的鼠标位置/位移拒绝整次更新并取消导航，
保留原相机；该输入拒绝不等同于校验任意程序对 Camera public 字段的写入。

InputManager 的 pressed 表示最后状态，just_pressed/just_released 表示本帧曾发生的
真实边沿，保留到 begin_frame 或显式 reset。重复 Pressed/Released 不撤销已记录边沿，
重复按下也不改写按下坐标；同帧真实释放后再按下仍记录新的按下位置。

InputManager reset 不会隐式取消 CameraController；拥有两者的 Renderer 在取消交互时
同时调用 Controller::cancel_interaction，撤销 pivot/pan/wheel 查询、锚点和中键模式，
保留相机位置与朝向。否则未执行 after_prepare 的旧查询可能在下一帧继续生效。
普通 `CameraController::update` 仅启用键盘/右键控制并清除导航查询与锚点；
`update_with_wheel_zoom` 启用完整中键、Shift+中键和滚轮控制，调用者必须在 after_prepare
消费三类查询。Triangle/ShaderToy 使用前者，Truvis 使用后者。

- 不依赖 `imgui`、`renderer-render-passes`、`truvis-renderer-shader-binding` 或任何具体渲染 subsystem。
- 不拥有具体 Renderer state、GUI backend、render controls、GPU pass、realtime/offline targets，也不提供可执行入口。
- `DebugImageSelection::selected_id()` 在可见性关闭时返回 `None`；`normalize_options()` 在模式切换或窗口隐藏时仍可由 Renderer 显式调用。
- `renderer-imgui` 持有 ImGui context、字体、backend 与选择器视图；`renderer-rendering` 持有具体 rendering subsystem 及其长期 GPU 资源。
- 纯 UI overlay、camera 和 input 不需要实现 `SubsystemLifecycle`；生命周期 trait 不引入 registry、visitor、动态分发或自动调度。
- 相机状态属于 Renderer；runtime 只消费 `RenderView`，不依赖相机控制策略。中键 pivot、Shift+中键拖拽与滚轮锚点状态留在 `CameraController`，同步 raycast 仍由具体 Renderer 在 `after_prepare` 阶段执行。

跨 crate 所有权和帧阶段见 [`docs/design/runtime-phase-contract.md`](../../docs/design/runtime-phase-contract.md)。
