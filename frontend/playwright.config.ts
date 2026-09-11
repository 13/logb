import { defineConfig, devices } from '@playwright/test';

/**
 * Every spec must pass on its own (`npx playwright test 03-search`), not only as part of a full
 * run. One server and one database are shared across the whole run, so it is easy to write a
 * spec that quietly depends on data an earlier one left behind -- and then a `-g` run to
 * investigate a failure fails for an unrelated reason, which is exactly when that is least
 * welcome. Seed what the spec needs, and use names distinctive enough that a query cannot
 * match another spec's rows in the shared database.
 */
export default defineConfig({
  testDir: 'tests-e2e',
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: { baseURL: 'http://127.0.0.1:8099' },
  projects: [{ name: 'mobile', use: { ...devices['Pixel 7'] } }],
  webServer: {
    // The scratch data directory is wiped by the server command itself: this config is
    // re-imported by every worker, so a module-scope rmSync would delete the database
    // out from under the running server.
    command: 'rm -rf ../.e2e-data && ../target/debug/logb',
    // Every spec signs in, all of them from 127.0.0.1 and all within one 60-second login
    // window, so the whole suite counts as a single client against the per-IP login rate
    // limit. At the default of 10 the suite was one test away from locking itself out, which
    // shows up as a sign-in that simply never navigates.
    env: {
      LOGB_DATA_DIR: '../.e2e-data', LOGB_PORT: '8099', LOGB_BIND: '127.0.0.1', LOGB_LOG: 'warn',
      LOGB_LOGIN_MAX_ATTEMPTS: '1000',
    },
    port: 8099,
    reuseExistingServer: false,
  },
});
