// The landing's stage (site/stage.js): a canvas script on a static page with no build step. It
// writes no HTML, so only the scope rule applies - a stage that throws leaves the page's still
// picture, and an unresolved name is the likeliest way for it to throw. Run from the repository's
// root (`npm run lint:stage`), since ESLint lints nothing outside the directory it starts from.
export default [
  {
    files: ["site/stage.js"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
      globals: {
        cancelAnimationFrame: "readonly", document: "readonly", matchMedia: "readonly",
        performance: "readonly", requestAnimationFrame: "readonly", ResizeObserver: "readonly",
        window: "readonly",
      },
    },
    rules: { "no-undef": "error" },
  },
];
