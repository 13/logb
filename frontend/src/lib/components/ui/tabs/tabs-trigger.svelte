<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.TriggerProps = $props();
</script>

<!-- The ring is drawn inside the tab (-outline-offset-2): the list scrolls, and a scroller clips
     whatever is painted past its edge. -->
<TabsPrimitive.Trigger
	bind:ref
	data-slot="tabs-trigger"
	class={cn(
		'inline-flex min-h-11 flex-1 cursor-pointer items-center justify-center gap-1 whitespace-nowrap border-b-2 border-transparent px-1.5 wide:flex-none wide:gap-1.5 wide:px-3 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 data-[state=active]:border-brand-ink data-[state=active]:font-semibold data-[state=active]:text-brand-ink',
		className
	)}
	{...restProps}
/>
