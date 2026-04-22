use ash::vk;
use itertools::Itertools;

use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_render_interface::geometry::RtGeometry;

/// CPU 侧的 Mesh 数据
pub struct Mesh {
    pub geometries: Vec<RtGeometry>,

    pub blas: Option<GfxAcceleration>,
    pub name: String,
    pub blas_device_address: Option<vk::DeviceAddress>,
}

impl Mesh {
    pub fn build_blas(&mut self) {
        if self.blas.is_some() {
            return; // 已经构建过了
        }

        let blas_infos = self.geometries.iter().map(|g| g.get_blas_geometry_info()).collect_vec();
        let blas = GfxAcceleration::build_blas_sync(
            &blas_infos,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            format!("{}-Blas", self.name),
        );

        self.blas_device_address = Some(blas.device_address());
        self.blas = Some(blas);
    }
}
