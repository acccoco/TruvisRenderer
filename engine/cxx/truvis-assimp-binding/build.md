目录结构

* `build/cxx/output/Debug` 和 `build/cxx/output/Release` 是 CMake 输出 `.lib` / `.dll` / `.pdb` 的位置
* Cargo 输出目录当前为 `build/debug` 和 `build/release`，是 `cxx-build` 复制运行时产物的位置，供 Rust 可执行文件加载
* `compile_commands.json` 由 `truvis-cxx-build` 从 `clang-cl-debug` preset 生成并同步到 `build/cxx/compile_commands.json` 和 `.vscode/compile_commands.json`

```
// truvis-assimp-binding/build.rs 生成 Assimp C API 的 FFI 绑定，并声明链接 truvixx-assimp-capi。
// 不需要在 Rust 源码中手写 #[link] extern block。
```

基本思路：

- `truvis-cxx-build` 负责调用 CMake preset，构建 C++ target，并把 `.lib` / `.dll` / `.pdb` 复制到 Cargo 输出目录
- `truvis-assimp-binding/build.rs` 负责从 `mods/truvixx-assimp/include/TruvixxAssimp/c_api/module.h` 生成 `_ffi_bindings.rs`
- `truvis-assimp-binding/build.rs` 通过 `cargo:rustc-link-search` 和 `cargo:rustc-link-lib` 链接 `truvixx-assimp-capi`
