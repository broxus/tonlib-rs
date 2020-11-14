cmake_minimum_required(VERSION 3.10)

set(PROJECT_NAME tonlib-sys)

project(${PROJECT_NAME} LANGUAGES CXX)

add_subdirectory(ton EXCLUDE_FROM_ALL)

add_library(${PROJECT_NAME} STATIC tonlib-sys.cpp tonlib-sys.hpp)

set_target_properties(${PROJECT_NAME} PROPERTIES
        CXX_STANDARD 17
        CXX_STANDARD_REQUIRED YES)

target_link_libraries(${PROJECT_NAME} PRIVATE
        tonlib tdactor adnllite tl_lite_api tl-lite-utils ton_crypto ton_block lite-client-common smc-envelope ftabi)
