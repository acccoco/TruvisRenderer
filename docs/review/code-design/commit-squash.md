# refactor 按子系统压缩记录

2026-10-30，按用户确认将全部 34 个未推送提交合并为 7 个子系统提交。
本次仅重组历史；以子系统作为审阅和回滚单位，替代此前逐行为拆分要求。

- 基准：`9527799b7ca72a25ffa715b6a6ec143f72b7c65d`。
- 压缩前 HEAD：`649943974f03dfc1ee168604be28ced9887ec4cb`。
- 此次旧历史保留于 `codex/backup-refactor-before-subsystem-squash`；更早的 `codex/backup-review-before-squash` 保持原样。
- 不 push；`master` 与远端引用保持原位。`refactor` 只在验证通过后更新。
- 最终除本映射文档外，所有受版本控制的文件内容、模式和子模块指针与压缩前 HEAD 完全一致。

## 本次分组

| 组 | 新提交 | 标题 |
|---|---|---|
| 1 | `db8ef894b47b152056c911c5ea454e8d13b7179a` | [fix] 完善构建工具链与资源获取 |
| 2 | `9727e88e3c3a3b18e25923823e5d557a47796ca9` | [fix] 收紧 Shader ABI 与 Native FFI 契约 |
| 3 | `6855ff061a7ee01cd5ba0aeb31f8d8f0f2361d79` | [fix] 统一资产导入降级与场景导出校验 |
| 4 | `365f00db8643806f48a2c97bda041c478d608ca4` | [refactor] 收敛 GPU 与 Runtime 生命周期和场景同步 |
| 5 | `890d83b610a89ecb905114fdea3768897a54f54c` | [fix] 完善光输运与 ReSTIR SHARC 历史处理 |
| 6 | `93cad1030fa5f97800c4394442189dbff1445d01` | [fix] 完善 Editor 输入与 Automation 集成 |
| 7 | 本文件最近一次修改所在提交 | [docs] 汇总审阅记录与提交压缩映射 |

公共签名、ABI 与必要调用方适配同组提交。生命周期清理先适配原有 RT 资源布局，
随后第 5 组整体引入算法、历史资源与配套读取契约；不重新引入已删除的回滚脚手架。
DLSS jitter 的机械清理并入第 4 组，ImGui DPI 清理并入第 6 组。
设计文档和模块 README 随所属组提交；审阅证据集中在第 7 组。

## 验证与限制

- 七个提交分别执行 `cargo check --workspace --all-targets`。
- 第 1、2、3 组执行 `just cxx-debug`；第 1、2、4、5 组执行 `just shader-force`，各验证 31 个 SPIR-V（`--uniform-buffer-standard-layout`）。
- 切换历史后出现旧 native binding 元数据与源码不一致；只清理相关 crate 后重新检查，第 2 组也补做了 binding 重建后的完整检查。
- 临时脚本、逐组日志、SPIR-V 反汇编和最终结构核对位于 `.temp/subsystem-squash/`。
- 最终文件等价仅证明压缩没有改变实现；没有新增 GPU 画面或性能证据，没有新增单元测试。
- 保留先前未完成项：Bistro/office/Sponza 退化拓扑导入拒绝、SDK overlay 下 present 同步错误、多帧 HDR/收敛/NaN/Inf 与分阶段 GPU 性能验证缺口。
- SHARC Snapshot、现有法线模型、swapchain 跨代 semaphore/presentation 完成证明缺口保持不变。
- 每组可作为整体审阅或回退；不承诺从后续依赖链中单独反向应用任意组都没有冲突。

## 34 个旧提交的归属

同一旧提交可能涉及多个子系统。表中组号指向上表的新提交；前 12 个旧提交中的审阅文档集中归入第 7 组。
旧实验 hash 与第三方版本号保留其原始证据含义，不机械替换为新 hash。
第 7 组可用 `git log -1 --format=%H -- docs/review/code-design/commit-squash.md` 定位，避免自引用 hash。

