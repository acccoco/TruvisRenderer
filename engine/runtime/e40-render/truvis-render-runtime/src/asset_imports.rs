use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slotmap::{SlotMap, new_key_type};

use truvis_asset::asset_load_service::{AssetLoadEvent, AssetLoadService};
use truvis_asset::handle::{
    EmbeddedTextureId, LoadStatus, MaterialAssetHandle, MeshAssetHandle, RawMaterialData, RawSceneData,
    RawTextureSource, SceneLoadDesc, SceneLoadHandle, TextureAssetHandle, TextureColorSpace, TextureLoadDesc,
    TextureLoadHandle,
};
use truvis_asset::material::MaterialData;
use truvis_asset::material_texture::TextureChannel;

use crate::asset_system::{AssetSource, AssetSystem, TextureState};

new_key_type! {
    /// 一次场景导入的查询身份，存活到 Runtime 销毁。
    pub struct SceneImportHandle;
}

/// 已完成资源身份解析的 scene 描述；它不创建 `GameWorld` instance。
#[derive(Clone, Debug)]
pub struct SceneData {
    pub scene_path: PathBuf,
    pub objects: Vec<SceneObjectData>,
}

#[derive(Clone, Debug)]
pub struct SceneObjectData {
    pub name: String,
    pub mesh: MeshAssetHandle,
    pub materials: Vec<MaterialAssetHandle>,
    pub transform: glam::Mat4,
}

struct SceneImportRecord {
    status: LoadStatus,
    error: Option<String>,
    data: Option<SceneData>,
}

/// 仅在 scene 预检阶段存在；data 中的 texture handle 是未发布占位，提交时按请求逐一替换。
struct PreparedSceneMaterial {
    data: MaterialData,
    textures: [Option<(AssetSource, TextureLoadDesc)>; TextureChannel::COUNT],
}

/// Runtime 的加载协调。worker 交付内存数据，本对象在 RenderThread 将其登记到资产表。
///
/// 导入结果描述可实例化的对象，由上层选择何时加入哪个 World。
pub struct AssetImports {
    load_service: AssetLoadService,
    scene_imports: SlotMap<SceneImportHandle, SceneImportRecord>,
    scene_loads: HashMap<SceneLoadHandle, SceneImportHandle>,
    texture_loads: HashMap<TextureLoadHandle, TextureAssetHandle>,
}

impl AssetImports {
    pub(crate) fn new() -> Self {
        Self {
            load_service: AssetLoadService::new(),
            scene_imports: SlotMap::with_key(),
            scene_loads: HashMap::new(),
            texture_loads: HashMap::new(),
        }
    }

    pub(crate) fn poll(&mut self, assets: &mut AssetSystem) {
        for event in self.load_service.update() {
            self.ingest_asset_event(assets, event);
        }
    }

    pub fn import_texture(
        &mut self,
        assets: &mut AssetSystem,
        path: PathBuf,
        color_space: TextureColorSpace,
    ) -> Result<TextureAssetHandle, String> {
        let path = std::fs::canonicalize(&path)
            .map_err(|err| format!("failed to canonicalize texture path '{}': {err}", path.display()))?;
        Ok(self.request_texture(
            assets,
            AssetSource::File { path: path.clone() },
            color_space,
            TextureLoadDesc::File { path, color_space },
        ))
    }

    /// 先成功派发请求，再发布 Loading 状态。Failed 可在原 identity 上显式重试。
    fn request_texture(
        &mut self,
        assets: &mut AssetSystem,
        source: AssetSource,
        color_space: TextureColorSpace,
        desc: TextureLoadDesc,
    ) -> TextureAssetHandle {
        if let Some(handle) = assets.find_texture(&source, color_space) {
            if assets.texture_record(handle).is_some_and(|record| record.state != TextureState::Failed) {
                return handle;
            }
        }

        let load = self.load_service.request_texture(desc);
        let handle = assets.begin_texture_load(source, color_space);
        self.texture_loads.insert(load, handle);
        handle
    }

