use glam::{Mat3, Vec3};

/// 显示变换；数值直接用于 Renderer shader ABI。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ToneMappingMode {
    AgX = 0,
    AcesFitted = 1,
    None = 2,
    #[default]
    PbrNeutral = 3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ExposureMode {
    Auto = 0,
    #[default]
    Manual = 1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum MeteringMode {
    #[default]
    CenterWeighted = 0,
    Average = 1,
}

/// 曝光仅缩放显示输入，不改变路径 radiance。EV 表达增益 stops，正值提亮，并非相机 EV100。
#[derive(Clone, Copy)]
pub struct ExposureSettings {
    pub mode: ExposureMode,
    pub manual_ev: f32,
    /// 仅 Auto 使用；Manual 不读取自动增益或补偿。
    pub auto_compensation_ev: f32,
    pub min_ev: f32,
    pub max_ev: f32,
    pub metering: MeteringMode,
    pub low_percentile: f32,
    pub high_percentile: f32,
    pub darken_seconds: f32,
    pub brighten_seconds: f32,
    /// 只锁定自动增益；显示曲线和用户补偿仍立即生效。
    pub locked: bool,
}

impl Default for ExposureSettings {
    fn default() -> Self {
        Self {
            mode: ExposureMode::default(),
            manual_ev: 0.0,
            auto_compensation_ev: 0.0,
            min_ev: -16.0,
            max_ev: 16.0,
            metering: MeteringMode::CenterWeighted,
            low_percentile: 0.1,
            high_percentile: 0.9,
            darken_seconds: 0.25,
            brighten_seconds: 1.0,
            locked: false,
        }
    }
}

impl ExposureSettings {
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        for (value, default, min, max) in [
            (&mut self.manual_ev, defaults.manual_ev, -24.0, 24.0),
            (&mut self.auto_compensation_ev, defaults.auto_compensation_ev, -8.0, 8.0),
            (&mut self.min_ev, defaults.min_ev, -24.0, 24.0),
            (&mut self.max_ev, defaults.max_ev, -24.0, 24.0),
            (&mut self.low_percentile, defaults.low_percentile, 0.0, 1.0),
            (&mut self.high_percentile, defaults.high_percentile, 0.0, 1.0),
            (&mut self.darken_seconds, defaults.darken_seconds, 0.01, 10.0),
            (&mut self.brighten_seconds, defaults.brighten_seconds, 0.01, 10.0),
        ] {
            *value = if value.is_finite() { value.clamp(min, max) } else { default };
        }
        if self.min_ev > self.max_ev {
            std::mem::swap(&mut self.min_ev, &mut self.max_ev);
        }
        if self.low_percentile >= self.high_percentile {
            self.low_percentile = defaults.low_percentile;
            self.high_percentile = defaults.high_percentile;
        }
    }
}

/// 曝光之后、显示映射之前的 linear Rec.709 调色；中性参数保持输入不变。
#[derive(Clone, Copy)]
pub struct ColorGradingSettings {
    /// 相对冷暖偏移而非 Kelvin；正值偏暖。
    pub temperature: f32,
    /// 相对绿—洋红偏移；正值偏洋红。
    pub tint: f32,
    pub contrast: f32,
    pub shadows_ev: f32,
    pub midtones_ev: f32,
    pub highlights_ev: f32,
    pub saturation: f32,
}

impl Default for ColorGradingSettings {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            tint: 0.0,
            contrast: 1.0,
            shadows_ev: 0.0,
            midtones_ev: 0.0,
            highlights_ev: 0.0,
            saturation: 1.0,
        }
    }
}

impl ColorGradingSettings {
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        for (value, default, min, max) in [
            (&mut self.temperature, defaults.temperature, -100.0, 100.0),
            (&mut self.tint, defaults.tint, -100.0, 100.0),
            (&mut self.contrast, defaults.contrast, 0.5, 1.5),
            (&mut self.shadows_ev, defaults.shadows_ev, -2.0, 2.0),
            (&mut self.midtones_ev, defaults.midtones_ev, -2.0, 2.0),
            (&mut self.highlights_ev, defaults.highlights_ev, -2.0, 2.0),
            (&mut self.saturation, defaults.saturation, 0.0, 2.0),
        ] {
            *value = if value.is_finite() { value.clamp(min, max) } else { default };
        }
    }

    /// Unity Graphics 03ca85dffdde4b7bc1d6870074e6f5ff9f0352a3：
    /// ColorUtils.ColorBalanceToLMSCoeffs 与 ShaderLibrary/Color.hlsl 的配套 LMS 矩阵。
    /// 将白点适应公式合成为 CPU 矩阵，避免逐像素重复转换；零偏移显式恒等以消除参考常量舍入误差。
    pub fn white_balance_matrix(&self) -> Mat3 {
        if self.temperature == 0.0 && self.tint == 0.0 {
            return Mat3::IDENTITY;
        }
        let temperature = self.temperature / 65.0;
        let x = 0.31271 - temperature * if temperature < 0.0 { 0.1 } else { 0.05 };
        let y = 2.87 * x - 3.0 * x * x - 0.27509507 + self.tint / 65.0 * 0.05;
        let xyz = Vec3::new(x / y, 1.0, (1.0 - x - y) / y);
        let reference_lms = Vec3::new(
            Vec3::new(0.7328, 0.4296, -0.1624).dot(xyz),
            Vec3::new(-0.7036, 1.6975, 0.0061).dot(xyz),
            Vec3::new(0.0030, 0.0136, 0.9834).dot(xyz),
        );
        let balance = Vec3::new(0.949237, 1.03542, 1.08728) / reference_lms;
        // 参考资料按行给出；glam 按列构造，因此显式转置。
        let rgb_to_lms = Mat3::from_cols_array_2d(&[
            [0.390405, 0.549941, 0.00892632],
            [0.0708416, 0.963172, 0.00135775],
            [0.0231082, 0.128021, 0.936245],
        ])
        .transpose();
        let lms_to_rgb = Mat3::from_cols_array_2d(&[
            [2.85847, -1.62879, -0.0248910],
            [-0.210182, 1.15820, 0.000324281],
            [-0.0418120, -0.118169, 1.06867],
        ])
        .transpose();
        lms_to_rgb * Mat3::from_diagonal(balance) * rgb_to_lms
    }

    /// 固定分区只在 shader 计算权重；用户 stops 的指数转换每次 pass 录制仅执行一次。
    pub fn tonal_gains(&self) -> Vec3 {
        Vec3::new(self.shadows_ev.exp2(), self.midtones_ev.exp2(), self.highlights_ev.exp2())
    }
}

/// SDR sRGB 显示设置，不表达完整 ACES/OCIO 或 HDR display transform。
#[derive(Clone, Copy)]
pub struct SdrPostProcessSettings {
    pub tone_mapping: ToneMappingMode,
    pub exposure: ExposureSettings,
    pub color_grading: ColorGradingSettings,
    pub dither: bool,
}

impl Default for SdrPostProcessSettings {
    fn default() -> Self {
        Self {
            tone_mapping: ToneMappingMode::default(),
            exposure: ExposureSettings::default(),
            color_grading: ColorGradingSettings::default(),
            dither: true,
        }
    }
}

impl SdrPostProcessSettings {
    pub fn normalize(&mut self) {
        self.exposure.normalize();
        self.color_grading.normalize();
    }
}
