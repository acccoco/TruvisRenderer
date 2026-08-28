cmake_minimum_required(VERSION 3.21)

project(TruvisNative LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON CACHE BOOL "Generate compile_commands.json" FORCE)

if(MSVC)
    add_compile_options(/utf-8)
endif()

set(TRUVIS_CXX_ROOT "${CMAKE_CURRENT_SOURCE_DIR}" CACHE INTERNAL "Truvis CXX project root")
get_filename_component(TRUVIS_WORKSPACE_ROOT "${TRUVIS_CXX_ROOT}/.." ABSOLUTE)
set(TRUVIS_WORKSPACE_ROOT "${TRUVIS_WORKSPACE_ROOT}" CACHE INTERNAL "Truvis workspace root")
set(
    TRUVIS_CXX_GENERATED_INCLUDE_DIR
    "${TRUVIS_WORKSPACE_ROOT}/build/cxx/generated/include"
    CACHE INTERNAL
    "Generated public CXX headers"
)

list(PREPEND CMAKE_MODULE_PATH "${TRUVIS_CXX_ROOT}/cmake")
include(TruvisCxx)

add_subdirectory(modules)

truvis_finalize_runtime_plans()
