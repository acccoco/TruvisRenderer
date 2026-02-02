
# 第三方库
###########################################################################
find_package(VulkanHeaders CONFIG REQUIRED)


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
)

