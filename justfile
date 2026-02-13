set shell := ["powershell.exe", "-c"]

build-all: shader cxx
	cargo build --all

# 编译着色器
shader:
	cargo run --bin shader-build
	cargo build -p truvis-shader-binding

# 编译 cxx 项目，编译 cxx-binding
cxx:
	cargo run --bin cxx-build
	cargo build -p truvis-cxx-binding

# 配置 cxx 的 cmake 项目
[working-directory: "engine/cxx"]
cxx-preset-clang:
	cmake --preset clang-cl-debug

[working-directory: "engine/cxx"]
cxx-build-clang:
	cmake --build --preset clang-cl-build-debug

[working-directory: "engine/cxx"]
cxx-build-clang-release:
	cmake --build --preset clang-cl-build-release

[working-directory: "engine/cxx"]
cxx-preset-clang-release:
	cmake --preset clang-cl-release

cornell: shader cxx
	cargo run --bin rt-cornell

sponza: shader cxx
	cargo run --bin rt-sponza

