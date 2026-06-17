# Realtime RT Ray Tracing 采样流程

> 状态：当前实现事实总结。本文说明 realtime RT 主路径中 raygen / path tracing 的运行顺序、NEE、
> HDRI、自发光三角形、MIS、多 bounce 和当前未接入的 light 类型。

## 职责边界

realtime RT 的 path 积分状态集中在 raygen 侧推进。closest-hit 和 miss shader 只把一次追踪事件整理成
`RtSurfaceHit`；材质采样、直接光照、MIS、debug 累积和最终写出都由 raygen 引用的 helper 处理。

主要入口：

- `engine/shader/entry/realtime_rt/raygen.slang`：每像素初始化、path loop、`TraceRay` 顺序和最终输出。
- `engine/shader/lib/realtime_rt/raygen_direct_lighting.slangi`：HDRI NEE、自发光三角形 NEE、visibility 和 shade。
- `engine/shader/lib/realtime_rt/raygen_material.slangi`：BRDF / delta 材质采样和 BRDF PDF。
- `engine/shader/lib/realtime_rt/raygen_path_state.slangi`：radiance、throughput、上一段 BRDF PDF、Russian roulette。

## Raygen 主循环

每个像素先用像素坐标、frame id 和 `spp_idx` 初始化随机种子，再生成 camera ray。路径最大深度为
`max_depth = 16`；Vulkan ray tracing pipeline recursion 只覆盖 shader 调用栈，真正的路径递归由 raygen 手动循环推进。

```text
camera ray
  -> for depth in 0..max_depth
     -> TraceRay
     -> surface/debug 早退
     -> miss sky 或 hit surface
     -> NEE / BRDF / Russian roulette
     -> 下一跳 ray
```

GBuffer、DLSS depth、motion vector 和 RR 材质输入只描述 camera primary 第一次可见事件。raygen 用
`gbuffer_written` 保证每个像素只写一次 primary hit 或 primary miss，后续 bounce 不覆盖这些输出。

surface-only debug channel 会在每次 `TraceRay` 返回后先尝试早退。normal、object normal、base color 等通道只依赖
当前 surface/miss 事件，因此不继续跑后续 path，减少 debug 模式下的噪声和成本。

## 每个 Bounce 的决策顺序

一次 `TraceRay` 只返回一个事件。raygen 按固定顺序处理：

1. **Miss sky**：miss shader 采样 sky 贴图，把 sky radiance 写入 `surface.emissive`。raygen 调用
   `add_sky_miss` 累加贡献并终止路径。
2. **Primary output**：如果这是第一次 hit/miss，写出 GBuffer / DLSS 输入。
3. **Hit emissive surface**：调用 `add_emissive` 累加 hit emission 并终止路径。
4. **普通 surface 的 HDRI NEE**：非 delta surface 生成一个 HDRI light candidate，trace shadow ray，通过后 shade。
5. **普通 surface 的 emissive triangle NEE**：开关启用且表非空时，生成一个 emissive triangle candidate，trace shadow ray，通过后 shade。
6. **普通 surface 的 analytic light NEE**：开关启用且 point / spot / area 数量非 0 时，生成一个 analytic light candidate，trace shadow ray，通过后 shade。
7. **BRDF / 材质采样**：采样下一跳方向和 throughput，记录本次 `brdf_pdf` 供后续 sky/emissive hit MIS 使用。
8. **Russian roulette**：depth 小于 3 时保留完整路径；depth >= 3 时按 throughput 最大通道决定是否继续。
9. **继续下一跳**：用材质采样返回的 origin / direction 更新 ray。

```mermaid
flowchart TD
    Ray["当前 ray"] --> Trace["TraceRay"]
    Trace --> Miss{"miss sky?"}
    Miss -- yes --> Sky["add_sky_miss<br/>累加 sky 并结束"]
    Miss -- no --> Emissive{"hit emissive?"}
    Emissive -- yes --> HitLight["add_emissive<br/>累加 hit emission 并结束"]
    Emissive -- no --> HdriNee["HDRI NEE"]
    HdriNee --> EmissiveNee["Emissive Triangle NEE"]
    EmissiveNee --> AnalyticNee["Analytic Light NEE"]
    AnalyticNee --> Brdf["BRDF / delta 材质采样"]
    Brdf --> RR{"Russian roulette<br/>depth >= 3"}
    RR -- survive --> Next["下一 bounce"]
    RR -- terminate --> Output["输出累计 radiance"]
```

