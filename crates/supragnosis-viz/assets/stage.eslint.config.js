// The landing's stage (site/stage.js) and controls (site/landing.js), the principles page's figures
// (site/principles.js) and the site's visit count (site/analytics.js): scripts on static pages with no build step. They write no
// HTML from outside the page - the principles page copies its own markup - so only the scope rule
// applies: a script that throws
// leaves the page's words, and an unresolved name is the likeliest way for it to throw. Run from the repository's
// root (`npm run lint:stage`), since ESLint lints nothing outside the directory it starts from.
export default [
  {
    files: ["site/stage.js", "site/landing.js", "site/principles.js", "site/analytics.js"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
      globals: {
        cancelAnimationFrame: "readonly", clearTimeout: "readonly", document: "readonly", getSelection: "readonly",
        IntersectionObserver: "readonly", location: "readonly", matchMedia: "readonly", navigator: "readonly", Path2D: "readonly", performance: "readonly",
        requestAnimationFrame: "readonly", ResizeObserver: "readonly", setTimeout: "readonly",
        window: "readonly",
      },
    },
    rules: { "no-undef": "error" },
  },
];
