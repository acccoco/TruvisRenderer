use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;

use slotmap::SlotMap;

use truvis_asset::handle::{
    MaterialAssetHandle, MeshAssetHandle, MeshData, TextureAssetHandle, TextureBytes, TextureColorSpace,
};
use truvis_asset::material::MaterialData;

use truvis_gfx::commands::barrier::GfxBarrierMask;
use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::gfx::{GfxDeviceCtx, GfxQueueCtx, GfxResourceCtx};
use truvis_render_foundation::frame_label::FrameLabel;

use self::mesh_geometry::MeshRenderData;
use crate::bindings::bindless_manager::BindlessSrvHandle;
use crate::bindings::shader_binding_system::ShaderBindingSystem;
use crate::resources::gfx_resource_registry::GfxResourceRegistry;
use crate::resources::gpu_asset_upload_queue::{CompletedTextureUpload, GpuAssetUploadQueue};

use self::gpu_mesh_store::GpuMeshStore;
use self::gpu_texture_store::GpuTextureStore;
pub(crate) use self::material_table::MaterialRevisions;
use self::material_table::MaterialTable;

mod gpu_mesh_store;
pub(crate) mod gpu_texture_store;
mod material_table;
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

/// Runtime 持有的共享资产。Scene 和导入任务分别由 World 与导入协调持有。
pub struct AssetSystem {
    textures: SlotMap<TextureAssetHandle, TextureRecord>,
    texture_by_key: HashMap<TextureKey, TextureAssetHandle>,
    meshes: SlotMap<MeshAssetHandle, MeshData>,
    materials: MaterialTable,
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
        frame_id: u64,
    ) -> Self {
        Self {
            textures: SlotMap::with_key(),
            texture_by_key: HashMap::new(),
            meshes: SlotMap::with_key(),
            materials: MaterialTable::new(resource_ctx, frame_id),
            revision: 0,
            gpu_textures: GpuTextureStore::new(),
            gpu_meshes: GpuMeshStore::new(device_ctx, queue_ctx, frame_id),
        }
    }

    pub(crate) fn begin_frame(&mut self, frame_id: u64) {
        self.gpu_textures.begin_frame(frame_id);
        self.gpu_meshes.begin_frame(frame_id);
        self.materials.begin_frame(frame_id);
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
        self.gpu_meshes.sync_scene(&self.meshes, resource_ctx, device_ctx, queue_ctx);
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
        self.materials.resolve_waiting(&self.textures, &self.gpu_textures);
    }

    pub(crate) fn upload_materials(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        cmd: &GfxCommandBuffer,
        barrier: GfxBarrierMask,
        label: FrameLabel,
    ) {
        self.materials.upload(resource_ctx, cmd, barrier, label);
    }

    pub(crate) fn commit_uploaded_frame(&mut self, label: FrameLabel) {
        self.materials.commit_submitted_frame(label);
    }

    pub(crate) fn material_buffer_device_address(&self, label: FrameLabel) -> ash::vk::DeviceAddress {
        self.materials.material_buffer_device_address(label)
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

    /// 材质登记成功后，逻辑 slot 和材质参数在同一查询中有效。
    pub(crate) fn material_binding(&self, handle: MaterialAssetHandle) -> Option<(u32, &MaterialData)> {
        self.materials.binding(handle)
    }

    pub(crate) fn material_render_revision(&self, handle: MaterialAssetHandle) -> Option<MaterialRevisions> {
        self.materials.revision(handle)
    }

    pub(crate) fn available_material_slots(&self) -> usize {
        self.materials.free_slot_count()
    }

    /// Runtime 已关闭共享上传队列并确认在飞帧结束，资源才能进入显式释放。
    pub(crate) fn destroy(
        self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        registry: &mut GfxResourceRegistry,
        bindings: &mut ShaderBindingSystem,
    ) {
        self.materials.destroy(resource_ctx);
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
        self.materials.handles()
    }

    pub fn material_data(&self, handle: MaterialAssetHandle) -> Option<&MaterialData> {
        self.materials.data(handle)
    }

    pub fn register_material(&mut self, data: MaterialData) -> anyhow::Result<MaterialAssetHandle> {
        self.validate_texture_refs(&data)?;
        let handle = self.materials.register(data, &self.textures, &self.gpu_textures)?;
        self.revision = self.revision.saturating_add(1);
        Ok(handle)
    }

    pub fn update_material(&mut self, handle: MaterialAssetHandle, data: MaterialData) -> anyhow::Result<()> {
        self.validate_texture_refs(&data)?;
        if self.materials.update(handle, data, &self.textures, &self.gpu_textures)? {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }

    fn validate_texture_refs(&self, data: &MaterialData) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.textures.iter().flatten().all(|slot| self.contains_texture(slot.texture)),
            "material references a missing texture",
        );
        Ok(())
    }

    /// 调用方在随后的提交前处理材质引用；GPU 对象随后按帧同步退役。
    pub(crate) fn remove_material(&mut self, handle: MaterialAssetHandle) -> bool {
        let removed = self.materials.remove(handle);
        self.revision = self.revision.saturating_add(u64::from(removed));
        removed
    }

    /// 调用方在随后的提交前处理实例引用。
    pub(crate) fn remove_mesh(&mut self, handle: MeshAssetHandle) -> bool {
        let removed = self.meshes.remove(handle).is_some();
        self.revision = self.revision.saturating_add(u64::from(removed));
        removed
    }

    /// 调用方在随后的提交前处理材质与天空引用。
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