| 旧提交 | 新组 | 原标题 |
|---|---|---|
| `9a97411ae2133a1a831ee85e3da96f7d94dca883` | 1、7 | [fix] 完善构建工具链与资源发布 |
| `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | 2、7 | [fix] 收紧 Shader ABI 与 Native FFI 契约 |
| `539436ead349a1ae325f2d2327a4893aa526adf5` | 4、7 | [fix] 完善 GPU 资源管理与 RenderGraph 同步 |
| `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | 3、7 | [fix] 完善资产导入与场景导出校验 |
| `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | 4、7 | [fix] 保证 CPU 到 GPU 场景同步与资源交接 |
| `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | 4、7 | [fix] 完善 Runtime 退出与窗口输入生命周期 |
| `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | 4、7 | [fix] 完善 Renderer 资源与 DLSS 集成 |
| `605e46f98296f28b50cecb315d10666babcde21a` | 5、7 | [fix] 修正材质求值与路径积分契约 |
| `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | 5、7 | [fix] 完善 ReSTIR 与 SHARC 估计及历史管理 |
| `ec287c305aa77ea5297dc8b9128c2cc692959a52` | 6、7 | [fix] 完善 Editor 与 Automation 请求一致性 |
| `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | 6、7 | [fix] 完善 ImGui 与 sample 集成 |
| `87d3d5c7c899fb123f9a5d3213202067323533e4` | 7 | [docs] 汇总全局设计复核与提交压缩记录 |
| `25967d90f4684a7db96549017e5365328b78cef3` | 4、5 | [refactor] 移除逐层 unwind 回滚并保留正常 GPU 生命周期 |
| `ea0a00e250d846c1ec3d408b8647992e9dddad8e` | 4 | [fix] 收敛 sky worker 失败并发布 generation 安全的 fallback |
| `d9610aaa0c3e0212a0cf9b6659bb30de11751b70` | 4 | [fix] 在 swapchain 重建后补齐 render-complete semaphore 容量 |
| `902bfc9fe8e02f54a822f8590556eb34cf974b74` | 4 | [fix] 区分 mapped buffer 初始化与 staging/SBT 只写路径 |
| `227d92f48ec2c14ca254f3e28a8257e6f269b4be` | 4 | [refactor] 删除 DLSS evaluate 不可达恢复状态并保留致命错误 |
| `bafdbf95a138b63e66744600862d2117cd43965d` | 6 | [fix] 允许数值手输精度并统一 Enter 和失焦校验 |
| `69b61df213734793499cd56c8d12c5ff36018150` | 6 | [refactor] 将输入状态透传规则收敛到 InputEvent 与 ImGui |
| `f40eef0a51c5f58cbf61ca174115cea04a8ac0ea` | 1 | [fix] 按单任务产物恢复 shader 增量构建并简化 DLL 指纹 |
| `a0b3b5e1d727799faad887eb93f95aabffabd4bb` | 1 | [fix] 在 HTTP 缓存访问前验证目标 receipt |
| `0a9a01d383dc4bf885340cf84b8e2fd7bd6bfcbe` | 1 | [fix] 按固定与浮动引用安全复用 Git checkout |
| `ab332c220540c46b362a3dbdeece70c534c3986b` | 3 | [fix] 将 glTF 不支持语义降级并保留结构校验 |
| `cf2db83c7cd9135704b836815b6dc600168bdac4` | 3 | [fix] 为 Assimp 静态语义和纹理映射提供明确降级 |
| `fa24e1bfcf02e85e2d96fb40bf6bfe21837541f5` | 5 | [fix] 让 SHARC 普通场景编辑保留渐进历史 |
| `869b0788adbf7bdef3397d3bf57508b3dfa6119b` | 5 | [refactor] 删除 SHARC 不可达的相邻 level 重投影 |
| `a06ed0d87393ba89debbc8829018023c5e476367` | 5 | [refactor] 将 ReSTIR 局部权重改为 float 并锁存溢出失败 |
| `40f0b946fc4665b9bd03b2bfb87248d0da10e31f` | 5 | [fix] 让 ReSTIR 复用权重与归一化保持原始有效 M |
| `b82f52a9fa2d513b92c6192fc922f9e6ef4324ce` | 5 | [refactor] 简化 spot 余弦衰减并保护硬锥边界 |
| `6e42e605a3d0f19b1c24a67783271ba2191e08f9` | 4、6 | [refactor] 清理重复 DPI 判断与 jitter 计数临时变量 |
| `af3af5509bfe763c4e4dc4f54c982a8a410f91a0` | 4 | [fix] 恢复 Runtime 顺序初始化的 Tracy 阶段埋点 |
| `caefe89a7f6154fb8d952ecf9bbfd31480c0f99e` | 4 | [docs] 清理旧回滚契约与审阅编号引用 |
| `1f261345f3dfde01dd031d8e074ec4361ebacdde` | 1 | [docs] 同步 shader 中断恢复的逐任务有效性契约 |
| `649943974f03dfc1ee168604be28ced9887ec4cb` | 4 | [refactor] 机械清理 Streamline 配置文件尾空行 |

---

以下完整保留上一次 239→12 压缩记录。其“当前分组”等措辞只描述当时历史状态；本次有效分组以上表为准。

# 审阅提交压缩映射

2026-10-30 将 `refactor` 的全部 239 个审阅提交按类型合并为 12 个；保留原有顺序，未实施新的代码修复。

- 基准：`9527799b7ca72a25ffa715b6a6ec143f72b7c65d`；`master` 保持此位置。
- 原 HEAD：`f33a5bb4b1cc8f894e88d6d99fbd5315e167ff19`，由本地分支 `codex/backup-review-before-squash` 保留。
- 第 1–11 组与对应原结束节点的完整 Git tree 相同；第 12 组只额外添加本映射、清单导航和报告历史说明。
- 最终源码、配置、锁文件、生成物及其他非审阅文档内容与备份逐项一致。
- 按本次用户确认，以完整功能范围为审阅/回滚单位，放宽此前单一行为提交粒度。没有重排或拆改实现。

## 引用规则

各报告原项目 hash 是压缩前证据基准，不能机械替换成新 hash：新提交对应一组的最终快照，
可能已经包含原实验对照中尚未应用的后续修复。第三方仓库 hash、输入摘要和 ABI 记录原样保留。
旧的“本报告所在提交”、原始提交次数和分组描述按备份历史解释；本文件下表则描述当前 12 组。
表中“本文件所在提交”指第 12 组；可用 `git log -1 --format=%H -- docs/review/code-design/commit-squash.md` 定位。
备份分支用于保留全部旧对象和实验可复现性，不推送、不删除。

## 当前分组

| 组 | 新提交 | 原提交数 | 原结束快照 | 标题 |
|---|---|---:|---|---|
| 01 | `9a97411ae2133a1a831ee85e3da96f7d94dca883` | 12 | `725b7ed8a0296b3efacd7225f8977de59dbed48b` | [fix] 完善构建工具链与资源发布 |
| 02 | `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | 18 | `bad7fae9cd661a8a1eac1b817b53dd8d6ac6c00c` | [fix] 收紧 Shader ABI 与 Native FFI 契约 |
| 03 | `539436ead349a1ae325f2d2327a4893aa526adf5` | 38 | `4b0c4cd3ae5326f7599d7a7534b51afef67d4e87` | [fix] 完善 GPU 资源管理与 RenderGraph 同步 |
| 04 | `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | 23 | `b40b0c74010d5b220ef4e2bedfee01c8ae5c0fba` | [fix] 完善资产导入与场景导出校验 |
| 05 | `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | 41 | `7059b0c23975094bd742ad9643c50ba8055db5fa` | [fix] 保证 CPU 到 GPU 场景同步与资源交接 |
| 06 | `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | 21 | `f8705444be71a1387ddb2a846844c0d53438321b` | [fix] 完善 Runtime 退出与窗口输入生命周期 |
| 07 | `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | 20 | `7156e63299de397d61aa9864ad1ec22b4bf26d75` | [fix] 完善 Renderer 资源与 DLSS 集成 |
| 08 | `605e46f98296f28b50cecb315d10666babcde21a` | 27 | `df51e3540c189b09e90f9e5c92408aed745cb2ce` | [fix] 修正材质求值与路径积分契约 |
| 09 | `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | 21 | `a7bd77032b38dc364553ad51cd8584433916caa0` | [fix] 完善 ReSTIR 与 SHARC 估计及历史管理 |
| 10 | `ec287c305aa77ea5297dc8b9128c2cc692959a52` | 11 | `812d1480e0fa9ca114ca1d16b6da9eb7df775072` | [fix] 完善 Editor 与 Automation 请求一致性 |
| 11 | `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | 5 | `48d079c04b9810cc364c1d3afa0153f857fcf471` | [fix] 完善 ImGui 与 sample 集成 |
| 12 | 本文件所在提交 | 2 | `f33a5bb4b1cc8f894e88d6d99fbd5315e167ff19` | [docs] 汇总全局设计复核与提交压缩记录 |

