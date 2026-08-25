/// runtime prepare 阶段读取的渲染视图快照。
///
/// `RenderView` 是 app 层相机状态和 render runtime 之间的窄边界：runtime 不关心相机
/// 如何被输入控制、如何存储欧拉角或轨道状态，只消费本帧 shader 与累积渲染需要的矩阵和方向。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderView {
    pub view: glam::Mat4,
    pub projection: glam::Mat4,
    pub inv_view: glam::Mat4,
    pub inv_projection: glam::Mat4,
    pub position_ws: glam::Vec3,
    pub forward_ws: glam::Vec3,
}

impl RenderView {
    /// 构造本帧渲染视图，并在边界处固定 inverse 矩阵。
    pub fn new(view: glam::Mat4, projection: glam::Mat4, position_ws: glam::Vec3, forward_ws: glam::Vec3) -> Self {
        Self {
            view,
            projection,
            inv_view: view.inverse(),
            inv_projection: projection.inverse(),
            position_ws,
            forward_ws,
        }
    }

    /// 返回累积渲染判断稳定性需要比较的相机签名。
    pub fn accum_signature(&self) -> RenderViewAccumSignature {
        RenderViewAccumSignature {
            view: self.view,
            projection: self.projection,
            position_ws: self.position_ws,
            forward_ws: self.forward_ws,
        }
    }
}

/// 累积渲染用于判断历史结果是否仍然可复用的视图签名。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderViewAccumSignature {
    pub view: glam::Mat4,
    pub projection: glam::Mat4,
    pub position_ws: glam::Vec3,
    pub forward_ws: glam::Vec3,
}
