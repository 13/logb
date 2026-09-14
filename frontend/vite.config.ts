/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';
import { readFileSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { pwaIcons } from './scripts/pwa-icons.ts';
import { files, householdData, neverCached, otherApi } from './src/lib/sw-routes.ts';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

/** The commit this bundle was built from, for Settings > About. `LOGB_BUILD_COMMIT` wins, for a
 *  build that runs without the repository (a container build copies the sources, not `.git`);
 *  otherwise git is asked, and a build with neither simply shows no commit. */
function buildCommit(): string {
  if (process.env.LOGB_BUILD_COMMIT) return process.env.LOGB_BUILD_COMMIT.slice(0, 12);
  try {
    return execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
  } catch {
    return '';
  }
}

export default defineConfig({
  plugins: [
    svelte(),
    VitePWA({
      registerType: 'autoUpdate',
      manifest: {
        name: 'LogB',
        short_name: 'LogB',
        description: 'Complete history of your owned objects',
        theme_color: '#1f6f5f',
        background_color: '#f7f7f5',
        display: 'standalone',
        start_url: '/',
        icons: pwaIcons,
      },
      workbox: {
        navigateFallback: '/index.html',
        // Push notifications: showing one, and opening the app when it is tapped. A separate
        // file pulled into the generated worker, so the rest of it stays generated.
        importScripts: ['push-sw.js'],
        navigateFallbackDenylist: [/^\/api\//],
        // Workbox only routes GETs, so writes always go straight to the network. Workbox tests
        // a RegExp urlPattern against the whole URL, not just the path, so plain `/^\/api\//`
        // patterns never matched here -- these function matchers check url.pathname instead.
        runtimeCaching: [
          // Session, administration, exports, sync and anything that must never be answered
          // from a cache: a stale /auth/me would show a signed-out user their old identity.
          { urlPattern: neverCached, handler: 'NetworkOnly', method: 'GET' },
          // Blobs are content-addressed and never change under a given id.
          {
            urlPattern: files,
            handler: 'CacheFirst',
            method: 'GET',
            options: {
              cacheName: 'logb-files',
              expiration: { maxEntries: 300, maxAgeSeconds: 60 * 60 * 24 * 30 },
              cacheableResponse: { statuses: [200] },
            },
          },
          // What a household reads with no connection: the network when it answers, the last
          // known response when it does not, so the dashboard and a timeline stay readable on
          // a dead connection.
          {
            urlPattern: householdData,
            handler: 'NetworkFirst',
            method: 'GET',
            options: {
              cacheName: 'logb-api',
              networkTimeoutSeconds: 4,
              expiration: { maxEntries: 200, maxAgeSeconds: 60 * 60 * 24 * 7 },
              cacheableResponse: { statuses: [200] },
            },
          },
          // Anything else under /api: network only, so a new endpoint is never cached by
          // accident.
          { urlPattern: otherApi, handler: 'NetworkOnly', method: 'GET' },
        ],
      },
    }),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    __BUILD_DATE__: JSON.stringify(new Date().toISOString()),
    __BUILD_COMMIT__: JSON.stringify(buildCommit()),
  },
  server: { proxy: { '/api': 'http://localhost:8080' } },
  test: { environment: 'node', include: ['tests/**/*.test.ts'] },
});
