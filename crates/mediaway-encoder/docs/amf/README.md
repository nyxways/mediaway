# mediaway-encoder-amf

AMD **vendor** backend for hardware encode (AMF) — deferred.

**Status: 🛠️, not ❌** — blockers found so far are our own binding/dependency
choices (a GPL-licensed crates.io squatter, an MSRV conflict in the real
Apache-2.0 bindings), not an AMD-side capability absence. No AMD GPU has been
available to verify against once built. See
[ADR-0001](adr/0001-amf-deferred-no-hardware.md).
