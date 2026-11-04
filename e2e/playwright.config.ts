import { defineConfig, devices } from '@playwright/test';

// Runs inside the container (scripts/e2e.sh). Each test starts its own server,
// so nothing here names a base URL: see fixtures.ts.
export default defineConfig({
  testDir: './specs',
  globalSetup: './global-setup.ts',
  outputDir: '/out/results',
  // A second chance would hide exactly what these tests are here to catch.
  retries: 0,
  fullyParallel: true,
  workers: Number(process.env.E2E_WORKERS) || 3,
  forbidOnly: true,
  timeout: 30_000,
  expect: { timeout: 7_000 },
  reporter: [['list'], ['html', { outputFolder: '/out/report', open: 'never' }]],
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    timezoneId: 'America/Sao_Paulo',
    viewport: { width: 1400, height: 900 },
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1400, height: 900 } } },
    // Close to Safari, and not Safari: the check CLAUDE.md asks for still stands.
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1400, height: 900 } } },
  ],
});
