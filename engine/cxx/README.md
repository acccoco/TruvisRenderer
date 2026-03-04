# Truvixx C++ 场景加载器 - AI 编程指南

## 项目概述
Truvixx 是为 Rust 渲染引擎 Truvis 提供 3D 资产加载能力的 C++ 库。生成一个共享库：
- `truvixx-interface.dll`：对外暴露 C FFI 接口，供 Rust 通过 FFI 调用
- `truvixx-assimp` (静态库)：基于 Assimp 的场景加载核心，被 interface 链接

## 架构设计

```
truvixx-interface.dll (C FFI 层, SHARED)
    └── truvixx-assimp (场景加载核心, STATIC)
            └── Assimp + GLM (vcpkg 依赖)
```

### 关键文件
| 文件 | 职责 |
|------|------|
| [truvixx-interface/include/TruvixxInterface/truvixx_api.h](truvixx-interface/include/TruvixxInterface/truvixx_api.h) | **FFI 入口**：所有 `extern "C"` 函数声明，Rust 调用此头文件 |
| [truvixx-assimp/include/TruvixxAssimp/base_type.h](truvixx-assimp/include/TruvixxAssimp/base_type.h) | C 兼容的 POD 类型：`TruvixxFloat4x4`, `TruvixxFloat3` 等 |
| [truvixx-assimp/include/TruvixxAssimp/scene_importer.hpp](truvixx-assimp/include/TruvixxAssimp/scene_importer.hpp) | Assimp 封装：`SceneImporter` 类 |
| [truvixx-assimp/include/TruvixxAssimp/scene_data.hpp](truvixx-assimp/include/TruvixxAssimp/scene_data.hpp) | 内部 C++ 数据结构：`SceneData`, `MeshInfo`, `MaterialData` |

## 构建命令

```powershell
# Visual Studio 2022
cmake --preset vs2022
cmake --build --preset vs2022-build-debug   # 或 vs2022-build-release

# Clang-cl + Ninja (更快)
cmake --preset clang-cl-debug
cmake --build --preset clang-cl-build-debug
```

输出位置：`build/output/Debug/` 或 `build/output/Release/`

## 依赖管理
项目使用 vcpkg manifest 模式（`vcpkg.json`），**不要**运行 `vcpkg install`。CMake 配置时自动安装。
- 需设置环境变量 `VCPKG_ROOT` 指向 vcpkg 安装目录
- 依赖版本锁定：Assimp 5.4.3, GLM 1.0.1#3

## FFI 接口规范

### 句柄模式 (不透明指针)
```cpp
// truvixx_api.h - 使用 typedef struct 前向声明
typedef struct TruvixxScene* TruvixxSceneHandle;

// truvixx_api.cpp - 实际定义
struct TruvixxScene {
    truvixx::SceneImporter importer;
};
```

### 函数签名约定
```cpp
// ✅ 正确模式
TruvixxSceneHandle TRUVIXX_INTERFACE_API truvixx_scene_load(const char* path);
ResType TRUVIXX_INTERFACE_API truvixx_mesh_get_info(TruvixxSceneHandle scene, uint32_t index, TruvixxMeshInfo* out);

// ❌ 禁止：返回 C++ 对象、抛异常、使用 std::string
```

### FFI 结构体设计
```cpp
// 字符串使用固定大小数组 (256 字节)
typedef struct {
    char name[256];           // 不用 std::string
    TruvixxFloat4 base_color; // 使用 base_type.h 中的 POD 类型
    float roughness;
} TruvixxMat;
```

## 数据访问模式

### 查询-分配-填充模式 (SOA 布局)
Rust 调用方需预分配缓冲区：
```cpp
// 1. 查询元信息
TruvixxMeshInfo info;
truvixx_mesh_get_info(scene, mesh_idx, &info);

// 2. 调用方分配 buffer (Rust 侧)

// 3. 填充数据 (两种方式)
truvixx_mesh_fill_positions(scene, mesh_idx, position_buffer);  // 拷贝到用户 buffer
const TruvixxFloat3* positions = truvixx_mesh_get_positions(scene, mesh_idx);  // 直接指针
```

### 坐标系约定
- **右手坐标系**：X-Right, Y-Up, Z-Out
- **矩阵存储**：列主序 (`TruvixxFloat4x4.col0[4]` 是第一列)
- **UV 原点**：左上角

## 修改检查清单
- [ ] FFI 函数保持 `extern "C"` 和 `TruvixxSceneHandle` 签名
- [ ] FFI 结构体使用固定大小数组，不用 STL 容器
- [ ] 新增 FFI 函数添加 `TRUVIXX_INTERFACE_API` 宏
- [ ] 返回 `ResType` 枚举而非 bool，支持扩展错误码
- [ ] 同时测试 VS2022 和 Clang-cl 构建
