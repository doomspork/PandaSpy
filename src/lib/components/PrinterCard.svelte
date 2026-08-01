<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import { formatDuration, roundOrNull } from '$lib/format';
	import ProgressBar from './ProgressBar.svelte';
	import ConnectionBadge from './ConnectionBadge.svelte';
	import AmsUnit from './AmsUnit.svelte';
	import HmsList from './HmsList.svelte';
	import Icon from './Icon.svelte';
	import type { PrinterView } from '$lib/ipc';

	let {
		printer,
		compact = false,
		onRemove
	}: {
		printer: PrinterView;
		compact?: boolean;
		onRemove: (serial: string) => void;
	} = $props();

	let expanded = $state(false);
	let confirmingRemove = $state(false);

	const title = $derived(printer.nickname || printer.model || printer.serial);
	const snapshot = $derived(printer.state);
	// In compact mode a card starts collapsed to name + status + progress and
	// can be tapped open; a full card always shows everything.
	const showDetails = $derived(!compact || expanded);
	const remaining = $derived(formatDuration(snapshot?.remainingSecs ?? null));
	const accentStatus = $derived(snapshot?.status ?? 'none');
</script>

<article class="card accent-{accentStatus}" class:compact>
	<header class="head">
		<Icon name="grip" size={14} />
		<div class="head-main">
			<h3 class="title">{title}</h3>
			<ConnectionBadge connection={printer.connection} />
		</div>
		<div class="head-actions">
			{#if compact}
				<button
					class="icon-btn"
					onclick={() => (expanded = !expanded)}
					aria-label={expanded ? t('card-collapse') : t('card-expand')}
				>
					<Icon name={expanded ? 'chevron-up' : 'chevron-down'} size={14} />
				</button>
			{/if}
			<button
				class="icon-btn danger"
				onclick={() => (confirmingRemove = true)}
				aria-label={t('card-remove-label')}
			>
				<Icon name="trash" size={14} />
			</button>
		</div>
	</header>

	{#if confirmingRemove}
		<div class="confirm">
			<p>{t('card-remove-confirm-title', { name: title })}</p>
			<p class="muted">{t('card-remove-confirm-body')}</p>
			<div class="confirm-actions">
				<button class="btn" onclick={() => (confirmingRemove = false)}>{t('common-cancel')}</button>
				<button
					class="btn danger"
					onclick={() => {
						confirmingRemove = false;
						onRemove(printer.serial);
					}}
				>
					{t('card-remove-confirm-confirm')}
				</button>
			</div>
		</div>
	{:else if snapshot}
		<div class="status-row">
			<span class="status-text">{t('print-status', { status: snapshot.status ?? 'unknown' })}</span>
			{#if snapshot.progressPercent !== null}
				<span class="percent">{roundOrNull(snapshot.progressPercent)}%</span>
			{/if}
		</div>

		{#if snapshot.progressPercent !== null}
			<ProgressBar value={snapshot.progressPercent} compact={compact && !expanded} />
		{/if}

		{#if showDetails}
			<div class="details">
				{#if snapshot.taskName}<p class="task">
						{t('card-task-name', { name: snapshot.taskName })}
					</p>{/if}

				<div class="meta-grid">
					{#if remaining}<span>{t('card-remaining', { time: remaining })}</span>{/if}
					{#if snapshot.layer !== null && snapshot.totalLayers !== null}
						<span>{t('card-layer', { layer: snapshot.layer, total: snapshot.totalLayers })}</span>
					{/if}
					{#if snapshot.nozzleTemp !== null}
						<span>
							{t('card-nozzle-temp', {
								hasTarget: snapshot.nozzleTarget !== null ? 'yes' : 'no',
								current: roundOrNull(snapshot.nozzleTemp) ?? 0,
								target: roundOrNull(snapshot.nozzleTarget) ?? 0
							})}
						</span>
					{/if}
					{#if snapshot.bedTemp !== null}
						<span>
							{t('card-bed-temp', {
								hasTarget: snapshot.bedTarget !== null ? 'yes' : 'no',
								current: roundOrNull(snapshot.bedTemp) ?? 0,
								target: roundOrNull(snapshot.bedTarget) ?? 0
							})}
						</span>
					{/if}
					{#if snapshot.chamberTemp !== null}
						<span
							>{t('card-chamber-temp', { current: roundOrNull(snapshot.chamberTemp) ?? 0 })}</span
						>
					{/if}
				</div>

				{#if snapshot.printError}
					<p class="print-error">
						<Icon name="warning" size={14} />
						{t('card-print-error', { message: snapshot.printError })}
					</p>
				{/if}

				{#if snapshot.ams.length > 0}
					<div class="ams-list">
						{#each snapshot.ams as unit, i (unit.id ?? i)}
							<AmsUnit {unit} index={i} activeTray={snapshot.activeTray} />
						{/each}
					</div>
				{/if}

				<HmsList entries={snapshot.hms} />
			</div>
		{/if}
	{/if}
</article>

<style>
	.card {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		background: var(--surface);
		border: 1px solid var(--border);
		border-top-width: 3px;
		border-radius: var(--radius-md);
		padding: 0.65rem 0.75rem;
		box-shadow: var(--shadow-1);
	}

	.card.accent-printing {
		border-top-color: var(--accent);
	}

	.card.accent-paused {
		border-top-color: var(--warn);
	}

	.card.accent-failed {
		border-top-color: var(--danger);
	}

	.head {
		display: flex;
		align-items: flex-start;
		gap: 0.4rem;
	}

	.head > :global(svg) {
		color: var(--text-muted);
		margin-top: 0.35rem;
		cursor: grab;
	}

	.head-main {
		flex: 1 1 auto;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}

	.title {
		font-size: 0.9rem;
		font-weight: 700;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.head-actions {
		display: flex;
		gap: 0.1rem;
		flex-shrink: 0;
	}

	.confirm {
		background: var(--danger-soft);
		border-radius: var(--radius-sm);
		padding: 0.5rem 0.6rem;
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		font-size: 0.8rem;
	}

	.confirm-actions {
		display: flex;
		gap: 0.4rem;
		justify-content: flex-end;
		margin-top: 0.2rem;
	}

	.status-row {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		font-size: 0.8rem;
		font-weight: 600;
	}

	.percent {
		font-family: var(--font-mono);
		color: var(--text-muted);
	}

	.details {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.task {
		font-size: 0.78rem;
		color: var(--text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.meta-grid {
		display: flex;
		flex-wrap: wrap;
		gap: 0.3rem 0.8rem;
		font-family: var(--font-mono);
		font-size: 0.75rem;
		color: var(--text-muted);
	}

	.print-error {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		color: var(--danger);
		background: var(--danger-soft);
		border-radius: var(--radius-sm);
		padding: 0.4rem 0.5rem;
		font-size: 0.78rem;
	}

	.ams-list {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}
</style>
