# assets 与 bindless 解耦分析（2026-04-23）

> 本文基于 2026-04-23 的代码现状，讨论 `truvis-asset` 与 `BindlessManager` 的耦合问题。
> 重点回答两个问题：
>
> - assets 模块和 bindless 应该如何拆分边界？
> - 拆分后，从 `TextureHandle` 到 `BindlessHandle` 的访问流程应该如何组织？
>
> 对照对象：
>
> - Bevy：`Asset Handle -> RenderAsset -> RenderAssets`
> - Filament：`MaterialInstance parameter -> Texture + TextureSampler`
> - Falcor：`CpuTextureHandle` 与 GPU-side texture handle 分离
> - Unreal Engine：`UTexture` / `FTextureResource` / RHI resource 分层

---

## 1. 总体判断

`assets` 不应该直接知道 bindless。

更准确地说，当前系统里有三种不同身份被混在同一条调用链里：

```text
AssetTextureHandle
  -> GfxImageViewHandle
  -> BindlessSrvHandle
```

这三者分别代表不同语义：

| 类型 | 所属层 | 含义 | 是否应暴露给上层场景 |
|---|---|---|---|
| `AssetTextureHandle` | asset / scene | CPU 资产语义，“我要哪张贴图” | 是 |
| `GfxImageHandle` / `GfxImageViewHandle` | render resource | GPU 资源身份，“这张贴图上传成哪个 image/view” | 否 |
| `BindlessSrvHandle` | descriptor / shader visible | shader 可见 descriptor slot，“shader 用哪个 index 采样” | 否，最多进入 GPU material |

因此，理想边界应该是：

```text
scene/material 只保存 AssetTextureHandle
asset 只管理加载状态和 CPU/GPU ready 状态
render prepare 阶段负责 AssetTextureHandle -> BindlessSrvHandle 的解析
shader 只接收 BindlessSrvHandle
```

当前路径的问题不是“中间经过的对象太多”，而是“这些对象暴露在了不该知道它们的模块里”。

---

## 2. 当前耦合点

### 2.1 `AssetHub` 直接依赖 `BindlessManager`

当前 `AssetHub::new()` / `AssetHub::update()` / `AssetHub::destroy()` 都接收 `BindlessManager`。

典型行为：

```text
AssetHub::new()
  -> create_fallback_texture()
  -> bindless_manager.register_srv(fallback_view)

AssetHub::update()
  -> texture upload finished
  -> create image view
  -> bindless_manager.register_srv(view_handle)

AssetHub::destroy()
  -> bindless_manager.unregister_srv(fallback_view)
```

这意味着资产层不仅知道 GPU image/view，还知道 shader-visible descriptor 注册策略。

这个边界过宽。

资产层可以知道“纹理上传完成，产生了 view handle”，但不应该决定：

```text
这个 view 是否进入 bindless
进入 SRV 还是 UAV bindless table
何时注册 / unregister
descriptor slot 如何回收
是否有 fallback bindless slot
```

这些都属于 render backend / prepare 阶段。

### 2.2 `SceneManager::prepare_render_data()` 同时依赖 asset 和 bindless

当前 `SceneManager::prepare_render_data()` 接收：

```rust
bindless_manager: &BindlessManager,
asset_hub: &AssetHub,
```

并在构建 material render data 时执行：

```text
material.diffuse_map path
  -> asset_hub.get_texture_by_path()
  -> AssetTexture.view_handle
  -> bindless_manager.get_shader_srv_handle(view_handle)
  -> MaterialRenderData.diffuse_bindless_handle
```

这让 `truvis-scene` 同时知道：

```text
1. asset path / AssetHub
2. GPU image view
3. bindless descriptor slot
4. shader binding 类型
```

这与 CPU scene 的职责不一致。

CPU scene 应该回答：

```text
场景里有哪些 mesh/material/instance/light？
material 引用了哪个 AssetTextureHandle？
```

不应该回答：

```text
shader 应该用哪个 bindless descriptor index 采样这张贴图？
```

### 2.3 `MaterialManager::TextureResolver` 是正确方向，但层级仍偏低

`truvis-scene::material_manager` 中已经有：

```rust
pub trait TextureResolver {
    fn is_texture_ready(&self, handle: AssetTextureHandle) -> bool;
    fn get_srv_handle(&self, handle: AssetTextureHandle) -> Option<BindlessSrvHandle>;
}
```