## NEE 通用候选契约

HDRI NEE、emissive triangle NEE 和 analytic light NEE 都先整理成 `RtLightCandidate`，再复用相同的 visibility
逻辑。HDRI / emissive 通过 `shade_candidate` 评估贡献；analytic light v1 不创建 TLAS 可命中的发光几何，因此通过
`shade_analytic_candidate` 使用固定 `MIS = 1`。
candidate 必须包含：

- `direction`：从 shading point 指向 light sample 的世界空间单位方向。
- `distance`：有限光源的实际距离；HDRI 使用大 TMax 表达无限远环境。
- `radiance`：light 侧 radiance，不包含 BRDF、cos、MIS 或 path throughput。
- `solid_angle_pdf`：light sample 的 solid-angle PDF。
- `shadow_ray`：与方向和距离配套的 visibility ray。

visibility 使用 inline `RayQuery`，只判断是否有遮挡，不运行 closest-hit。shade 公式是：

```text
contribution =
    path_throughput
  * light_radiance
  * BRDF_cos
  / light_pdf
  * MIS(light_pdf, brdf_pdf)
```

所有直接光 PDF 都必须是 solid angle 度量，这样才能和 BRDF PDF 做 MIS。

## HDRI / Sky 采样

HDRI 和 sky 的采样与 PDF 查询统一走 `EnvMap::sample` / `EnvMap::pdf`。

- 默认 `RtSkySamplingMode::Importance`：使用 `SkyBridge` 基于真实 sky CPU texture bytes 构建的 alias table。
- `RtSkySamplingMode::Uniform`：强制回退 uniform sphere，用于 A/B 对比。
- fallback sky：真实 sky GPU image 未 ready 前使用 1x1 均匀 distribution 和纯色 fallback 贴图。
- 无效 distribution：回退 uniform sphere，避免读取非法分布。

importance distribution 的 CPU 权重为：

```text
weight = luminance(texel) * solid_angle(texel)
```

shader 抽中 texel 后，会在该 texel 覆盖的 solid angle 内继续均匀采样方向。`EnvMap::pdf(dir)` 返回同一个
distribution 中该方向的 solid-angle PDF。`sky_brightness` 只在 shader 采样 sky 贴图后统一缩放 radiance；
它不改变 alias table 权重，也不改变 PDF。

HDRI NEE 与 BRDF sky miss MIS 必须读取同一个 `EnvMap::pdf`。更细的 HDRI alias table 和 PDF 语义见
[`hdri-sampling.md`](hdri-sampling.md)。

## 自发光三角形采样

自发光三角形由 `EmissiveLightTable` 在 prepare 阶段构建并上传：

- `emissive_triangle_lights`：world-space triangle record array。
- `emissive_light_alias_table`：只包含正面积、正 power record 的 NEE alias table。
- `instance_emissive_triangle_base_map`：instance-local submesh 到 record base 的映射，非 emissive 为 `UINT_MAX`。

emissive NEE 先在 alias table 中 O(1) 抽一个有效 record，再在三角形面积上均匀采样点。shader 插值 UV 后读取
`mat.emissive * base_color` 作为 radiance。面积 PDF 转换为方向 PDF：

```text
pdf_omega = select_pdf / area * distance^2 / abs(dot(light_normal, -light_dir))
```

shadow ray 使用 `TMax = distance - epsilon`，避免采样点所在的 light triangle 自遮挡。第一版沿用当前 hit emission
的双面语义，因此面积到方向 PDF 使用 `abs(dot(...))`。

BRDF 路径命中 emissive surface 时不走 alias table。closest-hit 已经写入 `instance_id`、`geometry_id` 和
`primitive_id`，raygen 通过直接寻址查询同一套 light PDF：

```text
base = instance_emissive_triangle_base_map[instance.geometry_indirect_idx + geometry_id]
light = emissive_triangle_lights[base + primitive_id]
```

