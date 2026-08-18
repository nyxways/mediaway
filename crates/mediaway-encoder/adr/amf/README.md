# `mediaway-encoder::amf` — ADRs

Module inside the `mediaway-encoder` crate (ADR-0021 `#[cfg]`-gated backend, `x86_64`
Linux only) — **not** a separate crate. As of ADR-0002, no `Cargo.toml`/`src/amf/` exists
yet either: ADR-0002 is a design + go-ahead decision, the module scaffold is the follow-up
implementation PR.

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-amf-deferred-no-hardware.md) | AMD AMF (`shiguredo_amf`) vendor encode — deferred, no implementation this stage | Accepted (defer) — superseded/amended by 0002 |
| [0002](0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) | AMD AMF (`shiguredo_amf`) vendor encode — proceed, module + design (still zero real-hardware verification) | Accepted |

Template: [`mediaway-encoder/adr/template.md`](../template.md) when the implementation PR needs
its own follow-up ADR.

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../../docs/adr/).