    pub fn import_scene(&mut self, path: PathBuf) -> SceneImportHandle {
        let path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(err) => {
                // 错误通过返回的 handle 查询。
                return self.scene_imports.insert(SceneImportRecord {
                    status: LoadStatus::Failed,
                    error: Some(format!("failed to canonicalize scene path '{}': {err}", path.display())),
                    data: None,
                });
            }
        };
        let scene_load = self.load_service.request_scene(SceneLoadDesc { path });
        let scene_import = self.scene_imports.insert(SceneImportRecord {
            status: LoadStatus::Loading,
            error: None,
            data: None,
        });
        self.scene_loads.insert(scene_load, scene_import);
        scene_import
    }

    pub fn scene_import_status(&self, handle: SceneImportHandle) -> Option<LoadStatus> {
        self.scene_imports.get(handle).map(|record| record.status)
    }
    pub fn scene_import_error(&self, handle: SceneImportHandle) -> Option<&str> {
        self.scene_imports.get(handle).and_then(|record| record.error.as_deref())
    }
    pub fn scene_data(&self, handle: SceneImportHandle) -> Option<&SceneData> {
        self.scene_imports.get(handle).and_then(|record| record.data.as_ref())
    }
    fn ingest_asset_event(&mut self, assets: &mut AssetSystem, event: AssetLoadEvent) {
        match event {
            AssetLoadEvent::TextureLoaded { handle, desc, data } => {
                let Some(texture) = self.texture_loads.remove(&handle) else {
                    log::error!("AssetImports: unknown texture load handle {handle:?}");
                    return;
                };
                if assets.contains_texture(texture) {
                    assets.mark_texture_loaded(texture, data);
                    log::debug!("Texture ready {:?}: {}", texture, desc.source_label());
                }
            }
            AssetLoadEvent::TextureFailed { handle, desc, error } => {
                let Some(texture) = self.texture_loads.remove(&handle) else {
                    log::error!("AssetImports: unknown texture load handle {handle:?}");
                    return;
                };
                if assets.contains_texture(texture) {
                    assets.mark_texture_failed(texture, error.clone());
                    log::error!("Texture load failed {:?} ({}): {}", texture, desc.source_label(), error);
                }
            }
            AssetLoadEvent::SceneLoaded { handle, desc, data } => {
                self.ingest_scene_loaded(assets, handle, desc.path, data)
            }
            AssetLoadEvent::SceneFailed { handle, error, .. } => {
                let scene_import = self.take_scene_import_for_load(handle);
                self.fail_scene_import(scene_import, error);
            }
        }
    }

    fn ingest_scene_loaded(
        &mut self,
        assets: &mut AssetSystem,
        scene_load: SceneLoadHandle,
        scene_path: PathBuf,
        raw: RawSceneData,
    ) {
        let scene_import = self.take_scene_import_for_load(scene_load);
        // 所有可返回错误的校验与路径解析先完成，失败不能登记资源或重试共享纹理。
        let prepared = (|| {
            Self::validate_scene_payload(&raw)?;
            for mesh in &raw.meshes {
                mesh.validate().map_err(|error| error.to_string())?;
            }
            let meshes = raw.meshes;
            let materials = raw
                .materials
                .into_iter()
                .map(|material| Self::prepare_scene_material(&scene_path, material))
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, String>((meshes, materials, raw.instances))
        })();
        let (prepared_meshes, prepared_materials, instances) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fail_scene_import(scene_import, error);
                return;
            }
        };

        let meshes: Vec<_> = prepared_meshes.into_iter().map(|mesh| assets.insert_validated_mesh(mesh)).collect();
        let mut materials = Vec::with_capacity(prepared_materials.len());
        for mut material in prepared_materials {
            for (channel, request) in TextureChannel::ALL.into_iter().zip(material.textures) {
                let Some((source, desc)) = request else {
                    continue;
                };
                let texture = self.request_texture(assets, source, channel.color_space(), desc);
                material.data.textures[channel as usize].as_mut().expect("prepared texture slot missing").texture =
                    texture;
            }
            materials.push(assets.register_material(material.data).expect("validated material"));
        }
        let objects = instances
            .into_iter()
            .map(|instance| SceneObjectData {
                name: instance.name,
                mesh: meshes[instance.mesh_index as usize],
                materials: instance.material_indices.into_iter().map(|index| materials[index as usize]).collect(),
                transform: instance.transform,
            })
            .collect();
        let record =
            self.scene_imports.get_mut(scene_import).expect("AssetImports: scene import disappeared during ingest");
        record.status = LoadStatus::Ready;
        record.error = None;
        record.data = Some(SceneData { scene_path, objects });
    }

    fn validate_scene_payload(raw: &RawSceneData) -> Result<(), String> {
        for instance in &raw.instances {
            if instance.mesh_index as usize >= raw.meshes.len() {
                return Err(format!(
                    "scene instance '{}' references missing mesh {}",
                    instance.name, instance.mesh_index
                ));
            }
            for &material_index in &instance.material_indices {
                if material_index as usize >= raw.materials.len() {
                    return Err(format!(
                        "scene instance '{}' references missing material {}",
                        instance.name, material_index
                    ));
                }
            }
            let expected = raw.meshes[instance.mesh_index as usize].submesh_count();
            if instance.material_indices.len() != expected {
                return Err(format!(
                    "scene instance '{}' material count mismatch: expected {}, got {}",
                    instance.name,
                    expected,
                    instance.material_indices.len()
                ));
            }
        }
        Ok(())
    }

    fn prepare_scene_material(scene_path: &Path, material: RawMaterialData) -> Result<PreparedSceneMaterial, String> {
        let mut prepared = PreparedSceneMaterial {
            data: MaterialData {
                base_color: material.base_color,
                metallic: material.metallic,
                roughness: material.roughness,
                class: material.class,
                coverage: material.coverage,
                textures: Default::default(),
                normal_scale: material.normal_scale,
                emissive_factor: material.emissive_factor,
                name: material.name,
            },
            textures: Default::default(),
        };
        for (channel, slot) in TextureChannel::ALL.into_iter().zip(material.textures) {
            if let Some(slot) = slot {
                prepared.textures[channel as usize] =
                    Some(Self::resolve_scene_texture(scene_path, slot.texture.clone(), channel)?);
                prepared.data.textures[channel as usize] = Some(slot.with_texture(TextureAssetHandle::default()));
            }
        }
        prepared.data.validate()?;
        Ok(prepared)
    }

    fn resolve_scene_texture(
        scene_path: &Path,
        source: RawTextureSource,
        channel: TextureChannel,
    ) -> Result<(AssetSource, TextureLoadDesc), String> {
        let color_space = channel.color_space();
        match source {
            RawTextureSource::ExternalPath(path) => {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    scene_path.parent().unwrap_or_else(|| Path::new("")).join(path)
                };
                let canonical = std::fs::canonicalize(&resolved).map_err(|err| {
                    format!("failed to canonicalize {} texture path '{}': {err}", channel.name(), resolved.display())
                })?;
                Ok((
                    AssetSource::File {
                        path: canonical.clone(),
                    },
                    TextureLoadDesc::File {
                        path: canonical,
                        color_space,
                    },
                ))
            }
            RawTextureSource::Embedded {
                identity: EmbeddedTextureId { image_index },
                bytes,
                mime_type,
            } => {
                // scene_path 已在请求入口 canonicalize；owned bytes 不依赖源 scene 文件继续存在。
                Ok((
                    AssetSource::Embedded {
                        scene_path: scene_path.to_path_buf(),
                        image_index,
                    },
                    TextureLoadDesc::Embedded {
                        identity: EmbeddedTextureId { image_index },
                        bytes,
                        mime_type,
                        color_space,
                    },
                ))
            }
        }
    }

    fn take_scene_import_for_load(&mut self, load: SceneLoadHandle) -> SceneImportHandle {
        self.scene_loads.remove(&load).expect("AssetImports: received event for unknown scene load handle")
    }

    fn fail_scene_import(&mut self, handle: SceneImportHandle, error: String) {
        let record = self.scene_imports.get_mut(handle).expect("AssetImports: scene import record disappeared");
        record.status = LoadStatus::Failed;
        record.error = Some(error.clone());
        record.data = None;
        log::error!("AssetImports: scene {:?} failed: {}", handle, error);
    }
}
