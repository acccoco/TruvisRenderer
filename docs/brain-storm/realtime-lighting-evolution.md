# Realtime Lighting 演进

## 目标

在现有 realtime RT 基础上继续提升直接光候选选择、temporal reuse 稳定性和后续 bounce 间接光复用质量。
目标是把噪声、残影、stale cache 和动态场景交互问题逐步变成可观测、可回退、可验证的工程能力。

## 当前基线

当前 realtime RT path loop、统一直接光候选、ReSTIR、SHARC、MIS 和 debug channel 语义见
[`../summaries/realtime-rt-raytracing-flow.md`](../summaries/realtime-rt-raytracing-flow.md)。

## 待推进内容

- Light-class PMF：用 power 或场景统计为 light class 选择分配概率，替代均匀 class 选择，并保持 PDF / MIS 闭合。
- ReSTIR 稳定化：补充 confidence、age、disocclusion、dynamic light/material rejection 和 debug visualization，
  让错误历史不会污染后续 frame。
- SHARC 历史控制：定义 scene reload、dynamic light、material edit、camera scale 和 stale entry 的 reset / decay 策略。
- Secondary reuse 评估：比较 secondary ReSTIR、ReSTIR GI、path guiding 与 world-space cache 的复杂度、bias 和可回退路径。
- Debug 收敛：让候选来源、history reject、cache query 和 final contribution 的观测入口服务稳定性判断，而不是堆叠临时通道。

## 边界与非目标

- 不把 DLSS、RR 或 tone mapping 状态纳入 light sampling / reservoir / cache owner。
- 不直接引入外部 SDK 作为 runtime 依赖；外部实现只作为算法参考。
- 不在没有稳定 reset 和 debug 入口前默认开启更激进的历史复用。
- 不把算法解释和当前 shader 事实放进本文件；事实仍进入 realtime RT summary。

## 完成标准

- Light-class PMF 的采样、PDF 查询和 MIS 在普通 NEE 与 hit/miss 竞争估计中保持同一度量。
- ReSTIR 在相机快速移动、动态光、材质变化和 resize 下没有长期污染，并能通过 debug 定位 reject 原因。
- SHARC 对 scene 变化的 stale / reset 策略可解释、可调试，并不破坏 primary 细节或 sharp specular 路径。
- 后续间接光复用路线有明确取舍结论，能决定是否进入实现。
