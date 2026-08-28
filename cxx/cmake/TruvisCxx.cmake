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

function(_truvis_runtime_configuration_key configuration output_var)
    string(TOUPPER "${configuration}" configuration_key)
    if(NOT configuration_key STREQUAL "DEBUG" AND NOT configuration_key STREQUAL "RELEASE")
        message(FATAL_ERROR "Unsupported CXX runtime configuration '${configuration}'")
    endif()
    set(${output_var} "${configuration_key}" PARENT_SCOPE)
endfunction()

function(_truvis_json_string value output_var)
    file(TO_CMAKE_PATH "${value}" normalized_value)
    string(REPLACE "\"" "\\\"" escaped_value "${normalized_value}")
    set(${output_var} "\"${escaped_value}\"" PARENT_SCOPE)
endfunction()

function(_truvis_register_runtime_file target configuration required source_path)
    _truvis_runtime_configuration_key("${configuration}" configuration_key)

    if(NOT IS_ABSOLUTE "${source_path}")
        get_filename_component(source_path "${source_path}" ABSOLUTE BASE_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
    endif()
    file(TO_CMAKE_PATH "${source_path}" source_path)
    get_filename_component(destination "${source_path}" NAME)
    if(destination STREQUAL "")
        message(FATAL_ERROR "CXX runtime source has no file name: ${source_path}")
    endif()

    string(TOLOWER "${destination}" destination_key)
    get_property(registered_destinations GLOBAL PROPERTY "TRUVIS_RUNTIME_DESTINATIONS_${configuration_key}")
    if(registered_destinations)
        list(FIND registered_destinations "${destination_key}" destination_index)
        if(NOT destination_index EQUAL -1)
            message(FATAL_ERROR "Duplicate ${configuration} CXX runtime destination '${destination}'")
        endif()
    endif()
    set_property(GLOBAL APPEND PROPERTY "TRUVIS_RUNTIME_DESTINATIONS_${configuration_key}" "${destination_key}")

    _truvis_json_string("${target}" target_json)
    _truvis_json_string("${source_path}" source_json)
    _truvis_json_string("${destination}" destination_json)
    set(item_json
        "    {\"target\": ${target_json}, \"source\": ${source_json}, \"destination\": ${destination_json}, \"required\": ${required}}"
    )
    set_property(GLOBAL APPEND PROPERTY "TRUVIS_RUNTIME_ITEMS_${configuration_key}" "${item_json}")
endfunction()

function(truvis_register_runtime_files)
    set(one_value_args TARGET)
    set(multi_value_args CONFIGURATIONS REQUIRED OPTIONAL)
    cmake_parse_arguments(ARG "" "${one_value_args}" "${multi_value_args}" ${ARGN})

    if(NOT ARG_TARGET OR NOT ARG_CONFIGURATIONS)
        message(FATAL_ERROR "truvis_register_runtime_files requires TARGET and CONFIGURATIONS")
    endif()
    if(NOT TARGET ${ARG_TARGET})
        message(FATAL_ERROR "Cannot register runtime files for missing target '${ARG_TARGET}'")
    endif()
    if(NOT ARG_REQUIRED AND NOT ARG_OPTIONAL)
        message(FATAL_ERROR "truvis_register_runtime_files requires REQUIRED or OPTIONAL files")
    endif()

    foreach(configuration IN LISTS ARG_CONFIGURATIONS)
        foreach(source_path IN LISTS ARG_REQUIRED)
            _truvis_register_runtime_file("${ARG_TARGET}" "${configuration}" true "${source_path}")
        endforeach()
        foreach(source_path IN LISTS ARG_OPTIONAL)
            _truvis_register_runtime_file("${ARG_TARGET}" "${configuration}" false "${source_path}")
        endforeach()
    endforeach()
endfunction()

function(truvis_finalize_runtime_plans)
    set(plan_dir "${CMAKE_BINARY_DIR}/runtime")
    file(MAKE_DIRECTORY "${plan_dir}")

    foreach(configuration IN ITEMS Debug Release)
        _truvis_runtime_configuration_key("${configuration}" configuration_key)
        get_property(items GLOBAL PROPERTY "TRUVIS_RUNTIME_ITEMS_${configuration_key}")
        if(items)
            string(JOIN ",\n" items_json ${items})
        else()
            set(items_json "")
        endif()

        set(plan_path "${plan_dir}/truvis-runtime-${configuration}.json")
        file(WRITE "${plan_path}"
            "{\n"
            "  \"version\": 1,\n"
            "  \"configuration\": \"${configuration}\",\n"
            "  \"artifacts\": [\n"
            "${items_json}\n"
            "  ]\n"
            "}\n"
        )
    endforeach()
endfunction()
