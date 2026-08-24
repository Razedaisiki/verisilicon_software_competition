cmake_minimum_required(VERSION 3.20)
project(verisilicon_sr_submission NONE)

if(NOT WIN32)
  message(FATAL_ERROR "This submission supports Windows only")
endif()

set(RUST_TOOLCHAIN "1.85.0")
set(RUSTC_IDENTITY "rustc 1.85.0 (4d91de4e4 2025-02-17)")
set(RUST_TARGET "x86_64-pc-windows-msvc")
set(MANIFEST "${CMAKE_CURRENT_LIST_DIR}/src/Cargo.toml")
set(CARGO_TARGET_DIR "${CMAKE_BINARY_DIR}/cargo-target")
set(CANDIDATE "${CARGO_TARGET_DIR}/${RUST_TARGET}/release/sr.exe")
set(REBUILT_DIR "${CMAKE_CURRENT_LIST_DIR}/rebuilt")
set(REBUILT "${REBUILT_DIR}/sr.exe")
set(PACKAGED "${CMAKE_CURRENT_LIST_DIR}/bin/sr.exe")

find_program(CARGO_EXECUTABLE NAMES cargo REQUIRED)
find_program(RUSTC_EXECUTABLE NAMES rustc REQUIRED)

execute_process(
  COMMAND "${RUSTC_EXECUTABLE}" "+${RUST_TOOLCHAIN}" --version
  RESULT_VARIABLE RUSTC_RESULT
  OUTPUT_VARIABLE RUSTC_VERSION
  OUTPUT_STRIP_TRAILING_WHITESPACE
)
if(NOT RUSTC_RESULT EQUAL 0 OR NOT RUSTC_VERSION STREQUAL RUSTC_IDENTITY)
  message(FATAL_ERROR "Exact ${RUSTC_IDENTITY} is required; received: ${RUSTC_VERSION}")
endif()

set(BUILD_SCRIPT "${CMAKE_BINARY_DIR}/BuildRust.cmake")
set(BUILD_SCRIPT_CONTENT [=[
string(ASCII 31 RUST_FLAG_SEPARATOR)
set(ENV{CARGO_NET_OFFLINE} "true")
set(ENV{CARGO_INCREMENTAL} "0")
set(ENV{CARGO_ENCODED_RUSTFLAGS}
  "-C${RUST_FLAG_SEPARATOR}link-arg=/Brepro${RUST_FLAG_SEPARATOR}-C${RUST_FLAG_SEPARATOR}target-feature=+crt-static${RUST_FLAG_SEPARATOR}--remap-path-prefix=@PACKAGE_ROOT@/src=."
)
execute_process(
  COMMAND "@CARGO_EXECUTABLE@" "+@RUST_TOOLCHAIN@" build
    --offline --locked --release
    --target "@RUST_TARGET@"
    --manifest-path "@MANIFEST@"
    --target-dir "@CARGO_TARGET_DIR@"
  RESULT_VARIABLE CARGO_RESULT
  COMMAND_ERROR_IS_FATAL ANY
)
file(MAKE_DIRECTORY "@REBUILT_DIR@")
file(COPY_FILE "@CANDIDATE@" "@REBUILT@" ONLY_IF_DIFFERENT)
execute_process(
  COMMAND "@CMAKE_COMMAND@" -E compare_files "@REBUILT@" "@PACKAGED@"
  RESULT_VARIABLE COMPARE_RESULT
)
if(NOT COMPARE_RESULT EQUAL 0)
  message(FATAL_ERROR "Rebuilt sr.exe differs from packaged bin/sr.exe")
endif()
message(STATUS "Offline rebuild is byte-identical to bin/sr.exe for @RUST_TARGET@")
]=])
set(PACKAGE_ROOT "${CMAKE_CURRENT_LIST_DIR}")
string(CONFIGURE "${BUILD_SCRIPT_CONTENT}" BUILD_SCRIPT_CONTENT @ONLY)
file(WRITE "${BUILD_SCRIPT}" "${BUILD_SCRIPT_CONTENT}")

add_custom_target(sr ALL
  COMMAND "${CMAKE_COMMAND}" -P "${BUILD_SCRIPT}"
  BYPRODUCTS "${CANDIDATE}" "${REBUILT}"
  COMMENT "Building sr.exe with Rust ${RUST_TOOLCHAIN} and verifying binary identity"
  VERBATIM
  USES_TERMINAL
)
