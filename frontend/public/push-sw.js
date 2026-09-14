// The service worker's half of push notifications, pulled into the generated worker through
// `workbox.importScripts` in vite.config.ts. Plain JavaScript on purpose: it runs as-is inside
// the worker, outside the app's build.
//
// The server sends `{ title, body, url }`, encrypted to this browser (see src/notify.rs).

self.addEventListener('push', (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch {
    // A payload that is not JSON still deserves a notification rather than silence.
    data = { body: event.data ? event.data.text() : '' };
  }
  event.waitUntil(
    self.registration.showNotification(data.title || 'LogB', {
      body: data.body || '',
      icon: '/pwa-192.png',
      badge: '/pwa-192.png',
      // One digest a day replaces the one before it instead of stacking up.
      tag: 'logb-digest',
      data: { url: data.url || '/' },
    }),
  );
});

// A tap opens the app where the reminder is dealt with -- in the window already open, when
// there is one, rather than a second copy of the app.
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const url = new URL((event.notification.data && event.notification.data.url) || '/', self.location.origin).href;
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((windows) => {
      for (const client of windows) {
        if (new URL(client.url).origin === self.location.origin && 'focus' in client) {
          return client.focus().then((focused) => (focused.navigate ? focused.navigate(url) : focused));
        }
      }
      return self.clients.openWindow(url);
    }),
  );
});
