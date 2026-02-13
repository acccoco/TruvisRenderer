# truvixx-gfx: C++ Vulkan Graphics Library
# 对 Rust truvis-gfx 模块的 C++ 替代实现

# 第三方库
###########################################################################
find_package(VulkanHeaders CONFIG REQUIRED)

# vcpkg 的 vulkan-loader 不提供 CMake config，需要手动查找
find_library(VULKAN_LOADER vulkan-1 REQUIRED)


# target
###########################################################################
file(GLOB_RECURSE SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)
file(GLOB_RECURSE HEADER_FILES_HPP ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxGfx/*.hpp)
file(GLOB_RECURSE HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxGfx/*.h)
add_library(truvixx-gfx STATIC
        ${SOURCE_FILES}
        ${HEADER_FILES_HPP}
        ${HEADER_FILES_H}
)

target_include_directories(truvixx-gfx PUBLIC include)
target_link_libraries(truvixx-gfx PUBLIC
    Vulkan::Headers
    ${VULKAN_LOADER}
)

# 确保使用 C++20
target_compile_features(truvixx-gfx PUBLIC cxx_std_20)

