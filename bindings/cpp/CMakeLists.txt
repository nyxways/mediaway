# Mediaway native bindings package (C ABI headers + C++ wrapper + prebuilt
# native lib for the host platform).
#
# Installs:
#   include/mediaway/container.h  include/mediaway/pipeline.h  include/mediaway/device.h
#   include/mediaway/mediaway.hpp (C++ RAII wrapper)
#   bin/mediaway_ffi.dll          (Windows GNU runtime — add bin/ to PATH)
#   lib/libmediaway_ffi.so/.dylib (Linux/macOS runtime)
#   lib/pkgconfig/mediaway.pc     (C consumers)
#   MediawayConfig.cmake          (C++ consumers: find_package(mediaway))
#
# Packaged as Mediaway-<version>-<platform>.zip/.tar.gz via CPack, one
# archive per (OS, arch) — see docs/adr/0024-multi-platform-native-binding-
# distribution.md. The native libs are Rust build artifacts staged by
# tools/scripts/copy-native-dlls.ts; point MEDIAWAY_NATIVE_DIR_OVERRIDE
# elsewhere to override, or MEDIAWAY_PLATFORM_TAG to force the archive
# name/RID when cross-building (CMAKE_SYSTEM_PROCESSOR does not reliably
# self-report the target arch for a cross-compiled macOS arm64 build).
#
# Usage:
#   cmake -S bindings/cpp -B build -DCMAKE_INSTALL_PREFIX=prefix
#   cmake --build build && cmake --install build
#   cpack --config build/CPackConfig.cmake

cmake_minimum_required(VERSION 3.20)
project(mediaway-bindings VERSION 0.1.0 LANGUAGES C CXX)

set(MEDIAWAY_HEADERS
  ${CMAKE_CURRENT_SOURCE_DIR}/../../crates/mediaway-ffi/include/mediaway/container.h
  ${CMAKE_CURRENT_SOURCE_DIR}/../../crates/mediaway-ffi/include/mediaway/pipeline.h
  ${CMAKE_CURRENT_SOURCE_DIR}/../../crates/mediaway-ffi/include/mediaway/device.h
)
set(MEDIAWAY_CPP_HEADER ${CMAKE_CURRENT_SOURCE_DIR}/include/mediaway/mediaway.hpp)

if(NOT MEDIAWAY_PLATFORM_TAG)
  if(WIN32)
    set(MEDIAWAY_PLATFORM_TAG "win64")
  elseif(APPLE)
    if(CMAKE_SYSTEM_PROCESSOR MATCHES "arm64|aarch64")
      set(MEDIAWAY_PLATFORM_TAG "macos-arm64")
    else()
      set(MEDIAWAY_PLATFORM_TAG "macos-x64")
    endif()
  else()
    set(MEDIAWAY_PLATFORM_TAG "linux-x64")
  endif()
endif()

# RID matching tools/scripts/copy-native-dlls.ts's staging layout
# (bindings/native/runtime/<rid>).
if(MEDIAWAY_PLATFORM_TAG STREQUAL "win64")
  set(MEDIAWAY_RID "win-x64")
elseif(MEDIAWAY_PLATFORM_TAG STREQUAL "macos-arm64")
  set(MEDIAWAY_RID "osx-arm64")
elseif(MEDIAWAY_PLATFORM_TAG STREQUAL "macos-x64")
  set(MEDIAWAY_RID "osx-x64")
else()
  set(MEDIAWAY_RID "linux-x64")
endif()

set(MEDIAWAY_NATIVE_DIR ${MEDIAWAY_NATIVE_DIR_OVERRIDE})
if(NOT MEDIAWAY_NATIVE_DIR)
  set(MEDIAWAY_NATIVE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../native/runtime/${MEDIAWAY_RID})
endif()

