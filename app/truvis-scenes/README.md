# truvis-scenes

两个 App 共用的 CPU 启动场景与参数模块，不依赖 Tauri、Editor bridge 或具体 Renderer。

- `InitialScene`：固定 Manual、Sponza、Cornell 三种预设，默认 Manual。
- `StartupOptions`：统一解析 `--scene value`、`--scene=value`、`--help` 和 `-h`；App 在创建窗口前处理结果。
- `SceneInitializer`：由 App Client 持有，只在 RenderThread 的 initialize/update 阶段借用 `GameWorld` 和初始相机。
- 私有场景模块负责相机、灯光、材质和布局；Manual/Sponza 复用一份材质样本定义。

Manual 固定展示三排：`z=-1.5` 的五个基础材质 sphere、`z=1.5` 的五个基础材质 cube，
以及 `z=4.5` 的五个贴图 cube。两排 cube 共享同一个 mesh；sphere 保持无贴图。
贴图排沿 X 从左到右使用 `resources/uv_checker.png`、`textures/Ask.jpg`、`textures/Ri.jpg`、
`textures/babala-opacity.png` 和 `textures/babala-transparent.png`（路径相对 `assets/`）。
所有贴图以 sRGB 绑定 BaseColor，每个面以完整 UV0 铺满图片；横图会压缩到正方形，不做等比留边。
`babala-opacity.png` 是不透明彩色图；透明版使用自身 alpha 和 `AlphaMask(0.5)` 裁切六个面，
透过孔洞可能看到背面图案，半透明边缘按阈值裁切，不进行 alpha blend。

Sponza 使用 `fbx/sponza/sponza.fbx`，Cornell 使用 `fbx/cornell-box.fbx`。共享导入流程在 CPU 数据 ready 后创建一次实例；成功、导入失败或实例创建失败都会清空 pending，不重试、不切换场景。失败保留已创建对象，并记录场景、路径和错误。

本模块不拥有 GPU 资源，不调用 Runtime prepare，也不自行驱动 loader。CPU 资源属于 `GameWorld`/`AssetSystem`，实例和灯光属于 SceneStore；Runtime 继续负责 prepare、上传与销毁。`renderer-kit::Camera` 和 shader-binding 灯光结构仅用作现有 CPU 数据接口，不访问其 GPU 能力。

场景切换只发生在启动时，不提供运行时注册表或通用 Scene trait。新增产品场景应在本 crate 的具体模块与预设枚举内实现。
