# Mediaway native bindings package (C ABI headers + C++ wrapper + win-x64 DLLs).
#
# Installs:
#   include/mediaway/container.h  include/mediaway/pipeline.h  include/mediaway/device.h
#   include/mediaway/mediaway.hpp (C++ RAII wrapper)
#   bin/mediaway_*_ffi.dll        (Windows GNU runtime — add bin/ to PATH)
#   lib/pkgconfig/mediaway.pc     (C consumers)
#   MediawayConfig.cmake          (C++ consumers: find_package(mediaway))
#
# Packaged as Mediaway-<version>-win64.zip/.tar.gz via CPack. The DLLs are
# Rust build artifacts staged by tools/scripts/copy-native-dlls.ts; point
# MEDIAWAY_NATIVE_DIR elsewhere to override.
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
set(MEDIAWAY_NATIVE_DIR ${MEDIAWAY_NATIVE_DIR_OVERRIDE})
if(NOT MEDIAWAY_NATIVE_DIR)
  set(MEDIAWAY_NATIVE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../native/runtime/win-x64)
endif()
file(GLOB MEDIAWAY_DLLS ${MEDIAWAY_NATIVE_DIR}/*.dll)

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
install(FILES ${MEDIAWAY_DLLS} DESTINATION bin)
file(GLOB MEDIAWAY_IMPORT_LIBS ${MEDIAWAY_NATIVE_DIR}/*.dll.a)
install(FILES ${MEDIAWAY_IMPORT_LIBS} DESTINATION lib)
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

# CPack: a self-contained win64 archive with headers + DLLs.
set(CPACK_PACKAGE_NAME "Mediaway")
set(CPACK_PACKAGE_VERSION ${PROJECT_VERSION})
set(CPACK_PACKAGE_FILE_NAME "Mediaway-${PROJECT_VERSION}-win64")
set(CPACK_GENERATOR "ZIP;TGZ")
set(CPACK_STRIP_FILES OFF)
# Stage packages (and the _CPack_Packages temp dir) inside the build dir —
# CPack's default output dir is the invoking cwd, which pollutes the repo
# root when run from there (and breaks the CI upload glob).
set(CPACK_OUTPUT_FILE_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/dist")
include(CPack)
