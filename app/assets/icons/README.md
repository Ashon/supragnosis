# Icons

The app's icons are [Lucide](https://lucide.dev) (`lucide-static` 1.52.0, ISC - see `LICENSE`),
copied here unmodified: the app works offline and its pages' policies load nothing remote, so the
icons it uses are files beside its pages rather than a package or a CDN.

Pages draw them as CSS masks over `currentColor` (the `.i` classes in `shell.css` and
`settings.css`), so an icon takes the color of the text around it. To add one, copy its SVG from
`lucide-static/icons/` at the same version and add its class.
