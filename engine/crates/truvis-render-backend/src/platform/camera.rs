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
    /// 相机的上参考向量
    const CAMERA_UP: glam::Vec3 = glam::Vec3::new(0.0, 1.0, 0.0);

    /// YXZ 表示 Y(yaw)-X(Pitch)-Z(Roll) 的旋转顺序
    const CAMERA_EULER: glam::EulerRot = glam::EulerRot::YXZ;

    /// 没有旋转的情况下，相机看向的是 -Z
    const CAMERA_FORWAED: glam::Vec3 = glam::Vec3::new(0.0, 0.0, -1.0);

    const CAMERA_RIGHT: glam::Vec3 = glam::Vec3::new(1.0, 0.0, 0.0);

    const K_PITCH: f32 = 89.5;
}

// getter
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

    /// 从 RightHand-Y-Up 的 ViewSpace 转换到 LeftHand-Y-Up 的 NDC
    pub fn get_projection_matrix(&self) -> glam::Mat4 {
        glam::Mat4::perspective_infinite_rh(self.fov_deg_vertical.to_radians(), self.asp, self.near)
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
        self.euler_yaw_deg += angle;
        self.euler_yaw_deg %= 360.0;
        if self.euler_yaw_deg < 0.0 {
            self.euler_yaw_deg += 360.0;
        }
    }

    pub fn rotate_pitch(&mut self, angle: f32) {
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
