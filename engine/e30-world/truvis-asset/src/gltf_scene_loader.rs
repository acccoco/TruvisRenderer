//! glTF scene 导入任务。
//!
//! 本模块运行在 asset 后台线程中，职责只到“从 glTF 文件复制出 owned CPU 数据”。
//! 它不分配 asset handle、不创建 GPU resource，也不把 glTF crate 的借用对象传出任务。
//! 返回给 `AssetLoadService` 的数据必须保持为 `RawSceneData` 这套现有边界格式，后续 texture
//! 路径解析、scene handle 分配和 render upload event 生成统一收敛在 `AssetSystem`。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use gltf::buffer;

use crate::material_texture::{TextureChannel, TextureSlot, TextureTransform, TextureSampler, TextureWrap, TextureFilter};

use crate::asset_load_worker::{LoadResult, SceneLoadRequest};
use crate::handle::{
    CoverageMode, EmbeddedTextureId, MaterialClass, MeshData, RawMaterialData, RawSceneData,
    RawSceneInstanceData, RawTextureSource, SubmeshData,
};

/// 实际的 glTF scene 导入任务。
///
/// panic 会被转换为失败结果，避免后台导入异常越过 `AssetLoadService` 的状态机边界。
/// `req.handle` 只用于把结果关联回 `AssetLoadService` 已经分配的 model asset，不参与文件读取。
pub(crate) fn load_gltf_scene_task(req: SceneLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_gltf_scene_task");
    log::info!("Loading glTF scene: {:?}", req.desc.path);

    let result = std::panic::catch_unwind(|| GltfSceneReader::load_path(&req.desc.path))
        .map_err(|_| "glTF scene import task panicked".to_string())
        .and_then(|result| result);

    match result {
        Ok(data) => LoadResult::SceneSuccess {
            handle: req.handle,
            data,
        },
        Err(error) => {
            log::error!("Failed to load glTF scene {:?}: {}", req.desc.path, error);
            LoadResult::SceneFailure(req.handle, error)
        }
    }
}

/// glTF scene 的只读复制器。
///
/// Reader 拥有 document、buffer 和 embedded image encoded bytes，只在本后台任务内
/// 借用 glTF 对象读取 primitive、material 和 node tree。所有输出都立即复制到 Rust owned
/// Vec/String/PathBuf/Arc，确保任务结束后不会留下 glTF crate 内部借用。
struct GltfSceneReader {
    document: gltf::Document,
    buffers: Vec<buffer::Data>,
    source_path: PathBuf,
    model_name: String,
    embedded_images: Vec<Option<(Arc<[u8]>, Option<String>)>>,
}

