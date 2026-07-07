# DLSS 质量验证与清理

## 目标

围绕 DLSS 的后续工作只保留画质验证、motion vector 质量、运行时降级和旧路径清理。目标是提升稳定性和可维护性，
而不是重新记录 Streamline 接入流程。

## 当前基线

当前渲染配置、DLSS mode、frame state、temporal state 和 realtime RT 数据边界见
[`../summaries/render-configuration-system.md`](../summaries/render-configuration-system.md) 和
[`../summaries/render-graph-and-data-flow.md`](../summaries/render-graph-and-data-flow.md)。

## 待推进内容

- 画质验证：建立 SR / RR 在静止画面、相机移动、resize、mode 切换、反射和透明材质下的手动或自动检查清单。
- Specular motion vector v2：评估 specular hit distance、多层反射/折射和失败 fallback 是否需要进入 shader 输出。
- Runtime degrade UI：让 feature support、evaluate failure、非法 optimal settings 或资源 tag 异常能通过 UI / 日志明确降级原因。
- 旧 pass 清理：删除或隔离不再进入主路径的 legacy denoise / accum 实验代码，避免误以为仍影响 realtime 输出。
- Debug viewer 校验：确认 DLSS 输入、输出和相关 debug image 的 import state、尺寸和 frame label 清楚可查。

## 边界与非目标

- 不把 Streamline FFI、resource tag 表和 evaluate 分支事实复写到本文件。
- 不让 DLSS state 参与 light sampling、ReSTIR、SHARC 或 offline accumulation。
- 不引入自动曝光或 display transform 重构；SDR tone mapping 仍属于 app / pass-local 方向。
- 不在没有画质证据时扩大 specular motion vector 的复杂度。

## 完成标准

- SR / RR 的关键场景有可复现的验证入口和日志判断标准。
- specular motion vector 的限制、fallback 和下一版目标有明确结论。
- runtime 降级能被用户和开发者看懂，不需要阅读低层日志才能判断原因。
- 旧 pass 清理后，主路径数据流与 summaries 一致。
