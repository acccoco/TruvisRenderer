# CXX

`cxx/` 是 Truvis 唯一的 native integration 子系统，同时包含完整 CMake project、C++ modules、构建工具和
Rust binding。它不镜像 `engine/`、`renderer/`、`app/` 的 Rust 分层，也不从 CMake 调用 Cargo。

## 目录

- `CMakeLists.txt`、`CMakePresets.json`、`vcpkg.json`：唯一 native project 入口。
- `cmake/`：export header 与 runtime plan 等少量公共 CMake helper。
- `modules/`：全部 C++ target；module 使用标准 CMake 直接依赖。
- `rust/tools/`：`truvis-cxx-build` 与 `truvis-cxx-binding-codegen`。
- `rust/bindings/`：Assimp、Streamline 等 public C ABI 的 Rust consumer。

当前 native module：

- `truvixx-utils`：内部 STATIC helper。
- `truvixx-assimp`：内部 Assimp 实现；`truvixx-assimp-capi` 是 Rust 使用的 SHARED C API。
- `truvixx-vk-capi`：使用 Vulkan C headers 的设备命令桥接，函数入口由 ash 注入，不链接 Vulkan loader。
- `truvixx-streamline-capi`：Streamline/DLSS SHARED C API，运行时 sidecar 与 JSON 由本 module 声明。

## 构建与增量

`truvis-cxx-build` 每次都调用 CMake configure/build。C++ source、header、编译参数和链接输入是否需要重新编译或
重新链接，只由 CMake generator、MSBuild/Ninja 与 compiler dependency information 判断；Rust 不保存 native 输入快照。
`--force` 映射到 CMake `--clean-first`，不会由 Rust 删除 object/cache。

CMake 在每次 Debug/Release build 完成后生成完整 runtime plan：public SHARED target 的 DLL/import library
与可选 PDB、target 声明的运行 DLL 及其递归依赖、module 声明的 required/optional sidecar。
第三方 DLL 从声明的来源解析，不以 native 输出目录中的残留文件为依据；内部 STATIC 产物不部署到 Cargo。
Rust 只消费这份 plan，将其部署到 `build/{debug,release}` 与其 `examples/`，不扫描 native 输出目录。

部署按文件内容判断是否需要更新，不能用修改时间代替内容身份。旧部署清单只能删除 profile 根目录与
`examples/` 的直接文件；清单路径及其实际父目录必须通过边界校验。必需源文件、目标重名、目录占用和旧清单
在复制前统一校验；首次复制前记录旧集合与当前计划的并集，成功后才缩减为当前集合。这样 IO 失败后重跑或
切换计划仍能清理部分部署的新文件。状态清单通过同目录临时文件替换；不支持并发部署，不保证多文件失败回滚，
失败后必须修正原因并重新成功部署，不能继续使用混合输出。进程崩溃、断电持久化不在此保证内。

常用入口：

```powershell
just cxx-debug
just cxx
just cxx-force
just cxx-compile-commands
```

`cxx-debug` 只构建并部署 Debug native 产物；最终的 `cargo run` 会在同一个 Cargo 构建图中编译所需 Rust binding。
Triangle、ShaderToy 也通过 Gfx 使用 native Vulkan binding，其 `just` 入口同样先执行 `cxx-debug`。
需要更新 clangd 数据库时单独运行 `cxx-compile-commands`，它不会构建 native target 或 Rust binding。

也可以直接使用独立 CMake project：

```powershell
Set-Location cxx
cmake --preset vs2026
cmake --build --preset vs2026-build-debug
```

## Rust FFI 边界

`truvixx-assimp` 的 FBX 世界输出为 m。Assimp 5.4.3 的 `FBX_CONVERT_TO_M` 不执行实际转换；
其 FBX converter 已将文件 `UnitScaleFactor` 应用到根节点，得到 cm 世界尺度，`SceneImporter::load`
随后仅对 FBX 根变换前乘 `scale(0.01)`。不同时启用 GlobalScale，不再次缩放 mesh 顶点；累计节点变换
和局部顶点共同定义米制世界坐标。其它格式不套用 FBX 规则；glTF 的生产导入路径位于 Rust loader。

- 对 Rust 暴露的 target 必须是 SHARED library，并只跨 DLL 暴露 `extern "C"`、固定宽度 POD 或 opaque handle。
- allocation/free、handle 生命周期和 callback 线程由 public C API 明确约束；exception、STL container 和 allocator
  ownership 不跨 DLL。
- binding crate 的 `build.rs` 单向读取 module header，经公共 codegen 生成到
  `build/bindings/{TARGET}/cxx/{crate}/`，再链接部署到 Cargo profile 目录的 import library。
- CMake/native project 不知道 Cargo crate，也不得链接或回调 Rust symbol。
- Vulkan 命令 binding 通过 owner 指定的 namespace import 复用 ash 类型；公共 codegen 不维护 Vulkan 类型映射。
- Streamline 的 RenderGraph pass 顺序和 Vulkan resource 生命周期仍由 Rust Renderer/Runtime owner 管理。

Streamline Rust 生命周期细节见
[`truvis-streamline-binding/README.md`](rust/bindings/truvis-streamline-binding/README.md)。

Vulkan 命令桥接和宿主 ABI 契约见
[`truvixx-vk/README.md`](modules/truvixx-vk/README.md) 与
[`truvis-vk-binding/README.md`](rust/bindings/truvis-vk-binding/README.md)。

## 生成目录

- CMake binary dir：`build/cxx/cmake/{vs2022,vs2026,clang-cl}`。
- Native 输出：`build/cxx/output/{toolchain}/{Debug,Release}`。
- export header：`build/cxx/generated/include/`。
- runtime plan：各 CMake binary dir 的 `runtime/`；configure 只生成脚本，成功 build 才发布版本 2 清单。
- Rust 部署所有权：`build/cxx/.state/`。
- Rust binding：`build/bindings/{TARGET}/cxx/{crate}/`。
- clangd database：`build/cxx/compile_commands.json` 与 `.vscode/compile_commands.json`。

需要 PATH 上的 CMake、MSVC C++ workload、`VCPKG_ROOT`，以及可选的 clang-cl/Ninja。
