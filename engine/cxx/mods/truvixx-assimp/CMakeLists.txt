
# 第三方库
###########################################################################
find_package(assimp CONFIG REQUIRED)
find_package(glm CONFIG REQUIRED)


# target
###########################################################################
file(GLOB_RECURSE SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)
file(GLOB_RECURSE HEADER_FILES_HPP ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxAssimp/*.hpp)
file(GLOB_RECURSE HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxAssimp/*.h)
add_library(truvixx-assimp STATIC
        ${SOURCE_FILES}
        ${HEADER_FILES_HPP}
        ${HEADER_FILES_H}
)

target_include_directories(truvixx-assimp PUBLIC include)
target_link_libraries(truvixx-assimp PUBLIC
        assimp::assimp
        glm::glm
)


# Rust FFI C API
###########################################################################
file(GLOB_RECURSE C_API_SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/c_api/*.cpp)
file(GLOB_RECURSE C_API_HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxAssimp/c_api/*.h)
add_library(truvixx-assimp-capi SHARED
        ${C_API_SOURCE_FILES}
        ${C_API_HEADER_FILES_H}
)

# 定义导入/导出宏
include(GenerateExportHeader)
generate_export_header(truvixx-assimp-capi
        BASE_NAME TRUVIXX_ASSIMP
        EXPORT_MACRO_NAME TRUVIXX_ASSIMP_API
        EXPORT_FILE_NAME ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxAssimp/c_api/truvixx_assimp.export.h
        STATIC_DEFINE TRUVIXX_ASSIMP_STATIC_DEFINE
)

target_include_directories(truvixx-assimp-capi PUBLIC include)
target_link_libraries(truvixx-assimp-capi PRIVATE
        truvixx-assimp
)
