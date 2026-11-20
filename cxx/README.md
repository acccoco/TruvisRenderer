# CXX

`cxx/` 是 Truvis 唯一的 native integration project，包含完整 CMake project、C++ modules 和 Rust
binding。它不镜像 `engine/`、`renderer/`、`app/` 的 Rust 分层，也不从 CMake 调用 Cargo。共用的
Rust binding 构建支持位于 `engine/tools/truvis-cxx-binding-build`。

## 目录

- `CMakeLists.txt`、`CMakePresets.json`、`vcpkg.json`：唯一 native project 入口。
- `cmake/`：export header 公共 CMake helper。
- `modules/`：全部 C++ target；module 使用标准 CMake 直接依赖。
- `bindings/`：Assimp、Streamline 等 public C ABI 的 Rust consumer。

当前 native module：

- `truvixx-utils`：内部 STATIC helper。
- `truvixx-assimp`：内部 Assimp 实现；`truvixx-assimp-capi` 是 Rust 使用的 SHARED C API。
- `truvixx-vk-capi`：使用 Vulkan C headers 的设备命令桥接，函数入口由 ash 注入，不链接 Vulkan loader。
- `truvixx-streamline-capi`：Streamline/DLSS SHARED C API，运行时 DLL 与 JSON 由对应 Rust binding 声明。

## 构建与增量

外层 `scripts/nu/cxx-cmake.nu` 顺序调用 CMake configure/build，再构建三个明确列出的 Rust binding。
C++ 编译与链接增量完全交给 CMake/MSBuild/Ninja；binding 的 `build.rs` 不启动 CMake。

`TRUVIS_CXX_TOOL` 未设置时默认使用 `clang-cl + Ninja`，也可显式指定 `clang-cl`、`vs2022` 或 `vs2026`。
CMake 和 Cargo 必须继承同一值；不自动探测或回退，也不保存当前工具链状态。仅支持 Windows x64 MSVC ABI、
Debug/Release 和项目 `build/` 输出布局，不支持并发切换工具链或向同一 profile 部署。
IDE 的 Cargo 检查也必须使用同一环境；不同 Cargo 构建缓存共享部署目录，混用工具链后需清理 binding 缓存再重建。

```powershell
just cxx                   # 顺序执行 Debug、Release 两套流程
just cxx-debug             # Debug native + 三个 binding + 运行文件
just cxx-force             # 两套流程的 CMake build 增加 --clean-first
just cxx-compile-commands  # 仅 clang-cl Debug configure，更新 clangd 数据库
```

三个 binding 分别手写链接库和运行文件。import library 直接从所选 native 输出目录链接，PDB 留在 native
目录；DLL/JSON 按内容比较后复制到 `build/{debug,release}` 及其 `deps/`，不默认部署到 `examples/`。
Assimp 的间接 DLL 来自所选 CMake build tree 的 vcpkg 安装目录；仅 Assimp 自身使用唯一文件名匹配以容纳
工具集后缀，其余文件全部列名。升级 SDK/依赖时必须人工核对列表，缺少必需源文件直接失败，不从旧部署补找。

部署不保存历史状态、不递归发现 DLL 依赖、不自动删除旧文件，也不保证多文件失败回滚。失败后修正原因再重建。
源 lib/DLL/JSON 与工具链变量由 Cargo 追踪；手工删除目标文件不会自动修复，需先清理对应 binding 的 Cargo 缓存。
三个 binding 的缓存可用 `cargo clean -p truvis-assimp-binding -p truvis-streamline-binding -p truvis-vk-binding` 清理。
删除或重命名依赖时应清理旧 profile 产物后重建；完整重置可以使用 `cargo clean`，随后重新准备 shader/native。
没有旧清单迁移或兼容入口。

运行示例的 recipe 仍先执行 `cxx-debug`。需要独立操作 CMake 时直接使用 presets；其本身不构建 Rust binding：

```powershell
Set-Location cxx
cmake --preset clang-cl-debug
cmake --build --preset clang-cl-build-debug
```

clangd 辅助 configure 不部署 DLL、不发布 Vulkan include roots，也不改变日常选择的工具链。

## Rust FFI 边界

`truvixx-assimp` 的 FBX 世界输出为 m。Assimp 5.4.3 的 `FBX_CONVERT_TO_M` 不执行实际转换；
其 FBX converter 已将文件 `UnitScaleFactor` 应用到根节点，得到 cm 世界尺度，`SceneImporter::load`
随后仅对 FBX 根变换前乘 `scale(0.01)`。不同时启用 GlobalScale，不再次缩放 mesh 顶点；累计节点变换
和局部顶点共同定义米制世界坐标。其它格式不套用 FBX 规则；glTF 的生产导入路径位于 Rust loader。

