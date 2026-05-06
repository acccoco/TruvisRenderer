use ash::vk;
use itertools::Itertools;

use truvis_gfx::gfx::{GfxDeviceCtx, GfxImmediateCtx, GfxResourceCtx};
use truvis_gfx::raytracing::acceleration::GfxAcceleration;
use truvis_gfx::resources::lifecycle::DestroyReason;
use truvis_render_interface::geometry::RtGeometry;

/// CPU 侧的 Mesh 数据
pub struct Mesh {
    pub geometries: Vec<RtGeometry>,

    pub blas: Option<GfxAcceleration>,
    pub name: String,
    pub blas_device_address: Option<vk::DeviceAddress>,
}

impl Mesh {
    pub fn build_blas(
        &mut self,
        resource_ctx: GfxResourceCtx<'_>,
        device_ctx: GfxDeviceCtx<'_>,
        immediate_ctx: GfxImmediateCtx<'_>,
    ) {
        if self.blas.is_some() {
            return; // 已经构建过了
        }

        let blas_infos = self.geometries.iter().map(|g| g.get_blas_geometry_info()).collect_vec();
        let blas = GfxAcceleration::build_blas_sync(
            resource_ctx,
            device_ctx,
            immediate_ctx,
            &blas_infos,
            vk::BuildAccelerationStructureFlagsKHR::empty(),
            format!("{}-Blas", self.name),
        );

        self.blas_device_address = Some(blas.device_address(device_ctx));
        self.blas = Some(blas);
    }

    pub fn destroy_mut(&mut self, resource_ctx: GfxResourceCtx<'_>, device_ctx: GfxDeviceCtx<'_>) {
        if let Some(blas) = self.blas.take() {
            blas.destroy(resource_ctx, device_ctx, DestroyReason::Shutdown);
        }
        for geometry in &mut self.geometries {
            geometry.destroy_mut(resource_ctx, DestroyReason::Shutdown);
        }
        self.blas_device_address = None;
    }
}
