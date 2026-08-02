# Rational

`mediaway-common::Rational` — `num / den` seconds. Integer fraction for easy mapping to `FFmpeg` `AVRational` and WebCodecs timescales.

```rust
pub struct Rational {
    pub num: u64,
    pub den: u32, // must be non-zero when constructed from untrusted input
}
```

- **Why:** avoids `f64` drift on 29.97 / 23.976 and other non-integer fps.
- **Non-goal (early):** full arithmetic helpers — document rules here when added.
- Crate: `crates/mediaway-common`
