cmake_minimum_required(VERSION 3.16)
project(cmake_calc C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

add_executable(calc
  src/main.c
  src/calc.c
  src/lexer.c
)

target_include_directories(calc PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
