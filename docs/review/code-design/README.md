# 代码设计分批扫描清单

> 当前提交历史：全部 239 个审阅提交已按类型压缩为 12 个，基准仍为 `9527799b`。完整对照见[提交压缩映射](commit-squash.md)。下文与各报告中的旧项目 hash、提交次数、分组及“本报告所在提交”均保留为压缩前历史/证据说明；第三方版本和原始实验记录不改写。

目标：检查各模块及项目整体是否存在冗余、过度抽象、职责错位和设计约束缺口。每批能够在独立会话中完成，并通过文件恢复上下文。

建单日期：2026-10-28。原始基准为 dlss5 的 `010d963db392fa579aeff18323a5dd7035896f9a`；迁入 master 后，目录与入口已按压缩前证据基准 `892332a4c47114ff664cde0d815d6875a04228ce` 复核（实现基准为 `9527799b7ca72a25ffa715b6a6ec143f72b7c65d`，其后两个提交只改文档）。
当前已完成 **B00 全局地图的 master 复核**及 Shader/native P3 文档局部修复，见 [B00 报告](B00.md)；**B01 基础工具与路径**已扫描并完成五项局部修复，见 [B01 报告](B01.md)，对应压缩后提交 `be9b4dec`、`db4375b5`。**B02 已完成约定范围扫描，CXX 子批已扫描**：[B02-a 首轮](B02-a.md) 对应 `94a9e96e`、`2b0eaee5`；[B02-a2 续接](B02-a2.md) 在 `refactor` 的 `2b0eaee5` 上完成当前产物归属、部署预检与失败重试修复，分为 `68704517` 的完整部署清单和 `232702f5` 的部署恢复两批；详见续接报告。Debug/Release 实际构建部署、临时夹具和 sample 编译通过；完整项目全新环境与 GPU 未验收，多文件事务回滚不作保证。B02-b1 单任务失败与重试链已审阅、修复并验证，见 [报告](B02-b1.md)。B02-b2 输入身份、发现与 binding 审阅及局部修复完成，见 [报告](B02-b2.md)。B02-b3 输出与失败状态闭环已完成，见 [报告](B02-b3.md)，B02-b 约定范围已扫描。B02-c 资源缓存、完整发布和路径契约已完成，见 [报告](B02-c.md)。B02 子批已全部闭合，交接见 [B02 汇总](B02.md)。B03-a descriptor 声明与写入已审阅、局部修复并验证，见 [报告](B03-a.md)；B03-b 共享结构与生成投影已审阅并完成局部修复，见 [报告](B03-b.md)；B03-c include 层级与 manifest layer 校验已审阅并修复，见 [报告](B03-c.md)；B03 三个子批已闭合，B04-a 首轮完成 Assimp 加载异常与路径修复，B04-a2 完成复制容量、空数据与借用对象存储修复，见 [续接报告](B04-a2.md)，B04-b 完成 Vulkan 桥接前置修复和 ABI/转发验证，见 [报告](B04-b.md)；B04-c1 已完成 loader 异常与诊断释放修复，见 [报告](B04-c1.md)；B04-c2 已修复 SDK 失败状态、callback 同步与路径身份，见 [报告](B04-c2.md)；B04-c3 已完成 feature ABI 审阅、SR 深度转发与跨语言调用边界修复，见 [报告](B04-c3.md)；B04 约定范围已扫描，见 [B04 交接](B04.md)。B05-a 已完成 root/device/context 首轮扫描，修复队列选择和 debug 回调边界，见 [报告](B05-a.md)；B05-a2 已修复 root 构造失败回收、VMA Drop 与生命周期注释，见 [报告](B05-a2.md)；B05-b1 已完成主机映射边界和 index 上传类型修复验证，见 [报告](B05-b.md)；B05-b2 已完成叶子构造与图像描述修复，见 [报告](B05-b2.md)；B05-c1 同步对象与 query ABI 已修复，见 [报告](B05-c1.md)；B05-c1b 已完成 command/pool 与提交前失败局部修复，见 [报告](B05-c1b.md)，B05-c2 已完成 descriptor/graphics 创建失败修复，见 [报告](B05-c2.md)，B05-c2b 已完成 compute/RT 组合构造修复，见 [报告](B05-c2b.md)，B05-c3 已完成 SBT/SoA 算术及 AS 未提交构造修复，见 [报告](B05-c3.md)，B05-c3b 已完成 AS 输入与同步构建已知安全阶段修复，见 [报告](B05-c3b.md)，B05-c4 已完成同步上传 helper 与组合构造回收，见 [报告](B05-c4.md)，B05-d 已完成 WSI 叶子创建、metadata 与 acquire/present 状态修复，见 [报告](B05-d.md)，B05-d2 已完成 Presenter 组合构造和重建失败局部修复，见 [报告](B05-d2.md)，跨代 present semaphore 与退场缺口交 B12/B27；B06-a 已完成 handles/registry 扫描、延迟回收与诊断异常收尾修复，见 [报告](B06-a.md)，B06-b 已完成 slot 回收时序与 binding 组合构造修复，见 [报告](B06-b.md)，B06-c 已删除闲置 staging owner 并修复 FIF 构造/释放收尾，见 [报告](B06-c.md)，B07-a 已完成资源登记、声明合并与核心写分类修复，见 [报告](B07-a.md)，B07-b 已完成屏障与物理解析修复，见 [报告](B07-b.md)，B07-c 已完成 semaphore 聚合、名称前置校验与执行契约修复，见 [报告](B07-c.md)，B08-a 已完成加载任务、handle 与异常/等待边界修复，见 [报告](B08-a.md)，B08-b 已完成真实编解码验证与文档补充，见 [报告](B08-b.md)，B08-c1 已修复资源字节与 URI，见 [报告](B08-c1.md)，B08-c1b 已修复版本与必需扩展声明检查，见 [报告](B08-c1b.md)，B08-c2a 已完成 accessor 边界修复，见 [报告](B08-c2a.md)，B08-c2b 已修复几何生成与着色属性，见 [报告](B08-c2b.md)，B08-c2c 已修复 scene 层级与变换，见 [报告](B08-c2c.md)，B08-c3a 已修复材质参数与 alpha 阈值，见 [报告](B08-c3a.md)，B08-c3b 已完成纹理槽审阅与能力边界修复，见 [报告](B08-c3b.md)，B08-d1 已完成 Assimp 数据发布前置修复，见 [报告](B08-d1.md)，B08-d2 已修复材质转录，见 [报告](B08-d2.md)，B08-d3 已完成几何与生命周期修复，见 [报告](B08-d3.md)，B08-e1 已修复导出文件边界，见 [报告](B08-e1.md)，B08-e2 已完成烘焙语义、当前帧与 glTF 预检修复，见 [报告](B08-e2.md)，B09-a1 已完成纹理请求去重与失败恢复，见 [报告](B09-a1.md)，B09-a2 已完成 scene ingest 发布预检与诊断异常恢复，见 [报告](B09-a2.md)，B09-b 已完成 instance/material 校验与引用一致性复核，见 [报告](B09-b.md)，B09-c 已完成灯光、内置几何与聚合边界复核，见 [报告](B09-c.md)，B10-a1 已完成共享上传队列查询/等待失败修复，见 [报告](B10-a1.md)，B10-a2 已完成构造/准备回收与提交接管，见 [报告](B10-a2.md)，B10-a3 已完成结果保留与 texture 发布回滚，见 [报告](B10-a3.md)，B10-e 已完成跨 queue wait 与 image/mesh 消费屏障修复，见 [报告](B10-e.md)，B11-a1 已完成 instance 对账与快照交付修复，见 [报告](B11-a1.md)，B11-a2 已完成通知交接与下游失效修复，见 [报告](B11-a2.md)，B11-b1 已完成 SceneBuffers 生命周期与扩容交接，见 [报告](B11-b1.md)，B11-b2 已完成矩阵录制与 FIF/历史确认，见 [报告](B11-b2.md)，B11-b3 已完成结构表索引校验、有效前缀复制与 root 转录核对，见 [报告](B11-b3.md)，B11-c 已完成 TLAS 关闭收尾与当前快照映射核对，见 [报告](B11-c.md)，B11-d1 已完成 analytic 生命周期与容量/上传修复，见 [报告](B11-d1.md)，B11-d2a 已完成 emissive 生命周期与候选扩容修复，见 [报告](B11-d2a.md)，B11-d2b 已完成 emissive 映射约束与环境交接复核，见 [报告](B11-d2b.md)，B11-e 已完成组合生命周期与职责汇总，见 [报告](B11-e.md)；下一批 B12；分批进度见 [B11 交接](B11.md)。Runtime 文档符号偏差已在 B12-d 修正；B05 约定范围已扫描但仍有跨层待办，B06 约定范围已扫描，B07 约定范围已扫描，B08 约定范围已扫描，B09 约定范围已扫描，B10 约定范围已扫描，B11 约定范围已扫描；B12-a1 已完成 RayCastService 组合与容量修复，见 [报告](B12-a1.md)，B12-a2 已完成提交/读回与输入边界修复，见 [报告](B12-a2.md)，B12-b1 已完成已知安全构造阶段回收，见 [报告](B12-b1.md)，B12-b2 已完成关闭诊断与 root 日志断点修复，见 [报告](B12-b2.md)，B12-c 已完成固定阶段/确认/失败出口复核及正常退场日志修复，见 [报告](B12-c.md)，B12-d 已完成帧状态、resize/present 与 Ctx 复核及帧号约束修复，见 [报告](B12-d.md)；B12 约定范围已扫描；B13-a 已完成线程完成通知与原始异常保留修复，见 [报告](B13-a.md)，B13-b 已完成窗口错误返回与关闭交接修复，见 [报告](B13-b.md)，B13-c 已完成输入退出边界与 sample 松键/坐标修复，见 [报告](B13-c.md)；B13 约定范围已扫描；B14-a 已完成输入边沿与相机取消修复，见 [报告](B14-a.md)，B14-b 已完成 roll 与导航能力修复，见 [报告](B14-b.md)，B14-b2 已完成相机数值边界与冗余清理，见 [报告](B14-b2.md)，B14-c 已完成组合/配置复核与 Ctx/诊断清理，见 [报告](B14-c.md)；B14 约定范围已扫描，B15-a1 已完成 present 格式变化后的 pipeline 重建修复，见 [报告](B15-a1.md)，B15-a2 已完成主视图提交确认和计数边界，见 [报告](B15-a2.md)，B15-a3 已完成共享 target 与组内释放修复，见 [报告](B15-a3.md)，B15-a4 已完成冗余主视图资源与顶层资源清理修复，见 [报告](B15-a4.md)，B15-a5 已完成共享 target 构造回收与 debug source 有效性修复，见 [报告](B15-a5.md)，B15-b1 已完成 SDR 提交状态与组内清理修复，见 [报告](B15-b1.md)，B15-b2 已完成曝光 kernel 构造回收、LUT 屏障清理和显示契约复核，见 [报告](B15-b2.md)；B15 约定范围已扫描；B16-a 已完成 feature 登记与释放失败状态修复，见 [报告](B16-a.md)，B16-b 已完成失败 graph 中止及 clip 变换精度修复，见 [报告](B16-b.md)，B16-c 已完成 exposure 构造回收、预留 hook 清理及生命周期收尾审阅，见 [报告](B16-c.md)；B16 约定范围已扫描；B17-a 已完成 GGX NDF 分布与低粗糙度精度修复，见 [报告](B17-a.md)；B17-b1 已完成公共材质参数校验与 MR 组合域修复，见 [报告](B17-b1.md)；B17-b2 已完成 RR delta guide 与默认 Surface F0 修复，见 [报告](B17-b2.md)；B17-c1 已完成光滑透射数值顺序修复并记录临界精度边界，见 [报告](B17-c1.md)；B17-c2a 已修复 trace 几何法线来源并交接重建数据缺口，见 [报告](B17-c2a.md)；B17-c2b 已统一着色半球有效域并交接法线/切线模型缺口，见 [报告](B17-c2b.md)；B17-c3 已修复 roulette 权重及随机边界并交接复合材质/数值支持域，见 [报告](B17-c3.md)；B17-d 已完成公共事件去重、纯发光契约修复及消费链交接，见 [报告](B17-d.md)；B17 约定范围已扫描，见 [汇总与交接](B17.md)；B18 已完成光源采样与路径积分的约定范围审阅，局部修复、证据限制与后续项见 [汇总](B18-d.md)；B19-a 已完成 surface 几何法线持久化与资源绑定修复，见 [报告](B19-a.md)；B19-b 已完成 initial/RIS 数值链局部修复，见 [报告](B19-b.md)；B19-c1 已完成历史年龄与校正分母修复，见 [报告](B19-c1.md)；B19-c2a 已修复零估计候选池与 M 计数，见 [报告](B19-c2a.md)；B19-c2b 已修复历史 target 观察方向并完成来源选择审阅，见 [报告](B19-c2b.md)；B19-c3a 已修复重连 Jacobian 与校正度量，见 [报告](B19-c3a.md)；B19-c3b 已修复配置切换时的旧池拒绝并清理 final 重复求值，见 [报告与 B19-c 汇总](B19-c3b.md)；B19-c 约定范围已扫描；B19-d1 已修复 ReSTIR 图像组合构造的前缀回收，见 [报告](B19-d1.md)；B19-d2 已修复实际提交帧 jitter 与 history 图像跨帧访问依赖，见 [报告](B19-d2.md)；B19-d3 已修复 SHARC debug initial 漏写并跳过未消费的 final 着色，见 [报告](B19-d3.md)；B19 约定范围已扫描，证据限制见 [汇总](B19.md)；B20-a 已完成 SHARC 资源生命周期审阅，修复分配前缀回收与首个 cache 访问依赖，见 [报告](B20-a.md)；B20-b1 已修复满探测窗口错误索引，见 [报告](B20-b1.md)；B20-b2 已修复 Resolve 稳定快照与资源同步接线，见 [报告](B20-b2.md)；B20-b3 已完成 Query 接线审阅与缓存失效修复，见 [报告](B20-b3.md)；B20-b4 已修复零容量 hash 入口并记录数值支持域，见 [报告](B20-b4.md)；B20-c 已修复 Off 与 miss 的 debug 观测边界，见 [报告](B20-c.md)；B20 约定范围已扫描；B21-a 已完成 Editor transport 与 timeout/shutdown 验证，见 [报告](B21-a.md)；B21-b 已完成 DTO、opaque ID、scene version 与 mutation 一致性审阅并修复 alpha cutoff 范围不一致，见 [报告](B21-b.md)；B21-c 已完成 Desktop 特权命令、import 交接与关闭顺序审阅并修复 SceneInitializer 终态重复消费，见 [报告](B21-c.md)；B21 约定范围已扫描；B22-a 已完成 Automation 控制器审阅并修复失效 import ID，见 [报告](B22-a.md)；B22-b 已完成 HTTP 与 schema 审阅并修复 Origin 校验和关闭排空，见 [报告](B22-b.md)；B22 约定范围已扫描；B23-a 已修复 slider 重复提交，见 [报告](B23-a.md)；B23-b 已修复 session 清理与材质 ACK 查询顺序，见 [报告](B23-b.md)；B23-c 已修复材质数值输入、mock 连接代次与 viewport 清理顺序，见 [报告](B23-c.md)；B23 与 B24 约定范围已扫描，见 [B24](B24.md)；B25-a 已修复动态 DPI 传播与字体重建，见 [报告](B25-a.md)；B25-b 已完成 App/场景/sample 集成复核并修复 layout owner，见 [报告](B25-b.md)；B26 已完成全局冗余与抽象复核，见 [报告](B26.md)；B27 已完成八条端到端链的全局严密性复核，见 [报告](B27.md)；本清单审阅阶段闭合，全局整改仍只输出方案，不实施。

