// The browser scenarios (docs/e2e.md Section 4.2). The system's Chrome, so nothing is downloaded;
// traces and screenshots are kept only when a scenario fails, as evidence (J4); the JSON report is
// what the scenario check reads (Section 7).
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  fullyParallel: true,
  reporter: [["list"], ["json", { outputFile: "results/report.json" }]],
  outputDir: "results/artifacts",
  use: {
    channel: "chrome",
    viewport: { width: 1440, height: 900 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
});
