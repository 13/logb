import { defineConfig, devices } from '@playwright/test';

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
    command: 'rm -rf ../.e2e-data && ../target/debug/memto',
    // Every spec signs in, all of them from 127.0.0.1 and all within one 60-second login
    // window, so the whole suite counts as a single client against the per-IP login rate
    // limit. At the default of 10 the suite was one test away from locking itself out, which
    // shows up as a sign-in that simply never navigates.
    env: {
      MEMTO_DATA_DIR: '../.e2e-data', MEMTO_PORT: '8099', MEMTO_BIND: '127.0.0.1', MEMTO_LOG: 'warn',
      MEMTO_LOGIN_MAX_ATTEMPTS: '1000',
    },
    port: 8099,
    reuseExistingServer: false,
  },
});