if(WIN32)
  file(GLOB MEDIAWAY_DLLS ${MEDIAWAY_NATIVE_DIR}/*.dll)
elseif(APPLE)
  file(GLOB MEDIAWAY_DLLS ${MEDIAWAY_NATIVE_DIR}/*.dylib)
else()
  file(GLOB MEDIAWAY_DLLS ${MEDIAWAY_NATIVE_DIR}/*.so)
endif()

# Header-only INTERFACE library — the whole "package" for C and C++ consumers.
add_library(mediaway INTERFACE)
target_include_directories(mediaway INTERFACE
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
# Import libs (libmediaway_*_ffi.dll.a) staged next to the DLLs — C++ consumers
# link the C ABI exports directly; C consumers use pkg-config instead.
target_link_directories(mediaway INTERFACE
  $<BUILD_INTERFACE:${MEDIAWAY_NATIVE_DIR}>
  $<INSTALL_INTERFACE:${CMAKE_INSTALL_PREFIX}/lib>
)
target_link_libraries(mediaway INTERFACE
  mediaway_ffi)
target_compile_definitions(mediaway INTERFACE MEDIAWAY_STATIC=1)

install(FILES ${MEDIAWAY_HEADERS} ${MEDIAWAY_CPP_HEADER} DESTINATION include/mediaway)
if(WIN32)
  # DLLs are loaded from PATH at runtime, not the linker search path — the
  # Windows convention is bin/, unlike Unix shared objects (lib/ below).
  install(FILES ${MEDIAWAY_DLLS} DESTINATION bin)
  file(GLOB MEDIAWAY_IMPORT_LIBS ${MEDIAWAY_NATIVE_DIR}/*.dll.a)
  install(FILES ${MEDIAWAY_IMPORT_LIBS} DESTINATION lib)
else()
  # Linux/macOS: the shared object is both the link-time and load-time
  # artifact, no separate import lib — lib/ matches the linker search path
  # already configured above ($<INSTALL_INTERFACE:${CMAKE_INSTALL_PREFIX}/lib>).
  install(FILES ${MEDIAWAY_DLLS} DESTINATION lib)
endif()
install(TARGETS mediaway EXPORT MediawayTargets)
install(EXPORT MediawayTargets
  NAMESPACE mediaway::
  DESTINATION lib/cmake/mediaway
)
include(CMakePackageConfigHelpers)
write_basic_package_version_file(
  ${CMAKE_CURRENT_BINARY_DIR}/MediawayConfigVersion.cmake
  VERSION ${PROJECT_VERSION}
  COMPATIBILITY SameMajorVersion
)
configure_package_config_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/cmake/MediawayConfig.cmake.in
  ${CMAKE_CURRENT_BINARY_DIR}/MediawayConfig.cmake
  INSTALL_DESTINATION lib/cmake/mediaway
)
install(FILES
  ${CMAKE_CURRENT_BINARY_DIR}/MediawayConfig.cmake
  ${CMAKE_CURRENT_BINARY_DIR}/MediawayConfigVersion.cmake
  DESTINATION lib/cmake/mediaway
)

# pkg-config for plain-C consumers. Written via file(WRITE) (not configure_file)
# so the installed .pc keeps LF endings — configure_file on Windows emits CRLF,
# and pkg-config mangles a trailing CR into the last flag.
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/mediaway.pc
"prefix=${CMAKE_INSTALL_PREFIX}\n"
"includedir=\${prefix}/include\n"
"libdir=\${prefix}/lib\n"
"\n"
"Name: mediaway\n"
"Description: Mediaway C ABI — fragmented-MP4 mux/demux, hardware video/audio encode, camera/mic capture\n"
"Version: ${PROJECT_VERSION}\n"
"Cflags: -I\${includedir}\n"
"Libs: -L\${libdir} -lmediaway_ffi\n")
install(FILES ${CMAKE_CURRENT_BINARY_DIR}/mediaway.pc
  DESTINATION lib/pkgconfig)

# CPack: a self-contained per-platform archive with headers + native lib.
set(CPACK_PACKAGE_NAME "Mediaway")
set(CPACK_PACKAGE_VERSION ${PROJECT_VERSION})
set(CPACK_PACKAGE_FILE_NAME "Mediaway-${PROJECT_VERSION}-${MEDIAWAY_PLATFORM_TAG}")
set(CPACK_GENERATOR "ZIP;TGZ")
set(CPACK_STRIP_FILES OFF)
# Stage packages (and the _CPack_Packages temp dir) inside the build dir —
# CPack's default output dir is the invoking cwd, which pollutes the repo
# root when run from there (and breaks the CI upload glob).
set(CPACK_OUTPUT_FILE_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/dist")
include(CPack)
