# truvixx-streamline: Streamline / DLSS C API wrapper

set(STREAMLINE_SDK_ROOT "${TRUVIS_WORKSPACE_ROOT}/external/streamline-sdk")
set(STREAMLINE_INCLUDE_DIR "${STREAMLINE_SDK_ROOT}/include")

# 这里只依赖 Streamline 头文件，不链接 sl.interposer.lib。
# C++ wrapper 会在运行时使用 Rust 传入的绝对路径 LoadLibraryW("sl.interposer.dll")，
# 避免链接期依赖让 Windows loader 在 truvixx-streamline-capi.dll 加载时提前拉起 SL runtime。
if (NOT EXISTS "${STREAMLINE_INCLUDE_DIR}/sl.h")
    message(FATAL_ERROR "Streamline SDK not found at ${STREAMLINE_SDK_ROOT}. Run `just fetch-res` first.")
endif()

find_package(VulkanHeaders CONFIG REQUIRED)

file(GLOB_RECURSE C_API_SOURCE_FILES CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/c_api/*.cpp)
file(GLOB_RECURSE C_API_HEADER_FILES_H CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxStreamline/c_api/*.h)

add_library(truvixx-streamline-capi SHARED
        ${C_API_SOURCE_FILES}
        ${C_API_HEADER_FILES_H}
)

truvis_generate_export_header(
        TARGET truvixx-streamline-capi
        BASE_NAME TRUVIXX_STREAMLINE
        EXPORT_MACRO_NAME TRUVIXX_STREAMLINE_API
        RELATIVE_PATH TruvixxStreamline/c_api/truvixx_streamline.export.h
)

target_include_directories(truvixx-streamline-capi PUBLIC
        include
        ${STREAMLINE_INCLUDE_DIR}
)

target_link_libraries(truvixx-streamline-capi PRIVATE
        truvixx-utils
        Vulkan::Headers
)

target_compile_features(truvixx-streamline-capi PUBLIC cxx_std_20)

set(STREAMLINE_REQUIRED_DLLS
        sl.interposer.dll
        sl.common.dll
        sl.pcl.dll
        sl.dlss.dll
        nvngx_dlss.dll
        sl.dlss_d.dll
        nvngx_dlssd.dll
        NvLowLatencyVk.dll
)

set(STREAMLINE_DEBUG_RUNTIME_FILES)
set(STREAMLINE_RELEASE_RUNTIME_FILES)
foreach(dll_name IN LISTS STREAMLINE_REQUIRED_DLLS)
    list(APPEND STREAMLINE_DEBUG_RUNTIME_FILES "${STREAMLINE_SDK_ROOT}/bin/x64/development/${dll_name}")
    list(APPEND STREAMLINE_RELEASE_RUNTIME_FILES "${STREAMLINE_SDK_ROOT}/bin/x64/${dll_name}")
endforeach()

truvis_register_runtime_files(
        TARGET truvixx-streamline-capi
        CONFIGURATIONS Debug
        REQUIRED ${STREAMLINE_DEBUG_RUNTIME_FILES}
        OPTIONAL "${STREAMLINE_SDK_ROOT}/bin/x64/development/sl.imgui.dll"
)

truvis_register_runtime_files(
        TARGET truvixx-streamline-capi
        CONFIGURATIONS Release
        REQUIRED ${STREAMLINE_RELEASE_RUNTIME_FILES}
)

file(GLOB STREAMLINE_CONFIG_FILES CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/runtime/sl.*.json")
if(NOT STREAMLINE_CONFIG_FILES)
    message(FATAL_ERROR "Streamline runtime config is missing from ${CMAKE_CURRENT_SOURCE_DIR}/runtime")
endif()

truvis_register_runtime_files(
        TARGET truvixx-streamline-capi
        CONFIGURATIONS Debug Release
        REQUIRED ${STREAMLINE_CONFIG_FILES}
)
