// window_config.cpp - compile+run check for the window-capture config (ADR-0005).
//
// The C++ wrapper hand-mirrors mediaway_desktop_capture_config_t through the C
// header. Pinning its layout here makes a reordered or resized field fail the
// build instead of silently corrupting a config. No window, GPU or hardware is
// needed: every runtime check below is rejected before any backend call.
//
//   g++ -std=c++17 -I bindings/cpp/include -I crates/mediaway-ffi/include
//       bindings/cpp/tests/window_config.cpp -L <native dir> -lmediaway_ffi

#include <mediaway/mediaway.hpp>

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>

// The layout below is the 64-bit one (uint64_t is 8-aligned). It matches the
// Rust `#[repr(C)]` MediawayDesktopCaptureConfig, pinned there by
// `config_layout_is_pinned`.
#if UINTPTR_MAX == UINT64_MAX
using Config = mediaway_desktop_capture_config_t;
static_assert(sizeof(Config) == 88, "config size changed: bump the ABI version and every binding");
static_assert(offsetof(Config, source_kind) == 0, "");
static_assert(offsetof(Config, source_index) == 4, "");
static_assert(offsetof(Config, time_base) == 8, "");
static_assert(offsetof(Config, gpu_device) == 24, "");
static_assert(offsetof(Config, window_handle) == 48, "");
static_assert(offsetof(Config, cursor) == 56, "");
static_assert(offsetof(Config, border) == 60, "");
static_assert(offsetof(Config, dimensions) == 64, "");
static_assert(offsetof(Config, region_x) == 68, "");
static_assert(offsetof(Config, region_y) == 72, "");
static_assert(offsetof(Config, region_width) == 76, "");
static_assert(offsetof(Config, region_height) == 80, "");
static_assert(offsetof(Config, region_enabled) == 84, "");
#endif

// The C++ enums must carry the header's values, not their own.
static_assert(static_cast<std::uint32_t>(mediaway::device::CursorCapture::Included) ==
              MEDIAWAY_CAPTURE_CURSOR_INCLUDED);
static_assert(static_cast<std::uint32_t>(mediaway::device::CaptureBorder::Hidden) ==
              MEDIAWAY_CAPTURE_BORDER_HIDDEN);
static_assert(static_cast<std::uint32_t>(mediaway::device::FrameDimensions::EvenCropped) ==
              MEDIAWAY_FRAME_DIMENSIONS_EVEN_CROPPED);

static int failures = 0;

#define CHECK(cond)                                                              \
  do {                                                                           \
    if (!(cond)) {                                                               \
      std::cerr << "FAILED: " #cond " (" << __FILE__ << ':' << __LINE__ << ")\n"; \
      ++failures;                                                                \
    }                                                                            \
  } while (0)

int main() {
  // The device ABI must be the version this wrapper was written against.
  CHECK(mediaway_device_ffi_abi_version() == MEDIAWAY_DEVICE_FFI_ABI_VERSION);
  CHECK(MEDIAWAY_DEVICE_FFI_ABI_VERSION == 2);

  const mediaway_gpu_device_handle_t none{};
  const mediaway_desktop_capture_config_t raw =
      mediaway_desktop_capture_config_window(0xABCD, {1, 30}, none);
  CHECK(raw.source_kind == MEDIAWAY_DESKTOP_CAPTURE_SOURCE_WINDOW);
  CHECK(raw.window_handle == 0xABCD);
  // Zero means "as before": whole surface, no pointer, OS border, native size.
  CHECK(raw.cursor == MEDIAWAY_CAPTURE_CURSOR_EXCLUDED);
  CHECK(raw.border == MEDIAWAY_CAPTURE_BORDER_SHOWN);
  CHECK(raw.dimensions == MEDIAWAY_FRAME_DIMENSIONS_NATIVE);
  CHECK(!raw.region_enabled);

  // REGION_OUT_OF_BOUNDS is its own wrapper status, not a generic CaptureError.
  try {
    mediaway::detail::checkDevice(MEDIAWAY_DEVICE_STATUS_REGION_OUT_OF_BOUNDS);
    CHECK(false);
  } catch (const mediaway::Error& error) {
    CHECK(error.status() == mediaway::Status::RegionOutOfBounds);
    CHECK(error.rawCode() == 14);
  }

  // An HWND of 0 names no window. Rejected before any backend call; off Windows
  // the Window source does not exist at all.
  try {
    (void)mediaway::device::WindowCapture::open({0, {1, 30}, none});
    CHECK(false);
  } catch (const mediaway::Error& error) {
    CHECK(error.status() == mediaway::Status::CaptureError ||
          error.status() == mediaway::Status::Unsupported);
  }

  if (failures != 0) return EXIT_FAILURE;
  std::cout << "window_config: ok\n";
  return EXIT_SUCCESS;
}
