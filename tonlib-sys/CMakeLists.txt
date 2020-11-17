cmake_minimum_required(VERSION 3.10)

set(PROJECT_NAME tonlib-sys-cpp)

project(${PROJECT_NAME} LANGUAGES CXX)

add_subdirectory(ton EXCLUDE_FROM_ALL)

add_library(${PROJECT_NAME} STATIC src/tonlib-sys.cpp src/tonlib-sys.hpp)

set_target_properties(${PROJECT_NAME} PROPERTIES
        CXX_STANDARD 17
        CXX_STANDARD_REQUIRED YES)

set_target_properties(${PROJECT_NAME} PROPERTIES
        LINK_FLAGS "-Wl,--whole-archive")

target_link_libraries(${PROJECT_NAME} PRIVATE
        tonlib tdactor adnllite tl_lite_api tl-lite-utils ton_crypto ton_block lite-client-common smc-envelope ftabi)

function(bundle_static_library tgt_name)
    set(bundled_tgt_name "${tgt_name}-bundled")

    list(APPEND static_libs ${tgt_name})

    function(_recursively_collect_dependencies input_target)
        set(_input_link_libraries LINK_LIBRARIES)
        get_target_property(_input_type ${input_target} TYPE)
        if (${_input_type} STREQUAL "INTERFACE_LIBRARY")
            set(_input_link_libraries INTERFACE_LINK_LIBRARIES)
        endif ()
        get_target_property(public_dependencies ${input_target} ${_input_link_libraries})
        foreach (dependency IN LISTS public_dependencies)
            if (TARGET ${dependency})
                get_target_property(alias ${dependency} ALIASED_TARGET)
                if (TARGET ${alias})
                    set(dependency ${alias})
                endif ()
                get_target_property(_type ${dependency} TYPE)
                if (${_type} STREQUAL "STATIC_LIBRARY")
                    list(APPEND static_libs ${dependency})
                endif ()

                get_property(library_already_added
                        GLOBAL PROPERTY _${tgt_name}_static_bundle_${dependency})
                if (NOT library_already_added)
                    set_property(GLOBAL PROPERTY _${tgt_name}_static_bundle_${dependency} ON)
                    _recursively_collect_dependencies(${dependency})
                endif ()
            endif ()
        endforeach ()
        set(static_libs ${static_libs} PARENT_SCOPE)
    endfunction()

    _recursively_collect_dependencies(${tgt_name})

    list(REMOVE_DUPLICATES static_libs)

    set(bundled_tgt_full_name
            ${CMAKE_INSTALL_PREFIX}/${CMAKE_STATIC_LIBRARY_PREFIX}${bundled_tgt_name}${CMAKE_STATIC_LIBRARY_SUFFIX})

    file(REMOVE ${bundled_tgt_full_name})

    if (CMAKE_CXX_COMPILER_ID MATCHES "^(Clang|GNU)$")
        file(WRITE ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar.in
                "CREATE ${bundled_tgt_full_name}\n")

        foreach (tgt IN LISTS static_libs)
            file(APPEND ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar.in
                    "ADDLIB $<TARGET_FILE:${tgt}>\n")
        endforeach ()

        file(APPEND ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar.in "SAVE\n")
        file(APPEND ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar.in "END\n")

        file(GENERATE
                OUTPUT ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar
                INPUT ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar.in)

        set(ar_tool ${CMAKE_AR})
        if (CMAKE_INTERPROCEDURAL_OPTIMIZATION)
            set(ar_tool ${CMAKE_CXX_COMPILER_AR})
        endif ()

        add_custom_command(
                COMMAND ${ar_tool} -M < ${CMAKE_BINARY_DIR}/${bundled_tgt_name}.ar
                OUTPUT ${bundled_tgt_full_name}
                COMMENT "Bundling ${bundled_tgt_name}"
                VERBATIM)
    elseif (MSVC)
        find_program(lib_tool lib)

        foreach (tgt IN LISTS static_libs)
            list(APPEND static_libs_full_names $<TARGET_FILE:${tgt}>)
        endforeach ()

        add_custom_command(
                COMMAND ${lib_tool} /NOLOGO /OUT:${bundled_tgt_full_name} ${static_libs_full_names}
                OUTPUT ${bundled_tgt_full_name}
                COMMENT "Bundling ${bundled_tgt_name}"
                VERBATIM)
    else ()
        message(FATAL_ERROR "Unknown bundle scenario!")
    endif ()

    add_custom_target(${bundled_tgt_name} ALL DEPENDS ${bundled_tgt_full_name})
    add_dependencies(${bundled_tgt_name} ${tgt_name})
endfunction()

bundle_static_library(tonlib-sys-cpp)
