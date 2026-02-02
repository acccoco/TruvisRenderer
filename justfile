set shell := ["powershell.exe", "-c"]

build-all: shader cxx
	cargo build --all

# 编译着色器
shader:
	cargo run --bin shader-build
	cargo build -p truvis-shader-binding

cxx:
	cargo run --bin cxx-build
	cargo build -p truvis-cxx-binding

cornell: shader cxx
	cargo run --bin rt-cornell

sponza: shader cxx
	cargo run --bin rt-sponza
