pub struct LabelColor;
impl LabelColor {
    const _RED: glam::Vec4 = glam::vec4(1.0, 0.0, 0.0, 1.0);
    const GREEN: glam::Vec4 = glam::vec4(0.0, 1.0, 0.0, 1.0);
    const BLUE: glam::Vec4 = glam::vec4(0.0, 0.0, 1.0, 1.0);
    const _WHITE: glam::Vec4 = glam::vec4(1.0, 1.0, 1.0, 1.0);
    const _BLACK: glam::Vec4 = glam::vec4(0.0, 0.0, 0.0, 1.0);
    const YELLOW: glam::Vec4 = glam::vec4(1.0, 1.0, 0.0, 1.0);
    const _CYAN: glam::Vec4 = glam::vec4(0.0, 1.0, 1.0, 1.0);
    const _MAGENTA: glam::Vec4 = glam::vec4(1.0, 0.0, 1.0, 1.0);
    const _GRAY: glam::Vec4 = glam::vec4(0.5, 0.5, 0.5, 1.0);
    const _LIGHT_GRAY: glam::Vec4 = glam::vec4(0.75, 0.75, 0.75, 1.0);
    const _DARK_GRAY: glam::Vec4 = glam::vec4(0.25, 0.25, 0.25, 1.0);

    pub const COLOR_PASS: glam::Vec4 = Self::BLUE;
    pub const COLOR_STAGE: glam::Vec4 = Self::YELLOW;
    pub const COLOR_CMD: glam::Vec4 = Self::GREEN;
}
