# ReSTIR DI 算法直觉与关键不变量

> 状态：算法理解与实现参考。本文不记录当前代码完成度；当前实现事实仍以
> [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md)、[`docs/summaries/`](../summaries/) 和代码为准。

本文用尽量直观的方式解释 ReSTIR DI。这里假设读者已经理解 HDRI、自发光三角形、analytic light 等光源采样算法，
也已经理解项目里的统一 Light Candidate System。ReSTIR DI 不是新的光源采样器，而是一个直接光样本复用算法。

## 1. 核心想法

普通 next-event estimation 通常是：

```text
当前 surface
  -> 从统一光源采样器抽 1 个 light sample
  -> 算 PDF、BRDF、cos、MIS
  -> 打 shadow ray
  -> 得到 direct lighting
```

每个像素每帧只抽一个样本时，高频 HDRI、多自发光三角形、小面积灯和高亮 analytic light 都容易产生噪声。
ReSTIR DI 的目标是：

```text
当前像素自己只抽 1 个样本，
但可以复用上一帧和邻居像素抽到的样本；
最终仍然只 shade 1 个样本，
只是这个样本更可能是重要样本。
```

因此 ReSTIR 的重点不是“抽一个新光源”，而是“从自己、历史和邻域提供的多个候选里，用公平的加权规则只保留一个代表样本”。

## 2. Reservoir 保存什么

Reservoir 可以理解成一个压缩样本池。它可以看过很多候选，但最终只保存一个样本：

```text
reservoir:
  selected_sample      当前选中的 light sample
  weight_sum           看过的所有候选权重总和
  M                    看过多少个候选
  target(selected)     选中样本在当前 surface 上的目标值
```

它的合并规则很简单。假设已有权重总和是 `old_weight_sum`，新候选权重是 `w`：

```text
new_weight_sum = old_weight_sum + w

以概率 w / new_weight_sum:
  selected_sample = new_sample
否则:
  保留旧 selected_sample
```

这样 reservoir 的内存始终是 O(1)，但权重大的候选更容易被留下。

## 3. Target 与权重

ReSTIR 需要一个标量衡量候选样本对当前 surface 的重要性。本项目使用完整 RGB 贡献的最大通道作为
reservoir target：

```text
target = visibility_current_surface * max_rgb(light_radiance * BRDF * cos * MIS)
```

这里不除以 light PDF。可以把它理解为：

```text
这个 light sample 在当前 surface 上可见时，对任意颜色通道最多有多重要；不可见时 target 为 0。
```

使用最大通道而不是视觉亮度，是为了让高饱和红/绿/蓝光源按最强通道参与 reservoir 竞争。只要 final
shade 使用同一个 target 计算 `W`，这仍是同一个 ReSTIR estimator，不改变 NEE / MIS / ReSTIR / RR 技术路线。

initial reservoir 由当前像素自己抽出的 8 个独立 proposal 生成：

```text
for each proposal:
  sample = unified_light_sampler(surface)
  target = evaluate_target(sample, surface)
  pdf = sample.solid_angle_pdf

  candidate_weight = target / pdf
  reservoir_combine(candidate_weight, target)

M = 8
```

如果只运行 `InitialOnly`，最终 shade 权重为：

```text
W = weight_sum / (target(selected_sample) * M)
```

当 proposal 数为 1 时可直接看出它退化为普通 NEE：

```text
W = (target / pdf) / target = 1 / pdf
```

最终贡献就回到普通直接光估计：

```text
contribution = light_radiance * BRDF * cos * MIS / pdf
```

这也是 `InitialOnly` 应该和普通 unified NEE 能量一致的原因。

## 4. Temporal Reuse

Temporal reuse 复用上一帧同一表面的 reservoir。它不能直接读取上一帧同屏幕坐标，而是要通过 motion vector 回投：

```text
current pixel
  -> motion vector
  -> previous pixel
  -> previous reservoir
```

