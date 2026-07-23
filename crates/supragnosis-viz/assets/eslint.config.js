// Security lint for the viewer script. no-unsanitized flags any innerHTML / insertAdjacentHTML /
// document.write sink whose value is not a plain literal, catching the exact XSS class that once lived
// in the inline HTML string (an entity/type name from an untrusted observe reaching innerHTML raw).
// The build does not use this - the crate embeds viewer.js via include_str!; this is dev/CI tooling.
import nounsanitized from "eslint-plugin-no-unsanitized";

export default [
  {
    files: ["viewer.js"],
    plugins: { "no-unsanitized": nounsanitized },
    languageOptions: { ecmaVersion: 2022, sourceType: "script" },
    rules: {
      "no-unsanitized/method": "error",
      "no-unsanitized/property": "error",
    },
  },
];
