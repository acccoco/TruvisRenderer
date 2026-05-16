# 使用 Visual Studio 作为项目 generator

推荐通过 workspace 命令自动检测 VS2026 / VS2022：

```shell
cargo run --bin cxx-build
```

`cxx-build` 只使用 PATH 上的 `cmake`。使用 VS2026 preset 时需要 CMake 4.2+。

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
  -S D:\code\Render-Rust-vk-Truvis\crates\truvis-cxx\cxx `
  -B D:\code\Render-Rust-vk-Truvis\crates\truvis-cxx\cxx\build-clang
```

# 更新 vcpkg

先更新 vcpkg 本地缓存：到达 vcpkg 目录，执行 `git pull` 

然后到达当前项目，更新 baseline: `vcpkg x-update-baseline`
