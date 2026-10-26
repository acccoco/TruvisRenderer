set(NGX_ROOT "${TRUVIS_WORKSPACE_ROOT}/external/dlss-sdk")
set(NR_ROOT "${TRUVIS_WORKSPACE_ROOT}/external/dlss-nr-sdk")
if(NOT EXISTS "${NR_ROOT}/include/nvsdk_ngx_helpers_dlssnr_vk.h")
    message(FATAL_ERROR "DLSS NR SDK missing. Run just fetch-res nvidia-dlss-sdk nvidia-dlss-nr-sdk")
endif()
find_package(VulkanHeaders CONFIG REQUIRED)
add_library(truvixx-dlss-nr-capi SHARED c_api/module.cpp include/TruvixxDlssNr/c_api/module.h)
truvis_generate_export_header(
    TARGET truvixx-dlss-nr-capi
    BASE_NAME TRUVIXX_DLSS_NR
    EXPORT_MACRO_NAME TRUVIXX_DLSS_NR_API
    RELATIVE_PATH TruvixxDlssNr/c_api/truvixx_dlss_nr.export.h
)
target_include_directories(truvixx-dlss-nr-capi PUBLIC include PRIVATE "${NR_ROOT}/include" "${NGX_ROOT}/include")
target_compile_definitions(truvixx-dlss-nr-capi PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN VK_NO_PROTOTYPES)
target_compile_features(truvixx-dlss-nr-capi PUBLIC cxx_std_20)
target_link_libraries(truvixx-dlss-nr-capi PRIVATE Vulkan::Headers truvixx-utils
    debug "${NGX_ROOT}/lib/Windows_x86_64/x64/nvsdk_ngx_d_dbg.lib"
    optimized "${NGX_ROOT}/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib"
)
truvis_register_runtime_files(
    TARGET truvixx-dlss-nr-capi CONFIGURATIONS Debug Release
    REQUIRED "${NR_ROOT}/lib/Windows_x86_64/rel/nvngx_dlssnr.dll"
)