master 只有 NR SDK/参考资源配置，没有 NR native module、binding 或 Renderer 接入；B04/B16 按现有实现扫描，NR 实现进入本分支后再补入范围。

本文件是工作清单，不是设计规范或审计结论。设计依据仍是 [ARCHITECTURE.md](../../ARCHITECTURE.md)、[design/](../../design/)；实现事实以当前代码为准。历史聊天与记忆只用于定位，不作为已验证证据。

## 1. 使用方式与上下文边界

1. 默认一次会话只执行一个批次，采用“分批审阅、局部修复、跨模块问题集中处理”。先做 B00，再依次完成模块批次，最后做 B26、B27；不必等全仓分析结束才修复。
2. 每批先读根 `AGENTS.md`、递归读取 `.agents/rules/` 全部 Markdown，再读架构入口、本批相关 design、模块 README 和已有直接相关报告。不要载入全部历史报告或整仓源码。
3. 每批记录当前 HEAD、工作区改动和实际阅读范围。HEAD 相同也要检查未提交改动；引用旧结论前，核对涉及的源码、调用方和契约是否变化。
4. 通常聚焦一个 owner 或一条状态链；从入口追到直接调用方、关键依赖及失败出口。先列文件，再按符号读局部，不一次打印大文件或所有 README。
5. 初始阅读范围建议控制在约 5–12 个核心文件；这只是拆批信号，不是覆盖完成标准。若还需展开第二条复杂状态机、大量调用方或算法参考，拆成 `Bxx-a`、`Bxx-b`，分别记录覆盖范围。
6. 越界问题只记录“路径/符号、疑点、目标批次、需要核对的契约”。允许读取直接依赖以证明问题，但不要顺势审计整个相邻模块。
7. 每批先将审阅证据写入本目录的 `Bxx.md`，再按下方准入条件处理局部修复，最后更新报告和本清单。临时命令输出、截图和实验产物放在根 `.temp/` 下。不主动新增单元测试。按当前用户授权，每个执行批次在必要验证完成后自动 commit，限制在两次以内，相关文档随行为提交；不推送 Git。
8. 执行本清单批次时，默认包含已确认、边界清楚且满足准入条件的局部修复；用户明确要求“仅分析/只审阅”时，只记录问题与修复计划。对旧批次的待办按交接进入相应批次处理，不因流程更新自动展开全部历史修复。

