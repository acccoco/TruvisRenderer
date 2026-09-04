use std::{fs, path::PathBuf};

use truvis_cxx_binding_codegen::{CxxBindingGenerator, CxxBindingSpec, ModuleRawLine};
use truvis_path::TruvisPath;

fn main() {
    let target = std::env::var("TARGET").expect("TARGET must be set by Cargo");
    assert_eq!(
        target, "x86_64-pc-windows-msvc",
        "Native Vulkan binding currently requires Windows x64 MSVC ABI"
    );

    let module_dir = TruvisPath::workspace().join("cxx/modules/truvixx-vk");
    let generated_include_dir = TruvisPath::target().join("cxx/generated/include");
    let roots_file = generated_include_dir.join("TruvixxVk/c_api/vulkan_include_roots.txt");
    let roots =
        fs::read_to_string(&roots_file).expect("Native Vulkan headers are not published; run `just cxx-debug` first");
    let mut include_roots = vec![module_dir.join("include"), generated_include_dir];
    let vulkan_roots: Vec<_> = roots
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();
    assert!(
        vulkan_roots
            .iter()
            .any(|path| path.join("vulkan/vulkan_core.h").is_file()),
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
    println!(
        "cargo:rustc-env=TRUVIS_VK_BINDINGS_RS={}",
        generated.source_path.display()
    );
    println!("cargo:rustc-env=TRUVIS_VK_BINDINGS_HASH={}", generated.content_hash);
    println!(
        "cargo:rustc-env=TRUVIS_VK_BINDINGS_HASH_FILE={}",
        generated.hash_path.display()
    );

    let profile = std::env::var("PROFILE").expect("PROFILE must be set by Cargo");
    println!(
        "cargo:rustc-link-search=native={}",
        TruvisPath::target().join(profile).display()
    );
    println!("cargo:rustc-link-lib=dylib=truvixx-vk-capi");
}
