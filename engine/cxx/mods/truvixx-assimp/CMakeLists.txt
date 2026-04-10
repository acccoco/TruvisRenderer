
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

