//! Sequential byte source — monomorphized, no `Box`.

#![forbid(unsafe_code)]

/// Sequential byte source over a slice.
#[derive(Debug, Clone, Copy)]
pub struct ByteSource<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteSource<'a> {
    /// Wrap a slice.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Take `n` bytes or `None` if truncated.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// Read big-endian `u32`.
    pub fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read big-endian `u64`.
    pub fn u64(&mut self) -> Option<u64> {
        let b = self.take(8)?;
        Some(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Read `u8`.
    pub fn u8(&mut self) -> Option<u8> {
        let b = self.take(1)?;
        Some(b[0])
    }
}