每个批次报告末尾保留一段简短交接：已确认的 owner/契约、重要发现、未查内容、跨批次问题、下一步。后续会话首先读交接和相关证据，再回到当前源码复核，不依赖上一轮聊天摘要。

状态使用：`待扫描 / 进行中 / 已扫描 / 部分完成 / 待复核`。勾选表示**本批约定范围完成审阅并有报告**，不表示没有问题、问题已修复或 GPU 已验证。拆成子批次后，全部子批次结束才勾选父项。

### 修复时机与准入条件

**修复范围由问题的影响边界决定，不由扫描批次的目录边界决定。** 按下表分流；严重程度决定处理优先级，不替代证据和边界检查。

| 问题类型 | 处理时机 |
|---|---|
| 已确认的局部错误、同一规则的重复实现、失效分支、文档错误 | 本批审阅落盘后即可修复，不等全部批次完成 |
| 涉及多个模块的状态、接口、资源生命周期或同步问题 | 先完成相关批次或必要的定向补查，走通受影响调用链，再集中设计和修复；不必机械等到 B27 |
| crate 合并、抽象层删除、全局配置或错误模型统一 | 留到 B26/B27，用全局证据输出建议与验收方案；按当前用户要求，不实施最终全局修改 |
| 证据不足、只有局部代码迹象或缺少运行验证 | 补证据并保留待办，不将猜测直接变成重构 |

局部修复须同时满足：问题已确认；owner 与直接调用/消费方已核对；最小修改范围清楚；所需验证可明确列出；不依赖尚未决定的跨模块设计，不改变未经确认的长期需求或设计不变量。改动前按 `AGENTS.md` 完成适用的架构预检，说明复用位置、所有权、依赖/线程/资源契约及文档影响。不能回答这些问题时，先补查或转交，不能以“只是清理”跳过。

例如，CPU→GPU 同步问题通常需要结合 B08–B11；帧阶段和 DLSS 生命周期问题可能需要结合 B12–B16。局部拼补会掩盖根因时，记录涉及的批次与尚缺的证据。高优先级问题可以提前做相关定向审阅，但不借机扩展成无关重构。

### 每批闭环与跨会话交接

1. **审阅落盘**：保存修复前基准、证据、反证和未查范围，先得出问题判断。
2. **确定处置**：逐项注明本批修复、转交哪个批次或暂不处理及原因；说明最小方案和验证方式。
3. **实施与验证**：只改当前明确范围，复用既有 owner；不夹带格式整理、兼容层或全局抽象。执行与改动匹配的必要检查，验证受限时如实记录缺口。
4. **回填报告**：保留修复前发现，在同一问题下补充修改位置、修复后基准（commit 或未提交文件）、验证结果和剩余风险。同步维护直接相关 design/模块 README，不自动更新 `docs/summary/`。
5. **复核影响并继续**：对受影响的旧报告结论标记“待复核”，写清变更点和复核入口；下一批先核对基准，只重查受影响部分，不把旧行号或旧结论当成当前事实。

问题处理状态在原报告中记录：`待修复 / 等待相关批次 / 已修改待验证 / 已修复并验证 / 暂不处理 / 待复核`，并注明验证范围。审阅是否完成、问题确信程度、修复是否闭合是三件事，不能互相替代；不要只因代码已改就标为“已修复并验证”。无需另建问题注册表或复制整份报告。

若局部修复超出本次上下文容量，先保存证据、最小方案、当前改动和剩余检查，下一会话继续修复再切换批次，不强行完成。当前用户已授权逐批提交：按语义和风险组织，每个执行批次最多两次 commit，每个提交包含一个可验证的行为变化，并能独立编译和回滚；相关报告与模块说明随行为提交，不额外拆成碎片提交。此授权不包含推送。完成 B26/B27 审阅后保留全局整改方案，不实施最终全局修改。

## 2. 所有批次共用的判断标准

