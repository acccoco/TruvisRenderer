# truvixx-utils: shared C++ helper utilities for native modules

file(GLOB_RECURSE SOURCE_FILES ${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp)
file(GLOB_RECURSE HEADER_FILES_HPP ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxUtils/*.hpp)
file(GLOB_RECURSE HEADER_FILES_H ${CMAKE_CURRENT_SOURCE_DIR}/include/TruvixxUtils/*.h)

add_library(truvixx-utils STATIC
        ${SOURCE_FILES}
        ${HEADER_FILES_HPP}
        ${HEADER_FILES_H}
)

target_include_directories(truvixx-utils PUBLIC include)
target_compile_features(truvixx-utils PUBLIC cxx_std_20)
