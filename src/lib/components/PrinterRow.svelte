<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import { formatDuration } from '$lib/format';
	import { condition, displayName, jobFraction } from '$lib/printerState';
	import StatusChip from './StatusChip.svelte';
	import TickRuler from './TickRuler.svelte';
	import Icon from './Icon.svelte';
	import type { PrinterView } from '$lib/ipc';

	let { printer, onOpen }: { printer: PrinterView; onOpen: () => void } = $props();

	const cond = $derived(condition(printer));
	const snap = $derived(printer.state);
	const name = $derived(displayName(printer));
	const remaining = $derived(formatDuration(snap?.remainingSecs ?? null));
	const hasLayers = $derived(snap !== null && snap.layer !== null && snap.totalLayers !== null);

	// One line under the name when the chip alone would not say what to do.
	const note = $derived.by(() => {
		const { status, reason } = printer.connection;
		if (status === 'failed') return t('connection-reason', { reason: reason ?? 'unknown' });
		if (status !== 'connected') return t('connection-status', { status });
		if (cond.chip === 'done' && snap?.taskName) return snap.taskName;
		return null;
	});
</script>

<button
	class="row"
	class:attention={cond.rank === 0}
	class:offline={cond.rank === 3}
	aria-expanded="false"
	aria-label={t('card-expand-named', { name })}
	onclick={onOpen}
>
	<span class="line">
		<StatusChip chip={cond.chip} />
		<span class="name">{name}</span>
		{#if cond.busy}
			{#if hasLayers}<span class="layers mono">{snap?.layer}/{snap?.totalLayers}</span>{/if}
			{#if remaining}<span class="time mono">{remaining}</span>{/if}
		{/if}
		<Icon name="chevron-down" size={14} />
	</span>
	{#if note}<span class="note" class:bad={printer.connection.status === 'failed'}>{note}</span>{/if}
	{#if cond.busy}
		<TickRuler
			fraction={jobFraction(printer)}
			size="sm"
			label={hasLayers
				? t('card-layer', { layer: snap?.layer ?? 0, total: snap?.totalLayers ?? 0 })
				: t('detail-progress')}
		/>
	{/if}
</button>

<style>
	.row {
		width: 100%;
		min-height: 48px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		background: var(--surface);
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
		transition: border-color 0.15s ease;
	}

	.row:hover {
		border-color: var(--border-strong);
	}

	.row.attention {
		border-color: var(--border-strong);
	}

	.row.offline {
		background: transparent;
		border-style: dashed;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
	}

	.line :global(svg) {
		flex-shrink: 0;
		color: var(--text-muted);
	}

	.name {
		flex: 1 1 auto;
		min-width: 0;
		font-size: 14px;
		font-weight: 700;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.offline .name {
		color: var(--text-muted);
	}

	.layers {
		font-size: 11px;
		color: var(--text-muted);
	}

	.time {
		font-size: 13px;
		font-weight: 600;
	}

	.note {
		font-size: 12px;
		color: var(--text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.note.bad {
		color: var(--danger);
	}
</style>
