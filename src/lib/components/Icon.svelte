<script lang="ts">
	export type IconName =
		| 'plus'
		| 'gear'
		| 'chevron-left'
		| 'chevron-down'
		| 'chevron-up'
		| 'trash'
		| 'grip'
		| 'close'
		| 'warning'
		| 'spool'
		| 'check'
		| 'refresh';

	let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<!-- One inline sprite sheet instead of an icon package: a dozen glyphs does
     not justify a dependency, and it keeps every icon themeable via
     `currentColor` without a build step. -->
<svg
	width={size}
	height={size}
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width="2"
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
	focusable="false"
>
	{#if name === 'plus'}
		<path d="M12 5v14M5 12h14" />
	{:else if name === 'gear'}
		<circle cx="12" cy="12" r="3" />
		<path
			d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.04 1.56V21a2 2 0 0 1-4 0v-.09A1.7 1.7 0 0 0 9 19.37a1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.04H3a2 2 0 0 1 0-4h.09A1.7 1.7 0 0 0 4.63 9a1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34H9a1.7 1.7 0 0 0 1.04-1.56V3a2 2 0 0 1 4 0v.09A1.7 1.7 0 0 0 15 4.63a1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87V9a1.7 1.7 0 0 0 1.56 1.04H21a2 2 0 0 1 0 4h-.09a1.7 1.7 0 0 0-1.51 1.04Z"
		/>
	{:else if name === 'chevron-left'}
		<path d="M15 18l-6-6 6-6" />
	{:else if name === 'chevron-down'}
		<path d="M6 9l6 6 6-6" />
	{:else if name === 'chevron-up'}
		<path d="M18 15l-6-6-6 6" />
	{:else if name === 'trash'}
		<path
			d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m2 0-.8 13.6a2 2 0 0 1-2 1.9H7.8a2 2 0 0 1-2-1.9L5 6"
		/>
	{:else if name === 'grip'}
		<circle cx="9" cy="6" r="1" /><circle cx="9" cy="12" r="1" /><circle cx="9" cy="18" r="1" />
		<circle cx="15" cy="6" r="1" /><circle cx="15" cy="12" r="1" /><circle cx="15" cy="18" r="1" />
	{:else if name === 'close'}
		<path d="M6 6l12 12M18 6 6 18" />
	{:else if name === 'warning'}
		<path d="M12 3 2 20h20L12 3Z" /><path d="M12 10v4" />
		<circle cx="12" cy="17" r="0.5" fill="currentColor" />
	{:else if name === 'spool'}
		<circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="3" />
	{:else if name === 'check'}
		<path d="M5 12l4 4 10-10" />
	{:else if name === 'refresh'}
		<path d="M21 12a9 9 0 1 1-3-6.7M21 3v5h-5" />
	{/if}
</svg>
