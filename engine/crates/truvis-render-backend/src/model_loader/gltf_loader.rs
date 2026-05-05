//! 进行模型处理
//!
//! gltf 的格式，参考 https://www.khronos.org/files/gltf20-reference-guide.pdf

use std::{mem::size_of, rc::Rc};

use ash::vk;
use glam::f32::Mat4;
use itertools::{izip, Itertools};
use static_init::raw_static::Static;

use crate::{
    resource::model::StaticMeshData,
    gfx_type::{image::GfxImage2D, sampler::GfxSampler},
};


/// 导入 gltf 格式的模型
///
/// 支持 mesh，不支持 skin，动画
pub struct GltfLoader
{
    gltf_doc: gltf::Document,
    buffers: Vec<gltf::buffer::Data>,
    images: Vec<gltf::image::Data>,
}

impl GltfLoader
{
    /// 从 gltf 文件中载入模型
    pub fn load(path: &std::path::Path) -> Vec<HissNode>
    {
        let loader = Self::from_file(core, path);
        loader.process_scene()
    }

    /// 根据 gltf 文件创建 loader，使用 loader 来读取模型数据
    fn from_file(core: Rc<HissGfxCore>, path: &std::path::Path) -> Self
    {
        let (doc, buffers, images) =
            gltf::import(path).unwrap_or_else(|_| panic!("failed to open gltf file: {:?}", path));

        Self {
            core,
            gltf_doc: doc,
            buffers,
            images,
        }
    }

    /// 处理 gltf 的场景，场景是 gltf 中最大的层级
    ///
    /// ```json
    /// {
    ///     "scene": 0, // 通过该字段指定默认场景
    ///     "scenes": [
    ///         { "nodes": [ 0, 1, 2 ] },
    ///         { "nodes": ... }
    ///     ],
    /// }
    /// ```
    fn process_scene(&self) -> Vec<HissNode>
    {
        // 读取默认场景，否则读取 0 号场景
        let default_scene = self.gltf_doc.default_scene();
        let scene = default_scene.unwrap_or_else(|| self.gltf_doc.scenes().next().unwrap());

        let mut nodes = vec![];
        for node in scene.nodes() {
            let node = self.process_node(&node, &Mat4::IDENTITY);
            nodes.push(node);
        }

        nodes
    }

    /// 处理 gltf 中的一个 node
    ///
    /// node 可以包含 transform，mesh，camera，light 信息
    ///
    /// ```json
    /// {
    ///     "children": ...,
    ///
    ///     // transform 以 matrix 形式整体指定，或者分别指定
    ///     "matrix": ...,
    ///     "translation": ...,
    ///     "rotation": ...,
    ///     "scale": ...,
    ///
    ///     // node 中要么包含 mesh，要么包含 camera
    ///     "mesh": 4,
    ///     "camera": 5,
    /// }
    /// ```
    fn process_node(&self, node: &gltf::Node, parent_matrix: &Mat4) -> HissNode
    {
        // 处理 transform 信息
        // gltf 文件采用的是「右手系」
        // gltf 这个库使用 column major 的方式存放矩阵（每个元素相当于矩阵的一列）
        let local_matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
        let matrix = parent_matrix.mul_mat4(&local_matrix);

        // gltf 的一个 mesh 中可以有多个 primitive，所以可以解析出多个可渲染的 Mesh 对象
        let meshes: Vec<Box<dyn HissMesh>> = node.mesh().map_or(vec![], |gltf_mesh| self.process_mesh(&gltf_mesh));

        let children: Vec<HissNode> = node.children().map(|node| self.process_node(&node, &matrix)).collect();

        HissNode::new(meshes, children, matrix)
    }

    /// 处理 gltf 中的 mesh
    ///
    /// mesh 包括：indices，vertices attribute
    ///
    /// 一个 mesh 中可以有多个 `primitive`，每个 `primitive` 都是单独可渲染的
    /// 一个 mesh 的结构如下：
    ///
    /// ```json
    /// {
    ///    "primitives": [
    ///        {
    ///            "mode": 4,  // 点，线，三角面
    ///            "indices: n,
    ///            "attributes": {
    ///                "POSITION": n,
    ///                "NORMAL": n,
    ///            },
    ///            "material": n,
    ///        }
    ///    ]
    /// },
    /// ```
    fn process_mesh(&self, mesh: &gltf::Mesh) -> Vec<Box<dyn HissMesh>>
    {
        let mut meshes: Vec<Box<dyn HissMesh>> = vec![];

        for primitive in mesh.primitives() {
            assert_eq!(primitive.mode(), gltf::mesh::Mode::Triangles);

            let (vertex_buffer, vertex_cnt) = self.loader_primitive_data(&primitive);
            let (index_buffer, index_cnt) = self.create_index_buffer(&primitive);
            let material = Rc::new(self.create_material(&primitive));

            let mesh = HissMeshPNTBuilder::default()
                .vertex_buffers(vec![vertex_buffer])
                .vertex_cnt(vertex_cnt)
                .index_buffer(index_buffer)
                .indices_cnt(index_cnt)
                .material(material)
                .build()
                .unwrap();
            meshes.push(Box::new(mesh));
        }

        meshes
    }

