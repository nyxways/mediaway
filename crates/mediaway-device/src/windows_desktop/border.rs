//! [`CaptureBorder`] — compiled on every target so the Windows API reads the same off Windows.

/// Whether Windows draws its yellow capture border around the window being captured.
///
/// The border is drawn by the compositor **on screen**, over the captured window. It never
/// appears in captured frames, so, unlike [`crate::desktop::CursorCapture`], this does not
/// change what gets recorded, only what the person in front of the window sees while it is
/// recorded.
///
/// Windows-only, because only WGC draws one. Hence an argument to
/// [`super::WindowsWindowCapture::open_with_border`] rather than a field on the cross-platform
/// [`crate::desktop::DesktopVideoCaptureConfig`].
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
