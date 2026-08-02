# Support matrix (pointer)

Human-facing codec tables live in the root [`README.md`](../../../../README.md#codec-support):

- **OS · CPU** / **OS · GPU** — native codec APIs by input class
- **GPU · by API** — graphics-API path (`GraphicsApi`)
- **GPU · by vendor** — direct HW codec SDKs (`VendorHw`); not a filter on API
- **CPU/SW** — pure Rust sans-io (`mediaway-sw`)

Selection model: [backend-preference](../encode/backend-preference.md).

Marks: `✅` · `⚡` · `🆗` · `🚧` · `🛠️` (planned) · `👻` (not planned).  
`⚡` = Zero-Copy (GPU-resident **or** shared CPU; no payload memcpy). Cell = encode/decode; `A/B` when they differ.

Update that README section when a cell’s mark changes (same PR as the code).
