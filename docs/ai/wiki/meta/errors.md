# Errors (`thiserror`)

Canonical: [`docs/conventions/error-handling.md`](../../../conventions/error-handling.md) · [ADR-0010](../../../adr/0010-thiserror-errors.md).

- Library / sans-io public errors: **`thiserror` enums**, English `#[error]`, prefer `#[non_exhaustive]`.
- No `anyhow` / `Box<dyn Error>` as the public library error type.
- Own errors in the core crate; facade may re-export (`mediaway_container::mp4::Error`).
- Examples: `iso_bmff::Error`, `mediaway-test-media::TestMediaError`.