这个抽象方向是对的：材质系统不直接持有 `AssetHub + BindlessManager`，而是通过 resolver 查询。

但它仍然有两个问题：

1. trait 定义在 `truvis-scene` 中，使 scene crate 认识 `BindlessSrvHandle`。
2. resolver 返回的是 bindless 句柄，而不是更完整的 texture binding 结果。

更干净的做法是把 `TextureResolver` 放到 render prepare / material upload 层，让 `truvis-scene` 只保留 CPU material params。

---

## 3. 参考项目对照

### 3.1 Bevy：CPU asset 与 GPU asset 分世界

Bevy 的关键设计是：

```text
main world asset
  -> extract
render world asset
  -> prepare
RenderAssets<T>
```

Bevy 的 `RenderAsset` trait 描述“如何把 main world 的 source asset 准备成 render world 的 GPU representation”。文档中明确写到：`SourceAsset` 会在 extract 阶段转移到 render world，随后在 `PrepareAssets` 阶段转换为 GPU representation。

对应到 Truvis：

```text
AssetTextureHandle / RawTextureData   <- main/world asset 语义
GpuTexture / GfxImageViewHandle       <- render world GPU representation
BindlessSrvHandle                     <- backend descriptor exposure
```

可借鉴点：

- `AssetHub` 不应该同时承担 CPU asset 与 render asset 的职责。
- GPU representation 应该属于 `RenderWorld` 或 render-side asset cache。
- 纹理上传预算、异步 ready、卸载 cleanup 都应在 prepare/update 边界显式处理。

不必照搬点：

- 不需要完整 ECS。
- 不需要完全复制 Bevy 的 system schedule。
- 当前项目用固定 phase + manager 方式即可。

参考：

- https://docs.rs/bevy/latest/bevy/render/render_asset/trait.RenderAsset.html
- https://docs.rs/bevy/latest/bevy/render/render_asset/index.html

### 3.2 Filament：material 只暴露语义参数，不暴露 descriptor slot

Filament 的 material 定义声明参数，例如 `sampler2d`。运行时通过 `MaterialInstance` 设置具体 texture 和 sampler。

核心启发：

```text
material/shader 作者看到的是“参数”
backend 内部决定如何绑定 texture/sampler
descriptor slot 不是资产系统语义的一部分
```

这与 Truvis 的推荐方向一致：

```text
Material
  -> AssetTextureHandle
Render material prepare
  -> TextureBinding { srv, sampler }
Shader
  -> SrvHandle index
```

Filament 还明确区分 texture 与 sampler，这提醒 Truvis 后续不要把 sampler 类型硬编码在 asset 或 material upload 的临时代码里。

推荐 `TextureBinding` 同时携带：

```rust
pub struct TextureBinding {
    pub srv: BindlessSrvHandle,
    pub sampler: gpu::ESamplerType,
    pub ready: bool,
    pub generation: u32,
}
```

参考：

- https://google.github.io/filament/main/materials.html

### 3.3 Falcor：显式区分 CPU texture handle 与 GPU-side texture handle

Falcor 7.0 release note 中有一条很有价值的变更：

```text
Rename TextureManager::TextureHandle to TextureManager::CpuTextureHandle
to avoid name clash with GPU-side TextureHandle.
Add convenience functions to convert between CPU and GPU texture handles.
```

这正好对应当前 Truvis 的问题。

Truvis 也应该明确命名：

```text
AssetTextureHandle        CPU/asset handle
GpuTextureHandle          render world GPU asset handle
BindlessSrvHandle         shader-visible descriptor handle
```

不要用一个笼统的 `TextureHandle` 覆盖多层语义。

参考：

- https://github.com/NVIDIAGameWorks/Falcor/releases

### 3.4 Unreal Engine：UTexture 与 render resource / RHI resource 分层

Unreal 的纹理大致分层是：

```text
UTexture
  -> FTextureResource
  -> TextureRHI / TextureReferenceRHI
```

`UTexture` 是资产 / UObject 层对象，`FTextureResource` 是 render thread 资源对象，RHI texture 是底层 GPU 资源。

可借鉴点：

- 资产对象和 render thread resource 生命周期分开。
- render resource 初始化在 render/RHI 边界发生。
- `TextureReferenceRHI` 用于在底层 RHI texture 变化时维护引用稳定性。

对应到 Truvis：

```text
AssetTextureHandle
  -> RenderTextureResource / GpuTexture
  -> GfxImageViewHandle
  -> BindlessSrvHandle
```

