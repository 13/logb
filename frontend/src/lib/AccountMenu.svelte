<script lang="ts">
  import { tick } from 'svelte';
  import SignedIn from './SignedIn.svelte';
  import { go } from './router';
  import { t } from '../i18n';
  import { user } from '../stores/session';

  let open = $state(false);
  let root = $state<HTMLDivElement | null>(null);
  let avatar = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLDivElement | null>(null);

  /** Opening moves focus into the panel, so a keyboard or screen-reader user lands on what just
   *  appeared instead of somewhere behind it. */
  async function show() {
    open = true;
    await tick();
    panel?.querySelector<HTMLButtonElement>('button')?.focus();
  }

  /** `restore` returns focus to the avatar -- after Escape, where the person is still here. A tap
   *  elsewhere already put focus where they wanted it. */
  function hide(restore: boolean) {
    open = false;
    if (restore) avatar?.focus();
  }

  $effect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (root && !root.contains(e.target as Node)) hide(false); };
    const keys = (e: KeyboardEvent) => { if (e.key === 'Escape') hide(true); };
    // Tabbing out of the panel closes it, rather than leaving it open over whatever has focus.
    const focus = (e: FocusEvent) => { if (root && !root.contains(e.target as Node)) hide(false); };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', keys);
    document.addEventListener('focusin', focus);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', keys);
      document.removeEventListener('focusin', focus);
    };
  });
</script>

{#if $user}
  <!-- Zero layout height, so the 44 px button overhangs the bar's centre line instead of making
       the top bar taller. Hidden on desktop, where the sidebar's foot carries all of it. -->
  <div class="relative flex h-0 shrink-0 items-center desk:hidden" bind:this={root}>
    <button data-slot="account-avatar" bind:this={avatar} aria-label={$t('account.menu', { name: $user.username })}
            aria-expanded={open} aria-haspopup="true" onclick={() => (open ? hide(false) : show())}
            class="grid size-11 cursor-pointer place-items-center rounded-full focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
      <span aria-hidden="true" class="grid size-9 place-items-center rounded-full bg-primary text-sm font-bold text-primary-foreground">{$user.username.slice(0, 1).toUpperCase()}</span>
    </button>
    {#if open}
      <div role="group" aria-label={$t('account.menu', { name: $user.username })} bind:this={panel}
           class="absolute top-[calc(22px+0.25rem)] right-0 z-20 flex w-[min(320px,calc(100vw-1.5rem))] flex-col gap-2 rounded-lg border border-border bg-popover p-2 text-popover-foreground shadow-xl">
        <SignedIn framed={false} />
        <button data-slot="account-settings" onclick={() => { hide(false); go('/settings/account'); }}
                class="flex min-h-11 w-full cursor-pointer items-center rounded-md px-3 text-left text-sm font-medium text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">{$t('account.settings')}</button>
      </div>
    {/if}
  </div>
{/if}
