import { mount } from 'svelte';
import { registerSW } from 'virtual:pwa-register';
import './app.css';
import App from './App.svelte';
import { flushOutbox } from './lib/api';

registerSW({ immediate: true });
void flushOutbox();

export default mount(App, { target: document.getElementById('app')! });