资产层不应该直接持有 shader-visible binding。

参考：

- https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/RenderCore/FRenderResource/InitRHI/2
- https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/Engine/FTextureResource/TextureReferenceRHI

---

## 4. 推荐目标结构

### 4.1 短期结构：先切断 bindless

最小可落地拆分：

```text
truvis-asset
  AssetHub
  AssetLoader
  AssetUploadManager
  AssetTextureHandle
  AssetTexture { image_handle, view_handle, sampler, ... }

truvis-renderer / render-side subsystem
  TextureBindingCache
  AssetTextureHandle -> TextureBinding
  register/unregister bindless

truvis-render-interface
  BindlessManager
  BindlessSrvHandle
```

短期仍允许 `truvis-asset` 创建 GPU image/view，因为当前 `AssetUploadManager` 已经深度依赖 `truvis-gfx`，一次性完全拆开成本较高。

但要先做到：

```text
AssetHub 不再 import BindlessManager
AssetHub 不再 register_srv / unregister_srv
Bindless 注册集中到 renderer/backend
```

可以引入事件：

```rust
pub enum AssetTextureEvent {
    Ready {
        handle: AssetTextureHandle,
        view_handle: GfxImageViewHandle,
        sampler: gpu::ESamplerType,
        generation: u32,
    },
    Removed {
        handle: AssetTextureHandle,
        view_handle: GfxImageViewHandle,
        generation: u32,
    },
}
```

然后：

```text
Renderer::update_assets()
  -> let events = AssetHub::update(&mut gfx_resource_manager)
  -> TextureBindingCache::apply_events(events, &mut bindless_manager)
```

### 4.2 中期结构：拆出 render-side GPU asset cache

中期推荐结构：

```text
truvis-asset
  AssetHub
  RawTextureData
  AssetTextureHandle
  LoadStatus

truvis-render-assets 或 renderer::subsystems::textures
  GpuTextureAssets
  TextureUploadQueue
  TextureBindingCache
  AssetTextureHandle -> GpuTextureHandle
  GpuTextureHandle -> GfxImageViewHandle
  GfxImageViewHandle -> BindlessSrvHandle
```

也就是：

```text
disk / CPU decode
  -> asset layer
GPU upload / view / descriptor
  -> render layer
```

这样更接近 Bevy 的 `Image -> GpuImage` 模型。

### 4.3 长期结构：资产层完全不依赖 gfx/render-interface

长期目标：

```text
truvis-asset dependencies:
  slotmap
  crossbeam/rayon
  image
  log/anyhow

not depend on:
  truvis-gfx
  truvis-render-interface
  ash
  vk-mem
  truvis-shader-binding
```

资产层输出 API：

```rust
pub struct RawTextureData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: AssetTextureFormat,
    pub mip_levels: u32,
    pub color_space: TextureColorSpace,
}
```

render-side prepare 再决定：

```text
AssetTextureFormat -> vk::Format
usage flags
image layout
view desc
sampler
bindless registration
```

---

## 5. 拆分后的访问流程

### 5.1 用户 / scene 侧

用户或 loader 创建 material 时：

```text
let albedo = asset_hub.load_texture(path);

material.diffuse_texture = Some(albedo);
```

此时返回的是 `AssetTextureHandle`，不要求贴图已经 ready。

CPU material 数据应类似：

```rust
pub struct ManagedMaterialParams {
    pub base_color: Vec4,
    pub diffuse_texture: Option<AssetTextureHandle>,
    pub normal_texture: Option<AssetTextureHandle>,
    pub metallic: f32,
    pub roughness: f32,
}
```

这里不出现：

```text
GfxImageViewHandle
BindlessSrvHandle
gpu::SrvHandle
```

### 5.2 asset update 阶段

每帧或固定 phase：

```text
AssetHub::poll()
  -> IO finished
  -> decoded RawTextureData ready
```

短期如果仍由 `AssetHub` 上传 GPU：

```text
AssetHub::update(&mut GfxResourceManager)
  -> Vec<AssetTextureEvent::Ready>
```

中长期则是：

```text
AssetHub::poll()
  -> Vec<DecodedAssetEvent>

GpuTextureAssets::prepare(decoded_events)
  -> create GfxImage / GfxImageView
  -> Vec<GpuTextureReadyEvent>
```

### 5.3 bindless 注册阶段

