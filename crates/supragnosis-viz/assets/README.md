# viewer frontend assets

The ontology viewer's frontend, split out of the Rust source into real files:

- `viewer.html` - markup (links the two assets below, same origin)
- `viewer.css` - styles
- `viewer.js` - the canvas graph explorer + side panels

## How it ships

`build.rs` copies all three into `OUT_DIR` and `crates/supragnosis-viz/src/lib.rs` embeds them from
there via `include_str!`, serving them at `/`, `/viewer.css`, `/viewer.js`. So the crate is still a
single self-contained binary that works offline (no CDN, no external fetch), and **the build stays
pure cargo - Node is not required to build or release** (the minifiers are Rust build-dependencies).

Minification happens at build time, **release builds only** (debug serves the files verbatim, so they
stay debuggable):

- CSS - `lightningcss`
- HTML - `minify-html` (markup whitespace; the external css/js links are untouched)
- JS - `oxc` code generator in minify mode: whitespace + comments removed, but **no identifier
  mangling and no dead-code elimination** (semantics-preserving printing). Parsing also fails the
  build on malformed JS, a free correctness check.

Edit the readable sources here; the minified output is derived and never committed.

## Dev tooling (optional, Node only)

The tooling here exists so the frontend gets what a raw Rust string could not: editor support and a
security lint. It is not part of the cargo build.

```sh
npm ci        # or: npm install
npm run lint  # ESLint with eslint-plugin-no-unsanitized
```

`no-unsanitized` flags every `innerHTML` / `insertAdjacentHTML` / `document.write` sink whose value is
not a plain literal - the exact XSS class (Principle 18) that once lived unnoticed in the inline HTML
string. The one other value it accepts is an `html` tagged template (`viewer.js`, beside `esc()`):
the tag escapes every interpolation unless it is itself markup built with `html`, so text that never
went through the tag cannot reach the DOM unescaped. There are no disable comments and no helper that
vouches for an arbitrary string. A fragment someone forgets to build with `html` shows up on screen as
visible tags rather than as markup. CI runs this on every change under this directory
(`.github/workflows/frontend-lint.yml`).

All untrusted content (entity/type names, etc. - they arrive via `observe`, including federation sync)
reaches HTML through `html`, never through string concatenation into a sink.
