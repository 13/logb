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
    env: { MEMTO_DATA_DIR: '../.e2e-data', MEMTO_PORT: '8099', MEMTO_BIND: '127.0.0.1', MEMTO_LOG: 'warn' },
    port: 8099,
    reuseExistingServer: false,
  },
});