| 维度 | 具体检查 | 判定问题所需证据 |
|---|---|---|
| 冗余 | 同一规则、转换、默认值、状态或算法是否维护多份；分支、字段、依赖是否已经失去用途 | 对照两处实现及调用方，说明它们为什么必须同步变化；删除候选须排查 feature、宏、生成代码、FFI、工具与示例入口 |
| 过度抽象 | trait、泛型、wrapper、Ctx、manager、crate 是否增加理解成本却没有保护边界；是否为假设需求设计框架 | 展示实际调用链，说明抽象承担的约束与替代方案；比较删层后是否仍保持生命周期、类型安全与依赖方向 |
| 抽象不足与职责错位 | 大型 owner 是否同时处理平台、业务、GPU、协议；相关状态和步骤是否散落；公共 API 是否泄漏内部细节 | 给出同一变化需要修改的分散位置、错误依赖方向或绕过 owner 的调用 |
| 设计不严密 | 非法状态能否构造；前置条件是否只靠调用者记忆；部分失败、过期结果、重复调用、退出是否闭合 | 给出触发条件、状态转移、缺失的检查及后果；能静态证明的标为静态证据，运行时猜测标为待验证 |
| 可修改性 | 关键契约是否用类型、断言或已有检查表达；中文注释是否说明非显然约束；文档是否偏离实现 | 指向直接承载契约的位置；不要求复制代码结构或给普通字段补重复注释 |

避免误判：

- 单实现 trait、薄 wrapper、较多文件、较长函数、少量相似代码都只是检查线索，不能单独证明过度设计。
- CPU 权威数据、GPU 派生状态、FIF 副本、协议 DTO、UI 编辑草稿可能有不同职责；先查同步和失效契约，再判断重复。
- 窄 Ctx、typed handle、显式 destroy 即使代码较多，也可能在保护阶段权限、身份或销毁顺序。
- 两处代码只在形状上相似、变化原因不同，不应强行合并。相反，同一业务规则在两个入口各自实现，即使很短也可能需要收敛。
- `unsafe`、`unwrap`、`Arc<Mutex<_>>`、布尔字段数量和文件行数只能用于定位；结论必须解释真实的前提与风险。
- 文档约束与实现不一致时记录偏差，不把当前实现自动升级成设计意图；也不把历史建议当成已经实施的架构。
- 扫描允许得出“保留现状”。不要为了输出发现而要求每批都找到问题。

## 3. 批次清单

路径均相对仓库根目录。每行的目录是候选范围，入口是阅读起点；完成时必须列出实际覆盖与未覆盖部分。相关 design 文件均位于 `docs/design/`。

### A. 建立基线与基础设施

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B00 全局地图** · 已扫描 · [报告](B00.md) | 根 `Cargo.toml`、各 crate manifest、`app/editor/web/package.json`、`shader-packages.toml`、`cxx/CMakeLists.txt`、架构入口及 `layering-and-dependency-boundaries.md` | 建立真实依赖图、主要状态 owner/线程表、源码覆盖地图。核对 `app -> renderer -> engine`、CXX 单向消费与 shader owner；检查 feature/build-dependency/代码生成形成的隐含依赖。只建立边界和候选问题，不深入算法。 |
| [x] | **B01 基础工具与路径** · 已扫描 · [报告](B01.md) | `engine/e00-foundation/`；`engine/e00-utils/truvis-path/`、`truvis-fetch-res/`；`paths.toml`、`resources.toml` | 日志/配置/路径职责是否清楚；路径规则、默认值与资源定位是否多份；下载、校验、失败清理是否完整；是否把业务状态下沉到通用 utils。列出保留或收敛的具体 owner。 |
| [x] | **B02 构建与生成工具链** · 已扫描并完成局部修复 · [拆分与交接](B02.md) | `engine/e00-utils/truvis-shader-{manifest,build,binding-codegen}/`；`cxx/rust/tools/`；`justfile`、`scripts/nu/`、`.cargo/`、CMake presets/helpers、各 `build.rs` | B02-a CXX 已扫描，续接修复与验证见 B02-a2；B02-b1–b3 的编译、输入、binding、输出与状态发布已扫描；B02-c 资源缓存、Git/file/archive 发布恢复与路径契约已扫描并完成局部修复。检查 manifest、依赖发现、增量失效、部署和 codegen 职责，区分 Shader 构建、CMake 增量与 Cargo binding。 |
| [x] | **B03 Shader ABI 与 descriptor 契约** · 已扫描并完成局部修复 · [拆分与交接](B03.md) | `engine/shader/abi/`、`renderer/shader/abi/`、两层 `truvis-*-shader-binding`；`truvis-descriptor-layout-{macro,trait}`；`shader-abi-and-native-integration.md` | B03-a 已完成 14 份 layout、57 个 binding 与 31 个 SPIR-V 对照及宏/写入修复；B03-b 已完成共享结构布局、生成投影和 vertex input 核对及两项局部修复；B03-c 已完成逐边 include、owner 权限、编辑器根与 layer 标注校验及局部修复。同一 ABI 是否只有一个定义 owner；Rust/C++/Slang 是否手工镜像；allowlist、namespace、include 方向、layout/padding/stride、binding 和 descriptor 宏是否一致。选择关键结构沿 CPU 写入到 shader 读取检查；建立剩余 ABI 覆盖清单，未逐项核对的不能宣称全部验证。 |
| [x] | **B04 Native CXX 与 FFI** · 已扫描并完成局部修复 · [拆分与交接](B04.md) | `cxx/modules/`、`cxx/rust/bindings/`：utils、Assimp、Vulkan、Streamline；相关 module README | B04-a / a2 加载、路径及数据接口已修复验证；B04-b Vulkan 桥接已闭合，B04-c1 loader 修复已完成，B04-c2 状态/回调/路径已完成，B04-c3 feature ABI 与调用边界已完成；下一批 B05-a。exception/POD/opaque handle、allocation/free、线程与回调契约是否明确；Rust wrapper 是否真正收窄 unsafe；失败时谁保留/释放资源。按 Assimp / Vulkan / NVIDIA 分子批次，构建器机制归 B02，NVIDIA 跨层生命周期归 B16。 |

