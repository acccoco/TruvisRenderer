# 在 native build 完成后执行，第三方 DLL 必须从 target 声明的真实来源解析。
# 不扫描 output 目录；那里可能残留其它分支或工具链的产物。
set(CMAKE_GET_RUNTIME_DEPENDENCIES_PLATFORM windows+pe)
set(CMAKE_GET_RUNTIME_DEPENDENCIES_TOOL dumpbin)
list(FILTER runtime_libraries EXCLUDE REGEX "^$")
list(REMOVE_DUPLICATES runtime_libraries)
if(runtime_libraries)
    file(TO_CMAKE_PATH "$ENV{SystemRoot}" system_root)
    string(TOLOWER "${system_root}" system_root)
    if(system_root STREQUAL "")
        message(FATAL_ERROR "SystemRoot is required to classify Windows runtime dependencies")
    endif()
    # Windows 路径大小写不敏感，按真实 SystemRoot 排除，避免误排项目内同名目录。
    set(system_pattern "^")
    string(REGEX MATCHALL "." system_characters "${system_root}/")
    foreach(character IN LISTS system_characters)
        if(character MATCHES "[a-z]")
            string(TOUPPER "${character}" upper)
            string(APPEND system_pattern "[${character}${upper}]")
        elseif(character STREQUAL "/")
            string(APPEND system_pattern "[/\\\\]")
        else()
            string(APPEND system_pattern "[${character}]")
        endif()
    endforeach()
    file(GET_RUNTIME_DEPENDENCIES
        LIBRARIES ${runtime_libraries}
        RESOLVED_DEPENDENCIES_VAR dependencies
        PRE_EXCLUDE_REGEXES "^(api-ms-|ext-ms-)"
        POST_EXCLUDE_REGEXES "${system_pattern}"
    )
    list(APPEND runtime_libraries ${dependencies})
    list(REMOVE_DUPLICATES runtime_libraries)
    list(SORT runtime_libraries)
    foreach(source IN LISTS runtime_libraries)
        file(TO_CMAKE_PATH "${source}" source)
        string(TOLOWER "${source}" source_lower)
        cmake_path(IS_PREFIX system_root "${source_lower}" NORMALIZE is_system)
        if(is_system)
            continue()
        endif()
        get_filename_component(destination "${source}" NAME)
        string(REPLACE "\"" "\\\"" source "${source}")
        string(APPEND items_json
            ",\n    {\"target\": \"linked-runtime\", \"source\": \"${source}\", \"destination\": \"${destination}\", \"required\": true}"
        )
    endforeach()
endif()

# 解析失败时不发布半份 plan，runner 也不会继续部署旧 plan。
file(WRITE "${plan_path}.tmp"
    "{\n  \"version\": 2,\n  \"configuration\": \"${configuration}\",\n  \"artifacts\": [\n${items_json}\n  ]\n}\n"
)
file(RENAME "${plan_path}.tmp" "${plan_path}")
