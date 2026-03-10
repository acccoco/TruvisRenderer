# CPU 侧场景数据模型

## 类图

```mermaid
classDiagram
    class SceneManager {
        all_instances: SlotMap~InstanceHandle, Instance~
        all_meshes: SlotMap~MeshHandle, Mesh~
        all_mats: SlotMap~MaterialHandle, Material~
        all_point_lights: SlotMap~LightHandle, PointLight~
    }

    class Instance {
        mesh: MeshHandle
        materials: Vec~MaterialHandle~
        transform: Mat4
    }

    class Mesh {
        geometries: Vec~RtGeometry~
        blas: Option~GfxAcceleration~
        blas_device_address: Option~DeviceAddress~
        name: String
    }

    class RtGeometry {
        vertex_buffer: RtVertexBuffer
        index_buffer: GfxBuffer
        index_cnt: u32
    }

    class Material {
        base_color: Vec4
        emissive: Vec4
        metallic: f32
        roughness: f32
        opaque: f32
        diffuse_map: String
        normal_map: String
    }

    SceneManager "1" o-- "0..*" Instance : all_instances
    SceneManager "1" o-- "0..*" Mesh : all_meshes
    SceneManager "1" o-- "0..*" Material : all_mats

    Instance "1" --> "1" Mesh : mesh (MeshHandle)
    Instance "1" --> "1..*" Material : materials (Vec~MaterialHandle~)

    Mesh "1" *-- "1..*" RtGeometry : geometries
```

## 关键约定

- `Instance.materials[i]` 与 `Mesh.geometries[i]` 一一对应，材质数量须与 submesh 数量严格对齐
- `SceneManager` 通过 `SlotMap` 管理所有资源，`Instance` 持有 `MeshHandle` / `MaterialHandle`，不直接拥有数据
- `Mesh.blas` 构建后才能用于 TLAS，`blas_device_address` 用于 `AccelerationStructureInstanceKHR`
