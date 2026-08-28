/// 主 RT 流程支持的调试通道。
///
/// 数值由 RT/Sdr shader push constant 消费；这里用 enum 固定语义，避免 UI 直接暴露 magic number。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathTracingDebugChannel {
    /// 标准最终颜色输出。
    Final,
    /// 显示 RT shading 当前实际使用的 world-space forward normal。
    ///
    /// 该法线经过 `faceforward` 翻面，会随入射 ray 保持同侧；这是旧 `normal` 通道的兼容语义。
    ForwardNormal,
    /// 显示未经过 `faceforward` 翻面的 world-space 几何法线。
    WorldNormal,
    /// 显示 mesh object/local space 中的插值顶点法线。
    ObjectNormal,
    /// 显示材质 base color / albedo。
    BaseColor,
    /// 显示 next-event estimation 中来自 HDRI 的直接光。
    NeeHdri,
    /// 显示自发光材质贡献。
    Emission,
    /// 显示 BRDF 采样到 HDRI 的间接贡献。
    BrdfHdri,
    /// 显示第 0 次 bounce 的 NEE 贡献。
    NeeBounce0,
    /// 显示第 1 次 bounce 的 NEE 贡献。
    NeeBounce1,
    /// 显示 next-event estimation 中来自自发光三角形的直接光。
    NeeEmissive,
    /// 显示 next-event estimation 中来自 analytic light 的直接光。
    NeeAnalytic,
    /// 显示 primary surface 的粗粒度材质分类。
    MaterialType,
    /// 显示 primary surface 是否属于 specular / transparent delta path。
    DeltaMask,
    /// 显示 DLSS RR 使用的 primary specular motion vector 长度。
    SpecularMotionMagnitude,
    /// 显示 ReSTIR DI initial reservoir 的权重强度。
    RestirInitialWeight,
    /// 显示 ReSTIR DI temporal reservoir 是否有效及 history age。
    RestirTemporalValid,
    /// 显示 ReSTIR DI final shade contribution。
    RestirFinalContribution,
    /// 显示 SHARC hash grid 在 primary hit 处的 voxel 着色，用于观察 grid 结构与 scene scale。
    SharcHashGrid,
    /// 显示 SHARC resolved 缓存在 primary hit 处的 radiance，用于确认 Update/Resolve 是否写入缓存。
    SharcCache,
    /// SHARC query 命中深度 heatmap（绿=depth1，黄=depth2，红=3+，黑=未命中），观察缓存使用与路径成本。
    SharcQueryDepth,
}

impl PathTracingDebugChannel {
    pub const ALL: [Self; 21] = [
        Self::Final,
        Self::ForwardNormal,
        Self::WorldNormal,
        Self::ObjectNormal,
        Self::BaseColor,
        Self::NeeHdri,
        Self::Emission,
        Self::BrdfHdri,
        Self::NeeBounce0,
        Self::NeeBounce1,
        Self::NeeEmissive,
        Self::NeeAnalytic,
        Self::MaterialType,
        Self::DeltaMask,
        Self::SpecularMotionMagnitude,
        Self::RestirInitialWeight,
        Self::RestirTemporalValid,
        Self::RestirFinalContribution,
        Self::SharcHashGrid,
        Self::SharcCache,
        Self::SharcQueryDepth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Final => "final",
            Self::ForwardNormal => "forward normal",
            Self::WorldNormal => "world normal",
            Self::ObjectNormal => "object normal",
            Self::BaseColor => "base color",
            Self::NeeHdri => "from NEE HDRI",
            Self::Emission => "from emission",
            Self::BrdfHdri => "from BRDF HDRI",
            Self::NeeBounce0 => "NEE bounce 0",
            Self::NeeBounce1 => "NEE bounce 1",
            Self::NeeEmissive => "from NEE emissive",
            Self::NeeAnalytic => "from NEE analytic",
            Self::MaterialType => "material type",
            Self::DeltaMask => "delta mask",
            Self::SpecularMotionMagnitude => "specular motion magnitude",
            Self::RestirInitialWeight => "ReSTIR initial weight",
            Self::RestirTemporalValid => "ReSTIR temporal valid",
            Self::RestirFinalContribution => "ReSTIR final contribution",
            Self::SharcHashGrid => "SHARC hash grid",
            Self::SharcCache => "SHARC cache radiance",
            Self::SharcQueryDepth => "SHARC query depth",
        }
    }

    pub fn shader_channel(self) -> u32 {
        match self {
            Self::Final => 0,
            Self::ForwardNormal => 1,
            Self::WorldNormal => 10,
            Self::ObjectNormal => 11,
            Self::BaseColor => 2,
            Self::NeeHdri => 4,
            Self::Emission => 5,
            Self::BrdfHdri => 6,
            Self::NeeBounce0 => 7,
            Self::NeeBounce1 => 8,
            Self::NeeEmissive => 9,
            Self::NeeAnalytic => 12,
            Self::MaterialType => 16,
            Self::DeltaMask => 17,
            Self::SpecularMotionMagnitude => 18,
            Self::RestirInitialWeight => 13,
            Self::RestirTemporalValid => 14,
            Self::RestirFinalContribution => 15,
            Self::SharcHashGrid => 19,
            Self::SharcCache => 20,
            Self::SharcQueryDepth => 21,
        }
    }
}
