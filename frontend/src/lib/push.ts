import { api } from './api';

/**
 * Browser push, from this device's side: whether it can, whether it has, and turning it on and
 * off. The service worker's half -- showing the notification and opening the app on a tap --
 * is `public/push-sw.js`.
 */

export type PushState = 'unsupported' | 'denied' | 'off' | 'on';

/** `applicationServerKey` takes the raw bytes; the server hands out base64url. */
export function base64UrlToBytes(value: string): Uint8Array<ArrayBuffer> {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(value.length / 4) * 4, '=');
  const binary = atob(padded);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** Push needs a service worker, the Push API, and notifications. iOS Safari has all three only
 *  inside an app added to the home screen, so a plain tab there reports `unsupported`. */
export function pushSupported(): boolean {
  return typeof navigator !== 'undefined' && 'serviceWorker' in navigator
    && typeof window !== 'undefined' && 'PushManager' in window && 'Notification' in window;
}

async function registration(): Promise<ServiceWorkerRegistration | null> {
  if (!pushSupported()) return null;
  // `ready` never settles where no worker is registered (a dev server, a browser that refused
  // it), so it is raced against a timeout rather than awaited bare.
  return Promise.race([
    navigator.serviceWorker.ready,
    new Promise<null>((resolve) => setTimeout(() => resolve(null), 3000)),
  ]);
}

export async function pushState(): Promise<PushState> {
  if (!pushSupported()) return 'unsupported';
  if (Notification.permission === 'denied') return 'denied';
  const reg = await registration();
  if (!reg) return 'unsupported';
  return (await reg.pushManager.getSubscription()) ? 'on' : 'off';
}

/** Asks for permission, subscribes this browser and tells the server. Answers the state it
 *  ended in, so a refused permission prompt is reported rather than thrown. */
export async function enablePush(vapidPublicKey: string): Promise<PushState> {
  const reg = await registration();
  if (!reg) return 'unsupported';
  const permission = await Notification.requestPermission();
  if (permission !== 'granted') return permission === 'denied' ? 'denied' : 'off';
  const sub = await reg.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: base64UrlToBytes(vapidPublicKey),
  });
  await api('POST', '/push/subscriptions', sub.toJSON());
  return 'on';
}

/** Unsubscribes this browser only. The server forgets it first, so a failure there leaves the
 *  browser subscribed and the switch honest, rather than the other way round. */
export async function disablePush(): Promise<PushState> {
  const reg = await registration();
  const sub = await reg?.pushManager.getSubscription();
  if (sub) {
    await api('DELETE', '/push/subscriptions', { endpoint: sub.endpoint });
    await sub.unsubscribe();
  }
  return 'off';
}