### B. GPU 基础能力与资源模型

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B05 Vulkan RHI** · 已扫描并完成局部修复 · [拆分与交接](B05.md) | `engine/e10-gfx/truvis-gfx/`：`gfx.rs`、`gfx_core.rs`、foundation、resources、commands、descriptors、pipelines、raytracing、swapchain | B05-a 首轮与 B05-a2 root 构造失败/VMA Drop 修复已完成，B05-b1 主机映射边界和 index 上传类型已修复验证；B05-b2 已完成叶子构造与图像描述修复，见 [报告](B05-b2.md)；B05-c1 同步对象与 query ABI 已修复，见 [报告](B05-c1.md)；B05-c1b 已完成 command/pool 与提交前失败局部修复，见 [报告](B05-c1b.md)，B05-c2 已完成 descriptor/graphics 创建失败修复，见 [报告](B05-c2.md)，B05-c2b 已完成 compute/RT 组合构造修复，见 [报告](B05-c2b.md)，B05-c3 已完成 SBT/SoA 算术及 AS 未提交构造修复，见 [报告](B05-c3.md)，B05-c3b 已完成 AS 输入与同步构建已知安全阶段修复，见 [报告](B05-c3b.md)，B05-c4 已完成同步上传 helper 与组合构造回收，见 [报告](B05-c4.md)，B05-d 已完成 WSI 叶子创建、metadata 与 acquire/present 状态修复，见 [报告](B05-d.md)，B05-d2 已完成 Presenter 组合构造和重建失败局部修复，见 [报告](B05-d2.md)，跨代 present semaphore 与退场缺口交 B12/B27；B06-a 已完成 handles/registry 扫描、延迟回收与诊断异常收尾修复，见 [报告](B06-a.md)，B06-b 已完成 slot 回收时序与 binding 组合构造修复，见 [报告](B06-b.md)，B06-c 已删除闲置 staging owner 并修复 FIF 构造/释放收尾，见 [报告](B06-c.md)，B07-a 已完成资源登记、声明合并与核心写分类修复，见 [报告](B07-a.md)，B07-b 已完成屏障与物理解析修复，见 [报告](B07-b.md)，B07-c 已完成 semaphore 聚合、名称前置校验与执行契约修复，见 [报告](B07-c.md)，B08-a 已完成加载任务、handle 与异常/等待边界修复，见 [报告](B08-a.md)，B08-b 已完成真实编解码验证与文档补充，见 [报告](B08-b.md)，B08-c1 已修复资源字节与 URI，见 [报告](B08-c1.md)，B08-c1b 已修复版本与必需扩展声明检查，见 [报告](B08-c1b.md)，B08-c2a 已完成 accessor 边界修复，见 [报告](B08-c2a.md)，B08-c2b 已修复几何生成与着色属性，见 [报告](B08-c2b.md)，B08-c2c 已修复 scene 层级与变换，见 [报告](B08-c2c.md)，B08-c3a 已修复材质参数与 alpha 阈值，见 [报告](B08-c3a.md)，B08-c3b 已完成纹理槽审阅与能力边界修复，见 [报告](B08-c3b.md)，B08-d1 已完成 Assimp 数据发布前置修复，见 [报告](B08-d1.md)，B08-d2 已修复材质转录，见 [报告](B08-d2.md)，B08-d3 已完成几何与生命周期修复，见 [报告](B08-d3.md)，B08-e1 已修复导出文件边界，见 [报告](B08-e1.md)，B08-e2 已完成烘焙语义、当前帧与 glTF 预检修复，见 [报告](B08-e2.md)，下一批 B09 GameWorld 与 SceneStore。承接 [B04-b](B04-b.md) 的 safe API/外部 Rc，以及 [B04-c3](B04-c3.md) 的 Streamline unsafe 调用前置。root owner、typed context 与 wrapper 是否有明确作用；unsafe 前提、创建失败、显式销毁、能力检查是否闭合；重复封装是否可以回到已有 owner。建议按 device/context、resource、command/pipeline 三个子批次，不在一轮逐行读完全部 RHI。 |
| [x] | **B06 GPU 资源句柄与公共契约** · 已扫描并完成局部修复 · [交接](B06.md) | `truvis-render-foundation/`；Runtime `resources/`、`bindings/`；`resource-lifecycle-and-destruction.md` | B06-a handles/registry 已完成局部修复，见 [报告](B06-a.md)；B06-b 已完成 slot/构造失败局部修复，见 [报告](B06-b.md)；B06-c 已完成 staging/FIF/scene/view 扫描与局部修复，见 [报告](B06-c.md)；下一批 B07-a。handle、registry、bindless、descriptor、sampler、staging、FIF data 的身份和回收职责是否重复；删除、slot 复用、容量扩展是否对所有消费者安全。交付资源创建/发布/退役/销毁链，区分 owner 与只读访问 trait；承接 [B03-b](B03-b.md) 的泛型字节转换、映射初始化与权限契约缺口。 |
| [x] | **B07 RenderGraph** · 已扫描并完成局部修复 · [交接](B07.md) | `truvis-render-graph/src/render_graph/`；`render-graph-and-data-flow.md`；代表性 pass 调用方 | B07-a 声明/编译入口已修复验证，见 [报告](B07-a.md)；B07-b 屏障与物理解析已修复，见 [报告](B07-b.md)；B07-c 执行/提交已完成，见 [报告](B07-c.md)；B08 约定范围已扫描，B09 接续 CPU scene 状态。pass 声明与实际 image/buffer 访问是否一致；graph 管理范围之外的 buffer、AS、外部 SDK 同步由谁负责；import/export、layout、semaphore、提交失败是否闭合。核对线性 pass 顺序与自动 barrier 的实际边界，不预设必须引入调度器或 aliasing。 |

### C. CPU 场景到 GPU 场景

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B08 Asset 与导入** · 已扫描并完成局部修复 · [交接](B08.md) | `truvis-asset/` 的 load service/worker、glTF/Assimp/texture loader、handle、material texture；`scripts/scene/` | B08-a 加载任务与完成边界已修复验证，见 [报告](B08-a.md)；B08-b 已完成，见 [报告](B08-b.md)；B08-c1 已修复，见 [报告](B08-c1.md)；B08-c1b 已修复，见 [报告](B08-c1b.md)；B08-c2a 已修复，见 [报告](B08-c2a.md)；B08-c2b 已修复，见 [报告](B08-c2b.md)；B08-c2c 已修复，见 [报告](B08-c2c.md)；B08-c3a 已修复，见 [报告](B08-c3a.md)；B08-c3b 已完成，见 [报告](B08-c3b.md)；B08-d1 已完成，见 [报告](B08-d1.md)；B08-d2 已完成，见 [报告](B08-d2.md)；B08-d3 已完成，见 [报告](B08-d3.md)；B08-e1 已修复导出文件边界，见 [报告](B08-e1.md)，B08-e2 已完成烘焙语义、当前帧与 glTF 预检修复，见 [报告](B08-e2.md)，下一批 B09 GameWorld 与 SceneStore。内容身份、去重、请求与完成状态是否清楚；失败/删除/晚到结果能否留下脏状态；不同导入器的坐标、单位、材质转换是否重复且不一致。区分外部格式差异与不必要的兼容设施。 |
| [x] | **B09 GameWorld 与 SceneStore** · 已扫描并完成局部修复 · [交接](B09.md) | `truvis-world/`：`lib.rs`、`scene_store.rs`、`asset_system.rs`、components、edit error、light parameters；`scene-sync-and-render-world.md` | B09-a1 纹理请求、显式重试与请求发布顺序已修复，见 [报告](B09-a1.md)；B09-a2 已修复 scene ingest 部分发布与诊断异常恢复，见 [报告](B09-a2.md)；B09-b 已完成 instance/material 校验与引用/版本一致性复核，见 [报告](B09-b.md)；B09-c 已完成灯光注册校验、Rect 绕序与剩余聚合契约复核，见 [报告](B09-c.md)；下一批 B10 CPU-to-GPU 资源桥接。CPU 唯一权威是否落实；资源与实例身份、共享材质与实例绑定是否混淆；变更是否原子、非法输入能否部分写入；revision/dirty 与最终状态对账的语义是否重复。核对 procedural mesh 等辅助逻辑归属。 |
| [x] | **B10 CPU-to-GPU 资源桥接** · 已扫描并完成局部修复 · [交接](B10.md) | Runtime `render_world/` 的 `render_asset_system.rs`、`gpu_*_store.rs`、`gpu_asset_upload_queue.rs`、`sky_distribution_builder.rs`、resolvers | CPU ready、上传中、GPU ready、frame visible 是否混成一个状态；异步结果是否验证身份/版本；材质与纹理依赖、fallback 替换、sky worker 是否重复实现调度。重点追一条删除/替换发生在上传中的路径。  承接 [B09](B09.md) 的 CPU Ready/失败重试、资源删除和材质修改到 GPU 发布/依赖唤醒的验证。  B10-a1 查询错误与关闭等待失败已修复，见 [报告](B10-a1.md)；B10-a2 已完成构造/准备回收与提交接管，见 [报告](B10-a2.md)，B10-a3 已完成结果保留与 texture 发布回滚，见 [报告](B10-a3.md)，B10-b1 已修复 mesh 查询/等待失败边界，见 [报告](B10-b1.md)，B10-b2 已完成准备回滚与提交接管，见 [报告](B10-b2.md)，B10-b3 已完成 mesh 结果/通知保留与退役收尾，见 [报告](B10-b3.md)，B10-c1 已完成 material FIF buffer 构造与关闭回收，见 [报告](B10-c1.md)，B10-c2 已完成 material 通知、slot 回收与上传确认修复，见 [报告](B10-c2.md)，B10-d1 已完成 sky worker 请求、结果与关闭修复，见 [报告](B10-d1.md)，B10-d2 已完成 sky 上传发布与 binding 通知修复，见 [报告](B10-d2.md)，B10-d3a 已完成 sky fallback 构造与关闭修复，见 [报告](B10-d3a.md)，B10-d3b 已完成组合构造与关闭修复，见 [报告](B10-d3b.md)，B10-e 已完成跨 queue wait 与 image/mesh 消费屏障修复，见 [报告](B10-e.md)；受控夹具、编译和无窗口 buffer GPU 探针通过，完整 image/RT/raster 回归未验收。下一批 B11 RenderWorld 派生场景。 |
| [x] | **B11 RenderWorld 派生场景** · 已扫描并完成局部修复 · [拆分与交接](B11.md) | Runtime `render_world/` 的 instance table、render data、buffers、scene TLAS、analytic/emissive light table、raster cache、environment binding | 完整对账、stable slot、FIF 镜像、TLAS revision 是否各司其职；场景缓存是否成为第二权威；删除/材质替换/mesh ready 是否同步影响 raster、RT、灯光与拾取映射。交付 prepare 输入到 scene root 的数据链。 B11-a1 已完成 instance table 内候选/变化标记保留和 slot 退役回收修复，见 [报告](B11-a1.md)；B11-a2 已完成 CPU/资源通知交接、下游失效登记与历史提示保留，见 [报告](B11-a2.md)，闭合 [B10-b3](B10-b3.md) 的通知交接待办；B11-b1 已完成 SceneBuffers 生命周期与扩容交接，见 [报告](B11-b1.md)，B11-b2 已完成矩阵录制与 FIF/历史确认，见 [报告](B11-b2.md)，B11-b3 已完成结构表索引校验、有效前缀复制与 root 转录核对，见 [报告](B11-b3.md)，B11-c 已完成 TLAS 关闭收尾与当前快照映射核对，见 [报告](B11-c.md)，B11-d1 已完成 analytic 生命周期与容量/上传修复，见 [报告](B11-d1.md)，B11-d2a 已完成 emissive 生命周期与候选扩容修复，见 [报告](B11-d2a.md)，B11-d2b 已完成 emissive 映射约束与环境交接复核，见 [报告](B11-d2b.md)，B11-e 已完成组合生命周期修复与 analytic 返回值简化，见 [报告](B11-e.md)；B11 约定范围闭合，下一批 B12。 |

