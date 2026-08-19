# D3D12 native HEVC decode — implemented, sans-io-verified only (ADR-0004)

- Module: `mediaway-decoder::windows::d3d12_video_decode` (still unregistered — neither
  H.264 nor HEVC is wired into `WindowsVideoDecoder` yet). ADR: [0004](../../../../crates/mediaway-decoder/adr/windows/0004-d3d12-hevc-single-forward-ref-p-slice-decode.md).
- **Implemented this pass — `cargo check`/`clippy --all-targets -- -D warnings`/42 new
  sans-io unit tests all pass. Zero real GPU hardware verification, deliberately** — the
  new hardware-gated integration test (`d3d12_video_decode_hevc_tests.rs`) is written and
  compiles but was **never run**. Do not run it, and do not run the existing H.264 D3D12
  decode hardware test either, as a side effect of anything touching this module — see
  [windows-decode](windows-decode.md)'s D3D12 section: that path has caused **8 confirmed
  `DXGI_ERROR_DEVICE_HUNG` TDRs**, root cause still unresolved.

## Scope decided

Single-forward-reference P-slice (`RefPicList0[0]` only, `num_ref_idx_l0_active == 1`) +
I/IDR, Main profile, 8-bit 4:2:0 NV12, single-tile/no-WPP, no long-term refs, no
`ref_pic_list_modification`. Mirrors the *shape* of
[`mediaway-decoder-linux` ADR-0002](../../../../crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md)'s
H.264 single-forward-reference cut, applied to HEVC's RPS-based reference model instead of
`frame_num` sliding window. Narrower than ADR-0002's original "general GOP from the start"
ambition for HEVC — justified by there being **zero GPU-verified non-IDR HEVC decode
anywhere in this workspace** to build on (see below).

## Real finding: the task's assumed VA-API HEVC decode source doesn't exist on `main`

A prior session's memory referenced a "VA-API HEVC single-forward-reference P-slice decoder"
at `mediaway-decoder::linux::vaapi::hevc*` + `adr/linux/0003-vaapi-hevc-p-slice-dpb.md`.
**Checked directly — neither exists.** `linux/vaapi/mod.rs` only has `h264`; `adr/linux/`
only has `0001` (H.264 IDR) and `0002` (H.264 P-slice, not HEVC — title similarity likely
caused the cross-reference). Used `crate::vulkan::{decoder_hevc,hevc_params,hevc_slice}`
instead — real, hardware-**verified-for-IDR** HEVC decode in this same crate (see
[vulkan-decode](vulkan-decode.md)). Its own P-slice/RPS parsing (`hevc_slice.rs`) is
real and unit-tested, but **`decode_slice_hevc` has never wired a P-slice to a real GPU
decode call** — so this ADR's P-slice scope is genuinely new territory for HEVC on any
backend, not a known-working design being ported.

## DXVA_PicParams_HEVC / DXVA_Slice_HEVC_Short / DXVA_Qmatrix_HEVC — hand-defined, not in `windows` 0.62.2

Same situation ADR-0002 found for H.264: `D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN` (and
Main10/12/444/Monochrome variants) GUIDs are present in the vendored `windows-0.62.2`
source; the DXVA-spec structs themselves are absent entirely (grepped, zero matches) — must
be hand-defined `repr(C)`, ground-truthed against the Wine `dxva.h` mirror (fetched this
session). One real, notable structural difference from H.264: `DXVA_Slice_HEVC_Short` has
**no `BitOffsetToSliceData`-equivalent field at all** (just
`BSNALunitDataLocation`/`SliceBytesInBuffer`/`wBadSliceChopping`) — the accelerator re-parses
the *entire* slice-segment header itself, so H.264's whole `BitOffsetToSliceData`
raw-vs-RBSP saga (ADR-0002's Bug 3, fixed then found backwards) is structurally impossible
here.

**Two things flagged as unconfirmed, not asserted**: Microsoft Learn's rendered
`DXVA_PicParams_HEVC` page nests fields in a way that can't be the real compilable layout
(likely a docs-pipeline brace-loss bug) — the Wine mirror is trusted instead, but not
cross-checked against a third source (`dxva2_hevc.c`) this session. Whether
`RefPicSetStCurrBefore`/`After`/`LtCurr` hold byte-indices into `RefPicList[15]` or raw DPB
slot numbers is *believed*, not independently confirmed — both flagged as the ADR's first
implementation-time verification tasks.

## Implementation shape: additive-only, zero edits to the existing (broken) H.264 files

`dpb.rs`/`setup.rs`/`util.rs` confirmed already codec-generic — reused unchanged, no edits.
`ops.rs`/`Session`/`D3d12VideoDecoder` stayed concretely H.264-typed; rather than generifying
them, a **parallel** `hevc_ops.rs`/`hevc_decoder.rs` was added instead (real, acknowledged
duplication of `ops.rs::decode_frame`'s shape, retyped for `DxvaPicParamsHevc`/
`DxvaQmatrixHevc`/`DxvaSliceHevcShort`/`HevcRefMeta`) — implementing HEVC cannot silently
change H.264 behavior while H.264's own hang is unresolved. New files: `hevc.rs` (open-time
support query, ~40 lines), `hevc_vps_sps_pps.rs` (SPS/PPS + 2-byte NAL header parse — **no
VPS parsing and no `hevc_ptl.rs` submodule**: `DXVA_PicParams_HEVC` has no profile/tier/
level field at all, so `profile_tier_level()`'s bits are skipped, not decoded — a genuine
simplification versus the Vulkan port), `hevc_slice.rs` (ported from
`crate::vulkan::hevc_slice`, extended with `num_ref_idx_l0_active == 1` +
`NumPicTotalCurr == 1` scope-check rejections the Vulkan source never needed), `hevc_poc.rs`
(genuinely new — no POC MSB-cycle tracking existed anywhere in this workspace for HEVC),
`hevc_refs.rs` (RPS-application DPB eviction + `RefPicList`/`RefPicSetStCurrBefore`/`After`
construction, also new), `hevc_pic_params.rs`, `hevc_ops.rs`, `hevc_decoder.rs`.

## Test plan — executed

Sans-io unit tests for every new file, zero hardware: 42 tests across
`hevc_vps_sps_pps_tests.rs`/`hevc_slice_tests.rs`/`hevc_poc_tests.rs`/`hevc_refs_tests.rs`/
`hevc_pic_params_tests.rs`, all pass. `cargo check -p mediaway-decoder --all-features` and
`cargo clippy -p mediaway-decoder --all-features --all-targets -- -D warnings` both clean.

The hardware-gated integration test (`d3d12_video_decode_hevc_tests.rs`) is written and
compiles but **was not run**, per the safety constraint above. **Real bitstream source
differs from the ADR's original sketch**: the ADR planned reusing
`mediaway-encoder-windows`'s native D3D12 HEVC GOP encoder (`gop_hevc.rs`) — that module
turned out to be `mod d3d12_video_encode;`, crate-private and unregistered, exactly like
this crate's own decode module, so it cannot be reached cross-crate without first changing
`mediaway-encoder-windows`'s own visibility (out of scope here). The test instead uses
`mediaway-encoder-windows`'s **public** `WindowsVideoEncoder` with `CodecKind::Hevc` (its
WMF HEVC encoder MFT path), the same one layer up the H.264 hardware test already uses for
H.264. HEVC has no CAVLC/`I_PCM`-style escape (even a PCM coding unit's `pcm_flag` is
CABAC-coded, ITU-T H.265 § 9.3), so hand-writing a legal bitstream isn't a realistic
alternative either way.
