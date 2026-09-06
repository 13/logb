/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';
import { readFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

export default defineConfig({
  plugins: [
    svelte(),
    VitePWA({
      registerType: 'autoUpdate',
      manifest: {
        name: 'memto',
        short_name: 'memto',
        description: 'Complete history of your owned objects',
        theme_color: '#1f6f5f',
        background_color: '#f7f7f5',
        display: 'standalone',
        start_url: '/',
        icons: [
          { src: 'icon.svg', sizes: 'any', type: 'image/svg+xml', purpose: 'any' },
          { src: 'pwa-192.png', sizes: '192x192', type: 'image/png' },
          { src: 'pwa-512.png', sizes: '512x512', type: 'image/png', purpose: 'any maskable' },
        ],
      },
      workbox: {
        navigateFallback: '/index.html',
        navigateFallbackDenylist: [/^\/api\//],
        // Workbox only routes GETs, so writes always go straight to the network.
        runtimeCaching: [
          // Session state must never be answered from a cache: a stale /auth/me would show a
          // signed-out user their old identity.
          { urlPattern: /^\/api\/auth\//, handler: 'NetworkOnly' },
          // Exports are large, one-shot downloads.
          { urlPattern: /^\/api\/(export|import)/, handler: 'NetworkOnly' },
          // Blobs are content-addressed and never change under a given id.
          {
            urlPattern: /^\/api\/files\//,
            handler: 'CacheFirst',
            options: {
              cacheName: 'memto-files',
              expiration: { maxEntries: 300, maxAgeSeconds: 60 * 60 * 24 * 30 },
              cacheableResponse: { statuses: [200] },
            },
          },
          // Everything else: the network when it answers, the last known response when it
          // does not, so the dashboard and a timeline stay readable on a dead connection.
          {
            urlPattern: /^\/api\//,
            handler: 'NetworkFirst',
            options: {
              cacheName: 'memto-api',
              networkTimeoutSeconds: 4,
              expiration: { maxEntries: 200, maxAgeSeconds: 60 * 60 * 24 * 7 },
              cacheableResponse: { statuses: [200] },
            },
          },
        ],
      },
    }),
  ],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  server: { proxy: { '/api': 'http://localhost:8080' } },
  test: { environment: 'node', include: ['tests/**/*.test.ts'] },
});