材质标量优先读取 Assimp PBR base color，缺省时读取 diffuse；metallic 使用 metallic factor，
不从传统 reflectivity 推断。emissive intensity 只乘 RGB 一次，非法强度作为带诊断的导入失败。
这只是已有载荷的字段转录，不宣称实现了全部 Assimp shading model 或纹理类型。
颜色贴图优先 BASE_COLOR，缺省时 DIFFUSE；NORMALS 保持独立。所消费槽只接受单层外部
纹理及当前 C ABI 能表达的默认 UV0/identity/repeat 映射；非默认映射、混合/flags、多层和
embedded 引用明确失败并保留材质诊断，不伪造文件路径。未消费的其它贴图类型仍不受支持。

几何输出限于静态三角形。postprocess 后仍缺少 position/normal/face、非三角 face、越界
索引、不能由 u32 表达的索引总数、无效 mesh/material 引用均在复制前失败；skin、morph
和顶点色尚无输出载荷，明确拒绝。消费外部贴图的 mesh 必须有二维 UV0；无贴图时可补零
UV0，已有非二维 UV0 不接受。额外未被消费的 UV 集不导出。动画不求值，相机/灯光不转录。

- 对 Rust 暴露的 target 必须是 SHARED library，并只跨 DLL 暴露 `extern "C"`、固定宽度 POD 或 opaque handle。
- allocation/free、handle 生命周期和 callback 线程由 public C API 明确约束；exception、STL container 和 allocator
  ownership 不跨 DLL。
- Assimp 加载入口用 native RAII 管理未发布的 scene；普通导入失败返回可查询错误且必须释放的句柄，
  C++ 异常返回 null。Rust asset reader 通过 guard 对称释放，并在返回后台结果前复制全部借用数据。
  模型及外部纹理路径显式按 UTF-8 转换，外部纹理输出绝对路径。固定 C ABI 路径数组容纳不了完整路径时，
  material_get 返回失败且不修改输出；不将截断的文件名作为成功结果。
  mesh fill 与 instance refs 显式接收输出元素容量，容量不足时失败且不改写任何输出；
  空数组复制不访问指针，缺失的可选 normal/tangent 仍返回失败。容量单位与借用前置见 public header。
  所有 mesh 借用数组由 SceneData 持有实际 C API 元素对象，加载完成后不再依赖 Assimp 顶点类型别名。
  原始 aiScene 在全部转换完成后释放；转换期间有临时双份属性存储。视图只读且有效至 scene_free，禁止并发释放。
  SceneImporter 持有一个可复用的 Assimp::Importer；clear 释放场景和本次结果/诊断，不重建格式插件。
- binding crate 的 `build.rs` 单向读取 module header，经公共 codegen 生成到
  `build/bindings/{TARGET}/cxx/{crate}/`，再直接链接所选 native 输出目录的 import library。
- CMake/native project 不知道 Cargo crate，也不得链接或回调 Rust symbol。
- Vulkan 命令 binding 通过 owner 指定的 namespace import 复用 ash 类型；公共 codegen 不维护 Vulkan 类型映射。
- Streamline 的 RenderGraph pass 顺序和 Vulkan resource 生命周期仍由 Rust Renderer/Runtime owner 管理。

Streamline Rust 生命周期细节见
[`truvis-streamline-binding/README.md`](bindings/truvis-streamline-binding/README.md)。

Vulkan 命令桥接和宿主 ABI 契约见
[`truvixx-vk/README.md`](modules/truvixx-vk/README.md) 与
[`truvis-vk-binding/README.md`](bindings/truvis-vk-binding/README.md)。

## 生成目录

- CMake binary dir：`build/cxx/cmake/{vs2022,vs2026,clang-cl}`。
- Native 输出：`build/cxx/output/{toolchain}/{Debug,Release}`。
- export header：`build/cxx/generated/include/`。
- Rust binding：`build/bindings/{TARGET}/cxx/{crate}/`。
- clangd database：`build/cxx/compile_commands.json` 与 `.vscode/compile_commands.json`。

默认构建需要 `PATH` 中的 `cmake.exe`、`clang-cl.exe` 和 `ninja.exe`，以及 MSVC C++ workload 和已配置的 `VCPKG_ROOT`。
vcpkg 的 `x64-windows` triplet 使用 MSVC 构建第三方依赖。
