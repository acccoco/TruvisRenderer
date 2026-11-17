use std::{fs, path::PathBuf};

use truvis_cxx_binding_build::{CxxBindingGenerator, CxxBindingSpec, ModuleRawLine, NativeLibrary};
use truvis_path::TruvisPath;

fn main() {
    let native = NativeLibrary::new();
    let module_dir = TruvisPath::workspace().join("cxx/modules/truvixx-vk");
    let generated_include_dir = TruvisPath::target().join("cxx/generated/include");
    let roots_file = generated_include_dir.join("TruvixxVk/c_api/vulkan_include_roots.txt");
    let roots =
        fs::read_to_string(&roots_file).expect("Native Vulkan headers are not published; run `just cxx-debug` first");
    let mut include_roots = vec![module_dir.join("include"), generated_include_dir];
    let vulkan_roots: Vec<_> = roots.lines().filter(|line| !line.is_empty()).map(PathBuf::from).collect();
    assert!(
        vulkan_roots.iter().any(|path| path.join("vulkan/vulkan_core.h").is_file()),
        "Published Vulkan include roots must contain vulkan/vulkan_core.h"
    );
    include_roots.extend(vulkan_roots);

    /// 只生成本模块 ABI；Vulkan 类型直接复用 ash，包含 descriptor 的借用生命周期。
    /// PFN 在 C 中允许为空；Option 保持其可空表示，实际 Vulkan 调用约定由 ash 的 system ABI 提供。
    const MODULE_RAW_LINES: [ModuleRawLine<'static>; 1] = [ModuleRawLine {
        module: "root",
        line: r#"
pub use ash::vk::{
    Bool32 as VkBool32, CommandBuffer as VkCommandBuffer, Device as VkDevice,
    PipelineBindPoint as VkPipelineBindPoint, PipelineLayout as VkPipelineLayout,
    WriteDescriptorSet as VkWriteDescriptorSet,
};
pub type PFN_vkGetDeviceProcAddr = Option<ash::vk::PFN_vkGetDeviceProcAddr>;
pub type PFN_vkCmdPushDescriptorSetKHR = Option<ash::vk::PFN_vkCmdPushDescriptorSetKHR>;
"#,
    }];
    let generated = CxxBindingGenerator::new(CxxBindingSpec {
        output_crate: "truvis-vk-binding",
        header: module_dir.join("include/TruvixxVk/c_api/module.h"),
        include_roots,
        allowlist_types: &["TruvixxVkDeviceDispatch"],
        allowlist_functions: &["truvixx_vk_.*"],
        allowlist_vars: &[],
        module_raw_lines: &MODULE_RAW_LINES,
        layout_tests: false,
    })
    .generate();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", roots_file.display());
    println!("cargo:rustc-env=TRUVIS_VK_BINDINGS_RS={}", generated.source_path.display());
    println!("cargo:rustc-env=TRUVIS_VK_BINDINGS_HASH={}", generated.content_hash);
    println!("cargo:rustc-env=TRUVIS_VK_BINDINGS_HASH_FILE={}", generated.hash_path.display());

    native.link("truvixx-vk-capi");
    native.copy_runtime(native.output_dir(), &["truvixx-vk-capi.dll"]);
}
