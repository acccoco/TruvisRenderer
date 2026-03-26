# truvis-scene

CPU 侧场景数据管理，维护 Mesh / Material / Instance / Light 生命周期，
并将场景数据打包为 `RenderData` 供渲染层 GPU 上传。

## 模块结构

```
src/
├── guid_new_type.rs   # SlotMap Key 类型（MeshHandle / MaterialHandle / InstanceHandle / LightHandle）
├── scene_manager.rs   # SceneManager：四张 SlotMap 统一管理所有场景对象
├── components/
│   ├── mesh.rs        # Mesh = Vec<RtGeometry> + 可选 BLAS
│   ├── material.rs    # Material：PBR 参数 + 贴图路径
│   └── instance.rs    # Instance：(MeshHandle, Vec<MaterialHandle>, Mat4)
└── shapes/            # 内置几何体：triangle / rect / floor / cube，均返回 RtGeometry
```

## 核心结构

```rust
// 场景注册表
pub struct SceneManager {
    all_meshes:       SlotMap<MeshHandle, Mesh>,
    all_mats:         SlotMap<MaterialHandle, Material>,
    all_instances:    SlotMap<InstanceHandle, Instance>,
    all_point_lights: SlotMap<LightHandle, gpu::PointLight>,
}

// 几何体 + BLAS（build_blas() 原地构建，幂等）
pub struct Mesh {
    pub geometries:          Vec<RtGeometry>,
    pub blas:                Option<GfxAcceleration>,
    pub blas_device_address: Option<vk::DeviceAddress>,
    pub name:                String,
}

// PBR 材质，贴图路径在 prepare_render_data 时解析为 Bindless Handle
pub struct Material {
    pub base_color: Vec4,  pub emissive: Vec4,
    pub metallic: f32,     pub roughness: f32,  pub opaque: f32,
    pub diffuse_map: String,  pub normal_map: String,
}

// 实例：materials 与 Mesh.geometries 一一对应
pub struct Instance {
    pub mesh:      MeshHandle,
    pub materials: Vec<MaterialHandle>,
    pub transform: glam::Mat4,
}
```

## 内置几何体（`shapes/`）

坐标系：右手 Y-Up，三角形绕序 CCW。

| 类型 | 说明 |
|---|---|
| `TriangleSoA::create_mesh()` | XY 平面正立三角形，法线 +Z |
| `Rect` | 矩形面片 |
| `Floor` | 水平地面 |
| `Cube` | 单位立方体 |

## 关键方法

- `SceneManager::prepare_render_data(bindless_manager, asset_hub) -> RenderData`
  遍历所有场景数据，构建自包含快照（含 mesh/material/instance index 映射），供 GPU 层独立上传。

## 典型用法

```rust
let mut scene = SceneManager::new();
let mesh_h = scene.add_mesh(Mesh { geometries: vec![TriangleSoA::create_mesh()], .. });
let mat_h  = scene.add_material(Material { base_color: Vec4::ONE, .. });
scene.add_instance(Instance { mesh: mesh_h, materials: vec![mat_h], transform: Mat4::IDENTITY });

// 每帧调用，打包渲染数据
let render_data = scene.prepare_render_data(&bindless_manager, &asset_hub);
```
