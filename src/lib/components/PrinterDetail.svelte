<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import { formatDuration, roundOrNull } from '$lib/format';
	import { condition, jobFraction } from '$lib/printerState';
	import TickRuler from './TickRuler.svelte';
	import AmsStrip from './AmsStrip.svelte';
	import HmsList from './HmsList.svelte';
	import type { PrinterView } from '$lib/ipc';

	let { printer, dense = false }: { printer: PrinterView; dense?: boolean } = $props();

	const snap = $derived(printer.state);
	const cond = $derived(condition(printer));
	const remaining = $derived(formatDuration(snap?.remainingSecs ?? null));
	const fraction = $derived(jobFraction(printer));
	const hasLayers = $derived(
		snap !== null && snap.layer !== null && snap.totalLayers !== null && snap.totalLayers > 0
	);

	const readouts = $derived(
		snap
			? [
					{ key: 'nozzle', current: snap.nozzleTemp, target: snap.nozzleTarget },
					{ key: 'bed', current: snap.bedTemp, target: snap.bedTarget },
					{ key: 'chamber', current: snap.chamberTemp, target: null }
				].filter((r) => r.current !== null)
			: []
	);

	// How close a heater is to its target, for the bar under each readout.
	// A target of 0 means the heater is off.
	function heat(current: number | null, target: number | null): number {
		if (current === null || target === null || target <= 0) return 0;
		return Math.max(0, Math.min(1, current / target));
	}
</script>

<div class="detail" class:dense>
	{#if printer.connection.status === 'failed'}
		<p class="problem">
			{t('connection-reason', { reason: printer.connection.reason ?? 'unknown' })}
		</p>
	{:else if !snap}
		<p class="quiet">
			{printer.connection.status === 'connected'
				? t('detail-waiting')
				: t('connection-status', { status: printer.connection.status })}
		</p>
	{:else}
		{#if cond.busy}
			<div class="job">
				<div class="job-top">
					<div class="stack">
						<span class="label">{hasLayers ? t('detail-layer') : t('detail-progress')}</span>
						<span class="big mono">
							{#if hasLayers}
								{snap.layer}<span class="of">/{snap.totalLayers}</span>
							{:else if snap.progressPercent !== null}
								{t('card-progress-percent', { percent: roundOrNull(snap.progressPercent) ?? 0 })}
							{:else}
								—
							{/if}
						</span>
					</div>
					{#if remaining}
						<div class="stack end">
							<span class="label">{t('detail-remaining')}</span>
							<span class="time mono">{remaining}</span>
						</div>
					{/if}
				</div>
				<TickRuler
					{fraction}
					size={dense ? 'sm' : 'lg'}
					label={hasLayers
						? t('card-layer', { layer: snap.layer ?? 0, total: snap.totalLayers ?? 0 })
						: t('detail-progress')}
				/>
				{#if snap.taskName}<p class="task" title={snap.taskName}>{snap.taskName}</p>{/if}
			</div>
		{:else}
			<div class="idle-line">
				<span class="mono">{t('print-status', { status: snap.status ?? 'unknown' })}</span>
				{#if snap.status === 'finished' && snap.taskName}
					<span class="task" title={snap.taskName}>{snap.taskName}</span>
				{/if}
			</div>
		{/if}

		{#if readouts.length > 0}
			<dl class="readouts">
				{#each readouts as r (r.key)}
					<div class="readout">
						<dt class="label">{t('readout', { which: r.key })}</dt>
						<dd class="mono">
							{roundOrNull(r.current)}°{#if r.target !== null && r.target > 0}<span class="of"
									>/{roundOrNull(r.target)}°</span
								>{/if}
						</dd>
						{#if r.key !== 'chamber'}
							<span class="heat" aria-hidden="true"
								><span style:width="{heat(r.current, r.target) * 100}%"></span></span
							>
						{/if}
					</div>
				{/each}
			</dl>
		{/if}

		{#if snap.ams.length > 0}
			<AmsStrip units={snap.ams} activeTray={snap.activeTray} {dense} />
		{/if}

		{#if snap.printError}
			<p class="problem">{t('card-print-error', { message: snap.printError })}</p>
		{/if}

		<HmsList entries={snap.hms} />
	{/if}
</div>

<style>
	.detail {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.detail.dense {
		gap: 12px;
	}

	.job {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		padding: 14px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.dense .job {
		background: transparent;
		border: 0;
		padding: 0;
		gap: 10px;
	}

	.job-top {
		display: flex;
		justify-content: space-between;
		align-items: flex-end;
		gap: 12px;
	}

	.stack {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.stack.end {
		align-items: flex-end;
	}

	.big {
		font-size: 34px;
		font-weight: 300;
		letter-spacing: -0.05em;
		line-height: 1;
	}

	.dense .big {
		font-size: 26px;
	}

	.of {
		font-size: 0.47em;
		letter-spacing: -0.02em;
		color: var(--text-muted);
	}

	.time {
		font-size: 15px;
		font-weight: 600;
	}

	.task {
		font-size: 13px;
		line-height: 1.35;
		color: var(--text-muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.idle-line {
		display: flex;
		align-items: baseline;
		gap: 10px;
		min-width: 0;
	}

	.idle-line .mono {
		font-size: 15px;
		font-weight: 600;
		flex-shrink: 0;
	}

	.readouts {
		margin: 0;
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 16px;
	}

	.readout {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	dd {
		margin: 0;
		font-size: 14px;
		font-weight: 600;
	}

	dd .of {
		font-size: 0.75em;
		font-weight: 300;
	}

	.heat {
		height: 3px;
		border-radius: 2px;
		background: var(--surface-2);
		overflow: hidden;
	}

	.heat > span {
		display: block;
		height: 100%;
		background: var(--heat);
		transition: width 0.4s ease;
	}

	.problem {
		font-size: 13px;
		line-height: 1.4;
		color: var(--danger);
	}

	.quiet {
		font-size: 13px;
		color: var(--text-muted);
	}
</style>