render-side texture binding cache 处理 ready event：

```text
TextureBindingCache::apply_ready_event()
  -> bindless_manager.register_srv(view_handle)
  -> srv = bindless_manager.get_shader_srv_handle(view_handle)
  -> map AssetTextureHandle -> TextureBinding
```

缓存结构可类似：

```rust
pub struct TextureBindingCache {
    bindings: SecondaryMap<AssetTextureHandle, TextureBinding>,
    fallback: TextureBinding,
}
```

`TextureBinding`：

```rust
pub struct TextureBinding {
    pub srv: BindlessSrvHandle,
    pub sampler: gpu::ESamplerType,
    pub ready: bool,
    pub generation: u32,
}
```

### 5.4 material prepare / upload 阶段

GPU material 构建时：

```text
MaterialManager::upload()
  -> resolver.resolve_texture(asset_texture_handle)
  -> TextureBinding
  -> write gpu::PBRMaterial
```

推荐 resolver API：

```rust
pub trait TextureResolver {
    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding;
}
```

而不是：

```rust
fn get_srv_handle(&self, handle: AssetTextureHandle) -> Option<BindlessSrvHandle>;
```

原因是 `TextureBinding` 能同时表达：

```text
是否 ready
fallback 是否被使用
sampler
generation
debug name / statistics
```

### 5.5 shader 访问阶段

shader 端保持当前 bindless 模式：

```text
gpu::PBRMaterial.diffuse_map: gpu::SrvHandle
gpu::PBRMaterial.diffuse_map_sampler_type: gpu::ESamplerType
```

shader 中：

```text
bindless_srv::sample(mat.diffuse_map, uv, mat.diffuse_map_sampler_type)
```

shader 不知道：

```text
AssetTextureHandle
GfxImageViewHandle
AssetHub
TextureBindingCache
```

---

## 6. TextureHandle 到 BindlessHandle 的路径是否太长？

结论：概念路径不长，但当前暴露路径太长。

合理路径：

```text
AssetTextureHandle
  -> TextureResolver
  -> TextureBinding
  -> BindlessSrvHandle
```

不合理路径：

```text
material path / AssetTextureHandle
  -> AssetHub::get_texture()
  -> AssetTexture.view_handle
  -> BindlessManager::get_shader_srv_handle()
  -> gpu::SrvHandle
```

当前路径的问题在于每一层都被调用者看见了。

应该把中间细节封装在 render prepare 中：

```rust
let binding = texture_resolver.resolve_texture(diffuse_handle);
gpu_material.diffuse_map = binding.srv.0;
gpu_material.diffuse_map_sampler_type = binding.sampler;
```

这样上层只看到一次解析。

### 6.1 解析频率

`AssetTextureHandle -> BindlessSrvHandle` 不应该在 draw/pass 阶段频繁解析。

推荐只在以下时机解析：

```text
material 创建
material 参数修改
texture loading -> ready
texture streaming / view replacement
texture unload / fallback replacement
```

解析结果写入 GPU material buffer 后，shader 直接读 `gpu::SrvHandle`。

### 6.2 generation 与 dirty 标记

建议为 texture binding 引入 generation：

```text
AssetTextureHandle A generation 1 -> fallback binding
AssetTextureHandle A generation 2 -> real texture binding
AssetTextureHandle A generation 3 -> streaming updated texture binding
```

MaterialManager 可以记录 material 使用的 texture generation。

当 `TextureBindingCache` 更新时：

```text
texture generation changed
  -> mark dependent materials dirty
  -> re-upload gpu::PBRMaterial
```

这样避免每帧全量重建 material buffer。

### 6.3 fallback 策略

当前 `AssetHub::get_texture()` 会在未 ready 时返回 fallback texture。

拆分后 fallback 应该移到 render binding 层：

```text
TextureBindingCache::resolve_texture(handle)
  if handle not ready:
    return fallback_binding
  else:
    return actual_binding
```

这样 `AssetHub` 不必把 fallback 注册进 bindless，也不必关心 shader fallback 行为。

---

## 7. 推荐实施路线

### P0：让 `AssetHub` 不再接收 `BindlessManager`

目标：

```text
AssetHub::new(&mut GfxResourceManager)
AssetHub::update(&mut GfxResourceManager) -> Vec<AssetTextureEvent>
AssetHub::destroy(&mut GfxResourceManager) -> Vec<AssetTextureEvent>
```

迁移行为：

