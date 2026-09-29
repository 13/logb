<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.ListProps = $props();
</script>

<!-- One row that scrolls sideways rather than wrapping: a wrapped tab row reads as two rows of
     choices, and a badge pushed onto its own line (the audit's red "1" under "Reminders") reads
     as a control of its own. -->
<TabsPrimitive.List
	bind:ref
	data-slot="tabs-list"
	class={cn('flex w-full overflow-x-auto border-b border-border [scrollbar-width:none] [&::-webkit-scrollbar]:hidden', className)}
	{...restProps}
/>
