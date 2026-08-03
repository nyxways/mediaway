"""Screen capture quick start.

STATUS: NOT AVAILABLE — the C ABI rejects screen capture today: it needs a
live GPU device handle (`ID3D11Device*`) with no CPU fallback, and its C
representation is deferred (see `bindings/README.md`'s capability truth
table). `VideoCapture.open(source="screen")` therefore raises
CaptureUnsupportedError. This is the capture-only analog of
`pipeline/screen_record.py`: it demonstrates the honest gap and exits
gracefully.
"""

from mediaway import CaptureUnsupportedError, Rational, VideoCapture


def main() -> None:
    try:
        capture = VideoCapture.open(source="screen", index=0, frame_rate=Rational(1, 30))
    except CaptureUnsupportedError as err:
        print("Screen capture is NOT available from this binding today:")
        print("  it needs a live GPU device handle (ID3D11Device*) with no")
        print("  CPU fallback, and its C representation is deferred.")
        print(f"  ({err})")
        return

    # Unreachable today — the ABI changed if we get here.
    capture.close()
    print("unexpected: screen capture opened — the ABI changed")


if __name__ == "__main__":
    main()
