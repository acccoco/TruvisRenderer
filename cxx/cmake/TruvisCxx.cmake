include_guard(GLOBAL)

include(CMakeParseArguments)
include(GenerateExportHeader)

function(truvis_generate_export_header)
    set(one_value_args TARGET BASE_NAME EXPORT_MACRO_NAME RELATIVE_PATH)
    cmake_parse_arguments(ARG "" "${one_value_args}" "" ${ARGN})

    foreach(required_arg IN ITEMS TARGET BASE_NAME EXPORT_MACRO_NAME RELATIVE_PATH)
        if(NOT ARG_${required_arg})
            message(FATAL_ERROR "truvis_generate_export_header requires ${required_arg}")
        endif()
    endforeach()

    if(NOT TARGET ${ARG_TARGET})
        message(FATAL_ERROR "Cannot generate an export header for missing target '${ARG_TARGET}'")
    endif()

    get_target_property(target_type ${ARG_TARGET} TYPE)
    if(NOT target_type STREQUAL "SHARED_LIBRARY")
        message(FATAL_ERROR "Public CXX target '${ARG_TARGET}' must be a SHARED library, got ${target_type}")
    endif()

    set(output_path "${TRUVIS_CXX_GENERATED_INCLUDE_DIR}/${ARG_RELATIVE_PATH}")
    get_filename_component(output_dir "${output_path}" DIRECTORY)
    file(MAKE_DIRECTORY "${output_dir}")

    generate_export_header(
        ${ARG_TARGET}
        BASE_NAME ${ARG_BASE_NAME}
        EXPORT_MACRO_NAME ${ARG_EXPORT_MACRO_NAME}
        EXPORT_FILE_NAME "${output_path}"
    )

    target_include_directories(
        ${ARG_TARGET}
        PUBLIC
            "$<BUILD_INTERFACE:${TRUVIS_CXX_GENERATED_INCLUDE_DIR}>"
    )
endfunction()