    fn loader_primitive_data(&self, primitive: &gltf::Primitive) -> StaticMeshData
    {
        const DEFAULT_NORMAL: [f32; 3] = [0_f32; 3];
        const DEFAULT_TANGENT: [f32; 4] = [0_f32; 4];
        const DEFAULT_UV: [f32; 2] = [0_f32; 2];

        let mut mesh_data = StaticMeshData::default();

        let reader = primitive.reader(|buffer| Some(self.buffers[buffer.index()].as_ref()));

        mesh_data.positions = reader.read_positions().unwrap().collect_vec();
        let vertex_cnt = mesh_data.positions.len();

        mesh_data.normal = reader.read_normals().map_or_else(|| vec![DEFAULT_NORMAL; vertex_cnt], Iterator::collect);
        mesh_data.tangent = reader.read_tangents().map_or_else(|| vec![DEFAULT_TANGENT; vertex_cnt], Iterator::collect);
        mesh_data.uv = reader.read_tex_coords(0).map_or_else(|| vec![DEFAULT_UV; vertex_cnt], Iterator::collect);

        assert!(
            mesh_data.normal.len() == vertex_cnt &&
                mesh_data.tangent.len() == vertex_cnt &&
                mesh_data.uv.len() == vertex_cnt
        );

        mesh_data.index = reader.read_indices().expect("gltf file has no indices.").into_u32().collect();

        mesh_data
    }


    /// 创建材质对象
    ///
    /// gltf 中一个 material 的组成
    /// ```json
    /// {
    ///     "pbrMetallicRoughness": {
    ///         "baseColorTexture": {},
    ///         "baseColorFactor": [f32; 4],
    ///         "metallicRoughnessTexture": {},
    ///         "metallicFactor": f32,
    ///         "roughnessFactor": f32,
    ///     },
    ///     "normalTexture": {},
    ///     "occlusionTexture": {},
    ///     "emissiveTexture": {},
    ///     "emissiveFactor": [f32; 3]
    /// }
    /// ```
    fn create_material(&self, primitive: &gltf::Primitive) -> HissMatMR
    {
        let pbr = primitive.material().pbr_metallic_roughness();

        // 读取 base color texture：以 sRGB 编码的
        let base_color_tex = pbr.base_color_texture().map(|info| self.create_texture(info, true));

        // 读取 metallic roughness texture
        // metallic 位于 Blue 通道；Roughness 位于 Green 通道
        // 线性编码
        let mr_tex = pbr.metallic_roughness_texture().map(|info| self.create_texture(info, false));

        let base_color_factor = pbr.base_color_factor();
        let matallic_factor = pbr.metallic_factor();
        let roughness_factor = pbr.roughness_factor();

        HissMatMR::builder()
            .base_color_tex(base_color_tex)
            .base_color_factor(base_color_factor)
            .metallic_roughness_tex(mr_tex)
            .metallic_factor(matallic_factor)
            .roughness_factor(roughness_factor)
            .build()
            .unwrap()
    }

    /// 根据 gltf 的 texture info 创建 Texture 对象
    ///
    /// 注 只支持 TexCoord == 0 的 texture
    fn create_texture(&self, tex_info: gltf::texture::Info, s_rgb: bool) -> HissTexture
    {
        assert_eq!(tex_info.tex_coord(), 0);

        let sampler = self.create_sampler(&tex_info.texture().sampler());
        let image = self.create_image(&self.images[tex_info.texture().source().index()], s_rgb);
        HissTexture::new(self.core.clone(), Rc::new(image), sampler)
    }

    fn create_image(&self, image: &gltf::image::Data, s_rgb: bool) -> GfxImage2D
    {
        let image_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(Self::gltf_format_to_vk(image.format, s_rgb))
            .extent(
                vk::Extent2D {
                    width: image.width,
                    height: image.height,
                }
                .into(),
            )
            .usage(vk::ImageUsageFlags::SAMPLED);

        let alloc_info = vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::AutoPreferDevice,
            ..Default::default()
        };

