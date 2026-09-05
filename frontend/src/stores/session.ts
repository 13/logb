import { writable } from 'svelte/store';
import { api, setUnauthorizedHandler } from '../lib/api';
import type { Settings, User } from '../lib/types';
import { go } from '../lib/router';

/** undefined = not loaded yet, null = anonymous */
export const user = writable<User | null | undefined>(undefined);
export const setupRequired = writable<boolean>(false);
export const currency = writable<string>('EUR');

setUnauthorizedHandler(() => {
  user.set(null);
  if (location.pathname !== '/login') go('/login', true);
});

export async function loadSession(): Promise<void> {
  const status = await api<{ setup_required: boolean }>('GET', '/auth/status');
  setupRequired.set(status.setup_required);
  if (status.setup_required) { user.set(null); return; }
  try {
    const me = await api<User>('GET', '/auth/me');
    user.set(me);
    const s = await api<Settings>('GET', '/settings');
    currency.set(s.currency);
  } catch {
    user.set(null);
  }
}

export async function login(username: string, password: string): Promise<void> {
  const me = await api<User>('POST', '/auth/login', { username, password });
  user.set(me);
  const s = await api<Settings>('GET', '/settings');
  currency.set(s.currency);
}

export async function logout(): Promise<void> {
  await api('POST', '/auth/logout');
  user.set(null);
  go('/login', true);
}