```text
fallback register_srv
texture ready register_srv
texture unregister_srv
```

移动到：

```text
Renderer / TextureBindingCache
```

收益：

- `truvis-asset` 不再依赖 `BindlessManager`。
- bindless 注册策略集中在 render side。
- 后续切换 bindless table、fallback table、descriptor array 策略不会影响 asset。

### P0：引入 `TextureBindingCache`

建议放置位置：

```text
短期：truvis-renderer/src/subsystems/texture_binding_cache.rs
中期：truvis-render-assets crate
```

职责：

```text
AssetTextureHandle -> TextureBinding
管理 fallback binding
处理 ready / removed event
调用 BindlessManager register/unregister
提供 TextureResolver 实现
```

### P1：让 `SceneManager::prepare_render_data()` 不再接收 `AssetHub + BindlessManager`

当前：

```rust
prepare_render_data(&self, bindless_manager: &BindlessManager, asset_hub: &AssetHub)
```

建议改成：

```rust
snapshot(&self) -> SceneSnapshot
```

或过渡方案：

```rust
prepare_render_data(&self, texture_resolver: &dyn TextureResolver)
```

更推荐分两层：

```text
SceneManager::snapshot()
  -> SceneSnapshot

SceneBridge::build_render_data(snapshot, texture_resolver)
  -> RenderData
```

收益：

- `truvis-scene` 回到 CPU scene 语义。
- render data 构建归入 extract/prepare 边界。
- 后续 material incremental update 更容易做。

### P1：统一 material texture 表达

当前项目中同时存在：

```text
Material.diffuse_map: String path
ManagedMaterialParams.diffuse_texture: Option<AssetTextureHandle>
MaterialRenderData.diffuse_bindless_handle: BindlessSrvHandle
```

建议统一方向：

```text
CPU material:
  Option<AssetTextureHandle>

Extracted / render material:
  Option<AssetTextureHandle> 或 TextureBinding

GPU material:
  gpu::SrvHandle
```

尽量减少 path 在 render path 内反复参与解析。

### P2：资产层完全去 GPU 化

在完成 P0/P1 后，再考虑把 `AssetUploadManager` 从 `truvis-asset` 移到 render side。

目标依赖关系：

```text
truvis-asset 不依赖:
  truvis-gfx
  truvis-render-interface
  ash
  vk-mem
  truvis-shader-binding
```

这一步收益大，但改动面也大，不建议作为第一步。

---

## 8. 目标数据流

### 8.1 短期目标数据流

```text
load_texture(path)
  -> AssetTextureHandle

Frame update:
  AssetHub::update(gfx_resource_manager)
    -> Vec<AssetTextureEvent::Ready { handle, view_handle, sampler, generation }>

Renderer:
  TextureBindingCache::apply_events(events, bindless_manager)
    -> bindless register
    -> cache AssetTextureHandle -> TextureBinding

Scene extract / material prepare:
  Material AssetTextureHandle
    -> TextureResolver::resolve_texture()
    -> TextureBinding
    -> gpu::PBRMaterial

Shader:
  gpu::SrvHandle
    -> bindless_srvs[index]
```

### 8.2 长期目标数据流

```text
Disk
  -> AssetLoader
  -> RawTextureData
  -> AssetHub event

Render prepare:
  -> GpuTextureAssets upload
  -> GfxImage/GfxImageView
  -> TextureBindingCache
  -> BindlessManager
  -> TextureBinding

Material upload:
  -> gpu::PBRMaterial

Shader:
  -> bindless sample
```

---

## 9. API 草案

### 9.1 AssetHub

```rust
pub struct AssetHub {
    texture_states: SlotMap<AssetTextureHandle, LoadStatus>,
    texture_cache: HashMap<PathBuf, AssetTextureHandle>,
    textures: SecondaryMap<AssetTextureHandle, AssetTexture>,
}

impl AssetHub {
    pub fn load_texture(&mut self, path: PathBuf) -> AssetTextureHandle;

    pub fn get_status(&self, handle: AssetTextureHandle) -> LoadStatus;

    pub fn get_texture(&self, handle: AssetTextureHandle) -> Option<&AssetTexture>;

    pub fn update(
        &mut self,
        gfx_resource_manager: &mut GfxResourceManager,
    ) -> Vec<AssetTextureEvent>;
}
```

短期 `AssetTexture` 仍可包含：