        GfxImage2D::new(&image_info, &alloc_info)
    }

    fn create_sampler(&self, sampler: &gltf::texture::Sampler) -> GfxSampler
    {
        let (min_filter, mipmap_mode) = Self::gltf_min_filter_to_vk(sampler.min_filter());
        let mag_filter = Self::gltf_mag_filter_to_vk(sampler.mag_filter());

        let sampler_info = vk::SamplerCreateInfo::builder()
            .min_filter(min_filter)
            .mag_filter(mag_filter)
            .mipmap_mode(mipmap_mode)
            .address_mode_u(Self::gltf_wrap_mode_to_vk(sampler.wrap_s()))
            .address_mode_v(Self::gltf_wrap_mode_to_vk(sampler.wrap_t()));

        unsafe {
            Gfx::instance()
                .device()
                .create_sampler(&sampler_info, None)
                .expect("failed to create sampler for gltf")
        }
    }
}


// 一些工具函数
impl GltfLoader
{
    /// 将 gltf 内定义的 format 转换为 vulkan 的 format
    ///
    /// 注 只支持有限的几种格式
    fn gltf_format_to_vk(format: gltf::image::Format, s_rgb: bool) -> vk::Format
    {
        use ash::vk::Format as v;
        use gltf::image::Format as g;

        if s_rgb {
            match format {
                g::R8 => v::R8_SRGB,
                g::R8G8 => v::R8G8_SRGB,
                g::R8G8B8 => v::R8G8B8_SRGB,
                g::R8G8B8A8 => v::R8G8B8A8_SRGB,
                _ => panic!("unsupported format"),
            }
        } else {
            match format {
                g::R8 => v::R8_UNORM,
                g::R8G8 => v::R8G8_UNORM,
                g::R8G8B8 => v::R8G8B8_UNORM,
                g::R8G8B8A8 => v::R8G8B8A8_UNORM,
                _ => panic!("unsupported format"),
            }
        }
    }

    /// 将 gltf 文件中的 texture min 参数（OpenGL 风格）转换为 vulkan 格式
    ///
    /// gltf 中的 min 对应者 vulkan 中的 min filter 以及 mipmap mod
    fn gltf_min_filter_to_vk(filter: Option<gltf::texture::MinFilter>) -> (vk::Filter, vk::SamplerMipmapMode)
    {
        use ash::vk::{Filter, SamplerMipmapMode};
        use gltf::texture::MinFilter;

        // 注：LinearMipmapNearest 表示 level 内 linear，level 之间 nearest
        filter.map_or((Filter::default(), SamplerMipmapMode::default()), |filter| match filter {
            MinFilter::Nearest => (Filter::NEAREST, SamplerMipmapMode::default()),
            MinFilter::Linear => (Filter::LINEAR, SamplerMipmapMode::default()),
            MinFilter::NearestMipmapNearest => (Filter::NEAREST, SamplerMipmapMode::NEAREST),
            MinFilter::LinearMipmapNearest => (Filter::LINEAR, SamplerMipmapMode::NEAREST),
            MinFilter::NearestMipmapLinear => (Filter::NEAREST, SamplerMipmapMode::LINEAR),
            MinFilter::LinearMipmapLinear => (Filter::LINEAR, SamplerMipmapMode::LINEAR),
        })
    }

    /// 将 gltf 中纹理的 mag 参数（OpenGL 风格）转换为 vulkan 格式
    fn gltf_mag_filter_to_vk(filter: Option<gltf::texture::MagFilter>) -> vk::Filter
    {
        use ash::vk::Filter;
        use gltf::texture::MagFilter;

        filter.map_or(Filter::default(), |filter| match filter {
            MagFilter::Nearest => Filter::NEAREST,
            MagFilter::Linear => Filter::LINEAR,
        })
    }

    /// 将 gltf 中纹理的 sampler wrap mode 转换成 vk 的 wrap mode
    fn gltf_wrap_mode_to_vk(wrap_mode: gltf::texture::WrappingMode) -> vk::SamplerAddressMode
    {
        use ash::vk::SamplerAddressMode;
        use gltf::texture::WrappingMode;

        match wrap_mode {
            WrappingMode::ClampToEdge => SamplerAddressMode::CLAMP_TO_EDGE,
            WrappingMode::MirroredRepeat => SamplerAddressMode::MIRRORED_REPEAT,
            WrappingMode::Repeat => SamplerAddressMode::REPEAT,
        }
    }
}
