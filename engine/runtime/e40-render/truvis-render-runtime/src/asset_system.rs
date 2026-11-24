use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;

use indexmap::IndexSet;
use slotmap::SlotMap;

use truvis_asset::handle::{
    MaterialAssetHandle, MeshAssetHandle, MeshData, TextureAssetHandle, TextureBytes, TextureColorSpace,
};
use truvis_asset::material::MaterialData;

use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::frame_label::FrameLabel;

use self::mesh_geometry::MeshRenderData;
use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::resources::gfx_resource_registry::GfxResourceRegistry;
use crate::resources::gpu_asset_upload_queue::{CompletedTextureUpload, GpuAssetUploadQueue};

use self::gpu_mesh_store::GpuMeshStore;
use self::gpu_texture_store::GpuTextureStore;
use self::material_table::MaterialTable;

mod gpu_mesh_store;
pub(crate) mod gpu_texture_store;
mod material_table;
mod texture_resolver;
pub(crate) mod mesh_geometry;

/// 资源的可持久化来源描述。来源不进入资源 handle，而是随 record 保存。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssetSource {
    File { path: PathBuf },
    Embedded { scene_path: PathBuf, image_index: u32 },
}

/// CPU 纹理记录。`state` 表示 CPU loader 阶段，GPU ready 由 render-side owner 维护。
#[derive(Clone, Debug)]
pub struct TextureRecord {
    pub source: AssetSource,
    pub color_space: TextureColorSpace,
    pub state: TextureState,
    pub data: Option<TextureBytes>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureState {
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    source: AssetSource,
    color_space: TextureColorSpace,
}

/// Prepare 对账后交给 RenderWorld 的资源变化身份。
#[derive(Default)]
pub(crate) struct AssetRenderChanges {
    pub(crate) published_meshes: IndexSet<MeshAssetHandle>,
    pub(crate) appearance_changed_materials: IndexSet<MaterialAssetHandle>,
    pub(crate) emissive_changed_materials: IndexSet<MaterialAssetHandle>,
}

/// Runtime 持有的共享资产。Scene 和导入任务分别由 World 与导入协调持有。
pub struct AssetSystem {
    textures: SlotMap<TextureAssetHandle, TextureRecord>,
    texture_by_key: HashMap<TextureKey, TextureAssetHandle>,
    meshes: SlotMap<MeshAssetHandle, MeshData>,
    materials: SlotMap<MaterialAssetHandle, (MaterialData, u64)>,
    material_table: MaterialTable,
    render_changes: AssetRenderChanges,
    revision: u64,

    /// GPU 投影由同一 owner 推进，共享资产寿命与 World 无关。
    gpu_textures: GpuTextureStore,
    gpu_meshes: GpuMeshStore,
}

impl AssetSystem {
    pub(crate) fn new(
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
        registry: &mut GfxResourceRegistry,
        bindings: &mut ShaderBindingSystem,
        frame_id: u64,
    ) -> Self {
        Self {
            textures: SlotMap::with_key(),
            texture_by_key: HashMap::new(),
            meshes: SlotMap::with_key(),
            materials: SlotMap::with_key(),
            material_table: MaterialTable::new(resource_ctx, frame_id),
            render_changes: AssetRenderChanges::default(),
            revision: 0,
            gpu_textures: GpuTextureStore::new(resource_ctx, device_ctx, immediate_ctx, registry, bindings),
            gpu_meshes: GpuMeshStore::new(device_ctx, queue_ctx, frame_id),
        }
    }

    pub(crate) fn begin_frame(&mut self, frame_id: u64) {
        self.gpu_textures.begin_frame(frame_id);
        self.gpu_meshes.begin_frame(frame_id);
        self.material_table.begin_frame(frame_id);
    }

