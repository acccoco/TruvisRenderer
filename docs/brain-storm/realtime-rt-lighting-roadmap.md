# 实时 RT 光照采样路线

> 状态：活跃方向，更新于 2026-06-15。当前事实以
> [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md)、[`docs/summaries/`](../summaries/) 和代码为准。

本文记录实时 RT 主流程后续光照采样与间接光缓存的阶段性路线。目标是先把直接光 NEE 的 PDF / MIS
语义做稳，再接入 primary surface 的 ReSTIR DI，最后引入 SHARC 类世界空间缓存截断后续间接路径。
DLSS SR/RR 只消费最终 HDR、GBuffer、depth 和 motion vectors，不参与光照采样、reservoir 或 cache 状态。

## 当前问题

当前 RT 路径已有 BRDF 采样、HDRI NEE 与 MIS，但 NEE 的光源侧分布仍较弱：

- HDRI / sky 仍使用均匀球面采样，未按环境贴图亮度构建 CDF、alias table 或 PDF texture。
- 自发光材质主要在路径命中时累加 emission，尚未作为 emissive triangle light 参与 NEE。
- 点光源、方向光、spot light 等 analytic light 尚未纳入统一的直接光候选系统。
- 后续 bounce 仍主要依赖普通 BSDF 路径延伸，缺少稳定的 world-space radiance cache。

因此后续路线应把“直接光采样”和“间接光复用”分开处理，避免把所有问题都压到同一个算法里。

## 路线总览

本路线采用项目原生实现，不直接引入 RTXDI / SHARC SDK 作为运行时依赖。阶段顺序固定为：

1. 光照采样契约基线。
2. HDRI / sky 重要性采样。
3. 自发光三角形 NEE。
4. 统一 Light Candidate System。
5. Primary ReSTIR DI。
6. ReSTIR DI 稳定化。
7. SHARC 资源与 Update / Resolve。
8. SHARC Query 接入后续 bounce。
9. 文档与调试收尾。

全局约束：

- NEE / ReSTIR DI 只负责直接光；SHARC 只负责后续间接光缓存。
- ReSTIR DI 第一版只做 primary visible surface，secondary hit 继续普通 NEE。
- 所有 screen-space 资源按 `render_extent` 维护；world-space cache 不使用 DLSS output 反哺。
- 每个阶段必须保留可回退路径和调试观测入口，稳定后再进入下一阶段。

## 第一阶段：光照采样契约基线

目标：先固定所有直接光候选使用的 PDF、MIS、visibility 和 shade 契约，避免后续阶段各自定义一套采样语义。

核心思路：把直接光样本抽象为 light candidate，统一表达方向、radiance、距离、采样来源、solid-angle PDF、
target 值、shadow ray 和最终贡献评估。BRDF 命中 sky 或 emissive surface 时，也必须能查询对应 light PDF。

关键约束：只整理语义和 helper 边界，不改变当前 HDRI uniform NEE 结果；所有 PDF 必须处在同一度量下，默认使用 solid angle。

完成标准：旧路径可无差异运行；debug 仍能区分 NEE HDRI、BRDF HDRI 和 emission；非法候选、零 PDF 和背面光照都有明确无效返回。

当前状态（2026-06-15）：契约基线已落地。realtime RT shader 已把 HDRI NEE 整理为 candidate、visibility 和 shade
三段，light-side PDF 统一以 solid angle 表达，并通过 `EnvMap::pdf` 供 NEE 和 sky miss MIS 共用。
HDRI 仍是 uniform sphere 采样；emissive hit 仍保持命中即累加 emission，在第三阶段生成 emissive triangle light table
之前不伪造可竞争的 light PDF；point / directional / spot light、reservoir 和 SHARC 均未接入。

## 第二阶段：HDRI / Sky 重要性采样

目标：替换当前均匀球面采样，让 HDRI / sky 按亮度与球面面积采样，降低高亮环境光噪声。

核心思路：SkyBridge 在真实 sky ready 后生成 `luminance(texel) * solid_angle(texel)` 分布；fallback sky 使用 1x1 常量分布。
shader 从该分布采样方向并返回 solid-angle PDF，NEE 和 sky miss 的 MIS 都读取同一 PDF 查询入口。

关键约束：lat-long 贴图必须计入 texel 对应球面面积；sky 切换、fallback/真实贴图切换时必须 reset 相关 temporal / reuse 状态。

完成标准：uniform sky sampling 可回退；HDRI NEE、sky miss MIS 和 debug 统计都使用一致 PDF；高亮 HDRI 场景下直接光噪声明显低于 uniform。

## 第三阶段：自发光三角形 NEE

目标：把自发光材质从“路径碰巧命中才加 emission”提升为可采样 area light。

核心思路：scene sync 枚举 active instance 的 emissive submesh triangles，生成 world-space triangle light records 和 power
alias table。NEE 先按 power 选择三角形，再在三角形面积上采样点，并把 area PDF 转换为 solid-angle PDF。

关键约束：路径直接命中 emissive surface 时必须使用同一套 light PDF 计算 MIS，避免 NEE 和 BRDF hit 双计能量；实例 transform
变化、材质变化和 mesh ready 状态变化必须能触发 light record 更新。

