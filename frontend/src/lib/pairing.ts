import { readonly, writable, type Readable } from 'svelte/store';
import { api } from './api';
import type { PairCode } from './types';

/**
 * The Account page's "Connect a phone" section: request a code from `POST /auth/pair`, show it
 * as a QR (or, on request, as the plain `uri`), count down to `expires_at`, and fall back to
 * offering a new one once it lapses. Kept free of the DOM -- and of Svelte 5 runes, which need a
 * `.svelte.ts` file -- so the whole state machine can be driven by a mocked clock and a mocked
 * `fetch` in a plain vitest test, the same way `../stores/session.ts` is.
 *
 * A factory rather than one module-level store: unlike the session, there is only ever one
 * "Connect a phone" section on screen, but a fresh instance per mount means a test (or a second
 * `Account.svelte`, in a future world with more than one) never inherits a timer or a code left
 * over from another.
 */

export type PairPhase = 'idle' | 'qr' | 'code' | 'expired';

export interface PairingState {
  phase: PairPhase;
  pair: PairCode | null;
  /** Whole seconds left until `pair.expires_at`, kept in state so the template need not
   *  recompute it every render. Meaningless (0) outside `qr`/`code`. */
  secondsLeft: number;
}

export interface PairingSession {
  state: Readable<PairingState>;
  /** Fetches a fresh code and starts its countdown, replacing whatever was shown before -- the
   *  server itself invalidates the caller's previous, unredeemed code the moment this succeeds
   *  (see `create_pair` in `src/api/pairing.rs`), so a stale one must not go on being displayed.
   *  Rejects (and leaves the state unchanged) on a failed request, so the caller can show the
   *  server's error through whatever error display the page already has -- this module has no
   *  error state of its own for exactly that reason. */
  request: () => Promise<void>;
  /** Reveals the code's `uri` as plain text instead of the QR. A no-op outside `qr`. */
  showCode: () => void;
  /** Switches back to the QR. A no-op outside `code`. */
  showQr: () => void;
  /** Stops the countdown timer without clearing what is on screen. Call from `onDestroy` so a
   *  timer set by one mount never outlives it and keeps firing after the component is gone. */
  stop: () => void;
}

const initial: PairingState = { phase: 'idle', pair: null, secondsLeft: 0 };

export function createPairing(): PairingSession {
  const store = writable<PairingState>({ ...initial });
  let timer: ReturnType<typeof setInterval> | null = null;

  function stop(): void {
    if (timer !== null) { clearInterval(timer); timer = null; }
  }

  /** One tick of the countdown for `expiresAt`. Seconds are rounded UP, so the very first tick
   *  right after the response arrives reads the full TTL rather than one second short of it,
   *  and the display never claims less time is left than the server actually granted. */
  function tick(expiresAt: string): void {
    const secondsLeft = Math.max(0, Math.ceil((new Date(expiresAt).getTime() - Date.now()) / 1000));
    if (secondsLeft <= 0) {
      stop();
      store.set({ ...initial, phase: 'expired' });
      return;
    }
    store.update((s) => ({ ...s, secondsLeft }));
  }

  async function request(): Promise<void> {
    stop();
    const pair = await api<PairCode>('POST', '/auth/pair');
    store.set({ phase: 'qr', pair, secondsLeft: 0 });
    tick(pair.expires_at);
    timer = setInterval(() => tick(pair.expires_at), 1000);
  }

  function showCode(): void {
    store.update((s) => (s.phase === 'qr' ? { ...s, phase: 'code' } : s));
  }

  function showQr(): void {
    store.update((s) => (s.phase === 'code' ? { ...s, phase: 'qr' } : s));
  }

  return { state: readonly(store), request, showCode, showQr, stop };
}
