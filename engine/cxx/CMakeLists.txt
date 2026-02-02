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

add_subdirectory(truvixx-assimp)
add_subdirectory(truvixx-interface)
add_subdirectory(truvixx-gfx)


# 在配置完成后复制 compile_commands.json 到 build 目录
if(EXISTS "${CMAKE_BINARY_DIR}/compile_commands.json")
  file(COPY "${CMAKE_BINARY_DIR}/compile_commands.json" 
       DESTINATION "${CMAKE_SOURCE_DIR}/build")
endif()
