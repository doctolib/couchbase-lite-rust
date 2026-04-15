cmake_minimum_required(VERSION 3.15)

project(c_playground)

include_directories(${CMAKE_SOURCE_DIR}/../libcblite_enterprise/include)

add_executable(Main main.c)

file(GLOB CBLITE_LIB "${CMAKE_SOURCE_DIR}/../libcblite_enterprise/lib/macos/libcblite.*.dylib")
target_link_libraries(Main PUBLIC ${CBLITE_LIB})
