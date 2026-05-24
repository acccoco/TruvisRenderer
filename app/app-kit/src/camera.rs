use truvis_render_foundation::render_view::RenderView;

/// app 层默认相机数据结构。
///
/// 相机由 app 层持有和更新；runtime 只通过 [`Camera::render_view`] 读取本帧渲染视图快照。
/// 坐标约定为右手系、Y 轴向上，未旋转时朝向 -Z；投影矩阵的 NDC 细节保持在 `glam` 投影函数约定内。
pub struct Camera {
    pub position: glam::Vec3,

    pub euler_yaw_deg: f32,
    pub euler_pitch_deg: f32,
    pub euler_roll_deg: f32,

    pub asp: f32,
    pub fov_deg_vertical: f32,
    pub near: f32,
}

// 一些常量
impl Camera {
    /// 世界空间的上方向，也是 look_to_rh 的 up 参考。
    const CAMERA_UP: glam::Vec3 = glam::Vec3::new(0.0, 1.0, 0.0);

    /// YXZ 表示 Y(yaw)-X(pitch)-Z(roll) 的旋转顺序，匹配第一人称相机的控制语义。
    const CAMERA_EULER: glam::EulerRot = glam::EulerRot::YXZ;

    /// 没有旋转的情况下，相机看向的是 -Z。
    const CAMERA_FORWAED: glam::Vec3 = glam::Vec3::new(0.0, 0.0, -1.0);

    /// 没有旋转的情况下，相机右方向是 +X。
    const CAMERA_RIGHT: glam::Vec3 = glam::Vec3::new(1.0, 0.0, 0.0);

    /// pitch 限制在接近正负 90 度以内，避免 look direction 与 up 向量退化。
    const K_PITCH: f32 = 89.5;
}

// 访问器
impl Camera {
    #[inline]
    fn yaw_rad(&self) -> f32 {
        self.euler_yaw_deg.to_radians()
    }

    #[inline]
    fn pitch_rad(&self) -> f32 {
        self.euler_pitch_deg.to_radians()
    }

    #[inline]
    fn roll_rad(&self) -> f32 {
        self.euler_roll_deg.to_radians()
    }

    pub fn get_view_matrix(&self) -> glam::Mat4 {
        let transform = glam::Mat4::from_euler(Self::CAMERA_EULER, self.yaw_rad(), self.pitch_rad(), self.roll_rad());
        let dir = transform.transform_vector3(Self::CAMERA_FORWAED);

        glam::Mat4::look_to_rh(self.position, dir, Self::CAMERA_UP)
    }

    /// 生成右手系、Y-Up 的无限远透视投影矩阵。
    ///
    /// 调用侧应把这里当作 app 默认相机的统一投影入口，避免在 shader/pass 中重复引入坐标修正。
    pub fn get_projection_matrix(&self) -> glam::Mat4 {
        glam::Mat4::perspective_infinite_rh(self.fov_deg_vertical.to_radians(), self.asp, self.near)
    }

    pub fn render_view(&self) -> RenderView {
        RenderView::new(self.get_view_matrix(), self.get_projection_matrix(), self.position, self.camera_forward())
    }

    pub fn camera_forward(&self) -> glam::Vec3 {
        let transform = glam::Mat4::from_euler(Self::CAMERA_EULER, self.yaw_rad(), self.pitch_rad(), self.roll_rad());
        transform.transform_vector3(Self::CAMERA_FORWAED)
    }

    pub fn camera_right(&self) -> glam::Vec3 {
        let transform = glam::Mat4::from_euler(Self::CAMERA_EULER, self.yaw_rad(), self.pitch_rad(), self.roll_rad());
        transform.transform_vector3(Self::CAMERA_RIGHT)
    }

    pub fn camera_up(&self) -> glam::Vec3 {
        let transform = glam::Mat4::from_euler(
            Self::CAMERA_EULER,
            self.euler_yaw_deg.to_radians(),
            self.euler_pitch_deg.to_radians(),
            self.euler_roll_deg.to_radians(),
        );
        transform.transform_vector3(Self::CAMERA_UP)
    }
}

// 相机控制
impl Camera {
    /// 朝相机看向的方向进行移动
    pub fn move_forward(&mut self, length: f32) {
        self.position += self.camera_forward() * length;
    }

    /// 沿当前相机局部右方向移动。
    pub fn move_right(&mut self, length: f32) {
        self.position += self.camera_right() * length;
    }

    pub fn set_aspect_ratio(&mut self, asp: f32) {
        self.asp = asp;
    }

    /// 朝世界的 Up 进行移动
    pub fn move_up(&mut self, length: f32) {
        self.position += Self::CAMERA_UP * length;
    }

    pub fn rotate_yaw(&mut self, angle: f32) {
        // yaw 保持在 [0, 360) 内，避免长时间运行后角度无限增长影响调试输出。
        self.euler_yaw_deg += angle;
        self.euler_yaw_deg %= 360.0;
        if self.euler_yaw_deg < 0.0 {
            self.euler_yaw_deg += 360.0;
        }
    }

    pub fn rotate_pitch(&mut self, angle: f32) {
        // pitch 不允许跨过正负 90 度，保持 camera_up 与 view direction 的关系稳定。
        self.euler_pitch_deg += angle;
        self.euler_pitch_deg = self.euler_pitch_deg.clamp(-Self::K_PITCH, Self::K_PITCH);
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: glam::Vec3::new(0.0, 0.0, 0.0),
            euler_yaw_deg: 0.0,
            euler_pitch_deg: 0.0,
            euler_roll_deg: 0.0,
            asp: 1.0,
            fov_deg_vertical: 60.0,
            near: 0.1,
        }
    }
}