    /// Mesh 使用 graphics timeline，纹理 copy 由 Runtime 的共享 transfer queue 推进。
    pub(crate) fn submit_uploads(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        queue_ctx: GfxQueueCtx<'_>,
        registry: &mut GfxResourceRegistry,
        bindings: &mut ShaderBindingSystem,
        queue: &mut GpuAssetUploadQueue,
    ) {
        self.gpu_textures.remove_stale_textures(&self.textures, registry, bindings);
        self.gpu_meshes.remove_stale_meshes(&self.meshes);
        self.gpu_textures.submit_uploads(&self.textures, resource_ctx, device_ctx, queue_ctx, queue);
        self.render_changes.published_meshes.extend(
            self.gpu_meshes.sync_scene(&self.meshes, resource_ctx, device_ctx, queue_ctx),
        );
    }

    pub(crate) fn publish_textures(
        &mut self,
        completed: &mut VecDeque<CompletedTextureUpload>,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        registry: &mut GfxResourceRegistry,
        bindings: &mut ShaderBindingSystem,
    ) {
        self.gpu_textures.publish_completed_uploads(
            completed,
            &self.textures,
            resource_ctx,
            device_ctx,
            registry,
            bindings,
        );
        let changes = self.material_table.sync_scene(&self.materials, &self.gpu_textures);
        self.render_changes.appearance_changed_materials.extend(changes.appearance_changed_materials);
        self.render_changes.emissive_changed_materials.extend(changes.emissive_changed_materials);
    }

    pub(crate) fn take_render_changes(&mut self) -> AssetRenderChanges {
        std::mem::take(&mut self.render_changes)
    }

    pub(crate) fn upload_materials(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier: GfxBarrierMask,
        label: FrameLabel,
    ) {
        self.material_table.upload(resource_ctx, cmd, barrier, label);
    }

    pub(crate) fn commit_uploaded_frame(&mut self, label: FrameLabel) {
        self.material_table.commit_submitted_frame(label);
    }

    pub(crate) fn material_buffer_device_address(&self, label: FrameLabel) -> ash::vk::DeviceAddress {
        self.material_table.material_buffer_device_address(label)
    }

    pub(crate) fn resolve_mesh(&self, handle: MeshAssetHandle) -> Option<MeshRenderData<'_>> {
        self.gpu_meshes.resolve_mesh(handle)
    }

    pub(crate) fn mesh_is_ready(&self, handle: MeshAssetHandle) -> bool {
        self.gpu_meshes.is_ready(handle)
    }

    pub(crate) fn texture_binding(&self, handle: TextureAssetHandle) -> Option<BindlessSrvHandle> {
        self.gpu_textures.binding(handle)
    }

    /// Prepare 投影有了 slot 和参数后，实例才能进入渲染快照。
    pub(crate) fn material_binding(&self, handle: MaterialAssetHandle) -> Option<(u32, &MaterialData)> {
        let slot = self.material_table.resolve_material_slot(handle)?;
        Some((slot, self.material_table.material_data(handle)?))
    }

    /// Runtime 已关闭共享上传队列并确认在飞帧结束，资源才能进入显式释放。
    pub(crate) fn destroy(
        self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        registry: &mut GfxResourceRegistry,
        bindings: &mut ShaderBindingSystem,
    ) {
        self.material_table.destroy(resource_ctx);
        self.gpu_textures.destroy(resource_ctx, device_ctx, registry, bindings);
        self.gpu_meshes.destroy(resource_ctx, device_ctx);
    }

