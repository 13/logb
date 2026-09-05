import { mount } from 'svelte';
import { registerSW } from 'virtual:pwa-register';
import './app.css';
import App from './App.svelte';

registerSW({ immediate: true });

export default mount(App, { target: document.getElementById('app')! });
