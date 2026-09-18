//! Options for [`super::WindowsWindowCapture::open_with`] — compiled on every target so the
//! Windows API reads the same off Windows.

/// Per-session WGC options that have no meaning outside Windows window capture, hence not
/// fields on the cross-platform [`crate::desktop::DesktopVideoCaptureConfig`].
///
/// Built from [`Default`] and then assigned, since new options may be added:
///
/// ```
/// use mediaway_device::windows_desktop::{CaptureBorder, FrameDimensions, WindowCaptureOptions};
///
/// let mut options = WindowCaptureOptions::default();
/// options.border = CaptureBorder::Hidden;
/// options.dimensions = FrameDimensions::EvenCropped;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct WindowCaptureOptions {
    /// Whether Windows draws its capture border. Defaults to [`CaptureBorder::Shown`].
    pub border: CaptureBorder,
    /// How frame dimensions relate to the window's. Defaults to [`FrameDimensions::Native`].
    pub dimensions: FrameDimensions,
}

/// Whether Windows draws its yellow capture border around the window being captured.
///
/// The border is drawn by the compositor **on screen**, over the captured window. It never
/// appears in captured frames, so, unlike [`crate::desktop::CursorCapture`], this does not
/// change what gets recorded, only what the person in front of the window sees while it is
/// recorded.
///
/// Windows-only, because only WGC draws one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CaptureBorder {
    /// Leave the OS default: the border is shown.
    #[default]
    Shown,
    /// Ask the OS not to draw it. Needs Windows 11 (build 22000+) and borderless-capture
    /// access, which the OS may refuse. Check [`super::WindowsWindowCapture::border_hidden`]
    /// for what actually happened. Capture proceeds either way, because the border does not
    /// affect the frames.
    Hidden,
}

/// How captured frame dimensions relate to the window's.
///
/// # Why `EvenCropped` exists
///
/// Every hardware video encoder rejects an odd width or height: 4:2:0 chroma is subsampled
/// 2×2, and there is no half chroma sample. Windows are resized to arbitrary sizes, so an
/// encoder fed straight from WGC fails on roughly three windows in four — measured on a live
/// Unity editor at 1137×636 on 2026-09-19, where no hardware HEVC or H.264 encoder would open.
///
/// # Why it is free
///
/// WGC **crops** a frame to its frame pool rather than scaling it. Measured 2026-09-19 against a
/// 2560×1392 window: with the pool at 2459×1341 the texture came back 2459×1341, and all
/// 3 297 519 overlapping pixels were identical to the full-size capture's top-left region.
/// So sizing the pool one pixel smaller on an odd axis drops that column or row with no copy
/// and no shader: the compositor simply writes less.
///
/// Cropping rather than padding is deliberate. A padded frame contains pixels the source never
/// drew, which in a recording used as evidence can be mistaken for a rendering bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FrameDimensions {
    /// Frames are exactly the window's size, odd or not.
    #[default]
    Native,
    /// Each odd axis loses its last column or row, so frames are always even-sized and any
    /// hardware encoder accepts them. Top-left anchored. Costs nothing (see type docs).
    EvenCropped,
}

impl FrameDimensions {
    /// The frame-pool size to request for a window whose content is `width`×`height`.
    ///
    /// `None` when the result would have a zero axis — a window 1 pixel wide cannot be
    /// even-cropped into anything.
    #[must_use]
    pub const fn pool_size(self, width: u32, height: u32) -> Option<(u32, u32)> {
        let (w, h) = match self {
            Self::Native => (width, height),
            Self::EvenCropped => (width & !1, height & !1),
        };
        if w == 0 || h == 0 { None } else { Some((w, h)) }
    }
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
