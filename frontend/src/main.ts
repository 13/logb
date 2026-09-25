import { mount } from 'svelte';
import { registerSW } from 'virtual:pwa-register';
import './app.css';
import App from './App.svelte';
import UpdateBanner from './lib/UpdateBanner.svelte';
import { flushOutbox } from './lib/api';
import { needReload, offerUpdate, scheduleUpdateChecks } from './lib/sw-update';

// Prompt mode (see ./lib/sw-update.ts): a new build is offered, never reloaded in on its own.
const updateSW = registerSW({
  immediate: true,
  onNeedRefresh: () => offerUpdate(() => updateSW(true)),
  onNeedReload: () => needReload(),
  onRegisteredSW: (_url, registration) => { if (registration) scheduleUpdateChecks(registration); },
});
void flushOutbox();

mount(UpdateBanner, { target: document.body });
export default mount(App, { target: document.getElementById('app')! });
