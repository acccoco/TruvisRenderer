find_package(VulkanHeaders CONFIG REQUIRED)

add_library(truvixx-vk-capi SHARED
    c_api/module.cpp
    include/TruvixxVk/c_api/module.h
)

truvis_generate_export_header(
    TARGET truvixx-vk-capi
    BASE_NAME TRUVIXX_VK
    EXPORT_MACRO_NAME TRUVIXX_VK_API
    RELATIVE_PATH TruvixxVk/c_api/truvixx_vk.export.h
)

target_include_directories(truvixx-vk-capi PUBLIC include)
target_link_libraries(truvixx-vk-capi PRIVATE Vulkan::Headers)
target_compile_definitions(truvixx-vk-capi PUBLIC VK_NO_PROTOTYPES)
target_compile_features(truvixx-vk-capi PUBLIC cxx_std_20)

# configure 只写入当前 binary dir；真正 build 后才发布给 Rust，避免 clangd 辅助 configure 覆盖来源。
set(vulkan_include_roots "${CMAKE_CURRENT_BINARY_DIR}/vulkan_include_roots.txt")
file(GENERATE OUTPUT "${vulkan_include_roots}"
    CONTENT "$<JOIN:$<TARGET_PROPERTY:Vulkan::Headers,INTERFACE_INCLUDE_DIRECTORIES>,\n>\n"
)
add_custom_target(truvixx-vk-bindgen-inputs ALL
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${vulkan_include_roots}"
        "${TRUVIS_CXX_GENERATED_INCLUDE_DIR}/TruvixxVk/c_api/vulkan_include_roots.txt"
    DEPENDS truvixx-vk-capi
    VERBATIM
)
