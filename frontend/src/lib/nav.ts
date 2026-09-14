import type { IconName } from './Icon.svelte';

export type Destination = 'objects' | 'search' | 'stats' | 'settings';

export type NavDestination = { id: Destination; path: string; icon: IconName; label: string };

/** The whole of LogB's top-level navigation. Four is few enough that all of them are always
 *  visible: no drawer, no overflow menu, no hamburger hiding two items behind a tap. */
export const DESTINATIONS: NavDestination[] = [
  { id: 'objects', path: '/', icon: 'object', label: 'nav.objects' },
  { id: 'search', path: '/search', icon: 'search', label: 'search.title' },
  { id: 'stats', path: '/stats', icon: 'chart', label: 'nav.stats' },
  { id: 'settings', path: '/settings', icon: 'settings', label: 'nav.settings' },
];

/** True when `p` is `prefix` itself or something below it -- `/settings` and
 *  `/settings/appearance`, but not `/settingsx`. The boundary is a path separator, because a
 *  bare `startsWith` would light up a destination for any route that happened to share its
 *  opening letters. */
function within(p: string, prefix: string): boolean {
  return p === prefix || p.startsWith(`${prefix}/`);
}

/** Which destination the current path belongs to, so a drill-down three screens deep still
 *  shows where it sits. Null on the routes that render without a shell at all (`/login`,
 *  `/setup`) and on anything unrecognised, where marking a destination would be a guess. */
export function activeDestination(p: string): Destination | null {
  if (p === '/' || within(p, '/objects')) return 'objects';
  if (within(p, '/search')) return 'search';
  if (within(p, '/stats')) return 'stats';
  if (within(p, '/settings')) return 'settings';
  return null;
}
