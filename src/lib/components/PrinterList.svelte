<script lang="ts">
	import { flip } from 'svelte/animate';
	import { t } from '$lib/i18nRuntime.svelte';
	import { formatDuration } from '$lib/format';
	import { condition, displayName, sortByAttention, summarize } from '$lib/printerState';
	import PrinterDetail from './PrinterDetail.svelte';
	import PrinterRow from './PrinterRow.svelte';
	import StatusChip from './StatusChip.svelte';
	import Icon from './Icon.svelte';
	import type { PrinterView } from '$lib/ipc';

	let {
		printers,
		onAddPrinter
	}: {
		printers: PrinterView[];
		onAddPrinter: () => void;
	} = $props();

	// Whatever needs the user floats to the top. With one printer the whole
	// window is its detail; with more, one is open at a time and the rest are
	// rows. Until the user picks one, the top printer is open — so a new
	// warning both moves its printer up and opens it.
	const sorted = $derived(sortByAttention(printers));
	const summary = $derived(summarize(printers));
	const nextDone = $derived(formatDuration(summary.nextDoneSecs));

	/** `undefined`: follow the top of the list. `null`: the user closed everything. */
	let chosen = $state<string | null | undefined>(undefined);
	const openSerial = $derived(chosen === undefined ? (sorted[0]?.serial ?? null) : chosen);
</script>

{#if printers.length === 0}
	<div class="empty">
		<Icon name="spool" size={32} />
		<p class="empty-title">{t('printer-list-empty-title')}</p>
		<p class="muted">{t('printer-list-empty-body')}</p>
		<button class="btn primary" onclick={onAddPrinter}>{t('printer-list-empty-cta')}</button>
	</div>
{:else if printers.length === 1}
	{@const printer = printers[0]}
	<section class="single">
		<header class="head">
			<h2 class="title">{displayName(printer)}</h2>
			<StatusChip chip={condition(printer).chip} />
		</header>
		<PrinterDetail {printer} />
	</section>
{:else}
	<dl class="summary">
		<div>
			<dt class="label">{t('summary', { field: 'printers' })}</dt>
			<dd class="mono">{summary.total}</dd>
		</div>
		<div>
			<dt class="label">{t('summary', { field: 'printing' })}</dt>
			<dd class="mono" class:run={summary.printing > 0}>{summary.printing}</dd>
		</div>
		<div>
			<dt class="label">{t('summary', { field: 'attention' })}</dt>
			<dd class="mono" class:warn={summary.attention > 0}>{summary.attention}</dd>
		</div>
		<div>
			<dt class="label">{t('summary', { field: 'next-done' })}</dt>
			<dd class="mono">{nextDone ?? '—'}</dd>
		</div>
	</dl>

	<ul class="list">
		{#each sorted as printer (printer.serial)}
			{@const open = printer.serial === openSerial}
			<li animate:flip={{ duration: 200 }}>
				{#if open}
					{@const cond = condition(printer)}
					<section class="open" class:attention={cond.rank === 0}>
						<header class="head">
							<StatusChip chip={cond.chip} />
							<h2 class="title">{displayName(printer)}</h2>
							<button
								class="icon-btn"
								aria-expanded="true"
								aria-label={t('card-collapse-named', { name: displayName(printer) })}
								onclick={() => (chosen = null)}
							>
								<Icon name="chevron-up" size={16} />
							</button>
						</header>
						<PrinterDetail {printer} dense />
					</section>
				{:else}
					<PrinterRow {printer} onOpen={() => (chosen = printer.serial)} />
				{/if}
			</li>
		{/each}
	</ul>
{/if}

<style>
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		text-align: center;
		gap: 0.5rem;
		padding: 3rem 1.5rem;
		color: var(--text-muted);
	}

	.empty :global(svg) {
		opacity: 0.5;
	}

	.empty-title {
		font-weight: 700;
		color: var(--text);
	}

	.single {
		padding: 16px;
		display: flex;
		flex-direction: column;
		gap: 16px;
	}

	.head {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.title {
		flex: 1 1 auto;
		min-width: 0;
		font-size: 19px;
		font-weight: 700;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.summary {
		margin: 0;
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		background: var(--bg);
		border-bottom: 1px solid var(--border);
	}

	.summary > div {
		display: flex;
		flex-direction: column-reverse;
		justify-content: flex-end;
		gap: 3px;
		padding: 8px 0 8px 12px;
		min-width: 0;
	}

	.summary > div:first-child {
		padding-left: 16px;
	}

	.summary > div + div {
		border-left: 1px solid var(--border);
	}

	.summary dd {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
		white-space: nowrap;
	}

	.summary dt {
		font-size: 8px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.summary .run {
		color: var(--accent);
	}

	.summary .warn {
		color: var(--warn);
	}

	.list {
		list-style: none;
		margin: 0;
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.open {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-md);
		padding: 6px 12px 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}

	.open.attention {
		border-color: var(--border-strong);
	}

	.open .title {
		font-size: 16px;
	}

	.open .icon-btn {
		margin-right: -8px;
	}
</style>