### D. 帧循环、平台和 Renderer 编排

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B12 Runtime 与阶段能力** · 已扫描并完成局部修复 · [拆分与交接](B12.md) | Runtime `render_runtime.rs`、`render_runtime_ctx.rs`、state、present、`ray_cast/`；`truvis-render-loop/`；`runtime-phase-contract.md` | Runtime/RenderLoop/Renderer 是否重复推进状态；Ctx 是限制能力还是只换名字；prepare、after_prepare、submit、present 与 history commit 是否有唯一顺序；无 present、失败和跳帧路径是否一致。承接 [B10-a2](B10-a2.md) 的不确定提交与未发出 signal 时的外层退场策略。承接 [B11-c](B11-c.md) 的 RayCastService 构造、提交、等待/readback、关闭及当前 prepare 阶段前提。 |
| [x] | **B13 线程与窗口宿主** · 已扫描并完成局部修复 · [拆分与交接](B13.md) | `truvis-render-thread/`、`truvis-winit-host/`；`frame-execution-and-window-lifecycle.md`、`threading-and-synchronization.md` | startup/panic/join、resize 合并、最小化/零尺寸、输入积压、关闭时 pending 工作是否收敛；窗口是否晚于 Vulkan 回收；standalone/embedded 共享和差异是否放在正确层。 |
| [x] | **B14 Renderer 组合与配置** · 已扫描并完成局部修复，见 [报告](B14.md) | `truvis-renderer/src/truvis_renderer.rs`、`renderer_client.rs`、`view_accum.rs`；`renderer-kit/`；`render-configuration-and-temporal-state.md` | Renderer、Client、Subsystem、Controller 的边界是否明确；配置 requested/effective、frame snapshot、累积签名是否各有 owner；UI 可见性是否错误控制业务更新；过多布尔组合是否允许非法状态。具体 DLSS 归 B16。 |
| [x] | **B15 渲染子系统与 pass 组织** · 已扫描并完成局部修复，见 [拆分报告](B15.md) | `renderer-rendering/` 的 realtime、offline、post_process、shared；`renderer-render-passes/` 的 compute、ray_tracing、effects/phong、post_process；对应 raster/post shader | 分成 **B15-a Realtime/Offline target 与 pass 编排**、**B15-b 后处理、曝光与显示链**。检查重复资源创建、pass 模板、RT pipeline、target 包装和设置转换；共享的依据是否是相同语义；模式切换、clear/accum/resolve、色彩空间、曝光历史是否一致。承接 [B08-c3b](B08-c3b.md) 的 min/mag/LOD、doubleSided 与 occlusion 跨层缺口；BSDF/估计器正确性转 B17–B20。 |
| [x] | **B16 DLSS SR/RR 集成** · 已扫描并完成局部修复，见 [拆分与交接](B16.md) | `truvis-renderer/src/dlss.rs`；DLSS/Streamline pass；Gfx 与 CXX Streamline 接入点 | 跟踪 requested/effective → capability → feature → tags/evaluate → reset/release。检查 SR/RR 状态切换；render/output extent、MV、jitter、guide 的契约；release 失败是否保留原状态；Streamline 与 Vulkan 销毁顺序。接续 [B04-c3](B04-c3.md)：异常后帧输出/恢复、shutdown 失败的退出政策、manual hook/unpacked 能力范围和上层 safe API 的阶段前置。分别记录静态契约和实际 SDK/GPU 证据。 |

### E. 渲染算法与跨端语义

这些批次执行时须按 `rendering-reference.md` 核对 `resources.toml` 指定的参考版本及实际 commit。这里只列检查项，不对算法正确性作结论。

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B17 材质与 BSDF** · 已扫描，局部修复与模型缺口见 [汇总与交接](B17.md) | `engine/shader/lib/engine/lighting/pbr.slangi`、scene/material access；Renderer raygen material、surface hit；CPU 材质数据与 ABI | 材质语义、eval/sample/pdf/throughput 是否同源；Realtime/Offline/ReSTIR/SHARC 是否私自维护公式；分支概率、delta、eta、法线、透射/透明度约定是否完整。区分 OpenPBR 目标语义、已实现子集和已知近似。承接 [B08-c3b](B08-c3b.md) 的 Surface F0/IOR、rough transmission、normal transform/切线基与双面法线问题，以及 [B08-d2](B08-d2.md) 的 Assimp opacity 映射与未消费材质能力。 |
| [x] | **B18 光源采样与路径积分** · 已扫描，局部修复与数值/模型缺口见 [汇总](B18-d.md) | Engine core sampling/MIS；Renderer lighting/env map、ray numeric、raygen direct lighting/path state；Realtime/Offline entry | 承接 [B17-d](B17-d.md) 的路径估计器/roulette/消费契约及未决数值边界；sky/analytic/emissive 的选择概率和概率度量是否统一；NEE、continuation miss、emission、Russian roulette 是否漏计或重复；随机数、射线偏移和有效性策略是否散落。按完整估计器链验证，不只比较局部公式。 |
| [x] | **B19 ReSTIR DI** · 已扫描并完成局部修复 · [汇总与交接](B19.md) | `renderer/shader/lib/renderer/realtime_rt/restir_di.slangi`、GBuffer/surface key；Realtime reservoir resources、相关 pass | 承接 [B17-c2a](B17-c2a.md) 的 surface key 缺少几何法线问题，补齐持久化/重建与资源契约；承接 [B18-d](B18-d.md) 的 initial PDF floor、raw weight/finalize 数值范围与跨视点度量；target/proposal/原始权重/归一化/时空复用/可见性修正是否配套；motion、surface identity、buffer ping-pong、reset 是否一致；是否重复承担主路径采样职责。与 B15、B17、B18 只交换明确的输入输出契约。 |
| [x] | **B20 SHARC 与调试** · 已扫描 · [拆分与交接](B20.md) | Renderer `sharc_*` shader、Realtime SHARC resources、raygen debug state；`radiance-cache-and-debug.md` | cache 保存的量、更新/查询、能量权重、early termination 是否一致；历史失效是否覆盖相机/材质/光照变化；debug 路径是否改变业务状态；是否存在第二套 BSDF 或不可解释的状态开关。性能结论必须另有测量。承接 [B16-c](B16-c.md) 的 SharcTargets 首次 clear 提交前 buffer 分配前缀回收，作为独立局部修复，不推迟到全局重构。 |

