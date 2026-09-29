<script lang="ts">
  import Activity from '@lucide/svelte/icons/activity';
  import CalendarClock from '@lucide/svelte/icons/calendar-clock';
  import CircleDot from '@lucide/svelte/icons/circle-dot';
  import ClipboardCheck from '@lucide/svelte/icons/clipboard-check';
  import Fuel from '@lucide/svelte/icons/fuel';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Hammer from '@lucide/svelte/icons/hammer';
  import Pill from '@lucide/svelte/icons/pill';
  import Route from '@lucide/svelte/icons/route';
  import Scale from '@lucide/svelte/icons/scale';
  import Settings2 from '@lucide/svelte/icons/settings-2';
  import ShoppingCart from '@lucide/svelte/icons/shopping-cart';
  import Stethoscope from '@lucide/svelte/icons/stethoscope';
  import Thermometer from '@lucide/svelte/icons/thermometer';
  import Wrench from '@lucide/svelte/icons/wrench';
  import type { Category } from './types';

  /** An entry's category as a glyph. From lucide (already a dependency via the generated
   *  components) rather than Icon.svelte: these load only with the object page, while Icon.svelte
   *  is in the chunk every start loads. The Record is exhaustive, so a new category is a type
   *  error here until it has an icon. Decorative: the tile around it carries the name. */
  let { category, size = 18 }: { category: Category; size?: number } = $props();

  const ICONS: Record<Category, typeof Wrench> = {
    maintenance: Wrench, repair: Hammer, purchase: ShoppingCart, inspection: ClipboardCheck, modification: Settings2,
    fuel: Fuel, usage: Gauge, other: CircleDot, symptom: Thermometer, treatment: Stethoscope, appointment: CalendarClock,
    medication: Pill, reading: Gauge, trip: Route, weight: Scale, session: Activity,
  };
  const Glyph = $derived(ICONS[category] ?? CircleDot);
</script>

<Glyph {size} strokeWidth={1.75} aria-hidden="true" />
