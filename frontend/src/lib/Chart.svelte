<script lang="ts">
  import { barShare, tickShown, visibleBars, type ChartBar } from './chart';

  /** A small column chart for a series over time, drawn by hand: the app has no chart library
   *  and needs only this. `label` names the series for screen readers, which read the table
   *  under the drawing rather than the drawing. Nothing renders when every bar is empty -- the
   *  caller decides whether its heading stays. */
  let { items, label }: { items: ChartBar[]; label: string } = $props();

  const bars = $derived(visibleBars(items));
  const max = $derived(Math.max(0, ...bars.map((b) => b.value)));
  const top = $derived(bars.find((b) => b.value === max));
  // viewBox units. The SVG is stretched to its box (preserveAspectRatio="none"), which is safe
  // because it holds only rectangles and lines -- every piece of text is HTML around it.
  const W = 100;
  const H = 50;
  // Slot i is the i-th of n equal columns, the same split as the label row's repeat(n, 1fr)
  // below, so a bar sits over its own label. A bar is capped so one or two do not go wide.
  const slot = $derived(W / Math.max(bars.length, 1));
  const bw = $derived(Math.min(slot * 0.64, 12));
</script>

{#if bars.length > 0}
  <figure class="m-0 flex flex-col gap-1">
    <!-- The scale: the tallest bar's value, at the dashed line it reaches. -->
    <p class="m-0 text-xs text-muted-foreground tabular-nums" aria-hidden="true">{top?.display}</p>
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" class="block h-28 w-full" aria-hidden="true" focusable="false">
      <line x1="0" y1="0.5" x2={W} y2="0.5" class="stroke-border" stroke-width="1" stroke-dasharray="2 2" vector-effect="non-scaling-stroke" />
      {#each bars as b, i (b.key)}
        {@const h = Math.max(1, barShare(b.value, max) * (H - 1))}
        <!-- brand-ink, not the amber fill: a bar is a graphic that carries meaning, so it needs
             3:1 against the card (1.4.11); the fill is 2.1:1. -->
        <rect data-testid="chart-bar" x={i * slot + (slot - bw) / 2} width={bw} y={H - h} height={h} class="fill-brand-ink">
          <title>{b.label}: {b.display}</title>
        </rect>
      {/each}
      <line x1="0" y1={H - 0.5} x2={W} y2={H - 0.5} class="stroke-input" stroke-width="1" vector-effect="non-scaling-stroke" />
    </svg>
    <div class="grid text-center text-xs text-muted-foreground" style={`grid-template-columns: repeat(${bars.length}, minmax(0, 1fr))`} aria-hidden="true">
      {#each bars as b, i (b.key)}<span class="min-w-0 whitespace-nowrap">{tickShown(i, bars.length) ? b.tick : ''}</span>{/each}
    </div>
    <table class="sr-only">
      <caption>{label}</caption>
      <tbody>
        {#each bars as b (b.key)}<tr><th scope="row">{b.label}</th><td>{b.display}</td></tr>{/each}
      </tbody>
    </table>
  </figure>
{/if}
