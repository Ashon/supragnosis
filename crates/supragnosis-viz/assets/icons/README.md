# Icons

The viewer's icons are [Lucide](https://lucide.dev) (`lucide-static` 1.52.0, ISC - see `LICENSE`),
copied here unmodified, the same set and release as the desktop app's (`app/assets/icons/`).

They are compiled into the binary, as the faces are, and served at
`/icons/lucide-1.52.0/<name>.svg` on the local console. The page draws
them as CSS masks over `currentColor` (the `.i` classes in `viewer.css`), so an icon takes the color of
the text around it (docs/design-system.md Section 28).

To add one, copy its SVG from `lucide-static/icons/` at the same version and add its class. A file
that the app also ships must be the same bytes in both places; `tests/icons.rs` reads both.