## 验证与限制

239 个旧提交按原顺序各归入一组，无遗漏、重复或 merge commit。逐组比较结束 tree 和净差异；
前 11 组不产生新文件状态。最后一组与旧 HEAD 的差异仅为本目录的历史说明。
新旧映射、父子链、备份可达性和文档链接均检查；临时脚本/完整验证日志位于 `.temp/review-squash/approved-12/`。
沿用原报告的编译和夹具结果及其局限；本次没有重跑完整构建、shader、GPU 或 HTTP，
tree 等价只证明文件内容相同，不是新获得的构建/运行验证，也不证明任意中间提交可单独反向应用而无冲突。

### 原有空白问题

原分组净差异的空白检查全部通过；历史说明新增内容另行检查。

## 全部旧提交到新提交的映射

| 原提交（压缩前证据） | 当前组/新提交 | 原标题 |
|---|---|---|
| `469aeae819f3da695cb8664dc253596c6f97db32` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [docs] 建立分批设计审阅流程并完成 B00 复核 |
| `be9b4dec4c717cc58ad33715243c95337b71e8f2` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 修正基础路径处理并清理工具依赖 |
| `db4375b5b802c52a5c707b3e6391a65c072cec4d` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善资源获取的版本更新与失败处理 |
| `94a9e96eaf664c8384b6b91df657c8ecfb9ed9a1` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善 CXX 部署的内容检查与清理边界 |
| `2b0eaee57d06a6090ce5f2bdc5b5afa6f68cfc94` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 补齐 sample 构建前置并记录 B02 审阅 |
| `68704517e17ce7e7aea13221c3fbeaf28cac7a08` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 按当前 CMake 目标与依赖生成完整 native 部署清单 |
| `232702f53514b7aad1565bc80ea126c2037b710d` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善 CXX 部署预检与失败重试的文件所有权 |
| `c992713b500a579c4fac447ea2117e33e9dd5e82` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善 Shader 编译完成判定与失败重试 |
| `68e661ad78179d6d3e9aad284ea26ff5d830b85f` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 按内容与实际工具身份验证 Shader 构建输入 |
| `9e5eeb6dd0ab10076ae1428bf176c0d1ddc0e855` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善 Shader 输出预检与失败恢复的产物归属 |
| `5c6b80eb46815bba9219efce358ef0151d5fd503` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 校验资源缓存身份并完善文件发布恢复 |
| `725b7ed8a0296b3efacd7225f8977de59dbed48b` | 01 / `9a97411ae2133a1a831ee85e3da96f7d94dca883` | [fix] 完善 Git 资源完整发布并完成 B02 审阅 |
| `cd4914c3e7f72f004a36653afb1ecf8a023c69b4` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 拒绝非法 descriptor 布局声明并统一生成投影 |
| `93532eec20c9b97e14ba193917cee42dafd48533` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 校验 descriptor 写入数量与边界并完成 B03-a 审阅 |
| `a396adf4acaa5d68c4d74f58df6fbd2c0be28136` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 限定 Shader binding 为数据投影 |
| `d988aef5f5593114a45e800f40d4a8ff53001401` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 从实际上传类型定义 ImGui 顶点布局并完成 B03-b 审阅 |
| `589eafc992275cc5da8d384ccf9cd9cf79722c58` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 按实际 owner 闭合 Shader include 预检 |
| `b94c5fffa604cf58a99876c73ab0f3f91cd991b2` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 校验 Shader layer 与路径前缀并完成 B03 审阅 |
| `fecd7b69e42aff4daf3f400ead66c053850cd940` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 在 Assimp C API 内收敛加载异常与释放 |
| `df68511fbbcb443a2b5f88ec38b5f22a6bf5b34a` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 保证 Assimp 路径的 UTF-8 与完整输出 |
| `2586b8b57969f3324fb95ed38a5d89ca2a74bb1d` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 校验 Assimp 复制容量并明确空数据契约 |
| `7df81047d724f37e66a989ae4d6e16dd0f90ad38` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 由 SceneData 持有 Assimp 的类型化借用数据 |
| `ead3c5cba108da9b3a27e7824923aac4e233c080` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 明确 Vulkan 函数表构造的 unsafe 前置 |
| `72b9e01180b15a8b8e942f0352c21e5fd3115415` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 在 Vulkan 桥接前拒绝空 push descriptor 列表 |
| `549f574bb7e9f4d0cabe58ba777d531df01bf807` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 确保 Win32 诊断缓冲在转换异常时释放 |
| `204a766505c1203af7b02bb1f8f9a6226b33fdec` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 收敛 Streamline 加载诊断异常并释放临时 DLL 引用 |
| `8bb6b9a33971d03570b6189e5d837feb5139c5b7` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 收敛 Streamline 失败状态与异步日志回调 |
| `b5c0b6b9cd3b17fabaac5f54c1df018f0c773c54` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 固定 Streamline 初始化路径并拒绝截断输入 |
| `7f41c5a3d5799fc28a0ac3a6f17dafad7a377dc6` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 按调用描述转发 Streamline SR 深度类型 |
| `bad7fae9cd661a8a1eac1b817b53dd8d6ac6c00c` | 02 / `c0d0ac3d5d8464385e76e3152dadb64f745e0ed3` | [fix] 明确 Streamline feature 跨语言调用安全边界 |
| `1870b85c7e428c7882fa7b77e8bfc2f3fffe3c3e` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 按完整能力与数量选择 Vulkan 队列家族 |
| `12c4d6f41a45f5ec1b84be9c2aa428bab91bc56b` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 隔离 Vulkan 诊断回调的可变状态与 panic |
| `15e2defbed55d05cdf98919599f9ba202934afd1` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 限定 VMA allocator 只能显式释放 |
| `20be07e6faa3c78321912edcee2bf817503f29c3` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 显式回收 Gfx 分阶段构造失败的资源 |
| `ebbd4c793055095c94687d969174dd6558e29688` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 收紧 GPU buffer 主机映射的类型与访问边界 |
| `39f44236148fa7806eb80e2b765aa40e0c9b04e2` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 按索引元素类型分配并上传 index buffer |
| `a3a8fc0d835de2910001a891e7aad8f2dcca6b53` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 显式回收未交付的 GPU 叶子资源 |
| `a8aed40260babcb052c9b162b5e95c203d230d7b` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 校验图像描述并完整投影子资源与上传范围 |
| `749da47c703ca42e6711d4bb545696520f77763a` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 限定同步对象的唯一 owner 并回收构造失败资源 |
| `035071bebc6dee546b96998972c96914adf50d2c` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 固定 query 结果宽度并校验查询范围 |
| `216309fa4df56d4f9cab5771cdc4858c471c73b4` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 校验命令池失效状态与批量释放来源 |
| `b0c052f860aae49b36ad5dff40798d68573b7e18` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收命令构造与提交前失败的分配 |
| `6d33287c2e679bb892dbdc076ec20eacf20526ba` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收 descriptor 与 shader 叶子创建失败资源 |
| `5f6758a24d6ed400b2f3e44870d0ca8ac9834a8c` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收 graphics pipeline 组合创建失败资源 |
| `837cf3ea8a0d74211228676f45af949bf8b712b0` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 完善 compute pass 参数契约与构造失败回收 |
| `fc594e3b95dc3df74fd0e075a6892498a9f83b9a` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收 RT pass 与 SBT 组合构造失败资源 |
| `34d5c5d92b34cf57edc7a5cd7e79c338676fa93f` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 检查 SBT 与 SoA 布局的尺寸和地址边界 |
| `403a4a60251cd955a7bcb1d3f9aa1c4d19eafa0e` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收 AS 未提交构造失败资源并预检数量 |
| `08eec51e8c6aa21afbc08d516f80d1c5456cdbd4` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 校验 AS 构建输入数量容量与对齐 |
| `6a43c1dea6fba6c886d54cafaf6c91f2a65c3f2c` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 完善同步 AS 构建的阶段交付与回收 |
| `a0514f5073df934fe0e05090aded8397557a150c` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 收敛同步上传并区分提交前失败回收 |
| `83bee18f3fe057878af5994d116ede0bbba41f6a` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收直接上传与 AS 构造的提交前失败资源 |
| `40ecf624617724e10839cb65a071cd2661759038` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 校验 WSI 创建能力并回收构造失败资源 |
| `827171b06909569b09054b0bca3b4648732847d3` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 约束 WSI acquire 与 present 状态转换 |
| `96477bccc6797f2f1e3e0f91b0399213d62ba3a0` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 完善窗口 Presenter 初始化的所有权与失败回收 |
| `f8a9d6335fbcb4aa05a4a288ea5a39e1f773f444` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 清理 swapchain 重建失败的未交付状态 |
| `c00695fa2409e0f3f6d0e4d61c2fb57fecfa994e` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 按最新退役帧回收 registry 资源 |
| `eb01e6749fe84dc27c33964962ea84444783b197` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 完成 registry 回收后再传播诊断异常 |
| `6abaf4411ced3513db783fc6f44fc2617e05a320` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 在注册前回收已到期的 bindless slot |
| `1b10855c8e41fa1d7362662207ed3b7fe9f8363a` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 回收全局 binding 组合构造的未交付资源 |
| `3fdf5f5dda83a2d60add9aa1d85e238f515c3589` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [refactor] 删除未使用的 staging buffer 管理封装 |
| `449ed314a165c991b3f6a5045255e0f8e5e41cbb` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 补齐 FIF 资源构造与销毁的失败回收 |
| `deb0675a481a67f1a96e0da8bd6bef9485338f56` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 校验 RenderGraph 资源登记与声明引用 |
| `8c637b21c2503169ce3438b192084ea9376e35e4` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 合并 RenderGraph 同一 pass 的完整图像访问范围 |
| `603145eaabec2f7e229c335b93c2b5cc4d099996` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 补齐 RenderGraph 连续读取与导出依赖 |
| `8e652499289d9f9c88240dd30afb3bea6667e171` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 核验 graph 物理资源并覆盖完整 image 同步范围 |
| `d838b2812dfa9de6f89b6c39d5b37e5990462dba` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 合并 graph semaphore 声明并拒绝提交冲突 |
| `4b0c4cd3ae5326f7599d7a7534b51afef67d4e87` | 03 / `539436ead349a1ae325f2d2327a4893aa526adf5` | [fix] 前置校验 graph pass 名称并明确执行契约 |
| `1b3ed9384583b8e5b7c8754302bf72cda133cfcd` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 收拢 Asset worker 的任务异常与退出等待 |
| `acad100c7e111c68d2d563e013ce8b993f7e234b` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 收紧 Asset 请求登记并清理冗余事件队列 |
| `16863b344c4de1965f92f2dbaa457ad59ec3fc36` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [docs] 完成 B08 纹理解码审阅并明确输入边界 |
| `841bf56d50b2f025e16dd138015afe492ddf867e` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 按真实长度校验 glTF buffer 与 view 范围 |
| `f93a57c54b3f372d88f72e93597f0541c4e6c2dd` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 统一 glTF image URI 解码与本地路径边界 |
| `d56e324aef326e16dd8ceb6a257963de9efff9ca` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 在资源读取前校验 glTF 文档版本要求 |
| `a65eb17ecaf80f56ee9dba59e5f5ae6dcc705e8d` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 glTF 必需扩展入口与声明关系 |
| `7251930997f12e872123106e0f182bbf1c0c8d35` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 收紧 glTF 几何 accessor 与 sparse 读取边界 |
| `03fbf2432b2d9f4082fdc96c3d2513522a28daea` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 按 glTF 语义生成缺失的逐面法线 |
| `1b33bcb07136242a8bcfd12b082e4f922b5e4fee` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 glTF 法线与切线方向语义 |
| `d487a2ff8cecf158855d77a6de7947cedb83dd89` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 glTF node 森林并消除递归遍历 |
| `b5454e0e041b53072936eb4660e7e17735df9d72` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 收紧 glTF 变换与静态实例发布边界 |
| `e6ede06eb95e9f934930cdb86a4f5d1e00b1bdbf` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 glTF 原始材质参数范围 |
| `e5435ab1c11e61a2003a3ec388c9c5e90176968a` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 保留 glTF MASK 的完整 alpha 阈值 |
| `9814402a85e6571921fdd513121b044fac58ea81` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 拒绝无法表达的 glTF 材质能力 |
| `e587fd84fa525a8f7f1dbd34725379b70eb9f2eb` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 Assimp 几何与实例发布前置 |
| `f32c4dfaef392e6d097bd0977445cea679ae5553` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 修正 Assimp 标量与发光材质转录 |
| `7b364475a4e4327fa69e43628207b1c57e049411` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 明确 Assimp 纹理引用与映射能力边界 |
| `6b422d3496c31f14b4f99189306bbe0cbc4978d0` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 校验 Assimp 静态几何与引用边界 |
| `86679a51448af89c82282e0abef2d1a60a8b5ece` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [refactor] 复用 Assimp importer 并保留清空语义 |
| `373d504b35e1e182860e7163234a6178e1dbbe4e` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 隔离场景导出的临时文件与源数据 |
| `bf64c21b49f9f966fa52f3ec9c39c8efa00050e3` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 保留场景导出的烘焙语义与当前帧 |
| `b40b0c74010d5b220ef4e2bedfee01c8ae5c0fba` | 04 / `c74c65420d6146bf427c7478dcd8334ce2f5f0a4` | [fix] 收紧 glTF 发布预检的输入与引用边界 |
| `afba45d7721cde3c0ef16a5f37959b32262cd344` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 恢复失败纹理的显式重试并约束请求发布 |
| `abd5f3d992cfd4d2e4ef0d9bcb38815704b0e13b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 在资源发布前完成 scene 导入预检 |
| `a17bd353eb011698aef7bce354c5016eb9204591` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 AssetSystem 完成事件并收敛诊断异常 |
| `c2a8a5cc384a1e7c84f7064c30371544d172c79f` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 在 instance 编辑边界校验 world transform |
| `25e9e4bd9027df90cba7e5c5423a8d4e0d1e2e9f` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 补齐材质数值校验并完成 B09-b 审阅 |
| `09edb661cc1d3e40cb0bbc022f39f6c42bf3484b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 统一灯光注册与编辑的参数校验 |
| `f9ec6f99571839695fa469c43404ac1097013d2b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 修正 Rect 绕序并完成 B09 审阅 |
| `b2cee8187ee670db959792c218c93ec1a696e65b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留资产上传等待失败的队列状态 |
| `5146e358bcd224eeb1dad6e1fb6e9c8a95db54f3` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善资产上传准备回收与提交所有权接管 |
| `4362577c4f19675b9445d9907e698fe49716e5f5` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留未移交的上传完成资源并收尾诊断异常 |
| `44647b752c55dec74497a38da69cc5cc91950552` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 回滚 texture 绑定安装失败并完成发布审阅 |
| `a53a16bdc03e7ba0854216445fd1e177f22b4132` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 mesh 上传等待失败的队列状态 |
| `7831825a73d6ef328bfc000fd27740a2ffbec6c6` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 mesh 上传准备回收与提交所有权接管 |
| `4926ec0a346a1412b8549c0052d86558712e7f2b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 mesh 完成通知并完善退役回收 |
| `f3df562bfe90d16d95898780749286b47e61e251` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 material FIF buffer 构造与关闭回收 |
| `72e24f6f2e6f56f7ccfb178a0728858d67205b89` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 material 同步通知并完整发布 slot |
| `cfeed488efeb1809d03500288f7bae056b119b0b` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 修正 material slot 回收顺序与上传确认 |
| `29c7f8309afcc9f06559b643f4fc9293b911cdd4` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 sky worker 请求与关闭契约 |
| `fb7db8206b247e561b70b61295055df9b4577088` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 sky 分布上传与发布前的结果所有权 |
| `ad09c0c8c1b16d1e830385b96226420ed95c83b3` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 在 sky binding 解析成功后确认变更通知 |
| `27724b4847482493858ef9d4753a60d1db660fa2` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 sky fallback 构造的资源回滚 |
| `efa8410986c56803f5e36ca6b029ee163ca1f535` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完整收尾 sky worker 与 GPU 资源关闭 |
| `ddad92d3719b5532c0bc1fa88cb6bb01a4bf8b85` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 回收 RenderAssetSystem 部分构造的资源 |
| `dea8f7422484f63f3acdada239224139bbec1bbc` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 按上传关闭状态完成资源 owner 收尾 |
| `0507b8277a7afa4868dce82ed31a28633b0cf8ab` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 为资源上传建立 graphics 队列等待依赖 |
| `a75c7db5f5cccc8f32a85184735e521741f9effc` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 补齐上传资源的 shader 与几何读取屏障 |
| `cdc36470667d51c360a14954c548dd4e47cbf1e1` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 instance 对账失败后的候选与变化通知 |
| `4233e6b766081e458d5f8af74aadf0b74d922cfd` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 prepare 通知并前置下游失效登记 |
| `da14ac09cd52234cbce1c8bfc666a1cc0492a7d5` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 SceneBuffers 构造与关闭回收 |
| `f6e807dc7e48e72060a3a6366d52324c91e696b4` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留场景 buffer 扩容后的 owner 与上传需求 |
| `a7bfc737545ba975ed882b08c2bc06397ab553d9` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 将 instance 版本确认绑定到完整上传录制 |
| `b3c7bfb2d860bdb671e3a6a0c2dd0dfac06f717d` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 校验场景结构表布局与 GPU 索引范围 |
| `55500f60612e1a2715c49614a27581413d3dbee3` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 仅复制场景结构表的有效数据前缀 |
| `68302364c11a5b43e90565358e19f7c4fbe1b1f5` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完成各 FIF 的 TLAS 关闭收尾 |
| `fe4fbd66236ce4639e91bdb4ca3ce9c5d1456182` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 analytic 灯光 buffer 的构造与关闭回收 |
| `b3f85f88c57a0aa5cee52695e52f72c16efc3be7` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 按需扩容 analytic 灯光表并上传有效前缀 |
| `94c8f9a7bc52d433558c5e3c514293f87d22f4f2` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 emissive 表组合构造与关闭回收 |
| `3c75d3a3f56e0ab992a986599172cb2e2e32345c` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 保留 emissive 扩容资源与完整上传需求 |
| `a9a48d86df1941e3ac2d4a90525f1761f21a8d0f` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 约束 emissive 表与几何映射的一致性 |
| `91b3feb56e06ee756e4ac92cec5d820346ea127a` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [fix] 完善 RenderWorld 组合构造与关闭回收 |
| `7059b0c23975094bd742ad9643c50ba8055db5fa` | 05 / `c34cfc8724dec783b2381ffe366ec27f2c9ba0fd` | [refactor] 简化 analytic 灯光返回值并完成 B11 复核 |
| `38193865c57edcba6c1761122f40bf5d915402c2` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 完善 RayCastService 组合构造与关闭回收 |
| `81b758392fdde552c3f45e2ff043a7e2902efc27` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保留 raycast 扩容前的完整资源组合 |
| `25c37ead979009b3b1e43829c76864e7af75fa58` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保留 raycast 未完成提交并回收录制失败命令 |
| `27535318fc0f6abefd86eb1bde5c36d8360033c1` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 校验 raycast 方向数值与设备派发上限 |
| `dd9f60a4850b62397f5b82b74d2c98d81713cf2f` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 回收 Runtime 已知安全构造阶段的候选资源 |
| `2304849883005130a82b9fc549592455d562b573` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 在 Runtime 关闭后传播已完成释放的诊断 |
| `c53844d0732282b2cd755b9b9de8596a0e1fc384` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 移除 Gfx 关闭 SDK 与 root 之间的日志断点 |
| `68f527c1f84a262eb917997d4651337002cae5f0` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保持 RenderLoop 正常退场的关闭顺序 |
| `a5cbfcae82801ceb5bd22193eb1ad6396ca45e9e` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 禁止帧号回绕并完成 B12 阶段能力复核 |
| `f1f8b78fa9d7be713a61c5417482d513996f1b2c` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保留渲染线程完成通知与原始异常 |
| `44b11dc499a378c635c75441d6df0c046a29fab5` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 在窗口关闭后交付宿主错误 |
| `4c63f69b940afa4feec2cfeba374e3216106e287` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 在输入投递与排空期间响应退出 |
| `5b14171c7c9c06f473caea7cd5c2bf4ca49a2411` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保留 sample 被 UI 捕获的松键与坐标更新 |
| `02205b2adf52f2b44ed2d8f30808877265134b77` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 保留同帧输入已观察到的按键边沿 |
| `26ccdbbb8336a050505e15519969f1784acaa970` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 在交互取消时清除相机导航状态 |
| `fda7cbcaf6299a93839f7a9957ab5b7ffaf338d8` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 统一相机视图与导航的 roll 姿态 |
| `93aae72f293038028fe06f54478451f8ac3a54aa` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 禁止普通相机更新创建导航查询 |
| `7b34bf3a320562c3311c02d466b27217a87dc2a5` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 拒绝非法相机输入与非有限视图快照 |
| `c61e4826c4d5bb0f5c312cfeb1e2645b25f13d0c` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [refactor] 移除滚轮导航的冗余距离缓存 |
| `b50ba2cf78f00e173c6bcf6bbe07bdb1df975a2e` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [refactor] 移除子系统录制上下文的未用字段 |
| `f8705444be71a1387ddb2a846844c0d53438321b` | 06 / `76fae4df1ca203be8dde066a4c7bc91e3a92bd21` | [fix] 明确稳定视图计数的诊断语义 |
| `cc56502cd3e6ac7f9bfda11e4a349e0306d65d7a` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 在输出格式变化时重建主视图 resolve pipeline |
| `57f4444f12f0060112d1355d64328cc29308fa40` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 同步 ImGui 与 sample 的输出格式重建 |
| `3bf92d98a4c017783f47ed154768dd4900c135f7` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 在队列提交后确认主视图累计与历史状态 |
| `f0d6d6c0a464fe48e2fb558b7b1f32a1c22a3a0c` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 拒绝离线样本耗尽并避免帧槽计算溢出 |
| `c5a8ab2a28c0257f64453a265f01420bf63ca570` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [refactor] 复用共享 target 管理 GBuffer 图像生命周期 |
| `428fc4f081a995bb700939394f7a86303e169d7c` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 完成同组 target 清理后再传播释放诊断 |
| `ff73f6299d22e8b9403ed508776ebe055db3cd35` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [refactor] 移除主视图未使用的深度资源与附件用途 |
| `8edccc6fb5aa620b0b873c07b0e3700e3bb1df4a` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 完成主视图资源集合清理后再传播异常 |
| `d8f5f2716d72153f39565ab7e292b38e2ea3d0cb` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 回收共享 target 已知安全阶段的构造半成品 |
| `b6e67ad07e5304f1957b243cdb97f9974a40677a` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 仅展示本帧有效的调试图像 |
| `335ab29caad47676f27d19901827479fc2317d6c` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 在队列提交后确认 SDR 曝光历史状态 |
| `38bbddae3a89623ed182c17314af9cdabbf67e1b` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 完成 SDR target 清理后再传播释放诊断 |
| `7ee19f91af6d988205cd420cc333f2772a21234b` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 回收曝光 kernel 构造失败时的已完成前缀 |
| `35b5067cda08d991a36c71acb70e068de9983ab9` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [refactor] 复用 LUT 上传可见性并完成 B15 显示链审阅 |
| `43d5a095b9fbc4d58d3e3d0141fc40329b011441` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 按场景 graph 录制登记 DLSS feature 释放责任 |
| `3b30b37e37347142a9e71086b9a781766fa19867` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 保留 DLSS 释放失败状态并停止重配置 |
| `15fc8ac11d373a201c69d17d5a7f10bdd0f33ebb` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] DLSS evaluate 失败时中止整份 graph 录制 |
| `a8fccc422e9c977db8ccfb5e7cfabcc255fd3812` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 复用视图逆矩阵构造 DLSS 双向 clip 变换 |
| `91dddc92d356a01644c6cba1e9951ad83cc50789` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [fix] 回收 SR exposure 上传完成后的构造半成品 |
| `7156e63299de397d61aa9864ad1ec22b4bf26d75` | 07 / `54efeaa41aa4cbe6b1dda5d720a6f4221a70f541` | [refactor] 移除未使用的 Streamline 手动 Vulkan hook API |
| `744cc6272f94fc27d414b7d3c9b45aeed9a0684e` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 修正 GGX NDF 采样分布与低粗糙度精度 |
| `a285862834ba659c364d905e24087caa01a9dc2b` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 约束材质参数与 MR 贴图组合的合法范围 |
| `900eeb8783c8b991b4d29bf38fa6634d2e278328` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 让 RR delta guide 复用实际材质反射权重 |
| `be22229f44f464b940e83e1493b1b0326af4a3fc` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 统一默认 Surface 的 F0 与材质消费入口 |
| `8ae1f6bb6ef2d4f22140b5288533d60da30f86b4` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定光滑透射界面的 Snell 与 Fresnel 求值 |
| `3577adee9e74162147351ad14e754c0fe50ce734` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 修正 trace 命中的几何法线来源 |
| `95e27d0810459fe30f3d095b1d73837a0681c1e0` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 统一 BSDF 与 RR 的着色半球有效域 |
| `bdddfcaf4d3d982564791ef8bf60b127fb582675` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 独立累计透射路径的 roulette 权重 |
| `5c57b0e7652529baf2ec3b5717ac4a3b5d049df6` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 修正 Russian roulette 的随机区间边界 |
| `d918b4aa846571367272b9f8271a20e71e247479` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [refactor] 合并 RT 公共事件与载荷定义 |
| `294e0b9f79e9d04b00a97b8943a72f366bb88bbd` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 对齐纯发光材质的 BSDF 与 RR 输入 |
| `64f08e17a9bb05f925cede792f375f74f98b00b4` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定 MIS 幂次权重并移除未使用变体 |
| `5e9c8ad348a48820828ea3cebe7d811aeb3803c9` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 补齐普通发光材质命中的 MIS 竞争 PDF |
| `d0db2678516e67da19d1a1f14e10cd10b3c0a603` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定 HDRI 极区方向采样与查询 |
| `51765ab036b157860607d1e585ca048e54b30609` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 按原始表面连接查询发光命中 PDF |
| `7e7ccf4ca5d4e8894a6606a9339c5ac6f4a3249e` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 连接两端偏移后的发光几何阴影射线 |
| `3e281514a56f3f38b30e3db4406193d9a53c2665` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定发光三角形面积与法线计算 |
| `d68d6e4e5646ed60e041525856147657429c55a1` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定发光几何的立体角 PDF 换算 |
| `cfb30d655dedf0c80577f962d01583d67ed341ed` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定球形光源采样与可见半球重建 |
| `76434dc82ca4e8398cb23c8eb531face1599da7e` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 统一矩形光源采样与重建的几何契约 |
| `e886d8277e63d4139ea3e546e680f9f5284f20fb` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 修正有限光源遮挡射线的目标连接 |
| `5a532e54a32d1b2788aea9435802cd77a1f1f08c` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 稳定 spot 窄锥衰减并明确等角边界 |
| `d3287ad2b5d65da94b0e98314d0c016e0fe6694a` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 按实际 PDF 计算路径与直接光权重 |
| `9fc10916439e62bbd5abc967b2f577d3057daace` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [refactor] 简化 roulette 返回值并明确补偿归属 |
| `4cd35fbc841f8a2446077beb51562c222d2478d2` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 修正路径深度上限处的 NEE 竞争权重 |
| `5912f4789caaa0ccfde24369c28b4041884f73e0` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [fix] 收紧纯发光材质的路径与反射 guide 消费 |
| `df51e3540c189b09e90f9e5c92408aed745cb2ce` | 08 / `605e46f98296f28b50cecb315d10666babcde21a` | [refactor] 清理失效射线常量并完成 B18 证据汇总 |
| `1d2645a978a457da5963e56eb4c24489742dde5d` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 保留 ReSTIR surface 几何法线并完善资源绑定 |
| `0f59dadc02112dc6f1a2bfe4dca725b23e2dbf70` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 修正 ReSTIR 初始权重与归一化数值范围 |
| `7979922e6ffc7747ea83d261addd19591a45c5ed` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 统一 ReSTIR 历史年龄的接受与归一化 |
| `de307abd80d5fd679f21b41a605b3d0fef9ebff1` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 扩展 ReSTIR 校正分母的数值范围 |
| `5ed98455b482c66146ccd67508062307c30d6cc8` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 保留 ReSTIR 零估计候选池的计数与校正 |
| `a1c15ba81a364d15f67463cf1bee0dd53d04261e` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 使用历史观察方向计算 ReSTIR 时序校正 |
| `3d1ddcb92ee2a914d126c574f43d67fbca094fb1` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 补齐 ReSTIR 跨视点重连的 Jacobian 校正 |
| `d13e6a276bfdd745b1d821a1310516dad859a0da` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 在 ReSTIR 采样配置变化时拒绝旧候选池 |
| `15757922a486bea54804c9caa4562c27ac3a1cf5` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [refactor] 合并 ReSTIR 最终着色的重复 BRDF 求值 |
| `33f9220821ddcf5a1fc3407b9f53f631ba0bcb30` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 回收 ReSTIR 组合构造失败时已完成的图像组 |
| `d203ebbd2b86908f20fe7801883f416869112a71` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 按实际提交帧补偿 ReSTIR 历史采样 jitter |
| `591483b6e6db9aff1c8a252f385ece20b8490a44` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 保留 ReSTIR 历史图像覆盖前的跨帧访问依赖 |
| `542b9fee313bf2b172938d495661c4d53ae48d7f` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 清空 SHARC debug 提前退出的 ReSTIR 初始候选 |
| `ae2187c9a3110132f57f2bfd8a29021db9f1cd97` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [refactor] 跳过 ReSTIR 调试通道未消费的最终着色 |
| `bfdd3db42a6e7f3d3da0c6c49f55f2f0707aee9b` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 回收 SHARC 初始化前分配失败的 buffer 前缀 |
| `9c0505b17e76d07c3b2eb6adec0f0a5aae535f37` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 补齐 SHARC 首次访问与跨帧复用的内存依赖 |
| `fd528a5170fc31aeff8891cdb299ab141df84a92` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 拒绝 SHARC 探测窗口耗尽后的无效缓存索引 |
| `20d85d2ce6bb787d1e641e7efeacc423fe240ed0` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 为 SHARC Resolve 提供稳定的跨槽历史快照 |
| `b6d41feb2cbcd78d47d7c0bbf685528f1e42c08e` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 在场景与配置变化后重置 SHARC 缓存 |
| `eb9b54e3e26502cab3e6a1b8a0351d30b02c3812` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 拒绝 SHARC 零容量哈希表访问 |
| `a7bd77032b38dc364553ad51cd8584433916caa0` | 09 / `bdfafeb1943cacfcbd88efe01b3c15b42a10f036` | [fix] 统一 SHARC 调试在关闭与未命中时的输出 |
| `15bbf3b853eea4a5be37d911b1761595f695c03b` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [docs] 明确 Editor 请求超时与关闭语义并完成 B21-a 审阅 |
| `74025fda59011629f38c39f5e3dec0c74fe352f8` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 保持 Editor 材质 alpha cutoff 的合法范围 |
| `7ac3d34a0809574424cea932d29e91a2c61598b6` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 防止场景导入终态被重复消费 |
| `e9558850d5f9828c6cbcd6aa3dbae3eb0e68a00b` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 区分失效导入身份与场景加载失败 |
| `f7ced0f4916d98e86a63f9fdef8d56cff593c35e` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 拒绝 MCP 原生接口上的浏览器 Origin 请求 |
| `3104100f8a317874e29b9fadda3fa68d086687ba` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 限制 MCP 关闭排空等待时间 |
| `a1e8cdd73324eec9ea240b9b9638a6eccba382bd` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 合并材质滑块释放与失焦的重复提交 |
| `b19ad8187f5f3b00e3524078e4903122f8c4411e` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 让 Editor 会话清理失效旧查询 |
| `9e18f56125dae83e2de399954b912fa29850ab50` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 完善材质回包与会话查询顺序 |
| `ac07db95e3a0a56be98f844e9920b85fe3015a23` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [fix] 收紧 Editor 数值输入与视口清理 |
| `812d1480e0fa9ca114ca1d16b6da9eb7df775072` | 10 / `ec287c305aa77ea5297dc8b9128c2cc692959a52` | [docs] 记录 B23-c Web Editor 复核 |
| `aa9219b0a44c8fc76a45078e0e3779beea6bf788` | 11 / `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | [fix] 收紧 ImGui 裁剪矩形边界 |
| `50f5612ad26e836da52502d051957f38ecd349cc` | 11 / `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | [docs] 记录 B24-a 拾取与 UI 复核 |
| `f4359826080c31d0bd2c519515ab20475b3d0efe` | 11 / `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | [docs] 完成 B24 拾取与渲染 UI 复核 |
| `40aaa07b7f9c2e0509a30dd1713021eeb576b88e` | 11 / `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | [fix] 传递窗口 DPI 变化并在安全阶段重建字体 |
| `48d079c04b9810cc364c1d3afa0153f857fcf471` | 11 / `8a50bef93d7f4ad98e9f5ee76e0eee19cce01186` | [fix] 收敛 sample pipeline layout 所有权并完成 B25 复核 |
| `7f7dfc3d89986786765bb7a7427734173e689762` | 12 / 本文件所在提交 | [docs] 完成 B26 全局冗余与抽象复核 |
| `f33a5bb4b1cc8f894e88d6d99fbd5315e167ff19` | 12 / 本文件所在提交 | [docs] 完成 B27 全局严密性与验收计划 |
