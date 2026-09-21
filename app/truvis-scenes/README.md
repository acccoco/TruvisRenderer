# truvis-scenes

两个 App 共用的 CPU 启动场景与参数模块，不依赖 Tauri、Editor bridge 或具体 Renderer。

- `InitialScene`：固定 Manual、Sponza、Cornell 三种预设，默认 Manual。
- `StartupOptions`：统一解析 `--scene value`、`--scene=value`、`--help` 和 `-h`；App 在创建窗口前处理结果。
- `SceneInitializer`：由 App Client 持有，只在 RenderThread 的 initialize/update 阶段借用 `GameWorld` 和初始相机。
- 私有场景模块负责相机、灯光、材质和布局；Manual/Sponza 复用一份材质样本定义。

Sponza 使用 `fbx/sponza/sponza.fbx`，Cornell 使用 `fbx/cornell-box.fbx`。共享导入流程在 CPU 数据 ready 后创建一次实例；成功、导入失败或实例创建失败都会清空 pending，不重试、不切换场景。失败保留已创建对象，并记录场景、路径和错误。

本模块不拥有 GPU 资源，不调用 Runtime prepare，也不自行驱动 loader。CPU 资源属于 `GameWorld`/`AssetSystem`，实例和灯光属于 SceneStore；Runtime 继续负责 prepare、上传与销毁。`renderer-kit::Camera` 和 shader-binding 灯光结构仅用作现有 CPU 数据接口，不访问其 GPU 能力。

场景切换只发生在启动时，不提供运行时注册表或通用 Scene trait。新增产品场景应在本 crate 的具体模块与预设枚举内实现。
