import { readable } from 'svelte/store';

function currentPath(): string {
  if (typeof location === 'undefined') return '/';
  return location.pathname;
}

export const path = readable<string>(currentPath(), (set) => {
  if (typeof window === 'undefined') return;
  const on = () => set(currentPath());
  window.addEventListener('popstate', on);
  return () => window.removeEventListener('popstate', on);
});

export function go(p: string, replace = false): void {
  if (replace) history.replaceState(null, '', p);
  else history.pushState(null, '', p);
  window.dispatchEvent(new PopStateEvent('popstate'));
  window.scrollTo(0, 0);
}

export function back(fallback = '/'): void {
  if (history.length > 1) history.back();
  else go(fallback, true);
}

/** `/objects/:id` against `/objects/42` → `{ id: '42' }`; null when it does not match. */
export function match(pattern: string, p: string): Record<string, string> | null {
  const norm = (s: string) => (s.length > 1 ? s.replace(/\/+$/, '') : s);
  const pp = norm(pattern).split('/');
  const xs = norm(p).split('/');
  if (pp.length !== xs.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < pp.length; i++) {
    if (pp[i].startsWith(':')) params[pp[i].slice(1)] = decodeURIComponent(xs[i]);
    else if (pp[i] !== xs[i]) return null;
  }
  return params;
}
