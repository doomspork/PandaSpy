<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import { roundOrNull, toCssColor } from '$lib/format';
	import type { ActiveTrayView, AmsUnitView } from '$lib/ipc';

	let {
		unit,
		index,
		activeTray
	}: { unit: AmsUnitView; index: number; activeTray: ActiveTrayView | null } = $props();

	// Bambu numbers AMS units and tray slots from 0; `unit.id`/tray position
	// are assumed to share that numbering with `ActiveTrayView.unit`/`.slot`.
	// TODO(fixture): confirm against a real multi-AMS capture.
	const unitNumber = $derived((unit.id ?? index) + 1);

	function isActiveSlot(slotIndex: number): boolean {
		if (!activeTray || activeTray.kind !== 'slot' || activeTray.slot === null) return false;
		if (activeTray.unit !== null && activeTray.unit !== (unit.id ?? index)) return false;
		return activeTray.slot === slotIndex;
	}
</script>

<div class="unit">
	<div class="unit-head">
		<span class="unit-title">{t('ams-kind', { kind: unit.kind ?? 'unknown' })} {unitNumber}</span>
		<span class="unit-meta">
			{#if unit.humidity !== null}
				<span>{t('ams-humidity', { percent: roundOrNull(unit.humidity) ?? 0 })}</span>
			{/if}
			{#if unit.temperature !== null}
				<span>{t('ams-temperature', { temp: roundOrNull(unit.temperature) ?? 0 })}</span>
			{/if}
		</span>
	</div>

	<div class="trays">
		{#each unit.trays as tray, i (tray.id ?? i)}
			<div class="tray" class:active={isActiveSlot(i)}>
				<span
					class="swatch"
					class:empty={!tray.occupied}
					style:background={tray.occupied ? (toCssColor(tray.color) ?? 'var(--surface-2)') : ''}
				></span>
				<span class="tray-label">
					{#if tray.occupied}
						{tray.filamentType ?? '—'}
						{#if tray.remainPercent !== null}
							<span class="remain"
								>{t('ams-tray-remaining', { percent: roundOrNull(tray.remainPercent) ?? 0 })}</span
							>
						{/if}
					{:else}
						{t('ams-tray-empty')}
					{/if}
				</span>
				{#if isActiveSlot(i)}<span class="active-badge">{t('ams-active-badge')}</span>{/if}
			</div>
		{/each}
	</div>
</div>

<style>
	.unit {
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 0.5rem 0.6rem;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.unit-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 0.5rem;
	}

	.unit-title {
		font-weight: 600;
		font-size: 0.8rem;
	}

	.unit-meta {
		display: flex;
		gap: 0.6rem;
		font-family: var(--font-mono);
		font-size: 0.72rem;
		color: var(--text-muted);
	}

	.trays {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}

	.tray {
		display: flex;
		align-items: center;
		gap: 0.35rem;
		border: 1px solid var(--border);
		border-radius: 999px;
		padding: 0.2rem 0.5rem 0.2rem 0.2rem;
		font-size: 0.72rem;
		background: var(--surface);
	}

	.tray.active {
		border-color: var(--accent);
		box-shadow: 0 0 0 1px var(--accent-soft);
	}

	.swatch {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		border: 1px solid var(--border);
		flex-shrink: 0;
	}

	.swatch.empty {
		border-style: dashed;
	}

	.tray-label {
		white-space: nowrap;
	}

	.remain {
		color: var(--text-muted);
		margin-left: 0.3em;
	}

	.active-badge {
		color: var(--accent);
		font-weight: 600;
		text-transform: uppercase;
		font-size: 0.62rem;
		letter-spacing: 0.03em;
	}
</style>