```rust
pub struct AssetTexture {
    pub image_handle: GfxImageHandle,
    pub view_handle: GfxImageViewHandle,
    pub sampler: gpu::ESamplerType,
    pub is_srgb: bool,
    pub mip_levels: u32,
    pub generation: u32,
}
```

### 9.2 TextureBindingCache

```rust
pub struct TextureBindingCache {
    fallback: TextureBinding,
    bindings: SecondaryMap<AssetTextureHandle, TextureBinding>,
    view_by_asset: SecondaryMap<AssetTextureHandle, GfxImageViewHandle>,
}

impl TextureBindingCache {
    pub fn new(
        gfx_resource_manager: &mut GfxResourceManager,
        bindless_manager: &mut BindlessManager,
    ) -> Self;

    pub fn apply_events(
        &mut self,
        events: &[AssetTextureEvent],
        bindless_manager: &mut BindlessManager,
    );

    pub fn resolve(&self, handle: AssetTextureHandle) -> TextureBinding;
}
```

### 9.3 TextureResolver

```rust
pub trait TextureResolver {
    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding;
}

impl TextureResolver for TextureBindingCache {
    fn resolve_texture(&self, handle: AssetTextureHandle) -> TextureBinding {
        self.resolve(handle)
    }
}
```

---

## 10. 风险与注意事项

### 10.1 descriptor slot 生命周期

`BindlessManager` 当前有延迟回收逻辑：

```text
unregister
  -> slot dirty
  -> age >= FIF_COUNT
  -> reclaim
```

`TextureBindingCache` 不能绕过这套机制。

当 texture unload 或替换 view 时：

```text
旧 view unregister_srv
新 view register_srv
dependent material dirty
```

不能直接复用旧 slot 除非 `BindlessManager` 明确支持 safe update。

### 10.2 descriptor update 与 material upload 顺序

推荐顺序：

```text
begin_frame
asset update
texture binding cache apply events
bindless_manager.prepare_render_data()
material dirty update / scene upload
render graph execute
```

这样 material 写入的 `BindlessSrvHandle` 对应的 descriptor 至少已经被登记为 dirty，并会在 render 前写入 descriptor set。

### 10.3 fallback binding 不应归 asset 拥有

fallback texture 是 renderer 的视觉策略，不是 asset 系统的加载语义。

因此 fallback 可由 `TextureBindingCache` 或 `RenderDefaultTextures` 拥有。

### 10.4 sampler 不要丢失

当前 `AssetTexture` 里有 `sampler: gpu::ESamplerType`，material upload 又硬编码 `LinearRepeat`。

拆分时应明确 sampler 来源：

```text
asset import setting
material override
default texture policy
```

最小实现可以先沿用 `LinearRepeat`，但 API 应为 sampler 留位置。

### 10.5 路径到 handle 的解析应提前发生

`SceneManager::prepare_render_data()` 里通过 `mat.diffuse_map: String` 查 `AssetHub::get_texture_by_path()` 不适合长期保留。

建议模型加载阶段就把 path 转为 `AssetTextureHandle`。

---

## 11. 建议结论

最推荐的第一步不是重写整个 asset 系统，而是做两个小边界：

```text
1. AssetHub 不再接收 BindlessManager
2. 新增 TextureBindingCache，集中处理 AssetTextureHandle -> BindlessSrvHandle
```

然后再拆：

```text
3. SceneManager 不再直接接收 AssetHub + BindlessManager
4. TextureResolver 移到 render prepare/material upload 层
5. AssetUploadManager 从 truvis-asset 移到 render-side asset prepare
```

完成前两步后，主要调用链会从：

```text
SceneManager
  -> AssetHub
  -> GfxImageViewHandle
  -> BindlessManager
  -> BindlessSrvHandle
```

变成：

```text
Scene / Material
  -> AssetTextureHandle

Render prepare
  -> TextureResolver
  -> TextureBinding
  -> BindlessSrvHandle
```

这条链并没有消失，但它被放到了正确的层里。

最终目标是：

```text
asset 负责“资源是什么、是否 ready”
render asset cache 负责“GPU 资源是什么”
bindless 负责“shader 如何看见 GPU 资源”
material upload 负责“把解析结果写入 GPU material”
shader 只负责“按 handle/index 采样”
```

这与 Bevy、Filament、Falcor、Unreal 的共同经验一致：资产身份、GPU 资源身份、shader 绑定身份应明确分层，不要让 asset handle 直接携带 descriptor slot。

