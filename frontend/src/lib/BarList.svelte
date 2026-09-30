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

  /** `labelClass` sets the name column's width: object names on Statistics need more than the
   *  categories and years Insights lists. */
  let { items, labelClass = 'w-24' }: { items: Bar[]; labelClass?: string } = $props();

  /** Bar width as a share of the largest value, so the widest bar always fills its track. Values
   *  are never negative: the API rejects negative costs at the boundary. */
  const max = $derived(Math.max(...items.map((b) => b.value), 1));

  /** Once any row can expand, every row reserves the toggle's width so names line up. Insights
   *  never sets `expanded`, so its layout has no toggle column. */
  const anyToggle = $derived(items.some((b) => b.expanded !== undefined));
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring';
</script>

<ul role="list" class="m-0 flex list-none flex-col p-0">
  {#each items as b (b.key)}
    <li data-testid="bar-row" class="flex min-h-11 items-center gap-2" style={b.depth ? `padding-left: ${b.depth}rem` : undefined}>
      <span class={`flex min-w-0 shrink-0 items-center gap-1 text-sm ${labelClass}`}>
        {#if b.expanded !== undefined}
          <button type="button" data-slot="bar-toggle" aria-expanded={b.expanded} aria-label={b.toggleLabel} onclick={b.onToggle}
                  class={`grid size-11 shrink-0 cursor-pointer place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground ${focus}`}>
            <span class={['inline-flex transition-transform', b.expanded && 'rotate-90']}><Icon name="chevron" size={14} /></span>
          </button>
        {:else if anyToggle}
          <span class="size-11 shrink-0" aria-hidden="true"></span>
        {/if}
        {#if b.onLabel}
          <button type="button" data-slot="bar-label" onclick={b.onLabel}
                  class={`min-h-11 min-w-0 cursor-pointer truncate text-left text-foreground hover:underline ${focus}`}>{b.label}</button>
        {:else}
          <span class="min-w-0 truncate text-foreground">{b.label}</span>
        {/if}
        {#if b.note}<span class="shrink-0 text-xs text-muted-foreground">{b.note}</span>{/if}
      </span>
      <!-- brand-ink, not the amber fill: a bar is a graphic that carries meaning (1.4.11). -->
      <span class="h-2.5 min-w-8 flex-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
        <span class="block h-full rounded-full bg-brand-ink" style={`width:${Math.round((b.value / max) * 100)}%`}></span>
      </span>
      <span class="shrink-0 text-sm text-foreground tabular-nums">{b.display}</span>
    </li>
  {/each}
</ul>
