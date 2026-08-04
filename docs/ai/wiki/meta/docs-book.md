# mdBook user guide (`docs/book/`)

Rendered user-facing documentation site, deployed to GitHub Pages. Separate
from `docs/spec/`/`docs/conventions/`/wiki (agent SSOT) and separate from
root `README.md` (GitHub landing page) — `docs/book/` is where narrative
getting-started/guide content lives, per
[`docs/conventions/docs-layout.md`](../../../conventions/docs-layout.md).

## Layout

```
docs/book/
├── book.toml          # mdbook config, site-url = "/mediaway/"
├── theme/
│   └── favicon.svg     # copy of docs/assets/mediaway-logo.svg
└── src/
    ├── SUMMARY.md      # table of contents
    ├── introduction.md, getting-started/, project/
    ├── guides/         # hand-written tutorials, one per examples/ sector
    └── reference/      # thin pages that {{#include}} README tables
```

`theme/favicon.svg` overrides mdBook's built-in default icon site-wide (every
page's `<link rel="icon">`, browser tab). It's a plain copy of
`docs/assets/mediaway-logo.svg` (the same logo used in the README) — if that
logo changes, re-copy it here. Only the SVG is overridden; mdBook drops the
PNG fallback link entirely once a custom `favicon.svg` is present, so no
`favicon.png` is needed.

Build: `cd docs/book && mdbook build` (output `docs/book/book/`, gitignored).
Local install: `cargo install mdbook`.

## Reference pages pull from README, not a copy

`reference/codec-support.md` etc. use mdBook's anchor include:
`{{#include ../../../../README.md:codec-support}}`. The README has matching
`<!-- ANCHOR: codec-support -->` / `<!-- ANCHOR_END: codec-support -->`
comments around its Codec support, Container support, Device capture, and
Crates sections. **Moving or renaming one of those sections in the README
must keep its anchor comments** — the book silently loses that content
otherwise (mdbook errors on a missing anchor at build time, so this fails
loud, not silent, if you drop the marker entirely — but a moved/renamed
section without matching anchors just needs the anchors re-added).

## Guides are NOT `{{#include}}`d from `examples/`

Deliberate choice: `guides/*.md` contain hand-written, annotated code
snippets (fenced ` ```rust,ignore ` blocks) that teach the shape of an API,
not literal includes of the compiling example files. `examples/*.rs` stays
the single source of truth for what actually compiles and runs; the book's
job is pedagogy, and a snippet trimmed/annotated for a reader often isn't
the same shape as a real runnable file (error handling, full imports,
CLI-arg parsing). Each guide links to its matching `examples/<sector>/*.rs`
file on GitHub for the reader to run themselves.

## CI deployment

`.github/workflows/docs.yml` builds the book and deploys via
`actions/upload-pages-artifact` + `actions/deploy-pages` on push to `main`
(paths: `docs/book/**`, `README.md`). Requires GitHub Pages to be enabled in
repo Settings → Pages → Source: **GitHub Actions** — this is a one-time
manual repo setting, not something the workflow can turn on itself.
