# truvis-scenes

两个 App 共用的 CPU 启动场景与参数模块，不依赖 Tauri、Editor bridge 或具体 Renderer。

- `InitialScene`：固定 Manual、Sponza、Cornell 三种预设，默认 Manual。
- `StartupOptions`：统一解析 `--scene value`、`--scene=value`、`--help` 和 `-h`；App 在创建窗口前处理结果。
- `SceneInitializer`：由 App Client 持有，只在 RenderThread 的 initialize/update 阶段借用 `GameWorld` 和初始相机。
- 私有场景模块负责相机、灯光、材质和布局；Manual/Sponza 复用一份材质样本定义。

Manual 展示三排材质样本与右侧的手工 Cornell box：`z=-1.5` 的五个基础材质 sphere、`z=1.5` 的五个基础材质 cube，
以及 `z=4.5` 的五个贴图 cube。两排 cube 共享同一个 mesh；sphere 保持无贴图。
贴图排沿 X 从左到右使用 `resources/uv_checker.png`、`textures/Ask.jpg`、`textures/Ri.jpg`、
`textures/babala-opacity.png` 和 `textures/babala-transparent.png`（路径相对 `assets/`）。
所有贴图以 sRGB 绑定 BaseColor，每个面以完整 UV0 铺满图片；横图会压缩到正方形，不做等比留边。
`babala-opacity.png` 是不透明彩色图；透明版使用自身 alpha 和 `AlphaMask(0.5)` 裁切六个面，
透过孔洞可能看到背面图案，半透明边缘按阈值裁切，不进行 alpha blend。

Cornell box 的箱内地面中心位于 `(13.5, 0.12, 0)`，内部宽高深均为 4 m，开口朝 `+Z`。
箱体整体位于原地面右侧之外，外侧 X 范围为 `[11.4,15.6]`，距地面右边界 `X=10` 留有 1.4 m 间隔。
五块厚 0.1 m 的墙板与两个旋转白色方块复用样本的 cube mesh；左墙红、右墙绿，其余为白色粗糙表面。
顶灯复用地面的 Floor mesh，翻转朝下，尺寸为 `1.2 × 1.0 m`，白色 emissive radiance 为 `(15,15,15)`，
仅通过自发光几何照明，不叠加同位置的 analytic light。箱体自带底板，底面高度为 `Y=0.02`，顶灯距顶板内表面 0.02 m。
新增实例使用 `cornell-` 前缀，共 8 个；整个 Manual 共 24 个实例，仍只注册 Cube、Floor、UvSphere 三个 mesh。
默认相机从 `(11,6,22)` 看向 `(5.5,1.5,1)`；箱内近景可从 `(13.5,2.12,8)` 沿 `-Z` 观察。
原有天空和灯光保持启用，开口允许外部光进入，因此该布局用于同场景观察，不代表标准 Cornell 光照基准。

Sponza 使用 `fbx/sponza/sponza.fbx`，Cornell 使用 `fbx/cornell-box.fbx`。共享导入流程在 CPU 数据 ready 后创建一次实例；成功、导入失败或实例创建失败都会清空 pending，不重试、不切换场景。失败保留已创建对象，并记录场景、路径和错误。

本模块不拥有 GPU 资源，不调用 Runtime prepare，也不自行驱动 loader。CPU 资源属于 `GameWorld`/`AssetSystem`，实例和灯光属于 SceneStore；Runtime 继续负责 prepare、上传与销毁。`renderer-kit::Camera` 和 shader-binding 灯光结构仅用作现有 CPU 数据接口，不访问其 GPU 能力。

场景切换只发生在启动时，不提供运行时注册表或通用 Scene trait。新增产品场景应在本 crate 的具体模块与预设枚举内实现。