完成标准：emissive triangle 可通过 NEE 被采样；emissive hit 和 NEE 的 MIS 能量稳定；非 emissive 场景不引入额外直接光候选。

## 第四阶段：统一 Light Candidate System

目标：把 HDRI、emissive triangle、point、directional 和 spot light 收敛到同一个直接光候选系统，为 ReSTIR DI 做准备。

核心思路：先按 light class / light power 选择候选来源，再调用各类型自己的 sample、PDF 查询、target、visibility ray 和 shade。
普通 NEE 先稳定运行，不接 reservoir。

关键约束：delta light 的概率只在离散 light 选择层表达；HDRI 和 area light 同时包含连续 PDF；所有候选最终都要能参与同一套 MIS。

完成标准：primary 和 secondary hit 都能通过统一候选系统做普通 NEE；每种 light 类型都能独立关闭或调试；旧 HDRI-only 路径可回退。

## 第五阶段：Primary ReSTIR DI

目标：只对 primary visible surface 的直接光启用 ReSTIR DI，提升每像素一条或少量 shadow ray 下的有效样本数。

核心思路：用统一候选系统生成 initial reservoir；通过 motion vector 和 previous primary surface history 做 temporal reuse；
通过 depth、normal、roughness 和 material rejection 做 spatial reuse；最终只 shade reservoir 选中的一个候选。

关键约束：第一版不处理 secondary reservoir；resize、DLSS mode 切换、sky/light/material/scene 变化必须 reset reservoir history；
默认最终 shade 阶段重新 trace visibility，不把不可信历史可见性直接当成真值。

完成标准：支持 Off / InitialOnly / Temporal / TemporalSpatial 模式；primary 直接光噪声低于普通统一 NEE；快速相机移动和 disocclusion
不会产生明显脏历史。

## 第六阶段：ReSTIR DI 稳定化

目标：控制 ReSTIR DI 的 bias、ghosting、boiling 和错误复用，让其具备默认开启条件。

核心思路：加入 reservoir confidence、candidate age、normal/depth 阈值、disocclusion rejection、visibility reuse 开关和 debug
visualization。用调试通道观察 initial、temporal、spatial 和 final reservoir。

关键约束：稳定性优先于极限性能；任何历史复用都必须能被 reset 或禁用；错误候选不能污染后续 frame 的 reservoir。

完成标准：动态 light、动态 emissive instance、相机快速移动和窗口 resize 下无长期残影；调试视图能定位 reject / reuse 原因。

## 第七阶段：SHARC 资源与 Update / Resolve

目标：建立 SHARC 类 world-space radiance cache 的资源和更新流程，但暂不改变主路径 query 行为。

核心思路：新增 hash entries、accumulation、resolved 等 app-owned cache buffers；用 sparse update pass 从少量路径写入当前帧
radiance；resolve pass 合并当前帧和历史数据，并处理 stale entry、sample count 和 hash collision。

关键约束：SHARC 只缓存后续 indirect radiance，不参与 direct light candidate selection；cache 生命周期、clear/reset、resize 和 scene
变化必须先定义清楚；Off 模式必须与 ReSTIR DI 路径一致。

完成标准：SHARC buffer 可以创建、清理、更新、resolve 和 debug 可视化；关闭 query 时画面不变；hash grid / sample count / stale
状态可观测。

## 第八阶段：SHARC Query 接入后续 Bounce

目标：在 hit1 / hit2 之后查询 world-space radiance cache，命中时用 cached radiance 估计剩余间接光并提前终止路径。

核心思路：path continuation 到达适合缓存的 surface 时查询 SHARC；命中后把 cached radiance 乘当前 throughput，未命中继续 path
tracing，并让 update pass 后续学习该区域。

关键约束：primary hit 不查询；delta / sharp specular 不查询；roughness 不足、路径段长度小于 voxel 尺寸或法线差异过大时不查询；
动态光和材质变化需要控制 stale history。

完成标准：SHARC On/Off 可独立对比；primary 细节、接触阴影和 sharp specular 不被缓存抹平；间接光噪声或路径成本相对普通路径下降。

## 第九阶段：文档与调试收尾

目标：把最终实现的职责边界、资源 owner、reset 条件和调试方法沉淀到活跃文档。

核心思路：同步更新架构 summary、模块 README 和本路线文档；整理 UI / debug channel，使 NEE、ReSTIR DI、SHARC 的开关和中间状态可观察。

关键约束：不建立归档目录；过期设计要提炼到活跃文档或删除；旧实验路径若不再属于主线，应从活跃实现事实中移除。

完成标准：文档能说明每个 pass 的输入输出、生命周期和 DLSS 边界；默认路径稳定，旧路径可回退；后续维护者不需要回读讨论记录理解主线。

## 暂不优先

- secondary ReSTIR DI：可以扩展到后续 hit 的 NEE，但 secondary hit 缺少稳定 screen-space 对应关系，第一版复杂度偏高。
- ReSTIR GI / ReSTIR PT：能复用间接路径样本，但涉及路径状态、Jacobian、重投影和偏差控制，应放在基础 NEE 与 radiance cache 稳定后再评估。
- path guiding：适合学习间接入射方向分布，但实时集成复杂度不低，暂不作为第一条路线。
