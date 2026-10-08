<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import { roundOrNull, toCssColor } from '$lib/format';
	import type { ActiveTrayView, AmsUnitView } from '$lib/ipc';

	let {
		units,
		activeTray,
		dense = false
	}: { units: AmsUnitView[]; activeTray: ActiveTrayView | null; dense?: boolean } = $props();

	// Bambu numbers AMS units and tray slots from 0; `unit.id`/tray position
	// are assumed to share that numbering with `ActiveTrayView.unit`/`.slot`.
	// TODO(fixture): confirm against a real multi-AMS capture.
	function isActive(unit: AmsUnitView, unitIndex: number, slotIndex: number): boolean {
		if (!activeTray || activeTray.kind !== 'slot' || activeTray.slot === null) return false;
		if (activeTray.unit !== null && activeTray.unit !== (unit.id ?? unitIndex)) return false;
		return activeTray.slot === slotIndex;
	}

	// Without a reported remainder the canister is drawn full: "we don't know"
	// should not look like "nearly empty".
	function level(remain: number | null): number {
		return remain === null || remain < 0 ? 100 : Math.max(4, Math.min(100, remain));
	}
</script>

<div class="strip" class:dense>
	{#each units as unit, u (unit.id ?? u)}
		<section class="unit" style:flex-grow={Math.max(unit.trays.length, 1)}>
			<header>
				<span class="name"
					>{t('ams-unit-name', { kind: unit.kind ?? 'unknown', number: (unit.id ?? u) + 1 })}</span
				>
				<span class="conditions">
					{#if unit.humidity !== null}{t('ams-humidity', {
							percent: roundOrNull(unit.humidity) ?? 0
						})}{/if}
					{#if unit.humidity !== null && unit.temperature !== null}·{/if}
					{#if unit.temperature !== null}{t('ams-temperature', {
							temp: roundOrNull(unit.temperature) ?? 0
						})}{/if}
				</span>
			</header>
			<ul class="trays">
				{#each unit.trays as tray, i (tray.id ?? i)}
					{@const active = isActive(unit, u, i)}
					<li class="tray" class:active>
						<span class="caret" aria-hidden="true"></span>
						<span class="canister" class:empty={!tray.occupied}>
							{#if tray.occupied}
								<span
									class="fill"
									style:height="{level(tray.remainPercent)}%"
									style:background={toCssColor(tray.color) ?? 'var(--surface-2)'}
								></span>
							{/if}
						</span>
						<span class="tray-label">
							{#if tray.occupied}{tray.filamentType ?? '—'}{:else}{t('ams-tray-empty')}{/if}
						</span>
						<span class="tray-sub">
							{#if active}
								<span class="feeding">{t('ams-active-badge')}</span>
							{/if}
							{#if tray.occupied && tray.remainPercent !== null && tray.remainPercent >= 0}
								<span
									>{t('ams-tray-remaining', {
										percent: roundOrNull(tray.remainPercent) ?? 0
									})}</span
								>
							{/if}
						</span>
					</li>
				{/each}
			</ul>
		</section>
	{/each}
</div>

<style>
	.strip {
		display: flex;
		flex-wrap: wrap;
		gap: 12px 14px;
	}

	.unit {
		flex-basis: 0;
		min-width: 88px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.unit + .unit {
		border-left: 1px solid var(--border);
		padding-left: 14px;
	}

	header {
		display: flex;
		flex-direction: column;
		gap: 1px;
		font-family: var(--font-mono);
		font-size: 9px;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: var(--text-muted);
	}

	.name {
		color: var(--text);
		font-weight: 600;
	}

	.conditions {
		display: flex;
		flex-wrap: wrap;
		column-gap: 0.5em;
		min-height: 1.2em;
	}

	.trays {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		gap: 6px;
	}

	.tray {
		flex: 1 1 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
	}

	.caret {
		width: 0;
		height: 0;
		border-left: 5px solid transparent;
		border-right: 5px solid transparent;
		border-top: 6px solid transparent;
	}

	.tray.active .caret {
		border-top-color: var(--accent);
	}

	.canister {
		width: 100%;
		height: 72px;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--surface);
		display: flex;
		align-items: flex-end;
		overflow: hidden;
	}

	.dense .canister {
		height: 36px;
		border-radius: 4px;
	}

	.canister.empty {
		border-style: dashed;
		border-color: var(--border-strong);
		background: transparent;
	}

	.tray.active .canister {
		border-color: var(--accent);
		box-shadow: 0 0 0 1px var(--accent);
	}

	.fill {
		width: 100%;
		transition: height 0.4s ease;
	}

	.tray-label {
		max-width: 100%;
		font-size: 12px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.canister.empty + .tray-label {
		color: var(--text-faint);
		font-weight: 500;
	}

	.tray-sub {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		font-family: var(--font-mono);
		font-size: 9px;
		color: var(--text-muted);
		min-height: 1.2em;
		white-space: nowrap;
	}

	.dense .tray-label {
		font-size: 11px;
	}

	.feeding {
		color: var(--accent);
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}
</style>
