import { mount } from 'svelte';
import { get } from 'svelte/store';
import { registerSW } from 'virtual:pwa-register';
import './app.tw.css';
import App from './App.svelte';
import UpdateBanner from './lib/UpdateBanner.svelte';
import { flushOutbox } from './lib/api';
import { ensureLocale, locale } from './i18n';
import { needReload, offerUpdate, scheduleUpdateChecks } from './lib/sw-update';

// Prompt mode (see ./lib/sw-update.ts): a new build is offered, never reloaded in on its own.
const updateSW = registerSW({
  immediate: true,
  onNeedRefresh: () => offerUpdate(() => updateSW(true)),
  onNeedReload: () => needReload(),
  onRegisteredSW: (_url, registration) => { if (registration) scheduleUpdateChecks(registration); },
});
void flushOutbox();

// Every language is its own chunk (see ./i18n): mounted only once the active one is in hand, so
// the first screen is never drawn in the wrong language or as bare keys. The chunks are precached
// by the service worker, so this is a local read on every start but the very first.
await ensureLocale(get(locale));

mount(UpdateBanner, { target: document.body });
export default mount(App, { target: document.getElementById('app')! });
