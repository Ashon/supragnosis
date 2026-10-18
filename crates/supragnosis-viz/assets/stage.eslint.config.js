// The landing's stage (site/stage.js) and the principles page's figures (site/principles.js): canvas
// scripts on static pages with no build step. They write no HTML from outside the page - the
// principles page copies its own markup - so only the scope rule applies: a script that throws
// leaves the page's words, and an unresolved name is the likeliest way for it to throw. Run from the repository's
// root (`npm run lint:stage`), since ESLint lints nothing outside the directory it starts from.
export default [
  {
    files: ["site/stage.js", "site/principles.js"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
      globals: {
        cancelAnimationFrame: "readonly", clearTimeout: "readonly", document: "readonly",
        IntersectionObserver: "readonly", matchMedia: "readonly", performance: "readonly",
        requestAnimationFrame: "readonly", ResizeObserver: "readonly", setTimeout: "readonly",
        window: "readonly",
      },
    },
    rules: { "no-undef": "error" },
  },
];
