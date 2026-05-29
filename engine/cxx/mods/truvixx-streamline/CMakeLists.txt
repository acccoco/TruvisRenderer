# truvixx-streamline: Streamline / DLSS C API wrapper

set(TRUVIS_WORKSPACE_ROOT "${CMAKE_SOURCE_DIR}/../..")
set(STREAMLINE_SDK_ROOT "${TRUVIS_WORKSPACE_ROOT}/tools/streamline-sdk")
set(STREAMLINE_INCLUDE_DIR "${STREAMLINE_SDK_ROOT}/include")
set(STREAMLINE_INTERPOSER_LIB "${STREAMLINE_SDK_ROOT}/lib/x64/sl.interposer.lib")

if (NOT EXISTS "${STREAMLINE_INCLUDE_DIR}/sl.h")
    message(FATAL_ERROR "Streamline SDK not found at ${STREAMLINE_SDK_ROOT}. Run `just fetch-res` first.")
endif()

if (NOT EXISTS "${STREAMLINE_INTERPOSER_LIB}")
    message(FATAL_ERROR "Streamline import library not found: ${STREAMLINE_INTERPOSER_LIB}")
endif()

file(GLOB_RECURSE C_API_SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/c_api/*.cpp)
file(GLOB_RECURSE C_API_HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxStreamline/c_api/*.h)

add_library(truvixx-streamline-capi SHARED
        ${C_API_SOURCE_FILES}
        ${C_API_HEADER_FILES_H}
)

include(GenerateExportHeader)
generate_export_header(truvixx-streamline-capi
        BASE_NAME TRUVIXX_STREAMLINE
        EXPORT_MACRO_NAME TRUVIXX_STREAMLINE_API
        EXPORT_FILE_NAME ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxStreamline/c_api/truvixx_streamline.export.h
        STATIC_DEFINE TRUVIXX_STREAMLINE_STATIC_DEFINE
)

target_include_directories(truvixx-streamline-capi PUBLIC
        include
        ${STREAMLINE_INCLUDE_DIR}
)

target_link_libraries(truvixx-streamline-capi PRIVATE
        ${STREAMLINE_INTERPOSER_LIB}
)

target_compile_features(truvixx-streamline-capi PUBLIC cxx_std_20)
