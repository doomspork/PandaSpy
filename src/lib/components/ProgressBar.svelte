<script lang="ts">
	let { value, compact = false }: { value: number | null; compact?: boolean } = $props();

	const clamped = $derived(value === null ? null : Math.max(0, Math.min(100, value)));
</script>

<div
	class="track"
	class:compact
	role="progressbar"
	aria-valuenow={clamped ?? undefined}
	aria-valuemin={0}
	aria-valuemax={100}
>
	<div class="fill" class:indeterminate={clamped === null} style:width={`${clamped ?? 0}%`}></div>
</div>

<style>
	.track {
		position: relative;
		width: 100%;
		height: 6px;
		border-radius: 999px;
		background: var(--surface-2);
		overflow: hidden;
	}

	.track.compact {
		height: 4px;
	}

	.fill {
		height: 100%;
		border-radius: inherit;
		background: var(--accent);
		transition: width 0.4s ease;
	}

	.fill.indeterminate {
		width: 30% !important;
		animation: slide 1.2s ease-in-out infinite;
	}

	@keyframes slide {
		0% {
			transform: translateX(-120%);
		}
		100% {
			transform: translateX(330%);
		}
	}
</style>
