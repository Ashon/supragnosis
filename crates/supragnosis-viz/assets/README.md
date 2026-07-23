# viewer frontend assets

The ontology viewer's frontend, split out of the Rust source into real files:

- `viewer.html` - markup (links the two assets below, same origin)
- `viewer.css` - styles
- `viewer.js` - the canvas graph explorer + side panels

## How it ships

`crates/supragnosis-viz/src/lib.rs` embeds all three at compile time via `include_str!` and serves
them at `/`, `/viewer.css`, `/viewer.js`. So the crate is still a single self-contained binary that
works offline (no CDN, no external fetch), and **the build stays pure cargo - Node is not required to
build or release**.

## Dev tooling (optional, Node only)

The tooling here exists so the frontend gets what a raw Rust string could not: editor support and a
security lint. It is not part of the cargo build.

```sh
npm ci        # or: npm install
npm run lint  # ESLint with eslint-plugin-no-unsanitized
```

`no-unsanitized` flags every `innerHTML` / `insertAdjacentHTML` / `document.write` sink whose value is
not a plain literal - the exact XSS class (Principle 18) that once lived unnoticed in the inline HTML
string. Each vetted sink carries an explicit
`// eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings`,
so the full set of HTML sinks is greppable and any NEW sink fails the lint until it is escaped and
consciously acknowledged. CI runs this on every change under this directory (`.github/workflows/frontend-lint.yml`).

All untrusted content (entity/type names, etc. - they arrive via `observe`, including federation sync)
must go through `esc()` before it reaches HTML.