    /// Editor 等 CPU 消费者使用的注册表版本，与 GPU 发布和 FIF 无关。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn texture_record(&self, handle: TextureAssetHandle) -> Option<&TextureRecord> {
        self.textures.get(handle)
    }

    pub(crate) fn contains_texture(&self, handle: TextureAssetHandle) -> bool {
        self.textures.contains_key(handle)
    }

    pub(crate) fn texture_data(&self, handle: TextureAssetHandle) -> Option<&TextureBytes> {
        self.textures.get(handle)?.data.as_ref()
    }

    pub(crate) fn find_texture(
        &self,
        source: &AssetSource,
        color_space: TextureColorSpace,
    ) -> Option<TextureAssetHandle> {
        self.texture_by_key
            .get(&TextureKey {
                source: source.clone(),
                color_space,
            })
            .copied()
    }

    pub(crate) fn begin_texture_load(
        &mut self,
        source: AssetSource,
        color_space: TextureColorSpace,
    ) -> TextureAssetHandle {
        let key = TextureKey {
            source: source.clone(),
            color_space,
        };
        let record = TextureRecord {
            source,
            color_space,
            state: TextureState::Loading,
            data: None,
            error: None,
        };
        let handle = match self.texture_by_key.get(&key).copied() {
            Some(handle) => {
                self.textures[handle] = record;
                handle
            }
            None => {
                let handle = self.textures.insert(record);
                self.texture_by_key.insert(key, handle);
                handle
            }
        };
        self.revision = self.revision.saturating_add(1);
        handle
    }

    pub(crate) fn mark_texture_loaded(&mut self, handle: TextureAssetHandle, data: TextureBytes) {
        let record = &mut self.textures[handle];
        record.state = TextureState::Ready;
        record.data = Some(data);
        record.error = None;
    }

    pub(crate) fn mark_texture_failed(&mut self, handle: TextureAssetHandle, error: String) {
        let record = &mut self.textures[handle];
        record.state = TextureState::Failed;
        record.data = None;
        record.error = Some(error);
    }

    pub fn mesh_handles(&self) -> impl Iterator<Item = MeshAssetHandle> + '_ {
        self.meshes.keys()
    }

    pub fn mesh_data(&self, handle: MeshAssetHandle) -> Option<&MeshData> {
        self.meshes.get(handle)
    }

    pub fn register_mesh(&mut self, data: MeshData) -> anyhow::Result<MeshAssetHandle> {
        data.validate().map_err(anyhow::Error::msg)?;
        Ok(self.insert_validated_mesh(data))
    }

    /// 批量导入先校验整批输入，随后复用这一登记路径。
    pub(crate) fn insert_validated_mesh(&mut self, data: MeshData) -> MeshAssetHandle {
        let handle = self.meshes.insert(data);
        self.revision = self.revision.saturating_add(1);
        handle
    }

    pub fn material_handles(&self) -> impl Iterator<Item = MaterialAssetHandle> + '_ {
        self.materials.keys()
    }

    pub fn material_data(&self, handle: MaterialAssetHandle) -> Option<&MaterialData> {
        self.materials.get(handle).map(|(data, _)| data)
    }

    pub fn register_material(&mut self, data: MaterialData) -> anyhow::Result<MaterialAssetHandle> {
        self.validate_material(&data)?;
        let handle = self.materials.insert((data, 1));
        self.revision = self.revision.saturating_add(1);
        Ok(handle)
    }

    pub fn update_material(&mut self, handle: MaterialAssetHandle, data: MaterialData) -> anyhow::Result<()> {
        self.validate_material(&data)?;
        let (old, revision) = self.materials.get_mut(handle).ok_or_else(|| anyhow::anyhow!("stale material asset handle"))?;
        if *old != data {
            *old = data;
            *revision = revision.saturating_add(1);
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }

    fn validate_material(&self, data: &MaterialData) -> anyhow::Result<()> {
        data.validate().map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            data.textures.iter().flatten().all(|slot| self.contains_texture(slot.texture)),
            "material references a missing texture",
        );
        Ok(())
    }

    /// 通过 update context 检查场景引用后撤销登记，GPU 对象按帧退役。
    pub(crate) fn remove_material(&mut self, handle: MaterialAssetHandle) -> bool {
        let removed = self.materials.remove(handle).is_some();
        self.revision = self.revision.saturating_add(u64::from(removed));
        removed
    }

    /// 通过 update context 检查场景引用后撤销登记，GPU 对象按帧退役。
    pub(crate) fn remove_mesh(&mut self, handle: MeshAssetHandle) -> bool {
        let removed = self.meshes.remove(handle).is_some();
        self.revision = self.revision.saturating_add(u64::from(removed));
        removed
    }

    /// 通过 update context 检查场景引用后撤销登记，GPU 对象按帧退役。
    pub(crate) fn remove_texture(&mut self, handle: TextureAssetHandle) -> bool {
        let Some(record) = self.textures.remove(handle) else {
            return false;
        };
        self.texture_by_key.remove(&TextureKey {
            source: record.source,
            color_space: record.color_space,
        });
        self.revision = self.revision.saturating_add(1);
        true
    }
}