impl GltfSceneReader {
    /// 加载一个 glTF / GLB 文件并复制成 `RawSceneData`。
    ///
    /// 只导入 buffer，不在 model task 中解码 image。外部 URI 保留为路径，GLB bufferView
    /// 和 data URI 复制 encoded bytes，后续由独立 texture task 异步解码。
    fn load_path(path: &Path) -> Result<RawSceneData, String> {
        if !path.exists() {
            return Err(format!("glTF scene file does not exist: {:?}", path));
        }

        let gltf = gltf::Gltf::open(path).map_err(|err| err.to_string())?;
        let document = gltf.document;
        let buffers = gltf::import_buffers(&document, path.parent(), gltf.blob).map_err(|err| err.to_string())?;
        // 仅复制实际受支持槽引用的内嵌图片；未消费的输入字段不产生图片资源。
        let mut used_images = std::collections::HashSet::new();
        for material in document.materials() {
            let pbr = material.pbr_metallic_roughness();
            for info in [pbr.base_color_texture(), pbr.metallic_roughness_texture(), material.emissive_texture()]
                .into_iter()
                .flatten()
            {
                used_images.insert(info.texture().source().index());
            }
            if let Some(info) = material.normal_texture() {
                used_images.insert(info.texture().source().index());
            }
        }
        let embedded_images = document
            .images()
            .map(|image| {
                if used_images.contains(&image.index()) {
                    Self::embedded_image_bytes(image, &buffers)
                } else {
                    Ok(None)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let reader = Self {
            document,
            buffers,
            source_path: path.to_path_buf(),
            model_name: Self::model_name(path),
            embedded_images,
        };

        reader.copy_scene()
    }

    /// 生成 scene 级默认名称。
    fn model_name(source_path: &Path) -> String {
        source_path.file_name().and_then(|name| name.to_str()).unwrap_or("scene").to_string()
    }

    /// 复制完整 glTF scene 数据。
    ///
    /// glTF material 绑定在 primitive 上，而当前 scene ingest 边界使用
    /// mesh + material index 组合；因此这里把每个 primitive 扁平化为一个 `MeshData`，
    /// 再让引用它的 node 生成对应的 prefab instance。
    fn copy_scene(&self) -> Result<RawSceneData, String> {
        let mut materials = Vec::new();
        let mut material_index_by_gltf_index = Vec::new();
        for material in self.document.materials() {
            material_index_by_gltf_index.push(materials.len() as u32);
            materials.push(self.copy_material(material)?);
        }
        let default_material_index = materials.len() as u32;
        materials.push(Self::default_material());

        let mut meshes = Vec::new();
        let mut primitive_refs = Vec::new();
        for mesh in self.document.meshes() {
            let mesh_name = mesh.name().unwrap_or(&self.model_name);
            let mut refs = Vec::new();
            for primitive in mesh.primitives() {
                let mesh_index = meshes.len() as u32;
                let material_index =
                    Self::primitive_material_index(&material_index_by_gltf_index, &primitive, default_material_index);
                let primitive_name = format!("{}-mesh{}-prim{}", mesh_name, mesh.index(), primitive.index());
                meshes.push(self.copy_primitive_mesh(&primitive, &materials[material_index as usize], primitive_name)?);
                refs.push(GltfPrimitiveRef {
                    mesh_index,
                    material_index,
                });
            }
            primitive_refs.push(refs);
        }

        let mut instances = Vec::new();
        let root_scene = self
            .document
            .default_scene()
            .or_else(|| self.document.scenes().next())
            .ok_or_else(|| format!("glTF scene {:?} contains no scenes", self.source_path))?;
        for node in root_scene.nodes() {
            self.copy_node_instances(&node, glam::Mat4::IDENTITY, &primitive_refs, &mut instances)?;
        }

        Ok(RawSceneData {
            source_path: self.source_path.clone(),
            name: self.model_name.clone(),
            meshes,
            materials,
            instances,
        })
    }

    /// 将 glTF material 复制到 AssetLoadService 的 raw material 边界格式。
    ///
    /// 四个核心纹理槽分别保留图片来源、UV 集、变换和 sampler。
    /// 外部 URI 保留为 importer 原始表达，稍后由 `AssetSystem` 根据 scene 路径统一解析；
    /// embedded source 只携带 owned encoded bytes。
    fn copy_material(&self, material: gltf::Material<'_>) -> Result<RawMaterialData, String> {
        let pbr = material.pbr_metallic_roughness();
        let name = material.name().map(str::to_string).unwrap_or_else(|| Self::material_fallback_name(material.index()));
        let transmission = material.transmission().map(|value| value.transmission_factor()).unwrap_or(0.0);
        let ior = material.ior().unwrap_or(MaterialClass::DEFAULT_IOR);
        let mut textures: [Option<TextureSlot<RawTextureSource>>; TextureChannel::COUNT] = Default::default();
        for (channel, info) in [
            (TextureChannel::BaseColor, pbr.base_color_texture()),
            (TextureChannel::MetallicRoughness, pbr.metallic_roughness_texture()),
            (TextureChannel::Emissive, material.emissive_texture()),
        ] {
            if let Some(info) = info {
                let transform = info.texture_transform().map(|value| (TextureTransform {
                    offset: value.offset().into(), rotation: value.rotation(), scale: value.scale().into(),
                }, value.tex_coord()));
                textures[channel as usize] = Some(self.copy_texture_slot(info.texture(), info.tex_coord(), transform)?);
            }
        }
        if let Some(info) = material.normal_texture() {
            textures[TextureChannel::Normal as usize] = Some(self.copy_texture_slot(
                info.texture(), info.tex_coord(), Self::raw_texture_transform(info.extension_value("KHR_texture_transform"))?,
            )?);
        }
        Ok(RawMaterialData {
            base_color: pbr.base_color_factor().into(),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            // glTF emission 是可反射表面的独立属性，不把其变成只发光的终止材质。
            class: Self::material_class(transmission, ior),
            coverage: Self::coverage_mode(&name, material.alpha_mode(), material.alpha_cutoff()),
            textures,
            normal_scale: material.normal_texture().map_or(1.0, |value| value.scale()),
            emissive_factor: (glam::Vec3::from_array(material.emissive_factor())
                * material.emissive_strength().unwrap_or(1.0)),
            name,
        })
    }

    /// normal 在当前 gltf crate 中通过通用扩展数据暴露，复用库的字段类型与默认值。
    fn raw_texture_transform(value: Option<&serde_json::Value>) -> Result<Option<(TextureTransform, Option<u32>)>, String> {
        value.map(|value| {
            let transform: gltf::json::extensions::texture::TextureTransform = serde_json::from_value(value.clone())
                .map_err(|error| format!("invalid KHR_texture_transform: {error}"))?;
            Ok((TextureTransform { offset: transform.offset.0.into(), rotation: transform.rotation.0,
                scale: transform.scale.0.into() }, transform.tex_coord))
        }).transpose()
    }

    fn copy_texture_slot(&self, texture: gltf::Texture<'_>, tex_coord: u32,
        transform: Option<(TextureTransform, Option<u32>)>) -> Result<TextureSlot<RawTextureSource>, String> {
        let source = self
            .texture_source(texture.source())?
            .ok_or_else(|| format!("texture {} has no image source", texture.index()))?;
        let sampler = texture.sampler();
        let (transform, override_coord) = transform.unwrap_or_default();
        if !transform.is_finite() {
            return Err(format!("texture {} has non-finite transform", texture.index()));
        }
        Ok(TextureSlot {
            texture: source, tex_coord: override_coord.unwrap_or(tex_coord), transform,
            sampler: TextureSampler {
                wrap_s: Self::texture_wrap(sampler.wrap_s()), wrap_t: Self::texture_wrap(sampler.wrap_t()),
                filter: match sampler.mag_filter() {
                    Some(gltf::texture::MagFilter::Nearest) => TextureFilter::Nearest,
                    _ => TextureFilter::Linear,
                },
            },
        })
    }

    fn texture_wrap(wrap: gltf::texture::WrappingMode) -> TextureWrap {
        match wrap {
            gltf::texture::WrappingMode::Repeat => TextureWrap::Repeat,
            gltf::texture::WrappingMode::ClampToEdge => TextureWrap::Clamp,
            gltf::texture::WrappingMode::MirroredRepeat => TextureWrap::MirroredRepeat,
        }
    }

    /// 复制一个 primitive 的 CPU mesh 数据。
    ///
    /// 当前 render-side mesh manager 要求 position / normal / tangent / uv 数组长度一致且
    /// index count 为 3 的倍数。glTF 允许部分属性缺失，因此这里按 v1 策略补齐默认值，
    /// 让缺属性模型仍能进入既有上传和 BLAS 构建路径。
    fn copy_primitive_mesh(&self, primitive: &gltf::Primitive<'_>, material: &RawMaterialData, name: String) -> Result<MeshData, String> {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            return Err(format!("glTF primitive '{}' is not triangle list", name));
        }

        let reader = primitive.reader(|buffer| self.buffers.get(buffer.index()).map(|data| data.0.as_slice()));
        let positions = reader
            .read_positions()
            .ok_or_else(|| format!("glTF primitive '{}' is missing POSITION attribute", name))?
            .map(|pos| glam::Vec3::new(pos[0], pos[1], pos[2]))
            .collect::<Vec<_>>();

        let vertex_count = positions.len();
        if vertex_count == 0 {
            return Err(format!("glTF primitive '{}' has no vertices", name));
        }

        let indices = reader
            .read_indices()
            .map(|iter| iter.into_u32().collect())
            .unwrap_or_else(|| (0..vertex_count as u32).collect::<Vec<_>>());
        let normals = reader
            .read_normals()
            .map(|iter| iter.map(|normal| glam::Vec3::new(normal[0], normal[1], normal[2])).collect())
            .unwrap_or_else(|| Self::generate_normals(&positions, &indices));
        let tangents = reader
            .read_tangents()
            .map(|iter| iter.map(glam::Vec4::from_array).collect())
            .unwrap_or_else(|| vec![glam::Vec4::ZERO; vertex_count]);
        let uv_count = primitive.attributes().filter_map(|(semantic, _)| match semantic {
            gltf::Semantic::TexCoords(set) => Some(set), _ => None,
        }).max().map(|set| set.checked_add(1)
            .ok_or_else(|| format!("glTF primitive '{name}' UV set index overflow"))).transpose()?.unwrap_or(0);
        // 不按不可信的最大集合编号预分配；稀疏或超大编号会在首个缺失集合处返回错误。
        let mut tex_coords = Vec::new();
        for set in 0..uv_count {
            let coords = reader.read_tex_coords(set)
                .ok_or_else(|| format!("glTF primitive '{name}' is missing TEXCOORD_{set}"))?
                .into_f32().map(glam::Vec2::from_array).collect::<Vec<_>>();
            tex_coords.push(coords);
        }
        for (channel, slot) in TextureChannel::ALL.into_iter().zip(material.textures.iter()) {
            if let Some(slot) = slot {
                if slot.tex_coord as usize >= tex_coords.len() {
                    return Err(format!("glTF primitive '{name}' {} requires missing TEXCOORD_{}", channel.name(), slot.tex_coord));
                }
            }
        }
        let tangent_tex_coord = material.textures[TextureChannel::Normal as usize].as_ref().map_or(0, |slot| slot.tex_coord);
        let submesh = SubmeshData {
            positions,
            normals,
            tangents,
            tex_coords,
            tangent_tex_coord,
            indices,
            name,
        };
        submesh.validate()?;
        Ok(MeshData::from_single_submesh(submesh))
    }

    /// 递归复制 node tree 中的 prefab instance。
    ///
    /// transform 累积顺序保持为 parent * local，输出的是 world transform。node 本身引用
    /// mesh 时，每个 primitive-ref 都拆成一条 instance，和现有 Assimp loader 对多 mesh
    /// node 的处理方式一致。
    fn copy_node_instances(
        &self,
        node: &gltf::Node<'_>,
        parent_transform: glam::Mat4,
        primitive_refs: &[Vec<GltfPrimitiveRef>],
        instances: &mut Vec<RawSceneInstanceData>,
    ) -> Result<(), String> {
        let transform = parent_transform * Self::mat4_from_gltf_transform(node.transform());

        if let Some(mesh) = node.mesh() {
            let refs = primitive_refs
                .get(mesh.index())
                .ok_or_else(|| format!("glTF node references missing mesh {}", mesh.index()))?;
            let node_name = node.name().unwrap_or("node");
            for (primitive_index, primitive_ref) in refs.iter().enumerate() {
                instances.push(RawSceneInstanceData {
                    mesh_index: primitive_ref.mesh_index,
                    material_indices: vec![primitive_ref.material_index],
                    transform,
                    name: format!("{}-{}", node_name, primitive_index),
                });
            }
        }

        for child in node.children() {
            self.copy_node_instances(&child, transform, primitive_refs, instances)?;
        }

        Ok(())
    }

    fn primitive_material_index(
        material_index_by_gltf_index: &[u32],
        primitive: &gltf::Primitive<'_>,
        default_material_index: u32,
    ) -> u32 {
        primitive
            .material()
            .index()
            .and_then(|index| material_index_by_gltf_index.get(index).copied())
            .unwrap_or(default_material_index)
    }

    fn default_material() -> RawMaterialData {
        RawMaterialData {
            base_color: glam::Vec4::ONE,
            metallic: 1.0,
            roughness: 1.0,
            class: MaterialClass::Surface,
            coverage: CoverageMode::Opaque,
            textures: Default::default(),
            normal_scale: 1.0,
            emissive_factor: glam::Vec3::ZERO,
            name: "material-default".to_string(),
        }
    }

    fn texture_source(&self, image: gltf::image::Image<'_>) -> Result<Option<RawTextureSource>, String> {
        match image.source() {
            gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
                let path = Self::percent_decode(uri)
                    .map_err(|error| format!("invalid external image URI '{uri}': {error}"))
                    .and_then(|bytes| {
                        String::from_utf8(bytes).map_err(|error| {
                            format!("invalid external image URI '{uri}': decoded path is not UTF-8: {error}")
                        })
                    })?;
                Ok(Some(RawTextureSource::ExternalPath(PathBuf::from(path))))
            }
            gltf::image::Source::Uri { mime_type, .. } => Ok(self
                .embedded_images
                .get(image.index())
                .cloned()
                .flatten()
                .map(|(bytes, embedded_mime_type)| RawTextureSource::Embedded {
                    identity: EmbeddedTextureId {
                        image_index: image.index() as u32,
                    },
                    bytes,
                    mime_type: mime_type.map(str::to_owned).or(embedded_mime_type),
                })),
            gltf::image::Source::View { mime_type, .. } => Ok(self
                .embedded_images
                .get(image.index())
                .cloned()
                .flatten()
                .map(|(bytes, embedded_mime_type)| RawTextureSource::Embedded {
                    identity: EmbeddedTextureId {
                        image_index: image.index() as u32,
                    },
                    bytes,
                    mime_type: Some(mime_type.to_owned()).or(embedded_mime_type),
                })),
        }
    }

    fn embedded_image_bytes(
        image: gltf::image::Image<'_>,
        buffers: &[buffer::Data],
    ) -> Result<Option<(Arc<[u8]>, Option<String>)>, String> {
        match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let data = buffers
                    .get(view.buffer().index())
                    .ok_or_else(|| format!("embedded image references missing buffer {}", view.buffer().index()))?;
                let start = view.offset();
                let end = start.saturating_add(view.length());
                let bytes = data.0.get(start..end).ok_or_else(|| "embedded image bufferView is out of range".to_string())?;
                Ok(Some((Arc::from(bytes.to_vec()), Some(mime_type.to_owned()))))
            }
            gltf::image::Source::Uri { uri, mime_type } if uri.starts_with("data:") => {
                let (metadata, encoded) = uri.split_once(',').ok_or_else(|| "invalid embedded data URI".to_string())?;
                let bytes = if metadata.split(';').any(|part| part.eq_ignore_ascii_case("base64")) {
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|err| format!("invalid embedded image data URI: {err}"))?
                } else {
                    Self::percent_decode(encoded)
                        .map_err(|err| format!("invalid embedded image data URI: {err}"))?
                };
                let uri_mime_type = metadata
                    .strip_prefix("data:")
                    .and_then(|metadata| metadata.split(';').next())
                    .filter(|mime_type| !mime_type.is_empty())
                    .map(str::to_owned);
                Ok(Some((Arc::from(bytes), mime_type.map(str::to_owned).or(uri_mime_type))))
            }
            gltf::image::Source::Uri { .. } => Ok(None),
        }
    }

    fn percent_decode(value: &str) -> Result<Vec<u8>, String> {
        let bytes = value.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            }
            if index + 2 >= bytes.len() {
                return Err("incomplete percent escape".to_string());
            }
            let high = bytes[index + 1] as char;
            let low = bytes[index + 2] as char;
            let value = high
                .to_digit(16)
                .and_then(|high| low.to_digit(16).map(|low| (high << 4) | low))
                .ok_or_else(|| "invalid percent escape".to_string())?;
            decoded.push(value as u8);
            index += 3;
        }
        Ok(decoded)
    }

    fn material_class(transmission_factor: f32, ior: f32) -> MaterialClass {
        if transmission_factor > 0.0 {
            return MaterialClass::transmission(1.0 - transmission_factor, ior);
        }

        MaterialClass::Surface
    }

    fn coverage_mode(name: &str, alpha_mode: gltf::material::AlphaMode, alpha_cutoff: Option<f32>) -> CoverageMode {
        match alpha_mode {
            gltf::material::AlphaMode::Mask => {
                CoverageMode::alpha_mask(alpha_cutoff.unwrap_or(CoverageMode::DEFAULT_ALPHA_CUTOFF))
            }
            gltf::material::AlphaMode::Blend => {
                log::warn!(
                    "glTF material '{}' uses alpha BLEND; v1 has no alpha blend path, importing coverage as Opaque",
                    name
                );
                CoverageMode::Opaque
            }
            gltf::material::AlphaMode::Opaque => CoverageMode::Opaque,
        }
    }

    fn material_fallback_name(index: Option<usize>) -> String {
        index.map(|index| format!("material-{}", index)).unwrap_or_else(|| "material-default".to_string())
    }

    fn mat4_from_gltf_transform(transform: gltf::scene::Transform) -> glam::Mat4 {
        let matrix = transform.matrix();
        glam::Mat4::from_cols_array(&[
            matrix[0][0],
            matrix[0][1],
            matrix[0][2],
            matrix[0][3],
            matrix[1][0],
            matrix[1][1],
            matrix[1][2],
            matrix[1][3],
            matrix[2][0],
            matrix[2][1],
            matrix[2][2],
            matrix[2][3],
            matrix[3][0],
            matrix[3][1],
            matrix[3][2],
            matrix[3][3],
        ])
    }

    fn generate_normals(positions: &[glam::Vec3], indices: &[u32]) -> Vec<glam::Vec3> {
        let mut normals = vec![glam::Vec3::ZERO; positions.len()];

        for tri in indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;
            let (Some(&p0), Some(&p1), Some(&p2)) = (positions.get(i0), positions.get(i1), positions.get(i2)) else {
                continue;
            };
            let face_normal = (p1 - p0).cross(p2 - p0);
            if face_normal.length_squared() <= f32::EPSILON {
                continue;
            }
            normals[i0] += face_normal;
            normals[i1] += face_normal;
            normals[i2] += face_normal;
        }

        normals
            .into_iter()
            .map(|normal| if normal.length_squared() > f32::EPSILON { normal.normalize() } else { glam::Vec3::Y })
            .collect()
    }


}

#[derive(Clone, Copy)]
struct GltfPrimitiveRef {
    mesh_index: u32,
    material_index: u32,
}

#[cfg(test)]
mod tests {
    use super::GltfSceneReader;

    #[test]
    fn percent_decodes_non_base64_data_uri_payload() {
        assert_eq!(GltfSceneReader::percent_decode("PNG%00%FF"), Ok(vec![b'P', b'N', b'G', 0, 255]));
    }
}
