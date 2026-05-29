cmake_minimum_required(VERSION 3.10)

set(CMAKE_TOOLCHAIN_FILE "$ENV{VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake" CACHE STRING "")
set(CMAKE_EXPORT_COMPILE_COMMANDS ON CACHE INTERNAL "")

project(Truvixx)

# compile options
set(CMAKE_CXX_STANDARD 20)
# set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wall -Wno-c++98-compat -Wno-c++98-compat-pedantic")

if (MSVC)
    add_compile_options(/utf-8)
endif()

add_subdirectory(mods/truvixx-assimp)
add_subdirectory(mods/truvixx-gfx)
add_subdirectory(mods/truvixx-utils)
add_subdirectory(mods/truvixx-streamline)
