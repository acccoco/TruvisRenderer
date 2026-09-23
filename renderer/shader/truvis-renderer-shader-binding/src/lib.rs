//! Renderer owner 的 shader ABI 自动生成绑定。

mod _shader_bindings {
    include!(env!("TRUVIS_RENDERER_SHADER_BINDINGS_RS"));
}

// 生成文件路径保持稳定；内容 hash 只用于让 Cargo / rust-analyzer 感知 binding 内容变化。
const _: &str = env!("TRUVIS_RENDERER_SHADER_BINDINGS_HASH");
const _: &str = include_str!(env!("TRUVIS_RENDERER_SHADER_BINDINGS_HASH_FILE"));

pub use crate::_shader_bindings::root as gpu;

/// bindgen 对跨 owner 的 Float4 引用不推导 Copy；vertex record 仅含两个可复制数值向量。
impl Copy for gpu::renderer::render_passes::light_overlay::Vertex {}
impl Clone for gpu::renderer::render_passes::light_overlay::Vertex {
    fn clone(&self) -> Self {
        *self
    }
}
