# Frontend baseline, 2026-09-29

Taken on `perf-bench` at `cbe4e98` (the `perf-round` base, no performance changes yet), with
`cd frontend && npm ci && npm run build` (Node 26.10, npm 12.1, Vite 8.3, Svelte 5).

## Built assets (`frontend/dist`)

Sizes in bytes. Gzip is `gzip -9` of each file, close to what the server's gzip layer sends
(Vite's own report says 113.98 kB for the main bundle at its default level).

| file | raw | gzip -9 |
|---|---:|---:|
| assets/index-C4UwrxG2.js (the whole app, one chunk) | 374 725 | 112 746 |
| assets/index-7vy1Y8SC.css | 31 352 | 6 553 |
| assets/workbox-window.prod.es5-Bd17z0YL.js | 5 653 | 2 214 |
| workbox-8b1f5376.js | 23 175 | 7 788 |
| sw.js | 2 623 | 1 298 |
| push-sw.js | 1 695 | 825 |
| index.html | 1 015 | 529 |
| manifest.webmanifest | 512 | 267 |
| icon.svg | 433 | 265 |
| apple-touch-icon.png | 7 975 | 7 987 |
| pwa-192.png | 8 142 | 8 140 |
| pwa-512.png | 24 351 | 23 589 |
| pwa-512-maskable.png | 6 559 | 6 346 |
| **total** | **488 210** | **178 567** |

What a first visit to the dashboard downloads before it can render: `index.html` + the JS
chunk + the CSS = 407 092 bytes raw, 119 828 gzip. The service worker precaches 10 entries,
404.73 KiB. There is one JS chunk: every route and both locales are in it.

## Requests before the dashboard shows data

Read from the code (`stores/session.ts` `doLoadSession`, `routes/Dashboard.svelte` `load`),
signed-in user, cold start, nothing answered by the service worker:

1. `GET /` (index.html)
2. `GET /assets/index-*.js` and `/assets/index-*.css`, in parallel
3. `GET /api/auth/status`
4. `GET /api/auth/me` -- the user is adopted here and the dashboard mounts
   (`GET /api/settings` follows, but nothing waits for it)
5. IndexedDB `pendingObjects()`, then `GET /api/objects?all=true&archived=false` and
   `...&archived=true`, in parallel
6. `GET /api/reminders/due?within_days=30`, only after both object lists have answered

So **six sequential network round trips** (four of them API calls) plus one IndexedDB read
before the dashboard has both its objects and its reminders.