读取历史后必须做 surface rejection：

```text
previous surface 是否有效？
depth / world position 是否接近？
normal 是否接近？
roughness / base_color / metallic 等材质签名是否接近？
光源集合和 sky/emissive/analytic 版本是否仍匹配？
```

通过检查后，历史 reservoir 才能参与当前像素合并。关键点是：历史 reservoir 代表过去看过的 `M` 个候选，
不能当作一个普通单样本随便塞进当前 reservoir。复用时要在当前 surface 上重新计算历史选中样本的 target：

```text
history_weight =
    history.weight_sum
  * target_current_surface(history.selected_sample)
  / target_history_surface(history.selected_sample)
```

直觉上，这是在问：

```text
这个历史样本过去在旧 surface 上很重要；
它现在对当前 surface 是否仍然重要？
```

本项目的 temporal history 读取上一帧 temporal reservoir，而不是上一帧 spatial/final reservoir。Spatial reuse
只服务当前帧出图；如果把 spatial final 再喂回 temporal，邻域样本会跨帧反馈，`M` 和相关性一起膨胀，最终表现为
大块低频彩色噪点。history 的 `M` 表示有效独立候选数，不是无限增长的帧计数器；裁剪历史 `M` 时必须按同一比例裁剪
history weight，保持 `W = weight_sum / (target * M)` 的含义不变。

## 5. Spatial Reuse

Spatial reuse 复用邻居像素的 reservoir：

```text
当前像素 reservoir
  + 左右上下等邻居 reservoir
  -> 合并成 final reservoir
```

同样必须先做 surface compatibility 检查，例如位置、深度、法线、roughness、base_color、metallic 是否足够接近。
邻居 reservoir 的 selected sample 也必须在当前 surface 上重新计算 target，再参与合并。

Spatial reuse 降噪明显，但也最容易出错。邻居像素可能在几何边缘、遮挡边缘或完全不同材质上；
如果 rejection 太松，就会把错误样本扩散到大片表面。

## 6. 最关键的不变量：保存 Light Sample Identity

ReSTIR 复用的不能是旧 shading point 的结果，而必须是可在当前 surface 上重新解释的 light sample identity。

错误做法是只保存：

```text
direction
distance
radiance
pdf
```

这些值只对原来的 shading point 成立。跨像素或跨帧复用时，当前 surface 看到同一个有限光源样本的方向、距离和 PDF
通常都已经变了。

正确做法是按 light 类型保存身份：

```text
HDRI:
  direction

emissive triangle:
  emissive_triangle_lights record index
  barycentric coordinate

point / spot sphere emitter:
  analytic light index
  sphere sample parameter

area light:
  area light index
  rectangle local sample (u, v)
```

然后每次在当前 surface 上重建候选：

```text
light_point = reconstruct_light_sample(sample_identity)
direction = normalize(light_point - current_surface.position)
distance = length(light_point - current_surface.position)
pdf = recompute_solid_angle_pdf(current_surface, sample_identity)
radiance = reevaluate_light_radiance(sample_identity)
shadow_ray = build_ray(current_surface, light_point)
```

HDRI 是特殊情况：方向本身就是无限远环境光样本身份，所以跨 surface 复用方向是成立的。
有限光源不是这样；如果复用旧方向，就可能让当前 surface 沿一条不指向原光源的方向 shade，产生亮斑、拖影和错误能量。

## 7. Final Shade

经过 initial、temporal、spatial 后，每个像素最终仍只有一个 selected sample。final shade 必须执行：

```text
1. 用当前 ReSTIR surface key 重建 primary surface，并确认它仍是 ReSTIR-eligible。
2. 用 selected sample identity 在当前 surface 上重建 light candidate。
3. 重新计算 target、PDF、radiance、direction、distance。
4. 重新 trace visibility。
5. 计算完整 RGB contribution。
6. 乘 reservoir shade weight。
7. 合入 HDR。
```

