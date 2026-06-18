# Analytic Light Sampling

> 状态：当前实现事实总结。本文说明 realtime RT 中 point / spot / area analytic light 的 scene 同步、NEE
> 采样、PDF、MIS 边界和调试入口。

## 职责边界

analytic light NEE 是 realtime RT 统一 Light Candidate System 中的一个直接光 class。它只负责 point / spot /
area 三类显式 light 的 next-event estimation；统一入口选中 analytic class 后再调用这里的 class 内部 sampler。
它不接 reservoir，也不参与 SHARC 或其它间接光缓存。

`PointLight` 和 `SpotLight` 在 RT 中不是数学 delta light，而是半径固定为 `0.5` 的 analytic sphere surface
emitter。`AreaLight` 是由 `center + half_u + half_v` 描述的矩形单面 emitter，正面法线为
`normalize(cross(half_u, half_v))`。

## Scene 数据流

CPU scene 由 `SceneManager` 分别保存 point / spot / area 三类 light；`InstanceBridge::prepare_render_data`
在 prepare 阶段读取这些只读快照并写入 `RenderData`。`GpuScene` 为三类 light 各自维护独立 structured buffer，
scene root buffer 写入对应 device address 与 count。

三类 light 使用独立 buffer，而不是混入一个通用 light 数组。这样 shared ABI、上传容量、shader 下标和调试语义都保持
类型明确；统一 Light Candidate System 只在更高层选择 analytic class，不反向拆解混合 buffer。

## 采样契约

raygen 在非 delta surface 上调用 analytic NEE。采样先在三类 light 的总数量中均匀选择一个 light，离散选择概率为：

```text
select_pdf = 1 / (point_count + spot_count + area_count)
```

Point / Spot 使用从 shading point 看到的 sphere visible cap 做 solid-angle 均匀采样：

```text
radius = 0.5
solid_angle = 2 * pi * (1 - cos(theta_max))
pdf_omega = select_pdf / solid_angle
```

shader 对采样方向做 ray-sphere 交点求解，并用交点距离设置 shadow ray `TMax = distance - epsilon`。`PointLight.color`
和 `SpotLight.color` 在 RT 中解释为 sphere emitter radiance。

Spot 在 sphere candidate 基础上追加 cone falloff。`inner_angle` / `outer_angle` 的单位固定为 radians；shader 用
`dot(normalize(spot.dir), light_to_surface_dir)` 与 inner / outer cone 计算平滑衰减。cone 外 falloff 为 0，candidate
无效。

Area light 在矩形面积上均匀采样点，并把面积 PDF 转换为 solid-angle PDF：

```text
area = 4 * length(cross(half_u, half_v))
pdf_omega = select_pdf * distance^2 / (area * dot(area_normal, -light_dir))
```

背面 `dot(area_normal, -light_dir) <= 0` 无效。所有 candidate 仍复用统一 visibility ray；被 scene TLAS 遮挡时不贡献。

## MIS 与 Debug

analytic light v1 不创建 TLAS 可命中的发光几何，BRDF path 不会直接 hit 到这些 light，因此没有与 analytic NEE 竞争的
BRDF-hit 估计器。analytic shade 固定 `MIS = 1`，仍使用 `BRDF * cos / light_pdf` 评估贡献。HDRI / emissive triangle
继续使用各自已有的 MIS 规则。

`RtPipelineSettings.analytic_nee_enabled` 默认开启；关闭时统一入口不会把 analytic class 纳入候选来源，但不影响
HDRI NEE、emissive triangle NEE、GBuffer 或 DLSS 输入。`NeeAnalytic` debug channel 只显示统一 NEE 中抽到
analytic class 的贡献；`NeeBounce0` 和 `NeeBounce1` 会继续累计所有 NEE 类型。

DLSS SR/RR 只消费 raygen 输出的 HDR、GBuffer、depth、motion vectors 和 RR 输入，不参与 analytic light 采样、PDF、
visibility、debug channel 或后续 reservoir / cache 状态。
