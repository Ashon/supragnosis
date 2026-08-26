// Security lint for the viewer script. no-unsanitized flags any innerHTML / insertAdjacentHTML /
// document.write sink whose value is not a plain literal, catching the exact XSS class that once lived
// in the inline HTML string (an entity/type name from an untrusted observe reaching innerHTML raw).
// The build does not use this - the crate embeds viewer.js via include_str!; this is dev/CI tooling.
import nounsanitized from "eslint-plugin-no-unsanitized";

export default [
  {
    files: ["viewer.js"],
    plugins: { "no-unsanitized": nounsanitized },
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
      // Listed rather than pulled from the `globals` package, which is one dependency this tooling
      // does not otherwise need. Exactly what the viewer uses: adding a browser API here is a
      // deliberate line, and anything NOT here that fails to resolve is the mistake being hunted.
      globals: {
        addEventListener: "readonly", document: "readonly", EventSource: "readonly",
        fetch: "readonly", history: "readonly", innerHeight: "readonly",
        innerWidth: "readonly", localStorage: "readonly", location: "readonly",
        matchMedia: "readonly", performance: "readonly", removeEventListener: "readonly",
        requestAnimationFrame: "readonly", setInterval: "readonly", setTimeout: "readonly",
        URL: "readonly", URLSearchParams: "readonly", window: "readonly",
      },
    },
    rules: {
      "no-unsanitized/method": "error",
      "no-unsanitized/property": "error",
      // A canvas render function that throws takes the whole frame with it: the graph vanishes and
      // only whatever drew before the throw remains on screen. That happened - a new hull layer
      // reached for `hullLabels`, a local of `draw()`, and nodes, edges and labels all stopped
      // drawing while the outlines stayed. Nothing here caught it, because this file was security
      // rules only. The declared globals below are what a browser script legitimately has; anything
      // else unresolved is a scope mistake, and this is the cheapest place to find one.
      "no-undef": "error",
    },
  },
];
