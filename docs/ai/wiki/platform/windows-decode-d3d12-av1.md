# D3D12 native AV1 decode — implemented, sans-io-verified only (ADR-0005)

- Module: `mediaway-decoder::windows::d3d12_video_decode` (still unregistered — neither
  H.264, HEVC, nor AV1 is wired into `WindowsVideoDecoder` yet). ADR: [0005](../../../../crates/mediaway-decoder/adr/windows/0005-d3d12-av1-key-frame-decode.md).
- **Implemented this pass — `cargo check`/`clippy --all-targets -- -D warnings`/`fmt --check`
  all clean, 43 new sans-io unit tests pass. Zero real GPU hardware verification,
  deliberately** — the new hardware-gated integration test
  (`d3d12_video_decode_av1_tests.rs`) is written and compiles but was **never run**. Do not
  run it, and do not run the existing H.264 or HEVC D3D12 decode hardware tests either, as
  a side effect of anything touching this module — see [windows-decode](windows-decode.md)'s
  D3D12 section: that path has caused **8 confirmed `DXGI_ERROR_DEVICE_HUNG` TDRs**, root
  cause still unresolved.

## Scope implemented

`KEY_FRAME`-only (`frame_type == 0`, `show_frame == 1`, `show_existing_frame == 0`), Main
profile (`seq_profile == 0`), 8-bit 4:2:0 NV12, single-tile. No reference-frame use of any
kind — **no `av1_refs.rs`/POC module exists at all**: `DXVA_PicParams_AV1`'s `frame_refs[7]`/
`RefFrameMapTextureIndex[8]` are always the trivial all-`0xFF` state (two independent
same-driver findings agree on this sentinel: this crate's own D3D12 AV1 *encoder*'s
`AV1_INVALID_DPB_RESOURCE_INDEX == 0xFF`, and `DXVA_PicEntry_AV1::Index`'s own documented
`0xFF` "unused" convention).

## Real, deliberate scope narrowing beyond ADR-0005's own literal text

Mirrors HEVC's own CRA-rejection precedent (ADR-0004) — narrowed further than the ADR's
written reject list for tractability, not incidental laziness, all documented in-module:

- `timing_info_present_flag == 1`, `initial_display_delay_present_flag == 1`,
  `operating_points_cnt_minus_1 != 0`, and `frame_id_numbers_present_flag == 1` are all
  rejected — none gates any `DXVA_PicParams_AV1` field, and this crate's own AV1 encoder
  never sets them.
- `tile_info()` supports `uniform_tile_spacing_flag == 1` only — explicit non-uniform
  per-tile widths/heights are rejected; meaningless for a genuinely single-tile stream.
- Only the `OBU_FRAME` shape (combined `frame_header_obu()` + `tile_group_obu()`) is
  decoded — a standalone `OBU_FRAME_HEADER`/`OBU_TILE_GROUP` is rejected. The only shape
  this crate's own AV1 encoder emits.

Every other frame-header/sequence-header field this scope does not explicitly reject
(`disable_cdf_update`, `frame_size_override_flag`, quantizer/loop-filter values,
`reduced_tx_set`, `order_hint`) is parsed as a **real, bitstream-derived value**, not
hardcoded — a real conformant stream could legally vary these within scope.

## `DXVA_PicParams_AV1`/`DXVA_PicEntry_AV1`/`DXVA_Tile_AV1` — primary-source ground truth

Absent from the vendored `windows-0.62.2` bindings (same situation as H.264/HEVC); the
decode **profile GUIDs** (`D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0`/etc.) are present —
confirmed directly in the vendored source this pass. Fetched the full struct definitions
(including the `cdef`/`segmentation`/`film_grain` sub-structs) directly from Microsoft's
own official Windows Driver DDI reference this implementation pass — a **primary** source,
stronger footing than H.264/HEVC's own Wine-mirror ground-truthing. Real structural
difference from H.264/HEVC: **no separate qmatrix DXVA argument** — `qm_y`/`qm_u`/`qm_v`
are plain scalar fields inline in `quantization`, so `av1_ops.rs` builds only two
`D3D12_VIDEO_DECODE_FRAME_ARGUMENT` entries, not three.

## Implementation shape: additive-only, zero edits to H.264/HEVC files

`dpb.rs`/`setup.rs`/`util.rs` reused unchanged (third codec now confirming this reuse).
`mediaway_sw::h264::BitReader`'s `read_bit`/`read_bits` reused for AV1's own `f(n)` fixed-
width reads (**not** `read_ue`/`read_se` — AV1 has no Exp-Golomb codes). New files:
`av1.rs` (open-time support query, mirrors `hevc.rs`), `av1_obu.rs` (`leb128()`/
`obu_header()` read-side + `split_obus` — the read-side mirror of
`mediaway-encoder-windows`'s `bitstream_av1.rs` write-side functions), `av1_sequence_header.rs`
/`av1_frame_header.rs` (parsing, cross-checked field-by-field against `bitstream_av1.rs`'s
own inference-rule comments), `av1_pic_params.rs`, `av1_decoder.rs`/`av1_ops.rs` (parallel
to `ops.rs`/`hevc_ops.rs`, real acknowledged duplication — same ADR-0004 precedent, not a
generified `Session<M>`).

## Test plan — executed

Sans-io unit tests for every new file, zero hardware: 43 tests across `av1_obu_tests.rs`/
`av1_sequence_header_tests.rs`/`av1_frame_header_tests.rs`/`av1_pic_params_tests.rs`, all
pass. `cargo check -p mediaway-decoder --all-features --all-targets`, `clippy
--all-targets --all-features -- -D warnings`, and `fmt --check` all clean.

The hardware-gated integration test (`d3d12_video_decode_av1_tests.rs`) is written and
compiles but **was not run**, per the safety constraint above. Uses
`mediaway-encoder-windows`'s **public** `WindowsVideoEncoder` with `CodecKind::Av1` (its
WMF AV1 encoder MFT path) as the planned bitstream source — same technique the HEVC
hardware test uses, one layer up (this crate's own D3D12 AV1 *encoder* is crate-private in
`mediaway-encoder-windows`, unreachable cross-crate without a visibility change).

**Open bitstream-source question for any future hardware attempt, doubly cautioned beyond
HEVC's own precedent**: `docs/standards/registry.toml`'s `av1-bitstream-spec` entry states
this crate family's own D3D12 AV1 *encoder* output is not confirmed decodable by
`libdav1d`. Even a future, separately-consented hardware attempt with this test may have no
valid input bitstream to chain from at all — resolving that (re-confirm the encoder's
current output, or source a bitstream from `mediaway-sw::av1`/`rav1e` instead) is the first
task before any real hardware attempt, independently of whether this decoder's own parsing
logic is otherwise ready.
