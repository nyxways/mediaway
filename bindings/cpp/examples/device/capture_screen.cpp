// capture_screen.cpp - device capability: screen capture quick start.
//
// Status: NOT AVAILABLE - the C ABI returns INVALID_INPUT/UNSUPPORTED for
// screen capture today (it needs a live GPU device handle with no CPU
// fallback; see bindings/c/README.md's truth table). This example is the
// capture-only analog of pipeline/screen_record.cpp: it calls
// ScreenCapture::open, expects the documented Status::Unsupported, prints the
// honest gap, and exits gracefully. The acquisition loop the wrapper should
// provide once the ABI lands is shown in screen_record.cpp.

#include <mediaway/mediaway.hpp>

#include <cstdlib>
#include <iostream>

int main() {
  try {
    mediaway::device::ScreenCapture screen =
        mediaway::device::ScreenCapture::open({0, {1, 30}});
    (void)screen;
    std::cerr << "unexpected: ScreenCapture::open succeeded — the ABI changed\n";
    return EXIT_FAILURE;
  } catch (const mediaway::Error& error) {
    if (error.status() != mediaway::Status::Unsupported) {
      std::cerr << "mediaway error: " << error.what() << " (status "
                << static_cast<int>(error.status()) << ")\n";
      return EXIT_FAILURE;
    }
    std::cout << "Screen capture is NOT available from this binding today:\n"
              << "  it needs a live GPU device handle (ID3D11Device*) with no\n"
              << "  CPU fallback, and its C representation is deferred.\n"
              << "  Exiting gracefully — nothing to capture yet.\n";
    return EXIT_SUCCESS;
  }
}
