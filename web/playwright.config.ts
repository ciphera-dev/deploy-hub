import { defineConfig, devices } from "@playwright/test";

/**
 * Deux modes :
 * - par defaut : la SPA construite servie par `vite preview` (e2e/*.spec.ts,
 *   sans golden-path) ;
 * - `CIPHERA_E2E_URL` defini : parcours golden-path contre une instance
 *   reelle installee par install/tests/golden-path/setup.sh (job CI
 *   e2e-golden-path), panneau en certificat auto-signe.
 */
const instance = process.env.CIPHERA_E2E_URL;

export default instance
  ? defineConfig({
      testDir: "./e2e",
      testMatch: "golden-path.spec.ts",
      outputDir: "./test-results",
      reporter: [["list"], ["html", { outputFolder: "playwright-report", open: "never" }]],
      workers: 1,
      retries: 0,
      use: { baseURL: instance, ignoreHTTPSErrors: true, trace: "retain-on-failure", screenshot: "only-on-failure", video: "retain-on-failure" },
      projects: [{ name: "golden-path", use: { ...devices["Desktop Chrome"] } }],
    })
  : defineConfig({
      testDir: "./e2e",
      testIgnore: "golden-path.spec.ts",
      outputDir: "./test-results",
      reporter: [["html", { outputFolder: "playwright-report", open: "never" }]],
      use: { baseURL: "http://localhost:4173" },
      projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
      webServer: {
        command: "npm run build && npx vite preview --port 4173 --strictPort",
        url: "http://localhost:4173",
        reuseExistingServer: true,
        timeout: 120_000,
      },
    });