### F. Editor、Automation 与产品入口

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B21 Editor 后端与协议边界** · 约定范围已扫描 · [拆分与交接](B21.md) | `renderer/editor/truvis-editor-bridge/`；`app/truvis-app/src/editor_ipc.rs`、`client/editor_controller.rs`、`client/desktop_command.rs`；`editor-boundary-and-consistency.md` | typed ports、envelope、DTO、controller 是否各有用途；转换/验证是否重复；队列满、timeout、已入队但未完成、通知丢失和全量恢复是否明确；scene version 与 CPU mutation 完成是否一致。 承接 [B09-a2](B09-a2.md) 的 import 状态查询/释放语义和 App 批量实例创建失败边界。 |
| [x] | **B22 MCP/Automation** · 约定范围已扫描 · [拆分与交接](B22.md) | `app/truvis-app/src/mcp_server.rs`、`client/world_automation_controller.rs`；bridge automation protocol；`agent-integration.md` | HTTP runtime、独立请求队列、RenderThread budget、expected version、导入状态、取消与 shutdown 是否闭合；和 Editor 是否重复业务规则；共享通道机制是否保留两条 lane 的隔离。不要把 CPU ACK 当成 GPU/像素完成，不将协议规定的版本处理直接判作冗余兼容层。 |
| [x] | **B23 Web Editor** · 已扫描 · [拆分与交接](B23.md) | `app/editor/web/src/`：transport、state、components、protocol/generated；Web 构建配置 | server snapshot、编辑草稿、selection、dirty/error 是否清楚分离；异步响应能否覆盖新状态；刷新恢复与订阅清理是否完整；hook/组件/参数控件是否过度拆分或重复。生成 DTO 查源头和消费方，UI 检查按 editor-ui-style 规则。 |
| [x] | **B24 拾取、Gizmo 与渲染 UI** · 已扫描（B24-a、B24-b） | Runtime ray_cast/selection、Engine raycast shader；Renderer selection/outline/overlay/gizmo；`renderer-imgui/`、`renderer-render-ui/` 及 UI shader | B24-a 已复核输入拾取到 selection/outline 的 owner 链并修复 GUI scissor 与 canvas 的边界交集，见 [B24-a](B24-a.md)；B24-b 已复核 ImGui 输入/DPI、控件归一化与资源生命周期，动态 DPI 传播缺口转交 B25，见 [B24-b](B24-b.md)。 |
| [x] | **B25 App、场景和示例** · 已扫描 · [拆分与交接](B25.md) | `app/truvis-app/` 启动与 desktop/client 组合；`app/truvis-scenes/`；`app/samples/`、`renderer/samples/`、sample shader | App 是否仅承担平台、产品场景及 CPU Client 业务；Truvis/Cornell 是否重复完整 Renderer；场景装配/导入状态/命令行默认值是否多份；Triangle/ShaderToy 的简单路径是否被主 Renderer 需求过度耦合。B25-a 已修复 DPI 传播及字体重建；B25-b 已复核场景与 sample 集成并修复 layout 构造失败回收。样例 shader 作品以集成契约为审阅范围，不逐个审计艺术算法。 承接 [B09-a2](B09-a2.md) 的 sample 部分实例创建与重复消费风险。 |

### G. 项目整体复核

| 完成 | 批次 / 状态 | 范围与入口 | 本批重点与交付 |
|---|---|---|---|
| [x] | **B26 全局冗余与抽象复核** · 已扫描，仅方案 · [报告](B26.md) | B00 地图、B01–B25 的结论与跨批次问题，再回到相关源码 | 复查模块局部“合理”的设计组合后是否重复：配置归一化、错误模型、资源 wrapper、Ctx/trait、协议映射、材质/采样公式。每个候选给出“保留/局部收敛/删除/重划职责”、最小修改范围及会失去的保护。不同报告的同根问题合并，不能只拼接发现列表。 |
| [x] | **B27 全局严密性与验收计划** · 已扫描，仅方案 · [报告](B27.md) | 全部报告中的生命周期/一致性契约；下方端到端场景 | 承接 [B16-c](B16-c.md) 的异常退出、SDK shutdown 失败后 root 销毁及组合构造 unknown submission 边界。 承接 [B17-b2](B17-b2.md) 的层能量、多次散射与 RR rough guide 统一模型提案；B17-d 汇总，最终只给方案；同时保留 [B17-c1](B17-c1.md) 的临界精度与 IOR 支持域边界。 承接 [B17-c2b](B17-c2b.md) 的几何侧约束、法线扰动能量、切线 UV/烘焙约定与双面状态提案，不实施全局模型迁移。 承接 [B17-c3](B17-c3.md) 的 smooth diffuse 丢失、rough BTDF 缺失与实际 radiance 溢出/下溢支持域。 统一模型的 owner、联动范围与验收提案见 [B17-d](B17-d.md)，最终只输出方案。用完整状态链复核跨模块断点，检查整改建议相互冲突或形成循环依赖。输出最终问题优先级、独立可回滚的整改批次、需保留的不变量、验证方法和未验证风险。尚未覆盖的模块明确列出，不能称为全仓已通过。 承接 [B09-a2](B09-a2.md) 的 import record 保留期与 source/content 版本策略，只给方案。  承接 [B09-c](B09-c.md) 的 u32 light revision 饱和容量边界，只评估方案。  承接 [B10-a2](B10-a2.md) 的失败 submit/等待与外层终止策略，只给方案。 |

## 4. 全局复核必须走通的场景

“走通”先指完整静态追踪；实际运行结果单独标注。每条链检查正常路径、失败出口和资源归属。

