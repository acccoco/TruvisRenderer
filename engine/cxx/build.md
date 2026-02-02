# 使用 VisualStudio 作为项目 generator

```shell
cmake --preset vs2022
# build debug
cmake --build --preset debug
# build release
cmake --build --preset release
```

# 使用 clang-cl 作为项目 generator

```shell
# debug
cmake --preset clang-cl-debug
cmake --build --preset clang-debug

# release
cmake --preset clang-cl-release
cmake --build --preset clang-release
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