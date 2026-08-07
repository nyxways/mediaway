/*
 * mediaway.hpp — header-only C++ wrapper over Mediaway's C ABI (umbrella
 * header: core.hpp + container.hpp + pipeline.hpp + device.hpp).
 *
 * Implements the DX contract in bindings/cpp/README.md (see also
 * docs/spec/c-ffi.md · ADR-0004): RAII classes own the opaque C handles
 * (unique_ptr + custom deleter), the ABI's per-crate status enums are
 * translated into mediaway::Error exceptions at the boundary, and the ABI's
 * handle-consumption traps (mediaway_encode_session_open / _finish consume
 * their handle unconditionally) are made unrepresentable via rvalue-qualified
 * typestate methods (begin() && / finish() &&).
 *
 * Split into core.hpp/container.hpp (+ per-format headers under container/)/
 * pipeline.hpp/device.hpp once wiring all 8 container formats pushed the
 * combined file past the workspace's 1000-line source-file cap — this file
 * is now a pure umbrella; `#include <mediaway/mediaway.hpp>` still pulls in
 * everything, unchanged for existing consumers.
 *
 * Capability truth (bindings/README.md truth table): container mux/demux
 * (all 8 formats) and the auto video encode -> fMP4 pipeline are real;
 * camera/mic capture are real (CPU frames); Screen capture is not
 * representable from C today — ScreenCapture::open() throws
 * Error(Status::Unsupported).
 */

#ifndef MEDIAWAY_MEDIAWAY_HPP
#define MEDIAWAY_MEDIAWAY_HPP

#include <mediaway/container.hpp>
#include <mediaway/core.hpp>
#include <mediaway/device.hpp>
#include <mediaway/pipeline.hpp>

#endif  // MEDIAWAY_MEDIAWAY_HPP
