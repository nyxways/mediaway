#!/usr/bin/env bash
# Block staging of test/binary media fixtures. Generate via mediaway-test-media instead.
# docs/conventions/testing.md
set -euo pipefail

# Common media / raw frame extensions used as fixtures
EXT_RE='\.(mp4|webm|mkv|mov|m4v|avi|wav|mp3|aac|flac|ogg|opus|png|jpe?g|gif|bmp|tiff?|webp|yuv|rgba|bgra|nv12|p010|pcm|raw)$'

EXIT=0

while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    # Normalize for matching
    lower=$(printf '%s' "$file" | tr '[:upper:]' '[:lower:]')
    # Package-icon exception: NuGet/npm/UPM package icons are real, small, non-test
    # binary assets under a package's own source tree (e.g. bindings/csharp/src/icon.png),
    # not a test fixture — SVG (this repo's other logo assets) isn't accepted by NuGet's
    # PackageIcon, so a raster icon.png is unavoidable here.
    if [[ "$lower" =~ (^|/)icon\.png$ ]]; then
        continue
    fi
    if [[ "$lower" =~ $EXT_RE ]]; then
        echo "❌ Staged media/binary fixture is forbidden: $file" >&2
        echo "   Generate with mediaway-test-media into local/.cache/ (gitignored)." >&2
        echo "   See docs/conventions/testing.md" >&2
        EXIT=1
    fi
    # Also block anything under a committed testdata/media tree if someone recreates it
    if [[ "$lower" =~ (^|/)(testdata|fixtures|test-media|test_media)/ ]] && [[ "$lower" =~ $EXT_RE ]]; then
        EXIT=1
    fi
done < <(git diff --cached --name-only --diff-filter=AM)

exit $EXIT