| 场景 | 必须追踪的链与反例 | 主要批次 |
|---|---|---|
| 启动和中途失败 | App → host/thread → Runtime/Gfx → Renderer；中间创建失败后，已创建对象如何退出 | B04–B06、B12–B13、B25 |
| 导入后删除或替换 | load → CPU 发布 → GPU upload → binding → instance/TLAS；晚到结果不能复活已删除对象 | B08–B11 |
| 编辑材质、灯光、环境 | Web/MCP → CPU mutation/version → prepare → GPU 数据 → temporal reset；CPU 接受不等于画面完成 | B09–B11、B14、B17–B23 |
| 相机与渲染模式变化 | requested → effective → frame input → resources/history → 实际 submit；跳帧和失败不能错误推进历史 | B12、B14–B16、B19–B20 |
| Resize、最小化与恢复 | latest size → 安全点 → swapchain/targets → SDK feature → present；包括零尺寸和连续尺寸变动 | B05–B07、B12–B16 |
| 拾取后场景变化 | 输入与显示坐标 → 当前 prepare 快照 → readback slot 映射 → selection/编辑；对象删除和 slot 复用 | B09、B11–B14、B24 |
| 队列满、超时与关闭 | Web/MCP/worker producer → bounded queue → consumer → completion；timeout 后 mutation 是否仍可能执行 | B08、B10、B13、B21–B23 |
| 完整 shutdown | 停止接收 → 结束 CPU 工作 → GPU 安全点 → SDK/Renderer → Runtime/Gfx → thread join → window | B04–B06、B10、B12–B13、B16、B21–B22、B25 |

## 5. 单批完成条件与报告模板

完成条件：

- [ ] 已注明基准 commit、相关未提交改动及本批范围；实际阅读文件/符号和未覆盖范围可定位。
- [ ] 已解释本批主要 owner、线程/阶段、调用方和失败出口；复杂链没有只看被调用函数。
- [ ] 三个核心维度均有结论：冗余、过度抽象、严密性；可以是“本范围未发现有充分证据的问题”。
- [ ] 每个确认问题都有触发条件、源码证据、后果和最小建议；保留理由与反证已考虑。
- [ ] 已区分确认事实、推断、待运行验证；给出验证动作，不把编译或静态阅读写成运行验收。
- [ ] 跨批次问题已注明目标批次；交接已写入文件，状态已更新。
- [ ] 每个问题已有处置状态；执行过修复的已回填改动和验证结果，受影响旧结论已标记待复核。

在本目录新增 `Bxx.md`，可用下面的简短结构；没有内容的项目省略，不生成空报告：

```markdown
# Bxx：批次主题

基准：日期、HEAD、相关未提交改动。
范围：实际已读文件/符号、抽查范围、明确未覆盖内容。
依据：相关设计约束与调用入口；若偏离，分别写出规范与实现。

## 边界与契约
主要 owner、调用/依赖方向、状态与生命周期；仅记录后续判断必要的信息。

## 发现
### 问题标题（优先级；已确认/推断/待验证）
- 证据：仓库相对路径、符号和当前行号；关键调用方/消费方。
- 触发与影响：具体条件 → 当前行为 → 破坏的约束或维护负担。
- 保留现状的理由/反证：为什么仍判断需要调整，或为何撤回疑点。
- 最小建议：所属 owner、改动范围、保持/改变的契约、代价。
- 验证：已执行及结果；仍需怎样证明修复有效。
- 处置：处理状态、本批修复或目标批次；延期/暂不处理的原因。
- 修复记录（执行后补充）：修改位置、保持/改变的契约、修复后基准、验证结果与剩余风险。

## 保留与未决项
看似重复但有必要的设计；缺失证据；转交目标批次与具体问题。

## 下一批交接
重要结论、未完成范围、待修复/待验证项、受影响旧报告与需复核源码、下一步入口。
```

优先级与确信程度分开记录：

- **P0**：已证实的严重破坏性问题，需要立即处理；不凭假设使用。
- **P1**：可导致错误结果、生命周期/同步失效、状态不一致或关键失败路径不可收敛。
- **P2**：有具体维护代价的重复逻辑、职责错位、无收益抽象或 API 约束缺口。
- **P3**：局部可读性、命名或文档问题，优先级低于行为与架构风险。

不能证明是 bug 的设计建议仍可记录，但不得伪装成已确认故障。暂不调整的问题写清原因，不要求统一重构。

## 6. 验证与结论边界

扫描阶段优先静态阅读和已有证据；按问题需要执行既有检查，不为每批固定重建整个项目。下面是后续验证入口，不代表本清单创建时已执行：

| 要证明的事项 | 可用入口 / 方法 | 不能由它证明的事项 |
|---|---|---|
| Rust 依赖与类型约束 | `cargo metadata --no-deps --format-version 1`、相关 crate 的 `cargo check`；最后视需要检查 workspace | GPU 完成、实际线程退出、SDK 生命周期 |
| Shader 构建与 ABI | `just shader`；必要时 `just shader-force`；SPIR-V offset/stride 与宿主布局对照，按规则使用 `spirv-dis` / `spirv-val` | 估计器无偏、数值稳定、画面正确 |
| Native 构建及部署 | `just cxx-debug`；需要两种配置时 `just cxx`；核对 public C ABI 和部署产物 | feature release 成功、动态 SDK/设备路径正确 |
| Web 协议与构建 | `just editor-web`、实际交互和响应乱序/恢复场景 | 只凭前端 build 不能证明后端一致性 |
| 完整入口 | 先查 `justfile`，选择 Triangle、ShaderToy、Cornell 或 Truvis；全构建入口 `just build-all` | 一次成功启动不能覆盖失败、resize、切换与 shutdown |
| GPU、算法与性能 | validation、已有数值检查、固定场景图像对照、同等画质下的 profiling | 静态复杂度、少几层调用或参考实现相似不能替代这些证据 |

命令依赖环境和生成产物；失败时先区分环境缺失与代码错误。检查前后对照 Git 状态，记录生成文件变化，保留用户改动。新增单元测试需要用户明确要求。

第三方源码、下载资源、生成绑定/TS DTO、build 输出不做普通重复代码清理：第三方只按引用与适配边界审阅，生成物追溯生成源。B00 要核对所有 workspace member、native module、shader package 和 Web/脚本入口均已归入批次；之后新增模块补入清单，避免清单本身过期。

## 7. 新会话可直接使用的提示词

将下面的 `Bxx` 换为待执行批次；已有未完成修复时先续接交接。仅需分析时，在提示词末尾明确追加“本批仅审阅，不执行修复”。

```text
请执行 E:\code_acc\Render-Rust-vk-Truvis\docs\review\code-design\README.md 中的 Bxx 批次。

先读当前 AGENTS.md、递归读取 .agents/rules 下全部 Markdown、docs/ARCHITECTURE.md，
以及该批相关 design、模块 README 和已存在的直接相关批次报告。
核对 HEAD 与相关未提交改动，不把旧报告或聊天记忆当成当前实现事实。

先做该批代码设计审阅，重点检查冗余、过度抽象、职责边界和设计严密性。
对每个问题给出路径/符号/行号、触发条件、影响、反证或保留理由、最小建议和验证方式。
不要仅凭单实现 trait、薄包装、派生状态或文件长度判断过度设计。
区分设计意图、必须保持的约束和当前实现；无法证明的标为推断或待验证。

先将审阅证据写入 docs/review/code-design/Bxx.md，再按清单的准入条件执行已确认、
边界清楚的局部修复；适用时先输出架构预检，保持既有设计不变量。
跨模块问题先完成相关审阅再集中处理；全局问题留到 B26/B27 复核，只输出方案，不实施最终全局修改；不实现证据不足的建议。
完成必要验证后回填问题处理状态、修改位置、修复后基准和验证缺口，更新清单状态，
并将受影响旧报告结论标为待复核；同步维护直接相关 design/模块 README。
不主动新增单元测试，不自动更新 docs/summary/；每批验证完成后自动 commit，最多两次，不推送 Git。
若范围过大，按独立 owner 或状态链拆子批次，写清覆盖与未覆盖部分；不要勾选未完成范围。
跨模块疑点记录目标批次，不自动扩展审阅。结束时给出可供新会话继续的简短交接。
```
