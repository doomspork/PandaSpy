<script lang="ts">
	import { t } from '$lib/i18nRuntime.svelte';
	import PrinterCard from './PrinterCard.svelte';
	import Icon from './Icon.svelte';
	import type { PrinterView } from '$lib/ipc';

	let {
		printers,
		onReorder,
		onRemove,
		onAddPrinter
	}: {
		printers: PrinterView[];
		onReorder: (serials: string[]) => void;
		onRemove: (serial: string) => void;
		onAddPrinter: () => void;
	} = $props();

	// Four or more printers stop fitting comfortably in a ~400px-wide popover
	// as full cards, so they collapse to name + status + progress (each still
	// individually expandable — see PrinterCard).
	const compact = $derived(printers.length >= 4);

	let dragSerial = $state<string | null>(null);
	let overSerial = $state<string | null>(null);

	function handleDrop(targetSerial: string) {
		const from = dragSerial;
		dragSerial = null;
		overSerial = null;
		if (!from || from === targetSerial) return;

		const order = printers.map((p) => p.serial);
		const fromIndex = order.indexOf(from);
		const toIndex = order.indexOf(targetSerial);
		if (fromIndex === -1 || toIndex === -1) return;

		order.splice(toIndex, 0, order.splice(fromIndex, 1)[0]);
		onReorder(order);
	}
</script>

{#if printers.length === 0}
	<div class="empty">
		<Icon name="spool" size={32} />
		<p class="empty-title">{t('printer-list-empty-title')}</p>
		<p class="muted">{t('printer-list-empty-body')}</p>
		<button class="btn primary" onclick={onAddPrinter}>{t('printer-list-empty-cta')}</button>
	</div>
{:else}
	<ul class="list">
		{#each printers as printer (printer.serial)}
			<li
				draggable="true"
				class:dragging={dragSerial === printer.serial}
				class:drag-over={overSerial === printer.serial && dragSerial !== printer.serial}
				ondragstart={() => (dragSerial = printer.serial)}
				ondragend={() => {
					dragSerial = null;
					overSerial = null;
				}}
				ondragover={(event) => {
					event.preventDefault();
					overSerial = printer.serial;
				}}
				ondrop={(event) => {
					event.preventDefault();
					handleDrop(printer.serial);
				}}
			>
				<PrinterCard {printer} {compact} {onRemove} />
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

	.list {
		list-style: none;
		margin: 0;
		padding: 0.6rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.list li {
		transition:
			opacity 0.15s ease,
			transform 0.15s ease;
	}

	.list li.dragging {
		opacity: 0.4;
	}

	.list li.drag-over {
		transform: translateY(2px);
	}
</style>
