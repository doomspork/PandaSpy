<script lang="ts">
	let {
		fraction,
		label,
		size = 'lg'
	}: {
		/** 0–1, or `null` while the printer has not said how far along it is. */
		fraction: number | null;
		/** Accessible name, e.g. "Layer 150 / 467". */
		label: string;
		size?: 'lg' | 'sm';
	} = $props();

	// A ruler rather than a bar: forty ticks, every fifth one taller, and the
	// tick for the layer being printed raised in gold. It reads like the
	// gauge on a machine, and 1/40 steps are fine enough for a glance.
	const TICKS = 40;
	// Rounded up, so layer 1 of 467 already lights the first tick.
	const done = $derived(fraction === null ? 0 : Math.ceil(fraction * TICKS));
	const ticks = $derived(
		Array.from({ length: TICKS }, (_, i) => ({
			current: fraction !== null && i === done - 1,
			filled: i < done,
			major: i % 5 === 4
		}))
	);
</script>

<div
	class="ruler {size}"
	role="progressbar"
	aria-label={label}
	aria-valuemin={0}
	aria-valuemax={100}
	aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
>
	{#each ticks as tick, i (i)}
		<span
			class="tick"
			class:filled={tick.filled}
			class:major={tick.major}
			class:current={tick.current}
		></span>
	{/each}
</div>

<style>
	.ruler {
		display: flex;
		align-items: flex-end;
		gap: 2px;
		height: 16px;
	}

	.ruler.sm {
		height: 8px;
	}

	.tick {
		flex: 1 1 0;
		height: 50%;
		border-radius: 1px;
		background: var(--border-strong);
		opacity: 0.6;
		transition: background-color 0.3s ease;
	}

	.tick.major {
		height: 70%;
	}

	.tick.filled {
		background: var(--text);
		opacity: 1;
	}

	.tick.current {
		height: 100%;
		background: var(--gold);
	}
</style>