更细的 record 字段、lookup 构建和 hit PDF 查询流程见 [`emissive-light-sampling.md`](emissive-light-sampling.md)。

## Analytic Light 采样

analytic light NEE 读取 GPU scene 中独立上传的 point / spot / area light buffer。Point / Spot 在 RT 中不是
delta light，而是半径固定为 `0.5` 的 analytic sphere surface emitter；Area 是 `center + half_u + half_v`
描述的矩形单面 emitter。

raygen 先在所有 analytic light 中均匀选择一个 light。Point / Spot 从 shading point 看到的 sphere visible cap
做 solid-angle 均匀采样，PDF 为 `select_pdf / solid_angle`；Spot 额外按 radians 表达的 inner / outer cone
计算 soft falloff。Area 在矩形面积上均匀采样，并把面积 PDF 转换为 solid-angle PDF，背面无效。

analytic light v1 没有 BRDF-hit 竞争估计器，因此 NEE shade 固定 `MIS = 1`。更细的 sphere / area 采样、PDF 和
调试边界见 [`analytic-light-sampling.md`](analytic-light-sampling.md)。

## BRDF、多 Bounce 与 Throughput

当前材质分类由 closest-hit 根据常量材质参数粗分：

- `EMISSIVE`：直接累加 `mat.emissive * base_color`，路径终止。
- `TRANSPARENT`：按 opaque 概率在折射和镜面反射之间选择，属于 delta path。
- `SPECULAR`：镜面反射，属于 delta path。
- `DIFFUSE`：按 roughness 在 cosine diffuse 与 GGX glossy 之间混合采样。

普通非 delta 材质采样后，throughput 使用完整混合 BRDF PDF：

```text
throughput *= BRDF_cos / brdf_pdf
```

raygen 同时保存 `prev_brdf_pdf` 和 `prev_is_delta`。下一次如果 miss sky 或 hit emissive，就可以判断是否需要
与对应 light PDF 做 MIS。delta path 不做 NEE，也不与 sky/emissive direct sampling 竞争；camera ray 或 delta 链路
直接看到 sky/emissive 时，保持完整直视/镜面语义。

Russian roulette 从 depth 3 开始启用。存活概率使用当前 throughput 的最大 RGB 通道，并 clamp 到 `[0.05, 0.95]`；
存活路径会除以该概率补偿 throughput，保持估计无偏。

## MIS 规则

当前使用 `Mis::power_heuristic`：

```text
w = pdf_a^2 / max(pdf_a^2 + pdf_b^2, epsilon)
```

MIS 出现在三类位置：

- **NEE shade**：light sample 通过 visibility 后，用 `MIS(light_pdf, brdf_pdf)` 调制直接光贡献。
- **BRDF sky miss**：非 delta BRDF 路径打到 sky 时，用 `MIS(prev_brdf_pdf, EnvMap::pdf(sky_dir))`。
- **BRDF emissive hit**：非 delta BRDF 路径打到 emissive surface 时，用
  `MIS(prev_brdf_pdf, emissive_hit_pdf(surface, light_dir))`。

如果是 camera ray 或上一段是 delta path，则不存在可竞争的 NEE 策略，sky / emissive 直接按当前 throughput 累加。
analytic light v1 不创建可命中的发光几何，因此 analytic NEE 固定 `MIS = 1`，不参与上述 BRDF-hit MIS。

## Debug 与当前边界

当前 RT debug channel 中，`NeeHdri` 只显示 HDRI NEE，`NeeEmissive` 只显示 emissive triangle NEE，`NeeAnalytic`
只显示 point / spot / area analytic NEE，`Emission` 显示 hit emissive contribution，`BrdfHdri` 显示 sky miss /
HDRI contribution，`NeeBounce0` 和 `NeeBounce1` 分别累加 depth 0 与 depth 1 的所有 NEE 贡献。

当前未接入 realtime RT raygen 的内容：

- HDRI 与 emissive triangle 还没有统一到 light-class PMF。
- ReSTIR DI、reservoir、SHARC / world-space radiance cache 尚未接入主路径。
- DLSS SR/RR 只消费 raygen 输出的 HDR、GBuffer、depth、motion vectors 和 RR 输入，不参与 light sampling、MIS、
  reservoir 或 radiance cache 状态。
