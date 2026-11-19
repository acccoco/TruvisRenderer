# renderer-dlss

`Dlss` 是 Renderer 持有的单主视图 DLSS owner。它根据用户配置和设备能力派生 render extent，持有时域历史、
SR 的固定曝光图像与 SR/RR 共用的 HDR 输出，并在已有 RenderGraph 内录制 Streamline evaluate。
Streamline 初始化由 Engine/Gfx 完成，DLSS 入口在其存活期间由同一 RenderThread 串行调用。

- 调用方传入 `DlssOptions`、motion vector 无效值与现有 phase ctx。初始化或 resize 先确定 render extent，再重建相关目标。
- `frame_input` 生成 prepare 所需的上一主视图与采样 jitter。RR options 的相机矩阵取自同帧 view。
- `DlssFrame::add_pass` 在输入生产完成后加入 SR 或 RR，返回线性 HDR 及尺寸。输入的图像 handle 来自当前 graph，
  owner 必须保留图像直到 GPU 完成。SR/RR 共用 [0,1] device depth 与含相机运动的像素 motion vectors，
  motion vectors 不含 jitter。RR 还需要同一可见表面的 diffuse/specular albedo、packed normal/roughness 和反射命中距离。
- graph 执行前调用 `begin_recording`。compute 录制后的帧状态供 debug preview 使用，queue submit 正常返回后调用
  `finish_rendered_frame`。无场景清屏不推进主视图历史，单次 evaluate 成功也不消费 pending reset。
- mode/resize/shutdown 在 GPU idle 边界释放旧 SDK feature，随后才允许销毁其引用的图像。shutdown 使用已完成 GPU 等待的
  `RenderRuntimeShutdownCtx`。当前固定使用 Streamline viewport 0。

帧阶段与时域约束见 [Render Configuration 与 Temporal State](../../docs/design/render-configuration-and-temporal-state.md)。
