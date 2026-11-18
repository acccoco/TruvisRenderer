//! glTF scene 导入任务。
//!
//! 本模块运行在 asset 后台线程中，职责只到“从 glTF 文件复制出 owned CPU 数据”。
//! 它不分配 asset handle、不创建 GPU resource，也不把 glTF crate 的借用对象传出任务。
//! 返回给 `AssetLoadService` 的数据必须保持为 `RawSceneData` 这套现有边界格式，后续 texture
//! 路径解析、scene handle 分配和 render upload event 生成统一收敛在 `AssetSystem`。

use std::borrow::Cow;
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
/// 整个任务的 panic 由 `AssetLoadWorker` 转为失败结果，包含入口和失败诊断。
/// `req.handle` 只关联一次性加载请求，不是长期 asset 身份，也不参与文件读取。
pub(crate) fn load_gltf_scene_task(req: SceneLoadRequest) -> LoadResult {
    let _span = tracy_client::span!("load_gltf_scene_task");
    log::info!("Loading glTF scene: {:?}", req.desc.path);

    match GltfSceneReader::load_path(&req.desc.path) {
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
        Self::validate_version(&document)?;
        Self::validate_extensions(&document)?;
        Self::validate_node_hierarchy(&document)?;
        let buffers = Self::load_buffers(&document, path, gltf.blob)?;
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

        // 降级不豁免输入完整性：忽略的动画、skin 和 morph accessor 也必须有合法字节范围与有限数值。
        for accessor in reader.document.accessors() {
            let strided = accessor.view().is_some_and(|view| view.stride().is_some());
            reader.validate_geometry_accessor(&accessor, strided)?;
        }
        for animation in reader.document.animations() {
            log::warn!("glTF animation {} ignored; importing existing node transforms", animation.index());
        }
        reader.copy_scene()
    }

    /// GLB header 只约束容器；JSON 版本必须在读取外部资源前满足本 Reader 的能力。
    fn validate_version(document: &gltf::Document) -> Result<(), String> {
        let asset = &document.as_json().asset;
        let version = Self::parse_version(&asset.version)?;
        if version.0 != 2 {
            return Err(format!("unsupported glTF asset version '{}': expected major 2", asset.version));
        }
        if let Some(minimum) = &asset.min_version {
            let minimum_version = Self::parse_version(minimum)?;
            if minimum_version > version {
                return Err(format!("glTF minVersion '{minimum}' exceeds asset version '{}'", asset.version));
            }
            if minimum_version != (2, 0) {
                return Err(format!("unsupported glTF minVersion '{minimum}': reader supports 2.0"));
            }
        }
        // 同 major 的 minor 更新允许增加可忽略字段；没有更高 minVersion 时不直接拒绝。
        Ok(())
    }

    fn parse_version(value: &str) -> Result<(u64, u64), String> {
        let (major, minor) = value
            .split_once('.')
            .filter(|(major, minor)| {
                !major.is_empty()
                    && !minor.is_empty()
                    && major.bytes().all(|ch| ch.is_ascii_digit())
                    && minor.bytes().all(|ch| ch.is_ascii_digit())
            })
            .ok_or_else(|| format!("invalid glTF version '{value}': expected major.minor"))?;
        let major = major.parse().map_err(|_| format!("glTF major version is out of range: '{value}'"))?;
        let minor = minor.parse().map_err(|_| format!("glTF minor version is out of range: '{value}'"))?;
        Ok((major, minor))
    }

    /// 库能解析扩展不代表 Reader 已消费；必需声明必须有本地导入入口。
    /// 这里只检查声明，具体材质字段与渲染语义仍由相应转换和消费者约束。
    fn validate_extensions(document: &gltf::Document) -> Result<(), String> {
        let mut used = std::collections::HashSet::new();
        for extension in document.extensions_used() {
            if !used.insert(extension) {
                return Err(format!("duplicate glTF extensionsUsed entry '{extension}'"));
            }
        }
        let mut required = std::collections::HashSet::new();
        for extension in document.extensions_required() {
            if !required.insert(extension) {
                return Err(format!("duplicate glTF extensionsRequired entry '{extension}'"));
            }
            if !used.contains(extension) {
                return Err(format!("required glTF extension '{extension}' is missing from extensionsUsed"));
            }
            if !matches!(
                extension,
                "KHR_texture_transform"
                    | "KHR_materials_transmission"
                    | "KHR_materials_ior"
                    | "KHR_materials_emissive_strength"
            ) {
                return Err(format!("required glTF extension '{extension}' has no Reader import path"));
            }
        }
        Ok(())
    }

    /// glTF 的 node 是互不相交的树；JSON 引用有效不等于无环、单父或合法 scene root。
    /// 在资源 IO 前遍历整个声明森林，连未被当前 scene 引用的循环也必须拒绝。
    fn validate_node_hierarchy(document: &gltf::Document) -> Result<(), String> {
        let nodes = &document.as_json().nodes;
        let mut has_parent = vec![false; nodes.len()];
        for (parent, node) in nodes.iter().enumerate() {
            for child in node.children.as_deref().unwrap_or_default() {
                if std::mem::replace(&mut has_parent[child.value()], true) {
                    return Err(format!("glTF node {} has repeated or multiple parents (including {parent})", child.value()));
                }
            }
        }
        let mut pending = has_parent.iter().enumerate().filter_map(|(index, &parent)| (!parent).then_some(index)).collect::<Vec<_>>();
        let mut visited = 0;
        while let Some(index) = pending.pop() {
            visited += 1;
            pending.extend(nodes[index].children.as_deref().unwrap_or_default().iter().map(|child| child.value()));
        }
        if visited != nodes.len() {
            return Err("glTF node hierarchy contains a cycle".to_string());
        }
        for scene in document.scenes() {
            let mut roots = std::collections::HashSet::new();
            for node in scene.nodes() {
                if has_parent[node.index()] || !roots.insert(node.index()) {
                    return Err(format!("glTF scene {} has non-root or repeated node {}", scene.index(), node.index()));
                }
            }
        }
        Ok(())
    }

    /// 校验原始长度后只保留声明范围，不能把补齐字节当作输入数据。
    /// gltf::import_buffers 会先补齐到 4 字节再比较长度，不适合这里的完整性检查。
    fn load_buffers(
        document: &gltf::Document,
        scene_path: &Path,
        mut blob: Option<Vec<u8>>,
    ) -> Result<Vec<buffer::Data>, String> {
        for view in document.views() {
            let end = view.offset().checked_add(view.length());
            if view.length() == 0 || end.is_none_or(|end| end > view.buffer().length()) {
                return Err(format!(
                    "bufferView {} exceeds declared buffer {} range",
                    view.index(),
                    view.buffer().index()
                ));
            }
        }

        document
            .buffers()
            .map(|buffer| {
                let mut bytes = match buffer.source() {
                    buffer::Source::Bin => blob.take().ok_or_else(|| "missing GLB BIN chunk".to_string())?,
                    buffer::Source::Uri(uri) if uri.starts_with("data:") => Self::data_uri_bytes(uri)?.0,
                    buffer::Source::Uri(uri) => {
                        let path =
                            scene_path.parent().unwrap_or_else(|| Path::new("")).join(Self::external_uri_path(uri)?);
                        std::fs::read(&path).map_err(|error| {
                            format!("failed to read buffer {} at '{}': {error}", buffer.index(), path.display())
                        })?
                    }
                };
                if buffer.length() == 0 || bytes.len() < buffer.length() {
                    return Err(format!(
                        "buffer {} has {} bytes, expected at least {} nonzero bytes",
                        buffer.index(),
                        bytes.len(),
                        buffer.length()
                    ));
                }
                // 文件额外数据与 GLB 对齐 padding 合法，但不属于该 buffer 的访问范围。
                bytes.truncate(buffer.length());
                Ok(buffer::Data(bytes))
            })
            .collect()
    }

    /// 将本地资源 URI 解码为路径；不把远程 scheme、query 或 fragment 当作文件名。
    fn external_uri_path(uri: &str) -> Result<PathBuf, String> {
        if uri.contains(['?', '#']) {
            return Err(format!("unsupported query or fragment in external URI '{uri}'"));
        }
        let file_path = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("file:"));
        let encoded = file_path.unwrap_or(uri);
        let decoded = String::from_utf8(Self::percent_decode(encoded)?)
            .map_err(|error| format!("external URI is not UTF-8: {error}"))?;
        if decoded.is_empty() || decoded.contains('\0') {
            return Err("external URI must name a nonempty path without NUL".to_string());
        }
        let path = PathBuf::from(decoded);
        if file_path.is_some() && !path.is_absolute() {
            return Err("file URI must contain an absolute local path".to_string());
        }
        if file_path.is_none() && uri.contains(':') && !path.is_absolute() {
            return Err(format!("unsupported external URI scheme: '{uri}'"));
        }
        Ok(path)
    }

    /// data URI 先按 URI 语法解码，再处理末尾的 base64 标志；两步各执行一次。
    fn data_uri_bytes(uri: &str) -> Result<(Vec<u8>, Option<String>), String> {
        let (metadata, encoded) = uri
            .strip_prefix("data:")
            .and_then(|uri| uri.split_once(','))
            .ok_or_else(|| "invalid data URI".to_string())?;
        let mut parts = metadata.split(';');
        let mime_type = parts.next().filter(|mime| !mime.is_empty()).map(str::to_owned);
        let mut is_base64 = false;
        for part in parts {
            if is_base64 {
                return Err("base64 must be the final data URI flag".to_string());
            }
            is_base64 = part.eq_ignore_ascii_case("base64");
        }
        // 常见 base64 文本没有转义，直接借用，避免为大型 buffer 再复制一份编码文本。
        let bytes = if encoded.contains('%') {
            Cow::Owned(Self::percent_decode(encoded)?)
        } else {
            Cow::Borrowed(encoded.as_bytes())
        };
        let bytes = if is_base64 {
            base64::engine::general_purpose::STANDARD
                .decode(bytes.as_ref())
                .map_err(|error| format!("invalid base64 data URI: {error}"))?
        } else {
            bytes.into_owned()
        };
        Ok((bytes, mime_type))
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

    /// 库的数值 wrapper 没有范围校验；先验证原始参数，避免构造器的 clamp/max 掩盖错误。
    /// 这里只落实 glTF 输入域，不改变 World 材质编辑范围或现有 shader 模型。
    fn validate_material_inputs(material: &gltf::Material<'_>) -> Result<(), String> {
        let pbr = material.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        let emissive = material.emissive_factor();
        let transmission = material.transmission();
        let transmission_factor = transmission.as_ref().map_or(0.0, |value| value.transmission_factor());
        for (field, values) in [("baseColorFactor", base_color.as_slice()), ("emissiveFactor", emissive.as_slice())] {
            if values.iter().any(|value| !(0.0..=1.0).contains(value)) {
                return Err(format!("{field} components must be finite and within [0, 1]"));
            }
        }
        for (field, value) in [
            ("metallicFactor", pbr.metallic_factor()),
            ("roughnessFactor", pbr.roughness_factor()),
            ("transmissionFactor", transmission_factor),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("{field} must be finite and within [0, 1]"));
            }
        }
        let strength = material.emissive_strength().unwrap_or(1.0);
        if !strength.is_finite() || strength < 0.0 {
            return Err("emissiveStrength must be finite and nonnegative".to_string());
        }
        if material.normal_texture().is_some_and(|normal| !normal.scale().is_finite()) {
            return Err("normalTexture.scale must be finite".to_string());
        }
        let ior = material.ior().unwrap_or(MaterialClass::DEFAULT_IOR);
        if transmission_factor > 0.0 && ior == 0.0 {
            // 扩展将 0 定义为特殊 Fresnel 模式；当前材质类型不能表达，不能用 max(1) 偷换含义。
            return Err("IOR zero compatibility mode is not supported".to_string());
        }
        if !ior.is_finite() || (ior != 0.0 && ior < 1.0) {
            return Err("IOR must be finite and at least 1".to_string());
        }
        // 当前 raw 类型只保留标量透射；明确诊断被忽略的可选语义。
        if transmission.is_some_and(|value| value.transmission_texture().is_some()) {
            log::warn!("glTF material {:?}: transmissionTexture ignored; retaining factor", material.index());
        }
        if transmission_factor == 0.0 && ior != MaterialClass::DEFAULT_IOR {
            log::warn!("glTF material {:?}: non-transmission IOR ignored", material.index());
        }
        // 非 MASK 模式按规范忽略 cutoff；MASK 的合法上界不限于 1。
        if material.alpha_mode() == gltf::material::AlphaMode::Mask {
            let cutoff = material.alpha_cutoff().unwrap_or(CoverageMode::DEFAULT_ALPHA_CUTOFF);
            if !cutoff.is_finite() || cutoff < 0.0 {
                return Err("alphaCutoff must be finite and nonnegative for MASK".to_string());
            }
        }
        Ok(())
    }

    /// 将 glTF material 复制到 AssetLoadService 的 raw material 边界格式。
    ///
    /// 四个核心纹理槽分别保留图片来源、UV 集、变换和 sampler。
    /// 外部 URI 解码为本地路径，稍后由 `AssetSystem` 根据 scene 路径定位并 canonicalize；
    /// embedded source 只携带 owned encoded bytes。
    fn copy_material(&self, material: gltf::Material<'_>) -> Result<RawMaterialData, String> {
        let pbr = material.pbr_metallic_roughness();
        let name = material.name().map(str::to_string).unwrap_or_else(|| Self::material_fallback_name(material.index()));
        Self::validate_material_inputs(&material).map_err(|error| format!("glTF material '{name}': {error}"))?;
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
            coverage: Self::coverage_mode(&name, material.alpha_mode(), material.alpha_cutoff())?,
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

    /// 只校验本 Reader 消费的几何布局；库的 JSON validation 不保证 typed iterator 可安全读取。
    /// 在分配顶点数组前拒绝错误类型、数量和字节范围，不能把损坏的属性当成缺失属性补齐。
    fn validate_primitive_accessors(&self, primitive: &gltf::Primitive<'_>) -> Result<(), String> {
        use gltf::accessor::{DataType, Dimensions};

        let position =
            primitive.get(&gltf::Semantic::Positions).ok_or_else(|| "missing POSITION attribute".to_string())?;
        for (semantic, accessor) in primitive.attributes() {
            let valid = match semantic {
                gltf::Semantic::Positions | gltf::Semantic::Normals => {
                    accessor.dimensions() == Dimensions::Vec3
                        && accessor.data_type() == DataType::F32
                        && !accessor.normalized()
                }
                gltf::Semantic::Tangents => {
                    accessor.dimensions() == Dimensions::Vec4
                        && accessor.data_type() == DataType::F32
                        && !accessor.normalized()
                }
                gltf::Semantic::TexCoords(_) => {
                    accessor.dimensions() == Dimensions::Vec2
                        && match accessor.data_type() {
                            DataType::F32 => !accessor.normalized(),
                            DataType::U8 | DataType::U16 => accessor.normalized(),
                            _ => false,
                        }
                }
                _ => true,
            };
            if !valid || accessor.count() != position.count() {
                return Err(format!("accessor {} has invalid {semantic:?} type or vertex count", accessor.index()));
            }
            self.validate_geometry_accessor(&accessor, true)?;
        }
        for target in primitive.morph_targets() {
            for accessor in [target.positions(), target.normals(), target.tangents()].into_iter().flatten() {
                if accessor.count() != position.count() || accessor.dimensions() != Dimensions::Vec3
                    || accessor.data_type() != DataType::F32 || accessor.normalized() {
                    return Err(format!("accessor {} has invalid morph layout", accessor.index()));
                }
                self.validate_geometry_accessor(&accessor, true)?;
            }
        }
        if let Some(accessor) = primitive.indices() {
            if accessor.dimensions() != Dimensions::Scalar
                || accessor.normalized()
                || !matches!(accessor.data_type(), DataType::U8 | DataType::U16 | DataType::U32)
            {
                return Err(format!("accessor {} has invalid index type", accessor.index()));
            }
            self.validate_geometry_accessor(&accessor, false)?;
        }
        Ok(())
    }

    /// dense 和 sparse 共用已加载 buffer 的边界。稀疏覆盖仍交给库完成，不复制一套解码器。
    fn validate_geometry_accessor(&self, accessor: &gltf::Accessor<'_>, vertex: bool) -> Result<(), String> {
        let count = accessor.count();
        // 顶点寻址和库 SparseIter 的游标都是 u32；先限制声明，避免分配后才发现不可表示。
        if count == 0 || count > u32::MAX as usize {
            return Err(format!("accessor {} count is outside 1..=u32::MAX", accessor.index()));
        }
        let size = accessor.size();
        let component_size = accessor.data_type().size();
        if let Some(view) = accessor.view() {
            let stride = view.stride().unwrap_or(size);
            if let Some(declared) = view.stride() {
                if !vertex || !(4..=252).contains(&declared) || declared % 4 != 0 {
                    return Err(format!("accessor {} has invalid byteStride", accessor.index()));
                }
            }
            if vertex && (accessor.offset() % 4 != 0 || stride % 4 != 0) {
                return Err(format!("accessor {} vertex elements are not 4-byte aligned", accessor.index()));
            }
            Self::accessor_range(&view, accessor.offset(), count, stride, size, component_size)?;
            if accessor.data_type() == gltf::accessor::DataType::F32 {
                self.validate_finite_accessor(&view, accessor.offset(), count, stride, size)?;
            }
        } else if self.document.as_json().accessors[accessor.index()].byte_offset.is_some() {
            return Err(format!("accessor {} has byteOffset without bufferView", accessor.index()));
        }
        if let Some(sparse) = accessor.sparse() {
            if sparse.count() == 0 || sparse.count() > count {
                return Err(format!("accessor {} has invalid sparse count", accessor.index()));
            }
            let indices = sparse.indices();
            let values = sparse.values();
            let index_view = indices.view();
            let value_view = values.view();
            for view in [&index_view, &value_view] {
                if view.stride().is_some() || view.target().is_some() {
                    return Err(format!("accessor {} sparse views must not have stride or target", accessor.index()));
                }
            }
            let index_size = indices.index_type().size();
            let range = Self::accessor_range(
                &index_view,
                indices.offset(),
                sparse.count(),
                index_size,
                index_size,
                index_size,
            )?;
            Self::accessor_range(&value_view, values.offset(), sparse.count(), size, size, component_size)?;
            if accessor.data_type() == gltf::accessor::DataType::F32 {
                self.validate_finite_accessor(&value_view, values.offset(), sparse.count(), size, size)?;
            }
            // load_buffers 已证明整个 view 位于真实 buffer 内；range 则保证访问器位于 view 内。
            let data = &self.buffers[index_view.buffer().index()].0;
            let mut previous = None;
            for encoded in
                data[index_view.offset() + range.start..index_view.offset() + range.end].chunks_exact(index_size)
            {
                let mut bytes = [0; 4];
                bytes[..index_size].copy_from_slice(encoded);
                let index = u32::from_le_bytes(bytes);
                if index as usize >= count || previous.is_some_and(|previous| index <= previous) {
                    return Err(format!(
                        "accessor {} sparse indices must increase strictly and stay below count",
                        accessor.index()
                    ));
                }
                previous = Some(index);
            }
        }
        Ok(())
    }

    /// 调用方已证明范围；逐元素跳过 interleaved padding，只检查实际浮点分量。
    fn validate_finite_accessor(&self, view: &gltf::buffer::View<'_>, offset: usize,
        count: usize, stride: usize, size: usize) -> Result<(), String> {
        let bytes = &self.buffers[view.buffer().index()].0;
        for index in 0..count {
            let start = view.offset() + offset + index * stride;
            for component in bytes[start..start + size].chunks_exact(4) {
                if !f32::from_le_bytes(component.try_into().expect("4-byte component")).is_finite() {
                    return Err(format!("bufferView {} contains non-finite accessor data", view.index()));
                }
            }
        }
        Ok(())
    }

    /// 在调用库的 unchecked 范围算术前，证明每个元素完整位于 view 内。
    fn accessor_range(
        view: &gltf::buffer::View<'_>,
        offset: usize,
        count: usize,
        stride: usize,
        size: usize,
        component_size: usize,
    ) -> Result<std::ops::Range<usize>, String> {
        let end = count
            .checked_sub(1)
            .and_then(|last| last.checked_mul(stride))
            .and_then(|span| span.checked_add(size))
            .and_then(|span| offset.checked_add(span));
        if stride < size
            || stride % component_size != 0
            || offset % component_size != 0
            || view.offset() % component_size != 0
            || end.is_none_or(|end| end > view.length())
        {
            return Err(format!("accessor range is invalid for bufferView {}", view.index()));
        }
        Ok(offset..end.expect("已检查范围终点"))
    }

    /// 复制一个 primitive 的 CPU mesh 数据。
    ///
    /// 当前 render-side mesh manager 要求 position / normal / tangent / uv 数组长度一致且
    /// index count 为 3 的倍数。缺少 NORMAL 时按 glTF 规则生成 flat normals，
    /// 并同步展开共享顶点的全部属性；零切线交给现有 shader 按 normal UV 重建。
    fn copy_primitive_mesh(&self, primitive: &gltf::Primitive<'_>, material: &RawMaterialData, name: String) -> Result<MeshData, String> {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            return Err(format!("glTF primitive '{}' is not triangle list", name));
        }
        if primitive.attributes().any(|(semantic, _)| matches!(semantic, gltf::Semantic::Colors(_))) {
            log::warn!("glTF primitive '{name}': vertex colors ignored");
        }
        self.validate_primitive_accessors(primitive).map_err(|error| format!("glTF primitive '{name}': {error}"))?;

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

        let unreadable = |attribute| format!("glTF primitive '{name}' cannot read declared {attribute}");
        let indices = if let Some(accessor) = primitive.indices() {
            let indices = reader.read_indices().ok_or_else(|| unreadable("indices"))?.into_u32().collect::<Vec<_>>();
            // glTF 禁止原 component type 的 restart 值；转为 u32 后不能丢失这个来源约束。
            let restart = match accessor.data_type() {
                gltf::accessor::DataType::U8 => u8::MAX as u32,
                gltf::accessor::DataType::U16 => u16::MAX as u32,
                _ => u32::MAX,
            };
            if indices.contains(&restart) {
                return Err(format!("glTF primitive '{name}' indices contain a primitive restart value"));
            }
            indices
        } else {
            (0..vertex_count as u32).collect::<Vec<_>>()
        };
        let has_normals = primitive.get(&gltf::Semantic::Normals).is_some();
        let normals = if has_normals {
            reader.read_normals().ok_or_else(|| unreadable("NORMAL"))?.map(glam::Vec3::from_array).collect()
        } else {
            // 临时法线只用于复用 SubmeshData 的输入校验；发布前必须替换为逐面法线。
            vec![glam::Vec3::Y; vertex_count]
        };
        let tangents = if has_normals && primitive.get(&gltf::Semantic::Tangents).is_some() {
            reader.read_tangents().ok_or_else(|| unreadable("TANGENT"))?.map(glam::Vec4::from_array).collect()
        } else {
            vec![glam::Vec4::ZERO; vertex_count]
        };
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
        let mut submesh = SubmeshData {
            positions,
            normals,
            tangents,
            tex_coords,
            tangent_tex_coord,
            indices,
            name,
        };
        submesh.normalize_imported_frame()?;
        if !has_normals {
            Self::generate_flat_normals(&mut submesh)?;
            submesh.validate()?;
        }
        Ok(MeshData::from_single_submesh(submesh))
    }

    /// 使用显式栈复制已校验的 node tree，避免合法深层级耗尽 worker 调用栈。
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
        let mut pending = vec![(node.clone(), parent_transform)];
        while let Some((node, parent_transform)) = pending.pop() {
            let transform = parent_transform * self.node_local_transform(&node)?;
            if !transform.is_finite() {
                return Err(format!("glTF node {} world transform is not finite", node.index()));
            }
            if node.skin().is_some() {
                log::warn!("glTF node {}: skin deformation ignored; using unskinned base mesh + existing node transform (not bind pose)", node.index());
            }
            if let Some(mesh) = node.mesh() {
                Self::validate_static_mesh(&node, &mesh)?;
                // 只发布可由 GPU mirror 的 f32 inverse 处理的实例；允许空 group 奇异及累计 world shear。
                let determinant = transform.determinant();
                if !determinant.is_finite() || determinant == 0.0 || !transform.inverse().is_finite() {
                    return Err(format!("glTF node {} world transform has no finite f32 inverse", node.index()));
                }
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
            // 仅反转刚压入的 children，使 LIFO 保持原来的前序与兄弟顺序。
            let first_child = pending.len();
            pending.extend(node.children().map(|child| (child, transform)));
            pending[first_child..].reverse();
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
                let path = Self::external_uri_path(uri)
                    .map_err(|error| format!("invalid external image URI '{uri}': {error}"))?;
                Ok(Some(RawTextureSource::ExternalPath(path)))
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
                Ok(Some((Arc::from(bytes), Some(mime_type.to_owned()))))
            }
            gltf::image::Source::Uri { uri, mime_type } if uri.starts_with("data:") => {
                let (bytes, uri_mime_type) = Self::data_uri_bytes(uri)
                    .map_err(|error| format!("invalid embedded image data URI: {error}"))?;
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

    fn coverage_mode(name: &str, alpha_mode: gltf::material::AlphaMode, alpha_cutoff: Option<f32>) -> Result<CoverageMode, String> {
        match alpha_mode {
            gltf::material::AlphaMode::Mask => {
                // 上游已验证输入；glTF 允许 cutoff > 1，不能被通用构造器的 [0,1] clamp 改写。
                Ok(CoverageMode::AlphaMask {
                    alpha_cutoff: alpha_cutoff.unwrap_or(CoverageMode::DEFAULT_ALPHA_CUTOFF),
                })
            }
            gltf::material::AlphaMode::Blend => {
                log::warn!("glTF material '{name}': alpha BLEND downgraded to Opaque");
                Ok(CoverageMode::Opaque)
            }
            gltf::material::AlphaMode::Opaque => Ok(CoverageMode::Opaque),
        }
    }

    fn material_fallback_name(index: Option<usize>) -> String {
        index.map(|index| format!("material-{}", index)).unwrap_or_else(|| "material-default".to_string())
    }

    /// 局部 matrix 必须可表示为 TRS；该条件不适用于 parent * local 后的 world matrix。
    /// 这里只检查格式约束，不依赖上层 World 的编辑器分解，也不重组输入 matrix。
    fn node_local_transform(&self, node: &gltf::Node<'_>) -> Result<glam::Mat4, String> {
        let raw = &self.document.as_json().nodes[node.index()];
        if raw.matrix.is_some() && (raw.translation.is_some() || raw.rotation.is_some() || raw.scale.is_some()) {
            return Err(format!("glTF node {} mixes matrix and TRS", node.index()));
        }
        let matrix = match node.transform() {
            gltf::scene::Transform::Matrix { matrix } => glam::Mat4::from_cols_array_2d(&matrix),
            gltf::scene::Transform::Decomposed { translation, rotation, scale } => {
                let rotation = glam::Quat::from_array(rotation);
                if !rotation.is_normalized() {
                    return Err(format!("glTF node {} rotation must be a unit quaternion", node.index()));
                }
                glam::Mat4::from_scale_rotation_translation(scale.into(), rotation, translation.into())
            }
        };
        if !matrix.is_finite() || matrix.row(3) != glam::Vec4::W {
            return Err(format!("glTF node {} local transform must be finite and affine", node.index()));
        }
        if raw.matrix.is_some() {
            // 正交列可以分解为旋转与有符号缩放；零列合法，不必为奇异 group 猜测旋转轴。
            let axes = [matrix.x_axis, matrix.y_axis, matrix.z_axis]
                .map(|axis| axis.truncate().as_dvec3().normalize_or_zero());
            if axes[0].dot(axes[1]).abs() > 1e-5
                || axes[0].dot(axes[2]).abs() > 1e-5
                || axes[1].dot(axes[2]).abs() > 1e-5
            {
                return Err(format!("glTF node {} local matrix contains shear", node.index()));
            }
        }
        Ok(matrix)
    }

    /// 当前 RawSceneData 只表达静态基础网格；node weights 优先于 mesh defaults。
    /// 不求值 morph；权重长度和有限性仍是结构约束。
    fn validate_static_mesh(node: &gltf::Node<'_>, mesh: &gltf::Mesh<'_>) -> Result<(), String> {
        let target_count = mesh.primitives().next().map_or(0, |primitive| primitive.morph_targets().len());
        if mesh.primitives().any(|primitive| primitive.morph_targets().len() != target_count) {
            return Err(format!("glTF mesh {} primitives have different morph target counts", mesh.index()));
        }
        if target_count != 0 {
            log::warn!("glTF node {}: morph deformation ignored; using base mesh", node.index());
        }
        if let Some(weights) = node.weights().or_else(|| mesh.weights()) {
            if weights.len() != target_count || weights.iter().any(|weight| !weight.is_finite()) {
                return Err(format!("glTF node {} has invalid morph weights", node.index()));
            }

        }
        Ok(())
    }

    /// 输入须已通过 SubmeshData::validate；glTF 缺失 NORMAL 时不能按共享顶点平滑。
    /// 展开后每个角点只属于一个面，三角形顺序/绕序与 UV 对应关系保持，已有切线必须丢弃。
    fn generate_flat_normals(submesh: &mut SubmeshData) -> Result<(), String> {
        let count = submesh.indices.len();
        if count > u32::MAX as usize
            || submesh.tex_coords.len().checked_mul(count).is_none_or(|size| size > u32::MAX as usize)
        {
            return Err(format!("submesh '{}' flat expansion exceeds GPU uint vertex/UV addressing range", submesh.name));
        }
        let mut normals = Vec::with_capacity(count);
        for tri in submesh.indices.chunks_exact(3) {
            // f64 覆盖所有有限 f32 坐标的差与叉积，避免合法小面被固定面积阈值抹去或大面溢出。
            let p0 = submesh.positions[tri[0] as usize].as_dvec3();
            let p1 = submesh.positions[tri[1] as usize].as_dvec3();
            let p2 = submesh.positions[tri[2] as usize].as_dvec3();
            let cross = (p1 - p0).cross(p2 - p0);
            // normalize_imported_frame 已拒绝零面积面；这里仅改变 flat shading 的顶点布局。
            let normal = cross.normalize().as_vec3();
            normals.extend([normal; 3]);
        }
        submesh.positions = submesh.indices.iter().map(|&index| submesh.positions[index as usize]).collect();
        for coords in &mut submesh.tex_coords {
            *coords = submesh.indices.iter().map(|&index| coords[index as usize]).collect();
        }
        submesh.normals = normals;
        submesh.tangents = vec![glam::Vec4::ZERO; count];
        submesh.indices = (0..count as u32).collect();
        Ok(())
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