这里的 current surface 不能偷用给 RR/SR 准备的压缩 GBuffer。ReSTIR final shade 会重新发 shadow ray，
并且 target 函数依赖 BRDF 材质签名，因此需要使用 ReSTIR 自己保存的高精度 position / normal / roughness /
base_color / metallic。

final shade 权重为：

```text
W = weight_sum / (target(selected_sample) * M)
```

最终贡献为：

```text
final_contribution =
    full_rgb_contribution(selected_sample)
  * W
```

visibility 必须在 final shade 阶段重新 trace。不能复用历史 visibility，因为当前 surface 到光源之间的遮挡关系可能已经改变。

## 8. Primary ReSTIR DI 流程

Primary ReSTIR DI 可以拆成四个阶段：

```text
Path phase:
  primary visible surface 自己抽少量 unified light samples
  写 initial reservoir 和 primary surface key
  secondary bounce 继续普通统一 NEE

Temporal phase:
  用 motion vector 读取 previous reservoir
  surface/version 通过 rejection 后合并历史样本

Spatial phase:
  读取 3x3 邻居 reservoir
  surface 兼容后合并邻居样本
  final reservoir 只用于当前帧 shade，不写回 temporal history

Final shade:
  读取最终 reservoir
  在当前 surface 上重建 selected sample
  重新 trace visibility
  把 direct contribution 合入 HDR
```

它最终只改变 primary visible surface 的直接光采样质量，不应该改变 secondary bounce 的 NEE 语义。

## 9. 容易出错的地方

下面这些错误通常会直接导致亮斑、鬼影或能量不稳定：

- 把旧 `direction / distance / pdf` 当成可跨 surface 复用的样本身份。
- 复用历史或邻居样本时，没有在当前 surface 上重新计算 target。
- final shade 复用历史 visibility，而不是重新 trace shadow ray。
- 用 RGB 贡献直接作为 reservoir 选择概率，而不是用明确的标量 target。
- proposal PDF、target 和 final shade 权重使用了不同度量，例如 area PDF 与 solid-angle PDF 混用。
- surface rejection 过松，导致不同几何、不同法线或 disocclusion 处互相复用。
- 不同 base_color / metallic / roughness 的表面共享同一个 reservoir，导致旧 target 分母和当前 RGB 贡献不匹配。
- 把上一帧 spatial/final reservoir 当 temporal history，导致邻域样本跨帧反馈。
- temporal `M` 随帧数无界增长，或裁剪 `M` 时没有同步缩放 history weight。
- sky、emissive、analytic light 或 light class 开关变化后仍继续使用旧 reservoir。

## 10. 必须守住的正确性规则

实现 ReSTIR DI 时至少要守住这些规则：

```text
1. reservoir 保存可重建的 light sample identity，而不是旧 shading point 的光照结果。
2. 每次 temporal/spatial reuse 都要在当前 surface 上重新计算 target。
3. final shade 必须从 ReSTIR surface key 重建当前 eligible primary surface，并重新计算 candidate、visibility 和完整 RGB contribution。
4. PDF 统一使用 solid angle 度量，保持 NEE、MIS、target 和 reservoir weight 一致。
5. reservoir 用标量 target 选择样本，最终输出仍使用完整 RGB contribution。
6. surface 不兼容、光源版本变化或 light class 集合变化时，必须拒绝历史。
7. temporal history 只能来自上一帧 temporal reservoir；spatial final 不回灌 temporal。
8. `M` 必须表示有效独立候选数；裁剪历史 `M` 时同步缩放权重。
9. `InitialOnly` 应该能量上等价于普通 unified NEE，这是最基础的回归检查。
```

只要这些规则成立，ReSTIR DI 就是在复用“光源样本”。如果这些规则不成立，它很容易退化成复用旧方向、旧可见性或旧光照结果，
从而污染当前帧和后续历史。
