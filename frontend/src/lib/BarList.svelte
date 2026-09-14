<script lang="ts" module>
  export interface Bar {
    key: string | number;
    label: string;
    value: number;
    display: string;
    /** Tree depth; 0 or absent is a root. */
    depth?: number;
    /** Muted text after the label, such as "archived". */
    note?: string;
    /** Makes the label a button. */
    onLabel?: () => void;
    /** Present only on a row that can expand; `true` while open. */
    expanded?: boolean;
    onToggle?: () => void;
    toggleLabel?: string;
  }
</script>

<script lang="ts">
  import Icon from './Icon.svelte';

  let { items }: { items: Bar[] } = $props();

  /** Bar width as a percentage of the largest value, so the widest bar always fills its track.
   *  Values are never negative: the API rejects negative costs at the boundary. */
  const max = $derived(Math.max(...items.map((b) => b.value), 1));
</script>

{#each items as b (b.key)}
  <div class="bar-row" style={b.depth ? `padding-left: calc(${b.depth} * var(--space-4))` : undefined}>
    <span class="label">
      {#if b.expanded !== undefined}
        <button type="button" class="toggle" class:open={b.expanded} aria-expanded={b.expanded} aria-label={b.toggleLabel} onclick={b.onToggle}>
          <Icon name="chevron" size={14} />
        </button>
      {/if}
      {#if b.onLabel}
        <button type="button" class="link" onclick={b.onLabel}>{b.label}</button>
      {:else}
        {b.label}
      {/if}
      {#if b.note}<span class="muted note">{b.note}</span>{/if}
    </span>
    <span class="track"><span class="fill" style={`width:${Math.round((b.value / max) * 100)}%`}></span></span>
    <span class="value tnum">{b.display}</span>
  </div>
{/each}

<style>
  .bar-row { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); }
  .label { flex: none; width: 90px; font-size: var(--text-sm); display: flex; align-items: center; gap: var(--space-1); min-width: 0; }
  /* Half the track's height, spelled as a literal, is a pill -- and a pill is a token. */
  .track { flex: 1; height: 10px; background: var(--surface-2); border-radius: var(--radius-full); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .value { flex: none; font-size: var(--text-sm); }
  .note { font-size: var(--text-xs, var(--text-sm)); }
  .toggle, .link { background: none; border: 0; padding: 0; color: inherit; font: inherit; cursor: pointer; }
  .link { text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* The global `button { min-height: var(--control); }` makes this a phone-sized tap target, but
     `inline-flex` alone stretches to fill it top-to-bottom, pinning the chevron above the
     baseline instead of centring it against the label text beside it. */
  .toggle { display: inline-flex; align-items: center; justify-content: center; transition: transform 120ms; }
  .toggle.open { transform: rotate(90deg); }
</style>
