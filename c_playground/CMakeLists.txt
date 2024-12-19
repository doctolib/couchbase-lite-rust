cmake_minimum_required(VERSION 3.15)

project(c_playground)

include_directories(${CMAKE_SOURCE_DIR}/../libcblite/include)

add_executable(Main main.c)

target_link_libraries(Main PUBLIC ${CMAKE_SOURCE_DIR}/../libcblite/lib/macos/libcblite.3.dylib)
