import { defineConfig, devices } from '@playwright/test';

/**
 * Every spec must pass on its own (`npx playwright test 03-search`), not only as part of a full
 * run. Each project (mobile, desktop) gets its own server and its own database, but within a
 * project the suite still runs against one shared server and one shared database, so it is easy
 * to write a spec that quietly depends on data an earlier one left behind -- and then a `-g` run
 * to investigate a failure fails for an unrelated reason, which is exactly when that is least
 * welcome. Seed what the spec needs, and use names distinctive enough that a query cannot match
 * another spec's rows in the shared database.
 */
export default defineConfig({
  testDir: 'tests-e2e',
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  // A trace and a screenshot on failure only. This suite has had a failure that reproduces on
  // CI and not locally, and a one-line "waiting for locator" message says nothing about what
  // the page was actually showing at the time -- which is the only question worth answering.
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  // Two viewports, because a desktop layout that is never rendered in a test is a desktop
  // layout that regresses silently. `fullyParallel: false` and `workers: 1` still apply, so
  // the two projects run one after the other, each against its own server and database.
  projects: [
    { name: 'mobile', use: { ...devices['Pixel 7'], baseURL: 'http://127.0.0.1:8099' } },
    {
      name: 'desktop',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 }, baseURL: 'http://127.0.0.1:8100' },
    },
  ],
  // Playwright's `webServer` array has no per-project binding: both entries below start on
  // every invocation, including a filtered `npx playwright test --project=mobile` run -- there
  // is no mechanism that starts only the server a filtered run will use. That is harmless here
  // (different ports, no interference), but it means the mobile/desktop split lives entirely in
  // each project's own `baseURL` above, not in which of these two servers happens to be running.
  webServer: [
    {
      // The scratch data directory is wiped by the server command itself: this config is
      // re-imported by every worker, so a module-scope rmSync would delete the database
      // out from under the running server.
      command: 'rm -rf ../.e2e-data-mobile && ../target/debug/logb',
      // Every spec signs in, all of them from 127.0.0.1 and all within one 60-second login
      // window, so the whole suite counts as a single client against the per-IP login rate
      // limit. At the default of 10 the suite was one test away from locking itself out, which
      // shows up as a sign-in that simply never navigates.
      env: {
        LOGB_DATA_DIR: '../.e2e-data-mobile', LOGB_PORT: '8099', LOGB_BIND: '127.0.0.1', LOGB_LOG: 'warn',
        LOGB_LOGIN_MAX_ATTEMPTS: '1000',
      },
      port: 8099,
      reuseExistingServer: false,
    },
    {
      // Same rationale as the mobile server above, mirrored on its own port and data directory
      // so the two projects never share a database.
      command: 'rm -rf ../.e2e-data-desktop && ../target/debug/logb',
      env: {
        LOGB_DATA_DIR: '../.e2e-data-desktop', LOGB_PORT: '8100', LOGB_BIND: '127.0.0.1', LOGB_LOG: 'warn',
        LOGB_LOGIN_MAX_ATTEMPTS: '1000',
      },
      port: 8100,
      reuseExistingServer: false,
    },
  ],
});
