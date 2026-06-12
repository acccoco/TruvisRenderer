# 使用 Visual Studio 作为项目 generator

推荐通过 workspace 命令自动检测 VS2026 / VS2022。日常运行 Truvis 时只需要 Debug 产物：

```shell
just cxx-debug
```

需要同时准备 Debug + Release 时使用：

```shell
just cxx
```

需要绕过 manifest、强制清理 profile 输出并重新构建时使用：

```shell
just cxx-force
```

`just cxx-debug` 会运行 `cargo run --bin cxx-build -- --profile debug` 并构建 CXX 绑定 crate；
`just cxx` 会运行 `cargo run --bin cxx-build -- --profile all`。`cxx-build` 在
`build/cxx/.state/` 记录 profile 级 manifest，输入未变化且 CMake/Cargo 输出仍存在时会跳过
CMake configure/build，只做必要的 DLL/json 同步检查。
`cxx-build` 只使用 PATH 上的 `cmake`。CMake presets 文件要求 CMake 3.21+；
使用 VS2026 preset 时需要 PATH 上的 CMake 4.2+。
CMake binary dir 位于 workspace 根目录的 `build/cxx/`，native 输出目录为
`build/cxx/output/{Debug,Release}`。Cargo 可执行文件和运行时复制目标仍是
`build/{debug,release}`。

## VS2026

```shell
cmake --preset vs2026
# build debug
cmake --build --preset vs2026-build-debug
# build release
cmake --build --preset vs2026-build-release
```

## VS2022

```shell
cmake --preset vs2022
# build debug
cmake --build --preset vs2022-build-debug
# build release
cmake --build --preset vs2022-build-release
```

# 使用 clang-cl 作为项目 generator

```shell
# debug
cmake --preset clang-cl-debug
cmake --build --preset clang-cl-build-debug

# release
cmake --preset clang-cl-release
cmake --build --preset clang-cl-build-release
```

对应的 cmake 命令为：

```shell
cmake `
  -DCMAKE_BUILD_TYPE=Debug `
  -DCMAKE_MAKE_PROGRAM=C:/Users/bigso/AppData/Local/Microsoft/WinGet/Links/ninja.exe `
  "-DCMAKE_C_COMPILER=C:/Program Files/LLVM/bin/clang-cl.exe" `
  "-DCMAKE_CXX_COMPILER=C:/Program Files/LLVM/bin/clang-cl.exe" `
  -G Ninja `
  -S D:\code\Render-Rust-vk-Truvis\engine\cxx `
  -B D:\code\Render-Rust-vk-Truvis\build\cxx\clang-cl\Debug
```

`compile_commands.json` 的复制由 `truvis-cxx-build` 负责：工具会尝试配置
`clang-cl-debug` preset，并把生成文件同步到 `build/cxx/compile_commands.json`
和 `.vscode/compile_commands.json`。该文件只服务 IDE/clangd，clang-cl 或 Ninja
不可用时不会阻断 Visual Studio preset 构建。

# 更新 vcpkg

先更新 vcpkg 本地缓存：到达 vcpkg 目录，执行 `git pull` 

然后到达当前项目，更新 baseline: `vcpkg x-update-baseline`
