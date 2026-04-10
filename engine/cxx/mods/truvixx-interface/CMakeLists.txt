


# target main
###########################################################################
file(GLOB_RECURSE SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)
file(GLOB_RECURSE HEADER_FILES_HPP ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxInterface/*.hpp)
file(GLOB_RECURSE HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxInterface/*.h)
add_library(truvixx-interface SHARED
        ${SOURCE_FILES}
        ${HEADER_FILES_HPP}
        ${HEADER_FILES_H}
)

# 定义导入/导出宏
include(GenerateExportHeader)
generate_export_header(truvixx-interface
        BASE_NAME TRUVIXX_INTERFACE
        EXPORT_MACRO_NAME TRUVIXX_INTERFACE_API
        EXPORT_FILE_NAME ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxInterface/truvixx_interface.export.h
        STATIC_DEFINE TRUVIXX_INTERFACE_STATIC_DEFINE
)


target_include_directories(truvixx-interface PUBLIC include)
target_link_libraries(truvixx-interface PUBLIC
        truvixx-assimp
        truvixx-gfx
)


# target main
###########################################################################
add_executable(truvixx-main main.cpp)
target_link_libraries(truvixx-main truvixx-assimp truvixx-interface)


